//! 对齐 `model/ArmatureData.ts`（`ArmatureData` + `BoneData`/`SurfaceData` + `SlotData`）。

use std::sync::Arc;

use indexmap::IndexMap;

use crate::core::enums::{ArmatureType, BlendMode, BoneType};
use crate::geom::color::ColorTransform;
use crate::geom::rectangle::Rectangle;
use crate::geom::transform::Transform;
use crate::model::animation_data::AnimationData;
use crate::model::canvas_data::CanvasData;
use crate::model::constraint_data::ConstraintData;
use crate::model::display_data::{DisplayData, GeometryData};
use crate::model::dragon_bones_data::RootInfo;
use crate::model::skin_data::SkinData;
use crate::model::user_data::{ActionData, UserData};

/// - 骨骼数据（`BoneData.ts`）。
///
/// `parent` 为父骨名引用（D-1.1）；`type` 由 [`BoneData::type_`] 按 [`BoneKind`] 映射。
#[derive(Debug, Clone, PartialEq)]
pub struct BoneData {
    pub inherit_translation: bool,
    pub inherit_rotation: bool,
    pub inherit_scale: bool,
    pub inherit_reflection: bool,
    pub length: f64,
    pub alpha: f64,
    pub name: String,
    pub transform: Transform,
    pub user_data: Option<UserData>,
    pub parent: Option<String>,
    pub kind: BoneKind,
}

/// `BoneData` / `SurfaceData` 的变体字段。
#[derive(Debug, Clone, PartialEq)]
pub enum BoneKind {
    /// `BoneData`。
    Bone,
    /// `SurfaceData`。
    Surface {
        segment_x: i32,
        segment_y: i32,
        geometry: GeometryData,
    },
}

impl Default for BoneData {
    fn default() -> Self {
        Self {
            inherit_translation: false,
            inherit_rotation: false,
            inherit_scale: false,
            inherit_reflection: false,
            length: 0.0,
            alpha: 1.0,
            name: String::new(),
            transform: Transform::default(),
            user_data: None,
            parent: None,
            kind: BoneKind::Bone,
        }
    }
}

impl BoneData {
    /// 对齐 TS 子类通过 `_onClear` 设置的 `type` 字段。
    #[must_use]
    pub fn type_(&self) -> BoneType {
        match self.kind {
            BoneKind::Bone => BoneType::Bone,
            BoneKind::Surface { .. } => BoneType::Surface,
        }
    }
}

/// - 插槽数据（`SlotData.ts`）。
///
/// `parent` 为父骨名引用；`color` 在 TS 中 `_onClear` 置 null、由解析器赋
/// `createColor()`，Rust 用 [`ColorTransform`] 默认值（乘子 1、偏移 0）表示。
#[derive(Debug, Clone, PartialEq)]
pub struct SlotData {
    pub blend_mode: BlendMode,
    pub display_index: i32,
    pub z_order: i32,
    /// 对齐 TS `zIndex: number`（D-4.35 关闭：允许小数，排序按原值）。
    pub z_index: f64,
    pub alpha: f64,
    pub name: String,
    pub color: ColorTransform,
    pub user_data: Option<UserData>,
    pub parent: String,
}

impl Default for SlotData {
    fn default() -> Self {
        Self {
            blend_mode: BlendMode::Normal,
            display_index: 0,
            z_order: 0,
            z_index: 0.0,
            alpha: 1.0,
            name: String::new(),
            color: ColorTransform::default(),
            user_data: None,
            parent: String::new(),
        }
    }
}

impl SlotData {
    /// 对齐 `SlotData.DEFAULT_COLOR`。
    pub const DEFAULT_COLOR: ColorTransform = ColorTransform {
        alpha_multiplier: 1.0,
        red_multiplier: 1.0,
        green_multiplier: 1.0,
        blue_multiplier: 1.0,
        alpha_offset: 0.0,
        red_offset: 0.0,
        green_offset: 0.0,
        blue_offset: 0.0,
    };

    /// 对齐 `SlotData.createColor`。
    #[must_use]
    pub fn create_color() -> ColorTransform {
        ColorTransform::default()
    }
}

/// - 骨架数据（`ArmatureData.ts`）。
///
/// `parent` 为共享根（D-1.1）；`default_skin` / `default_animation` 存名字。
/// `sorted_bones` / `sorted_slots` 存名字（数据本体的拥有者在 `bones` / `slots`）。
#[derive(Debug, Clone, PartialEq)]
pub struct ArmatureData {
    /// TS 字段名 `type`。
    pub armature_type: ArmatureType,
    pub frame_rate: f64,
    pub cache_frame_rate: f64,
    pub scale: f64,
    pub name: String,
    pub aabb: Rectangle,
    pub animation_names: Vec<String>,
    pub sorted_bones: Vec<String>,
    pub sorted_slots: Vec<String>,
    pub default_actions: Vec<ActionData>,
    pub actions: Vec<ActionData>,
    pub bones: IndexMap<String, BoneData>,
    pub slots: IndexMap<String, SlotData>,
    pub constraints: IndexMap<String, ConstraintData>,
    pub skins: IndexMap<String, SkinData>,
    pub animations: IndexMap<String, AnimationData>,
    pub default_skin: Option<String>,
    pub default_animation: Option<String>,
    pub canvas: Option<CanvasData>,
    pub user_data: Option<UserData>,
    pub parent: Option<Arc<RootInfo>>,
}

impl Default for ArmatureData {
    fn default() -> Self {
        Self {
            armature_type: ArmatureType::Armature,
            frame_rate: 0.0,
            cache_frame_rate: 0.0,
            scale: 1.0,
            name: String::new(),
            aabb: Rectangle::default(),
            animation_names: Vec::new(),
            sorted_bones: Vec::new(),
            sorted_slots: Vec::new(),
            default_actions: Vec::new(),
            actions: Vec::new(),
            bones: IndexMap::new(),
            slots: IndexMap::new(),
            constraints: IndexMap::new(),
            skins: IndexMap::new(),
            animations: IndexMap::new(),
            default_skin: None,
            default_animation: None,
            canvas: None,
            user_data: None,
            parent: None,
        }
    }
}

impl ArmatureData {
    /// 对齐 `ArmatureData.sortBones`（环形游标 + 等约束/等父先入的拓扑排序）。
    ///
    /// TS 在非法（含环）数据下会死循环，这里加入防御性上界，仅在非法数据时提前退出。
    pub fn sort_bones(&mut self) {
        let total = self.sorted_bones.len();
        if total == 0 {
            return;
        }

        let sort_helper = self.sorted_bones.clone();
        let mut index = 0usize;
        let mut count = 0usize;
        self.sorted_bones.clear();

        let mut iterations = 0usize;
        let max_iterations = total * total + total;
        while count < total {
            if iterations >= max_iterations {
                break;
            }
            iterations += 1;

            let bone = sort_helper[index].clone();
            index += 1;
            if index >= total {
                index = 0;
            }

            if self.sorted_bones.contains(&bone) {
                continue; // Wait self.
            }

            let mut wait_constraint = false;
            for constraint in self.constraints.values() {
                if constraint.root == bone && !self.sorted_bones.contains(&constraint.target) {
                    wait_constraint = true;
                    break;
                }
            }
            if wait_constraint {
                continue; // Wait constraint.
            }

            if let Some(parent) = self.bones.get(&bone).and_then(|data| data.parent.clone())
                && !self.sorted_bones.contains(&parent)
            {
                continue; // Wait parent.
            }

            self.sorted_bones.push(bone);
            count += 1;
        }
    }

    /// 对齐 `ArmatureData.addBone`（重名忽略）。
    pub fn add_bone(&mut self, value: BoneData) {
        if self.bones.contains_key(&value.name) {
            log::warn!("Same bone: {}", value.name);
            return;
        }

        self.sorted_bones.push(value.name.clone());
        self.bones.insert(value.name.clone(), value);
    }

    /// 对齐 `ArmatureData.addSlot`（重名忽略）。
    pub fn add_slot(&mut self, value: SlotData) {
        if self.slots.contains_key(&value.name) {
            log::warn!("Same slot: {}", value.name);
            return;
        }

        self.sorted_slots.push(value.name.clone());
        self.slots.insert(value.name.clone(), value);
    }

    /// 对齐 `ArmatureData.addConstraint`（重名忽略）。
    pub fn add_constraint(&mut self, value: ConstraintData) {
        if self.constraints.contains_key(&value.name) {
            log::warn!("Same constraint: {}", value.name);
            return;
        }

        self.constraints.insert(value.name.clone(), value);
    }

    /// 对齐 `ArmatureData.addSkin`（重名忽略；默认皮肤逻辑保留）。
    pub fn add_skin(&mut self, mut value: SkinData) {
        if self.skins.contains_key(&value.name) {
            log::warn!("Same skin: {}", value.name);
            return;
        }

        value.parent.clone_from(&self.name);
        if self.default_skin.is_none() {
            self.default_skin = Some(value.name.clone());
        }
        if value.name == "default" {
            self.default_skin = Some(value.name.clone());
        }

        self.skins.insert(value.name.clone(), value);
    }

    /// 对齐 `ArmatureData.addAnimation`（重名忽略；默认动画逻辑保留）。
    pub fn add_animation(&mut self, mut value: AnimationData) {
        if self.animations.contains_key(&value.name) {
            log::warn!("Same animation: {}", value.name);
            return;
        }

        value.parent.clone_from(&self.name);
        if self.default_animation.is_none() {
            self.default_animation = Some(value.name.clone());
        }

        self.animation_names.push(value.name.clone());
        self.animations.insert(value.name.clone(), value);
    }

    /// 对齐 `ArmatureData.addAction`。
    pub fn add_action(&mut self, value: ActionData, is_default: bool) {
        if is_default {
            self.default_actions.push(value);
        } else {
            self.actions.push(value);
        }
    }

    /// 对齐 `ArmatureData.getBone`。
    #[must_use]
    pub fn get_bone(&self, bone_name: &str) -> Option<&BoneData> {
        self.bones.get(bone_name)
    }

    /// 对齐 `ArmatureData.getSlot`。
    #[must_use]
    pub fn get_slot(&self, slot_name: &str) -> Option<&SlotData> {
        self.slots.get(slot_name)
    }

    /// 对齐 `ArmatureData.getConstraint`。
    #[must_use]
    pub fn get_constraint(&self, constraint_name: &str) -> Option<&ConstraintData> {
        self.constraints.get(constraint_name)
    }

    /// 对齐 `ArmatureData.getSkin`。
    #[must_use]
    pub fn get_skin(&self, skin_name: &str) -> Option<&SkinData> {
        self.skins.get(skin_name)
    }

    /// 对齐 `ArmatureData.getMesh`。
    #[must_use]
    pub fn get_mesh(
        &self,
        skin_name: &str,
        slot_name: &str,
        mesh_name: &str,
    ) -> Option<&DisplayData> {
        self.get_skin(skin_name)?.get_display(slot_name, mesh_name)
    }

    /// 对齐 `ArmatureData.getAnimation`。
    #[must_use]
    pub fn get_animation(&self, animation_name: &str) -> Option<&AnimationData> {
        self.animations.get(animation_name)
    }

    /// 回填共享根：设置 `parent` 并同步所有几何数据的 `root`（解析收尾时调用）。
    pub fn set_root_info(&mut self, root: &Arc<RootInfo>) {
        self.parent = Some(root.clone());

        for bone in self.bones.values_mut() {
            if let BoneKind::Surface { geometry, .. } = &mut bone.kind {
                geometry.root = Some(root.clone());
            }
        }

        for skin in self.skins.values_mut() {
            for displays in skin.displays.values_mut() {
                for display in displays.iter_mut().flatten() {
                    display.set_root_info(root);
                }
            }
        }
    }
}
