//! Headless integration tests for the asset-loading and visibility paths that
//! the rest of the suite bypasses (it builds assets by hand).
//!
//! - `AssetServer` really loads the adapter's `_ske.json` / `_tex.json` loaders,
//!   including the atlas' image path resolution.
//! - With `VisibilityPlugin` installed, hiding an ancestor hides an armature's
//!   slot (the adapter writes `Inherited`, and its sync runs before
//!   `VisibilitySystems::VisibilityPropagate`).

use std::sync::Arc;
use std::time::Duration;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::camera::visibility::VisibilityPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use dragonbones_bevy::{AtlasAsset, DragonBonesArmature, DragonBonesPlugin, SkeletonAsset};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;
use std::path::PathBuf;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn base_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        .init_asset::<Mesh>()
        // The atlas loader resolves `imagePath` into an Image handle, so the
        // asset type must exist even though this headless test never renders it.
        .init_asset::<Image>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .add_plugins(TransformPlugin)
        .add_plugins(VisibilityPlugin)
        .add_plugins(DragonBonesPlugin);
    app
}

#[test]
fn asset_server_loads_skeleton_and_atlas_through_the_adapter_loaders() {
    let mut app = base_app();
    let server = app.world().resource::<AssetServer>().clone();
    let skeleton: Handle<SkeletonAsset> = server.load("Dragon_ske.json");
    let atlas: Handle<AtlasAsset> = server.load("Dragon_tex.json");

    let mut loaded = false;
    for _ in 0..5000 {
        app.update();
        let skeletons = app.world().resource::<Assets<SkeletonAsset>>();
        let atlases = app.world().resource::<Assets<AtlasAsset>>();
        loaded = skeletons.get(&skeleton).is_some() && atlases.get(&atlas).is_some();
        if loaded {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(loaded, "adapter loaders did not finish in time");

    let skeletons = app.world().resource::<Assets<SkeletonAsset>>();
    let skeleton = skeletons.get(&skeleton).expect("skeleton asset");
    assert_eq!(skeleton.data.name, "Dragon");
    assert!(
        skeleton.data.armatures.contains_key("Dragon"),
        "skeleton contains the Dragon armature"
    );

    let atlases = app.world().resource::<Assets<AtlasAsset>>();
    let atlas = atlases.get(&atlas).expect("atlas asset");
    assert_eq!(atlas.data.name, "Dragon");
    assert!(atlas.data.width > 0.0 && atlas.data.height > 0.0);
    assert!(!atlas.data.textures.is_empty(), "atlas regions were parsed");
    assert_ne!(
        atlas.texture,
        Handle::<Image>::default(),
        "the loader resolves imagePath into a texture handle"
    );
}

fn install_dragon_assets(app: &mut App) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon_ske.json")).expect("read skeleton"),
    )
    .expect("parse skeleton");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon_tex.json")).expect("read atlas"),
    )
    .expect("parse atlas");
    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("parse dragon bones data");
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

#[test]
fn hiding_an_ancestor_hides_the_armature_slots() {
    let mut app = base_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let parent = app.world_mut().spawn(Visibility::Hidden).id();
    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Dragon".to_owned(),
                animation: Some("stand".to_owned()),
            },
            Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
            ChildOf(parent),
        ))
        .id();
    app.update();

    let slots = app
        .world()
        .get::<dragonbones_bevy::DragonBonesInstance>(root)
        .expect("instance")
        .slots
        .clone();
    assert!(!slots.is_empty());
    for slot in &slots {
        assert!(
            !app.world()
                .get::<InheritedVisibility>(*slot)
                .expect("inherited visibility")
                .get(),
            "an ancestor's Hidden must hide the slots"
        );
    }

    app.world_mut()
        .entity_mut(parent)
        .insert(Visibility::Inherited);
    app.update();
    for slot in &slots {
        assert!(
            app.world()
                .get::<InheritedVisibility>(*slot)
                .expect("inherited visibility")
                .get(),
            "showing the ancestor restores slot visibility"
        );
    }
}
