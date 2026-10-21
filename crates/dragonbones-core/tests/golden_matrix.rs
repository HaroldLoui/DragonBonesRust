//! 阶段 5 交叉动画矩阵验收（指南任务 5）。
//!
//! 读取 `testdata/matrix/**` 的 compact golden：每帧 states/events；checkpoint 帧
//! 全骨/槽 pose。逐场景复现 JS `tools/matrix.mjs` 的脚本时序（动作在 advance 之前）。

mod common;

use std::path::{Path, PathBuf};

use common::ir_snapshot::{asset_root, tex_stem};
use common::{EventLog, RecordingProxy, read_value};
use dragonbones_core::animation::StateId;
use dragonbones_core::armature::ArmatureIdx;
use dragonbones_core::core::enums::AnimationFadeOutMode;
use dragonbones_core::event::EventObject;
use dragonbones_core::factory::BaseFactory;
use serde_json::Value;

fn matrix_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/matrix")
}

fn matrix_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let path = entry.expect("read_dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&matrix_root(), &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "testdata/matrix 为空，先运行 `node tools/gen_matrix.mjs`"
    );
    files
}

#[test]
fn matrix_goldens_match() {
    for path in matrix_files() {
        run_matrix_case(&path);
    }
}

fn run_matrix_case(path: &Path) {
    let golden = read_value(path);
    let meta = &golden["meta"];
    let case_name = path
        .strip_prefix(matrix_root())
        .expect("matrix file under testdata/matrix")
        .to_string_lossy()
        .replace('\\', "/");
    let scenario = meta["scenario"].as_str().expect("scenario");
    let ske_file = meta["skeFile"].as_str().expect("skeFile");
    let ske_stem = ske_file.strip_suffix(".json").expect("ske suffix");
    let armature_name = meta["armature"].as_str().expect("armature");
    let anim_a = meta["animA"].as_str().expect("animA");
    let anim_b = meta["animB"].as_str().expect("animB");
    let dt = meta["dt"].as_f64().expect("dt");
    let frame_count = meta["frameCount"].as_u64().expect("frameCount") as usize;
    let checkpoints: Vec<usize> = meta["checkpoints"]
        .as_array()
        .expect("checkpoints")
        .iter()
        .map(|value| value.as_u64().expect("checkpoint") as usize)
        .collect();
    let compare_pose_enabled = meta["comparePose"].as_bool().expect("comparePose");

    let ske = read_value(&asset_root().join(ske_file));
    let tex = read_value(&asset_root().join(format!("{}.json", tex_stem(ske_stem))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, ske_stem)
        .unwrap_or_else(|| panic!("{case_name}: parse {ske_file}"));
    assert!(
        factory.parse_texture_atlas_data(&tex, ske_stem),
        "{case_name}: parse texture atlas"
    );

    let log = EventLog::default();
    let armature_count = if scenario == "dual_armature" { 2 } else { 1 };
    let mut armatures = Vec::new();
    for _ in 0..armature_count {
        let mut proxy = RecordingProxy::new(log.clone());
        proxy.record_all();
        let idx = factory
            .build_armature(armature_name, ske_stem, "", ske_stem, Box::new(proxy))
            .unwrap_or_else(|| panic!("{case_name}: build `{armature_name}`"));
        factory
            .attach_armature(factory.root_clock(), idx)
            .unwrap_or_else(|error| panic!("{case_name}: attach root clock: {error}"));
        armatures.push(idx);
    }

    let frames = golden["frames"].as_array().expect("frames");
    assert_eq!(frames.len(), frame_count, "{case_name}: frame count");
    let mut max_abs = 0.0_f64;
    let mut max_rel = 0.0_f64;

    for (step, frame) in frames.iter().enumerate() {
        apply_script(scenario, step, &mut factory, &armatures, anim_a, anim_b);
        factory.advance_time_with(dt, 100.0 + step as f64 * dt);

        let got_events = log.drain();
        compare_events(&case_name, step, frame, &got_events, &armatures);
        compare_states(&case_name, step, frame, &factory, &armatures);

        if compare_pose_enabled && checkpoints.contains(&step) {
            compare_pose(
                &case_name,
                step,
                frame,
                &factory,
                &armatures,
                &mut max_abs,
                &mut max_rel,
            );
        }
    }

    println!("{case_name}: frames={frame_count}, max_abs={max_abs:.3e}, max_rel={max_rel:.3e}");
}

fn goto_and_play(factory: &mut BaseFactory, idx: ArmatureIdx, name: &str) {
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time(name, 0.0, -1)
            .is_some(),
        "gotoAndPlayByTime({name})"
    );
}

fn fade_in(
    factory: &mut BaseFactory,
    idx: ArmatureIdx,
    name: &str,
    fade_in_time: f64,
    layer: i32,
    group: &str,
    mode: AnimationFadeOutMode,
) -> StateId {
    factory
        .armature_mut(idx)
        .animation
        .fade_in_with_options(name, fade_in_time, -1, layer, Some(group), mode)
        .unwrap_or_else(|| panic!("fadeIn({name})"))
}

fn state_mut(
    factory: &mut BaseFactory,
    idx: ArmatureIdx,
    id: StateId,
) -> &mut dragonbones_core::animation::AnimationState {
    factory
        .armature_mut(idx)
        .animation
        .state_mut(id)
        .expect("state alive")
}

fn apply_script(
    scenario: &str,
    step: usize,
    factory: &mut BaseFactory,
    armatures: &[ArmatureIdx],
    anim_a: &str,
    anim_b: &str,
) {
    let first = armatures[0];
    match scenario {
        "fade_cross" => {
            if step == 0 {
                goto_and_play(factory, first, anim_a);
            }
            if step == 30 {
                fade_in(
                    factory,
                    first,
                    anim_b,
                    0.2,
                    0,
                    "",
                    AnimationFadeOutMode::All,
                );
            }
        }
        "layer_overlay" => {
            if step == 0 {
                let id = fade_in(
                    factory,
                    first,
                    anim_a,
                    0.0,
                    0,
                    "g1",
                    AnimationFadeOutMode::SameLayerAndGroup,
                );
                state_mut(factory, first, id).reset_to_pose = false;
            }
            if step == 15 {
                let id = fade_in(
                    factory,
                    first,
                    anim_b,
                    0.0,
                    0,
                    "g2",
                    AnimationFadeOutMode::SameLayerAndGroup,
                );
                state_mut(factory, first, id).reset_to_pose = false;
            }
        }
        "reset_to_pose" => {
            if step == 0 {
                goto_and_play(factory, first, anim_a);
            }
            if step == 30 {
                fade_in(
                    factory,
                    first,
                    anim_b,
                    0.2,
                    0,
                    "",
                    AnimationFadeOutMode::All,
                );
            }
        }
        "replay" => {
            if step == 0 {
                goto_and_play(factory, first, anim_a);
            }
            if step == 30 {
                assert!(
                    factory
                        .armature_mut(first)
                        .animation
                        .play(Some(anim_a), -1)
                        .is_some()
                );
            }
        }
        "event_dense" => {
            if step == 0 {
                goto_and_play(factory, first, anim_a);
            }
        }
        "goto_roundtrip" => {
            if step == 0 {
                assert!(
                    factory
                        .armature_mut(first)
                        .animation
                        .goto_and_stop_by_frame(anim_a, 6)
                        .is_some()
                );
            }
            if step == 6 {
                assert!(
                    factory
                        .armature_mut(first)
                        .animation
                        .goto_and_play_by_frame(anim_a, 6, -1)
                        .is_some()
                );
            }
        }
        "timescale" => {
            if step == 0 {
                let id = fade_in(
                    factory,
                    first,
                    anim_a,
                    0.0,
                    0,
                    "",
                    AnimationFadeOutMode::SameLayerAndGroup,
                );
                state_mut(factory, first, id).time_scale = 2.0;
            }
        }
        "dual_armature" => {
            if step == 0 {
                goto_and_play(factory, armatures[0], anim_a);
                goto_and_play(factory, armatures[1], anim_b);
            }
        }
        "dispose" => {
            if step == 0 {
                goto_and_play(factory, first, anim_a);
            }
            if step == 30 {
                factory.dispose_armature(first);
            }
        }
        other => panic!("unknown matrix scenario `{other}`"),
    }
}

fn compare_events(
    case_name: &str,
    step: usize,
    frame: &Value,
    got: &[EventObject],
    armatures: &[ArmatureIdx],
) {
    let want: Vec<_> = frame["events"]
        .as_array()
        .expect("frame events")
        .iter()
        .map(|event| {
            (
                event["armature"].as_u64().unwrap_or(0) as usize,
                event["type"].as_str().unwrap_or("").to_owned(),
                event["name"].as_str().unwrap_or("").to_owned(),
                event["bone"].as_str().map(str::to_owned),
                event["slot"].as_str().map(str::to_owned),
            )
        })
        .collect();
    let actual: Vec<_> = got
        .iter()
        .map(|event| {
            let armature_index = event
                .armature
                .and_then(|idx| armatures.iter().position(|candidate| *candidate == idx))
                .unwrap_or(0);
            (
                armature_index,
                event.event_type.clone(),
                event.name.clone(),
                event.bone_name.clone(),
                event.slot_name.clone(),
            )
        })
        .collect();
    assert_eq!(actual, want, "{case_name} frame {step}: dispatched events");
}

fn state_rows(factory: &BaseFactory, armatures: &[ArmatureIdx]) -> Vec<StateRow> {
    let mut rows = Vec::new();
    for (armature_index, idx) in armatures.iter().enumerate() {
        let Some(armature) = factory.armatures.get(idx.index()).and_then(Option::as_ref) else {
            continue; // dispose 场景：flush 后骨架已回收
        };
        for (_, state) in armature.animation.states() {
            rows.push(StateRow {
                armature: armature_index,
                name: state.name.clone(),
                layer: state.layer,
                weight: state.weight,
                weight_result: state.weight_result,
                current_time: state.current_time(),
                fade_progress: state.fade_progress,
                playing: state.is_playing(),
                completed: state.is_completed(),
            });
        }
    }
    rows
}

#[derive(Debug, Clone, PartialEq)]
struct StateRow {
    armature: usize,
    name: String,
    layer: i32,
    weight: f64,
    weight_result: f64,
    current_time: f64,
    fade_progress: f64,
    playing: bool,
    completed: bool,
}

fn compare_states(
    case_name: &str,
    step: usize,
    frame: &Value,
    factory: &BaseFactory,
    armatures: &[ArmatureIdx],
) {
    let got = state_rows(factory, armatures);
    let want_values = frame["states"].as_array().expect("states");
    assert_eq!(
        got.len(),
        want_values.len(),
        "{case_name} frame {step}: state count (got {got:?})"
    );
    for (index, (got, want)) in got.iter().zip(want_values).enumerate() {
        let want = StateRow {
            armature: want["armature"].as_u64().expect("state armature") as usize,
            name: want["name"].as_str().expect("state name").to_owned(),
            layer: want["layer"].as_i64().expect("state layer") as i32,
            weight: want["weight"].as_f64().expect("state weight"),
            weight_result: want["weightResult"].as_f64().expect("state weightResult"),
            current_time: want["currentTime"].as_f64().expect("state currentTime"),
            fade_progress: want["fadeProgress"].as_f64().expect("state fadeProgress"),
            playing: want["playing"].as_bool().expect("state playing"),
            completed: want["completed"].as_bool().expect("state completed"),
        };
        assert_eq!(
            got.armature, want.armature,
            "{case_name} frame {step} state {index}: armature"
        );
        assert_eq!(
            got.name, want.name,
            "{case_name} frame {step} state {index}: name"
        );
        assert_eq!(
            got.layer, want.layer,
            "{case_name} frame {step} state {index}: layer"
        );
        assert_eq!(
            got.playing, want.playing,
            "{case_name} frame {step} state {index}: playing"
        );
        assert_eq!(
            got.completed, want.completed,
            "{case_name} frame {step} state {index}: completed"
        );
        for (field, got_value, want_value) in [
            ("weight", got.weight, want.weight),
            ("weightResult", got.weight_result, want.weight_result),
            ("currentTime", got.current_time, want.current_time),
            ("fadeProgress", got.fade_progress, want.fade_progress),
        ] {
            assert!(
                (got_value - want_value).abs() <= 1e-9 * (1.0 + want_value.abs()),
                "{case_name} frame {step} state {index} `{field}`: want={want_value} got={got_value}"
            );
        }
    }
}

fn compare_pose(
    case_name: &str,
    step: usize,
    frame: &Value,
    factory: &BaseFactory,
    armatures: &[ArmatureIdx],
    max_abs: &mut f64,
    max_rel: &mut f64,
) {
    for entry in frame["pose"].as_array().expect("pose") {
        let armature_index = entry["armature"].as_u64().expect("pose armature") as usize;
        let idx = armatures[armature_index];
        let armature = factory.armature(idx);
        for (name, expected) in entry["bones"].as_object().expect("pose bones") {
            let bone_idx = armature
                .get_bone(name)
                .unwrap_or_else(|| panic!("{case_name}: pose bone `{name}`"));
            let bone = armature
                .get_bones()
                .iter()
                .find(|bone| bone.idx == bone_idx)
                .expect("stable bone id");
            let global = &bone.global;
            let matrix = &bone.global_transform_matrix;
            let values = [
                global.x,
                global.y,
                global.rotation,
                global.scale_x,
                global.scale_y,
                global.skew,
                matrix.a,
                matrix.b,
                matrix.c,
                matrix.d,
                matrix.tx,
                matrix.ty,
                bone.global_alpha,
            ];
            compare_values(
                case_name, step, "bone", name, expected, &values, max_abs, max_rel,
            );
        }
        for (name, expected) in entry["slots"].as_object().expect("pose slots") {
            let slot_idx = armature
                .get_slot(name)
                .unwrap_or_else(|| panic!("{case_name}: pose slot `{name}`"));
            let slot = armature
                .get_slots()
                .iter()
                .find(|slot| slot.idx == slot_idx)
                .expect("stable slot id");
            let color = &slot.color_transform;
            let values = [
                f64::from(slot.display_index),
                f64::from(i32::from(slot.visible)),
                f64::from(slot.z_order),
                slot.z_index,
                color.red_multiplier,
                color.green_multiplier,
                color.blue_multiplier,
                color.alpha_multiplier,
                color.red_offset,
                color.green_offset,
                color.blue_offset,
                color.alpha_offset,
                slot.global.x,
                slot.global.y,
                slot.global_alpha,
            ];
            compare_values(
                case_name, step, "slot", name, expected, &values, max_abs, max_rel,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_values(
    case_name: &str,
    step: usize,
    kind: &str,
    name: &str,
    expected: &Value,
    got: &[f64],
    max_abs: &mut f64,
    max_rel: &mut f64,
) {
    let expected = expected.as_array().expect("pose vector");
    assert_eq!(
        expected.len(),
        got.len(),
        "{case_name}: {kind} `{name}` length"
    );
    for (index, (want, got)) in expected.iter().zip(got).enumerate() {
        let want = want
            .as_f64()
            .unwrap_or_else(|| panic!("{case_name}: {kind} `{name}`[{index}] 非数字"));
        let difference = (want - got).abs();
        *max_abs = max_abs.max(difference);
        *max_rel = max_rel.max(difference / (1.0 + want.abs()));
        assert!(
            difference <= 1e-9 * (1.0 + want.abs()),
            "{case_name} frame {step} {kind} `{name}`[{index}]: want={want} got={got} Δ={difference}"
        );
    }
}
