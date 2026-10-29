//! Integration tests for the runtime animation command API.
//!
//! Game code addresses an armature root entity with [`DragonBonesCommand`]
//! messages; the adapter applies them before the frame's `advance_time`. These
//! tests run a headless `App` and inspect the resulting core animation state.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesCommand, DragonBonesInstance,
    DragonBonesPlayConfig, DragonBonesPlugin, DragonBonesRuntime, SkeletonAsset,
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
            texture: Handle::default(),
        });
    (skeleton, atlas)
}

fn spawn_armature(app: &mut App, animation: &str) -> Entity {
    let (skeleton, atlas) = install_dragon_assets(app);
    app.world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Dragon".to_owned(),
                animation: Some(animation.to_owned()),
            },
            Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
        ))
        .id()
}

fn armature_index(app: &App, root: Entity) -> dragonbones_core::armature::ArmatureIdx {
    app.world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance")
        .armature
}

/// `(animation name, time scale, playhead time)` of every active state.
fn states(app: &App, root: Entity) -> Vec<(String, f64, f64)> {
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    let armature = runtime.factory.armatures[armature_index(app, root).index()]
        .as_ref()
        .expect("armature alive");
    armature
        .animation
        .states()
        .map(|(_, state)| (state.animation_name.clone(), state.time_scale, state.time))
        .collect()
}

fn is_playing(app: &App, root: Entity) -> bool {
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    runtime.factory.armatures[armature_index(app, root).index()]
        .as_ref()
        .expect("armature alive")
        .animation
        .is_playing()
}

fn advance(app: &mut App, seconds: f64) {
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        runtime.factory.advance_time(seconds);
    }
    // Let the adapter apply the resulting pose/state in the same frame.
    app.update();
}

#[test]
fn play_and_stop_switch_the_active_animation() {
    let mut app = test_app();
    let root = spawn_armature(&mut app, "stand");
    app.update();
    assert!(
        states(&app, root)
            .iter()
            .any(|(name, _, _)| name == "stand")
    );

    app.world_mut()
        .write_message(DragonBonesCommand::play(root, "walk"));
    app.update();
    let active = states(&app, root);
    assert!(
        active.iter().any(|(name, _, _)| name == "walk"),
        "play command should start walk, got {active:?}"
    );

    app.world_mut()
        .write_message(DragonBonesCommand::stop(root));
    advance(&mut app, 0.05);
    assert!(
        !is_playing(&app, root),
        "stop command should stop playback, got {:?}",
        states(&app, root)
    );
}

#[test]
fn pause_freezes_the_playhead_and_resume_continues() {
    let mut app = test_app();
    let root = spawn_armature(&mut app, "stand");
    app.update();
    advance(&mut app, 0.1);

    app.world_mut()
        .write_message(DragonBonesCommand::pause(root));
    app.update();
    assert!(
        states(&app, root).iter().all(|(_, scale, _)| *scale == 0.0),
        "pause sets time_scale to 0: {:?}",
        states(&app, root)
    );

    let paused_at = states(&app, root)[0].2;
    advance(&mut app, 0.5);
    let still_at = states(&app, root)[0].2;
    assert!(
        (still_at - paused_at).abs() < 1e-9,
        "paused playhead must not move: {paused_at} -> {still_at}"
    );

    app.world_mut()
        .write_message(DragonBonesCommand::resume(root));
    app.update();
    assert!(
        states(&app, root).iter().all(|(_, scale, _)| *scale == 1.0),
        "resume restores normal speed"
    );
    advance(&mut app, 0.2);
    assert!(
        states(&app, root)[0].2 > still_at,
        "playhead advances again after resume"
    );
}

#[test]
fn set_speed_and_play_config_reach_the_core_state() {
    let mut app = test_app();
    let root = spawn_armature(&mut app, "stand");
    app.update();

    app.world_mut()
        .write_message(DragonBonesCommand::set_speed(root, 2.5));
    app.update();
    assert!(
        states(&app, root).iter().all(|(_, scale, _)| *scale == 2.5),
        "set_speed applies to every active state: {:?}",
        states(&app, root)
    );

    let config = DragonBonesPlayConfig::new("walk")
        .with_play_times(3)
        .with_time_scale(1.5)
        .with_position(0.25);
    app.world_mut()
        .write_message(DragonBonesCommand::play_config(root, config));
    app.update();
    let walk = states(&app, root)
        .into_iter()
        .find(|(name, _, _)| name == "walk")
        .expect("walk state exists");
    assert!((walk.1 - 1.5).abs() < 1e-9, "play_config speed: {walk:?}");
    // The command is applied before this frame's advance, so the playhead is
    // `position` plus one (small) frame delta.
    assert!(
        (walk.2 - 0.25).abs() < 0.02,
        "play_config position seek: {walk:?}"
    );
}

#[test]
fn command_for_a_non_armature_entity_is_ignored() {
    let mut app = test_app();
    let unrelated = app.world_mut().spawn_empty().id();
    app.world_mut()
        .write_message(DragonBonesCommand::play(unrelated, "walk"));
    // Must not panic; the command is dropped with a warning.
    app.update();
}
