//! 对齐 `model/DisplayData.ts`（`GeometryData` + `DisplayData` 及 5 个子类 + `WeightData`）。

use std::sync::Arc;

use crate::core::enums::{DisplayType, binary_offset};
use crate::geom::point::Point;
use crate::geom::transform::Transform;
use crate::model::bounding_box::BoundingBoxData;
use crate::model::dragon_bones_data::RootInfo;
use crate::model::user_data::ActionData;

/// - 几何数据（`GeometryData.ts`，非 `BaseObject`，无变体）。
///
/// `root` 为 `DragonBonesData` 的共享根（D-1.1）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GeometryData {
    pub is_shared: bool,
    pub inherit_deform: bool,
    pub offset: i32,
    pub root: Option<Arc<RootInfo>>,
    pub weight: Option<Box<WeightData>>,
}

impl GeometryData {
    /// 对齐 `GeometryData.clear`。
    pub fn clear(&mut self) {
        self.is_shared = false;
        self.inherit_deform = false;
        self.offset = 0;
        self.root = None;
        self.weight = None;
    }

    /// 对齐 `GeometryData.shareFrom`。
    pub fn share_from(&mut self, value: &GeometryData) {
        self.is_shared = true;
        self.offset = value.offset;
        self.weight.clone_from(&value.weight);
    }

    /// 对齐 `GeometryData.vertexCount` getter。
    ///
    /// # Panics
    /// 当 `root` 未回填或 `offset` 越界时 panic，与 TS 直接读取 null/越界数组的行为一致。
    #[must_use]
    pub fn vertex_count(&self) -> i32 {
        let root = self.root.as_ref().expect("geometry root not set");
        root.ir.int_array[self.offset as usize + binary_offset::geometry::VERTEX_COUNT]
    }

    /// 对齐 `GeometryData.triangleCount` getter。
    ///
    /// # Panics
    /// 同 [`GeometryData::vertex_count`]。
    #[must_use]
    pub fn triangle_count(&self) -> i32 {
        let root = self.root.as_ref().expect("geometry root not set");
        root.ir.int_array[self.offset as usize + binary_offset::geometry::TRIANGLE_COUNT]
    }
}

/// - 显示数据（`DisplayData.ts` 的抽象基类）。
///
/// `parent` 为皮肤名引用；`type` 字段由 [`DisplayData::type_`] 按变体映射。
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayData {
    pub name: String,
    pub path: String,
    pub transform: Transform,
    pub parent: String,
    pub kind: DisplayKind,
}

/// 5 个子类的变体字段。
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayKind {
    /// `ImageDisplayData`。
    Image {
        pivot: Point,
        texture: Option<String>,
    },
    /// `ArmatureDisplayData`。
    Armature {
        inherit_animation: bool,
        actions: Vec<ActionData>,
        armature: Option<String>,
    },
    /// `MeshDisplayData`。
    Mesh {
        geometry: GeometryData,
        texture: Option<String>,
    },
    /// `BoundingBoxDisplayData`。
    BoundingBox {
        bounding_box: Option<Box<BoundingBoxData>>,
    },
    /// `PathDisplayData`。
    Path {
        closed: bool,
        constant_speed: bool,
        geometry: GeometryData,
        curve_lengths: Vec<f64>,
    },
}

impl Default for DisplayData {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            transform: Transform::default(),
            parent: String::new(),
            kind: DisplayKind::Image {
                pivot: Point::default(),
                texture: None,
            },
        }
    }
}

impl DisplayData {
    /// 对齐 TS 子类通过 `_onClear` 设置的 `type` 字段。
    #[must_use]
    pub fn type_(&self) -> DisplayType {
        match self.kind {
            DisplayKind::Image { .. } => DisplayType::Image,
            DisplayKind::Armature { .. } => DisplayType::Armature,
            DisplayKind::Mesh { .. } => DisplayType::Mesh,
            DisplayKind::BoundingBox { .. } => DisplayType::BoundingBox,
            DisplayKind::Path { .. } => DisplayType::Path,
        }
    }

    /// 回填几何数据持有的共享根（解析收尾时调用，见 `docs/guide/01-stage1.md` 任务 5）。
    pub(crate) fn set_root_info(&mut self, root: &Arc<RootInfo>) {
        match &mut self.kind {
            DisplayKind::Mesh { geometry, .. } | DisplayKind::Path { geometry, .. } => {
                geometry.root = Some(root.clone());
            }
            DisplayKind::Image { .. }
            | DisplayKind::Armature { .. }
            | DisplayKind::BoundingBox { .. } => {}
        }
    }
}

/// - 权重数据（`WeightData.ts`）。
///
/// `bones` 为骨名列表（D-1.1）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WeightData {
    pub count: i32,
    pub offset: i32,
    pub bones: Vec<String>,
}

impl WeightData {
    /// 对齐 `WeightData.addBone`。
    pub fn add_bone(&mut self, value: impl Into<String>) {
        self.bones.push(value.into());
    }
}
