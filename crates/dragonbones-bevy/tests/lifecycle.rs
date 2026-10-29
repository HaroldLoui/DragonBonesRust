//! Integration tests for the Bevy adapter's ECS-facing behaviour: asset
//! loading through [`DragonBonesPlugin`], slot spawning, transform sync, and the
//! despawn → dispose lifecycle that reclaims core arenas and GPU assets.
//!
//! The tests run a real (headless) [`App`] with `MinimalPlugins` + `AssetPlugin`,
//! so all schedule wiring and asset bookkeeping is exercised, but nothing is
//! rendered.

use std::sync::Arc;

use bevy::app::PostUpdate;
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesMaterial, DragonBonesPlugin,
    DragonBonesRuntime, DragonBonesSyncConfig, SkeletonAsset,
};
use dragonbones_core::armature::ArmatureIdx;
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;
use std::path::PathBuf;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn test_app_with_sync_config(config: DragonBonesSyncConfig) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        // `DragonBonesPlugin` registers the custom material (and its asset) for
        // us; only the mesh asset type needs an explicit `init_asset`.
        .init_asset::<Mesh>();
    // Inserted before the plugin: `DragonBonesPlugin` uses `init_resource`, which
    // keeps an existing value.
    app.insert_resource(config);
    app.add_plugins(DragonBonesPlugin);
    app
}

fn test_app() -> App {
    test_app_with_sync_config(DragonBonesSyncConfig::default())
}

/// Parses the Dragon rig with the core parser and stores it as adapter assets.
fn install_dragon_assets(app: &mut App) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon_ske.json")).expect("read Dragon_ske.json"),
    )
    .expect("parse Dragon_ske.json");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon_tex.json")).expect("read Dragon_tex.json"),
    )
    .expect("parse Dragon_tex.json");

    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("core parses Dragon skeleton");
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
            // Rendering is not exercised here, so the atlas page image does not
            // need to exist as an asset.
            texture: Handle::default(),
        });
    (skeleton, atlas)
}

fn spawn_armature(
    app: &mut App,
    skeleton: &Handle<SkeletonAsset>,
    atlas: &Handle<AtlasAsset>,
) -> Entity {
    app.world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton: skeleton.clone(),
                atlas: atlas.clone(),
                armature_name: "Dragon".to_owned(),
                animation: Some("stand".to_owned()),
            },
            // DragonBones data is y-down; the adapter expects the caller to flip
            // once at the armature root.
            Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
        ))
        .id()
}

fn instance(app: &App, root: Entity) -> (ArmatureIdx, Vec<Entity>) {
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter builds a DragonBonesInstance");
    (instance.armature, instance.slots.clone())
}

fn armature_is_live(app: &App, armature: ArmatureIdx) -> bool {
    app.world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource")
        .factory
        .armatures[armature.index()]
    .is_some()
}

#[test]
fn building_an_armature_shares_one_material_and_syncs_slots() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);

    app.update();

    let (armature, slots) = instance(&app, root);
    assert!(armature_is_live(&app, armature));
    assert!(slots.len() > 10, "Dragon rig spawns many slots");
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        1,
        "all slots of one atlas page share a single DragonBonesMaterial"
    );
    assert!(
        app.world().resource::<Assets<Mesh>>().len() >= slots.len(),
        "every slot owns a mesh asset"
    );

    // The sync system ran in PostUpdate: per-slot z follows the core draw order.
    for (order, slot) in slots.iter().enumerate() {
        let global = app.world().get::<GlobalTransform>(*slot).expect("slot GT");
        let expected_z = order as f32 * 0.001;
        assert!(
            (global.translation().z - expected_z).abs() < 1e-6,
            "slot {order} z is {} but the adapter should write {expected_z}",
            global.translation().z
        );
    }

    // Per-slot tint travels through vertex colors, not through the material.
    let mesh_handle = app
        .world()
        .get::<Mesh2d>(slots[0])
        .expect("slot mesh")
        .0
        .clone();
    let mesh = app
        .world()
        .resource::<Assets<Mesh>>()
        .get(&mesh_handle)
        .expect("mesh asset");
    assert!(
        mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some(),
        "slot meshes must carry a vertex color attribute for per-slot tinting"
    );
}

#[test]
fn despawning_the_root_releases_core_and_gpu_state() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);

    app.update();
    let (armature, slots) = instance(&app, root);
    let mesh_count = app.world().resource::<Assets<Mesh>>().len();
    assert!(mesh_count >= slots.len());

    app.world_mut().entity_mut(root).despawn();
    app.update();

    assert!(app.world().get_entity(root).is_err());
    assert!(
        !armature_is_live(&app, armature),
        "core dispose must be flushed by the advance step of the same frame"
    );
    assert_eq!(
        app.world().resource::<Assets<Mesh>>().len(),
        0,
        "all slot meshes are removed with the armature"
    );
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        0,
        "the shared atlas material is removed with the last armature"
    );
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    assert!(
        runtime.factory.data_map.is_empty(),
        "skeleton data registration is released with the last armature"
    );
    assert!(
        runtime.factory.atlas_map.is_empty(),
        "atlas registration is released with the last armature"
    );
}

#[test]
fn two_armatures_share_material_and_data_until_the_last_one_is_dropped() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let first = spawn_armature(&mut app, &skeleton, &atlas);
    let second = spawn_armature(&mut app, &skeleton, &atlas);

    app.update();

    let (first_armature, _) = instance(&app, first);
    let (second_armature, _) = instance(&app, second);
    assert_ne!(first_armature, second_armature);
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        1,
        "both armatures of the same atlas share one material"
    );

    app.world_mut().entity_mut(first).despawn();
    app.update();

    assert!(!armature_is_live(&app, first_armature));
    assert!(armature_is_live(&app, second_armature));
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        1,
        "the second armature keeps the shared material alive"
    );
    assert!(
        !app.world()
            .get_non_send::<DragonBonesRuntime>()
            .expect("runtime resource")
            .factory
            .data_map
            .is_empty(),
        "shared skeleton registration survives while one armature is alive"
    );

    app.world_mut().entity_mut(second).despawn();
    app.update();

    assert!(!armature_is_live(&app, second_armature));
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        0
    );
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    assert!(runtime.factory.data_map.is_empty());
    assert!(runtime.factory.atlas_map.is_empty());
}

/// Tampering with a slot's `GlobalTransform` is the observable probe for the
/// armature-level dirty check: a clean armature must not rewrite it, a dirty one
/// must.
const TAMPERED_X: f32 = 12_345.0;

fn tampered_slot(app: &mut App, slots: &[Entity]) -> Entity {
    let slot = slots[1];
    *app.world_mut()
        .get_mut::<GlobalTransform>(slot)
        .expect("slot GlobalTransform") =
        GlobalTransform::from_translation(Vec3::new(TAMPERED_X, 0.0, 0.0));
    slot
}

fn slot_x(app: &App, slot: Entity) -> f32 {
    app.world()
        .get::<GlobalTransform>(slot)
        .expect("slot GlobalTransform")
        .translation()
        .x
}

#[test]
fn idle_armature_skips_slot_writes_until_the_pose_changes() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();
    let (_, slots) = instance(&app, root);

    // No animation advance between these two syncs, so the pose is unchanged.
    let slot = tampered_slot(&mut app, &slots);
    app.world_mut().run_schedule(PostUpdate);
    assert_eq!(
        slot_x(&app, slot),
        TAMPERED_X,
        "a clean armature must not rewrite slot transforms"
    );

    // Advancing the animation changes the pose, so the armature becomes dirty and
    // the adapter writes the slot transforms again.
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        runtime.factory.advance_time(1.0 / 60.0);
    }
    app.world_mut().run_schedule(PostUpdate);
    assert_ne!(
        slot_x(&app, slot),
        TAMPERED_X,
        "an animated armature must be re-synced"
    );
}

#[test]
fn skip_clean_armatures_can_be_disabled() {
    let mut app = test_app_with_sync_config(DragonBonesSyncConfig {
        skip_clean_armatures: false,
    });
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();
    let (_, slots) = instance(&app, root);

    let slot = tampered_slot(&mut app, &slots);
    app.world_mut().run_schedule(PostUpdate);
    assert_ne!(
        slot_x(&app, slot),
        TAMPERED_X,
        "with the dirty check disabled every slot is written every sync"
    );
}
