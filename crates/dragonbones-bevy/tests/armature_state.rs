//! Integration tests for the one-shot `DragonBonesArmature` build request.
//!
//! The adapter reports the request phase through `DragonBonesArmatureState`,
//! rolls back factory registrations when building fails, and ignores field
//! changes after the armature was built (animation switching goes through
//! `DragonBonesCommand`).

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesArmaturePhase, DragonBonesArmatureState,
    DragonBonesInstance, DragonBonesPlugin, SkeletonAsset,
};
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
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);
    app
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

fn spawn_request(
    app: &mut App,
    skeleton: Handle<SkeletonAsset>,
    atlas: Handle<AtlasAsset>,
    armature_name: &str,
) -> Entity {
    app.world_mut()
        .spawn(DragonBonesArmature {
            skeleton,
            atlas,
            armature_name: armature_name.to_owned(),
            animation: Some("stand".to_owned()),
        })
        .id()
}

fn state(app: &App, root: Entity) -> Option<DragonBonesArmatureState> {
    app.world().get::<DragonBonesArmatureState>(root).cloned()
}

#[test]
fn missing_assets_keep_the_request_waiting_then_build_it() {
    let mut app = test_app();
    let root = spawn_request(&mut app, Handle::default(), Handle::default(), "Dragon");
    app.update();

    let waiting = state(&app, root).expect("state component");
    assert_eq!(waiting.phase, DragonBonesArmaturePhase::WaitingForAssets);
    assert!(
        app.world().get::<DragonBonesInstance>(root).is_none(),
        "no instance is built while assets are missing"
    );

    // "Loading" finishes: swap in the real assets.
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    app.world_mut()
        .entity_mut(root)
        .insert(DragonBonesArmature {
            skeleton,
            atlas,
            armature_name: "Dragon".to_owned(),
            animation: Some("stand".to_owned()),
        });
    app.update();

    let ready = state(&app, root).expect("state component");
    assert_eq!(ready.phase, DragonBonesArmaturePhase::Ready);
    assert!(app.world().get::<DragonBonesInstance>(root).is_some());
}

#[test]
fn failed_build_reports_failure_and_rolls_back_registrations() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_request(&mut app, skeleton, atlas, "NoSuchArmature");
    app.update();

    let failed = state(&app, root).expect("state component");
    assert!(
        matches!(&failed.phase, DragonBonesArmaturePhase::Failed { message } if message.contains("NoSuchArmature")),
        "unexpected phase: {:?}",
        failed.phase
    );
    assert!(app.world().get::<DragonBonesInstance>(root).is_none());
    let runtime = app
        .world()
        .get_non_send::<dragonbones_bevy::DragonBonesRuntime>()
        .expect("runtime resource");
    assert!(
        runtime.factory.data_map.is_empty(),
        "failed build must not leak the skeleton registration"
    );
    assert!(
        runtime.factory.atlas_map.is_empty(),
        "failed build must not leak the atlas registration"
    );
}

#[test]
fn changing_the_request_after_build_is_ignored() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_request(&mut app, skeleton, atlas, "Dragon");
    app.update();
    let armature = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance")
        .armature;
    let hash_before = state(&app, root).expect("state").request_hash;

    // Change the animation field; the built armature must stay untouched.
    let (skeleton, atlas) = {
        let request = app
            .world()
            .get::<DragonBonesArmature>(root)
            .expect("request");
        (request.skeleton.clone(), request.atlas.clone())
    };
    app.world_mut()
        .entity_mut(root)
        .insert(DragonBonesArmature {
            skeleton,
            atlas,
            armature_name: "Dragon".to_owned(),
            animation: Some("walk".to_owned()),
        });
    app.update();

    let after = state(&app, root).expect("state");
    assert_eq!(after.phase, DragonBonesArmaturePhase::Ready);
    assert_ne!(
        after.request_hash, hash_before,
        "the ignored request change is recorded so it is only warned about once"
    );
    assert_eq!(
        app.world()
            .get::<DragonBonesInstance>(root)
            .expect("instance")
            .armature,
        armature,
        "the existing armature is not rebuilt"
    );
}
