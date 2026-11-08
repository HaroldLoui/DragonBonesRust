//! Schedule-integration tests for the B2 design.
//!
//! The adapter writes each slot's world matrix **directly** into Bevy's
//! `GlobalTransform` instead of going through `Transform`. That only works if
//!
//! 1. the write happens after `TransformSystems::Propagate` — otherwise Bevy's
//!    propagation overwrites the slot's affine with the one derived from the
//!    placeholder `Transform` (usually identity), and
//! 2. the written values are visible to systems ordered after
//!    [`DragonBonesSystems::SyncTransforms`] in `PostUpdate`.
//!
//! These tests install the real `TransformPlugin`, so propagation actually runs,
//! and assert the resulting world matrices. They are the regression guard for
//! the version-sensitive `GlobalTransform` direct-write assumption described in
//! `docs/optimization-roadmap.md` §1.6.
//!
//! The tests run headless (no renderer, no visibility plugins): the schedule
//! ordering is what is under test, not rasterization.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesPlugin, DragonBonesRuntime,
    DragonBonesSlot, DragonBonesSlotVisual, DragonBonesSystems, SkeletonAsset, SlotRenderKind,
    matrix_to_affine3a, slot_z,
};
use dragonbones_core::armature::{Armature, ArmatureIdx};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        // Real transform propagation: this is what the direct write has to run
        // after.
        .add_plugins(TransformPlugin)
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);
    app
}

/// Parses a rig with the core parser and stores it as adapter assets.
fn install_assets(
    app: &mut App,
    skeleton_file: &str,
    atlas_file: &str,
) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(skeleton_file)).expect("read skeleton"),
    )
    .expect("parse skeleton");
    let atlas_json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(atlas_file)).expect("read atlas"))
            .expect("parse atlas");

    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("core parses skeleton");
    let mut atlas_data = TextureAtlasData::default();
    assert!(parser.parse_texture_atlas_data(&atlas_json, &mut atlas_data, 1.0));

    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(data),
        });
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas_data,
            texture: Handle::default(),
        });
    (skeleton, atlas)
}

fn spawn_armature(
    app: &mut App,
    skeleton: &Handle<SkeletonAsset>,
    atlas: &Handle<AtlasAsset>,
    armature_name: &str,
    transform: Transform,
) -> Entity {
    app.world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton: skeleton.clone(),
                atlas: atlas.clone(),
                armature_name: armature_name.to_owned(),
                animation: None,
            },
            transform,
        ))
        .id()
}

fn core_armature(app: &App, armature: ArmatureIdx) -> &Armature {
    app.world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource")
        .factory
        .armatures[armature.index()]
    .as_ref()
    .expect("armature built")
}

/// World matrix the adapter is expected to write for one slot entity.
fn expected_world(app: &App, root: Entity, slot_entity: Entity) -> Affine3A {
    let slot_ref = *app
        .world()
        .get::<DragonBonesSlot>(slot_entity)
        .expect("slot marker");
    let visual = app
        .world()
        .get::<DragonBonesSlotVisual>(slot_entity)
        .expect("slot visual");
    let armature = core_armature(app, slot_ref.armature);
    let order = armature
        .slots
        .iter()
        .position(|slot| slot.idx == slot_ref.slot)
        .expect("slot in armature");
    let slot = &armature.slots[order];
    let root_affine = app
        .world()
        .get::<GlobalTransform>(root)
        .expect("root GlobalTransform")
        .affine();

    let mut world = match visual.kind {
        SlotRenderKind::Local => {
            root_affine * matrix_to_affine3a(&slot.base.global_transform_matrix, 0.0)
        }
        SlotRenderKind::ArmatureSpace | SlotRenderKind::Empty => root_affine,
    };
    world.translation.z = root_affine.translation.z + slot_z(order);
    world
}

fn assert_affine_close(got: Affine3A, want: Affine3A, what: &str) {
    let got = Mat4::from(got).to_cols_array();
    let want = Mat4::from(want).to_cols_array();
    for (index, (got, want)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            (got - want).abs() <= 1e-4,
            "{what}: column-major element {index} differs, got {got}, want {want}"
        );
    }
}

fn instance(app: &App, root: Entity) -> (ArmatureIdx, Vec<Entity>) {
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter builds a DragonBonesInstance");
    (instance.armature, instance.slots.clone())
}

#[test]
fn root_motion_keeps_slot_local_offsets_after_propagation() {
    let mut app = test_app();
    let (skeleton, atlas) =
        install_assets(&mut app, "Dragon/Dragon_ske.json", "Dragon/Dragon_tex.json");
    let root = spawn_armature(
        &mut app,
        &skeleton,
        &atlas,
        "Dragon",
        Transform::from_scale(Vec3::new(0.75, -0.75, 1.0))
            .with_translation(Vec3::new(3.0, -4.0, 0.0)),
    );
    app.update();

    // Use a slot that really applies its own local matrix, so a propagation
    // overwrite (which would collapse it to the root affine) is observable.
    let (armature, slots) = instance(&app, root);
    let slot_entity = slots
        .iter()
        .copied()
        .find(|entity| {
            let Some(visual) = app.world().get::<DragonBonesSlotVisual>(*entity) else {
                return false;
            };
            if visual.kind != SlotRenderKind::Local {
                return false;
            }
            let Some(slot_ref) = app.world().get::<DragonBonesSlot>(*entity) else {
                return false;
            };
            core_armature(&app, armature)
                .slots
                .iter()
                .find(|slot| slot.idx == slot_ref.slot)
                .is_some_and(|slot| {
                    let matrix = slot.base.global_transform_matrix;
                    matrix.tx.abs() > 1.0 || matrix.ty.abs() > 1.0
                })
        })
        .expect("Dragon has a local-image slot with an offset");

    let slot_world = |app: &App| {
        app.world()
            .get::<GlobalTransform>(slot_entity)
            .expect("slot GlobalTransform")
            .affine()
    };

    assert_affine_close(
        slot_world(&app),
        expected_world(&app, root, slot_entity),
        "initial sync",
    );

    // Move the armature root: propagation rewrites the child GlobalTransforms
    // from their placeholder `Transform`s before the adapter re-syncs them.
    app.world_mut()
        .get_mut::<Transform>(root)
        .expect("root Transform")
        .translation = Vec3::new(13.0, 1.0, 0.0);
    app.update();

    let root_gt = app
        .world()
        .get::<GlobalTransform>(root)
        .expect("root GlobalTransform")
        .affine();
    assert!(
        (root_gt.translation.x - 13.0).abs() < 1e-5 && (root_gt.translation.y - 1.0).abs() < 1e-5,
        "TransformPlugin must have propagated the moved root, got {root_gt:?}"
    );
    assert_affine_close(
        slot_world(&app),
        expected_world(&app, root, slot_entity),
        "sync after root motion",
    );

    // The slot affine must not have collapsed to the root affine (that is what a
    // sync-before-propagate regression would produce).
    let slot_translation = slot_world(&app).translation;
    assert!(
        (slot_translation.x - root_gt.translation.x).abs() > 1.0
            || (slot_translation.y - root_gt.translation.y).abs() > 1.0,
        "slot local offset was lost: slot {slot_translation:?}, root {:?}",
        root_gt.translation
    );
}

#[test]
fn skinned_slots_follow_the_root_affine_only() {
    let mut app = test_app();
    let (skeleton, atlas) = install_assets(
        &mut app,
        "Rooster_Ani/Rooster_Ani_ske.json",
        "Rooster_Ani/Rooster_Ani_tex.json",
    );
    let root = spawn_armature(
        &mut app,
        &skeleton,
        &atlas,
        "armatureName",
        Transform::from_scale(Vec3::new(0.9, -0.9, 1.0)),
    );
    app.update();

    let (_armature, slots) = instance(&app, root);
    let skinned = slots
        .iter()
        .copied()
        .find(|entity| {
            app.world()
                .get::<DragonBonesSlotVisual>(*entity)
                .is_some_and(|visual| visual.kind == SlotRenderKind::ArmatureSpace)
        })
        .expect("Rooster_Ani has a skinned mesh slot");

    // Skinned vertices are rebuilt in armature space, so the slot transform is
    // just the root affine (plus the draw-order z offset).
    assert_affine_close(
        app.world()
            .get::<GlobalTransform>(skinned)
            .expect("slot GlobalTransform")
            .affine(),
        expected_world(&app, root, skinned),
        "skinned slot world matrix",
    );
}

#[derive(Resource, Default)]
struct Probe {
    snapshot: Option<(Entity, Affine3A)>,
}

fn probe_slot_globals(
    slots: Query<(Entity, &GlobalTransform), With<DragonBonesSlot>>,
    mut probe: ResMut<Probe>,
) {
    probe.snapshot = slots
        .iter()
        .next()
        .map(|(entity, global)| (entity, global.affine()));
}

#[test]
fn sync_writes_are_visible_to_systems_ordered_after_it() {
    let mut app = test_app();
    app.init_resource::<Probe>();
    app.add_systems(
        PostUpdate,
        probe_slot_globals.after(DragonBonesSystems::SyncTransforms),
    );

    let (skeleton, atlas) =
        install_assets(&mut app, "Dragon/Dragon_ske.json", "Dragon/Dragon_tex.json");
    let root = spawn_armature(
        &mut app,
        &skeleton,
        &atlas,
        "Dragon",
        Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
    );
    app.update();

    let (entity, observed) = app
        .world()
        .resource::<Probe>()
        .snapshot
        .expect("probe ran after sync");
    // A system ordered after SyncTransforms must see the adapter-written affine,
    // not the placeholder derived from the slot's identity Transform.
    assert_affine_close(
        observed,
        expected_world(&app, root, entity),
        "probe after sync",
    );
}
