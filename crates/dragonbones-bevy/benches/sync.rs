//! Criterion benchmarks for the adapter's `PostUpdate` sync path.
//!
//! The benchmark builds a headless [`App`] (no renderer) and measures:
//!
//! - `sync/static/*`: only the `PostUpdate` schedule runs, so the animation never
//!   advances. This is the "idle character" case that the armature-level dirty
//!   check in [`DragonBonesSyncConfig::skip_clean_armatures`] targets; the
//!   benchmark runs the case with the switch **off** and **on** so the two can be
//!   compared directly.
//! - `advance/{rig}/*`: only `BaseFactory::advance_time(1/60)` runs — the core
//!   animation cost that the adapter cannot remove.
//! - `sync/animated/{rig}/*`: the animation advances by 1/60 s before every
//!   sync, so every armature is genuinely dirty. Comparing this with
//!   `advance/{rig}` isolates the adapter's per-frame sync cost; comparing the
//!   rigs shows whether the cost lives in skinned-mesh updates (Rooster_Ani has
//!   weighted meshes) or in the per-slot transform write path (Dragon is mostly
//!   image slots). This is the profile evidence behind
//!   `docs/optimization-roadmap.md` §1.4 / §3.3.
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

/// Armature counts measured by the static A/B group.
const ARMATURE_COUNTS: [usize; 3] = [1, 10, 100];
/// Armature counts for the (more expensive) rig-comparison groups.
const RIG_COUNTS: [usize; 2] = [1, 10];

/// One benchmark rig: a skeleton/atlas pair plus the armature and animation to
/// play.
struct Rig {
    label: &'static str,
    skeleton_file: &'static str,
    atlas_file: &'static str,
    armature_name: &'static str,
    animation: &'static str,
}

/// Image-heavy rig (almost every slot is a plain atlas quad).
const DRAGON: Rig = Rig {
    label: "Dragon",
    skeleton_file: "Dragon/Dragon_ske.json",
    atlas_file: "Dragon/Dragon_tex.json",
    armature_name: "Dragon",
    animation: "stand",
};

/// Skinned rig: the same asset also has weighted meshes, so its per-frame sync
/// has to rebuild mesh vertex positions on the CPU.
const ROOSTER: Rig = Rig {
    label: "Rooster",
    skeleton_file: "Rooster_Ani/Rooster_Ani_ske.json",
    atlas_file: "Rooster_Ani/Rooster_Ani_tex.json",
    armature_name: "armatureName",
    animation: "rooster_idle_anim",
};

const RIGS: [&Rig; 2] = [&DRAGON, &ROOSTER];

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

/// Parses a rig's skeleton/atlas with the core parser and stores it as adapter
/// assets.
fn install_assets(app: &mut App, rig: &Rig) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(rig.skeleton_file)).expect("read skeleton"),
    )
    .expect("parse skeleton");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(rig.atlas_file)).expect("read atlas"),
    )
    .expect("parse atlas");

    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("core parses skeleton");
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

/// App with the rig assets loaded but no armatures: measures the fixed cost of
/// running the `PostUpdate` schedule once (the adapter sync plus the custom
/// material's `PostUpdate` bookkeeping from `Material2dPlugin`). Subtracting this
/// from the `sync/static` numbers gives the per-armature sync cost.
fn build_empty_app(skip_clean_armatures: bool, rig: &Rig) -> App {
    let mut app = test_app(skip_clean_armatures);
    let _ = install_assets(&mut app, rig);
    app
}

/// Builds `count` armatures and runs one full frame so they are all posed.
fn build_app(skip_clean_armatures: bool, count: usize, rig: &Rig) -> App {
    let mut app = test_app(skip_clean_armatures);
    let (skeleton, atlas) = install_assets(&mut app, rig);
    for _ in 0..count {
        app.world_mut().spawn((
            DragonBonesArmature {
                skeleton: skeleton.clone(),
                atlas: atlas.clone(),
                armature_name: rig.armature_name.to_owned(),
                animation: Some(rig.animation.to_owned()),
            },
            Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
        ));
    }
    app.update();
    app
}

/// Number of slots/bones of the first armature, for per-slot cost analysis.
fn rig_shape(app: &App) -> (usize, usize) {
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime");
    let armature = runtime
        .factory
        .armatures
        .iter()
        .flatten()
        .next()
        .expect("one built armature");
    (armature.slots.len(), armature.bones.len())
}

/// Runs only the `PostUpdate` schedule (the adapter sync lives there). The
/// animation does not advance, so idle armatures become clean after the first
/// frame.
fn run_sync(app: &mut App) {
    app.world_mut().run_schedule(PostUpdate);
}

/// Advances every armature by one 60 Hz tick (core cost only, no adapter work).
fn advance_only(app: &mut App) {
    let mut runtime = app
        .world_mut()
        .get_non_send_mut::<DragonBonesRuntime>()
        .expect("DragonBonesRuntime is inserted by the plugin");
    runtime.factory.advance_time(1.0 / 60.0);
}

/// Advances every armature by one 60 Hz tick, then syncs.
fn advance_and_sync(app: &mut App) {
    advance_only(app);
    run_sync(app);
}

fn bench_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync/baseline");
    for skip_clean in [false, true] {
        let mut app = build_empty_app(skip_clean, &DRAGON);
        let label = if skip_clean { "skip_on/" } else { "skip_off/" };
        group.bench_function(label, |b| {
            b.iter(|| run_sync(&mut app));
        });
    }
    group.finish();
}

fn bench_static(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync/static");
    for count in ARMATURE_COUNTS {
        for skip_clean in [false, true] {
            let mut app = build_app(skip_clean, count, &DRAGON);
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

fn bench_advance(c: &mut Criterion) {
    let mut group = c.benchmark_group("advance");
    for rig in RIGS {
        for count in RIG_COUNTS {
            let mut app = build_app(true, count, rig);
            let (slots, bones) = rig_shape(&app);
            if count == 1 {
                eprintln!(
                    "rig {}: {slots} slots, {bones} bones per armature",
                    rig.label
                );
            }
            group.bench_function(format!("{}/{count:03}_armatures/", rig.label), |b| {
                b.iter(|| advance_only(&mut app));
            });
        }
    }
    group.finish();
}

fn bench_animated(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync/animated");
    for rig in RIGS {
        for count in RIG_COUNTS {
            let mut app = build_app(true, count, rig);
            group.bench_function(format!("{}/{count:03}_armatures/", rig.label), |b| {
                b.iter(|| advance_and_sync(&mut app));
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_baseline,
    bench_static,
    bench_advance,
    bench_animated
);
criterion_main!(benches);
