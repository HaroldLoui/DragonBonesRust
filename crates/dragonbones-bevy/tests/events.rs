//! Integration tests for DragonBones animation events forwarded into Bevy
//! messages.
//!
//! The adapter installs `BevyArmatureProxy`, which records every core
//! `EventObject`; a system in `Update` drains the queue into
//! [`DragonBonesEvent`] messages. These tests drive a real headless `App` and
//! read the messages with a normal `MessageReader`.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesEvent, DragonBonesInstance, DragonBonesPlugin,
    DragonBonesRuntime, DragonBonesSystems, SkeletonAsset,
};
use dragonbones_core::event::EventObject;
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[derive(Resource, Default)]
struct Collected(Vec<DragonBonesEvent>);

fn collect_events(mut reader: MessageReader<DragonBonesEvent>, mut collected: ResMut<Collected>) {
    for event in reader.read() {
        collected.0.push(event.clone());
    }
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
    app.init_resource::<Collected>();
    // Read messages after the adapter's Advance set has forwarded them.
    app.add_systems(Update, collect_events.after(DragonBonesSystems::Advance));
    app
}

fn install_dragon_assets(app: &mut App) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon/Dragon_ske.json")).expect("read Dragon_ske.json"),
    )
    .expect("parse Dragon_ske.json");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon/Dragon_tex.json")).expect("read Dragon_tex.json"),
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

fn take_events(app: &mut App) -> Vec<DragonBonesEvent> {
    std::mem::take(&mut app.world_mut().resource_mut::<Collected>().0)
}

#[test]
fn forwarded_events_are_drained_and_not_replayed() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();
    let _ = take_events(&mut app);

    // Cross at least one loop boundary; the core buffers the lifecycle events and
    // the next Update drains them into Bevy messages.
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        runtime.factory.advance_time(2.0);
    }
    app.update();

    let first_batch = take_events(&mut app);
    assert!(
        first_batch
            .iter()
            .any(|event| event.event_type() == EventObject::LOOP_COMPLETE),
        "a looping animation forwards loopComplete: {:?}",
        first_batch
            .iter()
            .map(DragonBonesEvent::event_type)
            .collect::<Vec<_>>()
    );
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance");
    assert!(
        first_batch
            .iter()
            .all(|event| event.armature == instance.armature),
        "every forwarded event belongs to the emitting armature"
    );

    // The queue is drained: idle frames must not replay the previous batch.
    for _ in 0..3 {
        app.update();
        let later = take_events(&mut app);
        assert!(
            later
                .iter()
                .all(|event| event.event_type() != EventObject::LOOP_COMPLETE),
            "already-forwarded events must not be replayed: {:?}",
            later
                .iter()
                .map(DragonBonesEvent::event_type)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn advancing_time_emits_loop_complete_with_armature_association() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let root = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();
    let _ = take_events(&mut app);

    // Drive the clock far enough that a looping animation wraps at least once;
    // the core buffers the events and the next Update forwards them.
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        runtime.factory.advance_time(5.0);
    }
    app.update();

    let events = take_events(&mut app);
    let loop_complete = events
        .iter()
        .find(|event| event.event_type() == EventObject::LOOP_COMPLETE)
        .expect("looping animation forwards loopComplete");
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance");
    assert_eq!(loop_complete.armature, instance.armature);
    assert!(
        events.iter().all(|event| event.event.armature.is_some()),
        "every forwarded event carries its core armature index"
    );
}

#[test]
fn events_from_multiple_armatures_keep_their_own_identity() {
    let mut app = test_app();
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    let first = spawn_armature(&mut app, &skeleton, &atlas);
    let second = spawn_armature(&mut app, &skeleton, &atlas);
    app.update();

    let events = take_events(&mut app);
    let first_armature = app
        .world()
        .get::<DragonBonesInstance>(first)
        .expect("first instance")
        .armature;
    let second_armature = app
        .world()
        .get::<DragonBonesInstance>(second)
        .expect("second instance")
        .armature;
    assert_ne!(first_armature, second_armature);

    for armature in [first_armature, second_armature] {
        assert!(
            events.iter().any(|event| event.armature == armature),
            "expected at least one event for armature {armature:?}"
        );
    }
    // Nothing is attributed to armatures this app does not own.
    assert!(
        events
            .iter()
            .all(|event| event.armature == first_armature || event.armature == second_armature)
    );
}
