//! Cross-data skin replacement: a skin from another skeleton/atlas is applied to
//! the target armature.

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::image::Image;
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesCommand, DragonBonesInstance, DragonBonesPlugin,
    DragonBonesRuntime, DragonBonesSlotVisual, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::{Value, json};

fn skeleton(name: &str) -> Value {
    json!({
        "frameRate": 24,
        "name": name,
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "A",
            "aabb": { "x": 0, "y": 0, "width": 16, "height": 16 },
            "bone": [{ "name": "root" }],
            "slot": [{ "name": "s", "parent": "root" }],
            "skin": [
                {
                    "name": "",
                    "slot": [{ "name": "s", "display": [{ "type": "image", "name": "base", "path": "base" }] }]
                },
                {
                    "name": "alt",
                    "slot": [{ "name": "s", "display": [{ "type": "image", "name": "alt_tex", "path": "alt_tex" }] }]
                }
            ]
        }]
    })
}

fn atlas(name: &str, texture: &str) -> TextureAtlasData {
    let raw = json!({
        "name": name,
        "imagePath": format!("{name}.png"),
        "width": 32,
        "height": 32,
        "SubTexture": [{ "name": texture, "x": 0, "y": 0, "width": 16, "height": 16 }]
    });
    let mut data = TextureAtlasData::default();
    assert!(ObjectDataParser::default().parse_texture_atlas_data(&raw, &mut data, 1.0));
    data
}

fn parse_skeleton(value: &Value) -> dragonbones_core::model::DragonBonesData {
    ObjectDataParser::default()
        .parse_dragon_bones_data(value, 1.0)
        .expect("skeleton parses")
}

#[test]
fn replace_skin_from_another_skeleton_uses_its_texture() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .add_plugins(DragonBonesPlugin);

    let target_image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let source_image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let target_skeleton =
        app.world_mut()
            .resource_mut::<Assets<SkeletonAsset>>()
            .add(SkeletonAsset {
                data: Arc::new(parse_skeleton(&skeleton("Target"))),
            });
    let target_atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("target_atlas", "base"),
            texture: target_image,
        });
    let source_skeleton =
        app.world_mut()
            .resource_mut::<Assets<SkeletonAsset>>()
            .add(SkeletonAsset {
                data: Arc::new(parse_skeleton(&skeleton("Source"))),
            });
    let source_atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("source_atlas", "alt_tex"),
            texture: source_image.clone(),
        });

    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton: target_skeleton,
                atlas: target_atlas,
                armature_name: "A".to_owned(),
                animation: None,
            },
            Transform::default(),
        ))
        .id();
    app.update();

    let armature = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance")
        .armature;
    app.world_mut()
        .write_message(DragonBonesCommand::replace_skin_from(
            root,
            "A",
            "alt",
            source_skeleton,
            vec![source_atlas],
        ));
    app.update();

    let (texture, atlas_name) = {
        let runtime = app
            .world()
            .get_non_send::<DragonBonesRuntime>()
            .expect("runtime");
        let slot = &runtime.factory.armature(armature).slots[0];
        (
            slot.texture_data
                .as_ref()
                .map(|texture| texture.name.clone()),
            slot.texture_atlas_ref().map(|atlas| atlas.name.clone()),
        )
    };
    assert_eq!(texture.as_deref(), Some("alt_tex"));
    assert!(
        atlas_name
            .as_deref()
            .is_some_and(|name| name.contains("Source")),
        "texture must come from the source atlas registration: {atlas_name:?}"
    );

    let mut query = app.world_mut().query::<&DragonBonesSlotVisual>();
    let visual = query.iter(app.world()).next().expect("slot visual");
    assert_eq!(visual.atlas_image, source_image);
}
