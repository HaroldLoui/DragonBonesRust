//! DragonBones 运行时核心（纯逻辑，f64 全程，不依赖 Bevy）。
//!
//! 分阶段对齐 `DragonBonesJS/DragonBones/src/dragonBones/`：
//! - [`geom`]：`geom/*.ts` 5 个数学类
//! - [`core`]：`core/BaseObject`、`core/DragonBones` 静态部分与全部枚举
//! - [`model`]：`model/*.ts` 11 个数据模型
//! - [`parser`]：`ObjectDataParser`（JSON → model/IR）
//! - [`animation`]：`animation/*.ts`（阶段 3 占位，阶段 4 实现）
//! - [`armature`]：`armature/*.ts` 运行时层（阶段 3）
//! - [`event`]：`event/EventObject.ts`（阶段 3 最小骨架，阶段 5 收口）
//! - [`factory`]：`factory/BaseFactory.ts` 装配逻辑（阶段 3）

pub mod animation;
pub mod armature;
pub mod core;
pub mod event;
pub mod factory;
pub mod geom;
pub mod model;
pub mod parser;
