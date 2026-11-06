//! Integration tests for DragonBones blend-mode material selection.
//!
//! The adapter renders every slot with a shared custom
//! [`DragonBonesMaterial`], keyed by `(atlas page image, blend mode)`. These
//! tests exercise that on the `Dragon_Blend` sample rig, which the reference
//! files author with `add ×2`, `multiply ×1`, `overlay ×1` and `screen ×1`
//! slots, plus the runtime path where a core slot's blend mode changes after the
//! armature was spawned.

use std::sync::Arc;

use bevy::app::PostUpdate;
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesBlend, DragonBonesInstance, DragonBonesMaterial,
    DragonBonesPlugin, DragonBonesRuntime, DragonBonesSlot, DragonBonesSlotVisual, SkeletonAsset,
};
use dragonbones_core::armature::{Armature, ArmatureIdx, SlotIdx};
use dragonbones_core::core::enums::BlendMode;
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;
use std::path::PathBuf;

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
        // The custom material and its asset type are registered by
        // `DragonBonesPlugin` (`Material2dPlugin`).
        .init_asset::<Mesh>();
    app.add_plugins(DragonBonesPlugin);
    app
}

/// Parses one skeleton/atlas pair with the core parser and stores the adapter
/// assets, mirroring how the asset loaders install them in the demo.
fn install_assets(
    app: &mut App,
    skeleton_file: &str,
    atlas_file: &str,
) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(skeleton_file)).expect("read skeleton json"),
    )
    .expect("parse skeleton json");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(atlas_file)).expect("read atlas json"),
    )
    .expect("parse atlas json");

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
            // Rendering is not exercised, so the page image does not need to
            // exist as an asset.
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

fn core_armature(app: &App, armature: ArmatureIdx) -> &Armature {
    app.world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource")
        .factory
        .armatures[armature.index()]
    .as_ref()
    .expect("live armature")
}

fn slot_material(app: &App, entity: Entity) -> (Handle<DragonBonesMaterial>, DragonBonesBlend) {
    let handle = app
        .world()
        .get::<MeshMaterial2d<DragonBonesMaterial>>(entity)
        .expect("slot material")
        .0
        .clone();
    let blend = app
        .world()
        .get::<DragonBonesSlotVisual>(entity)
        .expect("slot visual")
        .blend;
    (handle, blend)
}

fn material_blend(app: &App, handle: &Handle<DragonBonesMaterial>) -> DragonBonesBlend {
    app.world()
        .resource::<Assets<DragonBonesMaterial>>()
        .get(handle)
        .expect("material asset")
        .blend
}

fn slot_entity_for(app: &App, slots: &[Entity], slot_idx: SlotIdx) -> Option<Entity> {
    slots.iter().copied().find(|entity| {
        app.world()
            .get::<DragonBonesSlot>(*entity)
            .is_some_and(|slot| slot.slot == slot_idx)
    })
}

#[test]
fn dragon_blend_rig_shares_one_material_per_image_and_blend() {
    let mut app = test_app();
    let (skeleton, atlas) = install_assets(
        &mut app,
        "Dragon_Blend/Dragon_Blend_ske.json",
        "Dragon_Blend/Dragon_Blend_tex.json",
    );
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();

    let (_armature, slots) = instance(&app, root);

    // The rig authors add×2 / multiply×1 / overlay×1 / screen×1; overlay has no
    // fixed-function equivalent and falls back to Normal.
    let mut add = 0;
    let mut multiply = 0;
    let mut screen = 0;
    let mut normal = 0;
    let mut materials = std::collections::HashMap::new();
    for (order, entity) in slots.iter().enumerate() {
        let (handle, blend) = slot_material(&app, *entity);
        match blend {
            DragonBonesBlend::Add => add += 1,
            DragonBonesBlend::Multiply => multiply += 1,
            DragonBonesBlend::Screen => screen += 1,
            DragonBonesBlend::Normal => normal += 1,
        }
        assert_eq!(
            material_blend(&app, &handle),
            blend,
            "slot {order} visual blend and material blend must agree"
        );
        materials.insert(handle.id(), blend);
    }
    assert_eq!(add, 2, "Dragon_Blend authors two additive slots");
    assert_eq!(multiply, 1, "Dragon_Blend authors one multiply slot");
    assert_eq!(screen, 1, "Dragon_Blend authors one screen slot");
    assert!(normal > 0, "the remaining slots use normal blending");

    // One atlas page x four distinct blend modes = four shared materials.
    assert_eq!(
        materials.len(),
        4,
        "one shared material per (atlas image, blend) pair"
    );
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        4
    );

    // All four sample the same atlas page.
    let atlas_texture = app
        .world()
        .resource::<Assets<AtlasAsset>>()
        .get(&atlas)
        .expect("atlas asset")
        .texture
        .id();
    for handle in app
        .world()
        .resource::<Assets<DragonBonesMaterial>>()
        .iter()
        .map(|(_, material)| material.texture.id())
    {
        assert_eq!(handle, atlas_texture, "materials sample the atlas page");
    }
}

#[test]
fn unsupported_overlay_slot_renders_with_its_normal_material() {
    let mut app = test_app();
    let (skeleton, atlas) = install_assets(
        &mut app,
        "Dragon_Blend/Dragon_Blend_ske.json",
        "Dragon_Blend/Dragon_Blend_tex.json",
    );
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();

    let (armature, slots) = instance(&app, root);
    let overlay = core_armature(&app, armature)
        .slots
        .iter()
        .find(|slot| slot.blend_mode == BlendMode::Overlay)
        .expect("Dragon_Blend has an overlay slot")
        .idx;
    let overlay_entity = slot_entity_for(&app, &slots, overlay).expect("overlay slot entity");
    let (overlay_handle, overlay_blend) = slot_material(&app, overlay_entity);
    assert_eq!(
        overlay_blend,
        DragonBonesBlend::Normal,
        "overlay falls back to normal until a shader-based blend exists"
    );

    let normal_entity = slots
        .iter()
        .copied()
        .find(|entity| {
            let idx = app
                .world()
                .get::<DragonBonesSlot>(*entity)
                .expect("slot")
                .slot;
            core_armature(&app, armature)
                .slots
                .iter()
                .find(|slot| slot.idx == idx)
                .is_some_and(|slot| slot.blend_mode == BlendMode::Normal)
        })
        .expect("Dragon_Blend has a normal slot");
    let (normal_handle, _) = slot_material(&app, normal_entity);
    assert_eq!(
        overlay_handle, normal_handle,
        "the overlay slot shares the normal-blend material"
    );
}

#[test]
fn runtime_blend_mode_change_swaps_material_and_cleanup_releases_all() {
    // The plain Dragon rig uses normal blending only, so it starts with exactly
    // one shared material.
    let mut app = test_app();
    let (skeleton, atlas) =
        install_assets(&mut app, "Dragon/Dragon_ske.json", "Dragon/Dragon_tex.json");
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();

    let (armature, slots) = instance(&app, root);
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        1
    );
    let (normal_handle, normal_blend) = slot_material(&app, slots[1]);
    assert_eq!(normal_blend, DragonBonesBlend::Normal);

    // Flip one core slot to additive blending and sync: the adapter must swap
    // the slot's material handle to the additive variant.
    let changed_idx = app
        .world()
        .get::<DragonBonesSlot>(slots[1])
        .expect("slot")
        .slot;
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        let armature = runtime.factory.armatures[armature.index()]
            .as_mut()
            .expect("live armature");
        let slot = armature
            .slots
            .iter_mut()
            .find(|slot| slot.idx == changed_idx)
            .expect("core slot");
        slot.blend_mode = BlendMode::Add;
    }
    app.world_mut().run_schedule(PostUpdate);

    let (add_handle, add_blend) = slot_material(&app, slots[1]);
    assert_eq!(add_blend, DragonBonesBlend::Add, "visual follows the swap");
    assert_eq!(material_blend(&app, &add_handle), DragonBonesBlend::Add);
    assert_ne!(
        add_handle, normal_handle,
        "the slot must now use a different material"
    );
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        2,
        "the armature now holds one normal and one additive material"
    );

    // The second slot still uses the normal material: only the changed slot was
    // re-pointed.
    let (other_handle, _) = slot_material(&app, slots[2]);
    assert_eq!(other_handle, normal_handle);

    // Disposing the armature releases both acquired material keys.
    app.world_mut().entity_mut(root).despawn();
    app.update();
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        0,
        "cleanup releases every (image, blend) key the armature acquired"
    );
}

#[test]
fn two_slots_switching_to_the_same_new_blend_in_one_frame_share_one_reference() {
    let mut app = test_app();
    let (skeleton, atlas) =
        install_assets(&mut app, "Dragon/Dragon_ske.json", "Dragon/Dragon_tex.json");
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();

    let (armature, slots) = instance(&app, root);
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        1,
        "plain Dragon starts with one normal-blend material"
    );
    let changed: Vec<SlotIdx> = slots
        .iter()
        .take(2)
        .map(|entity| {
            app.world()
                .get::<DragonBonesSlot>(*entity)
                .expect("slot")
                .slot
        })
        .collect();
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime");
        let core_armature = runtime.factory.armatures[armature.index()]
            .as_mut()
            .expect("live armature");
        for slot in core_armature.slots.iter_mut() {
            if changed.contains(&slot.idx) {
                slot.blend_mode = BlendMode::Add;
            }
        }
    }

    app.update();
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        2,
        "both slots share one additive material"
    );

    app.world_mut().entity_mut(root).despawn();
    app.update();
    assert_eq!(
        app.world().resource::<Assets<DragonBonesMaterial>>().len(),
        0,
        "the armature only acquired the additive material once and cleanup released it"
    );
}
