//! Cross-armature draw order: `DragonBonesSorting::armature_z_step` keeps each
//! top-level armature's slots in a contiguous z band.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesPlugin, DragonBonesSlot, DragonBonesSorting,
    SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[test]
fn armature_z_step_separates_top_level_armatures() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        .init_asset::<Mesh>()
        .insert_resource(DragonBonesSorting {
            armature_z_step: 0.1,
        })
        .add_plugins(DragonBonesPlugin);

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

    for _ in 0..2 {
        app.world_mut().spawn((
            DragonBonesArmature {
                skeleton: skeleton.clone(),
                atlas: atlas.clone(),
                armature_name: "Dragon".to_owned(),
                animation: None,
            },
            Transform::default(),
        ));
    }
    app.update();

    let mut query = app
        .world_mut()
        .query::<(&DragonBonesSlot, &GlobalTransform)>();
    let mut bands: std::collections::BTreeMap<u32, Vec<f32>> = Default::default();
    for (slot, global) in query.iter(app.world()) {
        bands
            .entry(slot.armature.index() as u32)
            .or_default()
            .push(global.translation().z);
    }
    assert_eq!(bands.len(), 2, "two top-level armatures");
    let mut min_max: Vec<(f32, f32)> = bands
        .values()
        .map(|zs| {
            (
                zs.iter().copied().fold(f32::INFINITY, f32::min),
                zs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            )
        })
        .collect();
    min_max.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        min_max[1].0 > min_max[0].1,
        "the second armature's lowest z must be above the first armature's highest z: {min_max:?}"
    );
}
