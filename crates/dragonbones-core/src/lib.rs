//! DragonBones 运行时核心（纯逻辑，f64 全程，不依赖 Bevy）。
//!
//! 分阶段对齐 `DragonBonesJS/DragonBones/src/dragonBones/`：
//! - [`geom`]：`geom/*.ts` 5 个数学类
//! - [`core`]：`core/BaseObject`、`core/DragonBones` 静态部分与全部枚举
//! - [`model`]：`model/*.ts` 11 个数据模型

pub mod core;
pub mod geom;
pub mod model;
