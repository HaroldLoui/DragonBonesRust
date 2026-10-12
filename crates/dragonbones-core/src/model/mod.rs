//! 数据模型层，逐文件对齐 `model/*.ts`（11 个文件 → 11 个模块）。
//!
//! 反向/横向引用按 `docs/model_mapping.md` 的 D-1.1 表替换为
//! “根共享（`Arc<RootInfo>`）/ 名字 / 直接拥有”，不使用 `Rc<RefCell>`。

pub mod animation_config;
pub mod animation_data;
pub mod armature_data;
pub mod bounding_box;
pub mod canvas_data;
pub mod constraint_data;
pub mod display_data;
pub mod dragon_bones_data;
pub mod skin_data;
pub mod texture_atlas_data;
pub mod user_data;

pub use animation_config::AnimationConfig;
pub use animation_data::{AnimationData, TimelineData};
pub use armature_data::{ArmatureData, BoneData, BoneKind, SlotData};
pub use bounding_box::{BoundingBoxData, BoundingBoxKind};
pub use canvas_data::CanvasData;
pub use constraint_data::{ConstraintData, ConstraintKind};
pub use display_data::{DisplayData, DisplayKind, GeometryData, WeightData};
pub use dragon_bones_data::{DragonBonesData, IrArrays, RootInfo};
pub use skin_data::SkinData;
pub use texture_atlas_data::{TextureAtlasData, TextureData};
pub use user_data::{ActionData, UserData};
