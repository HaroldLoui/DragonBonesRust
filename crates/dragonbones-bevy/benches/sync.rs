//! Criterion benchmarks for the adapter's `PostUpdate` sync path.
//!
//! The benchmark builds a headless [`App`] (no renderer) and measures:
//!
//! - `sync/static/*`: only the `PostUpdate` schedule runs, so the animation never
//!   advances. This is the "idle character" case that the armature-level dirty
//!   check in [`DragonBonesSyncConfig::skip_clean_armatures`] targets; the
//!   benchmark runs the case with the switch **off** and **on** so the two can be
//!   compared directly.
//! - `sync/animated/*`: the animation advances by 1/60 s before every sync, so
//!   every armature is genuinely dirty.
//!
//! Run with:
//!
//! ```text
//! cargo bench -p dragonbones-bevy
//! cargo bench -p dragonbones-bevy -- --sample-size 10 --warm-up-time 1 --measurement-time 2
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use bevy::app::PostUpdate;
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use criterion::{Criterion, criterion_group, criterion_main};
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesPlugin, DragonBonesRuntime, DragonBonesSyncConfig,
    SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

/// Armature counts measured by every scenario.
const ARMATURE_COUNTS: [usize; 3] = [1, 10, 100];

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn test_app(skip_clean_armatures: bool) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        // `DragonBonesPlugin` registers the adapter's custom material.
        .init_asset::<Mesh>();
    // Inserted before the plugin: `init_resource` keeps an existing value.
    app.insert_resource(DragonBonesSyncConfig {
        skip_clean_armatures,
    });
    app.add_plugins(DragonBonesPlugin);
    app
}

/// Parses the Dragon rig with the core parser and stores it as adapter assets.
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
            // No renderer in this app, so the page image need not exist.
            texture: Handle::default(),
        });
    (skeleton, atlas)
}

/// Builds `count` armatures and runs one full frame so they are all posed.
fn build_app(skip_clean_armatures: bool, count: usize) -> App {
    let mut app = test_app(skip_clean_armatures);
    let (skeleton, atlas) = install_dragon_assets(&mut app);
    for _ in 0..count {
        app.world_mut().spawn((
            DragonBonesArmature {
                skeleton: skeleton.clone(),
                atlas: atlas.clone(),
                armature_name: "Dragon".to_owned(),
                animation: Some("stand".to_owned()),
            },
            Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
        ));
    }
    app.update();
    app
}

/// Runs only the `PostUpdate` schedule (the adapter sync lives there). The
/// animation does not advance, so idle armatures become clean after the first
/// frame.
fn run_sync(app: &mut App) {
    app.world_mut().run_schedule(PostUpdate);
}

/// Advances every armature by one 60 Hz tick, then syncs.
fn advance_and_sync(app: &mut App) {
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("DragonBonesRuntime is inserted by the plugin");
        runtime.factory.advance_time(1.0 / 60.0);
    }
    run_sync(app);
}

fn bench_static(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync/static");
    for count in ARMATURE_COUNTS {
        for skip_clean in [false, true] {
            let mut app = build_app(skip_clean, count);
            let label = format!(
                "{count:03}_armatures/{}/",
                if skip_clean { "skip_on" } else { "skip_off" }
            );
            group.bench_function(label.clone(), |b| {
                b.iter(|| run_sync(&mut app));
            });
        }
    }
    group.finish();
}

fn bench_animated(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync/animated");
    for count in ARMATURE_COUNTS {
        let mut app = build_app(true, count);
        group.bench_function(format!("{count:03}_armatures/"), |b| {
            b.iter(|| advance_and_sync(&mut app));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_static, bench_animated);
criterion_main!(benches);
