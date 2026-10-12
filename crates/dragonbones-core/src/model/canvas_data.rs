//! 对齐 `model/CanvasData.ts`。

/// - 画布数据（`CanvasData.ts`）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CanvasData {
    pub has_background: bool,
    pub color: i32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
