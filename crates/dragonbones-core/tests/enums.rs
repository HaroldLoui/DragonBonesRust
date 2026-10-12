//! 枚举数值核对（P0-3）：全部枚举 + `BinaryOffset` 五组常量与
//! `core/DragonBones.ts:27-277` 逐项对齐。

use dragonbones_core::core::enums::*;

macro_rules! check {
    ($value:expr, $want:expr) => {
        assert_eq!($value as u32, $want, "{} = {}", stringify!($value), $want);
    };
}

#[test]
fn armature_type_values() {
    check!(ArmatureType::Armature, 0);
    check!(ArmatureType::MovieClip, 1);
    check!(ArmatureType::Stage, 2);
}

#[test]
fn bone_type_values() {
    check!(BoneType::Bone, 0);
    check!(BoneType::Surface, 1);
}

#[test]
fn display_type_values() {
    check!(DisplayType::Image, 0);
    check!(DisplayType::Armature, 1);
    check!(DisplayType::Mesh, 2);
    check!(DisplayType::BoundingBox, 3);
    check!(DisplayType::Path, 4);
}

#[test]
fn bounding_box_type_values() {
    check!(BoundingBoxType::Rectangle, 0);
    check!(BoundingBoxType::Ellipse, 1);
    check!(BoundingBoxType::Polygon, 2);
}

#[test]
fn action_type_values() {
    check!(ActionType::Play, 0);
    check!(ActionType::Frame, 10);
    check!(ActionType::Sound, 11);
}

#[test]
fn blend_mode_values() {
    check!(BlendMode::Normal, 0);
    check!(BlendMode::Add, 1);
    check!(BlendMode::Alpha, 2);
    check!(BlendMode::Darken, 3);
    check!(BlendMode::Difference, 4);
    check!(BlendMode::Erase, 5);
    check!(BlendMode::HardLight, 6);
    check!(BlendMode::Invert, 7);
    check!(BlendMode::Layer, 8);
    check!(BlendMode::Lighten, 9);
    check!(BlendMode::Multiply, 10);
    check!(BlendMode::Overlay, 11);
    check!(BlendMode::Screen, 12);
    check!(BlendMode::Subtract, 13);
}

#[test]
fn tween_type_values() {
    check!(TweenType::None, 0);
    check!(TweenType::Line, 1);
    check!(TweenType::Curve, 2);
    check!(TweenType::QuadIn, 3);
    check!(TweenType::QuadOut, 4);
    check!(TweenType::QuadInOut, 5);
}

#[test]
fn timeline_type_values() {
    check!(TimelineType::Action, 0);
    check!(TimelineType::ZOrder, 1);
    check!(TimelineType::BoneAll, 10);
    check!(TimelineType::BoneTranslate, 11);
    check!(TimelineType::BoneRotate, 12);
    check!(TimelineType::BoneScale, 13);
    check!(TimelineType::SlotDisplay, 20);
    check!(TimelineType::SlotColor, 21);
    check!(TimelineType::SlotDeform, 22);
    check!(TimelineType::SlotZIndex, 23);
    check!(TimelineType::SlotAlpha, 24);
    check!(TimelineType::IKConstraint, 30);
    check!(TimelineType::AnimationProgress, 40);
    check!(TimelineType::AnimationWeight, 41);
    check!(TimelineType::AnimationParameter, 42);
    check!(TimelineType::Surface, 50);
    check!(TimelineType::BoneAlpha, 60);
}

#[test]
fn offset_mode_values() {
    check!(OffsetMode::None, 0);
    check!(OffsetMode::Additive, 1);
    check!(OffsetMode::Override, 2);
}

#[test]
fn animation_fade_out_mode_values() {
    check!(AnimationFadeOutMode::SameLayer, 1);
    check!(AnimationFadeOutMode::SameGroup, 2);
    check!(AnimationFadeOutMode::SameLayerAndGroup, 3);
    check!(AnimationFadeOutMode::All, 4);
    check!(AnimationFadeOutMode::Single, 5);
}

#[test]
fn animation_blend_values() {
    check!(AnimationBlendType::None, 0);
    check!(AnimationBlendType::E1D, 1);
    check!(AnimationBlendMode::Additive, 0);
    check!(AnimationBlendMode::Override, 1);
}

#[test]
fn constraint_and_mode_values() {
    check!(ConstraintType::IK, 0);
    check!(ConstraintType::Path, 1);
    check!(PositionMode::Fixed, 0);
    check!(PositionMode::Percent, 1);
    check!(SpacingMode::Length, 0);
    check!(SpacingMode::Fixed, 1);
    check!(SpacingMode::Percent, 2);
    check!(RotateMode::Tangent, 0);
    check!(RotateMode::Chain, 1);
    check!(RotateMode::ChainScale, 2);
}

#[test]
fn binary_offset_values() {
    use dragonbones_core::core::enums::binary_offset::{deform, frame, geometry, timeline, weight};

    assert_eq!(weight::BONE_COUNT, 0);
    assert_eq!(weight::FLOAT_OFFSET, 1);
    assert_eq!(weight::BONE_INDICES, 2);

    assert_eq!(geometry::VERTEX_COUNT, 0);
    assert_eq!(geometry::TRIANGLE_COUNT, 1);
    assert_eq!(geometry::FLOAT_OFFSET, 2);
    assert_eq!(geometry::WEIGHT_OFFSET, 3);
    assert_eq!(geometry::VERTEX_INDICES, 4);

    assert_eq!(timeline::SCALE, 0);
    assert_eq!(timeline::OFFSET, 1);
    assert_eq!(timeline::KEY_FRAME_COUNT, 2);
    assert_eq!(timeline::FRAME_VALUE_COUNT, 3);
    assert_eq!(timeline::FRAME_VALUE_OFFSET, 4);
    assert_eq!(timeline::FRAME_OFFSET, 5);

    assert_eq!(frame::POSITION, 0);
    assert_eq!(frame::TWEEN_TYPE, 1);
    assert_eq!(frame::TWEEN_EASING_OR_CURVE_SAMPLE_COUNT, 2);
    assert_eq!(frame::CURVE_SAMPLES, 3);

    assert_eq!(deform::VERTEX_OFFSET, 0);
    assert_eq!(deform::COUNT, 1);
    assert_eq!(deform::VALUE_COUNT, 2);
    assert_eq!(deform::VALUE_OFFSET, 3);
    assert_eq!(deform::FLOAT_OFFSET, 4);
}
