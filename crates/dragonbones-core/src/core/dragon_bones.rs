//! `core/DragonBones.ts` 的静态部分 + 实例 runtime 归属说明。
//!
//! 官方实例字段（`_clock` / `_events` / `_objects` / `_eventManager` / `advanceTime`）
//! 在 Rust 由 [`crate::factory::BaseFactory`] 唯一持有（阶段 4/5 决策：不引入第二个
//! runtime owner）。本模块只保留可静态引用的常量。

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
