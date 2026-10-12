//! 对齐 `model/BoundingBoxData.ts`。
//!
//! 阶段 1 只建立类型与字段；`containsPoint` / `intersectsSegment` 等逻辑属于
//! P1-3，留待阶段 6。

use crate::core::enums::BoundingBoxType;

/// - 边界框数据（`BoundingBoxData.ts`，抽象类的单一落地实现）。
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBoxData {
    pub color: i32,
    pub width: f64,
    pub height: f64,
    pub kind: BoundingBoxKind,
}

/// 三个子类的变体字段（对应 `Rectangle` / `Ellipse` / `PolygonBoundingBoxData`）。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundingBoxKind {
    Rectangle,
    Ellipse,
    Polygon { x: f64, y: f64, vertices: Vec<f64> },
}

impl Default for BoundingBoxData {
    fn default() -> Self {
        Self {
            color: 0x000000,
            width: 0.0,
            height: 0.0,
            kind: BoundingBoxKind::Rectangle,
        }
    }
}

impl BoundingBoxData {
    /// 对齐 TS 子类通过 `_onClear` 设置的 `type` 字段。
    #[must_use]
    pub fn type_(&self) -> BoundingBoxType {
        match self.kind {
            BoundingBoxKind::Rectangle => BoundingBoxType::Rectangle,
            BoundingBoxKind::Ellipse => BoundingBoxType::Ellipse,
            BoundingBoxKind::Polygon { .. } => BoundingBoxType::Polygon,
        }
    }
}
