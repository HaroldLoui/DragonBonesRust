//! 对齐 `model/ConstraintData.ts`（`ConstraintData` + IK / Path 子类）。

use crate::core::enums::{ConstraintType, PositionMode, RotateMode, SpacingMode};

/// - 约束数据（`ConstraintData.ts`）。
///
/// `target` / `root` / `bone` 为骨名引用（D-1.1）。
/// 默认值对齐 `ConstraintData._onClear`：`order=0`、`name/target/root=""`、
/// `bone=null`、`kind`（type）= IK（由 [`ConstraintKind::default`] 提供）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConstraintData {
    pub order: i32,
    pub name: String,
    pub target: String,
    pub root: String,
    pub bone: Option<String>,
    pub kind: ConstraintKind,
}

/// IK / Path 两个子类的变体字段。
#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintKind {
    /// `IKConstraintData`。
    IK {
        scale_enabled: bool,
        bend_positive: bool,
        weight: f64,
    },
    /// `PathConstraintData`。
    Path {
        path_slot: String,
        path_display: Option<String>,
        bones: Vec<String>,
        position_mode: PositionMode,
        spacing_mode: SpacingMode,
        rotate_mode: RotateMode,
        position: f64,
        spacing: f64,
        rotate_offset: f64,
        rotate_mix: f64,
        translate_mix: f64,
    },
}

impl Default for ConstraintKind {
    /// 对齐 `ConstraintData._onClear` 的基类默认（`type = ConstraintType.IK`）。
    fn default() -> Self {
        Self::IK {
            scale_enabled: false,
            bend_positive: false,
            weight: 1.0,
        }
    }
}

impl ConstraintData {
    /// 对齐 TS 子类通过 `_onClear` 设置的 `type` 字段。
    #[must_use]
    pub fn type_(&self) -> ConstraintType {
        match self.kind {
            ConstraintKind::IK { .. } => ConstraintType::IK,
            ConstraintKind::Path { .. } => ConstraintType::Path,
        }
    }
}
