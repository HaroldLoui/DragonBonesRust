//! Core 真实素材压测（docs/optimization-roadmap.md 之外的「方案 A」）。
//!
//! 默认 `#[ignore]`，因为语料放在 gitignored 的 `_research/` 下、且依赖 Node 与
//! `DragonBonesJS/` 子模块做 JS 对拍。用法：
//!
//! ```text
//! cargo test -p dragonbones-core --test corpus_audit -- --ignored --nocapture
//! # 自定义语料（`;` 分隔）/ 帧数 / JS 对拍抽样数：
//! DB_CORPUS_DIRS="D:/corpus/a;D:/corpus/b" DB_CORPUS_FRAMES=120 \
//!   DB_CORPUS_PARITY=40 cargo test -p dragonbones-core --test corpus_audit -- --ignored --nocapture
//! ```
//!
//! 环境变量：
//! - `DB_CORPUS_DIRS`：语料根目录（递归查找 `*_ske.json`），`;` 分隔；缺省
//!   `_research/mods/rigs;_research/recovered/rigs`（存在时）。
//! - `DB_CORPUS_FRAMES`：每个动画推进的帧数，缺省 120。
//! - `DB_CORPUS_PARITY`：JS 对拍抽样数（0 = 关闭），缺省 40。
//! - `DB_CORPUS_LIMIT`：只审计前 N 个 rig（调试用，0 = 全部）。
//! - `DB_CORPUS_REPORT`：报告输出路径，缺省 `target/corpus_audit/report.md`。
//!
//! 审计内容分三层：
//! 1. **解析**：`parse_dragon_bones_data` / `parse_texture_atlas_data` 成功，且
//!    `sortedBones/sortedSlots/defaultAnimation/defaultSkin` 等结构自洽；
//! 2. **运行时**：每个 armature 都能 `build_armature`；首个 armature 的每个动画
//!    各 `goto_and_play_by_time` + 推进 N 帧，逐帧检查骨骼/插槽矩阵与 alpha 有限、
//!    动画状态时间有限（NaN/Inf 即失败）；整个 rig 包在 `catch_unwind` 里，panic
//!    归类为 `panic` 而不是中断整批；
//! 3. **JS 对拍（抽样）**：`tools/irdump.mjs` 跑官方 JS 运行时，逐值比较
//!    `meta` / `structure` / `atlas` 快照与 8 组 IR 数组（float 按 f32 位比较）。
//!
//! 失败分类写进 `target/corpus_audit/report.md`，方便逐类修。

mod common;

use std::any::Any;
use std::env;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use common::ir_snapshot::{norm, snapshot_atlas, snapshot_meta, snapshot_structure};
use dragonbones_core::armature::{Armature, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::model::dragon_bones_data::DragonBonesData;
use dragonbones_core::model::texture_atlas_data::TextureAtlasData;
use serde_json::Value;

/// 每帧步长 = 60fps。
const DT: f64 = 1.0 / 60.0;
const DEFAULT_FRAMES: usize = 120;
const DEFAULT_PARITY: usize = 40;
/// 单 rig 最多推进的动画数（个别素材有几十个动画，防御性上限）。
const MAX_ANIMATIONS_PER_RIG: usize = 64;
/// 单 rig 最多收集的失败条目（防止报告被同一个 rig 刷屏）。
const MAX_ERRORS_PER_RIG: usize = 40;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn corpus_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("DB_CORPUS_DIRS") {
        let dirs: Vec<PathBuf> = raw
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }

    let root = repo_root();
    let mut dirs = Vec::new();
    for rel in ["_research/mods/rigs", "_research/recovered/rigs", "assets"] {
        let path = root.join(rel);
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs
}

/// 递归收集 `*_ske.json`。
fn collect_skeletons(dirs: &[PathBuf], out: &mut Vec<PathBuf>) {
    for dir in dirs {
        walk(dir, out);
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_ske.json"))
        {
            out.push(path);
        }
    }
}

/// `<X>_ske.json` → 同目录 `<X>_tex.json`。
fn tex_path_for(ske: &Path) -> Option<PathBuf> {
    let stem = ske.file_stem()?.to_str()?;
    let tex_stem = stem.strip_suffix("_ske")?;
    let tex = ske.with_file_name(format!("{tex_stem}_tex.json"));
    tex.is_file().then_some(tex)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("JSON {}: {e}", path.display()))
}

#[derive(Debug)]
enum JsStatus {
    NotRun,
    Match,
    Diff(Vec<String>),
    Skip(String),
}

#[derive(Debug)]
struct RigResult {
    /// 相对语料根的短名（报告用）。
    name: String,
    armatures: usize,
    animations: usize,
    frames: usize,
    errors: Vec<String>,
    notes: Vec<String>,
    js: JsStatus,
}

impl RigResult {
    fn new(name: String) -> Self {
        Self {
            name,
            armatures: 0,
            animations: 0,
            frames: 0,
            errors: Vec::new(),
            notes: Vec::new(),
            js: JsStatus::NotRun,
        }
    }

    fn fail(&mut self, category: &str, detail: impl AsRef<str>) {
        if self.errors.len() < MAX_ERRORS_PER_RIG {
            self.errors.push(format!("{category}: {}", detail.as_ref()));
        } else if self.errors.len() == MAX_ERRORS_PER_RIG {
            self.errors.push(format!(
                "{category}: …（超过 {MAX_ERRORS_PER_RIG} 条，已截断）"
            ));
        }
    }
}

/// 每个 rig 的完整审计。内部仍可能 panic（故意不拦），由外层 `catch_unwind` 归类。
fn audit_rig(ske_path: &Path, frames: usize) -> RigResult {
    let name = ske_path.file_stem().map_or_else(
        || ske_path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    let mut result = RigResult::new(name.clone());

    // ---- 1. 原始 JSON + 解析 ----
    let ske_raw = match read_json(ske_path) {
        Ok(value) => value,
        Err(error) => {
            result.fail("parse_fail", error);
            return result;
        }
    };
    let tex_path = tex_path_for(ske_path);
    let tex_raw = match tex_path.as_deref().map(read_json) {
        Some(Ok(value)) => Some(value),
        Some(Err(error)) => {
            result.fail("tex_parse_fail", error);
            None
        }
        None => {
            result.notes.push("no_tex".to_string());
            None
        }
    };

    let key = name.strip_suffix("_ske").unwrap_or(&name).to_string();
    let mut factory = BaseFactory::new();
    let Some(data) = factory.parse_dragon_bones_data(&ske_raw, &key) else {
        result.fail("parse_fail", "parse_dragon_bones_data 返回 None");
        return result;
    };
    if let Some(raw) = &tex_raw {
        let parsed = factory.parse_texture_atlas_data(raw, &key);
        if !parsed {
            result.fail("atlas_fail", "parse_texture_atlas_data 返回 false");
        }
    }
    let atlas = factory
        .atlas_map
        .get(&key)
        .and_then(|list| list.first())
        .cloned();

    // ---- 2. 结构自检 ----
    structural_checks(&data, &mut result);
    if data.armature_names.is_empty() {
        return result;
    }
    result.armatures = data.armature_names.len();
    let first_arm = data.armature_names[0].clone();

    // ---- 3. 全 armature 构建（只验证 build，不推进） ----
    for armature_name in &data.armature_names {
        match factory.build_armature(
            armature_name,
            &key,
            "",
            &key,
            Box::new(NullArmatureProxy::new()),
        ) {
            Some(idx) => factory.dispose_armature(idx),
            None => result.fail("build_fail", format!("armature `{armature_name}` 构建失败")),
        }
    }
    factory.advance_time(0.0); // flush 掉上一轮 dispose 的回收

    // ---- 4. 首个 armature 的逐动画推进 ----
    let animations = data
        .armatures
        .get(&first_arm)
        .map(|armature| armature.animation_names.clone())
        .unwrap_or_default();
    if animations.is_empty() {
        result.notes.push("no_animation".to_string());
    }
    let animations: Vec<String> = animations
        .into_iter()
        .take(MAX_ANIMATIONS_PER_RIG)
        .collect();
    result.animations = animations.len();

    for animation in &animations {
        let Some(idx) = factory.build_armature(
            &first_arm,
            &key,
            "",
            &key,
            Box::new(NullArmatureProxy::new()),
        ) else {
            result.fail("build_fail", format!("{first_arm}/`{animation}` 重建失败"));
            continue;
        };
        if let Err(error) = factory.attach_armature(factory.root_clock(), idx) {
            result.fail("attach_fail", error.to_string());
        }
        let played = factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time(animation, 0.0, -1)
            .is_some();
        if !played {
            result.fail("animation_missing", format!("`{animation}` 播放失败"));
            let _ = factory.detach_armature(idx);
            factory.dispose_armature(idx);
            factory.advance_time(0.0);
            continue;
        }

        let mut stopped = false;
        for frame in 0..frames {
            factory.advance_time(DT);
            result.frames += 1;
            let armature = factory.armature(idx);
            if !check_finite(armature, &mut result, animation, frame) {
                stopped = true;
                break;
            }
        }

        let _ = factory.detach_armature(idx);
        factory.dispose_armature(idx);
        factory.advance_time(0.0);
        if stopped {
            break;
        }
    }

    // ---- 5. JS 对拍数据留给调用方（抽样阶段再跑 Node） ----
    // 这里只记录 report 需要的 armature 数等。
    result.notes.push(format!("atlas={}", atlas.is_some()));
    result
}

fn structural_checks(data: &DragonBonesData, result: &mut RigResult) {
    for armature_name in &data.armature_names {
        let Some(armature) = data.armatures.get(armature_name) else {
            result.fail(
                "structure",
                format!("armatureNames 含 `{armature_name}` 但 armatures 缺失"),
            );
            continue;
        };
        if armature.sorted_bones.len() != armature.bones.len() {
            result.fail(
                "structure",
                format!(
                    "`{armature_name}` sortedBones={} bones={}",
                    armature.sorted_bones.len(),
                    armature.bones.len()
                ),
            );
        }
        if armature.sorted_slots.len() != armature.slots.len() {
            result.fail(
                "structure",
                format!(
                    "`{armature_name}` sortedSlots={} slots={}",
                    armature.sorted_slots.len(),
                    armature.slots.len()
                ),
            );
        }
        if let Some(default_animation) = &armature.default_animation
            && !armature.animation_names.contains(default_animation)
        {
            result.fail(
                "structure",
                format!(
                    "`{armature_name}` defaultAnimation=`{default_animation}` 不在 animationNames"
                ),
            );
        }
        if let Some(default_skin) = &armature.default_skin
            && !armature.skins.contains_key(default_skin)
        {
            result.fail(
                "structure",
                format!("`{armature_name}` defaultSkin=`{default_skin}` 不在 skins"),
            );
        }
    }
}

/// 逐帧数值检查；返回 `false` 表示发现失败、应停止推进该动画。
fn check_finite(
    armature: &Armature,
    result: &mut RigResult,
    animation: &str,
    frame: usize,
) -> bool {
    let mut ok = true;
    for bone in armature.get_bones() {
        let matrix = &bone.base.global_transform_matrix;
        let finite_matrix = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
            .iter()
            .all(|value| value.is_finite());
        let transform = &bone.base.global;
        let finite_transform = [
            transform.x,
            transform.y,
            transform.skew,
            transform.rotation,
            transform.scale_x,
            transform.scale_y,
        ]
        .iter()
        .all(|value| value.is_finite());
        if !(finite_matrix
            && finite_transform
            && bone.base.global_alpha.is_finite()
            && bone.base.alpha.is_finite())
        {
            result.fail(
                "non_finite",
                format!(
                    "`{animation}` frame {frame}: 骨骼 `{}` 矩阵/变换/alpha 出现 NaN/Inf",
                    bone.bone_name
                ),
            );
            ok = false;
        }
    }
    for slot in armature.get_slots() {
        let matrix = &slot.base.global_transform_matrix;
        let finite_matrix = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
            .iter()
            .all(|value| value.is_finite());
        if !(finite_matrix && slot.base.global_alpha.is_finite() && slot.base.alpha.is_finite()) {
            result.fail(
                "non_finite",
                format!(
                    "`{animation}` frame {frame}: 插槽 `{}` 矩阵/alpha 出现 NaN/Inf",
                    slot.slot_name
                ),
            );
            ok = false;
        }
    }
    for (_, state) in armature.animation.states() {
        if !(state.current_time().is_finite() && state.total_time().is_finite()) {
            result.fail(
                "non_finite",
                format!(
                    "`{animation}` frame {frame}: 状态 `{}` current_time/total_time 非有限",
                    state.name
                ),
            );
            ok = false;
        }
    }
    ok
}

// ---------------------------------------------------------------------------
// JS 对拍
// ---------------------------------------------------------------------------

/// 跑 `tools/irdump.mjs` 并与 Rust 快照逐值比较。
fn js_parity(
    ske_path: &Path,
    tex_path: &Path,
    data: &DragonBonesData,
    atlas: Option<&TextureAtlasData>,
    out_dir: &Path,
) -> JsStatus {
    let stem = ske_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let js_out = out_dir.join(format!("{stem}.json"));
    let _ = fs::create_dir_all(out_dir);

    let irdump = repo_root().join("tools/irdump.mjs");
    let ske_file = ske_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let output = Command::new("node")
        .arg(&irdump)
        .arg(ske_path)
        .arg(tex_path)
        .arg(data.armature_names.first().map_or("", String::as_str))
        .arg(&js_out)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => return JsStatus::Skip(format!("无法启动 node: {error}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().next().unwrap_or("").trim();
        return JsStatus::Skip(format!("irdump 退出码 {:?}: {first}", output.status.code()));
    }
    let Ok(golden) = read_json(&js_out) else {
        return JsStatus::Skip("irdump 输出不可读".to_string());
    };

    let mut diffs = Vec::new();
    compare_snapshot(
        "meta",
        &norm(snapshot_meta(data, &ske_file)),
        &norm(golden["meta"].clone()),
        &mut diffs,
    );
    compare_snapshot(
        "structure",
        &norm(snapshot_structure(data)),
        &norm(golden["structure"].clone()),
        &mut diffs,
    );
    if let Some(atlas) = atlas {
        compare_snapshot(
            "atlas",
            &norm(snapshot_atlas(atlas)),
            &norm(golden["atlas"].clone()),
            &mut diffs,
        );
    }
    compare_arrays(data, &golden["arrays"], &mut diffs);

    if diffs.is_empty() {
        JsStatus::Match
    } else {
        JsStatus::Diff(diffs)
    }
}

/// 递归找第一处差异（最多 10 条）。
fn compare_snapshot(path: &str, got: &Value, want: &Value, diffs: &mut Vec<String>) {
    if diffs.len() >= 10 {
        return;
    }
    match (got, want) {
        (Value::Object(got), Value::Object(want)) => {
            for (key, got_value) in got {
                match want.get(key) {
                    Some(want_value) => {
                        compare_snapshot(&format!("{path}.{key}"), got_value, want_value, diffs);
                    }
                    None => diffs.push(format!("{path}.{key}: 仅 Rust 有")),
                }
            }
            for key in want.keys() {
                if !got.contains_key(key) {
                    diffs.push(format!("{path}.{key}: 仅 JS 有"));
                }
            }
        }
        (Value::Array(got), Value::Array(want)) => {
            if got.len() != want.len() {
                diffs.push(format!("{path}: 长度 Rust={} JS={}", got.len(), want.len()));
                return;
            }
            for (index, (got_value, want_value)) in got.iter().zip(want).enumerate() {
                compare_snapshot(&format!("{path}[{index}]"), got_value, want_value, diffs);
            }
        }
        (got, want) if got == want => {}
        (got, want) => diffs.push(format!("{path}: Rust={got} JS={want}")),
    }
}

fn compare_arrays(data: &DragonBonesData, golden: &Value, diffs: &mut Vec<String>) {
    let Some(root) = data.root.as_ref() else {
        diffs.push("arrays: Rust 无 root（finalize_root 未执行）".to_string());
        return;
    };
    let ir = &root.ir;
    compare_int_array("intArray", &ir.int_array, &golden["intArray"], diffs);
    compare_float_array("floatArray", &ir.float_array, &golden["floatArray"], diffs);
    compare_int_array(
        "frameIntArray",
        &ir.frame_int_array,
        &golden["frameIntArray"],
        diffs,
    );
    compare_float_array(
        "frameFloatArray",
        &ir.frame_float_array,
        &golden["frameFloatArray"],
        diffs,
    );
    compare_int_array("frameArray", &ir.frame_array, &golden["frameArray"], diffs);
    compare_int_array(
        "timelineArray",
        &ir.timeline_array,
        &golden["timelineArray"],
        diffs,
    );
    compare_int_array("colorArray", &ir.color_array, &golden["colorArray"], diffs);
    compare_int_array(
        "frameIndices",
        &ir.frame_indices,
        &golden["frameIndices"],
        diffs,
    );
}

fn compare_int_array(name: &str, got: &[i32], golden: &Value, diffs: &mut Vec<String>) {
    if diffs.len() >= 10 {
        return;
    }
    let Some(want) = golden.as_array() else {
        diffs.push(format!("arrays.{name}: JS 侧缺失"));
        return;
    };
    if got.len() != want.len() {
        diffs.push(format!(
            "arrays.{name}: 长度 Rust={} JS={}",
            got.len(),
            want.len()
        ));
        return;
    }
    for (index, (got_value, want_value)) in got.iter().zip(want).enumerate() {
        let want_value = want_value.as_i64().map(|v| v as i32);
        if want_value != Some(*got_value) {
            diffs.push(format!(
                "arrays.{name}[{index}]: Rust={got_value} JS={want_value:?}"
            ));
            return;
        }
    }
}

fn compare_float_array(name: &str, got: &[f64], golden: &Value, diffs: &mut Vec<String>) {
    if diffs.len() >= 10 {
        return;
    }
    let Some(want) = golden.as_array() else {
        diffs.push(format!("arrays.{name}: JS 侧缺失"));
        return;
    };
    if got.len() != want.len() {
        diffs.push(format!(
            "arrays.{name}: 长度 Rust={} JS={}",
            got.len(),
            want.len()
        ));
        return;
    }
    for (index, (got_value, want_value)) in got.iter().zip(want).enumerate() {
        // floatArray 的 f64 值都是 f32 量化后的精确值 → 用 f32 位比较。
        let got_bits = (*got_value as f32).to_bits();
        let want_bits = want_value.as_f64().map(|value| (value as f32).to_bits());
        if want_bits != Some(got_bits) {
            diffs.push(format!(
                "arrays.{name}[{index}]: Rust={got_value} JS={want_value:?}"
            ));
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "非字符串 panic payload".to_string()
    }
}

fn status_of(result: &RigResult) -> &'static str {
    if result.errors.is_empty() {
        "ok"
    } else if result.errors.iter().any(|e| e.starts_with("panic:")) {
        "panic"
    } else if result.errors.iter().any(|e| e.starts_with("parse_fail")) {
        "parse_fail"
    } else if result.errors.iter().any(|e| e.starts_with("build_fail")) {
        "build_fail"
    } else if result.errors.iter().any(|e| e.starts_with("non_finite")) {
        "non_finite"
    } else {
        "other"
    }
}

fn write_report(
    path: &Path,
    dirs: &[PathBuf],
    frames: usize,
    parity: usize,
    results: &[RigResult],
) {
    let mut ok = 0usize;
    let mut categories: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut js_match = 0usize;
    let mut js_diff = 0usize;
    let mut js_skip = 0usize;
    let mut total_frames = 0usize;
    let mut total_armatures = 0usize;
    let mut total_animations = 0usize;

    for result in results {
        total_frames += result.frames;
        total_armatures += result.armatures;
        total_animations += result.animations;
        if result.errors.is_empty() {
            ok += 1;
        }
        for error in &result.errors {
            if let Some((category, _)) = error.split_once(':') {
                *categories.entry(category).or_default() += 1;
            }
        }
        match &result.js {
            JsStatus::Match => js_match += 1,
            JsStatus::Diff(_) => js_diff += 1,
            JsStatus::Skip(_) => js_skip += 1,
            JsStatus::NotRun => {}
        }
    }

    let mut out = String::new();
    out.push_str("# Core 语料审计报告\n\n");
    out.push_str(&format!(
        "- 生成时间（UTC epoch 秒）：{}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    ));
    out.push_str("- 语料目录：\n");
    for dir in dirs {
        out.push_str(&format!("  - `{}`\n", dir.display()));
    }
    out.push_str(&format!(
        "- 每动画推进帧数：{frames}；JS 对拍抽样上限：{parity}\n\n"
    ));

    out.push_str("## 汇总\n\n");
    out.push_str("| 指标 | 值 |\n|---|---|\n");
    out.push_str(&format!("| 审计 rig 数 | {} |\n", results.len()));
    out.push_str(&format!("| 通过（0 错误） | {ok} |\n"));
    out.push_str(&format!("| 失败 rig 数 | {} |\n", results.len() - ok));
    out.push_str(&format!("| armature 总数 | {total_armatures} |\n"));
    out.push_str(&format!("| 推进动画总数 | {total_animations} |\n"));
    out.push_str(&format!("| 推进帧总数 | {total_frames} |\n"));
    out.push_str(&format!(
        "| JS 对拍：一致 / 不一致 / 跳过 | {js_match} / {js_diff} / {js_skip} |\n\n"
    ));

    if !categories.is_empty() {
        out.push_str("### 失败分类\n\n| 分类 | 条目数 |\n|---|---|\n");
        for (category, count) in &categories {
            out.push_str(&format!("| `{category}` | {count} |\n"));
        }
        out.push('\n');
    }

    let failed: Vec<&RigResult> = results.iter().filter(|r| !r.errors.is_empty()).collect();
    if failed.is_empty() {
        out.push_str("## 失败项\n\n无。\n\n");
    } else {
        out.push_str(&format!("## 失败项（{} 个 rig）\n\n", failed.len()));
        out.push_str("| rig | 状态 | 错误 |\n|---|---|---|\n");
        for result in &failed {
            let detail = result.errors.join("<br>").replace('|', "\\|");
            out.push_str(&format!(
                "| `{}` | {} | {detail} |\n",
                result.name,
                status_of(result)
            ));
        }
        out.push('\n');
    }

    let js_diffs: Vec<(&RigResult, &Vec<String>)> = results
        .iter()
        .filter_map(|result| match &result.js {
            JsStatus::Diff(diffs) => Some((result, diffs)),
            _ => None,
        })
        .collect();
    if !js_diffs.is_empty() {
        out.push_str("## JS 对拍差异\n\n");
        for (result, diffs) in js_diffs {
            out.push_str(&format!("### `{}`\n\n", result.name));
            for diff in diffs {
                out.push_str(&format!("- {diff}\n"));
            }
            out.push('\n');
        }
    }

    let js_skips: Vec<(&RigResult, &String)> = results
        .iter()
        .filter_map(|result| match &result.js {
            JsStatus::Skip(reason) => Some((result, reason)),
            _ => None,
        })
        .collect();
    if !js_skips.is_empty() {
        out.push_str("## JS 对拍跳过\n\n| rig | 原因 |\n|---|---|\n");
        for (result, reason) in js_skips {
            out.push_str(&format!(
                "| `{}` | {} |\n",
                result.name,
                reason.replace('|', "\\|")
            ));
        }
        out.push('\n');
    }

    out.push_str("## 全部 rig 结果\n\n");
    out.push_str(
        "| rig | armatures | animations | frames | 状态 | JS |\n|---|---|---|---|---|---|\n",
    );
    for result in results {
        let js = match &result.js {
            JsStatus::NotRun => "-",
            JsStatus::Match => "一致",
            JsStatus::Diff(_) => "不一致",
            JsStatus::Skip(_) => "跳过",
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {js} |\n",
            result.name,
            result.armatures,
            result.animations,
            result.frames,
            status_of(result)
        ));
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(error) = fs::write(path, out) {
        eprintln!("写报告失败 {}: {error}", path.display());
    } else {
        eprintln!("报告已写入 {}", path.display());
    }
}

#[test]
#[ignore = "需要本地素材语料（_research/ 或 DB_CORPUS_DIRS）+ Node；运行 cargo test --test corpus_audit -- --ignored --nocapture"]
fn audit_local_corpus() {
    let dirs = corpus_dirs();
    let mut skeletons = Vec::new();
    collect_skeletons(&dirs, &mut skeletons);
    skeletons.sort();
    skeletons.dedup();
    let limit = env_usize("DB_CORPUS_LIMIT", 0);
    if limit > 0 {
        skeletons.truncate(limit);
    }
    assert!(
        !skeletons.is_empty(),
        "没有找到 *_ske.json；请设置 DB_CORPUS_DIRS（`;` 分隔），当前目录：{dirs:?}"
    );

    let frames = env_usize("DB_CORPUS_FRAMES", DEFAULT_FRAMES);
    let parity = env_usize("DB_CORPUS_PARITY", DEFAULT_PARITY);
    let report_path = env::var("DB_CORPUS_REPORT").map_or_else(
        |_| repo_root().join("target/corpus_audit/report.md"),
        PathBuf::from,
    );

    // 抽样对拍：按顺序均匀取 `parity` 个（不足则全取）。
    let parity_indices: std::collections::BTreeSet<usize> = if parity == 0 {
        std::collections::BTreeSet::new()
    } else {
        let count = parity.min(skeletons.len());
        let step = skeletons.len().max(1).div_ceil(count);
        (0..skeletons.len()).step_by(step).take(count).collect()
    };

    let started = Instant::now();
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // 语料里预期会有 panic，静音以便报告可读

    let mut results = Vec::with_capacity(skeletons.len());
    for (index, ske_path) in skeletons.iter().enumerate() {
        let mut result = match catch_unwind(AssertUnwindSafe(|| audit_rig(ske_path, frames))) {
            Ok(result) => result,
            Err(payload) => {
                let mut result = RigResult::new(
                    ske_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                result.fail("panic", panic_message(payload.as_ref()));
                result
            }
        };

        if parity_indices.contains(&index)
            && let Some(tex_path) = tex_path_for(ske_path)
            && result.errors.is_empty()
        {
            match read_json(ske_path) {
                Ok(ske_raw) => {
                    let key = result
                        .name
                        .strip_suffix("_ske")
                        .unwrap_or(&result.name)
                        .to_string();
                    let mut factory = BaseFactory::new();
                    if let Some(data) = factory.parse_dragon_bones_data(&ske_raw, &key) {
                        let tex_raw = read_json(&tex_path).ok();
                        if let Some(tex) = &tex_raw {
                            factory.parse_texture_atlas_data(tex, &key);
                        }
                        let atlas = factory
                            .atlas_map
                            .get(&key)
                            .and_then(|list| list.first())
                            .cloned();
                        result.js = js_parity(
                            ske_path,
                            &tex_path,
                            &data,
                            atlas.as_ref(),
                            &repo_root().join("target/corpus_audit/js"),
                        );
                    } else {
                        result.js = JsStatus::Skip("Rust 解析失败，跳过对拍".to_string());
                    }
                }
                Err(error) => result.js = JsStatus::Skip(error),
            }
        }

        let elapsed = started.elapsed();
        eprintln!(
            "[{:>4}/{}] {} ({} arm, {} anim, {} frames) {:.1}s",
            index + 1,
            skeletons.len(),
            result.name,
            result.armatures,
            result.animations,
            result.frames,
            elapsed.as_secs_f64()
        );
        results.push(result);
    }

    std::panic::set_hook(old_hook);

    write_report(&report_path, &dirs, frames, parity, &results);

    let ok = results.iter().filter(|r| r.errors.is_empty()).count();
    let failed = results.len() - ok;
    let js_match = results
        .iter()
        .filter(|r| matches!(r.js, JsStatus::Match))
        .count();
    let js_diff = results
        .iter()
        .filter(|r| matches!(r.js, JsStatus::Diff(_)))
        .count();
    eprintln!(
        "\n审计完成：{} rigs，通过 {ok}，失败 {failed}，JS 对拍一致 {js_match} / 不一致 {js_diff}，用时 {:.1}s",
        results.len(),
        started.elapsed().as_secs_f64()
    );
    eprintln!("报告：{}", report_path.display());
}
