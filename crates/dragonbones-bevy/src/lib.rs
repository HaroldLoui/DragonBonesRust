//! Bevy adapter for `dragonbones-core`.
//!
//! This crate follows the "B2" path from
//! `docs/design/bevy_skew_transform.md`: core computes each slot's
//! `global_transform_matrix`; the adapter writes that affine matrix directly
//! into Bevy's `GlobalTransform` in `PostUpdate`, after Bevy's transform
//! propagation and before visibility/rendering consume it. No vertex baking is
//! needed for the transform itself, and skew is preserved exactly.
//!
//! The public surface is intentionally small:
//! - [`DragonBonesPlugin`] registers assets, runtime and systems.
//! - [`DragonBonesArmature`] is spawned on an entity to request an armature.
//! - [`DragonBonesSlot`] is added to spawned slot entities by the adapter.

mod asset;
mod math;
mod render;
mod runtime;
mod sync;

pub use asset::{AtlasAsset, AtlasAssetLoader, SkeletonAsset, SkeletonAssetLoader};
pub use render::{DragonBonesSlotVisual, SlotRenderKind};
pub use runtime::{
    DragonBonesArmature, DragonBonesInstance, DragonBonesPlugin, DragonBonesRuntime,
    DragonBonesSlot, DragonBonesSystems,
};

pub mod prelude {
    pub use crate::{
        AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesPlugin, DragonBonesSlot,
        DragonBonesSlotVisual, SkeletonAsset, SlotRenderKind,
    };
}
