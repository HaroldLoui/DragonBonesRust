//! `armature/Slot.ts`（1316 行）——任务 5 实现（第二大单体）。
//!
//! 本文件落地四类内容：
//! 1. [`SlotRenderAdapter`]：TS `Slot.ts:332-348` 的 14 个 abstract 方法 → trait
//!    （D-3.4；其中 `_updateVisible` 是 `public abstract`，TS:342——其余 13 个
//!    `protected abstract`），阶段 7 换 Bevy 实现（**签名冻结**）；
//! 2. [`NullSlotAdapter`]：= 阶段 0 `jsdump.mjs` 的 `NullSlot`（14 方法 no-op）；
//! 3. [`DisplayFrame`]（Slot.ts:27-139）与 [`SlotUpdateCtx`]（Armature 侧逐槽上下文）；
//! 4. [`Slot`]：字段全集 + `init` / `update` 脏标记状态机 / `_updateDisplayData` /
//!    `_updateDisplay`（子骨架接线）/ `replace*` 系列 / `_onClear`，逐行对照 TS。
//!
//! **官方有、暂缺（阶段 6/7 登记，实测 TS 行号）**：
//! - `displayList` get/set（Slot.ts:1146-1160）——setter 内 `displayFrameCount =
//!   value.length` + 逐项 `replaceDisplay`，依赖 `set_display_frame_count` 的收缩
//!   语义（官方 TS:1083-1089 死循环 → 实际仅截断，见该方法注释）；
//! - `display` setter（Slot.ts:1224-1235）——空帧表时 `displayFrameCount = 1` +
//!   `_displayIndex = 0` 再 `replaceDisplay`；
//! - `childArmature` setter（Slot.ts:1268-1274）——透传 `this.display = value`；
//! - `getDisplayFrameAt(index)`（Slot.ts:1042-1044）——`_displayFrames[index]` 裸下标；
//! - [`Slot::contains_point`] / [`Slot::intersects_segment`] 现签名 `&self`，
//!   阶段 6 实现要先调 `updateTransformAndMatrix`（Slot.ts:804-809，内部置脏/写
//!   矩阵）→ 届时需改 `&mut self`，**破坏性改签在此登记**（调用方多为命中检测
//!   外层，阶段 6 一并迁）；
//! - 缓存分支 `indices[cache_frame_index]`（本文件 `update` ⑩，现 :647，
//!   评审登记时点 :615；官方 Slot.ts:741）：官方对越界取 `undefined` → 后续
//!   `>= 0` 判定为 falsey（走 "Dirty" 段）；Rust `Vec` 下标会 panic → 阶段 6
//!   接入 `_cachedFrameIndices` 时需先对齐长度或夹取（`get` + None ≡ undefined），
//!   登记在此勿直接裸下标。
//!
//! 借用契约（阶段 3，见指南任务 6 / 笔记 C 节）：推进期间**自身骨架不在 arena**
//! （`RuntimeCtx` 标记的当前 Armature 槽位为 `None`）——`update` 内凡“访问自身骨架”的需求
//! 一律走 [`SlotUpdateCtx`]（actions / idx / cacheFrameRate），访问子/父骨架走
//! [`RuntimeCtx`]；**不要**试图 `world.armature(ctx.armature)`。
//!
//! 数据引用形态（D-3.3）：运行时不借 data 生命周期——`DisplayFrame` 内的
//! `DisplayData` / `TextureData` 为**值克隆**（data 解析后不可变，F3.3，值拷贝
//! 与引用语义恒等；TS 的对象身份比较在 Rust 落成值比较/下标比较，见各处注释）。

use std::sync::Arc;

use super::transform_object::TransformObject;
use super::{Armature, ArmatureIdx, Bone, BoneIdx, DisplayHandle, RuntimeCtx, SlotIdx};
use crate::animation::world_clock::ClockId;
use crate::core::dragon_bones::DragonBones;
use crate::core::enums::{BlendMode, BoneType, DisplayType};
use crate::event::EventObject;
use crate::geom::{ColorTransform, Matrix, Point};
use crate::model::{
    ActionData, ArmatureData, BoundingBoxData, DisplayData, DisplayKind, DragonBonesData,
    GeometryData, SlotData, TextureData,
};

/// Slot 的 14 抽象方法（对齐 `Slot.ts:332-348`，D-3.4）——
/// Bevy 适配层（阶段 7）的全部契约。
///
/// 注意：阶段 3 trait 方法**不携带 Slot 状态参数**（TS 是子类直接读自身字段），
/// Null 实现全 no-op；阶段 7 若需状态，届时扩展签名（记对照表）。
pub trait SlotRenderAdapter {
    /// `_initDisplay(value, isRetain)`（Slot.ts:332）。
    fn init_display(&mut self, value: DisplayHandle, retain: bool);
    /// `_disposeDisplay(value, isRelease)`（Slot.ts:333）。
    fn dispose_display(&mut self, value: DisplayHandle, release: bool);
    /// `_onUpdateDisplay()`（Slot.ts:334）——jsdump：`this._rd = this._display || this._rawDisplay`。
    fn update_display(&mut self);
    /// `_addDisplay()`（Slot.ts:335）。
    fn add_display(&mut self);
    /// `_replaceDisplay(value)`（Slot.ts:336）。
    fn replace_display(&mut self, prev: DisplayHandle);
    /// `_removeDisplay()`（Slot.ts:337）。
    fn remove_display(&mut self);
    /// `_updateZOrder()`（Slot.ts:338）。
    fn update_z_order(&mut self);
    /// `_updateVisible()`（Slot.ts:342，`public abstract`）。
    fn update_visible(&mut self);
    /// `_updateBlendMode()`（Slot.ts:343）。
    fn update_blend_mode(&mut self);
    /// `_updateColor()`（Slot.ts:344）。
    fn update_color(&mut self);
    /// `_updateFrame()`（Slot.ts:345）。
    fn update_frame(&mut self);
    /// `_updateMesh()`（Slot.ts:346）。
    fn update_mesh(&mut self);
    /// `_updateTransform()`（Slot.ts:347）。
    fn update_transform(&mut self);
    /// `_identityTransform()`（Slot.ts:348）。
    fn identity_transform(&mut self);
}

/// Null 实现 = `tools/jsdump.mjs:25-41` 的 `NullSlot`（14 方法全部 no-op）。
///
/// 偏差：jsdump 的 `_onUpdateDisplay` 里 `this._rd = …` 赋值无任何消费者，
/// Rust adapter 不持有 Slot 状态，故该行不复刻（纯 no-op，无行为影响）。
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSlotAdapter;

impl SlotRenderAdapter for NullSlotAdapter {
    fn init_display(&mut self, _value: DisplayHandle, _retain: bool) {}
    fn dispose_display(&mut self, _value: DisplayHandle, _release: bool) {}
    fn update_display(&mut self) {}
    fn add_display(&mut self) {}
    fn replace_display(&mut self, _prev: DisplayHandle) {}
    fn remove_display(&mut self) {}
    fn update_z_order(&mut self) {}
    fn update_visible(&mut self) {}
    fn update_blend_mode(&mut self) {}
    fn update_color(&mut self) {}
    fn update_frame(&mut self) {}
    fn update_mesh(&mut self) {}
    fn update_transform(&mut self) {}
    fn identity_transform(&mut self) {}
}

/// 显示帧（TS `Slot.ts:27-139` 的 `DisplayFrame`）。
///
/// 字段语义（笔记 A1 表）：
/// - `raw_display_data`：皮肤无关的“原始”显示数据（`_buildSlots` 经
///   `replace_raw_display_data` 写入；换皮肤时保留以便回退）。
/// - `display_data`：当前生效（被皮肤/`replace_display_data` 覆盖后）的显示数据。
/// - `texture_data`：显式覆盖的纹理（`replace_texture_data` 写入；TS 的
///   `getTextureData()` 三段式里 displayData 自带纹理的挂接在 Rust 由工厂
///   统一经 `replace_texture_data` 落进本字段，见 [`Self::get_texture_data`]）。
/// - `display`：实际显示对象，**或者是子骨架**（`DisplayHandle::Armature`）。
/// - `deform_vertices`：网格形变顶点缓冲（长度 = 蒙皮 `weight.count * 2` 或
///   GeometryVertexCount * 2，初始化为 0.0；阶段 6 P1-4 用）。
///
/// `DisplayData` / `TextureData` 为**值克隆**（D-3.3：data 解析后不可变，值拷贝
/// 与 TS 引用语义恒等）——TS 的 `!==` 身份比较在 Rust 落成 `PartialEq` 值比较：
/// `replace*` 的判重与 `_updateDisplayData` 的 diff 对“同值不同对象”场景的
/// 差异经论证无行为影响（同值 ⟹ 同变换/同几何/同纹理 → 刷与不刷结果一致）。
///
/// 默认值 = `_onClear` 复位值（Slot.ts:38-44），`derive(Default)` 逐字段等价。
#[derive(Debug, Default)]
pub struct DisplayFrame {
    /// 对齐 `rawDisplayData`（Slot.ts:32）。
    pub raw_display_data: Option<DisplayData>,
    /// 对齐 `displayData`（Slot.ts:33）。
    pub display_data: Option<DisplayData>,
    /// 对齐 `textureData`（Slot.ts:34）。
    pub texture_data: Option<TextureData>,
    /// 对齐 `display`（Slot.ts:35）——`None` = TS null；`Armature(idx)` = 子骨架。
    pub display: Option<DisplayHandle>,
    /// 对齐 `deformVertices`（Slot.ts:36，readonly number[]；`_onClear` `.length = 0`）。
    pub deform_vertices: Vec<f64>,
}

impl DisplayFrame {
    /// 对齐 `DisplayFrame.updateDeformVertices()`（Slot.ts:46-74）——
    /// 阶段 6（P1-4 deform 时间轴）用，阶段 3 无调用点。
    pub fn update_deform_vertices(&mut self) {
        // :47 只初始化一次（rawDisplayData 为空或已有缓冲 → 直接返回）
        if self.raw_display_data.is_none() || !self.deform_vertices.is_empty() {
            return;
        }

        // :51-60 Mesh / Path 才有 geometry，否则返回
        let raw_geometry_data: &GeometryData = match self.raw_display_data.as_ref().map(|d| &d.kind)
        {
            Some(DisplayKind::Mesh { geometry, .. }) => geometry,
            Some(DisplayKind::Path { geometry, .. }) => geometry,
            _ => return,
        };

        // :62-68 蒙皮按 weight.count，普通几何按 IR 的 GeometryVertexCount（各 *2）
        let vertex_count = if let Some(weight) = &raw_geometry_data.weight {
            weight.count * 2
        } else {
            raw_geometry_data.vertex_count() * 2
        };

        // :70-73 长度置位后逐个填 0.0
        self.deform_vertices.clear();
        self.deform_vertices
            .resize(vertex_count.max(0) as usize, 0.0);
    }

    /// 对齐 `DisplayFrame.getGeometryData()`（Slot.ts:76-98）——
    /// displayData → rawDisplayData 两段式（没有中间覆盖字段）。
    #[must_use]
    pub fn get_geometry_data(&self) -> Option<&GeometryData> {
        if let Some(display_data) = &self.display_data {
            // :78-84
            match &display_data.kind {
                DisplayKind::Mesh { geometry, .. } => return Some(geometry),
                DisplayKind::Path { geometry, .. } => return Some(geometry),
                _ => {}
            }
        }

        if let Some(raw_display_data) = &self.raw_display_data {
            // :88-94
            match &raw_display_data.kind {
                DisplayKind::Mesh { geometry, .. } => return Some(geometry),
                DisplayKind::Path { geometry, .. } => return Some(geometry),
                _ => {}
            }
        }

        None // :97
    }

    /// 对齐 `DisplayFrame.getBoundingBox()`（Slot.ts:100-110）。
    #[must_use]
    pub fn get_bounding_box(&self) -> Option<&BoundingBoxData> {
        // :101-103 displayData 是 BoundingBox → 直接返回它自己的 boundingBox；
        // 即使该值为 null（Rust `None`）也**就此返回**（TS 分支内即 return，
        // null 不落回 rawDisplayData）——勿回落 raw 段。
        if let Some(display_data) = &self.display_data
            && let DisplayKind::BoundingBox { bounding_box } = &display_data.kind
        {
            return bounding_box.as_deref();
        }

        // :105-108 displayData 非 BoundingBox 才回退 rawDisplayData（同理：raw 是
        // BoundingBox 时无条件返回其值，含 null）。
        if let Some(raw_display_data) = &self.raw_display_data
            && let DisplayKind::BoundingBox { bounding_box } = &raw_display_data.kind
        {
            return bounding_box.as_deref();
        }

        None // :109
    }

    /// 对齐 `DisplayFrame.getTextureData()`（Slot.ts:112-138）。
    ///
    /// TS 是三段式（displayData 自带 → 显式覆盖字段 `this.textureData` →
    /// rawDisplayData 自带）。Rust 侧 `DisplayData.texture` 是图集内**名字**
    /// （D-1.1），TS 在 factory `_getSlotDisplay`（BaseFactory.ts:265/275）把
    /// `_getTextureData(...)` 解析出的 TextureData 挂到 displayData.texture；
    /// Rust 工厂（任务 7）把同一解析结果经 `replace_texture_data` 落进
    /// `frame.texture_data`——两来源在 TS 同为同一对象，故三段式收敛为
    /// “显式字段”一段（运行时换装 P1-5 若出现 displayData 自带与覆盖字段
    /// 不一致的场景，届时在工厂侧按 displayData 优先级覆写本字段）。
    #[must_use]
    pub fn get_texture_data(&self) -> Option<&TextureData> {
        self.texture_data.as_ref()
    }
}
/// `Slot.update` 的逐槽上下文（任务 6：Armature 侧循环每槽构造一次）。
///
/// 推进期间自身骨架不在 arena（`RuntimeCtx` 当前 Armature 槽位为 `None`，阶段 3
/// 借用契约）——凡“访问自身骨架”的需求走本结构（actions / idx / cacheFrameRate），
/// 访问子/父骨架走 [`RuntimeCtx`]。
pub struct SlotUpdateCtx<'a> {
    /// 父骨（按 `Slot::parent` 取；TS `_parent`，init 后必非空——Slot.ts:656-663）。
    pub parent: &'a Bone,
    /// 自身骨架的骨骼表（`is_bones_update` 遍历 / geometryBones 按名解析，
    /// Slot.ts:363/487）。
    pub bones: &'a [Bone],
    /// 自身骨架的 action 队列（子骨架 defaultActions 头插 = `_bufferAction(_, false)`，
    /// Slot.ts:571 / Armature.ts:235-242）。
    pub actions: &'a mut Vec<EventObject>,
    /// 自身骨架索引（事件回指 Slot.ts:569、子骨架 parent 接线 Slot.ts:548）。
    pub armature: ArmatureIdx,
    /// 自身骨架当前 clock；Slot.ts:549 将其继承给新 child armature。
    pub clock: Option<ClockId>,
    /// 自身骨架 cacheFrameRate（Slot.ts:552 `this._armature.cacheFrameRate`；
    /// 阶段 3 data 未设 cacheFrames 时为 0）。
    pub cache_frame_rate: f64,
}

/// 对齐 `Slot extends TransformObject`（Slot.ts:164 类声明行，实测）。
///
/// 跨模块契约字段（**勿改名**）：
/// - `base`：TransformObject 组合 + `Deref` 透传
/// - `idx`：本骨架内插槽索引（init 里按 `armature.slots.len()` 预置，见 [`Self::init`]）
/// - `slot_name`：→ `Armature::get_slot` 按名查找
/// - `parent`：按 `SlotData.parent` 名解析的父骨（Slot.ts:656；无 Option——数据必有父骨）
///
/// `_slotData: SlotData` 的 Rust 形态（**D-3.3 缓存字段方案**，指南明文推荐）：
/// SlotData 是 data 层对象（factory / `Arc<DragonBonesData>` 持有），运行时**不持
/// `&SlotData` 生命周期**（会污染 arena 所有权），只缓存需要的字段——
/// `name` → `slot_name`、`blendMode/zOrder/zIndex/alpha/color` → 同名字段
/// （`alpha` 在 `base`）、`parent` → 解析后的 `parent: BoneIdx`、
/// `displayIndex` → 由工厂 `set_display_index(_, true)` 写入（BaseFactory.ts:222，
/// **非 init**——TS init 不读 slotData.displayIndex）。
pub struct Slot {
    /// TransformObject 继承（TS `super` 字段）。
    pub base: TransformObject,
    /// 本骨架内插槽索引（init 预置 = 推入 `Armature.slots` 后的下标）。
    pub idx: SlotIdx,
    /// 插槽名（= `SlotData.name`，Slot.ts:1133-1135）。
    pub slot_name: String,
    /// 父骨索引（TS `_parent: Bone`，Slot.ts:656 按名解析，"Never;" null 分支不出现）。
    pub parent: BoneIdx,

    /// 对齐 `displayController`（Slot.ts:183），`_onClear` = null（:294）。
    pub display_controller: Option<String>,
    /// 对齐 `_displayDataDirty`（Slot.ts:184），`_onClear` = false（:296）。
    pub display_data_dirty: bool,
    /// 对齐 `_displayDirty`（Slot.ts:185），`_onClear` = false（:297）。
    pub display_dirty: bool,
    /// 对齐 `_geometryDirty`（Slot.ts:186），`_onClear` = false（:298）。
    pub geometry_dirty: bool,
    /// 对齐 `_textureDirty`（Slot.ts:187），`_onClear` = false（:299）。
    pub texture_dirty: bool,
    /// 对齐 `_visibleDirty`（Slot.ts:188），`_onClear` = false（:300）。
    pub visible_dirty: bool,
    /// 对齐 `_blendModeDirty`（Slot.ts:189），`_onClear` = false（:301）。
    pub blend_mode_dirty: bool,
    /// 对齐 `_zOrderDirty`（Slot.ts:190），`_onClear` = false（:302）。
    pub z_order_dirty: bool,
    /// 对齐 `_colorDirty`（Slot.ts:194，`@internal`），`_onClear` = false（:303）。
    pub color_dirty: bool,
    /// 对齐 `_verticesDirty`（Slot.ts:198，`@internal`），`_onClear` = false（:304）。
    pub vertices_dirty: bool,
    /// 对齐 `_transformDirty`（Slot.ts:199），`_onClear` = false（:305）。
    pub transform_dirty: bool,
    /// 对齐 `_visible`（Slot.ts:200），`_onClear` = **true**（:306）。
    pub visible: bool,
    /// 对齐 `_blendMode`（Slot.ts:201），`_onClear` = `BlendMode::Normal`（:307）。
    pub blend_mode: BlendMode,
    /// 对齐 `_displayIndex`（Slot.ts:202），`_onClear` = -1（:308）。
    pub display_index: i32,
    /// 对齐 `_animationDisplayIndex`（Slot.ts:203），`_onClear` = -1（:309）——
    /// 只在 [`Slot::set_display_index`] 内被读写（笔记 A7.6）。
    pub animation_display_index: i32,
    /// 对齐 `_zOrder`（Slot.ts:208，`@internal`），`_onClear` = 0（:310）。
    pub z_order: i32,
    /// 对齐 `_zIndex`（Slot.ts:212，`@internal`），`_onClear` = 0（:311）。
    pub z_index: i32,
    /// 对齐 `_cachedFrameIndex`（Slot.ts:204），`_onClear` = -1（:312）；
    /// 阶段 3 恒 -1（P1-4 缓存帧阶段 6）。
    pub cached_frame_index: i32,
    /// 对齐 `_pivotX`（Slot.ts:216，`@internal`），`_onClear` = 0.0（:313）。
    pub pivot_x: f64,
    /// 对齐 `_pivotY`（Slot.ts:220，`@internal`），`_onClear` = 0.0（:314）。
    pub pivot_y: f64,
    /// 对齐 `_localMatrix`（Slot.ts:221，readonly），`_onClear` `.identity()`（:315）。
    pub local_matrix: Matrix,
    /// 对齐 `_colorTransform`（Slot.ts:225，readonly `@internal`），`_onClear` `.identity()`（:316）。
    pub color_transform: ColorTransform,
    /// 对齐 `_displayFrames`（Slot.ts:229，readonly `@internal`），`_onClear` `.length = 0`（:317）。
    pub display_frames: Vec<DisplayFrame>,
    /// 对齐 `_geometryBones`（Slot.ts:233，readonly `@internal`），`_onClear` `.length = 0`（:318）——
    /// `None` = TS 的 null（`getBone` 按名解析失败，Slot.ts:487-488）。
    pub geometry_bones: Vec<Option<BoneIdx>>,
    /// 对齐 `_displayFrame`（Slot.ts:241，`@internal`），`_onClear` = null（:320）——
    /// TS 存 DisplayFrame **对象引用**；Rust 存**下标** `Option<usize>`：
    /// 同帧原地 mutate（replace*）时 TS 对象身份不变 ⟺ Rust 下标不变（等价）；
    /// 下标复用（shrink 后 regrow）时靠 `_updateDisplayData` 的 geometry/texture
    /// 值比较兜底（内容相同 → 无需刷）。
    pub display_frame: Option<usize>,
    /// 对齐 `_geometryData`（Slot.ts:245），`_onClear` = null（:321）——
    /// `display_frames[display_frame]` 内 geometry 的值克隆（归属见 DisplayFrame 文档）。
    pub geometry_data: Option<GeometryData>,
    /// 对齐 `_boundingBoxData`（Slot.ts:246），`_onClear` = null（:322）。
    pub bounding_box_data: Option<BoundingBoxData>,
    /// 对齐 `_textureData`（Slot.ts:247），`_onClear` = null（:323）。
    pub texture_data: Option<TextureData>,
    /// 对齐 `_rawDisplay`（Slot.ts:248），`_onClear` = null（:324）。
    pub raw_display: DisplayHandle,
    /// 对齐 `_meshDisplay`（Slot.ts:249），`_onClear` = null（:325）；可能与 raw 相同。
    pub mesh_display: DisplayHandle,
    /// 对齐 `_display`（Slot.ts:250），`_onClear` = null（:326）。
    pub display: Option<DisplayHandle>,
    /// 对齐 `_childArmature`（Slot.ts:251），`_onClear` = null（:327）——D-3.2 索引解环。
    pub child_armature: Option<ArmatureIdx>,
    /// 对齐 `_cachedFrameIndices`（Slot.ts:259，`@internal`），`_onClear` = null（:329）；
    /// 阶段 3 恒 `None`（P1-4 缓存帧阶段 6）。
    pub cached_frame_indices: Option<Vec<i32>>,

    // —— Rust 侧（D-3.3 缓存 / 渲染契约，无对应 TS 行）——
    /// 阶段 2 产物根（D-3.3：`Arc` 持有、data 解析后不可变）——
    /// `_updateDisplayData` 的骨架缩放（Slot.ts:412）与
    /// `replace_raw_display_data` 的 defaultSkin 回填（Slot.ts:826-831）用。
    pub data: Arc<DragonBonesData>,
    /// 骨架名（→ `data.armatures[name]`；与 `slot_name` 一起构成缓存字段方案，
    /// `on_clear` 清空以对齐 TS `_slotData = null` / `_armature = null`）。
    pub armature_name: String,
    /// 图集缩放（Rust 侧缓存）：TS 每次 `_updateDisplayData` 逐贴图取
    /// `_textureData.parent.scale * armatureData.scale`（Slot.ts:412，**随当前贴图
    /// 所属图集**），Rust `TextureData.parent` 是图集名（D-1.1）→ 工厂在
    /// [`Slot::init`] 下传一个 build 级标量（`BaseFactory::texture_atlas_scale`
    /// 取 atlasName/dataName 首命中图集）。
    /// **不改行为**（阶段 3 单图集 build 足够，工厂侧同款标注见
    /// `base_factory.rs::texture_atlas_scale`）。已知会取错的三个场景：
    /// ① `_getTextureData` ② 段（`factory.autoSearch` 全表扫，BaseFactory.ts:94-105）
    ///    命中**别的 data** 的 atlas——build 查表仍取 atlasName/dataName 首命中；
    /// ② 同名多 atlas（`atlas_map[key]` 是 Vec，笔记 A.2-4）——查表取 `first()`，
    ///    而实际贴图可能命中列表里第 2+ 个不同 scale 的 atlas；
    /// ③ `parse_texture_atlas_data` 解析失败仍无条件入 map 的空壳（BaseFactory.ts:406，
    ///    scale 停留默认 1.0）占列表首位 → `first()` 取空壳而非真实图集。
    /// TODO P1-5（阶段 6）修法：`DisplayFrame` 随 [`Slot::replace_texture_data`]
    /// 记下该帧实际图集 scale（`texture_atlas_scale` 帧内字段），
    /// `_updateDisplayData` 改按**当前帧**读（对齐 Slot.ts:412 逐贴图语义）；
    /// 运行时 replaceSkin 换图集同样在 `replace_texture_data` 同步。
    pub texture_atlas_scale: f64,
    /// 渲染适配器（TS 的 14 个 abstract 由引擎子类实现 → Rust trait 对象，D-3.4）。
    pub adapter: Box<dyn SlotRenderAdapter>,
}

/// 默认值 = `_onClear` 复位值（Slot.ts:261-330 + super TransformObject.ts:100-111）。
impl Default for Slot {
    fn default() -> Self {
        Self {
            base: TransformObject::default(), // super._onClear()（TransformObject.ts:100-111）
            idx: SlotIdx(0),                  // Rust 侧（init 预置）
            slot_name: String::new(),         // :319 `_slotData = null`
            // :328 `_parent = null as any`——契约 `parent: BoneIdx` 非 Option（数据必有
            // 骨），未初始化槽在 init 时重写；0 占位无消费者（update 前必 init）。
            parent: BoneIdx(0),
            display_controller: None,                   // :294
            display_data_dirty: false,                  // :296
            display_dirty: false,                       // :297
            geometry_dirty: false,                      // :298
            texture_dirty: false,                       // :299
            visible_dirty: false,                       // :300
            blend_mode_dirty: false,                    // :301
            z_order_dirty: false,                       // :302
            color_dirty: false,                         // :303
            vertices_dirty: false,                      // :304
            transform_dirty: false,                     // :305
            visible: true,                              // :306
            blend_mode: BlendMode::Normal,              // :307
            display_index: -1,                          // :308
            animation_display_index: -1,                // :309
            z_order: 0,                                 // :310
            z_index: 0,                                 // :311
            cached_frame_index: -1,                     // :312
            pivot_x: 0.0,                               // :313
            pivot_y: 0.0,                               // :314
            local_matrix: Matrix::default(),            // :315 `.identity()`
            color_transform: ColorTransform::default(), // :316 `.identity()`
            display_frames: Vec::new(),                 // :317 `.length = 0`
            geometry_bones: Vec::new(),                 // :318 `.length = 0`
            display_frame: None,                        // :320
            geometry_data: None,                        // :321
            bounding_box_data: None,                    // :322
            texture_data: None,                         // :323
            raw_display: DisplayHandle::Null,           // :324
            mesh_display: DisplayHandle::Null,          // :325
            display: None,                              // :326
            child_armature: None,                       // :327
            cached_frame_indices: None,                 // :329
            // Rust 侧（D-3.3；on_clear 保活 Arc、清空 armature_name）
            data: Arc::new(DragonBonesData::default()),
            armature_name: String::new(),
            texture_atlas_scale: 1.0,
            adapter: Box::new(NullSlotAdapter),
        }
    }
}
impl Slot {
    /// 对齐 `Slot.init(slotData, armatureValue, rawDisplay, meshDisplay)`（Slot.ts:639-674）。
    ///
    /// # 参数（任务 7 调用面）
    /// - `slot_data`：只读来源，值缓存进本槽（D-3.3，见结构文档）。
    /// - `armature: &Armature` = TS 的 `armatureValue`——同时提供
    ///   `idx`（→ `base.armature` / `idx`）、`data`+`armature_name`（→ D-3.3 缓存）、
    ///   `bones`/`slots`（→ parent 按名解析 Slot.ts:656 / `idx` 计数）、
    ///   defaultSkin（`replace_raw_display_data` 回填 Slot.ts:826-831）。
    /// - `texture_atlas_scale`：见字段文档（TS 经 `_textureData.parent.scale`
    ///   对象引用取图集缩放，Rust 图集不在 core 所有权内 → 工厂下传）。
    /// - `raw_display` / `mesh_display` / `adapter`：显示对象占位 + 渲染契约。
    ///
    /// **不 push**：TS:665 `armature._addSlot(this)` 由调用方（任务 7）在本方法
    /// 返回后 `armature.slots.push(slot)` 承担；`idx` 在此按 `armature.slots.len()`
    /// 预置（= push 后的下标）。若调用方以其他方式持有槽（暂不推入），需自行设 `idx`。
    pub fn init(
        &mut self,
        slot_data: &SlotData,
        armature: &Armature,
        texture_atlas_scale: f64,
        raw_display: DisplayHandle,
        mesh_display: DisplayHandle,
        adapter: Box<dyn SlotRenderAdapter>,
    ) {
        // :640 幂等：`if (this._slotData !== null) return;`
        // 判定用 `slot_name`（`_slotData` 的缓存，D-3.3）而非 `base.armature`：
        // `on_clear` 复位 `base`（:262 super._onClear）**且** :319 `_slotData = null`
        // → Rust 把 slot_name 清空（见 on_clear），故“已初始化”即 slot_name 非空——
        // 比 `base.armature.is_some()` 更贴近 TS 信号（字段文档 :319 / on_clear 契约）。
        if !self.slot_name.is_empty() {
            return;
        }

        // :644-653（slot_data 值缓存，D-3.3）
        self.slot_name.clone_from(&slot_data.name); // :644
        self.color_dirty = true; // :645 强制首帧刷颜色
        self.blend_mode_dirty = true; // :646 同上
        self.blend_mode = slot_data.blend_mode; // :647
        self.z_order = slot_data.z_order; // :648
        self.z_index = slot_data.z_index; // :649
        self.alpha = slot_data.alpha; // :650
        self.color_transform.copy_from(slot_data.color); // :651
        self.raw_display = raw_display; // :652
        self.mesh_display = mesh_display; // :653

        // Rust 侧（D-3.3）：data/名字/图集缩放缓存 + 预置 idx + 注入 adapter（见方法文档）
        self.data.clone_from(&armature.data);
        self.armature_name.clone_from(&armature.armature_name);
        self.texture_atlas_scale = texture_atlas_scale;
        self.idx = SlotIdx(armature.slots.len() as u32);
        self.adapter = adapter;

        // :655-663
        self.base.armature = Some(armature.idx); // :655
        let slot_parent = armature.get_bone(&slot_data.parent); // :656 按名字查父骨
        if let Some(slot_parent) = slot_parent {
            self.parent = slot_parent; // :659
        } else {
            // TS:662 `// Never;`（数据必有父骨）——Rust 防御性告警后保留占位值
            log::warn!(
                "slot {}: parent bone '{}' not found (Slot.ts:662 Never branch)",
                self.slot_name,
                slot_data.parent
            );
        }

        // TS:665 `armature._addSlot(this)` → 调用方 push（见方法文档）

        // :667-670
        self.adapter.init_display(self.raw_display, false); // :667 isRetain = false
        if self.raw_display != self.mesh_display {
            // :668 二者可能是同一个对象
            self.adapter.init_display(self.mesh_display, false); // :669
        }

        self.adapter.update_display(); // :672 `_onUpdateDisplay`
        self.adapter.add_display(); // :673 `_addDisplay`
    }

    /// 构造 + [`Slot::init`]（任务 7 装配面）：`let slot = Slot::new(…); armature.slots.push(slot);`
    ///
    /// TS 走“池借出（`_onClear`）→ `init`”，Rust 无池 → `Default` 后 `init`
    ///（`Armature::new` 同法，`model_mapping.md` §4 池语义偏差）。参数含义见
    /// [`Slot::init`]；`idx` 依 init 语义按 `armature.slots.len()` 预置，
    /// **调用方随后 push**。
    #[must_use]
    pub fn new(
        slot_data: &SlotData,
        armature: &Armature,
        texture_atlas_scale: f64,
        raw_display: DisplayHandle,
        mesh_display: DisplayHandle,
        adapter: Box<dyn SlotRenderAdapter>,
    ) -> Self {
        let mut slot = Self::default();
        slot.init(
            slot_data,
            armature,
            texture_atlas_scale,
            raw_display,
            mesh_display,
            adapter,
        );
        slot
    }

    /// 对齐 `Slot.update(cacheFrameIndex)`（Slot.ts:678-784）——脏标记状态机。
    ///
    /// 三个返回点（笔记 A5 结论 1）：`:698-700`（display 为 None）、`:735-737`
    /// （蒙皮/Surface 网格“Compatible”）、正常走完。
    ///
    /// - `cache_frame_index`：阶段 3 恒 -1（P1-4 缓存帧阶段 6 接入）。
    /// - `ctx`：自身骨架侧（推进期间自身不在 arena——见模块文档借用契约）。
    /// - `world`：子骨架接线用（Slot.ts:537-580）。
    pub fn update(
        &mut self,
        cache_frame_index: i32,
        ctx: &mut SlotUpdateCtx<'_>,
        world: &mut RuntimeCtx<'_>,
    ) {
        // ---------- ① displayData 脏（:679-682）----------
        if self.display_data_dirty {
            self.update_display_data(ctx.bones); // :680（连带置 transform/texture/geometry 脏）
            self.display_data_dirty = false; // :681
        }

        // ---------- ② display 脏（:684-687）----------
        if self.display_dirty {
            self.update_display(ctx, world); // :685（可能触发 _replaceDisplay / 子骨架切换）
            self.display_dirty = false; // :686
        }

        // ---------- ③ frame 脏（:689-696）----------
        if self.geometry_dirty || self.texture_dirty {
            // :690 只有“自带显示对象”（null/raw/mesh）才刷帧；外部自定义 display 不刷
            if self.display.is_none()
                || self.display == Some(self.raw_display)
                || self.display == Some(self.mesh_display)
            {
                self.adapter.update_frame(); // :691
            }
            self.geometry_dirty = false; // :694
            self.texture_dirty = false; // :695
        }

        // ---------- ④ display == null 提前返回（:698-700，坑 10）----------
        // 空显示时仍会刷帧数据，但**不会**刷可见性/混合/颜色/zOrder/mesh/变换。
        if self.display.is_none() {
            return;
        }

        // ---------- ⑤ visible（:702-705）----------
        if self.visible_dirty {
            self.adapter.update_visible(); // :703
            self.visible_dirty = false; // :704
        }

        // ---------- ⑥ blendMode（:707-710）----------
        if self.blend_mode_dirty {
            self.adapter.update_blend_mode(); // :708
            self.blend_mode_dirty = false; // :709
        }

        // ---------- ⑦ color（:712-715，含 _updateAlpha 的副作用）----------
        if self.color_dirty {
            self.adapter.update_color(); // :713
            self.color_dirty = false; // :714
        }

        // ---------- ⑧ zOrder（:717-720）----------
        if self.z_order_dirty {
            self.adapter.update_z_order(); // :718
            self.z_order_dirty = false; // :719
        }

        // ---------- ⑨ mesh 分支（:722-738）----------
        if self.geometry_data.is_some() && self.display == Some(self.mesh_display) {
            // :723 蒙皮网格
            let is_skinned = self
                .geometry_data
                .as_ref()
                .is_some_and(|geometry| geometry.weight.is_some());
            // :724 父骨是 Surface（网格面片骨）
            let is_surface = ctx.parent.bone_type != BoneType::Bone;

            if self.vertices_dirty // :727 外部显式改顶点
                || (is_skinned && self.is_bones_update(ctx.bones)) // :728 蒙皮：任一 geometryBone 脏
                || (is_surface && ctx.parent.children_transform_dirty)
            // :729
            {
                self.vertices_dirty = false; // :731 先清再写，允许 _updateMesh 内部重置
                self.adapter.update_mesh(); // :732
            }

            if is_skinned || is_surface {
                // :735 Compatible.
                return; // :736 蒙皮/Surface 网格到此为止，不再走缓存/变换矩阵分支
            }
        }

        // ---------- ⑩ 缓存帧索引判定（:740-766）----------
        // TS 改写的是入参副本（:763），Rust 用局部可变副本同构。
        // 分支守卫 `cacheFrameIndex >= 0 && indices !== null`（:740）同构为
        // `cache_frame_index >= 0 && let Some(indices)`（等价，避免先查后取）。
        let mut cache_frame_index = cache_frame_index;
        if cache_frame_index >= 0
            && let Some(indices) = self.cached_frame_indices.as_mut()
        {
            // —— P1-4（阶段 6）缓存帧表分支：阶段 3 `_cachedFrameIndices` 恒 None →
            // 整段不可达；结构照抄以便阶段 6 直接接 ——
            let cached_frame_index = indices[cache_frame_index as usize]; // :741 本槽在该缓存帧上的槽位号
            if cached_frame_index >= 0 && self.cached_frame_index == cached_frame_index {
                // :742 Same cache.
                self.transform_dirty = false; // :743
            } else if cached_frame_index >= 0 {
                // :745 Has been Cached.
                self.transform_dirty = true; // :746
                self.cached_frame_index = cached_frame_index; // :747
            } else if self.transform_dirty || ctx.parent.children_transform_dirty {
                // :749 Dirty.
                self.transform_dirty = true; // :750
                self.cached_frame_index = -1; // :751
            } else if self.cached_frame_index >= 0 {
                // :753 Same cache, but not set index yet.
                self.transform_dirty = false; // :754
                indices[cache_frame_index as usize] = self.cached_frame_index; // :755 反向登记
            } else {
                // :757 Dirty.
                self.transform_dirty = true; // :758
                self.cached_frame_index = -1; // :759
            }
        } else if self.transform_dirty || ctx.parent.children_transform_dirty {
            // :762 非缓存分支（阶段 3 走这里）
            cache_frame_index = -1; // :763 入参副本 → 后续 isCache = false
            self.transform_dirty = true; // :764
            self.cached_frame_index = -1; // :765
        }

        // ---------- ⑪ 变换（:768-783）----------
        if self.transform_dirty {
            if self.cached_frame_index < 0 {
                // :769 没有可用缓存
                let is_cache = cache_frame_index >= 0; // :770
                self.update_global_transform_matrix(is_cache, ctx.parent); // :771

                if is_cache && self.cached_frame_indices.is_some() {
                    // :773-775 `_armatureData.setCacheFrame(...)` 写缓存表——
                    // P1-4（阶段 6）；阶段 3 不可达（见 ⑩ 分支守卫）。
                    unreachable!("P1-4 阶段 6：ArmatureData::setCacheFrame");
                }
            } else {
                // :777-778 `_armatureData.getCacheFrame(...)` 直接读——P1-4（阶段 6）；
                // `cached_frame_index >= 0` 只在 ⑩ 缓存分支被写入（阶段 3 恒 None）→ 不可达。
                unreachable!("P1-4 阶段 6：ArmatureData::getCacheFrame");
            }

            self.adapter.update_transform(); // :781 abstract（引擎把矩阵应用到显示对象）
            self.transform_dirty = false; // :782
        }
    }

    /// 对齐 `_updateAlpha()`（Slot.ts:374-381）——`Armature::advance_time` 的
    /// alpha 传播在 bones/slots update **之前**每帧调一次（Armature.ts:328-339）。
    ///
    /// 链路：根骨 `alpha * armature.globalAlpha`（Bone.ts:339）→ 子骨 `* parent` →
    /// slot `_alpha * parentBone.globalAlpha`（Slot.ts:375）→ 子骨架（Armature.ts:330）。
    ///
    /// 副作用（:379，别漏）：`global_alpha` 变化时置 `color_dirty`，
    /// update() 的 :712 分支随后调 `_updateColor()` 合成给引擎。
    ///
    /// 注：TS 没有 `_parent === null` 保护（Slot.ts:375 直接解引用）——
    /// Rust 以 `parent: &Bone` 参数体现“init 后必非空”契约。
    pub fn update_alpha(&mut self, parent: &Bone) {
        let global_alpha = self.alpha * parent.global_alpha; // :375 父骨的 globalAlpha
        if self.global_alpha != global_alpha {
            // :377 只有变化才动
            self.global_alpha = global_alpha; // :378
            self.color_dirty = true; // :379
        }
    }

    /// 对齐 `Slot.invalidUpdate()`（Slot.ts:795-800）——只设脏，不立即算；
    /// `Armature::invalid_update` / 工厂收尾（F3.11）调用。
    pub fn invalid_update(&mut self) {
        self.display_data_dirty = true; // :796 下一帧重算 displayData/pivot/origin/localMatrix
        self.display_dirty = true; // :797 下一帧重算 display / childArmature
        //
        self.transform_dirty = true; // :799 下一帧重算矩阵并 _updateTransform
    }

    /// 对齐 `Slot.updateTransformAndMatrix()`（Slot.ts:804-809）——
    /// 命中检测前确保矩阵最新（containsPoint:946 / intersectsSegment:994 调用）。
    ///
    /// **不调** `_updateTransform()`（不推渲染端），`is_cache = false` →
    /// `global` 仍是懒更新。
    pub fn update_transform_and_matrix(&mut self, parent: &Bone) {
        if self.transform_dirty {
            // :805
            self.update_global_transform_matrix(false, parent); // :806
            self.transform_dirty = false; // :807
        }
    }

    /// 对齐 `_updateGlobalTransformMatrix(isCache)`（Slot.ts:583-594）。
    ///
    /// `globalTransformMatrix = localMatrix ∘ parentMatrix`（先槽本地、后父骨，
    /// Matrix.ts:189-219 的行向量约定）；Surface 父骨按当前 global 位置在曲面上
    /// 采样（一次迭代滞后，笔记 A6.1 结论 5）。Slot 没有 `animationPose`、
    /// 不读 `armature.flipX/flipY`（flip 只在 Bone 内消化，Bone.ts:119-120）。
    fn update_global_transform_matrix(&mut self, is_cache: bool, parent: &Bone) {
        let parent_matrix = if parent.bone_type == BoneType::Bone {
            // :584 普通父骨：直接矩阵
            parent.global_transform_matrix
        } else {
            // :584 Surface 父骨：双线性采样（任务 4）
            parent.get_global_transform_matrix(self.global.x, self.global.y)
        };
        // 注：`Slot` 经 Deref 访问 TransformObject 字段，同一表达式里“可变接收者 +
        // 不可变参数”会整体借用 `*self` → 拷贝到局部（矩阵/变换均 Copy）再调用。
        let local_matrix = self.local_matrix;
        self.global_transform_matrix.copy_from(&local_matrix); // :585
        self.global_transform_matrix.concat(&parent_matrix); // :586

        if is_cache {
            // :588-589 立刻回写 global
            let matrix = self.global_transform_matrix;
            self.global.from_matrix(&matrix);
        } else {
            // :592 延迟：真正读 global 前调 TransformObject::update_global_transform()
            self.global_dirty = true;
        }
    }
    /// 对齐 `_updateDisplayData()`（Slot.ts:383-501）——origin / pivot /
    /// localMatrix / geometryBones 的解析与各脏标记联动（update ① 调用）。
    fn update_display_data(&mut self, bones: &[Bone]) {
        // :384-386 记录旧值（take 即 TS 的“先记 prev 再置 null”）
        let prev_display_frame = self.display_frame;
        let prev_geometry_data = self.geometry_data.take();
        let prev_texture_data = self.texture_data.take();
        let mut raw_display_data: Option<DisplayData> = None; // :387
        let mut display_data: Option<DisplayData> = None; // :388

        self.display_frame = None; // :390
        self.bounding_box_data = None; // :392（geometry/texture 已 take 为 None，:391/:393）

        // :395
        if self.display_index >= 0 && (self.display_index as usize) < self.display_frames.len() {
            let index = self.display_index as usize;
            self.display_frame = Some(index); // :396
            let frame = &self.display_frames[index];
            raw_display_data.clone_from(&frame.raw_display_data); // :397
            display_data.clone_from(&frame.display_data); // :398
            self.geometry_data = frame.get_geometry_data().cloned(); // :400
            self.bounding_box_data = frame.get_bounding_box().cloned(); // :401
            self.texture_data = frame.get_texture_data().cloned(); // :402
        }

        // :405-408 有 diff 才继续
        if self.display_frame != prev_display_frame
            || self.geometry_data != prev_geometry_data
            || self.texture_data != prev_texture_data
        {
            // ---- pivot（:409-457）----
            // TS `this._geometryData === null && this._textureData !== null`（:410）
            if self.geometry_data.is_none()
                && let Some(texture_data) = self.texture_data.as_ref()
            {
                // :410 纯贴图才有 pivot
                // :412 图集缩放 × 骨架缩放（图集缩放为 Rust 侧缓存，见字段文档）
                let scale = self.texture_atlas_scale * self.armature_data().scale;
                // :411 displayData 是 Image 用 displayData，否则用 rawDisplayData（TS 直接 as）
                let image_display_data: &DisplayData = if let Some(display_data) = &display_data
                    && display_data.type_() == DisplayType::Image
                {
                    display_data
                } else {
                    raw_display_data.as_ref().expect(
                        "Slot.ts:411 同点解引用 rawDisplayData（textureData 非空 ⟹ factory 已写 raw）",
                    )
                };
                // :415-416 数据里的归一化 pivot（TS as 成功后 .pivot 必在——进入本分支
                // 的 geometry 为空 ⟹ 非 Mesh/Path，texture 非空 ⟹ 非 Armature/BoundingBox）
                let pivot = match &image_display_data.kind {
                    DisplayKind::Image { pivot, .. } => *pivot,
                    _ => unreachable!("imageDisplayData 非 Image 类型（Slot.ts:411 as）"),
                };
                self.pivot_x = pivot.x; // :415
                self.pivot_y = pivot.y; // :416

                // :418-425
                let frame = texture_data.frame; // :413 可为 None（子纹理）
                let rect = frame.unwrap_or(texture_data.region); // :418
                let mut width = rect.width; // :419
                let mut height = rect.height; // :420
                if texture_data.rotated && frame.is_none() {
                    // :422 图集旋转 + 无 frame → 宽高互换
                    width = rect.height; // :423
                    height = rect.width; // :424
                }

                self.pivot_x *= width * scale; // :427
                self.pivot_y *= height * scale; // :428

                if let Some(frame) = frame {
                    // :430 子纹理偏移
                    self.pivot_x += frame.x * scale; // :431
                    self.pivot_y += frame.y * scale; // :432
                }

                // :435-447 换皮 pivot 修正（TS 用对象身份比较 `imageDisplayData !==
                // rawDisplayData`；Rust 两个本地引用即对象身份 → `ptr::eq` 恒等翻译）
                if let Some(raw) = raw_display_data.as_ref()
                    && !std::ptr::eq(image_display_data, raw)
                {
                    // TS 静态 _helpMatrix/_helpPoint → 局部临时（任务 1 偏差）
                    let mut help_matrix = Matrix::default();
                    raw.transform.to_matrix(&mut help_matrix); // :437
                    help_matrix.invert(); // :438
                    let (x, y) = help_matrix.transform_point(0.0, 0.0, false); // :439
                    self.pivot_x -= x; // :440
                    self.pivot_y -= y; // :441

                    image_display_data.transform.to_matrix(&mut help_matrix); // :443
                    help_matrix.invert(); // :444
                    let (x, y) = help_matrix.transform_point(0.0, 0.0, false); // :445
                    self.pivot_x += x; // :446
                    self.pivot_y += y; // :447
                }

                // :450-452 非 yDown 翻转 pivot.y（`DragonBones::Y_DOWN` 恒 true →
                // 死分支，结构保留与 TS 同形）
                if !DragonBones::Y_DOWN {
                    let flip_height = if texture_data.rotated {
                        texture_data.region.width
                    } else {
                        texture_data.region.height
                    };
                    self.pivot_y = flip_height * scale - self.pivot_y; // :451
                }
            } else {
                // :454 没有贴图（mesh/空/子骨架）
                self.pivot_x = 0.0; // :455
                self.pivot_y = 0.0; // :456
            }

            // ---- original transform（:459-468）----
            if let Some(raw) = &raw_display_data {
                // :460 // Compatible.（优先 raw！）
                self.origin = Some(raw.transform); // :461（F3.3 值拷贝，data 只读）
            } else if let Some(display) = &display_data {
                // :463 // Compatible.
                self.origin = Some(display.transform); // :464
            } else {
                self.origin = None; // :467
            }

            // TODO remove slot offset.（TS:470 原注释保留）
            // （Deref 借用限制 → 拷贝局部再调用，见 `_updateGlobalTransformMatrix` 注）
            if let Some(origin) = self.origin {
                // :471 global 临时被覆盖成 origin+offset
                self.global.copy_from(&origin); // :472
                let offset = self.offset;
                self.global += offset; // :472（Transform.add）
                let global = self.global;
                global.to_matrix(&mut self.local_matrix); // :472
            } else {
                let offset = self.offset;
                self.global.copy_from(&offset); // :475
                let global = self.global;
                global.to_matrix(&mut self.local_matrix); // :475
            }

            // ---- geometry diff（:478-496）----
            if self.geometry_data != prev_geometry_data {
                // :479
                self.geometry_dirty = true; // :480
                self.vertices_dirty = true; // :481
                if self.geometry_data.is_some() {
                    // :483
                    self.geometry_bones.clear(); // :484
                    let weight = self
                        .geometry_data
                        .as_ref()
                        .and_then(|geometry| geometry.weight.clone()); // :485
                    if let Some(weight) = weight {
                        for bone_name in &weight.bones {
                            // :486 按名查骨；解析失败 = TS 的 null bone（:487-488）
                            let bone = bones
                                .iter()
                                .position(|b| b.bone_name == *bone_name)
                                .map(|i| BoneIdx(i as u32)); // :487
                            self.geometry_bones.push(bone); // :488
                        }
                    }
                } else {
                    self.geometry_bones.clear(); // :493
                    self.geometry_data = None; // :494
                }
            }

            self.texture_dirty = self.texture_data != prev_texture_data; // :498
            self.transform_dirty = true; // :499
        }
    }

    /// 对齐 `_updateDisplay()`（Slot.ts:503-581）——display / childArmature 切换
    /// 与子骨架接线（update ② 调用）。
    fn update_display(&mut self, ctx: &mut SlotUpdateCtx<'_>, world: &mut RuntimeCtx<'_>) {
        // :504 旧“有效显示”：display 为 null 时取 rawDisplay
        let prev_display = self.display.unwrap_or(self.raw_display);
        let prev_child_armature = self.child_armature; // :505

        // ---- Update display and child armature（:507-521）----
        let frame_display = self
            .display_frame
            .and_then(|index| self.display_frames.get(index))
            .and_then(|frame| frame.display);
        if let Some(frame_display) = frame_display {
            self.display = Some(frame_display); // :509
            if let DisplayHandle::Armature(child_idx) = frame_display {
                // :510 `_display instanceof Armature`
                self.child_armature = Some(child_idx); // :511
                match world.armature(child_idx) {
                    Some(child) => {
                        // :512 `_display = childArmature.display`（null → None）
                        self.display =
                            (child.display != DisplayHandle::Null).then_some(child.display);
                    }
                    None => {
                        // Rust 防御：子骨架未 build 或已 dispose（TS 对象引用不可能为空）
                        log::warn!(
                            "slot {}: child armature {} not in arena (Slot.ts:512)",
                            self.slot_name,
                            child_idx.0
                        );
                        self.display = None;
                    }
                }
            } else {
                self.child_armature = None; // :515
            }
        } else {
            self.display = None; // :519
            self.child_armature = None; // :520
        }

        // ---- Update display（:523-535）----
        let current_display = self.display.unwrap_or(self.raw_display); // :524
        if current_display != prev_display {
            // :525
            self.texture_dirty = true; // :526
            self.visible_dirty = true; // :527
            self.blend_mode_dirty = true; // :528
            // TS:529 `// this._zOrderDirty = true;`（原版注释保留）
            self.color_dirty = true; // :530
            self.transform_dirty = true; // :531

            self.adapter.update_display(); // :533 `_onUpdateDisplay`
            self.adapter.replace_display(prev_display); // :534 `_replaceDisplay`
        }

        // ---- Update child armature（:537-580）----
        if self.child_armature != prev_child_armature {
            if let Some(prev_idx) = prev_child_armature
                && let Some(inherit_animation) = world.armature_mut(prev_idx).map(|prev| {
                    prev.parent = None; // :540 解绑父插槽回指
                    prev.inherit_animation
                })
            {
                if let Err(error) = world.move_armature_clock(prev_idx, None) {
                    log::warn!(
                        "slot {}: failed to detach previous child armature clock: {error}",
                        self.slot_name
                    );
                    // 官方顺序是 parent 先、clock 后（Slot.ts:539-541），时钟迁移
                    // 失败时回滚回指，保持“parent 回指与 clock 成员表一致”。
                    if let Some(prev) = world.armature_mut(prev_idx) {
                        prev.parent = Some((ctx.armature, self.idx));
                    }
                }
                if inherit_animation {
                    // :542
                    if let Some(prev) = world.armature_mut(prev_idx) {
                        prev.animation.reset(); // :543
                    }
                }
            }

            if let Some(child_idx) = self.child_armature {
                // :559-564 先取出当前 displayFrame 的 Armature 型 actions（&self 局部）
                let actions = self.current_armature_display_actions();
                let has_display_frame = self.display_frame.is_some(); // :559

                let previous_parent = world.armature(child_idx).and_then(|child| child.parent);
                let child_exists = if let Some(child) = world.armature_mut(child_idx) {
                    child.parent = Some((ctx.armature, self.idx)); // :548（D-3.2 索引回指）
                    true
                } else {
                    false
                };

                if child_exists {
                    if let Err(error) = world.move_armature_clock(child_idx, ctx.clock) {
                        log::warn!(
                            "slot {}: failed to attach child armature clock: {error}",
                            self.slot_name
                        );
                        // 方案 a（保持 TS 可观察时序）：官方 Slot.ts:548 先写 parent、
                        // 再写 clock（:549）。时钟迁移失败时回滚 parent 回指，避免
                        // 回指与成员表不一致；成功路径的中间态可见性与 TS 相同。
                        if let Some(child) = world.armature_mut(child_idx) {
                            child.parent = previous_parent;
                        }
                    }

                    if let Some(child) = world.armature_mut(child_idx)
                        && child.inherit_animation
                    {
                        // :550
                        if child.cache_frame_rate() == 0.0 {
                            // :551 Set child armature cache frameRate.
                            let cache_frame_rate = ctx.cache_frame_rate; // :552
                            if cache_frame_rate != 0.0 {
                                // TODO 阶段 6（P1-4）: child.cacheFrameRate = cacheFrameRate
                                // （Slot.ts:554 → `ArmatureData.cacheFrames` setter 写数据，
                                // 阶段 3 无对应字段，按任务约定跳过）
                            }
                        }

                        if has_display_frame {
                            // :559 Child armature action.
                            if !actions.is_empty() {
                                // :566
                                for action in &actions {
                                    // :567
                                    let mut event_object = EventObject::default();
                                    EventObject::action_data_to_instance(
                                        action,
                                        &mut event_object,
                                        ctx.armature,
                                        ctx.bones,
                                        &[], // 槽循环内 slots 解析结果随后被 :570 覆写
                                    ); // :569
                                    event_object.slot = Some(self.idx); // :570 `eventObject.slot = this`
                                    event_object.slot_name = Some(self.slot_name.clone()); // D-4.20：索引给核心逻辑、名字给事件消费者
                                    ctx.actions.insert(0, event_object); // :571 `_bufferAction(_, false)` 头插
                                }
                            } else {
                                // :575 TS `play()`（Animation.ts:506 默认 playTimes = -1）
                                child.animation.play(None, -1);
                            }
                        }
                    }
                } else {
                    log::warn!(
                        "slot {}: child armature {} not in arena (Slot.ts:548)",
                        self.slot_name,
                        child_idx.0
                    );
                }
            }
        }
    }

    /// 对齐 `Slot._isBonesUpdate()`（Slot.ts:362-370）——蒙皮网格感知
    /// geometryBones 的 `_childrenTransformDirty`（update ⑨ :728 调用）。
    #[must_use]
    pub fn is_bones_update(&self, bones: &[Bone]) -> bool {
        for bone in &self.geometry_bones {
            // TS:364 `bone !== null && bone._childrenTransformDirty`
            if let Some(bone_idx) = bone
                && bones[bone_idx.index()].children_transform_dirty
            {
                return true;
            }
        }
        false
    }

    /// 对齐 `Slot._hasDisplay(display)`（Slot.ts:350-358）——
    /// 遍历所有 frame 查重（`replace_display` 用）。
    fn has_display(&self, display: Option<DisplayHandle>) -> bool {
        self.display_frames
            .iter()
            .any(|display_frame| display_frame.display == display)
    }

    /// 对齐 `_updateDisplay` 内的 actions 提取（Slot.ts:559-564）：
    /// `displayData` 优先、否则 `rawDisplayData`，类型为 Armature 才取其 `actions`。
    fn current_armature_display_actions(&self) -> Vec<ActionData> {
        let Some(display_frame) = self
            .display_frame
            .and_then(|index| self.display_frames.get(index))
        else {
            return Vec::new(); // :559 `if (this._displayFrame !== null)`
        };

        let display_data = display_frame
            .display_data
            .as_ref()
            .or(display_frame.raw_display_data.as_ref()); // :561
        match display_data.map(|data| &data.kind) {
            // :562-563 `displayData.type === DisplayType.Armature`
            Some(DisplayKind::Armature { actions, .. }) => actions.clone(),
            _ => Vec::new(),
        }
    }

    /// 对齐 `_setDisplayIndex(value, isAnimation = false)`（Slot.ts:598-614）。
    ///
    /// **无 `force` 参数**（笔记 A7.6 已核实：`grep force Slot.ts` = 0 命中）。
    /// `is_animation = true` = 调用者是动画时间轴（额外记 `_animationDisplayIndex`）。
    /// 只置脏不刷新；公开 setter `displayIndex`（TS:1117-1120）= 本方法 + `update(-1)`，
    /// 后者需要 ctx/world → 由调用方（任务 6/7）自行驱动。
    pub fn set_display_index(&mut self, value: i32, is_animation: bool) {
        if is_animation {
            // :599
            if self.animation_display_index == value {
                // :600 与动画上次给的值相同 → 直接返回
                return;
            }
            self.animation_display_index = value; // :604
        }

        if self.display_index == value {
            // :607 已是目标值 → 返回（不置任何脏）
            return;
        }

        // :611 超上界 → 夹到最后一个；负值原样通过（如 -1）
        self.display_index = if value < self.display_frames.len() as i32 {
            value
        } else {
            self.display_frames.len() as i32 - 1
        };
        self.display_data_dirty = true; // :612
        self.display_dirty = self.display_index < 0 // :613 -1 → 必脏
            || self.display != self.display_frames[self.display_index as usize].display;
    }

    /// 对齐 `_setZOrder(value)`（Slot.ts:618-627）。
    ///
    /// TS:619-621 的 `if (this._zOrder === value) { // return false; }` 中
    /// `return` 被原版注释掉 → 无论值是否变化都置脏（照抄行为）。
    pub fn set_z_order(&mut self, value: i32) -> bool {
        self.z_order = value; // :623
        self.z_order_dirty = true; // :624
        self.z_order_dirty // :626
    }

    /// 对齐 `_setColor(value)`（Slot.ts:631-635）。
    pub fn set_color(&mut self, value: ColorTransform) -> bool {
        self.color_transform.copy_from(value); // :632
        self.color_dirty = true; // :634
        self.color_dirty // :634 返回赋值结果
    }

    /// 对齐 `visible` setter（Slot.ts:1060-1067）。
    ///
    /// TS 变化时**直调** `_updateVisible()`（不置 `_visibleDirty`），照抄。
    pub fn set_visible(&mut self, value: bool) {
        if self.visible == value {
            // :1061
            return;
        }
        self.visible = value; // :1065
        self.adapter.update_visible(); // :1066
    }

    /// 对齐 `_updateVisible()`（Slot.ts:342）的公开转发——
    /// `Bone::visible` setter 直接调 `slot._updateVisible()`（Bone.ts:547）用。
    pub fn update_visible(&mut self) {
        self.adapter.update_visible();
    }

    /// 对齐 `replaceRawDisplayData(displayData, index = -1)`（Slot.ts:813-839）。
    ///
    /// 注意 `:825-833` 的怪癖：传 `None`（TS null）时从 **defaultSkin** 回填
    /// `rawDisplayData`（`defaultSkin.getDisplays(slotData.name)[index]`）。
    pub fn replace_raw_display_data(&mut self, display_data: Option<DisplayData>, index: i32) {
        // :814-819 负索引 → 当前 displayIndex（再负则 0）；非负越界 → 静默 return
        let mut index = index;
        if index < 0 {
            index = if self.display_index < 0 {
                0
            } else {
                self.display_index
            };
        } else if (index as usize) >= self.display_frames.len() {
            return;
        }

        // :821-822 只有变化才动
        let changed = self
            .display_frames
            .get(index as usize)
            .expect("displayFrames[index]（TS 越界同样崩溃，Slot.ts:821）")
            .raw_display_data
            != display_data;
        if !changed {
            return;
        }

        // :825-833 传 null → 回退 defaultSkin（先解析再做可变借用）
        let mut display_data = display_data;
        if display_data.is_none() {
            let armature_data = self.armature_data();
            let default_skin = armature_data
                .default_skin
                .as_deref()
                .and_then(|skin_name| armature_data.get_skin(skin_name)); // :826-827
            if let Some(default_raw_display_datas) = default_skin
                .and_then(|skin| skin.get_displays(&self.slot_name)) // :828
                .and_then(|displays| displays.get(index as usize))
            // :829 `index < length`
            {
                display_data = default_raw_display_datas.clone(); // :830
            }
        }

        // :823-824
        let display_frame = self
            .display_frames
            .get_mut(index as usize)
            .expect("displayFrames[index]（TS 越界同样崩溃，Slot.ts:821）");
        display_frame.deform_vertices.clear(); // :823 形变缓冲失效
        display_frame.raw_display_data = display_data; // :824

        if index == self.display_index {
            // :835 只影响当前显示索引时才置脏
            self.display_data_dirty = true; // :836
        }
    }

    /// 对齐 `replaceDisplayData(displayData, index = -1)`（Slot.ts:843-859）。
    pub fn replace_display_data(&mut self, display_data: Option<DisplayData>, index: i32) {
        // :844-849
        let mut index = index;
        if index < 0 {
            index = if self.display_index < 0 {
                0
            } else {
                self.display_index
            };
        } else if (index as usize) >= self.display_frames.len() {
            return;
        }

        // :851-852 双重判重
        let display_frame = self
            .display_frames
            .get_mut(index as usize)
            .expect("displayFrames[index]（TS 越界同样崩溃，Slot.ts:851）");
        if display_frame.display_data != display_data
            && display_frame.raw_display_data != display_data
        {
            display_frame.display_data = display_data; // :853
            if index == self.display_index {
                // :855
                self.display_data_dirty = true; // :856（只置 displayData 脏）
            }
        }
    }

    /// 对齐 `replaceTextureData(textureData, index = -1)`（Slot.ts:863-879）。
    pub fn replace_texture_data(&mut self, texture_data: Option<TextureData>, index: i32) {
        // :864-869
        let mut index = index;
        if index < 0 {
            index = if self.display_index < 0 {
                0
            } else {
                self.display_index
            };
        } else if (index as usize) >= self.display_frames.len() {
            return;
        }

        // :871-872
        let display_frame = self
            .display_frames
            .get_mut(index as usize)
            .expect("displayFrames[index]（TS 越界同样崩溃，Slot.ts:871）");
        if display_frame.texture_data != texture_data {
            display_frame.texture_data = texture_data; // :873
            if index == self.display_index {
                // :875
                self.display_data_dirty = true; // :876 只置 displayData 脏（不置 displayDirty）
            }
        }
    }

    /// 对齐 `replaceDisplay(value, index = -1)`（Slot.ts:883-922）。
    ///
    /// `value: Option<DisplayHandle>`：`None` = TS null；`Armature(idx)` = 子骨架。
    pub fn replace_display(&mut self, value: Option<DisplayHandle>, index: i32) {
        // :884-889
        let mut index = index;
        if index < 0 {
            index = if self.display_index < 0 {
                0
            } else {
                self.display_index
            };
        } else if (index as usize) >= self.display_frames.len() {
            return;
        }

        // :891-894（先结束 display_frames 的可变借用，再做全 self 访问）
        let (changed, prev_display) = {
            let display_frame = self
                .display_frames
                .get_mut(index as usize)
                .expect("displayFrames[index]（TS 越界同样崩溃，Slot.ts:891）");
            let changed = display_frame.display != value; // :892
            let prev_display = display_frame.display; // :893
            if changed {
                display_frame.display = value; // :894
            }
            (changed, prev_display)
        };
        if !changed {
            return;
        }

        // :896-907 旧 display 不再被任何 frame 引用 → 释放
        if let Some(prev) = prev_display
            && prev != self.raw_display
            && prev != self.mesh_display
            && !self.has_display(prev_display)
        // :899
        {
            if let DisplayHandle::Armature(_) = prev {
                // :901-903 `(eachDisplay as Armature).dispose()` 在原版被注释掉——
                // 子骨架不在此释放（on_clear 侧同样有 arena 语义 TODO）
            } else {
                self.adapter.dispose_display(prev, true); // :905 isRelease = true
            }
        }

        // :909-916 新 display 需要初始化
        if let Some(value) = value
            && value != self.raw_display
            && value != self.mesh_display
            && !self.has_display(prev_display) // :912 原文就是 prevDisplay（疑似原版笔误，照抄）
            && !matches!(value, DisplayHandle::Armature(_))
        // :913 子骨架不 _initDisplay
        {
            self.adapter.init_display(value, true); // :915 isRetain = true
        }

        if index == self.display_index {
            // :918
            self.display_dirty = true; // :919 置 displayDirty，不是 displayDataDirty
        }
    }

    /// 对齐 `displayFrameCount` getter（Slot.ts:1071-1073）。
    #[must_use]
    pub fn display_frame_count(&self) -> usize {
        self.display_frames.len()
    }

    /// 对齐 `displayFrameCount` setter（Slot.ts:1074-1091）。
    pub fn set_display_frame_count(&mut self, value: usize) {
        let prev_count = self.display_frames.len(); // :1075
        if prev_count < value {
            // :1076 扩容（TS 从池借 DisplayFrame → Rust default，_onClear 等价）
            self.display_frames
                .resize_with(value, DisplayFrame::default); // :1077-1081
        } else if prev_count > value {
            // 官方 TS:1083-1089 收缩循环 `for (i = prevCount - 1; i < value; --i)` 的
            // 条件**恒假**（i 起步 ≥ value、步进为 --i → 永不进入循环体，上游 bug）：
            // 实际可观测行为 = 不调 `replaceDisplay(null, i)`、不 `returnToPool`、
            // 不置任何脏，只剩 :1089 `this._displayFrames.length = value` 截断。
            // 此处照抄其**可观测行为**：仅 resize，不释放不置脏（勿"修正"成意图循环）。
            self.display_frames.truncate(value); // :1089
        }
    }

    /// 对齐 `containsPoint(x, y)`（Slot.ts:941-953）。
    #[must_use]
    pub fn contains_point(&self, _x: f64, _y: f64) -> bool {
        false // TODO P1-3（阶段 6）：BoundingBox（内部 updateTransformAndMatrix + 逆矩阵变换点）
    }

    /// 对齐 `intersectsSegment(…)`（Slot.ts:984-1038）。
    ///
    /// 返回值：TS 为 `intersectionCount: number`（0 = 不相交）；阶段 3 恒无命中
    /// → `None`（阶段 6 接 BoundingBox 后返回 `Some(count)`）。
    #[allow(clippy::too_many_arguments)] // TS 形参即 7 个（4 坐标 + 3 出参），照抄签名
    #[must_use]
    pub fn intersects_segment(
        &self,
        _x_a: f64,
        _y_a: f64,
        _x_b: f64,
        _y_b: f64,
        _intersection_point_a: Option<&mut Point>,
        _intersection_point_b: Option<&mut Point>,
        _normal_radians: Option<&mut Point>,
    ) -> Option<i32> {
        None // TODO P1-3（阶段 6）：BoundingBox（逆变换端点 → BoundingBoxData.intersectsSegment → 正变换回世界）
    }

    /// 对齐 `Slot.armatureData`（经 TS `_armature._armatureData`，Slot.ts 内 3 处：
    /// pivot 缩放 :412、defaultSkin 回填 :826）——Rust 侧走 D-3.3 缓存的
    /// `data + armature_name`（不依赖 arena，装配期即可用）。
    ///
    /// # Panics
    /// `init` 后 `armature_name` 必存在于 `data.armatures`；不存在说明装配时数据
    /// 被换（对应 TS `_armature === null` 的空引用分支，Rust 以 expect 暴露）。
    #[must_use]
    pub fn armature_data(&self) -> &ArmatureData {
        self.data
            .armatures
            .get(&self.armature_name)
            .expect("armature data not found (dangling armature_name)")
    }

    /// 对齐 `Slot._onClear()`（Slot.ts:261-330）。
    ///
    /// 副作用顺序照抄：先收集并销毁“非 raw/mesh 的显示对象”（子骨架 / 自定义
    /// display），再 `_disposeDisplay(mesh, false)` / `_disposeDisplay(raw, false)`
    /// （raw/mesh 不回池、由工厂拥有），最后字段复位（:294-329 逐行）。
    ///
    /// 偏差：
    /// - `_on_clear` 拿不到 [`RuntimeCtx`]（阶段 3 借用契约）→ 子骨架的 `dispose()`
    ///   （TS:279）**只记 TODO**，由 arena dispose 路径（任务 6/7：dispose 骨架时
    ///   经 `Slot::child_armature` 递归释放）承担；
    /// - DisplayFrame “回池”（TS:274 returnToPool → `_onClear`）在 Rust 为直接
    ///   drop/复位（D-3.1 池语义偏差：Bone/Slot/DisplayFrame 随 Armature 生灭）。
    pub fn on_clear(&mut self) {
        // :262 super._onClear() → TransformObject 字段复位（TransformObject.ts:100-111）
        self.base = TransformObject::default();

        // :264-275 收集待销毁显示（非 raw/mesh 且去重）
        // TS 的 `display !== _rawDisplay` 中 display 可为 null——与非空 raw/mesh 比较
        // 为 true、null 会照样入列并在 :282 调 `_disposeDisplay(null, true)`；
        // Rust 把 `None` 归一为 `DisplayHandle::Null` 参与比较（None ≡ null，
        // 于是 “display=null 且 rawDisplay=null” 与 TS 一样不入列）。
        let raw_display = self.raw_display;
        let mesh_display = self.mesh_display;
        let mut dispose_display_list: Vec<DisplayHandle> = Vec::new();
        for display_frame in &self.display_frames {
            let display = display_frame.display.unwrap_or(DisplayHandle::Null);
            if display != raw_display
                && display != mesh_display
                && !dispose_display_list.contains(&display)
            {
                dispose_display_list.push(display);
            }
        }

        // :277-284
        for each_display in dispose_display_list {
            if let DisplayHandle::Armature(child_idx) = each_display {
                // TS:279 `eachDisplay.dispose()`——`_on_clear` 无 world 句柄（阶段 3
                // 借用契约：自身骨架不在 arena），子骨架释放交给 arena dispose 路径。
                // TODO 阶段 7 / arena 语义：dispose 骨架时经 `child_armature` 递归释放
                log::warn!(
                    "slot {}: child armature {} not disposed in on_clear (needs RuntimeCtx, Slot.ts:279)",
                    self.slot_name,
                    child_idx.0
                );
            } else {
                self.adapter.dispose_display(each_display, true); // :282 isRelease = true
            }
        }

        // :286-292（raw/mesh 不回池，isRelease = false）
        if self.mesh_display != DisplayHandle::Null && self.mesh_display != self.raw_display {
            // :286 May be _meshDisplay and _rawDisplay is the same one.
            self.adapter.dispose_display(self.mesh_display, false); // :287
        }
        if self.raw_display != DisplayHandle::Null {
            self.adapter.dispose_display(self.raw_display, false); // :291
        }

        // :294-329 字段复位（逐行）
        self.display_controller = None; // :294
        self.display_data_dirty = false; // :296
        self.display_dirty = false; // :297
        self.geometry_dirty = false; // :298
        self.texture_dirty = false; // :299
        self.visible_dirty = false; // :300
        self.blend_mode_dirty = false; // :301
        self.z_order_dirty = false; // :302
        self.color_dirty = false; // :303
        self.vertices_dirty = false; // :304
        self.transform_dirty = false; // :305
        self.visible = true; // :306
        self.blend_mode = BlendMode::Normal; // :307
        self.display_index = -1; // :308
        self.animation_display_index = -1; // :309
        self.z_order = 0; // :310
        self.z_index = 0; // :311
        self.cached_frame_index = -1; // :312
        self.pivot_x = 0.0; // :313
        self.pivot_y = 0.0; // :314
        self.local_matrix.identity(); // :315
        self.color_transform.clear(); // :316 `.identity()`（Rust 侧对应 clear()）
        self.display_frames.clear(); // :317（TS returnToPool → _onClear；Rust 直接 drop，D-3.1）
        self.geometry_bones.clear(); // :318
        self.slot_name = String::new(); // :319 `_slotData = null`（缓存字段方案 D-3.3）
        self.display_frame = None; // :320
        self.geometry_data = None; // :321
        self.bounding_box_data = None; // :322
        self.texture_data = None; // :323
        self.raw_display = DisplayHandle::Null; // :324
        self.mesh_display = DisplayHandle::Null; // :325
        self.display = None; // :326
        self.child_armature = None; // :327
        // :328 `_parent = null as any`——契约 `parent: BoneIdx` 非 Option：清空后的槽
        // 必被复用方 init 重写，保持占位值（`slot_name` 已清空可判定“未初始化”）。
        self.cached_frame_indices = None; // :329

        // Rust 侧复位（无对应 TS 行）：`_slotData/_armature = null` 的缓存字段语义——
        // `armature_name` 清空后 `armature_data()` 不可达（等价 TS 空引用解引用）；
        // `data: Arc` 保活（Rust 无 null，见 model_mapping §4 数据销毁语义偏差）。
        self.armature_name.clear();
        self.texture_atlas_scale = 1.0;
    }
}

impl std::ops::Deref for Slot {
    type Target = TransformObject;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl std::ops::DerefMut for Slot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

#[cfg(test)]
mod tests {
    use super::{DisplayFrame, Slot, SlotUpdateCtx};
    use crate::animation::world_clock::{ClockArena, ClockId, WorldClock};
    use crate::armature::{
        Armature, ArmatureIdx, Bone, DisplayHandle, NullArmatureProxy, RuntimeCtx,
    };
    use crate::model::DragonBonesData;
    use std::sync::Arc;

    fn armature(idx: ArmatureIdx) -> Armature {
        Armature::new(
            idx,
            Arc::new(DragonBonesData::default()),
            "slot-clock-test",
            Box::new(NullArmatureProxy::new()),
            DisplayHandle::Null,
        )
    }

    #[test]
    fn failed_child_clock_move_rolls_back_child_parent_reference() {
        let parent_idx = ArmatureIdx(0);
        let child_idx = ArmatureIdx(1);
        let mut armatures = vec![Some(armature(parent_idx)), Some(armature(child_idx))];
        armatures[child_idx.index()]
            .as_mut()
            .expect("child armature")
            .inherit_animation = false;

        let mut clocks = ClockArena::new();
        let _valid = clocks.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let mut pending_events = Vec::new();

        let mut slot = Slot {
            slot_name: "child-slot".to_string(),
            ..Slot::default()
        };
        slot.display_frames.push(DisplayFrame {
            display: Some(DisplayHandle::Armature(child_idx)),
            ..DisplayFrame::default()
        });
        slot.display_frame = Some(0);

        let bone = Bone::default();
        let mut actions = Vec::new();
        let mut ctx = SlotUpdateCtx {
            parent: &bone,
            bones: &[],
            actions: &mut actions,
            armature: parent_idx,
            clock: Some(ClockId(99)), // 无效 ClockId，强制时钟迁移失败
            cache_frame_rate: 0.0,
        };

        {
            let mut world =
                RuntimeCtx::new(&mut armatures, &mut clocks, &mut pending_events, None, None);
            slot.update_display(&mut ctx, &mut world);
        }

        let child = armatures[child_idx.index()]
            .as_ref()
            .expect("child remains in arena");
        assert_eq!(slot.child_armature, Some(child_idx));
        assert_eq!(child.parent, None, "失败路径必须回滚 parent 回指");
        assert_eq!(child.clock(), None, "失败的时钟迁移不得写 back-reference");
    }
}
