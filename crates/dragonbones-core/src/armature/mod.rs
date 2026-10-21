//! 阶段 3：`armature/*.ts` 运行时层（TransformObject / Bone / Surface / Slot /
//! Armature / Constraint / IArmatureProxy）。
//!
//! 设计决策见 `docs/guide/03-stage3.md` D-3.1～D-3.4：
//! - arena + newtype 索引（[`ArmatureIdx`] / [`BoneIdx`] / [`SlotIdx`]），禁 `Rc<RefCell>`（D3）；
//! - 环引用（Armature ↔ Slot 子骨架回指）双方只存纯数值索引（D-3.2）；
//! - 渲染契约拆成 trait（[`SlotRenderAdapter`] / [`ArmatureProxy`]），阶段 3 交 Null 实现（D-3.4）。
//!
//! **索引类型放本模块而非 `factory/base_factory.rs`**（指南 D-3.1 草图写在 factory）：
//! `armature` 层自身（Slot/Armature/EventObject）都要用索引，放这里避免
//! `armature → factory` 的反向依赖；`factory` 侧直接 `use crate::armature::*`。

// D7（1 TS 文件 → 1 Rust 模块）：`armature/Armature.ts` → `armature/armature.rs`，
// 与容器模块同名是刻意的，压掉 `module_inception` 提示。
#[allow(clippy::module_inception)]
pub mod armature;
pub mod bone;
pub mod constraint;
pub mod frame_cache;
pub mod i_armature_proxy;
pub mod path_constraint;
pub mod runtime_ctx;
pub mod slot;
pub mod surface;
pub mod transform_object;

pub use armature::Armature;
pub use bone::Bone;
pub use constraint::Constraint;
pub use frame_cache::{AnimationFrameCache, FrameCache};
pub use i_armature_proxy::{ArmatureProxy, DbEventListener, ListenerId, NullArmatureProxy};
pub use path_constraint::PathConstraint;
pub use runtime_ctx::{RuntimeCtx, RuntimeCtxError};
pub use slot::{DisplayFrame, NullSlotAdapter, Slot, SlotRenderAdapter, SlotUpdateCtx};
pub use transform_object::TransformObject;

/// 骨架 arena 索引（D-3.1）。指向 `BaseFactory.armatures` 的槽位；
/// `None` 空位 = 已 dispose（D-3.1 池语义的 arena 版）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArmatureIdx(pub u32);

/// 本骨架内骨骼索引。指向 `Armature.bones`，创建序 = `sortedBones` 拓扑序（F3.2）——
/// `Armature::advance_time` 的 `split_at_mut(i)` 顺序更新依赖"父索引 < 子索引"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoneIdx(pub u32);

/// 本骨架内插槽索引 = **创建序身份**（`Slot::init` 预置、永不改写）。
///
/// 对应官方的对象引用语义：槽排序重排（`advanceTime` ③）只动 `Vec` 位置、不动
/// `Slot.idx`，因此**排序后不可拿它当下标**——位置访问必须按 `slot.idx` 扫描
/// （build 期 slots 恒创建序，`slots[idx]` 才安全；语义详见 `armature.rs` 排序处
/// D-4.29 注）。消费方：`Armature.parent` 回指、`EventObject.slot`、alpha 传播。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIdx(pub u32);

/// Constraint identity is creation order and remains stable for the armature lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintIdx(pub u32);

macro_rules! impl_idx {
    ($t:ty) => {
        impl $t {
            /// 索引值 → `usize` 下标。
            #[must_use]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}
impl_idx!(ArmatureIdx);
impl_idx!(BoneIdx);
impl_idx!(SlotIdx);
impl_idx!(ConstraintIdx);

/// core 不认识引擎显示对象（raw / mesh / 子骨架 display）→ 占位枚举，
/// 阶段 7 由 Bevy 适配层映射成实体（渲染契约隔离，关键结论 2 / D-3.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayHandle {
    /// 空显示（`displayIndex == -1` 或 displayFrame.display 为 null）。
    Null,
    /// 槽的原始显示对象（`_rawDisplay`，TS `Slot.ts:248`）。
    Raw,
    /// 槽的网格显示对象（`_meshDisplay`，TS `Slot.ts:249`；可能与 raw 相同）。
    Mesh,
    /// 子骨架显示（TS `_display instanceof Armature` 分支，`Slot.ts:510-513`）。
    Armature(ArmatureIdx),
}
