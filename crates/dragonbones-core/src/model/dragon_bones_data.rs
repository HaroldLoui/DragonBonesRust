//! 对齐 `model/DragonBonesData.ts`（`DragonBonesData` + 共享根 `RootInfo` / IR 数组）。
//!
//! IR（扁平数组）是整个复刻的地基（D6），所有时间轴按 `core::enums::binary_offset`
//! 定义的偏移读取。`RootInfo` 把 TS 中 `DragonBonesData.name` 与 8 组数组打包成
//! 可被 `Arc` 共享的只读根，供 `ArmatureData.parent` / `GeometryData.root` 引用（D-1.1）。

use std::sync::Arc;

use indexmap::IndexMap;

use crate::model::armature_data::ArmatureData;
use crate::model::user_data::UserData;

/// 扁平 IR 数组集合（对应 TS `DragonBonesData` 的 8 组数组）。
///
/// # 值精度（实测修正 requirements D1，见 `docs/model_mapping.md` §4.1）
///
/// JS 中**两条路径的运行时值精度相同**——JSON 路径 parse 收尾的 `_modifyArray()`
/// （`ObjectDataParser.ts:2229+`）会把 `number[]` 重解释为 TypedArray：
/// float 组 → `Float32Array`（f32 量化），int 组 → `Int16Array` / `Uint16Array`。
/// 容器用 `f64` / `i32` 承载，但**阶段 2 必须在 [`DragonBonesData::finalize_root`]
/// 处模拟该量化**（`v as f32 as f64`、i16/u16 wrap、奇数长度补 0 对齐），
/// 否则 IR 逐元素断言会输给 JS。
///
/// - `color_array` 在 TS 是 `Int16Array | Uint16Array`，统一存 `i32`。
/// - `frame_indices` 是普通 `number[]`（不经 `_modifyArray`），无量化，值为整数索引。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct IrArrays {
    pub int_array: Vec<i32>,
    pub float_array: Vec<f64>,
    pub frame_int_array: Vec<i32>,
    pub frame_float_array: Vec<f64>,
    pub frame_array: Vec<i32>,
    pub timeline_array: Vec<i32>,
    pub color_array: Vec<i32>,
    pub frame_indices: Vec<i32>,
}

impl IrArrays {
    /// 对齐 `ObjectDataParser._modifyArray` 的**写出**（P1-10）。
    ///
    /// 顺序与 `_modifyArray` 完全一致：int i16、float f32、frameInt i16、
    /// frameFloat f32、frame i16、timeline u16、color i16；均为小端。
    /// 调用前数组必须已经过 padding（[`crate::parser::ObjectDataParser`] 的 JSON 路径）。
    #[must_use]
    pub fn to_binary(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            self.int_array.len() * 2
                + self.float_array.len() * 4
                + self.frame_int_array.len() * 2
                + self.frame_float_array.len() * 4
                + self.frame_array.len() * 2
                + self.timeline_array.len() * 2
                + self.color_array.len() * 2,
        );
        for value in &self.int_array {
            bytes.extend_from_slice(&(*value as i16).to_le_bytes());
        }
        for value in &self.float_array {
            bytes.extend_from_slice(&(*value as f32).to_le_bytes());
        }
        for value in &self.frame_int_array {
            bytes.extend_from_slice(&(*value as i16).to_le_bytes());
        }
        for value in &self.frame_float_array {
            bytes.extend_from_slice(&(*value as f32).to_le_bytes());
        }
        for value in &self.frame_array {
            bytes.extend_from_slice(&(*value as i16).to_le_bytes());
        }
        for value in &self.timeline_array {
            bytes.extend_from_slice(&(*value as u16).to_le_bytes());
        }
        for value in &self.color_array {
            bytes.extend_from_slice(&(*value as i16).to_le_bytes());
        }
        bytes
    }
}

/// 共享根：TS 中所有 “parent → DragonBonesData” 实际读取的内容（D-1.1）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RootInfo {
    pub name: String,
    pub ir: IrArrays,
}

/// - 龙骨数据（`DragonBonesData.ts`）。
///
/// 一个 `DragonBonesData` 包含多个 [`ArmatureData`]。
#[derive(Debug, Clone, PartialEq)]
pub struct DragonBonesData {
    pub name: String,
    /// 解析收尾时由 [`DragonBonesData::finalize_root`] 回填的共享根。
    pub root: Option<Arc<RootInfo>>,
    pub auto_search: bool,
    pub frame_rate: f64,
    pub version: String,
    /// TS `stage: ArmatureData | null` → 骨架名引用。
    pub stage: Option<String>,
    pub user_data: Option<UserData>,
    pub armature_names: Vec<String>,
    pub armatures: IndexMap<String, ArmatureData>,
    /// P1-4（`setCacheFrame`）写入点，阶段 1 只留字段。
    pub cached_frames: Vec<f64>,
    /// JS 在 JSON 路径收尾也会构建 `binary`（`_modifyArray` 的 ArrayBuffer，实测非 null）；
    /// Rust 暂不复刻，阶段 2 / P1-10 再定是否保留。
    pub binary: Option<Vec<u8>>,
}

impl Default for DragonBonesData {
    fn default() -> Self {
        Self {
            name: String::new(),
            root: None,
            auto_search: false,
            frame_rate: 0.0,
            version: String::new(),
            stage: None,
            user_data: None,
            armature_names: Vec::new(),
            armatures: IndexMap::new(),
            cached_frames: Vec::new(),
            binary: None,
        }
    }
}

impl DragonBonesData {
    /// 对齐 `DragonBonesData.addArmature`（重名忽略）。
    ///
    /// `parent` 共享根在 [`DragonBonesData::finalize_root`] 中统一回填。
    pub fn add_armature(&mut self, value: ArmatureData) {
        if self.armatures.contains_key(&value.name) {
            log::warn!("Same armature: {}", value.name);
            return;
        }

        self.armature_names.push(value.name.clone());
        self.armatures.insert(value.name.clone(), value);
    }

    /// 对齐 `DragonBonesData.getArmature`。
    #[must_use]
    pub fn get_armature(&self, armature_name: &str) -> Option<&ArmatureData> {
        self.armatures.get(armature_name)
    }

    /// 解析收尾：用累积好的 IR 数组构建共享根，并回填到全部骨架与几何数据。
    pub fn finalize_root(&mut self, ir: IrArrays) {
        let root = Arc::new(RootInfo {
            name: self.name.clone(),
            ir,
        });

        self.root = Some(root.clone());
        for armature in self.armatures.values_mut() {
            armature.set_root_info(&root);
        }
    }

    /// JSON 解析收尾：按 `_modifyArray` 布局生成 `data.binary`（P1-10）。
    ///
    /// 注意这不是 dbbin 文件：没有 `DBDT` tag/header，只是 7 组 TypedArray 的字节流。
    pub fn pack_binary(&mut self) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        self.binary = Some(root.ir.to_binary());
    }
}
