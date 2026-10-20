//! 阶段 4 animation 验收补课（任务 7「额外 animation tests」1/2/3/4 + D-4.36）。
//!
//! - `goto_and_stop_by_frame` 冻结与同帧 `goto_and_play_by_frame` 往返（任务 7 项 1）。
//! - `AnimationState.time_scale=2` 的 60 帧对 golden 120 帧奇数帧（任务 7 项 4 / 任务 9 §3）。
//! - 反向 `AnimationState.time_scale=-1` 的姿态逆序（任务 7 项 4）。
//! - `resetToPose` 未覆盖 bone/slot 的真实素材端到端用例（任务 7 项 2；模块内合成
//!   用例见 `animation_state.rs`）：
//!   `Dragon` 上 walk→stand 的骨复位 + 与"从零播放 stand"的整姿态对照，
//!   `PlantAnimate` 上 Shoot→Idle 的插槽 displayIndex 复位 + 关闭配置的残留对照。
//! - 子骨架同帧 state 创建（D-4.36）。
//!
//! 容差一律 `1e-9 * (1 + |want|)`；失败信息中文带动画/帧/目标上下文。

mod common;

use common::ir_snapshot::{asset_root, tex_stem};
use common::{golden_path, read_value};
use dragonbones_core::animation::StateId;
use dragonbones_core::armature::{ArmatureIdx, DisplayHandle, NullArmatureProxy, SlotIdx};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::transform::Transform;
use dragonbones_core::model::{
    ActionData, AnimationConfig, AnimationData, ArmatureData, DisplayData, DisplayKind,
};
use serde_json::Value;
use std::sync::Arc;

type BonePose = (String, [f64; 13]);
type SlotPose = (String, [f64; 15]);

fn bone_values(bone: &dragonbones_core::armature::Bone) -> [f64; 13] {
    let global = &bone.global;
    let matrix = &bone.global_transform_matrix;
    [
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
    ]
}

fn slot_values(slot: &dragonbones_core::armature::Slot) -> [f64; 15] {
    let color = &slot.color_transform;
    [
        f64::from(slot.display_index),
        f64::from(i32::from(slot.visible)),
        f64::from(slot.z_order),
        f64::from(slot.z_index),
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
    ]
}

fn pose_snapshot(factory: &BaseFactory, idx: ArmatureIdx) -> (Vec<BonePose>, Vec<SlotPose>) {
    let armature = factory.armature(idx);
    let bones = armature
        .bones
        .iter()
        .map(|bone| (bone.bone_name.clone(), bone_values(bone)))
        .collect();
    let slots = armature
        .slots
        .iter()
        .map(|slot| (slot.slot_name.clone(), slot_values(slot)))
        .collect();
    (bones, slots)
}

fn assert_pose_eq(
    context: &str,
    want: &(Vec<BonePose>, Vec<SlotPose>),
    got: &(Vec<BonePose>, Vec<SlotPose>),
) {
    assert_eq!(
        want.0.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        got.0.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "{context}: 骨名称/顺序不一致"
    );
    assert_eq!(
        want.1.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        got.1.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        "{context}: 槽名称/顺序不一致"
    );
    for ((name, want), (_, got)) in want.0.iter().zip(&got.0) {
        for (index, (want, got)) in want.iter().zip(got).enumerate() {
            assert!(
                (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                "{context} 骨 `{name}`[{index}]: want={want} got={got} Δ={}",
                (want - got).abs()
            );
        }
    }
    for ((name, want), (_, got)) in want.1.iter().zip(&got.1) {
        for (index, (want, got)) in want.iter().zip(got).enumerate() {
            assert!(
                (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                "{context} 槽 `{name}`[{index}]: want={want} got={got} Δ={}",
                (want - got).abs()
            );
        }
    }
}

/// 读 golden 并装配到 root clock；不注册任何事件探针（本文件不比较 events）。
fn build_golden_fixture(dir: &str, animation: &str) -> (BaseFactory, ArmatureIdx, Value) {
    let golden = read_value(&golden_path(dir, animation));
    let ske_file = golden["meta"]["skeFile"].as_str().expect("ske file");
    let ske_stem = ske_file.strip_suffix(".json").expect("ske suffix");
    let armature_name = golden["meta"]["armature"].as_str().expect("armature name");
    let ske = read_value(&asset_root().join(ske_file));
    let tex = read_value(&asset_root().join(format!("{}.json", tex_stem(ske_stem))));

    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, ske_stem)
        .unwrap_or_else(|| panic!("{dir}/{animation}: parse_dragon_bones_data 失败"));
    assert!(
        factory.parse_texture_atlas_data(&tex, ske_stem),
        "{dir}/{animation}: parse_texture_atlas_data 失败"
    );
    let idx = factory
        .build_armature(
            armature_name,
            ske_stem,
            "",
            ske_stem,
            Box::new(NullArmatureProxy::new()),
        )
        .unwrap_or_else(|| panic!("{dir}/{animation}: build_armature 失败"));
    factory
        .attach_armature(factory.root_clock(), idx)
        .unwrap_or_else(|error| panic!("{dir}/{animation}: 挂 root clock 失败：{error}"));
    (factory, idx, golden)
}

fn golden_dt(golden: &Value) -> f64 {
    golden["meta"]["dt"].as_f64().expect("golden dt")
}

/// 复现 jsdump 的逐帧累加顺序（保持 f64 舍入路径与 golden 一致）。
fn accumulated_time(steps: usize, dt: f64) -> f64 {
    let mut time = 0.0;
    for _ in 0..steps {
        time += dt;
    }
    time
}

fn assert_frame_matches_golden(
    factory: &BaseFactory,
    idx: ArmatureIdx,
    golden_name: &str,
    frame_index: usize,
    frame: &Value,
) {
    let armature = factory.armature(idx);
    for (name, want) in frame["bones"].as_object().expect("bone snapshots") {
        let bone_idx = armature
            .get_bone(name)
            .unwrap_or_else(|| panic!("{golden_name} frame {frame_index}: 缺少骨 `{name}`"));
        let bone = armature
            .bones
            .iter()
            .find(|bone| bone.idx == bone_idx)
            .expect("stable bone id");
        let got = bone_values(bone);
        for (index, (want, got)) in want
            .as_array()
            .expect("bone vector")
            .iter()
            .zip(got)
            .enumerate()
        {
            let want = want
                .as_f64()
                .unwrap_or_else(|| panic!("{golden_name} frame {frame_index}: 骨 `{name}` 非数字"));
            assert!(
                (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                "{golden_name} frame {frame_index} 骨 `{name}`[{index}]: want={want} got={got} Δ={}",
                (want - got).abs()
            );
        }
    }
    for (name, want) in frame["slots"].as_object().expect("slot snapshots") {
        let slot_idx = armature
            .get_slot(name)
            .unwrap_or_else(|| panic!("{golden_name} frame {frame_index}: 缺少槽 `{name}`"));
        let slot = armature
            .slots
            .iter()
            .find(|slot| slot.idx == slot_idx)
            .expect("stable slot id");
        let got = slot_values(slot);
        for (index, (want, got)) in want
            .as_array()
            .expect("slot vector")
            .iter()
            .zip(got)
            .enumerate()
        {
            let want = want
                .as_f64()
                .unwrap_or_else(|| panic!("{golden_name} frame {frame_index}: 槽 `{name}` 非数字"));
            assert!(
                (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                "{golden_name} frame {frame_index} 槽 `{name}`[{index}]: want={want} got={got} Δ={}",
                (want - got).abs()
            );
        }
    }
}

/// 任务 7 项 1：`gotoAndStopByFrame` 后时间推进姿态冻结；同帧
/// `gotoAndPlayByFrame` 往返后在相同 playhead 位置得到同一姿态。
#[test]
fn goto_and_stop_by_frame_freezes_pose_and_play_round_trip() {
    let (mut factory, idx, golden) = build_golden_fixture("Dragon", "walk");
    let dt = golden_dt(&golden);
    let frame = 6;

    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_stop_by_frame("walk", frame)
            .is_some(),
        "goto_and_stop_by_frame(walk, {frame}) 应成功"
    );
    // 第一帧驱动 timeline 重建并应用停止时刻的 pose；之后 5 帧必须逐位不变。
    factory.advance_time(dt);
    let frozen = pose_snapshot(&factory, idx);
    for tick in 0..5 {
        factory.advance_time(dt);
        let got = pose_snapshot(&factory, idx);
        assert_pose_eq(
            &format!("gotoAndStopByFrame(walk, {frame}) 第 {tick} 个冻结帧"),
            &frozen,
            &got,
        );
    }

    // 同一 playhead 位置重新 gotoAndPlay：用**一个零时间更新帧**（`advance_one(0)`）
    // 完成 timeline 重建而不推进 playhead，姿态必须与 stop 时一致。
    // 注意：若这里用 `advance_time(dt)`，playhead 会到 k+1 帧，姿态就不再是
    // stop(k) 的冻结姿态；任务 7 项 1 的「往返」按同一 playhead 位置验证。
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_frame("walk", frame, -1)
            .is_some(),
        "同帧 goto_and_play_by_frame(walk, {frame}) 应成功"
    );
    factory.advance_one(idx, 0.0);
    let round_trip = pose_snapshot(&factory, idx);
    assert_pose_eq(
        &format!("gotoAndPlayByFrame(walk, {frame}) 同帧往返"),
        &frozen,
        &round_trip,
    );
}

/// 任务 7 项 4：同一 golden，state `time_scale=2` 跑 60 帧，第 i 帧应等于
/// golden 第 `2i+1` 帧（jsdump 先推进后采样：golden 帧 i 时间 `(i+1)*dt`）。
///
/// 跳过 events：timeScale 会改变 loopComplete/帧事件的帧序号，但同动画时间点的
/// pose 仍必须一致。窗口只到 golden 第 119 帧，仍在 120 帧采集范围内。
///
/// **精确帧边界浮点豁免（不静默）**：`time_scale=2` 每帧累加 `2*dt`，而 golden
/// 逐帧累加 `dt`。两者数学时间相同，但 f64 加法顺序不同：A 3 次得到 `0.2`，
/// golden 6 次得到 `0.19999999999999998`，同一时刻因 `floor(time*frameRate)`
/// 落到相邻关键帧。因此当（且仅当）A 时间落在整数帧边界 `1e-12` 内、且与
/// golden 累加时间属于同一循环时刻时，本测试记录该帧为豁免帧并跳过逐位比较；
/// 非边界偏差、跨循环但非边界的时间差都会直接失败。豁免数量在测试输出中打印。
#[test]
fn state_time_scale_two_sixty_frames_match_golden_odd_frames() {
    let (mut factory, idx, golden) = build_golden_fixture("PlantAnimate", "Shoot");
    let dt = golden_dt(&golden);
    let state_id = factory
        .armature_mut(idx)
        .animation
        .goto_and_play_by_time("Shoot", 0.0, -1)
        .expect("play Shoot");
    factory
        .armature_mut(idx)
        .animation
        .state_mut(state_id)
        .expect("Shoot state")
        .time_scale = 2.0;

    let frames = golden["frames"].as_array().expect("golden frames");
    let frame_rate = golden["meta"]["frameRate"]
        .as_f64()
        .expect("golden frameRate");
    let mut boundary_exemptions = Vec::new();
    for frame in 0..60 {
        factory.advance_time(dt);
        // A 第 frame 帧时间 = (frame+1)*dt*2；golden 第 (2*frame+1) 帧时间 =
        // ((2*frame+1)+1)*dt = (2*frame+2)*dt，两者数学上相等。
        let a_state = factory
            .armature(idx)
            .animation
            .state(state_id)
            .expect("Shoot state");
        let a_time = a_state.current_time();
        let target = 2 * frame + 1;
        // golden 也是逐帧累加 dt（jsdump 循环），再用同样的动画时长 wrap；
        // 用累加值复现其量化边界与循环后的有效时间。
        let duration = a_state.duration;
        let target_time = accumulated_time(target + 1, dt).rem_euclid(duration);
        let a_bucket = (a_time * frame_rate).floor();
        let target_bucket = (target_time * frame_rate).floor();
        if a_bucket != target_bucket {
            // 浮点累加顺序差异：A 每帧累加 2*dt（3 次到 0.2），golden 累加 6 次
            // 得到 0.19999999999999998；同一数学时刻因 floor 落到相邻关键帧。
            // 这只允许发生在 A 时间恰为整数帧边界且与 golden 数学时间相等的帧；
            // 记录豁免帧并在测试末尾打印，不做静默放宽。
            let scaled = a_time * frame_rate;
            let on_integer_boundary = (scaled - scaled.round()).abs() <= 1e-12;
            assert!(
                on_integer_boundary,
                "timeScale=2 A帧{frame}: A时间 {a_time}（scaled={scaled}）不在整数帧边界附近，\
                 却与 golden 帧桶不一致（a_bucket={a_bucket}, target_bucket={target_bucket}），\
                 不是可豁免的边界量化"
            );
            let same_loop_boundary = (duration - a_time).abs() <= 1e-9 && target_time <= 1e-9
                || a_time <= 1e-9 && (duration - target_time).abs() <= 1e-9;
            assert!(
                (a_time - target_time).abs() <= 1e-9 || same_loop_boundary,
                "timeScale=2 A帧{frame}: A时间 {a_time} 与 golden 目标时间 {target_time} \
                 不是同一数学时刻（差 {}；duration={duration}）",
                (a_time - target_time).abs()
            );
            boundary_exemptions.push((frame, target, a_time, target_time));
            continue;
        }
        assert_frame_matches_golden(
            &factory,
            idx,
            &format!("PlantAnimate/Shoot timeScale=2 A帧{frame} → golden帧{target}"),
            target,
            &frames[target],
        );
    }
    // 不静默：明确记录本次运行因精确帧边界浮点量化而豁免的帧。
    assert!(
        boundary_exemptions.len() < 60,
        "timeScale=2 对拍不应全部豁免（实际 {} 帧）",
        boundary_exemptions.len()
    );
    if !boundary_exemptions.is_empty() {
        eprintln!(
            "timeScale=2 精确帧边界浮点豁免帧数={}（A帧, golden目标帧, A时间, golden累加时间）: {:?}",
            boundary_exemptions.len(),
            boundary_exemptions
        );
    }
}

/// 任务 7 项 4：反向 state timeScale。正向记录 fade 完成后的 5 帧姿态，再设
/// `time_scale=-1` 逐步回退，姿态序列必须与正向记录逆序一致（窗口避开 loop 边界）。
#[test]
fn reverse_state_time_scale_replays_pose_in_reverse_order() {
    let (mut factory, idx, golden) = build_golden_fixture("PlantAnimate", "Shoot");
    let dt = golden_dt(&golden);
    let state_id = factory
        .armature_mut(idx)
        .animation
        .goto_and_play_by_time("Shoot", 0.0, -1)
        .expect("play Shoot");

    // Shoot 时长约 0.867s；5 帧 (≈0.167s) 远离 loop 边界。
    let mut forward = Vec::new();
    for _ in 0..5 {
        factory.advance_time(dt);
        forward.push(pose_snapshot(&factory, idx));
    }
    factory
        .armature_mut(idx)
        .animation
        .state_mut(state_id)
        .expect("Shoot state")
        .time_scale = -1.0;

    for step in 0..4 {
        factory.advance_time(dt);
        let want = &forward[3 - step];
        let got = pose_snapshot(&factory, idx);
        assert_pose_eq(
            &format!(
                "timeScale=-1 第 {} 步（应回到正向第 {} 帧）",
                step + 1,
                4 - step
            ),
            want,
            &got,
        );
    }
}

const CHILD_ARMATURE: &str = "AcceptanceChild";
const PARENT_KEY: &str = "Dragon_ske";

/// D-4.36：父插槽子骨架的 animation 继承在父骨架同帧 advance 尾部创建；
/// `play()` 返回瞬间子状态尚未创建，但一次 `factory.advance_time(dt)` 后
/// 父/子都已在同帧推进（正常 arena 序父 idx < 子 idx）。
#[test]
fn child_animation_state_is_created_and_advanced_in_same_frame_as_parent_play() {
    let ske = read_value(&asset_root().join(format!("{PARENT_KEY}.json")));
    let tex = read_value(&asset_root().join(format!("{}.json", tex_stem(PARENT_KEY))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, PARENT_KEY)
        .expect("parse Dragon");
    assert!(factory.parse_texture_atlas_data(&tex, PARENT_KEY));
    inject_child_with_walk_animation(&mut factory);

    let parent = factory
        .build_armature(
            "Dragon",
            PARENT_KEY,
            "",
            PARENT_KEY,
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build Dragon");
    let (child, _slot) = find_child_slot(&factory, parent);
    let root = factory.root_clock();
    factory
        .attach_armature(root, parent)
        .expect("parent 挂 root clock");
    assert_eq!(
        factory.armature(child).clock(),
        Some(root),
        "子骨架应随父骨架继承 root clock"
    );

    assert!(
        factory
            .armature_mut(parent)
            .animation
            .play(Some("walk"), -1)
            .is_some(),
        "父骨架 play(walk) 应成功"
    );
    // D-4.36：请求在父 advance 尾部排空，play 返回时子状态尚未创建。
    assert!(
        factory
            .armature(child)
            .animation
            .get_state("walk", -1)
            .is_none(),
        "D-4.36：父 play() 返回瞬间子骨架不应已有 walk state（请求仅入队）"
    );

    factory.advance_time(1.0 / 24.0);
    let child_state = factory
        .armature(child)
        .animation
        .get_state("walk", -1)
        .unwrap_or_else(|| {
            panic!("D-4.36：父骨架同帧 advance 后子骨架应创建 walk state（inherit_animation=true）")
        });
    let current = factory
        .armature(child)
        .animation
        .state(child_state)
        .expect("child walk state")
        .current_time();
    assert!(
        current > 0.0,
        "D-4.36：子骨架应在父同帧内被推进，current_time 应 > 0，实际 {current}"
    );
}

/// 任务 7 项 3：子骨架 inheritAnimation 的父子时间缩放组合顺序——
/// `child._inheritTimeScale = parent._inheritTimeScale * child.timeScale`。
/// 单层亲子场景验证：父 2.0 与子 0.5 组合成净 1.0（父缩放只乘一次，子自身
/// timeScale 再叠加），子改回 1.0 后完整继承父的 2.0。
#[test]
fn child_animation_composes_inherited_parent_time_scale_once() {
    let ske = read_value(&asset_root().join(format!("{PARENT_KEY}.json")));
    let tex = read_value(&asset_root().join(format!("{}.json", tex_stem(PARENT_KEY))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, PARENT_KEY)
        .expect("parse Dragon");
    assert!(factory.parse_texture_atlas_data(&tex, PARENT_KEY));
    inject_child_with_walk_animation(&mut factory);

    let parent = factory
        .build_armature(
            "Dragon",
            PARENT_KEY,
            "",
            PARENT_KEY,
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build Dragon");
    let (child, _slot) = find_child_slot(&factory, parent);
    factory
        .attach_armature(factory.root_clock(), parent)
        .expect("parent 挂 root clock");

    // 关闭 fade 暂停与淡入，让 playhead 每帧严格前进 dt，便于比较缩放增量。
    let mut config = AnimationConfig {
        name: "walk".to_owned(),
        animation: "walk".to_owned(),
        pause_fade_in: false,
        fade_in_time: 0.0,
        ..AnimationConfig::default()
    };
    assert!(
        factory
            .armature_mut(parent)
            .animation
            .play_config(&mut config)
            .is_some(),
        "parent play_config(walk) 应成功"
    );
    let dt = 1.0 / 24.0;
    // 子状态在本帧尾部创建，并在同一次 root-clock 派发内被推进（D-4.36 / arena 序）。
    factory.advance_time(dt);

    let parent_state = factory
        .armature(parent)
        .animation
        .get_state("walk", -1)
        .expect("parent walk state");
    let child_state = factory
        .armature(child)
        .animation
        .get_state("walk", -1)
        .expect("child walk state");
    let state_time = |factory: &BaseFactory, armature: ArmatureIdx, state: StateId| {
        factory
            .armature(armature)
            .animation
            .state(state)
            .expect("state alive")
            .current_time()
    };
    let parent_t0 = state_time(&factory, parent, parent_state);
    let child_t0 = state_time(&factory, child, child_state);
    assert!(
        parent_t0 > 0.0 && child_t0 > 0.0,
        "父子 walk 状态都应已推进：parent={parent_t0} child={child_t0}"
    );

    // 父 2.0 × 子 0.5 → 子继承 1.0：父缩放只经 parent._inheritTimeScale 乘一次。
    factory.armature_mut(parent).animation.time_scale = 2.0;
    factory.armature_mut(child).animation.time_scale = 0.5;
    factory.advance_time(dt);
    let parent_delta = state_time(&factory, parent, parent_state) - parent_t0;
    let child_delta = state_time(&factory, child, child_state) - child_t0;
    assert!(
        (parent_delta - 2.0 * dt).abs() <= 1e-12,
        "父 timeScale=2 时每帧应推进 2*dt，实际 Δ={parent_delta}"
    );
    assert!(
        (child_delta - dt).abs() <= 1e-12,
        "子应继承 2.0*0.5=1.0 的净缩放，每帧推进 dt，实际 Δ={child_delta}"
    );

    // 子自身改回 1.0 → 只继承父的 2.0。
    let parent_t1 = state_time(&factory, parent, parent_state);
    let child_t1 = state_time(&factory, child, child_state);
    factory.armature_mut(child).animation.time_scale = 1.0;
    factory.advance_time(dt);
    let parent_delta = state_time(&factory, parent, parent_state) - parent_t1;
    let child_delta = state_time(&factory, child, child_state) - child_t1;
    assert!(
        (parent_delta - 2.0 * dt).abs() <= 1e-12,
        "父 timeScale=2 时每帧应推进 2*dt，实际 Δ={parent_delta}"
    );
    assert!(
        (child_delta - 2.0 * dt).abs() <= 1e-12,
        "子 timeScale=1 时应继承父的 2.0，每帧推进 2*dt，实际 Δ={child_delta}"
    );
}

fn inject_child_with_walk_animation(factory: &mut BaseFactory) {
    let arc = factory
        .data_map
        .get_mut(PARENT_KEY)
        .unwrap_or_else(|| panic!("data_map 中无 `{PARENT_KEY}`"));
    let data = Arc::get_mut(arc)
        .unwrap_or_else(|| panic!("DragonBonesData 的 Arc 非独占，无法注入子骨架"));
    let mut child = ArmatureData {
        name: CHILD_ARMATURE.to_owned(),
        frame_rate: 24.0,
        ..ArmatureData::default()
    };
    child.add_animation(AnimationData {
        name: "idle".to_owned(),
        duration: 1.0,
        frame_count: 24,
        ..AnimationData::default()
    });
    child.add_animation(AnimationData {
        name: "walk".to_owned(),
        duration: 1.0,
        frame_count: 24,
        ..AnimationData::default()
    });
    data.add_armature(child);

    let armature_data = data.armatures.get_mut("Dragon").expect("Dragon armature");
    let skin_name = armature_data.default_skin.clone().expect("default skin");
    let skin = armature_data.skins.get_mut(&skin_name).expect("skin");
    let (slot_name, display_index) = {
        let mut found = None;
        for name in armature_data.sorted_slots.clone() {
            let Some(slot_data) = armature_data.slots.get(&name) else {
                continue;
            };
            let Some(displays) = skin.displays.get(&name) else {
                continue;
            };
            let Ok(index) = usize::try_from(slot_data.display_index) else {
                continue;
            };
            if index < displays.len() {
                found = Some((name, index));
                break;
            }
        }
        found.expect("可注入子骨架显示的槽")
    };
    let displays = skin.displays.get_mut(&slot_name).expect("slot displays");
    displays[display_index] = Some(DisplayData {
        name: CHILD_ARMATURE.to_owned(),
        path: CHILD_ARMATURE.to_owned(),
        transform: Transform::default(),
        parent: skin_name,
        kind: DisplayKind::Armature {
            inherit_animation: true,
            actions: Vec::<ActionData>::new(),
            armature: Some(CHILD_ARMATURE.to_owned()),
        },
    });
}

fn find_child_slot(factory: &BaseFactory, parent: ArmatureIdx) -> (ArmatureIdx, SlotIdx) {
    for slot in factory.armature(parent).slots.iter() {
        if let Some(DisplayHandle::Armature(child)) = slot.display {
            return (child, slot.idx);
        }
    }
    panic!("父骨架没有 display 为 Armature 的槽（子骨架接线失败）");
}

/// S4-Event / F4.9：Sound action 不受 listener 闸门影响，且会出现在
/// `take_flushed_events()` 的本帧快照中（不是只验证 action 轴内部缓冲）。
#[test]
fn sound_action_appears_in_flushed_event_snapshot_without_listener() {
    let ske = serde_json::json!({
        "version": "5.6",
        "name": "Sfx",
        "frameRate": 24.0,
        "armature": [{
            "name": "Sfx",
            "frameRate": 24.0,
            "animation": [{
                "name": "sfx",
                "duration": 24,
                "frame": [{ "duration": 24, "sound": [{ "name": "boom" }] }]
            }]
        }]
    });
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, "sfx")
        .unwrap_or_else(|| panic!("最小 Sound action ske 应解析成功"));
    let idx = factory
        .build_armature("Sfx", "sfx", "", "", Box::new(NullArmatureProxy::new()))
        .unwrap_or_else(|| panic!("最小 Sound action 骨架应 build 成功"));
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("挂 root clock");
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time("sfx", 0.0, -1)
            .is_some(),
        "sfx 动画应存在"
    );

    // 未注册任何 listener：Frame/lifecycle 会被 gate，但 Sound 必须入队。
    let mut sound_events = Vec::new();
    for _ in 0..4 {
        factory.advance_time(1.0 / 24.0);
        sound_events.extend(
            factory.take_flushed_events().into_iter().filter(|event| {
                event.event_type == dragonbones_core::event::EventObject::SOUND_EVENT
            }),
        );
    }
    assert_eq!(
        sound_events.len(),
        1,
        "无 listener 时 Sound action 仍应恰好 flush 出 1 条 soundEvent，实际 {sound_events:?}"
    );
    assert_eq!(
        sound_events[0].name, "boom",
        "soundEvent.name 应为 action 名 `boom`"
    );
}

/// 任务 7 项 4 的确定性对照：同样 60 个逻辑帧、state `time_scale=2`，但把每帧的
/// `dt` 拆成两次 `dt/2` advance。`time_scale=2` 下每次只累加 `dt`，与 golden 的
/// 120 次 `dt` 累加顺序完全一致，因此 60 帧可 0 豁免逐位对拍 golden 第 `2i+1` 帧。
/// 该测试隔离并证明上面的边界豁免只来自 f64 加法顺序，而非动画数值实现偏差。
#[test]
fn state_time_scale_two_half_step_control_matches_all_golden_odd_frames() {
    let (mut factory, idx, golden) = build_golden_fixture("PlantAnimate", "Shoot");
    let dt = golden_dt(&golden);
    let state_id = factory
        .armature_mut(idx)
        .animation
        .goto_and_play_by_time("Shoot", 0.0, -1)
        .expect("play Shoot");
    factory
        .armature_mut(idx)
        .animation
        .state_mut(state_id)
        .expect("Shoot state")
        .time_scale = 2.0;

    let frames = golden["frames"].as_array().expect("golden frames");
    for frame in 0..60 {
        factory.advance_time(dt / 2.0);
        factory.advance_time(dt / 2.0);
        let golden_index = 2 * frame + 1;
        assert_frame_matches_golden(
            &factory,
            idx,
            &format!("PlantAnimate/Shoot timeScale=2 半步对照 A帧{frame} → golden帧{golden_index}"),
            golden_index,
            &frames[golden_index],
        );
    }
}

/// 任务 7 项 2（真实素材端到端）：`resetToPose` 把"上一动画覆盖、目标动画不覆盖"
/// 的骨复位到数据姿态，且"目标动画覆盖的骨"与从零播放同一动画的对照骨架逐位一致
/// ——证明 resetToPose 只补未覆盖目标，不改变被覆盖目标的动画数值。
#[test]
fn reset_to_pose_restores_uncovered_bones_after_real_animation_switch() {
    let (mut played, idx, golden) = build_golden_fixture("Dragon", "walk");
    let dt = golden_dt(&golden);

    // 覆盖事实（`assets/Dragon_ske.json`）：walk 动 18 根骨，stand 只动 8 根，
    // `body` 属于"walk 覆盖、stand 不覆盖"。
    let (walk_covers_body, stand_covers_body) = {
        let data = played.armature(idx).armature_data();
        (
            data.animations
                .get("walk")
                .expect("walk data")
                .get_bone_timelines("body")
                .is_some(),
            data.animations
                .get("stand")
                .expect("stand data")
                .get_bone_timelines("body")
                .is_some(),
        )
    };
    assert!(walk_covers_body, "walk 应覆盖 `body` 骨");
    assert!(!stand_covers_body, "stand 不应覆盖 `body` 骨");

    let body_pose = |factory: &BaseFactory, idx: ArmatureIdx| {
        let body = factory
            .armature(idx)
            .get_bone("body")
            .expect("`body` 骨存在");
        factory
            .armature(idx)
            .bones
            .iter()
            .find(|bone| bone.idx == body)
            .expect("`body` 骨索引有效")
            .animation_pose
    };
    assert_eq!(
        body_pose(&played, idx),
        Transform::default(),
        "初始动画姿态应为数据姿态"
    );

    // 1) 播 walk，直到 `body` 留下非零动画姿态残留。
    played
        .armature_mut(idx)
        .animation
        .play(Some("walk"), -1)
        .expect("play(walk) 应成功");
    let mut residual = Transform::default();
    for _ in 0..24 {
        played.advance_time(dt);
        residual = body_pose(&played, idx);
        if residual != Transform::default() {
            break;
        }
    }
    assert_ne!(
        residual,
        Transform::default(),
        "walk 播放后 `body` 的动画姿态应非零（否则本用例无法证明复位）"
    );

    // 2) 切到不覆盖 `body` 的 stand，开启 resetToPose，淡入 0.2s。
    //    `pause_fade_in` 默认 true 会暂停淡入期间的 playhead，使切换后的状态时间
    //    滞后于从零播放的对照骨架；对照用例显式关闭，只比较同 playhead 位置的姿态。
    let play_stand = |factory: &mut BaseFactory, idx: ArmatureIdx| {
        let mut config = AnimationConfig {
            name: "stand".to_owned(),
            animation: "stand".to_owned(),
            reset_to_pose: true,
            pause_fade_in: false,
            fade_in_time: 0.2,
            ..AnimationConfig::default()
        };
        assert!(
            factory
                .armature_mut(idx)
                .animation
                .play_config(&mut config)
                .is_some(),
            "play_config(stand) 应成功"
        );
    };
    play_stand(&mut played, idx);
    // 12 帧 = 0.5s：覆盖 0.2s 淡入并留出 pose 轴移除后的余量。
    for _ in 0..12 {
        played.advance_time(dt);
    }
    assert_eq!(
        body_pose(&played, idx),
        Transform::default(),
        "resetToPose 应把未被 stand 覆盖的 `body` 复位到数据姿态"
    );

    // 3) 对照：同一素材从零播放 stand 相同帧数。覆盖骨（tail/clothes/eyes/…）的
    //    动画数值必须与切换后的骨架逐位一致，未覆盖骨在两边都是数据姿态。
    let (mut control, control_idx, _) = build_golden_fixture("Dragon", "walk");
    play_stand(&mut control, control_idx);
    for _ in 0..12 {
        control.advance_time(dt);
    }
    assert_pose_eq(
        "resetToPose walk→stand 与从零播放 stand 的整姿态对照",
        &pose_snapshot(&control, control_idx),
        &pose_snapshot(&played, idx),
    );
}

/// 任务 7 项 2（真实素材端到端）：`resetToPose` 把上一动画改过的插槽 displayIndex
/// 复位到数据姿态；关闭该配置的对照骨架保留残留，证明复位确由 `resetToPose` 驱动。
#[test]
fn reset_to_pose_restores_uncovered_slot_displays_after_real_animation_switch() {
    let (mut played, idx, golden) = build_golden_fixture("PlantAnimate", "Shoot");
    let dt = golden_dt(&golden);

    // 覆盖事实（`assets/PlantAnimate_ske.json`）：Shoot 有 10 条 slot 时间轴，
    // Idle 一条都没有 → 切到 Idle 时所有槽目标都未被覆盖。
    let (shoot_slot_timelines, idle_slot_timelines, data_indices) = {
        let data = played.armature(idx).armature_data();
        (
            data.animations
                .get("Shoot")
                .expect("Shoot data")
                .slot_timelines
                .len(),
            data.animations
                .get("Idle")
                .expect("Idle data")
                .slot_timelines
                .len(),
            data.slots
                .iter()
                .map(|(name, slot)| (name.clone(), slot.display_index))
                .collect::<Vec<_>>(),
        )
    };
    assert!(shoot_slot_timelines > 0, "Shoot 应有 slot 时间轴");
    assert_eq!(
        idle_slot_timelines, 0,
        "Idle 不应有 slot 时间轴（槽目标全部未被覆盖）"
    );

    let deviating_slots = |factory: &BaseFactory, idx: ArmatureIdx| {
        factory
            .armature(idx)
            .slots
            .iter()
            .filter_map(|slot| {
                let (_, data_index) = data_indices
                    .iter()
                    .find(|(name, _)| name == &slot.slot_name)?;
                (slot.display_index != *data_index).then_some(slot.slot_name.clone())
            })
            .collect::<Vec<_>>()
    };
    let play_shoot = |factory: &mut BaseFactory, idx: ArmatureIdx| {
        assert!(
            factory
                .armature_mut(idx)
                .animation
                .play(Some("Shoot"), -1)
                .is_some(),
            "play(Shoot) 应成功"
        );
    };
    let play_idle = |factory: &mut BaseFactory, idx: ArmatureIdx, reset_to_pose: bool| {
        let mut config = AnimationConfig {
            name: "Idle".to_owned(),
            animation: "Idle".to_owned(),
            reset_to_pose,
            fade_in_time: 0.2,
            ..AnimationConfig::default()
        };
        assert!(
            factory
                .armature_mut(idx)
                .animation
                .play_config(&mut config)
                .is_some(),
            "play_config(Idle) 应成功"
        );
    };

    // 1) 播 Shoot，直到出现 displayIndex 偏离数据姿态的槽。
    play_shoot(&mut played, idx);
    let mut changed = Vec::new();
    for _ in 0..24 {
        played.advance_time(dt);
        changed = deviating_slots(&played, idx);
        if !changed.is_empty() {
            break;
        }
    }
    assert!(
        !changed.is_empty(),
        "Shoot 播放后应有槽 displayIndex 偏离数据姿态（否则无法证明复位）"
    );

    // 2) 切到不覆盖任何槽的 Idle，开启 resetToPose：全部槽必须回到数据姿态。
    play_idle(&mut played, idx, true);
    for _ in 0..12 {
        played.advance_time(dt);
    }
    for slot in played.armature(idx).slots.iter() {
        let (_, data_index) = data_indices
            .iter()
            .find(|(name, _)| name == &slot.slot_name)
            .unwrap_or_else(|| panic!("槽 `{}` 缺少数据", slot.slot_name));
        assert_eq!(
            slot.display_index, *data_index,
            "resetToPose 后槽 `{}` 的 displayIndex 应复位到数据姿态（Shoot 残留：{:?}）",
            slot.slot_name, changed
        );
    }

    // 3) 对照：同样 Shoot→Idle，但 reset_to_pose=false → 残留必须保留。
    let (mut kept, kept_idx, _) = build_golden_fixture("PlantAnimate", "Shoot");
    play_shoot(&mut kept, kept_idx);
    let mut kept_changed = Vec::new();
    for _ in 0..24 {
        kept.advance_time(dt);
        kept_changed = deviating_slots(&kept, kept_idx);
        if !kept_changed.is_empty() {
            break;
        }
    }
    assert!(
        !kept_changed.is_empty(),
        "对照组 Shoot 播放后应有槽 displayIndex 偏离数据姿态"
    );
    play_idle(&mut kept, kept_idx, false);
    for _ in 0..12 {
        kept.advance_time(dt);
    }
    assert!(
        !deviating_slots(&kept, kept_idx).is_empty(),
        "reset_to_pose=false 时 Idle 不覆盖槽目标，Shoot 的 displayIndex 残留应保留"
    );
}
