//! Bevy adapter for `dragonbones-core`.
//!
//! This crate follows the "B2" path from
//! `docs/design/bevy_skew_transform.md`: core computes each slot's
//! `global_transform_matrix`; the adapter writes that affine matrix directly
//! into Bevy's `GlobalTransform` in `PostUpdate`, after Bevy's transform
//! propagation and before visibility/rendering consume it. No vertex baking is
//! needed for the transform itself, and skew is preserved exactly.
//!
//! Slots are drawn with the adapter's own [`DragonBonesMaterial`], a custom
//! `Material2d` shared per `(atlas page image, blend mode)`. It supports the
//! fixed-function DragonBones blend modes ([`DragonBonesBlend`]: Normal, Add,
//! Multiply, Screen); modes that need render-target reads (Overlay, …) fall back
//! to Normal for now.
//!
//! The public surface is intentionally small:
//! - [`DragonBonesPlugin`] registers assets, runtime and systems.
//! - [`DragonBonesArmature`] is spawned on an entity to request an armature.
//! - [`DragonBonesSlot`] is added to spawned slot entities by the adapter.

mod asset;
mod blend;
mod events;
mod hash;
mod material;
mod math;
mod player;
mod render;
mod runtime;
mod sync;

pub use asset::{AtlasAsset, AtlasAssetLoader, SkeletonAsset, SkeletonAssetLoader};
pub use blend::DragonBonesBlend;
pub use events::DragonBonesEvent;
pub use material::DragonBonesMaterial;
pub use math::{matrix_to_affine3a, slot_z};
pub use player::{DragonBonesAction, DragonBonesCommand, DragonBonesPlayConfig};
pub use render::{DragonBonesSlotVisual, SlotRenderKind};
pub use runtime::{
    DragonBonesArmature, DragonBonesChildArmature, DragonBonesInstance, DragonBonesPlugin,
    DragonBonesRuntime, DragonBonesSlot, DragonBonesSyncConfig, DragonBonesSystems,
};

pub mod prelude {
    pub use crate::{
        AtlasAsset, DragonBonesAction, DragonBonesArmature, DragonBonesBlend,
        DragonBonesChildArmature, DragonBonesCommand, DragonBonesEvent, DragonBonesInstance,
        DragonBonesMaterial, DragonBonesPlayConfig, DragonBonesPlugin, DragonBonesSlot,
        DragonBonesSlotVisual, DragonBonesSyncConfig, SkeletonAsset, SlotRenderKind,
    };
}
