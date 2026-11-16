//! Multi-atlas adapter path: one skeleton whose slots use two `_tex.json` pages.

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::image::Image;
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesCommand, DragonBonesExtraAtlases,
    DragonBonesInstance, DragonBonesPlugin, DragonBonesRuntime, DragonBonesSlot,
    DragonBonesSlotVisual, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::json;

fn skeleton() -> serde_json::Value {
    json!({
        "frameRate": 24,
        "name": "MultiAtlas",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "A",
            "aabb": { "x": 0, "y": 0, "width": 32, "height": 32 },
            "bone": [{ "name": "root" }],
            "slot": [
                { "name": "s0", "parent": "root" },
                { "name": "s1", "parent": "root" }
            ],
            "skin": [{
                "name": "",
                "slot": [
                    { "name": "s0", "display": [{ "type": "image", "name": "t0", "path": "t0" }] },
                    { "name": "s1", "display": [{ "type": "image", "name": "t1", "path": "t1" }] }
                ]
            }]
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
    assert!(
        ObjectDataParser::default().parse_texture_atlas_data(&raw, &mut data, 1.0),
        "atlas parses"
    );
    data
}

#[test]
fn slots_follow_their_own_atlas_page() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .add_plugins(DragonBonesPlugin);

    let data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton(), 1.0)
        .expect("skeleton parses");
    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(data),
        });
    let image0 = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let image1 = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let primary = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("page0", "t0"),
            texture: image0.clone(),
        });
    let secondary = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("page1", "t1"),
            texture: image1.clone(),
        });

    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas: primary,
                armature_name: "A".to_owned(),
                animation: None,
            },
            DragonBonesExtraAtlases(vec![secondary]),
            Transform::default(),
        ))
        .id();
    app.update();

    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance");
    assert_eq!(instance.slots.len(), 2);
    let mut query = app
        .world_mut()
        .query::<(&DragonBonesSlot, &DragonBonesSlotVisual)>();
    let visuals: Vec<(usize, Handle<Image>)> = query
        .iter(app.world())
        .map(|(slot, visual)| (slot.slot.index(), visual.atlas_image.clone()))
        .collect();
    assert_eq!(visuals.len(), 2);
    let page_of = |slot: usize| {
        visuals
            .iter()
            .find(|(idx, _)| *idx == slot)
            .map(|(_, image)| image.clone())
            .expect("slot visual")
    };
    assert_eq!(page_of(0), image0);
    assert_eq!(page_of(1), image1);
    assert_eq!(
        app.world()
            .resource::<Assets<dragonbones_bevy::DragonBonesMaterial>>()
            .len(),
        2,
        "two atlas pages with the same blend need two shared materials"
    );
}

#[test]
fn replace_atlas_command_refreshes_the_page_and_slot_image() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .add_plugins(DragonBonesPlugin);

    let data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton(), 1.0)
        .expect("skeleton parses");
    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(data),
        });
    let image0 = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let image1 = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let image2 = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let primary = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("page0", "t0"),
            texture: image0,
        });
    let secondary = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas("page1", "t1"),
            texture: image1,
        });
    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas: primary,
                armature_name: "A".to_owned(),
                animation: None,
            },
            DragonBonesExtraAtlases(vec![secondary]),
            Transform::default(),
        ))
        .id();
    app.update();

    let mut replacement = atlas("page1", "t1");
    replacement
        .textures
        .get_mut("t1")
        .expect("texture")
        .region
        .x = 8.0;
    let replacement = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: replacement,
            texture: image2.clone(),
        });
    let armature = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance")
        .armature;
    app.world_mut()
        .write_message(DragonBonesCommand::replace_atlas(root, 1, replacement));
    app.update();

    let region_x = {
        let runtime = app
            .world()
            .get_non_send::<DragonBonesRuntime>()
            .expect("runtime");
        runtime.factory.armature(armature).slots[1]
            .texture_data
            .as_ref()
            .map(|texture| texture.region.x)
    };
    assert_eq!(
        region_x,
        Some(8.0),
        "slot texture data must follow the replaced page"
    );

    let mut query = app
        .world_mut()
        .query::<(&DragonBonesSlot, &DragonBonesSlotVisual)>();
    let image = query
        .iter(app.world())
        .find(|(slot, _)| slot.slot.index() == 1)
        .map(|(_, visual)| visual.atlas_image.clone())
        .expect("slot visual");
    assert_eq!(image, image2, "slot visual must use the new page image");
}
