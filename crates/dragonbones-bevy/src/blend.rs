//! Fixed-function blend states for DragonBones slot blend modes.
//!
//! DragonBones' `blendMode` is a per-slot rendering hint. Core exposes the full
//! TS enum ([`BlendMode`]), but most of its variants need reads of the render
//! target inside the fragment shader (overlay, darken, lighten, difference,
//! hardLight, …), which the fixed-function blend unit cannot express.
//!
//! This module maps the subset that *can* be expressed as a
//! `wgpu`/Bevy [`BlendState`] onto a small, hashable enum that doubles as the
//! material pipeline key (see [`crate::material::DragonBonesMaterial`]):
//!
//! | DragonBones   | color src | color dst              | formula (S = src, D = dst) |
//! |---------------|-----------|------------------------|----------------------------|
//! | `Normal`      | `SrcAlpha`| `OneMinusSrcAlpha`     | `S·Sa + D·(1 − Sa)`        |
//! | `Add`         | `SrcAlpha`| `One`                  | `S·Sa + D`                 |
//! | `Multiply`    | `Dst`     | `OneMinusSrcAlpha`     | `S·D + D·(1 − Sa)`         |
//! | `Screen`      | `One`     | `OneMinusSrcColor`     | `S + D·(1 − S)`            |
//!
//! All four use `BlendOperation::Add` and standard source-over alpha
//! (`BlendComponent::OVER`), matching the formulas used by Pixi and by the
//! official DragonBones runtimes.
//!
//! Caveat: Bevy's 2D pipeline feeds *straight* (non-premultiplied) alpha to the
//! blend unit, while Pixi's reference blend states assume premultiplied colors.
//! `Normal` and `Add` are exact in both conventions; `Multiply` and `Screen`
//! match the reference only for fully opaque slots, so their soft alpha edges
//! are an approximation until a premultiplied shader path exists.
//!
//! Every other core variant (including `Overlay`, which the optimization
//! roadmap calls out as needing a custom shader) falls back to `Normal`:
//! rendering those slots with normal alpha blending is what the adapter did
//! before blend modes were supported at all.

use dragonbones_core::core::enums::BlendMode;

use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};

/// The subset of DragonBones blend modes the adapter can render with the
/// fixed-function blend unit.
///
/// Also used as the pipeline key (`AsBindGroup::Data`) of
/// [`crate::material::DragonBonesMaterial`], so the enum must be `Clone`,
/// `Hash`, `PartialEq`/`Eq`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DragonBonesBlend {
    /// Standard source-over alpha blending (`BlendMode::Normal`).
    #[default]
    Normal,
    /// Additive blending (`BlendMode::Add`).
    Add,
    /// Multiplicative blending (`BlendMode::Multiply`).
    Multiply,
    /// Screen blending (`BlendMode::Screen`).
    Screen,
}

impl DragonBonesBlend {
    /// Every blend mode the adapter renders natively.
    pub const ALL: [Self; 4] = [Self::Normal, Self::Add, Self::Multiply, Self::Screen];

    /// Maps a core [`BlendMode`] onto the supported subset.
    ///
    /// Unsupported modes (overlay, darken, lighten, hardLight, difference,
    /// subtract, invert, erase, alpha, layer) fall back to
    /// [`DragonBonesBlend::Normal`]; see the module docs for why.
    pub fn from_core(mode: BlendMode) -> Self {
        match mode {
            BlendMode::Normal => Self::Normal,
            BlendMode::Add => Self::Add,
            BlendMode::Multiply => Self::Multiply,
            BlendMode::Screen => Self::Screen,
            _ => Self::Normal,
        }
    }

    /// The `wgpu` blend state used by this mode in the material's
    /// `specialize` pass.
    pub(crate) fn blend_state(self) -> BlendState {
        let color = match self {
            Self::Normal => BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            Self::Add => BlendComponent {
                src_factor: BlendFactor::SrcAlpha,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            Self::Multiply => BlendComponent {
                src_factor: BlendFactor::Dst,
                dst_factor: BlendFactor::OneMinusSrcAlpha,
                operation: BlendOperation::Add,
            },
            Self::Screen => BlendComponent {
                src_factor: BlendFactor::One,
                // `BlendFactor` is component-wise: `Src` is `S.rgb` for the
                // color channels, i.e. `1 - S` here is `1 - S.rgb`.
                dst_factor: BlendFactor::OneMinusSrc,
                operation: BlendOperation::Add,
            },
        };
        // Alpha always accumulates source-over, independently of the color
        // channels, so alpha-blended and additive slots compose predictably
        // into the transparent phase's target.
        BlendState {
            color,
            alpha: BlendComponent::OVER,
        }
    }
}

impl From<BlendMode> for DragonBonesBlend {
    fn from(mode: BlendMode) -> Self {
        Self::from_core(mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_blend_modes_map_one_to_one() {
        assert_eq!(
            DragonBonesBlend::from_core(BlendMode::Normal),
            DragonBonesBlend::Normal
        );
        assert_eq!(
            DragonBonesBlend::from_core(BlendMode::Add),
            DragonBonesBlend::Add
        );
        assert_eq!(
            DragonBonesBlend::from_core(BlendMode::Multiply),
            DragonBonesBlend::Multiply
        );
        assert_eq!(
            DragonBonesBlend::from_core(BlendMode::Screen),
            DragonBonesBlend::Screen
        );
    }

    #[test]
    fn unsupported_blend_modes_fall_back_to_normal() {
        // Overlay needs render-target reads; the adapter deliberately renders
        // it as Normal until a shader-based path exists.
        for mode in [
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::HardLight,
            BlendMode::Difference,
            BlendMode::Subtract,
            BlendMode::Invert,
            BlendMode::Erase,
            BlendMode::Alpha,
            BlendMode::Layer,
        ] {
            assert_eq!(
                DragonBonesBlend::from_core(mode),
                DragonBonesBlend::Normal,
                "{mode:?} must fall back to Normal"
            );
        }
    }

    #[test]
    fn blend_states_use_the_documented_factors() {
        let normal = DragonBonesBlend::Normal.blend_state();
        assert_eq!(normal, BlendState::ALPHA_BLENDING);
        assert_eq!(normal.alpha, BlendComponent::OVER);

        let add = DragonBonesBlend::Add.blend_state();
        assert_eq!(add.color.src_factor, BlendFactor::SrcAlpha);
        assert_eq!(add.color.dst_factor, BlendFactor::One);

        let multiply = DragonBonesBlend::Multiply.blend_state();
        assert_eq!(multiply.color.src_factor, BlendFactor::Dst);
        assert_eq!(multiply.color.dst_factor, BlendFactor::OneMinusSrcAlpha);

        let screen = DragonBonesBlend::Screen.blend_state();
        assert_eq!(screen.color.src_factor, BlendFactor::One);
        assert_eq!(screen.color.dst_factor, BlendFactor::OneMinusSrc);

        for blend in DragonBonesBlend::ALL {
            assert_eq!(
                blend.blend_state().color.operation,
                BlendOperation::Add,
                "{blend:?} must use additive blending"
            );
        }
    }
}
