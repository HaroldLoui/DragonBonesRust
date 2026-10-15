//! 阶段 3 静姿态黄金差分测试（对应 **P0-6 / P0-7 / M3**，即 `docs/guide/03-stage3.md`
//! 验收清单第 1-5 条；清单第 6 条「阶段 0-2 全量回归」由既有 `ir_golden` /
//! `geom_golden` / `golden_*` 测试承担）。
//!
//! 流程镜像 `tools/posedump.mjs`：parse ske/tex → `build_armature`
//! （内部已 `invalid_update(null,true)` + `advanceTime(0.0)`，F3.11）→
//! `factory.advance_time(0.0)`（posedump 步骤 4 的第二遍全局 advance——官方
//! `WorldClock.advanceTime(0)` 在 WorldClock.ts:114-116 早退，此处是**镜像调用 +
//! 再验一次 no-op**，真正的收敛点是 build 内的 `advance_one(idx, 0.0)`）→ dump 比对。
//!
//! 逐素材断言：
//! 1. **骨骼 13 元**（顺序 = posedump：`x, y, rotation, scaleX, scaleY, skew,
//!    a, b, c, d, tx, ty, globalAlpha`）逐元素 == 黄金（验收清单 1）；
//! 2. **插槽 15 元**（`displayIndex, visible, zOrder, zIndex, rM, gM, bM, aM,
//!    rO, gO, bO, aO, gx, gy, globalAlpha`）前 4 元整数**全等**、后 11 元同容差
//!    （验收清单 2）；
//! 3. **名序**：`meta.boneNames` == `get_bones()` 名序、`meta.slotNames` ==
//!    `get_slots()` 名序（全等，**F3.10 排序锚点**，验收清单 3）；
//! 4. **frameRate**：`meta.frameRate` == `armature_data().frame_rate`（全等或 1e-12）；
//! 5. **geometry**：黄金 `geometry` 每槽 → 运行时同名槽当前显示帧的
//!    `vertex_count()/triangle_count()` 全等；**反向**：运行时有几何而黄金没有 →
//!    失败（完整性，验收清单 5，含 Surface_P1_4 的 mesh 槽）。
//!
//! 统一容差 `|got - want| <= 1e-9 * (1.0 + |want|)`（**不放宽**到 1e-6）；
//! 黄金 `null`（posedump `fin()` 的 NaN 哨兵）与运行时 NaN 双方视为相等。
//! 失败信息全中文带上下文，并在 panic 里累计「每素材最大绝对/相对误差 +
//! 前 5 最差条目（含 want/got）+ 失败分类计数」，便于定位。

mod common;

use common::ir_snapshot::{asset_root, read_json, tex_stem};
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use serde_json::Value;
use std::path::PathBuf;

/// 黄金清单（`testdata/pose/<name>.json`，9 个素材，硬编码，仿 `golden_inventory` 风格）。
const POSE_NAMES: &[&str] = &[
    "龙",
    "Dragon",
    "Dragon_Blend",
    "PlantAnimate",
    "PlantAnimate_compressed",
    "Rooster_Ani",
    "Rooster_Blend",
    "Rooster_P1",
    "Surface_P1_4",
];

/// 骨骼 13 元字段名（顺序 = `tools/posedump.mjs` 116-120）。
const BONE_FIELDS: &[&str] = &[
    "x",
    "y",
    "rotation",
    "scaleX",
    "scaleY",
    "skew",
    "a",
    "b",
    "c",
    "d",
    "tx",
    "ty",
    "globalAlpha",
];

/// 插槽 15 元字段名（顺序 = `tools/posedump.mjs` 126-132）。
const SLOT_FIELDS: &[&str] = &[
    "displayIndex",
    "visible",
    "zOrder",
    "zIndex",
    "redM",
    "greenM",
    "blueM",
    "alphaM",
    "redO",
    "greenO",
    "blueO",
    "alphaO",
    "gx",
    "gy",
    "globalAlpha",
];

const BONE_VEC_LEN: usize = 13;
const SLOT_VEC_LEN: usize = 15;

/// 数值容差（全测试统一，不放宽）：`|got-want| <= 1e-9 * (1.0 + |want|)`。
const REL_TOL: f64 = 1e-9;
/// `meta.frameRate` 容差（验收：全等或 1e-12）。
const FRAME_RATE_TOL: f64 = 1e-12;

fn pose_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/pose")
}

/// 黄金数值元素：`null`（posedump `fin()` 的 NaN 哨兵）→ NaN，其余取 f64。
fn golden_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// 黄金字符串数组（`meta.boneNames` / `meta.slotNames`）。
fn golden_names(v: &Value, name: &str, field: &str) -> Vec<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("{name}: golden {field} 应为 array"))
        .iter()
        .map(|x| {
            x.as_str()
                .unwrap_or_else(|| panic!("{name}: golden {field} 元素应为 string"))
                .to_string()
        })
        .collect()
}

/// 单素材差分收集器：失败条目 + 误差诊断（最大绝对/相对误差 + 前 5 最差条目）。
///
/// 全部数值检查跑完后 [`Self::finish`] 统一 panic（一条不落的诊断都在 panic 消息里）。
struct PoseDiag {
    /// 素材名（黄金文件名，失败信息前缀）。
    name: String,
    /// 全部失败条目（中文带上下文）。
    failures: Vec<String>,
    /// 已检查的数值项数（含 NaN 哨兵配对）。
    checked: usize,
    /// 最大绝对误差及其条目。
    max_abs: f64,
    max_abs_ctx: String,
    /// 最大相对误差（`|Δ|/(1+|want|)`）及其条目。
    max_rel: f64,
    max_rel_ctx: String,
    /// 前 5 最差条目（按 `|Δ|/(1+|want|)` 降序，含 want/got）。
    worst: Vec<(f64, String)>,
}

impl PoseDiag {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            failures: Vec::new(),
            checked: 0,
            max_abs: 0.0,
            max_abs_ctx: String::new(),
            max_rel: 0.0,
            max_rel_ctx: String::new(),
            worst: Vec::new(),
        }
    }

    /// 结构性失败（名序 / geometry 完整性等非数值逐元素项）。
    fn fail(&mut self, message: String) {
        self.failures.push(message);
    }

    /// 数值比较：`tol` 为相对系数（0.0 = 整数/全等项）。
    ///
    /// 失败条目格式（验收要求）：`{素材}: {ctx}: want={w} got={g}`，
    /// 其中 `ctx` 形如 ``骨 `name` 第{i}元 {字段}`` / ``槽 `name` 第{i}元 {字段}``。
    fn num(&mut self, ctx: &str, want: f64, got: f64, tol: f64) {
        self.checked += 1;

        // NaN 哨兵：双方 NaN 视为相等（黄金 null ↔ 运行时 NaN）
        if want.is_nan() && got.is_nan() {
            return;
        }

        let abs = (got - want).abs();
        let rel = if abs.is_nan() {
            f64::INFINITY
        } else {
            abs / (1.0 + want.abs())
        };
        let entry = format!("{ctx}: want={want} got={got}");

        if !abs.is_nan() && abs > self.max_abs {
            self.max_abs = abs;
            self.max_abs_ctx = entry.clone();
        }
        if !rel.is_nan() && rel > self.max_rel {
            self.max_rel = rel;
            self.max_rel_ctx = entry.clone();
        }
        self.note_worst(rel, entry);

        if abs.is_nan() || abs > tol * (1.0 + want.abs()) {
            self.failures
                .push(format!("{}: {ctx}: want={want} got={got}", self.name));
        }
    }

    /// 常规容差（1e-9）数值项。
    fn close(&mut self, ctx: &str, want: f64, got: f64) {
        self.num(ctx, want, got, REL_TOL);
    }

    /// 全等项（整数状态 / geometry 计数）。
    fn exact(&mut self, ctx: &str, want: f64, got: f64) {
        self.num(ctx, want, got, 0.0);
    }

    /// 名序全等（Vec<String> 比较，失败时两侧整表带出）。
    fn names(&mut self, ctx: &str, want: &[String], got: &[String]) {
        if want != got {
            self.failures.push(format!(
                "{}: {ctx} 不等\n    want({} 项) = {want:?}\n    got ({} 项) = {got:?}",
                self.name,
                want.len(),
                got.len()
            ));
        }
    }

    /// 保持前 5 最差（按误差比降序）。
    fn note_worst(&mut self, ratio: f64, entry: String) {
        let ratio = if ratio.is_nan() { f64::INFINITY } else { ratio };
        let pos = self
            .worst
            .iter()
            .position(|(r, _)| *r <= ratio)
            .unwrap_or(self.worst.len());
        self.worst.insert(pos, (ratio, entry));
        if self.worst.len() > 5 {
            self.worst.truncate(5);
        }
    }

    /// 有失败则带完整诊断 panic；无失败静默返回（测试通过）。
    fn finish(self) {
        if self.failures.is_empty() {
            return;
        }

        let bone_fail = self
            .failures
            .iter()
            .filter(|s| s.contains(": 骨 `"))
            .count();
        let slot_fail = self
            .failures
            .iter()
            .filter(|s| s.contains(": 槽 `"))
            .count();
        let geo_fail = self.failures.iter().filter(|s| s.contains("geometry")).count();
        let meta_fail = self
            .failures
            .iter()
            .filter(|s| s.contains("meta."))
            .count();
        let total = self.failures.len();
        let other = total
            .saturating_sub(bone_fail + slot_fail + geo_fail + meta_fail);

        let mut msg = format!(
            "{}: 静姿态差分失败：共 {total} 条（已检查数值 {} 项）；\
             失败分类：骨 {bone_fail} / 槽 {slot_fail} / meta(名序、frameRate) {meta_fail} \
             / geometry {geo_fail} / 其它 {other}；\
             最大绝对误差 = {}（{}）；最大相对误差 = {}（{}）",
            self.name, self.checked, self.max_abs, self.max_abs_ctx, self.max_rel, self.max_rel_ctx
        );

        msg.push_str("\n前 5 最差条目（按 |Δ|/(1+|want|) 降序）：");
        for (i, (ratio, entry)) in self.worst.iter().enumerate() {
            msg.push_str(&format!("\n  {}. [比 {ratio:e}] {entry}", i + 1));
        }

        msg.push_str("\n前 12 条失败：");
        for failure in self.failures.iter().take(12) {
            msg.push_str("\n  ");
            msg.push_str(failure);
        }
        if total > 12 {
            msg.push_str(&format!("\n  …（其余 {} 条略）", total - 12));
        }

        panic!("{msg}");
    }
}

/// 单素材全量差分（9 个 `#[test]` 共用的公共 helper）。
fn check_pose(name: &str) {
    let mut diag = PoseDiag::new(name);

    // ---- 1. 黄金 + meta ----
    let golden = read_json(&pose_root().join(format!("{name}.json")));
    let meta = &golden["meta"];
    let ske_file = meta["skeFile"]
        .as_str()
        .unwrap_or_else(|| panic!("{name}: golden meta.skeFile 应为 string"));
    let armature_name = meta["armature"]
        .as_str()
        .unwrap_or_else(|| panic!("{name}: golden meta.armature 应为 string"));
    let want_bone_names = golden_names(&meta["boneNames"], name, "meta.boneNames");
    let want_slot_names = golden_names(&meta["slotNames"], name, "meta.slotNames");
    let want_frame_rate = meta["frameRate"]
        .as_f64()
        .unwrap_or_else(|| panic!("{name}: golden meta.frameRate 应为 number"));

    // ---- 2. 素材文件名映射：assets/<skeFile> + assets/<tex_stem>.json ----
    let ske_stem = ske_file
        .strip_suffix(".json")
        .unwrap_or_else(|| panic!("{name}: meta.skeFile 应以 .json 结尾（{ske_file}）"));
    let ske = read_json(&asset_root().join(ske_file));
    let tex_path = asset_root().join(format!("{}.json", tex_stem(ske_stem)));
    let tex = read_json(&tex_path);

    // ---- 3. factory 流程（posedump 93-104 同序）----
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, ske_stem)
        .unwrap_or_else(|| panic!("{name}: parse_dragon_bones_data 失败（ske = {ske_file}）"));
    if !factory.parse_texture_atlas_data(&tex, ske_stem) {
        diag.fail(format!(
            "{name}: parse_texture_atlas_data 返回 false（tex = {}）",
            tex_path.display()
        ));
    }
    let armature_idx = factory
        .build_armature(
            armature_name,
            ske_stem,
            "",
            ske_stem,
            Box::new(NullArmatureProxy::new()),
        )
        .unwrap_or_else(|| {
            panic!("{name}: build_armature(`{armature_name}`, key = `{ske_stem}`) 失败")
        });
    // posedump 步骤 4 的第二遍**全局** advance：官方 `WorldClock.advanceTime(0.0)`
    // 在 WorldClock.ts:114-116 直接 return → 此调用在官方管线里就是 no-op；
    // 真正的收敛点是 buildArmature 收尾的那遍 `armature.advanceTime(0)`
    // （BaseFactory.ts:713，Rust 侧 = `advance_one(idx, 0.0)`，build_armature 内部已完成）。
    // 此处保留 = 再验一次"全局 advance(0) 不改姿态"（等价 no-op 检查）。
    factory.advance_time(0.0);
    let armature = factory.armature(armature_idx);

    // ---- 4. 名序（验收清单 3；F3.10 排序锚点）+ frameRate ----
    let got_bone_names: Vec<String> = armature
        .get_bones()
        .iter()
        .map(|b| b.bone_name.clone())
        .collect();
    diag.names(
        "meta.boneNames 与运行时 get_bones() 名序",
        &want_bone_names,
        &got_bone_names,
    );
    let got_slot_names: Vec<String> = armature
        .get_slots()
        .iter()
        .map(|s| s.slot_name.clone())
        .collect();
    diag.names(
        "meta.slotNames 与运行时 get_slots() 名序（F3.10 排序锚点）",
        &want_slot_names,
        &got_slot_names,
    );
    diag.num(
        "meta.frameRate",
        want_frame_rate,
        armature.armature_data().frame_rate,
        FRAME_RATE_TOL,
    );

    // ---- 5. 骨骼 13 元（验收清单 1）----
    let golden_bones = golden["bones"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: golden bones 应为 object"));
    for (bone_name, vector) in golden_bones {
        let elements = vector
            .as_array()
            .unwrap_or_else(|| panic!("{name}: 骨 `{bone_name}` 黄金向量应为 array"));
        if elements.len() != BONE_VEC_LEN {
            diag.fail(format!(
                "{name}: 骨 `{bone_name}` 黄金向量长度 {} != {BONE_VEC_LEN}",
                elements.len()
            ));
            continue;
        }

        let Some(bone_idx) = armature.get_bone(bone_name) else {
            diag.fail(format!(
                "{name}: 骨 `{bone_name}` 在运行时骨架中不存在（get_bone 返回 None）"
            ));
            continue;
        };
        let bone = &armature.get_bones()[bone_idx.index()];
        let global = &bone.global;
        let matrix = &bone.global_transform_matrix;
        let got = [
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

        for (i, element) in elements.iter().enumerate() {
            let ctx = format!("骨 `{bone_name}` 第{i}元 {}", BONE_FIELDS[i]);
            diag.close(&ctx, golden_f64(element), got[i]);
        }
    }

    // ---- 6. 插槽 15 元（验收清单 2）----
    let golden_slots = golden["slots"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: golden slots 应为 object"));
    for (slot_name, vector) in golden_slots {
        let elements = vector
            .as_array()
            .unwrap_or_else(|| panic!("{name}: 槽 `{slot_name}` 黄金向量应为 array"));
        if elements.len() != SLOT_VEC_LEN {
            diag.fail(format!(
                "{name}: 槽 `{slot_name}` 黄金向量长度 {} != {SLOT_VEC_LEN}",
                elements.len()
            ));
            continue;
        }

        let Some(slot_idx) = armature.get_slot(slot_name) else {
            diag.fail(format!(
                "{name}: 槽 `{slot_name}` 在运行时骨架中不存在（get_slot 返回 None）"
            ));
            continue;
        };
        // SlotIdx 是创建序身份，槽可能被排序重排 → 按身份扫描而非位置下标
        let Some(slot) = armature.get_slots().iter().find(|s| s.idx == slot_idx) else {
            diag.fail(format!(
                "{name}: 槽 `{slot_name}` 按身份扫描未找到（get_slot 已返回 {slot_idx:?}）"
            ));
            continue;
        };
        let color = &slot.color_transform;
        let got = [
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
        ];

        for (i, element) in elements.iter().enumerate() {
            let ctx = format!("槽 `{slot_name}` 第{i}元 {}", SLOT_FIELDS[i]);
            let want = golden_f64(element);
            if i < 4 {
                // displayIndex / visible / zOrder / zIndex：整数全等
                diag.exact(&ctx, want, got[i]);
            } else {
                diag.close(&ctx, want, got[i]);
            }
        }
    }

    // ---- 7. geometry 摘要（验收清单 5，正向 + 反向完整性）----
    let golden_geometry = golden["geometry"]
        .as_object()
        .unwrap_or_else(|| panic!("{name}: golden geometry 应为 object"));
    for (slot_name, counts) in golden_geometry {
        let counts = counts
            .as_array()
            .unwrap_or_else(|| panic!("{name}: geometry 槽 `{slot_name}` 应为 array"));
        if counts.len() != 2 {
            diag.fail(format!(
                "{name}: geometry 槽 `{slot_name}` 黄金向量长度 {} != 2",
                counts.len()
            ));
            continue;
        }
        let (want_vertex, want_triangle) = (golden_f64(&counts[0]), golden_f64(&counts[1]));

        let Some(slot_idx) = armature.get_slot(slot_name) else {
            diag.fail(format!(
                "{name}: geometry 槽 `{slot_name}` 在运行时骨架中不存在"
            ));
            continue;
        };
        let Some(slot) = armature.get_slots().iter().find(|s| s.idx == slot_idx) else {
            diag.fail(format!(
                "{name}: geometry 槽 `{slot_name}` 按身份扫描未找到（get_slot 已返回 {slot_idx:?}）"
            ));
            continue;
        };
        let Some(frame) = usize::try_from(slot.display_index)
            .ok()
            .and_then(|i| slot.display_frames.get(i))
        else {
            diag.fail(format!(
                "{name}: geometry 槽 `{slot_name}`: display_index = {} 越界（display_frames 共 {} 帧），无法取当前显示帧",
                slot.display_index,
                slot.display_frames.len()
            ));
            continue;
        };
        let Some(geometry_data) = frame.get_geometry_data() else {
            diag.fail(format!(
                "{name}: geometry 槽 `{slot_name}`: 运行时当前显示帧无 geometry，黄金却登记了 vertexCount/triangleCount"
            ));
            continue;
        };
        diag.exact(
            &format!("geometry 槽 `{slot_name}` vertexCount"),
            want_vertex,
            f64::from(geometry_data.vertex_count()),
        );
        diag.exact(
            &format!("geometry 槽 `{slot_name}` triangleCount"),
            want_triangle,
            f64::from(geometry_data.triangle_count()),
        );
    }

    // 反向：运行时任何槽有 geometry 而黄金没有 → 失败（完整性）
    for slot in armature.get_slots() {
        let frame = usize::try_from(slot.display_index)
            .ok()
            .and_then(|i| slot.display_frames.get(i));
        if let Some(frame) = frame
            && let Some(geometry_data) = frame.get_geometry_data()
            && !golden_geometry.contains_key(&slot.slot_name)
        {
            diag.fail(format!(
                "{}: geometry 完整性：运行时槽 `{}` 有 geometry（vertexCount = {} triangleCount = {}）但黄金未登记",
                name,
                slot.slot_name,
                geometry_data.vertex_count(),
                geometry_data.triangle_count()
            ));
        }
    }

    diag.finish();
}

// ---- 9 个素材各一个 #[test]（清单硬编码，仿 golden_inventory 风格）----

/// 素材 1：`龙`（ske = `龙_ske.json`，60 骨 29 槽，含 5 个 mesh 槽）。
#[test]
fn pose_golden_long() {
    check_pose(POSE_NAMES[0]);
}

/// 素材 2：`Dragon`（19 骨 18 槽）。
#[test]
fn pose_golden_dragon() {
    check_pose(POSE_NAMES[1]);
}

/// 素材 3：`Dragon_Blend`（同 Dragon 骨架、Blend 数据）。
#[test]
fn pose_golden_dragon_blend() {
    check_pose(POSE_NAMES[2]);
}

/// 素材 4：`PlantAnimate`（29 骨 31 槽）。
#[test]
fn pose_golden_plant_animate() {
    check_pose(POSE_NAMES[3]);
}

/// 素材 5：`PlantAnimate_compressed`（压缩 JSON 版本，key = `PlantAnimate_ske_compressed`）。
#[test]
fn pose_golden_plant_animate_compressed() {
    check_pose(POSE_NAMES[4]);
}

/// 素材 6：`Rooster_Ani`（63 骨 15 槽，8 个 mesh 槽）。
#[test]
fn pose_golden_rooster_ani() {
    check_pose(POSE_NAMES[5]);
}

/// 素材 7：`Rooster_Blend`。
#[test]
fn pose_golden_rooster_blend() {
    check_pose(POSE_NAMES[6]);
}

/// 素材 8：`Rooster_P1`。
#[test]
fn pose_golden_rooster_p1() {
    check_pose(POSE_NAMES[7]);
}

/// 素材 9：`Surface_P1_4`（4 骨 2 槽，Surface 曲面骨通路，验收清单 4）。
#[test]
fn pose_golden_surface_p1_4() {
    check_pose(POSE_NAMES[8]);
}
