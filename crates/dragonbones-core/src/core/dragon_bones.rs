//! `core/DragonBones.ts` 的静态部分。
//!
//! 实例类（`_clock` / `_events` / `_objects` / `advanceTime` / `bufferEvent`）依赖
//! `WorldClock`（阶段 4）与 `EventObject`（阶段 5），本阶段不实现，阶段 5 收口（P0-12）。

/// DragonBones 命名空间常量。
pub struct DragonBones;

impl DragonBones {
    /// 对齐 `DragonBones.VERSION`。
    pub const VERSION: &'static str = "5.7.000";
    /// 对齐 `DragonBones.yDown`（数据坐标系 Y 轴向下）。
    pub const Y_DOWN: bool = true;
    /// 对齐 `DragonBones.debug`。
    pub const DEBUG: bool = false;
    /// 对齐 `DragonBones.debugDraw`。
    pub const DEBUG_DRAW: bool = false;
}
