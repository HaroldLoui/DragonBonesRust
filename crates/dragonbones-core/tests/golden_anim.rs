//! Stage 4/5 animation pose and dispatched-event golden comparison.

mod common;

use common::ir_snapshot::{asset_root, tex_stem};
use common::{EventLog, RecordingProxy};
use common::{all_golden_files, read_value};
use dragonbones_core::event::EventObject;
use dragonbones_core::factory::BaseFactory;
use serde_json::Value;
use std::path::Path;

#[test]
fn animation_poses_and_dispatched_events_match_all_goldens() {
    for golden_path in all_golden_files() {
        run_animation_golden(&golden_path);
    }
}

/// S5-Event listener 闸门正/反向对照：同一 Shoot fixture 先注册 9 类 listener
/// 证明事件确实产生并**派发**，再清空 listener 证明 Frame/lifecycle 全部被入队闸门抑制。
#[test]
fn event_listener_gates_suppress_frame_and_lifecycle_actions() {
    let positive = run_shoot_listener_probe(true);
    assert!(
        !positive.is_empty(),
        "正向对照失败：9 类 listener 全部注册时 Shoot 应派发非空事件，\
         否则后续「无 listener 为空」无法证明闸门生效（S5-Event）"
    );
    assert!(
        positive
            .iter()
            .any(|event| event.event_type == EventObject::FRAME_EVENT),
        "正向对照应包含 Shoot 的 frameEvent（实际类型：{:?}）",
        positive
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        positive
            .iter()
            .any(|event| event.event_type == EventObject::FADE_IN),
        "正向对照应包含播放入口产生的 fadeIn（实际类型：{:?}）",
        positive
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );

    let negative = run_shoot_listener_probe(false);
    assert!(
        negative.is_empty(),
        "反向对照失败：未注册任何 listener 时 Frame/lifecycle 事件应全部不入队、不派发，\
         实际派发 {:?}（S5-Event listener 闸门；Sound 例外不在此动画触发路径）",
        negative
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>()
    );
}

fn run_shoot_listener_probe(with_probes: bool) -> Vec<EventObject> {
    let golden_path = common::golden_path("PlantAnimate", "Shoot");
    let golden = read_value(&golden_path);
    let ske_file = golden["meta"]["skeFile"].as_str().expect("ske file");
    let armature_name = golden["meta"]["armature"].as_str().expect("armature name");
    let ske_stem = ske_file.strip_suffix(".json").expect("ske suffix");
    let ske = common::read_value(&asset_root().join(ske_file));
    let tex = common::read_value(&asset_root().join(format!("{}.json", tex_stem(ske_stem))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, ske_stem)
        .expect("parse PlantAnimate");
    assert!(factory.parse_texture_atlas_data(&tex, ske_stem));

    let log = EventLog::default();
    let mut proxy = RecordingProxy::new(log.clone());
    if with_probes {
        proxy.record_all();
    }
    let armature_idx = factory
        .build_armature(armature_name, ske_stem, "", ske_stem, Box::new(proxy))
        .expect("build Plant armature");
    factory
        .attach_armature(factory.root_clock(), armature_idx)
        .expect("attach Plant to root clock");
    assert!(
        factory
            .armature_mut(armature_idx)
            .animation
            .goto_and_play_by_time("Shoot", 0.0, -1)
            .is_some()
    );

    let dt = golden["meta"]["dt"].as_f64().expect("golden dt");
    let mut events = Vec::new();
    for _ in 0..120 {
        factory.advance_time(dt);
        events.extend(log.drain());
    }
    events
}

fn run_animation_golden(golden_path: &Path) {
    let golden = read_value(golden_path);
    let meta = &golden["meta"];
    let golden_name = golden_path
        .strip_prefix(common::golden_root())
        .expect("golden lives under testdata/golden")
        .to_string_lossy()
        .replace('\\', "/");
    let ske_file = meta["skeFile"].as_str().expect("ske file name");
    let ske_stem = ske_file.strip_suffix(".json").expect("ske suffix");
    let animation_name = meta["animation"].as_str().expect("animation name");
    let armature_name = meta["armature"].as_str().expect("armature name");
    let frame_rate = meta["frameRate"].as_f64().expect("frame rate");
    let dt = meta["dt"].as_f64().expect("frame dt");
    let frames = golden["frames"].as_array().expect("golden frame list");

    let ske = common::read_value(&asset_root().join(ske_file));
    let tex_path = asset_root().join(format!("{}.json", tex_stem(ske_stem)));
    let tex = common::read_value(&tex_path);
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, ske_stem)
        .unwrap_or_else(|| panic!("{golden_name}: failed to parse {ske_file}"));
    assert!(
        factory.parse_texture_atlas_data(&tex, ske_stem),
        "{golden_name}: texture atlas parse"
    );
    let log = EventLog::default();
    let mut proxy = RecordingProxy::new(log.clone());
    proxy.record_all();
    let armature_idx = factory
        .build_armature(armature_name, ske_stem, "", ske_stem, Box::new(proxy))
        .unwrap_or_else(|| panic!("{golden_name}: failed to build `{armature_name}`"));
    factory
        .attach_armature(factory.root_clock(), armature_idx)
        .unwrap_or_else(|error| panic!("{golden_name}: attach to root clock: {error}"));
    assert!(
        factory
            .armature_mut(armature_idx)
            .animation
            .goto_and_play_by_time(animation_name, 0.0, -1)
            .is_some(),
        "{golden_name}: animation should exist"
    );

    assert_eq!(frames.len(), 120, "{golden_name}: expected 120 frames");
    let want_bone_names = string_list(&meta["boneNames"]);
    let want_slot_names = string_list(&meta["slotNames"]);
    let mut max_abs = 0.0_f64;
    let mut max_rel = 0.0_f64;
    for (frame_index, frame) in frames.iter().enumerate() {
        factory.advance_time(dt);
        let got_events = log.drain();
        compare_events(&golden_name, frame_index, frame, &got_events);

        let armature = factory.armature(armature_idx);
        for (name, expected) in frame["bones"].as_object().expect("bone snapshots") {
            let idx = armature
                .get_bone(name)
                .unwrap_or_else(|| panic!("{golden_name}: missing bone {name}"));
            let bone = armature
                .get_bones()
                .iter()
                .find(|bone| bone.idx == idx)
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
                &golden_name,
                frame_index,
                "bone",
                name,
                expected,
                &values,
                &mut max_abs,
                &mut max_rel,
            );
        }

        for (name, expected) in frame["slots"].as_object().expect("slot snapshots") {
            let idx = armature
                .get_slot(name)
                .unwrap_or_else(|| panic!("{golden_name}: missing slot {name}"));
            let slot = armature
                .get_slots()
                .iter()
                .find(|slot| slot.idx == idx)
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
                &golden_name,
                frame_index,
                "slot",
                name,
                expected,
                &values,
                &mut max_abs,
                &mut max_rel,
            );
        }
    }
    let armature = factory.armature(armature_idx);
    let got_bone_names: Vec<_> = armature
        .get_bones()
        .iter()
        .map(|bone| bone.bone_name.clone())
        .collect();
    assert_eq!(
        got_bone_names, want_bone_names,
        "{golden_name}: final bone order"
    );
    let got_slot_names: Vec<_> = armature
        .get_slots()
        .iter()
        .map(|slot| slot.slot_name.clone())
        .collect();
    assert_eq!(
        got_slot_names, want_slot_names,
        "{golden_name}: final slot order"
    );
    assert!(frame_rate > 0.0, "{golden_name}: invalid frame rate");
    assert!(
        max_abs.is_finite(),
        "{golden_name}: observed non-finite pose error {max_abs}"
    );
    // 任务 7 加固：全库实测 max_rel = 3.085e-14，距 1e-12 仍有约 30 倍余量；
    // 该断言用于防止 compare_values 以外的路径静默放宽相对误差。
    assert!(
        max_rel <= 1e-12,
        "{golden_name}: 最大相对姿态误差 {max_rel:.3e} 超过 1e-12（实测全库最大 3.085e-14）"
    );
    println!(
        "{golden_name}: max absolute pose error={max_abs:.3e}, max relative pose error={max_rel:.3e}"
    );
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("string list")
        .iter()
        .map(|value| value.as_str().expect("string item").to_owned())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn compare_values(
    golden_name: &str,
    frame_index: usize,
    target_kind: &str,
    target_name: &str,
    expected: &Value,
    got: &[f64],
    max_abs: &mut f64,
    max_rel: &mut f64,
) {
    let expected = expected.as_array().expect("snapshot vector");
    assert_eq!(
        expected.len(),
        got.len(),
        "{golden_name}: {target_kind} {target_name} vector length"
    );
    for (index, (expected, got)) in expected.iter().zip(got).enumerate() {
        // 任务 7 加固：expected 必须是数字；任何 null/字符串都直接失败，
        // 禁止 `unwrap_or(NaN)` 的静默跳过路径（NaN 哨兵即真实的输入损坏）。
        assert!(
            expected.is_number(),
            "{golden_name} frame {frame_index} {target_kind} `{target_name}`[{index}]: \
             golden 期望值必须是数字，实际 {expected}（任务 7：禁止 null→NaN 静默哨兵）"
        );
        let expected = expected.as_f64().unwrap_or_else(|| {
            panic!(
                "{golden_name} frame {frame_index} {target_kind} `{target_name}`[{index}]: \
                 JSON number 无法转成 f64（实际 {expected}）"
            )
        });
        assert!(
            expected.is_finite() && got.is_finite(),
            "{golden_name} frame {frame_index} {target_kind} `{target_name}`[{index}]: \
             期望/实际出现非有限值 want={expected} got={got}"
        );
        let difference = (expected - got).abs();
        *max_abs = max_abs.max(difference);
        *max_rel = max_rel.max(difference / (1.0 + expected.abs()));
        assert!(
            difference <= 1e-9 * (1.0 + expected.abs()),
            "{golden_name} frame {frame_index} {target_kind} `{target_name}`[{index}]: want={expected} got={got} Δ={difference}"
        );
    }
}

fn compare_events(golden_name: &str, frame_index: usize, frame: &Value, got: &[EventObject]) {
    let want: Vec<_> = frame["events"]
        .as_array()
        .expect("frame event list")
        .iter()
        .map(|event| {
            (
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
            (
                event.event_type.clone(),
                event.name.clone(),
                event.bone_name.clone(),
                event.slot_name.clone(),
            )
        })
        .collect();
    assert_eq!(
        actual, want,
        "{golden_name} frame {frame_index} dispatched events"
    );
}
