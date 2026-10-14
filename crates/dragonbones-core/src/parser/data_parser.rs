//! 对齐 `parser/DataParser.ts`：数据键常量 + 版本门 + 字符串→枚举映射。

use serde_json::Value;

use crate::core::enums::{
    ActionType, AnimationBlendType, ArmatureType, BlendMode, BoneType, BoundingBoxType,
    DisplayType, PositionMode, RotateMode, SpacingMode,
};
use crate::parser::js_compat::get_string;

pub const DATA_VERSION_2_3: &str = "2.3";
pub const DATA_VERSION_3_0: &str = "3.0";
pub const DATA_VERSION_4_0: &str = "4.0";
pub const DATA_VERSION_4_5: &str = "4.5";
pub const DATA_VERSION_5_0: &str = "5.0";
pub const DATA_VERSION_5_5: &str = "5.5";
pub const DATA_VERSION_5_6: &str = "5.6";

/// 被接受的版本集合（`2.3`/`3.0` 不在内，见 `docs/guide/02-stage2.md` F2.1）。
pub const DATA_VERSIONS: [&str; 5] = [
    DATA_VERSION_4_0,
    DATA_VERSION_4_5,
    DATA_VERSION_5_0,
    DATA_VERSION_5_5,
    DATA_VERSION_5_6,
];

pub const TEXTURE_ATLAS: &str = "textureAtlas";
pub const SUB_TEXTURE: &str = "SubTexture";
pub const FORMAT: &str = "format";
pub const IMAGE_PATH: &str = "imagePath";
pub const WIDTH: &str = "width";
pub const HEIGHT: &str = "height";
pub const ROTATED: &str = "rotated";
pub const FRAME_X: &str = "frameX";
pub const FRAME_Y: &str = "frameY";
pub const FRAME_WIDTH: &str = "frameWidth";
pub const FRAME_HEIGHT: &str = "frameHeight";

pub const DRAGON_BONES: &str = "dragonBones";
pub const USER_DATA: &str = "userData";
pub const ARMATURE: &str = "armature";
pub const CANVAS: &str = "canvas";
pub const BONE: &str = "bone";
pub const SURFACE: &str = "surface";
pub const SLOT: &str = "slot";
pub const CONSTRAINT: &str = "constraint";
pub const SKIN: &str = "skin";
pub const DISPLAY: &str = "display";
pub const FRAME: &str = "frame";
pub const IK: &str = "ik";
pub const PATH_CONSTRAINT: &str = "path";

pub const ANIMATION: &str = "animation";
pub const TIMELINE: &str = "timeline";
pub const FFD: &str = "ffd";
pub const TRANSLATE_FRAME: &str = "translateFrame";
pub const ROTATE_FRAME: &str = "rotateFrame";
pub const SCALE_FRAME: &str = "scaleFrame";
pub const DISPLAY_FRAME: &str = "displayFrame";
pub const COLOR_FRAME: &str = "colorFrame";
pub const DEFAULT_ACTIONS: &str = "defaultActions";
pub const ACTIONS: &str = "actions";
pub const EVENTS: &str = "events";

pub const INTS: &str = "ints";
pub const FLOATS: &str = "floats";
pub const STRINGS: &str = "strings";

pub const TRANSFORM: &str = "transform";
pub const PIVOT: &str = "pivot";
pub const AABB: &str = "aabb";
pub const COLOR: &str = "color";

pub const VERSION: &str = "version";
pub const COMPATIBLE_VERSION: &str = "compatibleVersion";
pub const FRAME_RATE: &str = "frameRate";
pub const TYPE: &str = "type";
pub const SUB_TYPE: &str = "subType";
pub const NAME: &str = "name";
pub const PARENT: &str = "parent";
pub const TARGET: &str = "target";
pub const STAGE: &str = "stage";
pub const SHARE: &str = "share";
pub const PATH: &str = "path";
pub const LENGTH: &str = "length";
pub const DISPLAY_INDEX: &str = "displayIndex";
pub const Z_ORDER: &str = "zOrder";
pub const Z_INDEX: &str = "zIndex";
pub const BLEND_MODE: &str = "blendMode";
pub const INHERIT_TRANSLATION: &str = "inheritTranslation";
pub const INHERIT_ROTATION: &str = "inheritRotation";
pub const INHERIT_SCALE: &str = "inheritScale";
pub const INHERIT_REFLECTION: &str = "inheritReflection";
pub const INHERIT_ANIMATION: &str = "inheritAnimation";
pub const INHERIT_DEFORM: &str = "inheritDeform";
pub const SEGMENT_X: &str = "segmentX";
pub const SEGMENT_Y: &str = "segmentY";
pub const BEND_POSITIVE: &str = "bendPositive";
pub const CHAIN: &str = "chain";
pub const WEIGHT: &str = "weight";

pub const BLEND_TYPE: &str = "blendType";
pub const FADE_IN_TIME: &str = "fadeInTime";
pub const PLAY_TIMES: &str = "playTimes";
pub const SCALE: &str = "scale";
pub const OFFSET: &str = "offset";
pub const POSITION: &str = "position";
pub const DURATION: &str = "duration";
pub const TWEEN_EASING: &str = "tweenEasing";
pub const TWEEN_ROTATE: &str = "tweenRotate";
pub const TWEEN_SCALE: &str = "tweenScale";
pub const CLOCK_WISE: &str = "clockwise";
pub const CURVE: &str = "curve";
pub const SOUND: &str = "sound";
pub const EVENT: &str = "event";
pub const ACTION: &str = "action";

pub const X: &str = "x";
pub const Y: &str = "y";
pub const SKEW_X: &str = "skX";
pub const SKEW_Y: &str = "skY";
pub const SCALE_X: &str = "scX";
pub const SCALE_Y: &str = "scY";
pub const VALUE: &str = "value";
pub const ROTATE: &str = "rotate";
pub const SKEW: &str = "skew";
pub const ALPHA: &str = "alpha";

pub const ALPHA_OFFSET: &str = "aO";
pub const RED_OFFSET: &str = "rO";
pub const GREEN_OFFSET: &str = "gO";
pub const BLUE_OFFSET: &str = "bO";
pub const ALPHA_MULTIPLIER: &str = "aM";
pub const RED_MULTIPLIER: &str = "rM";
pub const GREEN_MULTIPLIER: &str = "gM";
pub const BLUE_MULTIPLIER: &str = "bM";

pub const UVS: &str = "uvs";
pub const VERTICES: &str = "vertices";
pub const TRIANGLES: &str = "triangles";
pub const WEIGHTS: &str = "weights";
pub const SLOT_POSE: &str = "slotPose";
pub const BONE_POSE: &str = "bonePose";

pub const BONES: &str = "bones";
pub const POSITION_MODE: &str = "positionMode";
pub const SPACING_MODE: &str = "spacingMode";
pub const ROTATE_MODE: &str = "rotateMode";
pub const SPACING: &str = "spacing";
pub const ROTATE_OFFSET: &str = "rotateOffset";
pub const ROTATE_MIX: &str = "rotateMix";
pub const TRANSLATE_MIX: &str = "translateMix";

pub const TARGET_DISPLAY: &str = "targetDisplay";
pub const CLOSED: &str = "closed";
pub const CONSTANT_SPEED: &str = "constantSpeed";
pub const VERTEX_COUNT: &str = "vertexCount";
pub const LENGTHS: &str = "lengths";

pub const GOTO_AND_PLAY: &str = "gotoAndPlay";

pub const DEFAULT_NAME: &str = "default";

/// 版本门：`version` 或 `compatibleVersion` 命中 [`DATA_VERSIONS`] 即接受。
#[must_use]
pub fn is_supported_version(raw: &Value) -> bool {
    let version = get_string(raw, VERSION, "");
    let compatible_version = get_string(raw, COMPATIBLE_VERSION, "");
    DATA_VERSIONS.contains(&version.as_str())
        || DATA_VERSIONS.contains(&compatible_version.as_str())
}

#[must_use]
pub fn get_armature_type(value: &str) -> ArmatureType {
    match value.to_lowercase().as_str() {
        "stage" => ArmatureType::Stage,
        "movieclip" => ArmatureType::MovieClip,
        _ => ArmatureType::Armature,
    }
}

#[must_use]
pub fn get_bone_type(value: &str) -> BoneType {
    match value.to_lowercase().as_str() {
        "surface" => BoneType::Surface,
        _ => BoneType::Bone,
    }
}

#[must_use]
pub fn get_position_mode(value: &str) -> PositionMode {
    match value.to_lowercase().as_str() {
        "fixed" => PositionMode::Fixed,
        _ => PositionMode::Percent,
    }
}

#[must_use]
pub fn get_spacing_mode(value: &str) -> SpacingMode {
    match value.to_lowercase().as_str() {
        "percent" => SpacingMode::Percent,
        "fixed" => SpacingMode::Fixed,
        _ => SpacingMode::Length,
    }
}

#[must_use]
pub fn get_rotate_mode(value: &str) -> RotateMode {
    match value.to_lowercase().as_str() {
        "tangent" => RotateMode::Tangent,
        "chainscale" => RotateMode::ChainScale,
        _ => RotateMode::Chain,
    }
}

#[must_use]
pub fn get_display_type(value: &str) -> DisplayType {
    match value.to_lowercase().as_str() {
        "mesh" => DisplayType::Mesh,
        "armature" => DisplayType::Armature,
        "boundingbox" => DisplayType::BoundingBox,
        "path" => DisplayType::Path,
        _ => DisplayType::Image,
    }
}

#[must_use]
pub fn get_bounding_box_type(value: &str) -> BoundingBoxType {
    match value.to_lowercase().as_str() {
        "ellipse" => BoundingBoxType::Ellipse,
        "polygon" => BoundingBoxType::Polygon,
        _ => BoundingBoxType::Rectangle,
    }
}

#[must_use]
pub fn get_blend_mode(value: &str) -> BlendMode {
    match value.to_lowercase().as_str() {
        "add" => BlendMode::Add,
        "alpha" => BlendMode::Alpha,
        "darken" => BlendMode::Darken,
        "difference" => BlendMode::Difference,
        "erase" => BlendMode::Erase,
        "hardlight" => BlendMode::HardLight,
        "invert" => BlendMode::Invert,
        "layer" => BlendMode::Layer,
        "lighten" => BlendMode::Lighten,
        "multiply" => BlendMode::Multiply,
        "overlay" => BlendMode::Overlay,
        "screen" => BlendMode::Screen,
        "subtract" => BlendMode::Subtract,
        _ => BlendMode::Normal,
    }
}

#[must_use]
pub fn get_animation_blend_type(value: &str) -> AnimationBlendType {
    match value.to_lowercase().as_str() {
        "1d" => AnimationBlendType::E1D,
        _ => AnimationBlendType::None,
    }
}

#[must_use]
pub fn get_action_type(value: &str) -> ActionType {
    match value.to_lowercase().as_str() {
        "frame" => ActionType::Frame,
        "sound" => ActionType::Sound,
        _ => ActionType::Play,
    }
}
