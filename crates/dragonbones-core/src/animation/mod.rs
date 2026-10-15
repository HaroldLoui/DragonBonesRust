//! `animation/Animation.ts`（959 行）的阶段 3 占位（阶段 4 完整实现，工期 8-12 天）。
//!
//! 阶段 3 只需要这些方法**存在且 no-op/None**：
//! - `Armature::advance_time` 收尾前调 `advance_time`（Armature.ts:313）；
//! - 子骨架接线调 `play`/`reset`（Slot.ts:543/575、BaseFactory `_getSlotDisplay`）；
//! - action 队列调 `fade_in`（Armature.ts:360/368/374）。
//!
//! 返回值阶段 3 一律 `None`（TS 返回 `AnimationState`，该类型阶段 4 才有）。
//!
//! ## 占位签名与官方差异（阶段 4 全量重写时一并改调用点）
//!
//! 调用点：`slot.rs:1018`（子骨架 `play(None, -1.0)`）、`base_factory.rs`
//! `_getSlotDisplay`（同款 `play(None, -1.0)`）、`armature.rs` actions 段
//! （3 处 `fade_in(name, -1.0)`）。逐条实测 TS 行号（进阶段 4 指南任务清单）：
//!
//! | 项 | 官方（实测行号） | 阶段 3 占位 | 差异 |
//! |---|---|---|---|
//! | `play` | `play(animationName: string \| null = null, playTimes: number = -1)`（Animation.ts:506） | `play(Option<&str>, f64)` | 第 2 参是**循环次数**（`playTimes`：-1 用数据默认、0 无限循环、[1~N] 循环 N 次）**非秒**——现占位误为 `f64` 淡入时间 |
//! | `stop` | `stop(animationName: string \| null = null)`（Animation.ts:304） | `stop(&mut self)` 无参 | 缺「按名停单条 / null 停全部」形参 |
//! | `fadeIn` | `fadeIn(animationName, fadeInTime = -1.0, playTimes = -1, layer = 0, group = null, fadeOutMode = AnimationFadeOutMode.SameLayerAndGroup)`（Animation.ts:567-570） | `fade_in(&str, f64)` | 缺 `playTimes/layer/group/fadeOutMode` 四参 |
//! | `gotoAndPlayByTime` | `gotoAndPlayByTime(animationName, time = 0.0, playTimes = -1)`（Animation.ts:599） | **缺** | jsdump 的播放入口（tools/jsdump.mjs:117） |
//! | `getState` | `getState(animationName, layer = -1)`（Animation.ts:787） | **缺** | |
//! | `hasAnimation` | `hasAnimation(animationName)`（Animation.ts:812） | **缺** | 子骨架同名继承判定用（Animation.ts:447） |
//! | `isPlaying` | getter（Animation.ts:840-848） | **缺** | `play` 的恢复分支用（:523） |
//! | `_animations` 挂表 | `animations` setter（Animation.ts:911-926）；Armature init 挂接 `this._animation.animations = this._armatureData.animations`（Armature.ts:290，Rust `Armature::new` 注释同址） | **缺**（`Animation` 仅存 `armature` 回指） | `playConfig` 的 `animationName in _animations` 判定依赖此表 |
//! | `timeScale` | 字段（Animation.ts:54），`_onClear` 复位 1.0（:90） | **缺** | |
//! | `_inheritTimeScale` | 字段（Animation.ts:59），`advanceTime` 内按父骨架/自身 `timeScale` 推导（:177-186） | **缺** | |
//!
//! 另注：`reset`（Animation.ts:280-289）≠ `_onClear`（Animation.ts:68-100）——
//! `armature.rs` `on_clear` 现用 `reset` 欠清（R4），阶段 4 补
//! `Animation::on_clear()` 后替换该调用。

use crate::armature::ArmatureIdx;

/// 对齐 `Animation`（Animation.ts）——阶段 3 占位。
#[derive(Debug, Default, Clone)]
pub struct Animation {
    /// 对齐 `_armature`（`Animation.init` 写入）。
    pub armature: Option<ArmatureIdx>,
}

impl Animation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 对齐 `Animation.init(armatureValue)`（阶段 3 仅记录回指）。
    pub fn init(&mut self, armature: ArmatureIdx) {
        self.armature = Some(armature);
    }

    /// 对齐 `Animation.advanceTime(passedTime)`（Armature.ts:313 调用）。
    /// TODO 阶段 4：fade 状态机 + 时间轴推进。
    pub fn advance_time(&mut self, _passed_time: f64) {}

    /// 对齐 `Animation.play(animationName: string | null = null, playTimes: number = -1)`
    ///（Animation.ts:506；Slot.ts:575 无参调用）。第 2 参是**循环次数**非秒——
    /// 现占位 `f64` 形参语义错位（模块头差异表），阶段 4 重构。
    /// TODO 阶段 4：返回 `AnimationState`。
    pub fn play(&mut self, _animation_name: Option<&str>, _fade_in_time: f64) -> Option<()> {
        None
    }

    /// 对齐 `Animation.fadeIn(animationName, fadeInTime = -1.0, playTimes = -1,
    /// layer = 0, group = null, fadeOutMode = SameLayerAndGroup)`（Animation.ts:567-570；
    /// 调用点 Armature.ts:360/368/374 只传首参 → 现占位传 -1.0 淡入时间等价默认）。
    /// 阶段 4 补齐后四参并改签名。
    pub fn fade_in(&mut self, _animation_name: &str, _fade_in_time: f64) -> Option<()> {
        None
    }

    /// 对齐 `Animation.reset()`（Animation.ts:280-289；Slot.ts:543 子骨架解绑时）。
    pub fn reset(&mut self) {}

    /// 对齐 `Animation.stop(animationName = null)`（Animation.ts:304）。
    /// 现占位**无参**且阶段 3 恒 no-op；阶段 4 补形参（null = 停全部、传名 = 停单条）。
    pub fn stop(&mut self) {}
}
