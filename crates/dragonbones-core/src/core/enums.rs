//! 枚举定义，逐项对齐 `core/DragonBones.ts:27-277`。
//!
//! - 所有枚举带显式判别值 + `#[repr(u32)]`，便于与 JSON 中的数字形式直接互转。
//! - `BinaryOffset` 在 TS 里是五组共用、判别值大量重复的 `const enum`，直接翻成
//!   单个 Rust enum 会触发 E0081（判别值重复），因此拆成五组 `usize` 模块常量。

/// `BinaryOffset` 五组数组下标常量（`DragonBones.ts:27-55`）。
pub mod binary_offset {
    /// WeightData 数组布局。
    pub mod weight {
        pub const BONE_COUNT: usize = 0;
        pub const FLOAT_OFFSET: usize = 1;
        pub const BONE_INDICES: usize = 2;
    }

    /// GeometryData 数组布局。
    pub mod geometry {
        pub const VERTEX_COUNT: usize = 0;
        pub const TRIANGLE_COUNT: usize = 1;
        pub const FLOAT_OFFSET: usize = 2;
        pub const WEIGHT_OFFSET: usize = 3;
        pub const VERTEX_INDICES: usize = 4;
    }

    /// TimelineData 数组布局。
    pub mod timeline {
        pub const SCALE: usize = 0;
        pub const OFFSET: usize = 1;
        pub const KEY_FRAME_COUNT: usize = 2;
        pub const FRAME_VALUE_COUNT: usize = 3;
        pub const FRAME_VALUE_OFFSET: usize = 4;
        pub const FRAME_OFFSET: usize = 5;
    }

    /// Frame 数组布局。
    pub mod frame {
        pub const POSITION: usize = 0;
        pub const TWEEN_TYPE: usize = 1;
        pub const TWEEN_EASING_OR_CURVE_SAMPLE_COUNT: usize = 2;
        pub const CURVE_SAMPLES: usize = 3;
    }

    /// Deform 数组布局。
    pub mod deform {
        pub const VERTEX_OFFSET: usize = 0;
        pub const COUNT: usize = 1;
        pub const VALUE_COUNT: usize = 2;
        pub const VALUE_OFFSET: usize = 3;
        pub const FLOAT_OFFSET: usize = 4;
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmatureType {
    Armature = 0,
    MovieClip = 1,
    Stage = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoneType {
    Bone = 0,
    Surface = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayType {
    Image = 0,
    Armature = 1,
    Mesh = 2,
    BoundingBox = 3,
    Path = 4,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundingBoxType {
    Rectangle = 0,
    Ellipse = 1,
    Polygon = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    Play = 0,
    Frame = 10,
    Sound = 11,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Normal = 0,
    Add = 1,
    Alpha = 2,
    Darken = 3,
    Difference = 4,
    Erase = 5,
    HardLight = 6,
    Invert = 7,
    Layer = 8,
    Lighten = 9,
    Multiply = 10,
    Overlay = 11,
    Screen = 12,
    Subtract = 13,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TweenType {
    None = 0,
    Line = 1,
    Curve = 2,
    QuadIn = 3,
    QuadOut = 4,
    QuadInOut = 5,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineType {
    Action = 0,
    ZOrder = 1,

    BoneAll = 10,
    BoneTranslate = 11,
    BoneRotate = 12,
    BoneScale = 13,

    SlotDisplay = 20,
    SlotColor = 21,
    SlotDeform = 22,
    SlotZIndex = 23,
    SlotAlpha = 24,

    IKConstraint = 30,

    AnimationProgress = 40,
    AnimationWeight = 41,
    AnimationParameter = 42,

    Surface = 50,
    BoneAlpha = 60,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OffsetMode {
    None = 0,
    Additive = 1,
    Override = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationFadeOutMode {
    SameLayer = 1,
    SameGroup = 2,
    SameLayerAndGroup = 3,
    All = 4,
    Single = 5,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationBlendType {
    None = 0,
    E1D = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationBlendMode {
    Additive = 0,
    Override = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintType {
    IK = 0,
    Path = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionMode {
    Fixed = 0,
    Percent = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpacingMode {
    Length = 0,
    Fixed = 1,
    Percent = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RotateMode {
    Tangent = 0,
    Chain = 1,
    ChainScale = 2,
}
