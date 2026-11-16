//! Color offset (DragonBones `ColorTransform` offsets) uses a per-slot material.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesMaterial, DragonBonesPlugin,
    DragonBonesRuntime, DragonBonesSlot, SkeletonAsset,
};
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
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);
    app
}

fn spawn_dragon(app: &mut App) -> (Entity, dragonbones_core::armature::ArmatureIdx, Entity) {
    let dir = assets_dir();
    let skeleton_json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("Dragon/Dragon_ske.json")).unwrap())
            .unwrap();
    let atlas_json: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("Dragon/Dragon_tex.json")).unwrap())
            .unwrap();
    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("parse Dragon");
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
    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Dragon".to_owned(),
                animation: Some("stand".to_owned()),
            },
            Transform::default(),
        ))
        .id();
    app.update();
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance");
    let slot_entity = instance.slots[0];
    (root, instance.armature, slot_entity)
}

fn material_offset(app: &App, slot: Entity) -> Vec4 {
    let handle = app
        .world()
        .get::<MeshMaterial2d<DragonBonesMaterial>>(slot)
        .expect("slot material")
        .0
        .clone();
    app.world()
        .resource::<Assets<DragonBonesMaterial>>()
        .get(&handle)
        .expect("material asset")
        .color_offset
}

#[test]
fn color_offset_uses_a_per_slot_material_and_clears_back_to_shared() {
    let mut app = test_app();
    let (_root, armature, slot_entity) = spawn_dragon(&mut app);
    let _ = app.world().get::<DragonBonesSlot>(slot_entity).unwrap();
    assert_eq!(material_offset(&app, slot_entity), Vec4::ZERO);

    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime");
        let slot = runtime
            .factory
            .armature_mut(armature)
            .slots
            .first_mut()
            .unwrap();
        slot.color_transform.red_offset = 0.5;
        slot.color_transform.alpha_offset = 0.25;
    }
    app.update();
    let offset = material_offset(&app, slot_entity);
    assert!(
        (offset.x - 0.5).abs() < 1e-6 && (offset.w - 0.25).abs() < 1e-6,
        "offset uniform: {offset:?}"
    );

    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime");
        let slot = runtime
            .factory
            .armature_mut(armature)
            .slots
            .first_mut()
            .unwrap();
        slot.color_transform.red_offset = 0.0;
        slot.color_transform.alpha_offset = 0.0;
    }
    app.update();
    assert_eq!(
        material_offset(&app, slot_entity),
        Vec4::ZERO,
        "clearing the offset must return the slot to the shared material"
    );
}
