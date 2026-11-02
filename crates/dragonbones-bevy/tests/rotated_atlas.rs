//! End-to-end test for `rotated: true` atlas regions.
//!
//! No shipped asset uses rotated regions, so this test drives a synthetic
//! skeleton/atlas pair through the real adapter path and asserts the quad
//! geometry and UV mapping that the reference adapters use
//! (`PhaserSlot.ts:174-182`, `CocosSlot.ts:221-229`): the displayed content is
//! the rotated (width/height swapped) region and the atlas UVs rotate with it.

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesPlugin, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::{Value, json};

fn skeleton_json(rotated: bool) -> Value {
    json!({
        "frameRate": 24,
        "name": if rotated { "SyntheticRotated" } else { "SyntheticPlain" },
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "Armature",
            "aabb": { "x": 0, "y": 0, "width": 8, "height": 16 },
            "bone": [{ "name": "root" }],
            "slot": [{ "name": "slot", "parent": "root" }],
            "skin": [{
                "name": "",
                "slot": [{
                    "name": "slot",
                    "display": [{
                        "type": "image",
                        "name": "tex",
                        "transform": { "x": 0, "y": 0 }
                    }]
                }]
            }],
            "animation": [{
                "duration": 1,
                "playTimes": 0,
                "name": "idle",
                "bone": [{ "name": "root" }]
            }]
        }]
    })
}

fn atlas_json(rotated: bool) -> Value {
    json!({
        "name": if rotated { "SyntheticRotated" } else { "SyntheticPlain" },
        "imagePath": "synthetic.png",
        "width": 32,
        "height": 32,
        "SubTexture": [{
            "name": "tex",
            "x": 0,
            "y": 0,
            "width": 16,
            "height": 8,
            "rotated": rotated
        }]
    })
}

/// Builds a headless app whose assets are the synthetic JSON above.
fn app_with_synthetic_rig(rotated: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);

    let skeleton_data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton_json(rotated), 1.0)
        .expect("synthetic skeleton parses");
    let mut atlas_data = TextureAtlasData::default();
    assert!(
        ObjectDataParser::default().parse_texture_atlas_data(
            &atlas_json(rotated),
            &mut atlas_data,
            1.0
        ),
        "synthetic atlas parses"
    );

    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(skeleton_data),
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
                armature_name: "Armature".to_owned(),
                animation: Some("idle".to_owned()),
            },
            Transform::from_scale(Vec3::new(1.0, -1.0, 1.0)),
        ))
        .id();
    app.update();
    (app, root)
}

fn slot_mesh(app: &App, root: Entity) -> Mesh {
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance");
    let slot = instance.slots.first().copied().expect("one slot entity");
    let mesh_handle = app
        .world()
        .get::<Mesh2d>(slot)
        .expect("slot mesh")
        .0
        .clone();
    app.world()
        .resource::<Assets<Mesh>>()
        .get(&mesh_handle)
        .expect("mesh asset")
        .clone()
}

fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("unexpected position attribute: {other:?}"),
    }
}

fn uvs(mesh: &Mesh) -> Vec<[f32; 2]> {
    match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(bevy::mesh::VertexAttributeValues::Float32x2(values)) => values.clone(),
        other => panic!("unexpected uv attribute: {other:?}"),
    }
}

#[test]
fn rotated_atlas_region_swaps_content_size_and_rotates_uvs() {
    let (app, root) = app_with_synthetic_rig(true);
    let mesh = slot_mesh(&app, root);

    // Stored region 16 x 8 on a 32 x 32 page, displayed rotated: 8 wide x 16 high.
    // The default DragonBones pivot is the region centre, so the quad is centred.
    assert_eq!(
        positions(&mesh),
        vec![
            [-4.0, -8.0, 0.0],
            [4.0, -8.0, 0.0],
            [4.0, 8.0, 0.0],
            [-4.0, 8.0, 0.0],
        ]
    );
    assert_eq!(
        uvs(&mesh),
        vec![[0.5, 0.0], [0.5, 0.25], [0.0, 0.25], [0.0, 0.0],]
    );
}

#[test]
fn plain_atlas_region_keeps_region_size_and_linear_uvs() {
    let (app, root) = app_with_synthetic_rig(false);
    let mesh = slot_mesh(&app, root);

    assert_eq!(
        positions(&mesh),
        vec![
            [-8.0, -4.0, 0.0],
            [8.0, -4.0, 0.0],
            [8.0, 4.0, 0.0],
            [-8.0, 4.0, 0.0],
        ]
    );
    assert_eq!(
        uvs(&mesh),
        vec![[0.0, 0.0], [0.5, 0.0], [0.5, 0.25], [0.0, 0.25],]
    );
}
