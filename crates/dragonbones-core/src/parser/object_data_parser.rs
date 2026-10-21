//! 对齐 `parser/ObjectDataParser.ts`（2436 行）：JSON → 内部扁平 IR。
//!
//! 逐函数、逐分支镜像 TS 实现；所有 JS 数值语义经 [`crate::parser::js_compat`]。
//! 关键不变量见 `docs/guide/02-stage2.md` 与 `docs/model_mapping.md` §4.1。

use std::collections::HashMap;

use serde_json::Value;

use crate::core::enums::{
    ActionType, BlendMode, BoneType, DisplayType, TimelineType, TweenType, binary_offset,
};
use crate::geom::color::ColorTransform;
use crate::geom::matrix::Matrix;
use crate::geom::point::Point;
use crate::geom::transform::{DEG_RAD, PI_D, Transform, normalize_radian};
use crate::model::animation_data::{AnimationData, TimelineData};
use crate::model::armature_data::{ArmatureData, BoneData, BoneKind, SlotData};
use crate::model::bounding_box::{BoundingBoxData, BoundingBoxKind};
use crate::model::canvas_data::CanvasData;
use crate::model::constraint_data::{ConstraintData, ConstraintKind};
use crate::model::display_data::{DisplayData, DisplayKind, GeometryData, WeightData};
use crate::model::dragon_bones_data::{DragonBonesData, IrArrays};
use crate::model::skin_data::SkinData;
use crate::model::texture_atlas_data::{TextureAtlasData, TextureData};
use crate::model::user_data::{ActionData, UserData};
use crate::parser::binary_data_parser::BinaryArrays;
use crate::parser::data_parser::*;
use crate::parser::js_compat::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameValueType {
    Step,
    Int,
    Float,
}

/// 帧解析器分发（TS 用方法引用，Rust 用 enum + match，见指南任务 6.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum FrameKind {
    Frame,
    Tween,
    SingleValue,
    DoubleValue,
    BoneAll,
    BoneTranslate,
    BoneRotate,
    BoneScale,
    SlotDisplay,
    SlotColor,
    SlotDeform,
    Deform,
    ZOrder,
    Action,
    Ik,
}

/// TS 的帧既可能是 JSON 对象，也可能是内部 `ActionFrame`。
#[derive(Clone, Copy)]
enum FrameRef<'a> {
    Json(&'a Value),
    Action(&'a ActionFrame),
}

/// 对齐 `ActionFrame`。
#[derive(Debug, Clone, Default)]
struct ActionFrame {
    frame_start: i32,
    actions: Vec<i32>,
}

/// 共享网格延迟链接记录（对应 `_cacheRawMeshes` / `_cacheMeshes`）。
struct ShareSpec {
    this_skin: String,
    this_slot: String,
    this_mesh: String,
    target_skin: String,
    target_mesh: String,
}

/// 对齐 `ObjectDataParser`。
pub struct ObjectDataParser {
    raw_bones: Vec<String>,
    data: Option<DragonBonesData>,
    armature: Option<ArmatureData>,
    bone: Option<String>,
    slot: Option<String>,
    skin: Option<SkinData>,
    animation: Option<AnimationData>,
    timeline: Option<TimelineData>,

    frame_value_type: FrameValueType,
    default_color_offset: i32,
    prev_clockwise: i32,
    prev_rotation: f64,
    frame_default_value: f64,
    frame_value_scale: f64,

    help_transform: Transform,
    help_color: ColorTransform,
    help_point: Point,
    help_matrix_a: Matrix,
    help_matrix_b: Matrix,

    int_array: Vec<i32>,
    float_array: Vec<f64>,
    frame_int_array: Vec<i32>,
    frame_float_array: Vec<f64>,
    frame_array: Vec<i32>,
    timeline_array: Vec<i32>,
    color_array: Vec<i32>,
    frame_indices: Vec<i32>,

    action_frames: Vec<ActionFrame>,
    weight_slot_pose: HashMap<String, Vec<f64>>,
    weight_bone_poses: HashMap<String, Vec<f64>>,
    cache_bones: HashMap<String, Vec<String>>,
    slot_child_actions: HashMap<String, Vec<ActionData>>,
    share_specs: Vec<ShareSpec>,

    /// 当前 deform 目标几何信息（替代 TS `_geometry` / `_mesh` 的直接引用）。
    deform_offset: i32,
    deform_weight: Option<WeightData>,
    deform_mesh_name: Option<String>,

    raw_texture_atlases: Option<Vec<Value>>,
    raw_texture_atlas_index: usize,

    /// dbbin 模式：`Some` 时 `parse_array/parse_animation/parse_geometry` 走二进制分支。
    binary: Option<BinaryArrays>,
}

impl Default for ObjectDataParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectDataParser {
    #[must_use]
    pub fn new() -> Self {
        Self {
            raw_bones: Vec::new(),
            data: None,
            armature: None,
            bone: None,
            slot: None,
            skin: None,
            animation: None,
            timeline: None,
            frame_value_type: FrameValueType::Step,
            default_color_offset: -1,
            prev_clockwise: 0,
            prev_rotation: 0.0,
            frame_default_value: 0.0,
            frame_value_scale: 1.0,
            help_transform: Transform::default(),
            help_color: ColorTransform::default(),
            help_point: Point::default(),
            help_matrix_a: Matrix::default(),
            help_matrix_b: Matrix::default(),
            int_array: Vec::new(),
            float_array: Vec::new(),
            frame_int_array: Vec::new(),
            frame_float_array: Vec::new(),
            frame_array: Vec::new(),
            timeline_array: Vec::new(),
            color_array: Vec::new(),
            frame_indices: Vec::new(),
            action_frames: Vec::new(),
            weight_slot_pose: HashMap::new(),
            weight_bone_poses: HashMap::new(),
            cache_bones: HashMap::new(),
            slot_child_actions: HashMap::new(),
            share_specs: Vec::new(),
            deform_offset: 0,
            deform_weight: None,
            deform_mesh_name: None,
            raw_texture_atlases: None,
            raw_texture_atlas_index: 0,
            binary: None,
        }
    }

    // ---- IR buffer helpers（镜像 JS 数组的自动扩容 + 显式 length +=）----

    fn iset(&mut self, i: usize, v: i32) {
        if i >= self.int_array.len() {
            self.int_array.resize(i + 1, 0);
        }
        self.int_array[i] = v;
    }

    fn fset(&mut self, i: usize, v: f64) {
        if i >= self.float_array.len() {
            self.float_array.resize(i + 1, 0.0);
        }
        self.float_array[i] = v;
    }

    fn fintset(&mut self, i: usize, v: i32) {
        if i >= self.frame_int_array.len() {
            self.frame_int_array.resize(i + 1, 0);
        }
        self.frame_int_array[i] = v;
    }

    fn ffloatset(&mut self, i: usize, v: f64) {
        if i >= self.frame_float_array.len() {
            self.frame_float_array.resize(i + 1, 0.0);
        }
        self.frame_float_array[i] = v;
    }

    fn frameset(&mut self, i: usize, v: i32) {
        if i >= self.frame_array.len() {
            self.frame_array.resize(i + 1, 0);
        }
        self.frame_array[i] = v;
    }

    fn tlset(&mut self, i: usize, v: i32) {
        if i >= self.timeline_array.len() {
            self.timeline_array.resize(i + 1, 0);
        }
        self.timeline_array[i] = v;
    }

    fn colset(&mut self, i: usize, v: i32) {
        if i >= self.color_array.len() {
            self.color_array.resize(i + 1, 0);
        }
        self.color_array[i] = v;
    }

    fn idxset(&mut self, i: usize, v: i32) {
        if i >= self.frame_indices.len() {
            self.frame_indices.resize(i + 1, 0);
        }
        self.frame_indices[i] = v;
    }

    fn reserve_int(&mut self, n: usize) {
        let l = self.int_array.len();
        self.int_array.resize(l + n, 0);
    }

    fn reserve_float(&mut self, n: usize) {
        let l = self.float_array.len();
        self.float_array.resize(l + n, 0.0);
    }

    fn reserve_frame_int(&mut self, n: usize) {
        let l = self.frame_int_array.len();
        self.frame_int_array.resize(l + n, 0);
    }

    fn reserve_frame_float(&mut self, n: usize) {
        let l = self.frame_float_array.len();
        self.frame_float_array.resize(l + n, 0.0);
    }

    fn reserve_frame(&mut self, n: usize) {
        let l = self.frame_array.len();
        self.frame_array.resize(l + n, 0);
    }

    fn reserve_timeline(&mut self, n: usize) {
        let l = self.timeline_array.len();
        self.timeline_array.resize(l + n, 0);
    }

    fn reserve_color(&mut self, n: usize) {
        let l = self.color_array.len();
        self.color_array.resize(l + n, 0);
    }

    // ---- 入口 1：骨架数据 ----

    /// 对齐 `ObjectDataParser.parseDragonBonesData`（JSON 路径）。
    pub fn parse_dragon_bones_data(&mut self, raw: &Value, scale: f64) -> Option<DragonBonesData> {
        self.binary = None;
        self.parse_dragon_bones_data_impl(raw, scale)
    }

    /// 对齐 `BaseFactory.parseDragonBonesData(ArrayBuffer)` → `BinaryDataParser`。
    ///
    /// 只替换入口与三个解析分支；骨架/皮肤/显示/约束的静态结构复用 JSON 解析。
    pub fn parse_binary_dragon_bones_data(
        &mut self,
        bytes: &[u8],
        scale: f64,
    ) -> Option<DragonBonesData> {
        let binary = BinaryArrays::decode(bytes)?;
        let header = binary.header.clone();
        self.binary = Some(binary);
        self.frame_indices.clear();
        self.parse_dragon_bones_data_impl(&header, scale)
    }

    fn parse_dragon_bones_data_impl(
        &mut self,
        raw: &Value,
        scale: f64,
    ) -> Option<DragonBonesData> {
        if !is_supported_version(raw) {
            return None;
        }

        let version = get_string(raw, VERSION, "");
        let name = get_string(raw, NAME, "");
        let mut frame_rate = get_number(raw, FRAME_RATE, 24.0);
        if frame_rate == 0.0 {
            frame_rate = 24.0;
        }

        let data = DragonBonesData {
            version,
            name,
            frame_rate,
            ..Default::default()
        };

        // armature 分支不再提前 return（TS 亦无早退）：官方在 `if (ARMATURE in rawData)`
        // 块**之后**无条件捕获内嵌图集（ObjectDataParser.ts:2336-2338），两路汇合到
        // 同一个 return——Rust 同构为 if/else 汇合后再捕获，避免 armature 键在时漏捕。
        let out = if has_key(raw, ARMATURE) {
            self.data = Some(data);
            self.parse_array();
            self.frame_indices.clear();
            self.frame_value_type = FrameValueType::Step;
            self.default_color_offset = -1;
            self.prev_clockwise = 0;
            self.prev_rotation = 0.0;
            self.raw_bones.clear();
            self.cache_bones.clear();
            self.share_specs.clear();

            let raw_armatures = raw[ARMATURE].as_array().cloned().unwrap_or_default();
            for raw_armature in &raw_armatures {
                let armature = self.parse_armature(raw_armature, scale);
                self.data.as_mut().unwrap().add_armature(armature);
            }

            let mut data = self.data.take().unwrap();
            if let Some(binary) = self.binary.take() {
                // dbbin：数组已是最终 TypedArray 视图，不做 padding/量化。
                let ir = IrArrays {
                    int_array: binary.int_array,
                    float_array: binary.float_array,
                    frame_int_array: binary.frame_int_array,
                    frame_float_array: binary.frame_float_array,
                    frame_array: binary.frame_array,
                    timeline_array: binary.timeline_array,
                    color_array: binary.color_array,
                    frame_indices: std::mem::take(&mut self.frame_indices),
                };
                data.finalize_root(ir);
                data.binary = Some(binary.payload);
            } else {
                self.modify_array();
                let ir = self.build_ir();
                data.finalize_root(ir);
                data.pack_binary();
            }

            if has_key(raw, STAGE) {
                let stage_name = get_string(raw, STAGE, "");
                if data.get_armature(&stage_name).is_some() {
                    data.stage = Some(stage_name);
                }
            } else if !data.armature_names.is_empty() {
                data.stage = Some(data.armature_names[0].clone());
            }

            Some(data)
        } else {
            // 无 armature 时仍返回数据（TS 行为——`:2340 return data` 在 if 块外）。
            Some(data)
        };

        // TS:2336-2338：返回前**无条件**捕获内嵌 `textureAtlas`（armature 键在否无关）。
        // 漏掉这条 → BaseFactory::parse_dragon_bones_data 的内嵌图集 while 循环
        // （BaseFactory.ts:357-366）首圈即 break，含内嵌图集的 ske 图集静默丢失。
        if has_key(raw, TEXTURE_ATLAS) {
            self.raw_texture_atlases = raw[TEXTURE_ATLAS].as_array().cloned();
        }
        out
    }

    /// 对齐 `ObjectDataParser.parseTextureAtlasData`（简化 null 回退路径）。
    pub fn parse_texture_atlas_data(
        &mut self,
        raw: &Value,
        texture_atlas_data: &mut TextureAtlasData,
        scale: f64,
    ) -> bool {
        if raw.is_null() {
            if let Some(list) = self.raw_texture_atlases.clone()
                && self.raw_texture_atlas_index < list.len()
            {
                let item = list[self.raw_texture_atlas_index].clone();
                self.raw_texture_atlas_index += 1;
                let ok = self.parse_texture_atlas_data(&item, texture_atlas_data, scale);
                if self.raw_texture_atlas_index >= list.len() {
                    self.raw_texture_atlas_index = 0;
                    self.raw_texture_atlases = None;
                }
                return ok;
            }
            return false;
        }

        texture_atlas_data.width = get_number(raw, WIDTH, 0.0);
        texture_atlas_data.height = get_number(raw, HEIGHT, 0.0);
        texture_atlas_data.scale = if scale == 1.0 {
            1.0 / get_number(raw, SCALE, 1.0)
        } else {
            scale
        };
        texture_atlas_data.name = get_string(raw, NAME, "");
        texture_atlas_data.image_path = get_string(raw, IMAGE_PATH, "");

        if let Some(raw_textures) = raw.get(SUB_TEXTURE).and_then(Value::as_array) {
            for raw_texture in raw_textures {
                let frame_width = get_number(raw_texture, FRAME_WIDTH, -1.0);
                let frame_height = get_number(raw_texture, FRAME_HEIGHT, -1.0);

                let mut texture = TextureData {
                    rotated: get_boolean(raw_texture, ROTATED, false),
                    name: get_string(raw_texture, NAME, ""),
                    ..Default::default()
                };
                texture.region.x = get_number(raw_texture, X, 0.0);
                texture.region.y = get_number(raw_texture, Y, 0.0);
                texture.region.width = get_number(raw_texture, WIDTH, 0.0);
                texture.region.height = get_number(raw_texture, HEIGHT, 0.0);

                if frame_width > 0.0 && frame_height > 0.0 {
                    let frame = crate::geom::rectangle::Rectangle {
                        x: get_number(raw_texture, FRAME_X, 0.0),
                        y: get_number(raw_texture, FRAME_Y, 0.0),
                        width: frame_width,
                        height: frame_height,
                    };
                    texture.frame = Some(frame);
                }

                texture_atlas_data.add_texture(texture);
            }
        }

        true
    }

    fn parse_array(&mut self) {
        if let Some(binary) = self.binary.as_ref() {
            // dbbin：数组直接来自 TypedArray，保留精确长度（不 padding）。
            self.int_array.clone_from(&binary.int_array);
            self.float_array.clone_from(&binary.float_array);
            self.frame_int_array.clone_from(&binary.frame_int_array);
            self.frame_float_array.clone_from(&binary.frame_float_array);
            self.frame_array.clone_from(&binary.frame_array);
            self.timeline_array.clone_from(&binary.timeline_array);
            self.color_array.clone_from(&binary.color_array);
            return;
        }
        self.int_array.clear();
        self.float_array.clear();
        self.frame_int_array.clear();
        self.frame_float_array.clear();
        self.frame_array.clear();
        self.timeline_array.clear();
        self.color_array.clear();
    }

    /// 对齐 `_modifyArray` 的 padding（量化在 [`Self::build_ir`]）。
    fn modify_array(&mut self) {
        if !self.int_array.len().is_multiple_of(2) {
            self.int_array.push(0);
        }
        if !self.frame_int_array.len().is_multiple_of(2) {
            self.frame_int_array.push(0);
        }
        if !self.frame_array.len().is_multiple_of(2) {
            self.frame_array.push(0);
        }
        if !self.timeline_array.len().is_multiple_of(2) {
            self.timeline_array.push(0);
        }
        // 第 5 个 padding 分支：条件是 timelineArray.length、写 colorArray（F2.6，勿"修复"）。
        if !self.timeline_array.len().is_multiple_of(2) {
            self.color_array.push(0);
        }
    }

    /// 量化并产出 [`IrArrays`]（`docs/model_mapping.md` §4.1）。
    fn build_ir(&mut self) -> IrArrays {
        let int_array = self
            .int_array
            .iter()
            .map(|&v| js_store_i16(f64::from(v)))
            .collect();
        let frame_int_array = self
            .frame_int_array
            .iter()
            .map(|&v| js_store_i16(f64::from(v)))
            .collect();
        let color_array = self
            .color_array
            .iter()
            .map(|&v| js_store_i16(f64::from(v)))
            .collect();
        let frame_array = self
            .frame_array
            .iter()
            .map(|&v| js_store_i16(f64::from(v)))
            .collect();
        let timeline_array = self
            .timeline_array
            .iter()
            .map(|&v| js_store_u16(f64::from(v)))
            .collect();
        let float_array = self.float_array.iter().map(|&v| js_store_f32(v)).collect();
        let frame_float_array = self
            .frame_float_array
            .iter()
            .map(|&v| js_store_f32(v))
            .collect();

        IrArrays {
            int_array,
            float_array,
            frame_int_array,
            frame_float_array,
            frame_array,
            timeline_array,
            color_array,
            frame_indices: std::mem::take(&mut self.frame_indices),
        }
    }

    // ---- armature ----

    fn parse_armature(&mut self, raw: &Value, scale: f64) -> ArmatureData {
        let mut armature = ArmatureData {
            name: get_string(raw, NAME, ""),
            frame_rate: get_number(
                raw,
                FRAME_RATE,
                self.data.as_ref().map_or(24.0, |d| d.frame_rate),
            ),
            scale,
            ..Default::default()
        };

        if has_key(raw, TYPE) && raw[TYPE].is_string() {
            armature.armature_type = get_armature_type(&get_string(raw, TYPE, ""));
        } else {
            armature.armature_type = num_to_armature_type(get_number(
                raw,
                TYPE,
                crate::core::enums::ArmatureType::Armature as u32 as f64,
            ));
        }

        if armature.frame_rate == 0.0 {
            armature.frame_rate = 24.0;
        }

        self.armature = Some(armature);

        if has_key(raw, CANVAS) {
            let raw_canvas = &raw[CANVAS];
            let canvas = CanvasData {
                has_background: has_key(raw_canvas, COLOR),
                color: get_number(raw_canvas, COLOR, 0.0) as i32,
                x: get_number(raw_canvas, X, 0.0) * scale,
                y: get_number(raw_canvas, Y, 0.0) * scale,
                width: get_number(raw_canvas, WIDTH, 0.0) * scale,
                height: get_number(raw_canvas, HEIGHT, 0.0) * scale,
            };
            self.armature.as_mut().unwrap().canvas = Some(canvas);
        }

        if has_key(raw, AABB) {
            let raw_aabb = &raw[AABB];
            let a = self.armature.as_mut().unwrap();
            a.aabb.x = get_number(raw_aabb, X, 0.0) * scale;
            a.aabb.y = get_number(raw_aabb, Y, 0.0) * scale;
            a.aabb.width = get_number(raw_aabb, WIDTH, 0.0) * scale;
            a.aabb.height = get_number(raw_aabb, HEIGHT, 0.0) * scale;
        }

        if let Some(raw_bones) = raw.get(BONE).and_then(Value::as_array) {
            for raw_bone in raw_bones {
                let parent_name = get_string(raw_bone, PARENT, "");
                let mut bone = self.parse_bone(raw_bone);

                if !parent_name.is_empty() {
                    if self
                        .armature
                        .as_ref()
                        .unwrap()
                        .get_bone(&parent_name)
                        .is_some()
                    {
                        bone.parent = Some(parent_name.clone());
                    } else {
                        self.cache_bones
                            .entry(parent_name)
                            .or_default()
                            .push(bone.name.clone());
                    }
                }

                if let Some(children) = self.cache_bones.remove(&bone.name) {
                    for child in children {
                        if let Some(c) = self.armature.as_mut().unwrap().bones.get_mut(&child) {
                            c.parent = Some(bone.name.clone());
                        }
                    }
                }

                let name = bone.name.clone();
                self.armature.as_mut().unwrap().add_bone(bone);
                self.raw_bones.push(name);
            }
        }

        if let Some(raw_iks) = raw.get(IK).and_then(Value::as_array) {
            for raw_ik in raw_iks {
                if let Some(constraint) = self.parse_ik_constraint(raw_ik) {
                    self.armature.as_mut().unwrap().add_constraint(constraint);
                }
            }
        }

        self.armature.as_mut().unwrap().sort_bones();

        if let Some(raw_slots) = raw.get(SLOT).and_then(Value::as_array) {
            let mut z_order = 0.0;
            for raw_slot in raw_slots {
                let slot = self.parse_slot(raw_slot, z_order);
                self.armature.as_mut().unwrap().add_slot(slot);
                z_order += 1.0;
            }
        }

        if let Some(raw_skins) = raw.get(SKIN).and_then(Value::as_array) {
            for raw_skin in raw_skins {
                let skin = self.parse_skin(raw_skin);
                self.armature.as_mut().unwrap().add_skin(skin);
            }
        }

        if let Some(raw_paths) = raw.get(PATH_CONSTRAINT).and_then(Value::as_array) {
            for raw_path in raw_paths {
                if let Some(constraint) = self.parse_path_constraint(raw_path) {
                    self.armature.as_mut().unwrap().add_constraint(constraint);
                }
            }
        }

        self.link_shared_meshes();

        if let Some(raw_animations) = raw.get(ANIMATION).and_then(Value::as_array) {
            for raw_animation in raw_animations {
                let animation = self.parse_animation(raw_animation);
                self.armature.as_mut().unwrap().add_animation(animation);
            }
        }

        if has_key(raw, DEFAULT_ACTIONS) {
            let actions =
                self.parse_action_data(&raw[DEFAULT_ACTIONS], ActionType::Play, None, None);
            for action in actions {
                if action.action_type == ActionType::Play {
                    let name = action.name.clone();
                    self.armature.as_mut().unwrap().add_action(action, true);
                    if self
                        .armature
                        .as_ref()
                        .unwrap()
                        .get_animation(&name)
                        .is_some()
                    {
                        self.armature.as_mut().unwrap().default_animation = Some(name);
                    }
                } else {
                    self.armature.as_mut().unwrap().add_action(action, true);
                }
            }
        }

        if has_key(raw, ACTIONS) {
            let actions = self.parse_action_data(&raw[ACTIONS], ActionType::Play, None, None);
            for action in actions {
                self.armature.as_mut().unwrap().add_action(action, false);
            }
        }

        self.raw_bones.clear();
        self.cache_bones.clear();
        self.slot_child_actions.clear();
        self.weight_slot_pose.clear();
        self.weight_bone_poses.clear();

        self.armature.take().unwrap()
    }

    fn link_shared_meshes(&mut self) {
        for spec in &self.share_specs {
            let target = self
                .armature
                .as_ref()
                .and_then(|a| a.skins.get(&spec.target_skin))
                .and_then(|s| s.get_display("", &spec.target_mesh))
                .map(|d| &d.kind);
            let target_geometry = match target {
                Some(DisplayKind::Mesh { geometry, .. }) => Some(geometry.clone()),
                _ => None,
            };

            let Some(target_geometry) = target_geometry else {
                continue;
            };

            if let Some(armature) = self.armature.as_mut()
                && let Some(displays) = armature
                    .skins
                    .get_mut(&spec.this_skin)
                    .and_then(|s| s.displays.get_mut(&spec.this_slot))
            {
                for display in displays.iter_mut().flatten() {
                    if display.name == spec.this_mesh
                        && let DisplayKind::Mesh { geometry, .. } = &mut display.kind
                    {
                        geometry.share_from(&target_geometry);
                    }
                }
            }
        }
    }

    fn parse_bone(&mut self, raw: &Value) -> BoneData {
        let bone_type = if has_key(raw, TYPE) && raw[TYPE].is_string() {
            get_bone_type(&get_string(raw, TYPE, ""))
        } else {
            num_to_bone_type(get_number(raw, TYPE, BoneType::Bone as u32 as f64))
        };

        let scale = self.armature.as_ref().unwrap().scale;

        if bone_type == BoneType::Bone {
            let mut bone = BoneData {
                inherit_translation: get_boolean(raw, INHERIT_TRANSLATION, true),
                inherit_rotation: get_boolean(raw, INHERIT_ROTATION, true),
                inherit_scale: get_boolean(raw, INHERIT_SCALE, true),
                inherit_reflection: get_boolean(raw, INHERIT_REFLECTION, true),
                length: get_number(raw, LENGTH, 0.0) * scale,
                alpha: get_number(raw, ALPHA, 1.0),
                name: get_string(raw, NAME, ""),
                ..Default::default()
            };

            if has_key(raw, TRANSFORM) {
                parse_transform(&raw[TRANSFORM], &mut bone.transform, scale);
            }

            return bone;
        }

        let mut geometry = GeometryData::default();
        self.parse_geometry(raw, &mut geometry);

        BoneData {
            alpha: get_number(raw, ALPHA, 1.0),
            name: get_string(raw, NAME, ""),
            kind: BoneKind::Surface {
                segment_x: get_number(raw, SEGMENT_X, 0.0) as i32,
                segment_y: get_number(raw, SEGMENT_Y, 0.0) as i32,
                geometry,
            },
            ..Default::default()
        }
    }

    fn parse_ik_constraint(&mut self, raw: &Value) -> Option<ConstraintData> {
        let armature = self.armature.as_ref().unwrap();
        let bone_name = get_string(raw, BONE, "");
        armature.get_bone(&bone_name)?;
        let target_name = get_string(raw, TARGET, "");
        armature.get_bone(&target_name)?;

        let chain = get_number(raw, CHAIN, 0.0);
        let bone_parent = armature.get_bone(&bone_name).and_then(|b| b.parent.clone());

        let (root, bone) = match bone_parent {
            Some(parent) if chain > 0.0 => (parent, Some(bone_name)),
            _ => (bone_name, None),
        };

        Some(ConstraintData {
            order: 0,
            name: get_string(raw, NAME, ""),
            target: target_name,
            root,
            bone,
            kind: ConstraintKind::IK {
                scale_enabled: get_boolean(raw, SCALE, false),
                bend_positive: get_boolean(raw, BEND_POSITIVE, true),
                weight: get_number(raw, WEIGHT, 1.0),
            },
        })
    }

    fn parse_path_constraint(&mut self, raw: &Value) -> Option<ConstraintData> {
        let target_name = get_string(raw, TARGET, "");
        let default_skin = self.armature.as_ref().unwrap().default_skin.clone()?;
        let target_parent = self
            .armature
            .as_ref()
            .unwrap()
            .get_slot(&target_name)?
            .parent
            .clone();

        let target_display_name = get_string(raw, TARGET_DISPLAY, &target_name);
        let is_path = self
            .armature
            .as_ref()
            .unwrap()
            .get_skin(&default_skin)?
            .get_display(&target_name, &target_display_name)
            .map(|d| matches!(d.kind, DisplayKind::Path { .. }))
            .unwrap_or(false);
        if !is_path {
            return None;
        }

        let bone_names: Vec<String> = raw
            .get(BONES)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if bone_names.is_empty() {
            return None;
        }

        let mut bones = Vec::new();
        let mut root: Option<String> = None;
        for bone_name in bone_names {
            if self
                .armature
                .as_ref()
                .unwrap()
                .get_bone(&bone_name)
                .is_some()
            {
                if root.is_none() {
                    root = Some(bone_name.clone());
                }
                bones.push(bone_name);
            }
        }

        Some(ConstraintData {
            order: 0,
            name: get_string(raw, NAME, ""),
            target: target_parent,
            root: root.unwrap_or_default(),
            bone: None,
            kind: ConstraintKind::Path {
                path_slot: target_name,
                path_display: Some(target_display_name),
                bones,
                position_mode: get_position_mode(&get_string(raw, POSITION_MODE, "")),
                spacing_mode: get_spacing_mode(&get_string(raw, SPACING_MODE, "")),
                rotate_mode: get_rotate_mode(&get_string(raw, ROTATE_MODE, "")),
                position: get_number(raw, POSITION, 0.0),
                spacing: get_number(raw, SPACING, 0.0),
                rotate_offset: get_number(raw, ROTATE_OFFSET, 0.0),
                rotate_mix: get_number(raw, ROTATE_MIX, 1.0),
                translate_mix: get_number(raw, TRANSLATE_MIX, 1.0),
            },
        })
    }

    fn parse_slot(&mut self, raw: &Value, z_order: f64) -> SlotData {
        let blend_mode = if has_key(raw, BLEND_MODE) && raw[BLEND_MODE].is_string() {
            get_blend_mode(&get_string(raw, BLEND_MODE, ""))
        } else {
            num_to_blend_mode(get_number(raw, BLEND_MODE, BlendMode::Normal as u32 as f64))
        };

        let parent_name = get_string(raw, PARENT, "");
        let parent = if self
            .armature
            .as_ref()
            .unwrap()
            .get_bone(&parent_name)
            .is_some()
        {
            parent_name
        } else {
            String::new()
        };

        let color = if has_key(raw, COLOR) {
            let mut color = SlotData::create_color();
            parse_color_transform(&raw[COLOR], &mut color);
            color
        } else {
            SlotData::DEFAULT_COLOR
        };

        let slot = SlotData {
            display_index: get_number(raw, DISPLAY_INDEX, 0.0) as i32,
            z_order: z_order as i32,
            z_index: get_number(raw, Z_INDEX, 0.0),
            alpha: get_number(raw, ALPHA, 1.0),
            name: get_string(raw, NAME, ""),
            blend_mode,
            color,
            parent,
            ..Default::default()
        };

        if has_key(raw, ACTIONS) {
            let actions = self.parse_action_data(&raw[ACTIONS], ActionType::Play, None, None);
            self.slot_child_actions.insert(slot.name.clone(), actions);
        }

        slot
    }

    fn parse_skin(&mut self, raw: &Value) -> SkinData {
        let mut name = get_string(raw, NAME, DEFAULT_NAME);
        if name.is_empty() {
            name = DEFAULT_NAME.to_string();
        }

        let mut skin = SkinData {
            name,
            ..Default::default()
        };

        if let Some(raw_slots) = raw.get(SLOT).and_then(Value::as_array) {
            for raw_slot in raw_slots {
                let slot_name = get_string(raw_slot, NAME, "");
                if self
                    .armature
                    .as_ref()
                    .unwrap()
                    .get_slot(&slot_name)
                    .is_none()
                {
                    continue;
                }
                self.slot = Some(slot_name.clone());

                if let Some(raw_displays) = raw_slot.get(DISPLAY).and_then(Value::as_array) {
                    for raw_display in raw_displays {
                        let display = if raw_display.is_null() {
                            None
                        } else {
                            self.parse_display(raw_display)
                        };
                        skin.add_display(slot_name.clone(), display);
                    }
                }

                self.slot = None;
            }
        }

        skin
    }

    fn parse_display(&mut self, raw: &Value) -> Option<DisplayData> {
        let name = get_string(raw, NAME, "");
        let path = get_string(raw, PATH, "");

        let display_type = if has_key(raw, TYPE) && raw[TYPE].is_string() {
            get_display_type(&get_string(raw, TYPE, ""))
        } else {
            num_to_display_type(get_number(raw, TYPE, DisplayType::Image as u32 as f64))
        };

        let mut display = DisplayData {
            name: name.clone(),
            path: if !path.is_empty() { path } else { name.clone() },
            ..Default::default()
        };

        match display_type {
            DisplayType::Image => {
                let mut pivot = Point::new(0.5, 0.5);
                if has_key(raw, PIVOT) {
                    let raw_pivot = &raw[PIVOT];
                    pivot.x = get_number(raw_pivot, X, 0.0);
                    pivot.y = get_number(raw_pivot, Y, 0.0);
                }
                display.kind = DisplayKind::Image {
                    pivot,
                    texture: None,
                };
            }
            DisplayType::Armature => {
                let mut actions = Vec::new();
                if has_key(raw, ACTIONS) {
                    actions = self.parse_action_data(&raw[ACTIONS], ActionType::Play, None, None);
                } else if let Some(slot_name) = self.slot.clone()
                    && let Some(slot_actions) = self.slot_child_actions.get(&slot_name).cloned()
                {
                    let displays_len = self
                        .skin
                        .as_ref()
                        .and_then(|s| s.get_displays(&slot_name))
                        .map(<[Option<DisplayData>]>::len);
                    let display_index = self
                        .armature
                        .as_ref()
                        .unwrap()
                        .get_slot(&slot_name)
                        .map_or(0, |s| s.display_index);
                    let should_take = match displays_len {
                        None => display_index == 0,
                        Some(len) => display_index as usize == len,
                    };
                    if should_take {
                        actions = slot_actions;
                        self.slot_child_actions.remove(&slot_name);
                    }
                }
                display.kind = DisplayKind::Armature {
                    inherit_animation: true,
                    actions,
                    armature: None,
                };
            }
            DisplayType::Mesh => {
                let mut geometry = GeometryData {
                    inherit_deform: get_boolean(raw, INHERIT_DEFORM, true),
                    ..Default::default()
                };

                if has_key(raw, SHARE) {
                    let this_skin = self.skin.as_ref().map_or(String::new(), |s| s.name.clone());
                    let this_slot = self.slot.clone().unwrap_or_default();
                    let target_skin_name = {
                        let mut n = get_string(raw, SKIN, DEFAULT_NAME);
                        if n.is_empty() {
                            n = DEFAULT_NAME.to_string();
                        }
                        n
                    };
                    self.share_specs.push(ShareSpec {
                        this_skin,
                        this_slot,
                        this_mesh: display.name.clone(),
                        target_skin: target_skin_name,
                        target_mesh: get_string(raw, SHARE, ""),
                    });
                } else {
                    self.parse_mesh(raw, &mut geometry, &display.name);
                }

                display.kind = DisplayKind::Mesh {
                    geometry,
                    texture: None,
                };
            }
            DisplayType::BoundingBox => {
                if let Some(bounding_box) = self.parse_bounding_box(raw) {
                    display.kind = DisplayKind::BoundingBox {
                        bounding_box: Some(Box::new(bounding_box)),
                    };
                } else {
                    return None;
                }
            }
            DisplayType::Path => {
                let mut geometry = GeometryData::default();
                self.parse_geometry(raw, &mut geometry);
                let curve_lengths = raw
                    .get(LENGTHS)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
                    .unwrap_or_default();
                display.kind = DisplayKind::Path {
                    closed: get_boolean(raw, CLOSED, false),
                    constant_speed: get_boolean(raw, CONSTANT_SPEED, false),
                    geometry,
                    curve_lengths,
                };
            }
        }

        if has_key(raw, TRANSFORM) {
            let scale = self.armature.as_ref().unwrap().scale;
            parse_transform(&raw[TRANSFORM], &mut display.transform, scale);
        }

        Some(display)
    }

    fn parse_mesh(&mut self, raw: &Value, geometry: &mut GeometryData, mesh_name: &str) {
        self.parse_geometry(raw, geometry);

        if has_key(raw, WEIGHTS) {
            let raw_slot_pose = raw
                .get(SLOT_POSE)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_f64().unwrap_or(f64::NAN))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let raw_bone_poses = raw
                .get(BONE_POSE)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_f64().unwrap_or(f64::NAN))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let skin_name = self.skin.as_ref().map_or(String::new(), |s| s.name.clone());
            let slot_name = self.slot.clone().unwrap_or_default();
            let key = format!("{skin_name}_{slot_name}_{mesh_name}");
            self.weight_slot_pose.insert(key.clone(), raw_slot_pose);
            self.weight_bone_poses.insert(key, raw_bone_poses);
        }
    }

    fn parse_bounding_box(&mut self, raw: &Value) -> Option<BoundingBoxData> {
        let bb_type = if has_key(raw, SUB_TYPE) && raw[SUB_TYPE].is_string() {
            get_bounding_box_type(&get_string(raw, SUB_TYPE, ""))
        } else {
            num_to_bounding_box_type(get_number(
                raw,
                SUB_TYPE,
                crate::core::enums::BoundingBoxType::Rectangle as u32 as f64,
            ))
        };

        let mut bounding_box = BoundingBoxData {
            color: get_number(raw, COLOR, 0x000000 as f64) as i32,
            ..Default::default()
        };

        match bb_type {
            crate::core::enums::BoundingBoxType::Polygon => {
                let (kind, width, height) = self.parse_polygon_bounding_box(raw);
                bounding_box.kind = kind;
                bounding_box.width = width;
                bounding_box.height = height;
            }
            crate::core::enums::BoundingBoxType::Ellipse => {
                bounding_box.kind = BoundingBoxKind::Ellipse;
                bounding_box.width = get_number(raw, WIDTH, 0.0);
                bounding_box.height = get_number(raw, HEIGHT, 0.0);
            }
            crate::core::enums::BoundingBoxType::Rectangle => {
                bounding_box.kind = BoundingBoxKind::Rectangle;
                bounding_box.width = get_number(raw, WIDTH, 0.0);
                bounding_box.height = get_number(raw, HEIGHT, 0.0);
            }
        }

        Some(bounding_box)
    }

    fn parse_polygon_bounding_box(&mut self, raw: &Value) -> (BoundingBoxKind, f64, f64) {
        let scale = self.armature.as_ref().unwrap().scale;
        let mut x = 0.0;
        let mut y = 0.0;
        let mut width = 0.0;
        let mut height = 0.0;
        let mut vertices = Vec::new();

        if let Some(raw_vertices) = raw.get(VERTICES).and_then(Value::as_array) {
            vertices.resize(raw_vertices.len(), 0.0);
            let mut i = 0;
            while i < raw_vertices.len() {
                let vx = raw_vertices[i].as_f64().unwrap_or(0.0) * scale;
                let vy = raw_vertices[i + 1].as_f64().unwrap_or(0.0) * scale;
                vertices[i] = vx;
                vertices[i + 1] = vy;

                if i == 0 {
                    x = vx;
                    y = vy;
                    width = vx;
                    height = vy;
                } else {
                    if vx < x {
                        x = vx;
                    } else if vx > width {
                        width = vx;
                    }
                    if vy < y {
                        y = vy;
                    } else if vy > height {
                        height = vy;
                    }
                }
                i += 2;
            }
            width -= x;
            height -= y;
        } else {
            log::warn!("Data error.\n Please reexport DragonBones Data to fixed the bug.");
        }

        (BoundingBoxKind::Polygon { x, y, vertices }, width, height)
    }

    // ---- animation ----

    /// `BinaryDataParser._parseAnimation`（BinaryDataParser.ts:186-329）。
    fn parse_binary_animation(&mut self, raw: &Value) -> AnimationData {
        let frame_count = get_number(raw, DURATION, 0.0) as i32;
        let frame_rate = self.armature.as_ref().unwrap().frame_rate;
        let mut name = get_string(raw, NAME, DEFAULT_NAME);
        if name.is_empty() {
            name = DEFAULT_NAME.to_string();
        }
        let offsets = raw.get(OFFSET).and_then(Value::as_array);
        let offset_at = |index: usize| -> i32 {
            offsets
                .and_then(|values| values.get(index))
                .and_then(Value::as_f64)
                .map_or(0, |value| value as i32)
        };
        let animation = AnimationData {
            blend_type: crate::parser::data_parser::get_animation_blend_type(&get_string(
                raw, BLEND_TYPE, "",
            )),
            frame_count,
            play_times: get_number(raw, PLAY_TIMES, 1.0) as i32,
            duration: f64::from(frame_count) / frame_rate,
            fade_in_time: get_number(raw, FADE_IN_TIME, 0.0),
            scale: get_number(raw, SCALE, 1.0),
            name,
            frame_int_offset: offset_at(0),
            frame_float_offset: offset_at(1),
            frame_offset: offset_at(2),
            ..Default::default()
        };
        self.animation = Some(animation);

        if let Some(offset) = raw.get(ACTION).and_then(Value::as_f64)
            && let Some(timeline) =
                self.parse_binary_timeline(TimelineType::Action, offset as i32, 0.0, 0.0)
        {
            self.animation.as_mut().unwrap().action_timeline = Some(Box::new(timeline));
        }
        if let Some(offset) = raw.get(Z_ORDER).and_then(Value::as_f64)
            && let Some(timeline) =
                self.parse_binary_timeline(TimelineType::ZOrder, offset as i32, 0.0, 0.0)
        {
            self.animation.as_mut().unwrap().z_order_timeline = Some(Box::new(timeline));
        }

        for (key, target_is_bone) in [(BONE, true), (SLOT, false), (CONSTRAINT, false)] {
            let Some(groups) = raw.get(key).and_then(Value::as_object) else {
                continue;
            };
            for (target_name, values) in groups {
                let exists = match (key, self.armature.as_ref()) {
                    (BONE, Some(armature)) => armature.get_bone(target_name).is_some(),
                    (SLOT, Some(armature)) => armature.get_slot(target_name).is_some(),
                    (CONSTRAINT, Some(armature)) => {
                        armature.constraints.contains_key(target_name)
                    }
                    _ => false,
                };
                if !exists {
                    continue;
                }
                let Some(values) = values.as_array() else {
                    continue;
                };
                let mut index = 0;
                while index + 1 < values.len() {
                    let Some(kind) = values[index].as_f64() else {
                        break;
                    };
                    let Some(offset) = values[index + 1].as_f64() else {
                        break;
                    };
                    let timeline_type = num_to_timeline_type(kind);
                    if let Some(timeline) =
                        self.parse_binary_timeline(timeline_type, offset as i32, 0.0, 0.0)
                    {
                        let animation = self.animation.as_mut().unwrap();
                        if target_is_bone {
                            animation.add_bone_timeline(target_name, timeline);
                        } else if key == CONSTRAINT {
                            animation.add_constraint_timeline(target_name, timeline);
                        } else {
                            animation.add_slot_timeline(target_name, timeline);
                        }
                    }
                    index += 2;
                }
            }
        }

        if let Some(raw_timelines) = raw.get(TIMELINE).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                let offset = get_number(raw_timeline, OFFSET, -1.0);
                if offset < 0.0 {
                    continue;
                }
                let kind = get_number(raw_timeline, TYPE, f64::from(TimelineType::Action as u32));
                let timeline_type = num_to_timeline_type(kind);
                let timeline_name = get_string(raw_timeline, NAME, "");
                let x = get_number(raw_timeline, X, 0.0);
                let y = get_number(raw_timeline, Y, 0.0);
                let Some(timeline) =
                    self.parse_binary_timeline(timeline_type, offset as i32, x, y)
                else {
                    continue;
                };
                let animation = self.animation.as_mut().unwrap();
                match timeline_type {
                    TimelineType::Action | TimelineType::ZOrder => {}
                    TimelineType::BoneTranslate
                    | TimelineType::BoneRotate
                    | TimelineType::BoneScale
                    | TimelineType::Surface
                    | TimelineType::BoneAlpha => animation.add_bone_timeline(&timeline_name, timeline),
                    TimelineType::SlotDisplay
                    | TimelineType::SlotColor
                    | TimelineType::SlotDeform
                    | TimelineType::SlotZIndex
                    | TimelineType::SlotAlpha => animation.add_slot_timeline(&timeline_name, timeline),
                    TimelineType::IKConstraint => {
                        animation.add_constraint_timeline(&timeline_name, timeline)
                    }
                    TimelineType::AnimationProgress
                    | TimelineType::AnimationWeight
                    | TimelineType::AnimationParameter => {
                        animation.add_animation_timeline(&timeline_name, timeline)
                    }
                    TimelineType::BoneAll => animation.add_bone_timeline(&timeline_name, timeline),
                }
            }
        }

        self.animation.take().expect("binary animation was set")
    }

    /// `BinaryDataParser._parseBinaryTimeline`（:141-184）。
    fn parse_binary_timeline(
        &mut self,
        timeline_type: TimelineType,
        offset: i32,
        x: f64,
        y: f64,
    ) -> Option<TimelineData> {
        let animation = self.animation.as_ref()?;
        let frame_count = animation.frame_count;
        let frame_offset = animation.frame_offset;
        let offset_index = usize::try_from(offset).ok()?;
        let key_frame_count = *self
            .timeline_array
            .get(offset_index + binary_offset::timeline::KEY_FRAME_COUNT)?;
        let mut frame_indices_offset = -1i32;
        if key_frame_count > 1 {
            let total = usize::try_from(frame_count + 1).ok()?;
            frame_indices_offset = i32::try_from(self.frame_indices.len()).ok()?;
            self.frame_indices.resize(self.frame_indices.len() + total, 0);
            let mut key_index = 0usize;
            let mut frame_start = 0i32;
            let mut count = 0i32;
            let key_count = usize::try_from(key_frame_count).ok()?;
            for frame in 0..total {
                if frame_start + count <= frame as i32 && key_index < key_count {
                    let entry = offset_index
                        + binary_offset::timeline::FRAME_OFFSET
                        + key_index;
                    let relative = *self.timeline_array.get(entry)?;
                    frame_start = *self
                        .frame_array
                        .get(frame_offset as usize + relative as usize)?;
                    count = if key_index + 1 == key_count {
                        frame_count - frame_start
                    } else {
                        let next_entry = entry + 1;
                        let next_relative = *self.timeline_array.get(next_entry)?;
                        *self
                            .frame_array
                            .get(frame_offset as usize + next_relative as usize)?
                            - frame_start
                    };
                    key_index += 1;
                }
                self.frame_indices[frame_indices_offset as usize + frame] =
                    i32::try_from(key_index).ok()? - 1;
            }
        }
        Some(TimelineData {
            timeline_type,
            offset,
            frame_indices_offset,
            x,
            y,
        })
    }

    /// `BinaryDataParser._parseGeometry`（:331-365）。
    fn parse_binary_geometry(&mut self, raw: &Value, geometry: &mut GeometryData) {
        let offset = get_number(raw, OFFSET, -1.0);
        geometry.offset = offset as i32;
        let Some(geometry_offset) = usize::try_from(offset as i32).ok() else {
            return;
        };
        let mut weight_offset = self
            .int_array
            .get(geometry_offset + binary_offset::geometry::WEIGHT_OFFSET)
            .copied()
            .unwrap_or(-1);
        if weight_offset < -1 {
            weight_offset += 65536;
        }
        if weight_offset < 0 {
            return;
        }
        let Some(weight_offset) = usize::try_from(weight_offset).ok() else {
            return;
        };
        let vertex_count = self
            .int_array
            .get(geometry_offset + binary_offset::geometry::VERTEX_COUNT)
            .copied()
            .unwrap_or(0);
        let bone_count = self
            .int_array
            .get(weight_offset + binary_offset::weight::BONE_COUNT)
            .copied()
            .unwrap_or(0);
        if bone_count < 0 || vertex_count < 0 {
            return;
        }
        let mut weight = WeightData {
            offset: weight_offset as i32,
            count: 0,
            ..Default::default()
        };
        for index in 0..bone_count {
            let bone_index = self
                .int_array
                .get(weight_offset + binary_offset::weight::BONE_INDICES + index as usize)
                .copied()
                .unwrap_or(0);
            if let Some(name) = usize::try_from(bone_index)
                .ok()
                .and_then(|index| self.raw_bones.get(index))
            {
                weight.add_bone(name.clone());
            }
        }
        let mut cursor = weight_offset + binary_offset::weight::BONE_INDICES + bone_count as usize;
        let mut weight_count = 0i32;
        for _ in 0..vertex_count {
            let vertex_bone_count = self.int_array.get(cursor).copied().unwrap_or(0);
            weight_count += vertex_bone_count;
            cursor += 1 + usize::try_from(vertex_bone_count).unwrap_or(0);
        }
        weight.count = weight_count;
        geometry.weight = Some(Box::new(weight));
    }

    fn parse_animation(&mut self, raw: &Value) -> AnimationData {
        if self.binary.is_some() {
            return self.parse_binary_animation(raw);
        }
        let frame_count = get_number(raw, DURATION, 0.0) as i32;
        let frame_rate = self.armature.as_ref().unwrap().frame_rate;
        let mut name = get_string(raw, NAME, DEFAULT_NAME);
        if name.is_empty() {
            name = DEFAULT_NAME.to_string();
        }

        let animation = AnimationData {
            blend_type: crate::parser::data_parser::get_animation_blend_type(&get_string(
                raw, BLEND_TYPE, "",
            )),
            frame_count,
            play_times: get_number(raw, PLAY_TIMES, 1.0) as i32,
            duration: f64::from(frame_count) / frame_rate,
            fade_in_time: get_number(raw, FADE_IN_TIME, 0.0),
            scale: get_number(raw, SCALE, 1.0),
            name,
            frame_int_offset: self.frame_int_array.len() as i32,
            frame_float_offset: self.frame_float_array.len() as i32,
            frame_offset: self.frame_array.len() as i32,
            ..Default::default()
        };

        self.animation = Some(animation);

        if let Some(raw_frames) = raw.get(FRAME).and_then(Value::as_array) {
            let mut frame_start = 0.0;
            for raw_frame in raw_frames {
                self.parse_action_data_in_frame(raw_frame, frame_start as i32, None, None);
                frame_start += get_number(raw_frame, DURATION, 1.0);
            }
        }

        if has_key(raw, Z_ORDER) {
            let frames = frames_from(&raw[Z_ORDER], FRAME);
            let timeline = frames.as_deref().and_then(|f| {
                self.parse_timeline(
                    Some(&raw[Z_ORDER]),
                    f,
                    TimelineType::ZOrder,
                    FrameValueType::Step,
                    0,
                    FrameKind::ZOrder,
                    None,
                )
            });
            self.animation.as_mut().unwrap().z_order_timeline = timeline.map(Box::new);
        }

        if let Some(raw_timelines) = raw.get(BONE).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                self.parse_bone_timeline(raw_timeline);
            }
        }

        if let Some(raw_timelines) = raw.get(SLOT).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                self.parse_slot_timeline(raw_timeline);
            }
        }

        if let Some(raw_timelines) = raw.get(FFD).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                let mut skin_name = get_string(raw_timeline, SKIN, DEFAULT_NAME);
                let slot_name = get_string(raw_timeline, SLOT, "");
                let display_name = get_string(raw_timeline, NAME, "");
                if skin_name.is_empty() {
                    skin_name = DEFAULT_NAME.to_string();
                }

                let Some(mesh_geometry) = self.mesh_geometry(&skin_name, &slot_name, &display_name)
                else {
                    continue;
                };
                self.slot = Some(slot_name.clone());
                self.deform_offset = mesh_geometry.0;
                self.deform_weight = mesh_geometry.1;
                self.deform_mesh_name = Some(format!("{skin_name}_{slot_name}_{display_name}"));

                let frames = frames_from(raw_timeline, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw_timeline),
                        f,
                        TimelineType::SlotDeform,
                        FrameValueType::Float,
                        0,
                        FrameKind::SlotDeform,
                        None,
                    )
                });

                if let Some(timeline) = timeline {
                    self.animation
                        .as_mut()
                        .unwrap()
                        .add_slot_timeline(&slot_name, timeline);
                }

                self.slot = None;
                self.deform_mesh_name = None;
            }
        }

        if let Some(raw_timelines) = raw.get(IK).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                let constraint_name = get_string(raw_timeline, NAME, "");
                if self
                    .armature
                    .as_ref()
                    .unwrap()
                    .get_constraint(&constraint_name)
                    .is_none()
                {
                    continue;
                }

                let frames = frames_from(raw_timeline, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw_timeline),
                        f,
                        TimelineType::IKConstraint,
                        FrameValueType::Int,
                        2,
                        FrameKind::Ik,
                        None,
                    )
                });

                if let Some(timeline) = timeline {
                    self.animation
                        .as_mut()
                        .unwrap()
                        .add_constraint_timeline(&constraint_name, timeline);
                }
            }
        }

        if !self.action_frames.is_empty() {
            let action_frames = self.action_frames.clone();
            let frames: Vec<FrameRef<'_>> = action_frames.iter().map(FrameRef::Action).collect();
            let timeline = self.parse_timeline(
                None,
                &frames,
                TimelineType::Action,
                FrameValueType::Step,
                0,
                FrameKind::Action,
                None,
            );
            self.animation.as_mut().unwrap().action_timeline = timeline.map(Box::new);
            self.action_frames.clear();
        }

        if let Some(raw_timelines) = raw.get(TIMELINE).and_then(Value::as_array) {
            for raw_timeline in raw_timelines {
                self.parse_generic_timeline(raw_timeline);
            }
        }

        self.animation.take().unwrap()
    }

    fn parse_generic_timeline(&mut self, raw: &Value) {
        let timeline_type =
            num_to_timeline_type(get_number(raw, TYPE, TimelineType::Action as u32 as f64));
        let timeline_name = get_string(raw, NAME, "");

        match timeline_type {
            TimelineType::Action | TimelineType::ZOrder | TimelineType::BoneAll => {}
            TimelineType::SlotDisplay
            | TimelineType::SlotZIndex
            | TimelineType::BoneAlpha
            | TimelineType::SlotAlpha
            | TimelineType::AnimationProgress
            | TimelineType::AnimationWeight => {
                if timeline_type == TimelineType::SlotDisplay {
                    self.frame_value_type = FrameValueType::Step;
                    self.frame_value_scale = 1.0;
                } else {
                    self.frame_value_type = FrameValueType::Int;
                    self.frame_value_scale = match timeline_type {
                        TimelineType::SlotZIndex => 1.0,
                        TimelineType::AnimationProgress | TimelineType::AnimationWeight => 10000.0,
                        _ => 100.0,
                    };
                }
                self.frame_default_value = if timeline_type == TimelineType::BoneAlpha
                    || timeline_type == TimelineType::SlotAlpha
                    || timeline_type == TimelineType::AnimationWeight
                {
                    1.0
                } else {
                    0.0
                };

                let existing = if timeline_type == TimelineType::AnimationProgress
                    && self.animation.as_ref().unwrap().blend_type
                        != crate::core::enums::AnimationBlendType::None
                {
                    Some(TimelineData {
                        x: get_number(raw, X, 0.0),
                        y: get_number(raw, Y, 0.0),
                        ..Default::default()
                    })
                } else {
                    None
                };

                let frames = frames_from(raw, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw),
                        f,
                        timeline_type,
                        self.frame_value_type,
                        1,
                        FrameKind::SingleValue,
                        existing,
                    )
                });
                self.add_generic_timeline(timeline_type, &timeline_name, timeline);
            }
            TimelineType::BoneTranslate
            | TimelineType::BoneRotate
            | TimelineType::BoneScale
            | TimelineType::IKConstraint
            | TimelineType::AnimationParameter => {
                if timeline_type == TimelineType::IKConstraint
                    || timeline_type == TimelineType::AnimationParameter
                {
                    self.frame_value_type = FrameValueType::Int;
                    self.frame_value_scale = if timeline_type == TimelineType::AnimationParameter {
                        10000.0
                    } else {
                        100.0
                    };
                } else {
                    self.frame_value_scale = if timeline_type == TimelineType::BoneRotate {
                        DEG_RAD
                    } else {
                        1.0
                    };
                    self.frame_value_type = FrameValueType::Float;
                }
                self.frame_default_value = if timeline_type == TimelineType::BoneScale
                    || timeline_type == TimelineType::IKConstraint
                {
                    1.0
                } else {
                    0.0
                };

                let frames = frames_from(raw, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw),
                        f,
                        timeline_type,
                        self.frame_value_type,
                        2,
                        FrameKind::DoubleValue,
                        None,
                    )
                });
                self.add_generic_timeline(timeline_type, &timeline_name, timeline);
            }
            TimelineType::Surface => {
                let info = self.surface_geometry(&timeline_name);
                let Some(info) = info else {
                    return;
                };
                self.deform_offset = info.0;
                self.deform_weight = info.1;

                let frames = frames_from(raw, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw),
                        f,
                        TimelineType::Surface,
                        FrameValueType::Float,
                        0,
                        FrameKind::Deform,
                        None,
                    )
                });
                self.add_generic_timeline(timeline_type, &timeline_name, timeline);
            }
            TimelineType::SlotDeform => {
                let info = self.find_deform_geometry_by_name(&timeline_name);
                let Some(info) = info else {
                    return;
                };
                self.deform_offset = info.0;
                self.deform_weight = info.1;

                let frames = frames_from(raw, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw),
                        f,
                        TimelineType::SlotDeform,
                        FrameValueType::Float,
                        0,
                        FrameKind::Deform,
                        None,
                    )
                });
                self.add_generic_timeline(timeline_type, &timeline_name, timeline);
            }
            TimelineType::SlotColor => {
                let frames = frames_from(raw, FRAME);
                let timeline = frames.as_deref().and_then(|f| {
                    self.parse_timeline(
                        Some(raw),
                        f,
                        TimelineType::SlotColor,
                        FrameValueType::Int,
                        1,
                        FrameKind::SlotColor,
                        None,
                    )
                });
                self.add_generic_timeline(timeline_type, &timeline_name, timeline);
            }
        }
    }

    fn add_generic_timeline(
        &mut self,
        timeline_type: TimelineType,
        timeline_name: &str,
        timeline: Option<TimelineData>,
    ) {
        let Some(timeline) = timeline else {
            return;
        };
        let animation = self.animation.as_mut().unwrap();
        match timeline_type {
            TimelineType::BoneTranslate
            | TimelineType::BoneRotate
            | TimelineType::BoneScale
            | TimelineType::Surface
            | TimelineType::BoneAlpha => animation.add_bone_timeline(timeline_name, timeline),

            TimelineType::SlotDisplay
            | TimelineType::SlotColor
            | TimelineType::SlotDeform
            | TimelineType::SlotZIndex
            | TimelineType::SlotAlpha => animation.add_slot_timeline(timeline_name, timeline),

            TimelineType::IKConstraint => {
                animation.add_constraint_timeline(timeline_name, timeline)
            }

            TimelineType::AnimationProgress
            | TimelineType::AnimationWeight
            | TimelineType::AnimationParameter => {
                animation.add_animation_timeline(timeline_name, timeline)
            }

            TimelineType::Action | TimelineType::ZOrder | TimelineType::BoneAll => {}
        }
    }

    fn parse_bone_timeline(&mut self, raw: &Value) {
        let bone_name = get_string(raw, NAME, "");
        if self
            .armature
            .as_ref()
            .unwrap()
            .get_bone(&bone_name)
            .is_none()
        {
            return;
        }

        self.bone = Some(bone_name.clone());
        self.slot = self
            .armature
            .as_ref()
            .unwrap()
            .get_slot(&bone_name)
            .map(|_| bone_name.clone());

        if let Some(frames) = frames_from(raw, TRANSLATE_FRAME) {
            self.frame_default_value = 0.0;
            self.frame_value_scale = 1.0;
            let timeline = self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::BoneTranslate,
                FrameValueType::Float,
                2,
                FrameKind::DoubleValue,
                None,
            );
            if let Some(timeline) = timeline {
                self.animation
                    .as_mut()
                    .unwrap()
                    .add_bone_timeline(&bone_name, timeline);
            }
        }

        if let Some(frames) = frames_from(raw, ROTATE_FRAME) {
            self.frame_default_value = 0.0;
            self.frame_value_scale = 1.0;
            let timeline = self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::BoneRotate,
                FrameValueType::Float,
                2,
                FrameKind::BoneRotate,
                None,
            );
            if let Some(timeline) = timeline {
                self.animation
                    .as_mut()
                    .unwrap()
                    .add_bone_timeline(&bone_name, timeline);
            }
        }

        if let Some(frames) = frames_from(raw, SCALE_FRAME) {
            self.frame_default_value = 1.0;
            self.frame_value_scale = 1.0;
            let timeline = self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::BoneScale,
                FrameValueType::Float,
                2,
                FrameKind::BoneScale,
                None,
            );
            if let Some(timeline) = timeline {
                self.animation
                    .as_mut()
                    .unwrap()
                    .add_bone_timeline(&bone_name, timeline);
            }
        }

        if let Some(frames) = frames_from(raw, FRAME) {
            let timeline = self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::BoneAll,
                FrameValueType::Float,
                6,
                FrameKind::BoneAll,
                None,
            );
            if let Some(timeline) = timeline {
                self.animation
                    .as_mut()
                    .unwrap()
                    .add_bone_timeline(&bone_name, timeline);
            }
        }

        self.bone = None;
        self.slot = None;
    }

    fn parse_slot_timeline(&mut self, raw: &Value) {
        let slot_name = get_string(raw, NAME, "");
        if self
            .armature
            .as_ref()
            .unwrap()
            .get_slot(&slot_name)
            .is_none()
        {
            return;
        }
        self.slot = Some(slot_name.clone());

        let display_key = if has_key(raw, DISPLAY_FRAME) {
            DISPLAY_FRAME
        } else {
            FRAME
        };
        let display_timeline = frames_from(raw, display_key).and_then(|frames| {
            self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::SlotDisplay,
                FrameValueType::Step,
                0,
                FrameKind::SlotDisplay,
                None,
            )
        });

        let color_key = if has_key(raw, COLOR_FRAME) {
            COLOR_FRAME
        } else {
            FRAME
        };
        let color_timeline = frames_from(raw, color_key).and_then(|frames| {
            self.parse_timeline(
                Some(raw),
                &frames,
                TimelineType::SlotColor,
                FrameValueType::Int,
                1,
                FrameKind::SlotColor,
                None,
            )
        });

        if let Some(timeline) = display_timeline {
            self.animation
                .as_mut()
                .unwrap()
                .add_slot_timeline(&slot_name, timeline);
        }
        if let Some(timeline) = color_timeline {
            self.animation
                .as_mut()
                .unwrap()
                .add_slot_timeline(&slot_name, timeline);
        }

        self.slot = None;
    }

    // ---- timeline ----

    #[allow(clippy::too_many_arguments)]
    fn parse_timeline(
        &mut self,
        raw_data: Option<&Value>,
        raw_frames: &[FrameRef<'_>],
        timeline_type: TimelineType,
        frame_value_type: FrameValueType,
        frame_value_count: i32,
        frame_kind: FrameKind,
        existing: Option<TimelineData>,
    ) -> Option<TimelineData> {
        if raw_frames.is_empty() {
            return None;
        }

        let key_frame_count = raw_frames.len();
        let frame_int_len = self.frame_int_array.len();
        let frame_float_len = self.frame_float_array.len();
        let timeline_offset = self.timeline_array.len();

        let mut timeline = existing.unwrap_or_default();
        timeline.timeline_type = timeline_type;
        timeline.offset = timeline_offset as i32;
        self.frame_value_type = frame_value_type;
        self.timeline = Some(timeline);

        self.reserve_timeline(1 + 1 + 1 + 1 + 1 + key_frame_count);

        if let Some(raw) = raw_data {
            self.tlset(
                timeline_offset + binary_offset::timeline::SCALE,
                js_round(get_number(raw, SCALE, 1.0) * 100.0) as i32,
            );
            self.tlset(
                timeline_offset + binary_offset::timeline::OFFSET,
                js_round(get_number(raw, OFFSET, 0.0) * 100.0) as i32,
            );
        } else {
            self.tlset(timeline_offset + binary_offset::timeline::SCALE, 100);
            self.tlset(timeline_offset + binary_offset::timeline::OFFSET, 0);
        }

        self.tlset(
            timeline_offset + binary_offset::timeline::KEY_FRAME_COUNT,
            key_frame_count as i32,
        );
        self.tlset(
            timeline_offset + binary_offset::timeline::FRAME_VALUE_COUNT,
            frame_value_count,
        );

        let frame_value_offset = match self.frame_value_type {
            FrameValueType::Step => 0,
            FrameValueType::Int => {
                frame_int_len as i32 - self.animation.as_ref().unwrap().frame_int_offset
            }
            FrameValueType::Float => {
                frame_float_len as i32 - self.animation.as_ref().unwrap().frame_float_offset
            }
        };
        self.tlset(
            timeline_offset + binary_offset::timeline::FRAME_VALUE_OFFSET,
            frame_value_offset,
        );

        let anim_frame_count = self.animation.as_ref().unwrap().frame_count;
        let anim_frame_offset = self.animation.as_ref().unwrap().frame_offset;

        if key_frame_count == 1 {
            let value = self.dispatch_frame(frame_kind, &raw_frames[0], 0, 0) - anim_frame_offset;
            self.timeline.as_mut().unwrap().frame_indices_offset = -1;
            self.tlset(
                timeline_offset + binary_offset::timeline::FRAME_OFFSET,
                value,
            );
        } else {
            let total_frame_count = (anim_frame_count + 1) as usize;
            let frame_indices_offset = self.frame_indices.len();
            self.frame_indices
                .resize(frame_indices_offset + total_frame_count, 0);
            self.timeline.as_mut().unwrap().frame_indices_offset = frame_indices_offset as i32;

            let mut frame_start = 0usize;
            let mut frame_count = 0usize;
            let mut ik = 0usize;
            for i in 0..total_frame_count {
                if frame_start + frame_count <= i && ik < key_frame_count {
                    let raw_frame = raw_frames[ik];
                    frame_start = i;

                    frame_count = if ik == key_frame_count - 1 {
                        anim_frame_count as usize - frame_start
                    } else {
                        match raw_frame {
                            FrameRef::Action(_) => {
                                self.action_frames[ik + 1].frame_start as usize - frame_start
                            }
                            FrameRef::Json(v) => get_number(v, DURATION, 1.0) as usize,
                        }
                    };

                    let value =
                        self.dispatch_frame(frame_kind, &raw_frame, frame_start, frame_count)
                            - anim_frame_offset;
                    self.tlset(
                        timeline_offset + binary_offset::timeline::FRAME_OFFSET + ik,
                        value,
                    );
                    ik += 1;
                }

                self.idxset(frame_indices_offset + i, ik as i32 - 1);
            }
        }

        self.timeline.take()
    }

    fn dispatch_frame(
        &mut self,
        kind: FrameKind,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        match kind {
            FrameKind::Frame => self.parse_frame(frame, frame_start),
            FrameKind::Tween => self.parse_tween_frame(frame, frame_start, frame_count),
            FrameKind::SingleValue => {
                self.parse_single_value_frame(frame, frame_start, frame_count)
            }
            FrameKind::DoubleValue => {
                self.parse_double_value_frame(frame, frame_start, frame_count)
            }
            FrameKind::BoneAll => self.parse_bone_all_frame(frame, frame_start, frame_count),
            FrameKind::BoneTranslate => {
                self.parse_bone_translate_frame(frame, frame_start, frame_count)
            }
            FrameKind::BoneRotate => self.parse_bone_rotate_frame(frame, frame_start, frame_count),
            FrameKind::BoneScale => self.parse_bone_scale_frame(frame, frame_start, frame_count),
            FrameKind::SlotDisplay => self.parse_slot_display_frame(frame, frame_start),
            FrameKind::SlotColor => self.parse_slot_color_frame(frame, frame_start, frame_count),
            FrameKind::SlotDeform => self.parse_slot_deform_frame(frame, frame_start, frame_count),
            FrameKind::Deform => self.parse_deform_frame(frame, frame_start, frame_count),
            FrameKind::ZOrder => self.parse_z_order_frame(frame, frame_start),
            FrameKind::Action => self.parse_action_frame(frame, frame_start),
            FrameKind::Ik => self.parse_ik_constraint_frame(frame, frame_start, frame_count),
        }
    }

    // ---- frame parsers ----

    fn parse_frame(&mut self, _frame: &FrameRef<'_>, frame_start: usize) -> i32 {
        let frame_offset = self.frame_array.len();
        self.reserve_frame(1);
        self.frameset(
            frame_offset + binary_offset::frame::POSITION,
            frame_start as i32,
        );
        frame_offset as i32
    }

    fn parse_tween_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let frame_offset = self.parse_frame(frame, frame_start) as usize;
        let raw = json_of(frame);

        if frame_count > 0 {
            if has_key(raw, CURVE) {
                let sample_count = frame_count + 1;
                let curve: Vec<f64> = raw[CURVE]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
                    .unwrap_or_default();
                let mut samples = vec![0.0; sample_count];
                let is_omited = self.sampling_easing_curve(&curve, &mut samples);

                self.reserve_frame(1 + 1 + samples.len());
                self.frameset(
                    frame_offset + binary_offset::frame::TWEEN_TYPE,
                    TweenType::Curve as u32 as i32,
                );
                self.frameset(
                    frame_offset + binary_offset::frame::TWEEN_EASING_OR_CURVE_SAMPLE_COUNT,
                    if is_omited {
                        sample_count as i32
                    } else {
                        -(sample_count as i32)
                    },
                );
                for (i, sample) in samples.iter().enumerate() {
                    self.frameset(
                        frame_offset + binary_offset::frame::CURVE_SAMPLES + i,
                        js_round(sample * 10000.0) as i32,
                    );
                }
            } else {
                let no_tween = -2.0;
                let mut tween_easing = no_tween;
                if has_key(raw, TWEEN_EASING) {
                    tween_easing = get_number(raw, TWEEN_EASING, no_tween);
                }

                if tween_easing == no_tween {
                    self.reserve_frame(1);
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_TYPE,
                        TweenType::None as u32 as i32,
                    );
                } else if tween_easing == 0.0 {
                    self.reserve_frame(1);
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_TYPE,
                        TweenType::Line as u32 as i32,
                    );
                } else if tween_easing < 0.0 {
                    self.reserve_frame(1 + 1);
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_TYPE,
                        TweenType::QuadIn as u32 as i32,
                    );
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_EASING_OR_CURVE_SAMPLE_COUNT,
                        js_round(-tween_easing * 100.0) as i32,
                    );
                } else if tween_easing <= 1.0 {
                    self.reserve_frame(1 + 1);
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_TYPE,
                        TweenType::QuadOut as u32 as i32,
                    );
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_EASING_OR_CURVE_SAMPLE_COUNT,
                        js_round(tween_easing * 100.0) as i32,
                    );
                } else {
                    self.reserve_frame(1 + 1);
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_TYPE,
                        TweenType::QuadInOut as u32 as i32,
                    );
                    self.frameset(
                        frame_offset + binary_offset::frame::TWEEN_EASING_OR_CURVE_SAMPLE_COUNT,
                        js_round(tween_easing * 100.0 - 100.0) as i32,
                    );
                }
            }
        } else {
            self.reserve_frame(1);
            self.frameset(
                frame_offset + binary_offset::frame::TWEEN_TYPE,
                TweenType::None as u32 as i32,
            );
        }

        frame_offset as i32
    }

    fn parse_single_value_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        match self.frame_value_type {
            FrameValueType::Step => {
                let frame_offset = self.parse_frame(frame, frame_start) as usize;
                self.reserve_frame(1);
                self.frameset(
                    frame_offset + 1,
                    get_number(raw, VALUE, self.frame_default_value) as i32,
                );
                frame_offset as i32
            }
            FrameValueType::Int => {
                let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
                let frame_value_offset = self.frame_int_array.len();
                self.reserve_frame_int(1);
                self.fintset(
                    frame_value_offset,
                    js_round(
                        get_number(raw, VALUE, self.frame_default_value) * self.frame_value_scale,
                    ) as i32,
                );
                frame_offset
            }
            FrameValueType::Float => {
                let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
                let frame_value_offset = self.frame_float_array.len();
                self.reserve_frame_float(1);
                self.ffloatset(
                    frame_value_offset,
                    get_number(raw, VALUE, self.frame_default_value) * self.frame_value_scale,
                );
                frame_offset
            }
        }
    }

    fn parse_double_value_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        match self.frame_value_type {
            FrameValueType::Step => {
                let frame_offset = self.parse_frame(frame, frame_start) as usize;
                self.reserve_frame(2);
                self.frameset(
                    frame_offset + 1,
                    get_number(raw, X, self.frame_default_value) as i32,
                );
                self.frameset(
                    frame_offset + 2,
                    get_number(raw, Y, self.frame_default_value) as i32,
                );
                frame_offset as i32
            }
            FrameValueType::Int => {
                let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
                let frame_value_offset = self.frame_int_array.len();
                self.reserve_frame_int(2);
                self.fintset(
                    frame_value_offset,
                    js_round(get_number(raw, X, self.frame_default_value) * self.frame_value_scale)
                        as i32,
                );
                self.fintset(
                    frame_value_offset + 1,
                    js_round(get_number(raw, Y, self.frame_default_value) * self.frame_value_scale)
                        as i32,
                );
                frame_offset
            }
            FrameValueType::Float => {
                let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
                let frame_value_offset = self.frame_float_array.len();
                self.reserve_frame_float(2);
                self.ffloatset(
                    frame_value_offset,
                    get_number(raw, X, self.frame_default_value) * self.frame_value_scale,
                );
                self.ffloatset(
                    frame_value_offset + 1,
                    get_number(raw, Y, self.frame_default_value) * self.frame_value_scale,
                );
                frame_offset
            }
        }
    }

    fn parse_action_frame(&mut self, frame: &FrameRef<'_>, frame_start: usize) -> i32 {
        let action_frame = match frame {
            FrameRef::Action(a) => a,
            FrameRef::Json(_) => return 0,
        };

        let frame_offset = self.frame_array.len();
        let action_count = action_frame.actions.len();
        self.reserve_frame(1 + 1 + action_count);
        self.frameset(
            frame_offset + binary_offset::frame::POSITION,
            frame_start as i32,
        );
        self.frameset(
            frame_offset + binary_offset::frame::POSITION + 1,
            action_count as i32,
        );
        for i in 0..action_count {
            self.frameset(
                frame_offset + binary_offset::frame::POSITION + 2 + i,
                action_frame.actions[i],
            );
        }

        frame_offset as i32
    }

    fn parse_z_order_frame(&mut self, frame: &FrameRef<'_>, frame_start: usize) -> i32 {
        let frame_offset = self.parse_frame(frame, frame_start) as usize;
        let raw = json_of(frame);

        if has_key(raw, Z_ORDER)
            && let Some(raw_z_order) = raw[Z_ORDER].as_array()
            && !raw_z_order.is_empty()
        {
            let slot_count = self.armature.as_ref().unwrap().sorted_slots.len();
            let mut unchanged = vec![0.0; slot_count - raw_z_order.len() / 2];
            let mut z_orders = vec![-1.0; slot_count];

            let mut original_index = 0usize;
            let mut unchanged_index = 0usize;
            let mut i = 0;
            while i < raw_z_order.len() {
                let slot_index = raw_z_order[i].as_f64().unwrap_or(0.0) as usize;
                let z_order_offset = raw_z_order[i + 1].as_f64().unwrap_or(0.0);

                while original_index != slot_index {
                    unchanged[unchanged_index] = original_index as f64;
                    unchanged_index += 1;
                    original_index += 1;
                }

                let index = original_index + z_order_offset as usize;
                z_orders[index] = original_index as f64;
                original_index += 1;
                i += 2;
            }

            while original_index < slot_count {
                unchanged[unchanged_index] = original_index as f64;
                unchanged_index += 1;
                original_index += 1;
            }

            self.reserve_frame(1 + slot_count);
            self.frameset(frame_offset + 1, slot_count as i32);

            let mut i = slot_count;
            while i > 0 {
                i -= 1;
                if z_orders[i] == -1.0 {
                    unchanged_index -= 1;
                    self.frameset(frame_offset + 2 + i, unchanged[unchanged_index] as i32);
                } else {
                    self.frameset(frame_offset + 2 + i, z_orders[i] as i32);
                }
            }

            return frame_offset as i32;
        }

        self.reserve_frame(1);
        self.frameset(frame_offset + 1, 0);
        frame_offset as i32
    }

    fn continue_rotation(&mut self, mut rotation: f64, frame_start: usize, clockwise: i32) -> f64 {
        if frame_start != 0 {
            if self.prev_clockwise == 0 {
                rotation = self.prev_rotation + normalize_radian(rotation - self.prev_rotation);
            } else {
                if (self.prev_clockwise > 0 && rotation >= self.prev_rotation)
                    || (self.prev_clockwise < 0 && rotation <= self.prev_rotation)
                {
                    self.prev_clockwise += if self.prev_clockwise > 0 { -1 } else { 1 };
                }
                // 逐字照抄，不化简（F2.8）。
                rotation = self.prev_rotation + rotation - self.prev_rotation
                    + PI_D * f64::from(self.prev_clockwise);
            }
        }
        self.prev_clockwise = clockwise;
        self.prev_rotation = rotation;
        rotation
    }

    fn parse_bone_all_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        self.help_transform.identity();
        if has_key(raw, TRANSFORM) {
            parse_transform(&raw[TRANSFORM], &mut self.help_transform, 1.0);
        }

        let rotation = self.help_transform.rotation;
        let clockwise = get_number(raw, TWEEN_ROTATE, 0.0) as i32;
        let rotation = self.continue_rotation(rotation, frame_start, clockwise);

        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut frame_float_offset = self.frame_float_array.len();
        self.reserve_frame_float(6);
        self.ffloatset(frame_float_offset, self.help_transform.x);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, self.help_transform.y);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, rotation);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, self.help_transform.skew);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, self.help_transform.scale_x);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, self.help_transform.scale_y);

        let bone = self.bone.clone();
        let slot = self.slot.clone();
        self.parse_action_data_in_frame(raw, frame_start as i32, bone.as_deref(), slot.as_deref());

        frame_offset
    }

    fn parse_bone_translate_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut frame_float_offset = self.frame_float_array.len();
        self.reserve_frame_float(2);
        self.ffloatset(frame_float_offset, get_number(raw, X, 0.0));
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, get_number(raw, Y, 0.0));
        frame_offset
    }

    fn parse_bone_rotate_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        let rotation = get_number(raw, ROTATE, 0.0) * DEG_RAD;
        let clockwise = get_number(raw, CLOCK_WISE, 0.0) as i32;
        let rotation = self.continue_rotation(rotation, frame_start, clockwise);

        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut frame_float_offset = self.frame_float_array.len();
        self.reserve_frame_float(2);
        self.ffloatset(frame_float_offset, rotation);
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, get_number(raw, SKEW, 0.0) * DEG_RAD);
        frame_offset
    }

    fn parse_bone_scale_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut frame_float_offset = self.frame_float_array.len();
        self.reserve_frame_float(2);
        self.ffloatset(frame_float_offset, get_number(raw, X, 1.0));
        frame_float_offset += 1;
        self.ffloatset(frame_float_offset, get_number(raw, Y, 1.0));
        frame_offset
    }

    fn parse_slot_display_frame(&mut self, frame: &FrameRef<'_>, frame_start: usize) -> i32 {
        let raw = json_of(frame);
        let frame_offset = self.parse_frame(frame, frame_start) as usize;
        self.reserve_frame(1);

        if has_key(raw, VALUE) {
            self.frameset(frame_offset + 1, get_number(raw, VALUE, 0.0) as i32);
        } else {
            self.frameset(frame_offset + 1, get_number(raw, DISPLAY_INDEX, 0.0) as i32);
        }

        let slot_name = self.slot.clone();
        let parent = slot_name
            .as_ref()
            .and_then(|s| self.armature.as_ref().unwrap().get_slot(s))
            .map(|s| s.parent.clone());
        self.parse_action_data_in_frame(
            raw,
            frame_start as i32,
            parent.as_deref(),
            slot_name.as_deref(),
        );

        frame_offset as i32
    }

    fn parse_slot_color_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut color_offset: i32 = -1;

        if has_key(raw, VALUE) || has_key(raw, COLOR) {
            let raw_color = if has_key(raw, VALUE) {
                &raw[VALUE]
            } else {
                &raw[COLOR]
            };
            let has_entries = raw_color.as_object().is_some_and(|m| !m.is_empty());
            if has_entries {
                parse_color_transform(raw_color, &mut self.help_color);
                let offset = self.color_array.len();
                self.reserve_color(8);
                self.colset(
                    offset,
                    js_round(self.help_color.alpha_multiplier * 100.0) as i32,
                );
                self.colset(
                    offset + 1,
                    js_round(self.help_color.red_multiplier * 100.0) as i32,
                );
                self.colset(
                    offset + 2,
                    js_round(self.help_color.green_multiplier * 100.0) as i32,
                );
                self.colset(
                    offset + 3,
                    js_round(self.help_color.blue_multiplier * 100.0) as i32,
                );
                self.colset(offset + 4, js_round(self.help_color.alpha_offset) as i32);
                self.colset(offset + 5, js_round(self.help_color.red_offset) as i32);
                self.colset(offset + 6, js_round(self.help_color.green_offset) as i32);
                self.colset(offset + 7, js_round(self.help_color.blue_offset) as i32);
                color_offset = offset as i32;
            }
        }

        if color_offset < 0 {
            if self.default_color_offset < 0 {
                let offset = self.color_array.len();
                self.default_color_offset = offset as i32;
                self.reserve_color(8);
                self.colset(offset, 100);
                self.colset(offset + 1, 100);
                self.colset(offset + 2, 100);
                self.colset(offset + 3, 100);
                self.colset(offset + 4, 0);
                self.colset(offset + 5, 0);
                self.colset(offset + 6, 0);
                self.colset(offset + 7, 0);
            }
            color_offset = self.default_color_offset;
        }

        let frame_int_offset = self.frame_int_array.len();
        self.reserve_frame_int(1);
        self.fintset(frame_int_offset, color_offset);

        frame_offset
    }

    fn parse_slot_deform_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let frame_float_offset = self.frame_float_array.len();
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);

        let raw_vertices = raw.get(VERTICES).and_then(Value::as_array);
        let offset = get_number(raw, OFFSET, 0.0);
        let vertex_count =
            self.int_array[self.deform_offset as usize + binary_offset::geometry::VERTEX_COUNT];

        let mut i_b = 0usize;
        let mut i_v = 0usize;
        let mut had_weight = false;

        if let Some(weight) = self.deform_weight.clone() {
            had_weight = true;
            let mesh_name = self.deform_mesh_name.clone().unwrap_or_default();
            let raw_slot_pose = self
                .weight_slot_pose
                .get(&mesh_name)
                .cloned()
                .unwrap_or_default();
            self.help_matrix_a.copy_from_array(&raw_slot_pose, 0);
            self.reserve_frame_float(weight.count as usize * 2);
            i_b = weight.offset as usize + binary_offset::weight::BONE_INDICES + weight.bones.len();
        } else {
            self.reserve_frame_float(vertex_count as usize * 2);
        }

        let mut i = 0usize;
        while i < vertex_count as usize * 2 {
            let (mut x, mut y) = match raw_vertices {
                Some(rv) => {
                    let vx = if (i as f64) < offset || i as f64 - offset >= rv.len() as f64 {
                        0.0
                    } else {
                        rv[i - offset as usize].as_f64().unwrap_or(0.0)
                    };
                    let vy = if ((i + 1) as f64) < offset
                        || (i + 1) as f64 - offset >= rv.len() as f64
                    {
                        0.0
                    } else {
                        rv[i + 1 - offset as usize].as_f64().unwrap_or(0.0)
                    };
                    (vx, vy)
                }
                None => (0.0, 0.0),
            };

            if had_weight {
                let weight = self.deform_weight.as_ref().unwrap();
                let mesh_name = self.deform_mesh_name.clone().unwrap_or_default();
                let raw_bone_poses = self
                    .weight_bone_poses
                    .get(&mesh_name)
                    .cloned()
                    .unwrap_or_default();
                let vertex_bone_count = self.int_array[i_b];
                i_b += 1;

                let (tx, ty) = self.help_matrix_a.transform_point(x, y, true);
                x = tx;
                y = ty;

                let _ = weight;
                for _ in 0..vertex_bone_count {
                    let bone_index = self.int_array[i_b];
                    i_b += 1;
                    self.help_matrix_b
                        .copy_from_array(&raw_bone_poses, bone_index as usize * 7 + 1);
                    self.help_matrix_b.invert();
                    let (bx, by) = self.help_matrix_b.transform_point(x, y, true);
                    self.ffloatset(frame_float_offset + i_v, bx);
                    i_v += 1;
                    self.ffloatset(frame_float_offset + i_v, by);
                    i_v += 1;
                }
            } else {
                self.ffloatset(frame_float_offset + i, x);
                self.ffloatset(frame_float_offset + i + 1, y);
            }
            i += 2;
        }

        if frame_start == 0 {
            let frame_int_offset = self.frame_int_array.len();
            self.reserve_frame_int(5);
            self.fintset(
                frame_int_offset + binary_offset::deform::VERTEX_OFFSET,
                self.deform_offset,
            );
            self.fintset(
                frame_int_offset + binary_offset::deform::COUNT,
                self.frame_float_array.len() as i32 - frame_float_offset as i32,
            );
            self.fintset(
                frame_int_offset + binary_offset::deform::VALUE_COUNT,
                self.frame_float_array.len() as i32 - frame_float_offset as i32,
            );
            self.fintset(frame_int_offset + binary_offset::deform::VALUE_OFFSET, 0);
            self.fintset(
                frame_int_offset + binary_offset::deform::FLOAT_OFFSET,
                frame_float_offset as i32 - self.animation.as_ref().unwrap().frame_float_offset,
            );
            let timeline_offset = self.timeline.as_ref().unwrap().offset as usize;
            self.tlset(
                timeline_offset + binary_offset::timeline::FRAME_VALUE_COUNT,
                frame_int_offset as i32 - self.animation.as_ref().unwrap().frame_int_offset,
            );
        }

        frame_offset
    }

    fn parse_deform_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let frame_float_offset = self.frame_float_array.len();
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);

        let raw_vertices = if has_key(raw, VERTICES) {
            raw[VERTICES].as_array()
        } else if has_key(raw, VALUE) {
            raw[VALUE].as_array()
        } else {
            None
        };
        let offset = get_number(raw, OFFSET, 0.0);
        let vertex_count =
            self.int_array[self.deform_offset as usize + binary_offset::geometry::VERTEX_COUNT];

        if self.deform_weight.is_some() {
            // TODO（对齐 TS `_parseDeformFrame` 的 weight 分支）。
        } else {
            self.reserve_frame_float(vertex_count as usize * 2);
            let mut i = 0usize;
            while i < vertex_count as usize * 2 {
                let (x, y) = match raw_vertices {
                    Some(rv) => {
                        let vx = if (i as f64) < offset || i as f64 - offset >= rv.len() as f64 {
                            0.0
                        } else {
                            rv[i - offset as usize].as_f64().unwrap_or(0.0)
                        };
                        let vy = if ((i + 1) as f64) < offset
                            || (i + 1) as f64 - offset >= rv.len() as f64
                        {
                            0.0
                        } else {
                            rv[i + 1 - offset as usize].as_f64().unwrap_or(0.0)
                        };
                        (vx, vy)
                    }
                    None => (0.0, 0.0),
                };
                self.ffloatset(frame_float_offset + i, x);
                self.ffloatset(frame_float_offset + i + 1, y);
                i += 2;
            }
        }

        if frame_start == 0 {
            let frame_int_offset = self.frame_int_array.len();
            self.reserve_frame_int(5);
            self.fintset(
                frame_int_offset + binary_offset::deform::VERTEX_OFFSET,
                self.deform_offset,
            );
            self.fintset(
                frame_int_offset + binary_offset::deform::COUNT,
                self.frame_float_array.len() as i32 - frame_float_offset as i32,
            );
            self.fintset(
                frame_int_offset + binary_offset::deform::VALUE_COUNT,
                self.frame_float_array.len() as i32 - frame_float_offset as i32,
            );
            self.fintset(frame_int_offset + binary_offset::deform::VALUE_OFFSET, 0);
            self.fintset(
                frame_int_offset + binary_offset::deform::FLOAT_OFFSET,
                frame_float_offset as i32 - self.animation.as_ref().unwrap().frame_float_offset,
            );
            let timeline_offset = self.timeline.as_ref().unwrap().offset as usize;
            self.tlset(
                timeline_offset + binary_offset::timeline::FRAME_VALUE_COUNT,
                frame_int_offset as i32 - self.animation.as_ref().unwrap().frame_int_offset,
            );
        }

        frame_offset
    }

    fn parse_ik_constraint_frame(
        &mut self,
        frame: &FrameRef<'_>,
        frame_start: usize,
        frame_count: usize,
    ) -> i32 {
        let raw = json_of(frame);
        let frame_offset = self.parse_tween_frame(frame, frame_start, frame_count);
        let mut frame_int_offset = self.frame_int_array.len();
        self.reserve_frame_int(2);
        self.fintset(
            frame_int_offset,
            if get_boolean(raw, BEND_POSITIVE, true) {
                1
            } else {
                0
            },
        );
        frame_int_offset += 1;
        self.fintset(
            frame_int_offset,
            js_round(get_number(raw, WEIGHT, 1.0) * 100.0) as i32,
        );
        frame_offset
    }

    // ---- geometry ----

    fn parse_geometry(&mut self, raw: &Value, geometry: &mut GeometryData) {
        if self.binary.is_some() {
            self.parse_binary_geometry(raw, geometry);
            return;
        }
        let raw_vertices = raw.get(VERTICES).and_then(Value::as_array);
        let vertex_count = raw_vertices.map_or(0, |v| v.len() / 2);

        let mut triangle_count = 0usize;
        let geometry_offset = self.int_array.len();
        let vertices_offset = self.float_array.len();

        geometry.offset = geometry_offset as i32;
        // geometry.root 留到 finalize_root 回填（D-1.1）。

        self.reserve_int(1 + 1 + 1 + 1);
        self.iset(
            geometry_offset + binary_offset::geometry::VERTEX_COUNT,
            vertex_count as i32,
        );
        self.iset(
            geometry_offset + binary_offset::geometry::FLOAT_OFFSET,
            vertices_offset as i32,
        );
        self.iset(geometry_offset + binary_offset::geometry::WEIGHT_OFFSET, -1);

        self.reserve_float(vertex_count * 2);
        if let Some(raw_vertices) = raw_vertices {
            for (i, v) in raw_vertices.iter().take(vertex_count * 2).enumerate() {
                self.fset(vertices_offset + i, v.as_f64().unwrap_or(0.0));
            }
        }

        if let Some(raw_triangles) = raw.get(TRIANGLES).and_then(Value::as_array) {
            triangle_count = raw_triangles.len() / 3;
            self.reserve_int(triangle_count * 3);
            for (i, v) in raw_triangles.iter().take(triangle_count * 3).enumerate() {
                self.iset(
                    geometry_offset + binary_offset::geometry::VERTEX_INDICES + i,
                    v.as_f64().unwrap_or(0.0) as i32,
                );
            }
        }
        self.iset(
            geometry_offset + binary_offset::geometry::TRIANGLE_COUNT,
            triangle_count as i32,
        );

        if let Some(raw_uvs) = raw.get(UVS).and_then(Value::as_array) {
            let uv_offset = vertices_offset + vertex_count * 2;
            self.reserve_float(vertex_count * 2);
            for (i, v) in raw_uvs.iter().take(vertex_count * 2).enumerate() {
                self.fset(uv_offset + i, v.as_f64().unwrap_or(0.0));
            }
        }

        if let Some(raw_weights) = raw.get(WEIGHTS).and_then(Value::as_array) {
            let raw_weights: Vec<f64> = raw_weights
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0))
                .collect();
            let weight_count =
                ((raw_weights.len() as f64 - vertex_count as f64).floor() / 2.0) as usize;
            let weight_offset = self.int_array.len();
            let float_offset = self.float_array.len();
            let mut weight = WeightData {
                count: weight_count as i32,
                offset: weight_offset as i32,
                ..Default::default()
            };

            self.reserve_int(1 + 1 + vertex_count + weight_count);
            self.iset(
                weight_offset + binary_offset::weight::FLOAT_OFFSET,
                float_offset as i32,
            );

            if let Some(raw_bone_poses) = raw.get(BONE_POSE).and_then(Value::as_array) {
                let raw_slot_pose = raw
                    .get(SLOT_POSE)
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|v| v.as_f64().unwrap_or(0.0))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let raw_bone_poses: Vec<f64> = raw_bone_poses
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0))
                    .collect();

                let weight_bone_count = raw_bone_poses.len() / 7;
                let mut weight_bone_indices = Vec::with_capacity(weight_bone_count);

                let sorted_bones = self.armature.as_ref().unwrap().sorted_bones.clone();
                for i in 0..weight_bone_count {
                    let raw_bone_index = raw_bone_poses[i * 7] as usize;
                    let bone_name = self.raw_bones[raw_bone_index].clone();
                    weight.add_bone(bone_name.clone());
                    weight_bone_indices.push(raw_bone_index);
                    let idx = sorted_bones
                        .iter()
                        .position(|b| *b == bone_name)
                        .map_or(-1, |p| p as i32);
                    self.iset(weight_offset + binary_offset::weight::BONE_INDICES + i, idx);
                }

                self.reserve_float(weight_count * 3);
                self.help_matrix_a.copy_from_array(&raw_slot_pose, 0);

                let mut i_w = 0usize;
                let mut i_b =
                    weight_offset + binary_offset::weight::BONE_INDICES + weight_bone_count;
                let mut i_v = float_offset;
                for i in 0..vertex_count {
                    let i_d = i * 2;
                    let vertex_bone_count = raw_weights[i_w] as i32;
                    i_w += 1;
                    self.iset(i_b, vertex_bone_count);
                    i_b += 1;

                    let x = self.float_array[vertices_offset + i_d];
                    let y = self.float_array[vertices_offset + i_d + 1];
                    let (x, y) = self.help_matrix_a.transform_point(x, y, false);

                    for _ in 0..vertex_bone_count {
                        let raw_bone_index = raw_weights[i_w] as usize;
                        i_w += 1;
                        let bone_index = weight_bone_indices
                            .iter()
                            .position(|b| *b == raw_bone_index)
                            .map_or(0, |p| p as i32);
                        self.help_matrix_b
                            .copy_from_array(&raw_bone_poses, bone_index as usize * 7 + 1);
                        self.help_matrix_b.invert();
                        let (bx, by) = self.help_matrix_b.transform_point(x, y, false);
                        self.iset(i_b, bone_index);
                        i_b += 1;
                        self.fset(i_v, raw_weights[i_w]);
                        i_w += 1;
                        i_v += 1;
                        self.fset(i_v, bx);
                        i_v += 1;
                        self.fset(i_v, by);
                        i_v += 1;
                    }
                }
            } else if let Some(raw_bones) = raw.get(BONES).and_then(Value::as_array) {
                let raw_bone_indices: Vec<usize> = raw_bones
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as usize)
                    .collect();
                let sorted_bones = self.armature.as_ref().unwrap().sorted_bones.clone();
                for (i, &raw_bone_index) in raw_bone_indices.iter().enumerate() {
                    let bone_name = self.raw_bones[raw_bone_index].clone();
                    weight.add_bone(bone_name.clone());
                    let idx = sorted_bones
                        .iter()
                        .position(|b| *b == bone_name)
                        .map_or(-1, |p| p as i32);
                    self.iset(weight_offset + binary_offset::weight::BONE_INDICES + i, idx);
                }

                self.reserve_float(weight_count * 3);
                let mut i_w = 0usize;
                let mut i_v = 0usize;
                let mut i_b =
                    weight_offset + binary_offset::weight::BONE_INDICES + raw_bone_indices.len();
                let mut i_f = float_offset;
                for _ in 0..weight_count {
                    let vertex_bone_count = raw_weights[i_w] as i32;
                    i_w += 1;
                    self.iset(i_b, vertex_bone_count);
                    i_b += 1;

                    for _ in 0..vertex_bone_count {
                        let bone_index = raw_weights[i_w] as usize;
                        i_w += 1;
                        let bone_weight = raw_weights[i_w];
                        i_w += 1;
                        let x = raw_vertices
                            .and_then(|v| v.get(i_v))
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        i_v += 1;
                        let y = raw_vertices
                            .and_then(|v| v.get(i_v))
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        i_v += 1;

                        let idx = raw_bone_indices
                            .iter()
                            .position(|b| *b == bone_index)
                            .map_or(-1, |p| p as i32);
                        self.iset(i_b, idx);
                        i_b += 1;
                        self.fset(i_f, bone_weight);
                        i_f += 1;
                        self.fset(i_f, x);
                        i_f += 1;
                        self.fset(i_f, y);
                        i_f += 1;
                    }
                }
            }

            geometry.weight = Some(Box::new(weight));
        }
    }

    // ---- actions ----

    fn parse_action_data_in_frame(
        &mut self,
        raw: &Value,
        frame_start: i32,
        bone: Option<&str>,
        slot: Option<&str>,
    ) {
        if has_key(raw, EVENT) {
            self.merge_action_frame(&raw[EVENT], frame_start, ActionType::Frame, bone, slot);
        }
        if has_key(raw, SOUND) {
            self.merge_action_frame(&raw[SOUND], frame_start, ActionType::Sound, bone, slot);
        }
        if has_key(raw, ACTION) {
            self.merge_action_frame(&raw[ACTION], frame_start, ActionType::Play, bone, slot);
        }
        if has_key(raw, EVENTS) {
            self.merge_action_frame(&raw[EVENTS], frame_start, ActionType::Frame, bone, slot);
        }
        if has_key(raw, ACTIONS) {
            self.merge_action_frame(&raw[ACTIONS], frame_start, ActionType::Play, bone, slot);
        }
    }

    fn merge_action_frame(
        &mut self,
        raw: &Value,
        frame_start: i32,
        action_type: ActionType,
        bone: Option<&str>,
        slot: Option<&str>,
    ) {
        let action_offset = self.armature.as_ref().unwrap().actions.len() as i32;
        let actions = self.parse_action_data(raw, action_type, bone, slot);

        for action in &actions {
            self.armature
                .as_mut()
                .unwrap()
                .add_action(action.clone(), false);
        }

        if self.action_frames.is_empty() {
            self.action_frames.push(ActionFrame {
                frame_start: 0,
                actions: Vec::new(),
            });
        }

        let mut insert_at = self.action_frames.len();
        let mut found = false;
        for (idx, each) in self.action_frames.iter().enumerate() {
            if each.frame_start == frame_start {
                insert_at = idx;
                found = true;
                break;
            } else if each.frame_start > frame_start {
                insert_at = idx;
                break;
            }
        }
        if !found {
            self.action_frames.insert(
                insert_at,
                ActionFrame {
                    frame_start,
                    actions: Vec::new(),
                },
            );
        }

        for i in 0..actions.len() {
            self.action_frames[insert_at]
                .actions
                .push(action_offset + i as i32);
        }
    }

    fn parse_action_data(
        &mut self,
        raw: &Value,
        action_type: ActionType,
        bone: Option<&str>,
        slot: Option<&str>,
    ) -> Vec<ActionData> {
        let mut actions = Vec::new();

        if let Some(value) = raw.as_str() {
            actions.push(ActionData {
                action_type,
                name: value.to_string(),
                bone: bone.map(str::to_string),
                slot: slot.map(str::to_string),
                data: None,
            });
        } else if let Some(list) = raw.as_array() {
            for raw_action in list {
                let mut action = ActionData::default();

                if has_key(raw_action, GOTO_AND_PLAY) {
                    action.action_type = ActionType::Play;
                    action.name = get_string(raw_action, GOTO_AND_PLAY, "");
                } else {
                    action.action_type = if has_key(raw_action, TYPE)
                        && raw_action[TYPE].is_string()
                    {
                        get_action_type(&get_string(raw_action, TYPE, ""))
                    } else {
                        num_to_action_type(get_number(raw_action, TYPE, action_type as u32 as f64))
                    };
                    action.name = get_string(raw_action, NAME, "");
                }

                action.bone = if has_key(raw_action, BONE) {
                    let name = get_string(raw_action, BONE, "");
                    self.armature
                        .as_ref()
                        .unwrap()
                        .get_bone(&name)
                        .map(|_| name)
                } else {
                    bone.map(str::to_string)
                };

                action.slot = if has_key(raw_action, SLOT) {
                    let name = get_string(raw_action, SLOT, "");
                    self.armature
                        .as_ref()
                        .unwrap()
                        .get_slot(&name)
                        .map(|_| name)
                } else {
                    slot.map(str::to_string)
                };

                let mut user_data: Option<UserData> = None;
                if let Some(raw_ints) = raw_action.get(INTS).and_then(Value::as_array) {
                    let ud = user_data.get_or_insert_with(UserData::default);
                    for v in raw_ints {
                        ud.add_int(v.as_f64().unwrap_or(0.0) as i32);
                    }
                }
                if let Some(raw_floats) = raw_action.get(FLOATS).and_then(Value::as_array) {
                    let ud = user_data.get_or_insert_with(UserData::default);
                    for v in raw_floats {
                        ud.add_float(v.as_f64().unwrap_or(0.0));
                    }
                }
                if let Some(raw_strings) = raw_action.get(STRINGS).and_then(Value::as_array) {
                    let ud = user_data.get_or_insert_with(UserData::default);
                    for v in raw_strings {
                        ud.add_string(v.as_str().unwrap_or(""));
                    }
                }

                action.data = user_data.map(Box::new);
                actions.push(action);
            }
        }

        actions
    }

    // ---- easing curve ----

    fn sampling_easing_curve(&mut self, curve: &[f64], samples: &mut [f64]) -> bool {
        let curve_count = curve.len();

        if curve_count % 3 == 1 {
            let mut step_index: isize = -2;
            for i in 0..samples.len() {
                let t = (i + 1) as f64 / (samples.len() + 1) as f64;
                while (if step_index + 6 < curve_count as isize {
                    curve[(step_index + 6) as usize]
                } else {
                    1.0
                }) < t
                {
                    step_index += 6;
                }

                let is_in_curve = step_index >= 0 && step_index + 6 < curve_count as isize;
                let x1 = if is_in_curve {
                    curve[step_index as usize]
                } else {
                    0.0
                };
                let y1 = if is_in_curve {
                    curve[(step_index + 1) as usize]
                } else {
                    0.0
                };
                let x2 = curve[(step_index + 2) as usize];
                let y2 = curve[(step_index + 3) as usize];
                let x3 = curve[(step_index + 4) as usize];
                let y3 = curve[(step_index + 5) as usize];
                let x4 = if is_in_curve {
                    curve[(step_index + 6) as usize]
                } else {
                    1.0
                };
                let y4 = if is_in_curve {
                    curve[(step_index + 7) as usize]
                } else {
                    1.0
                };

                let mut lower = 0.0;
                let mut higher = 1.0;
                while higher - lower > 0.0001 {
                    let percentage = (higher + lower) * 0.5;
                    let (px, py) = get_curve_point(x1, y1, x2, y2, x3, y3, x4, y4, percentage);
                    self.help_point.x = px;
                    self.help_point.y = py;
                    if t - self.help_point.x > 0.0 {
                        lower = percentage;
                    } else {
                        higher = percentage;
                    }
                }

                samples[i] = self.help_point.y;
            }

            true
        } else {
            let mut step_index = 0usize;
            for i in 0..samples.len() {
                let t = (i + 1) as f64 / (samples.len() + 1) as f64;
                while curve[step_index + 6] < t {
                    step_index += 6;
                }

                let x1 = curve[step_index];
                let y1 = curve[step_index + 1];
                let x2 = curve[step_index + 2];
                let y2 = curve[step_index + 3];
                let x3 = curve[step_index + 4];
                let y3 = curve[step_index + 5];
                let x4 = curve[step_index + 6];
                let y4 = curve[step_index + 7];

                let mut lower = 0.0;
                let mut higher = 1.0;
                while higher - lower > 0.0001 {
                    let percentage = (higher + lower) * 0.5;
                    let (px, py) = get_curve_point(x1, y1, x2, y2, x3, y3, x4, y4, percentage);
                    self.help_point.x = px;
                    self.help_point.y = py;
                    if t - self.help_point.x > 0.0 {
                        lower = percentage;
                    } else {
                        higher = percentage;
                    }
                }

                samples[i] = self.help_point.y;
            }

            false
        }
    }

    fn surface_geometry(&self, bone_name: &str) -> Option<(i32, Option<WeightData>)> {
        self.armature
            .as_ref()
            .and_then(|a| a.get_bone(bone_name))
            .and_then(|b| match &b.kind {
                BoneKind::Surface { geometry, .. } => {
                    Some((geometry.offset, geometry.weight.as_deref().cloned()))
                }
                BoneKind::Bone => None,
            })
    }

    fn find_deform_geometry_by_name(
        &self,
        display_name: &str,
    ) -> Option<(i32, Option<WeightData>)> {
        let armature = self.armature.as_ref()?;
        for skin in armature.skins.values() {
            for displays in skin.displays.values() {
                for display in displays.iter().flatten() {
                    if display.name == display_name
                        && let DisplayKind::Mesh { geometry, .. } = &display.kind
                    {
                        return Some((geometry.offset, geometry.weight.as_deref().cloned()));
                    }
                }
            }
        }
        None
    }

    fn mesh_geometry(
        &self,
        skin_name: &str,
        slot_name: &str,
        mesh_name: &str,
    ) -> Option<(i32, Option<WeightData>)> {
        let armature = self.armature.as_ref()?;
        let display = armature
            .get_skin(skin_name)?
            .get_displays(slot_name)?
            .iter()
            .flatten()
            .find(|d| d.name == mesh_name)?;
        if let DisplayKind::Mesh { geometry, .. } = &display.kind {
            Some((geometry.offset, geometry.weight.as_deref().cloned()))
        } else {
            None
        }
    }
}

// ---- 自由函数 ----

fn json_of<'a>(frame: &FrameRef<'a>) -> &'a Value {
    match frame {
        FrameRef::Json(v) => v,
        FrameRef::Action(_) => panic!("expected JSON frame"),
    }
}

/// `apply`：`rawData[framesKey]`（存在且非空）。
fn frames_from<'a>(raw: &'a Value, key: &str) -> Option<Vec<FrameRef<'a>>> {
    let arr = raw.get(key)?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    Some(arr.iter().map(FrameRef::Json).collect())
}

#[allow(clippy::too_many_arguments)]
fn get_curve_point(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    x4: f64,
    y4: f64,
    t: f64,
) -> (f64, f64) {
    let l_t = 1.0 - t;
    let pow_a = l_t * l_t;
    let pow_b = t * t;
    let k_a = l_t * pow_a;
    let k_b = 3.0 * t * pow_a;
    let k_c = 3.0 * l_t * pow_b;
    let k_d = t * pow_b;

    (
        k_a * x1 + k_b * x2 + k_c * x3 + k_d * x4,
        k_a * y1 + k_b * y2 + k_c * y3 + k_d * y4,
    )
}

fn parse_transform(raw: &Value, transform: &mut Transform, scale: f64) {
    transform.x = get_number(raw, X, 0.0) * scale;
    transform.y = get_number(raw, Y, 0.0) * scale;

    if has_key(raw, ROTATE) || has_key(raw, SKEW) {
        transform.rotation = normalize_radian(get_number(raw, ROTATE, 0.0) * DEG_RAD);
        transform.skew = normalize_radian(get_number(raw, SKEW, 0.0) * DEG_RAD);
    } else if has_key(raw, SKEW_X) || has_key(raw, SKEW_Y) {
        transform.rotation = normalize_radian(get_number(raw, SKEW_Y, 0.0) * DEG_RAD);
        transform.skew =
            normalize_radian(get_number(raw, SKEW_X, 0.0) * DEG_RAD) - transform.rotation;
    }

    transform.scale_x = get_number(raw, SCALE_X, 1.0);
    transform.scale_y = get_number(raw, SCALE_Y, 1.0);
}

fn parse_color_transform(raw: &Value, color: &mut ColorTransform) {
    color.alpha_multiplier = get_number(raw, ALPHA_MULTIPLIER, 100.0) * 0.01;
    color.red_multiplier = get_number(raw, RED_MULTIPLIER, 100.0) * 0.01;
    color.green_multiplier = get_number(raw, GREEN_MULTIPLIER, 100.0) * 0.01;
    color.blue_multiplier = get_number(raw, BLUE_MULTIPLIER, 100.0) * 0.01;
    color.alpha_offset = get_number(raw, ALPHA_OFFSET, 0.0);
    color.red_offset = get_number(raw, RED_OFFSET, 0.0);
    color.green_offset = get_number(raw, GREEN_OFFSET, 0.0);
    color.blue_offset = get_number(raw, BLUE_OFFSET, 0.0);
}

// ---- 数字→枚举（TS `_getNumber` 分支）----

fn num_to_armature_type(v: f64) -> crate::core::enums::ArmatureType {
    use crate::core::enums::ArmatureType::*;
    match v as u32 {
        1 => MovieClip,
        2 => Stage,
        _ => Armature,
    }
}

fn num_to_bone_type(v: f64) -> BoneType {
    match v as u32 {
        1 => BoneType::Surface,
        _ => BoneType::Bone,
    }
}

fn num_to_blend_mode(v: f64) -> BlendMode {
    match v as u32 {
        1 => BlendMode::Add,
        2 => BlendMode::Alpha,
        3 => BlendMode::Darken,
        4 => BlendMode::Difference,
        5 => BlendMode::Erase,
        6 => BlendMode::HardLight,
        7 => BlendMode::Invert,
        8 => BlendMode::Layer,
        9 => BlendMode::Lighten,
        10 => BlendMode::Multiply,
        11 => BlendMode::Overlay,
        12 => BlendMode::Screen,
        13 => BlendMode::Subtract,
        _ => BlendMode::Normal,
    }
}

fn num_to_display_type(v: f64) -> DisplayType {
    match v as u32 {
        1 => DisplayType::Armature,
        2 => DisplayType::Mesh,
        3 => DisplayType::BoundingBox,
        4 => DisplayType::Path,
        _ => DisplayType::Image,
    }
}

fn num_to_bounding_box_type(v: f64) -> crate::core::enums::BoundingBoxType {
    use crate::core::enums::BoundingBoxType::*;
    match v as u32 {
        1 => Ellipse,
        2 => Polygon,
        _ => Rectangle,
    }
}

fn num_to_action_type(v: f64) -> ActionType {
    match v as u32 {
        10 => ActionType::Frame,
        11 => ActionType::Sound,
        _ => ActionType::Play,
    }
}

fn num_to_timeline_type(v: f64) -> TimelineType {
    use crate::core::enums::TimelineType::*;
    match v as u32 {
        0 => Action,
        1 => ZOrder,
        10 => BoneAll,
        11 => BoneTranslate,
        12 => BoneRotate,
        13 => BoneScale,
        20 => SlotDisplay,
        21 => SlotColor,
        22 => SlotDeform,
        23 => SlotZIndex,
        24 => SlotAlpha,
        30 => IKConstraint,
        40 => AnimationProgress,
        41 => AnimationWeight,
        42 => AnimationParameter,
        50 => Surface,
        60 => BoneAlpha,
        _ => Action,
    }
}
