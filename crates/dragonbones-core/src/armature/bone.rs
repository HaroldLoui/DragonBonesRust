//! `armature/Bone.ts`（578 行）——任务 3 实现（本阶段最大单体）。
//!
//! 落地内容：
//! 1. 字段全集 + `Default`（逐行对齐 `_onClear`，Bone.ts:92-107 + super）；
//! 2. [`Bone::init`]（TS:345-361）——幂等 guard、按名解析父骨、D-3.3 缓存；
//! 3. [`Bone::update`]（TS:365-441）——四分支脏标记机（含 Surface 覆盖的分派）；
//! 4. [`Bone::update_global_transform_matrix`]（TS:109-330）——offsetMode 三分支 +
//!    inherit 大分支的两条数学路径，全阶段最关键 220 行；
//! 5. [`BoneRuntime`]——普通骨/Surface 的运行时载荷枚举（指南任务 4 形态），
//!    Surface 载荷与算法体在 `surface.rs`。
//!
//! ## 签名偏差（相对任务书固定签名，TS 原文强制）
//!
//! `update` 的父骨参数从 `parent: Option<&Bone>` 改为三段骨表
//! `left: &mut [Bone]` / `right: &mut [Bone]`（调用方 `split_at_mut(i)` 再
//! `split_first_mut()`，左段 = 索引小于自身的骨、右段 = 索引大于自身的骨）；
//! `update_global_transform_matrix` 的对应参数仍叫 `ancestors`（= 该骨的
//! `[0, idx)` 祖先区间，即调用方视角的 `left`）：
//!
//! - **TS:201/257/184 的 `parent/surfaceBone.updateGlobalTransform()` 必须回写父骨**
//!   （TransformObject.ts:135-140：`global ← 矩阵分解` + `_globalDirty = false`）。
//!   `_globalDirty = true` 的唯一写点是 Bone.ts:235（非 cache 路径）——实测
//!   Rooster 素材 `foot_lt`（inheritRotation=false → TS:201）的父骨 `shin_lt`
//!   （有父且 inheritScale=true → TS:235 置脏）在 JS 里于当帧被分解成世界值，
//!   `&Bone` 无法回写则静姿态 golden 的 `shin_lt.global` 必错（F3.5 同级证据）。
//! - **TS:175 的 surfaceBone（Surface 宿主骨）是另一个骨对象**，`parent` 字段里
//!   只有索引，需要骨切片才能取到（TS:184-186 读它、回写它）。
//! - **`right` 段是约束（IK 链骨）专用**（阶段 3 收口，F3.1 已证伪）：`Constraint.update`
//!   的双骨变体要写 root **之后**的链骨（`chain.parent == root` ⟹ F3.2 ⟹ `idx > root`），
//!   见 `constraint.rs` 模块文档的借用方案表。
//!
//! 父/宿主解析在 `update_global_transform_matrix` 内部完成（对齐 TS:111-117 /
//! TS:174-175 的位置，调用方传 `left` 即可）；**约束侧**因链骨祖先区间跨三段，
//! 改由 `update_by_constraint_resolved` 收预解析借用（见该方法文档）。
//! F3.2（sortedBones 拓扑序）保证父/宿主下标 < 自身下标 → 都在 `left` 里；
//! 违反时直接 panic（免费断言，指南任务 6 坑 4 同款处理）。

use super::constraint::Constraint;
use super::slot::Slot;
use super::surface::SurfaceState;
use super::transform_object::TransformObject;
use super::{ArmatureIdx, BoneIdx};
use crate::core::dragon_bones::DragonBones;
use crate::core::enums::{BoneType, OffsetMode};
use crate::geom::transform::PI;
use crate::geom::{Matrix, Transform};
use crate::model::{BoneData, BoneKind};

/// Bone 的运行时载荷（指南任务 4 形态：普通骨无载荷，Surface 挂网格状态）。
///
/// TS 用类继承（`Surface extends Bone`，Surface.ts:27）+ `instanceof` 分派；
/// Rust 单结构体 + 枚举载荷，`bone_type`（D-3.3 缓存）保持与 data 层一致供
/// Slot 的 mesh 分支（Slot.ts:724）零修改读取。
#[derive(Default)]
pub enum BoneRuntime {
    /// 普通骨（`Bone`）——`_onClear` 复位目标。
    #[default]
    Bone,
    /// `Surface`（Surface.ts:27）——载荷字段见 `surface.rs::SurfaceState`。
    Surface(SurfaceState),
}

/// 对齐 `Bone extends TransformObject`（Bone.ts:43）。
///
/// 跨模块契约字段（**勿改名**，`event`/`slot`/`armature`/`factory` 依赖）：
/// - `base`：TransformObject 组合 + `Deref`/`DerefMut` 透传（`bone.global` 同 TS 写法）
/// - `idx`：本骨架内索引（约束 `Constraint::root === this` 的比较用，Bone.ts:378）
/// - `bone_name`：→ `Armature::get_bone` 按名查找（Armature.ts:589）
/// - `parent`：按 `BoneData.parent` 名解析（Bone.ts:354-356；F3.2 拓扑序保证 p < idx）
/// - `bone_type`：`BoneData.kind.type_()` 的缓存（D-3.3，update 热路径零查找）——
///   Slot 的 mesh 分支读 `parent.bone_type`（Slot.ts:724）
/// - `children_transform_dirty`：Slot mesh 分支读**父骨**的该脏标记
///   （Slot.ts:728-729；`_isBonesUpdate` 亦只读 `_childrenTransformDirty`，:362-364）
/// - `transform_dirty`：Slot **从不读**骨的该标记——Slot.ts:199/749 的 `_transformDirty`
///   是 Slot 自己的字段；骨的 `transform_dirty` 外部写方是 TimelineState（阶段 5/6）
///
/// 其余字段默认值 = `_onClear` 复位值（见 [`Default`] 逐行注释）。
pub struct Bone {
    /// TransformObject 继承（TS `super` 字段）。
    pub base: TransformObject,
    /// 本骨架内骨骼索引（推入 `Armature.bones` 时定）。
    pub idx: BoneIdx,
    /// 骨骼名（= `BoneData.name`；兼作 `_boneData !== null` 幂等判定，见 [`Self::init`]）。
    pub bone_name: String,
    /// 父骨索引（TS `_parent: Bone | null`）。
    pub parent: Option<BoneIdx>,
    /// 对齐 `_boneData.type`（D-3.3 缓存；`BoneKind::type_()`）。
    pub bone_type: BoneType,
    /// 对齐 `_transformDirty`（Bone.ts:67），`_onClear` = false。
    pub transform_dirty: bool,
    /// 对齐 `_childrenTransformDirty`（Bone.ts:71），`_onClear` = false。
    pub children_transform_dirty: bool,

    // —— 以下为 TS `_onClear`（Bone.ts:92-107）字段全集 ——
    /// 对齐 `offsetMode`（Bone.ts:59），`_onClear` = `OffsetMode::Additive`（:95）。
    pub offset_mode: OffsetMode,
    /// 对齐 `animationPose`（Bone.ts:63），`_onClear` `.identity()`（:96）。
    pub animation_pose: Transform,
    /// 对齐 `_localDirty`（Bone.ts:72），`_onClear` = **true**（:100）——
    /// 与相邻的两个脏标记相反：每次 `update` 尾部还被重置为 true（:440，F3.6），
    /// 只有 `updateByConstraint`（:447）会把它置 false，防止同帧重复算矩阵。
    pub local_dirty: bool,
    /// 对齐 `_hasConstraint`（Bone.ts:76），`_onClear` = false（:101）——
    /// 写入点在 `Constraint::init`（Constraint.ts:203，**只标约束根骨**）：
    /// F3.1「9 素材无约束」已证伪（龙/Rooster×3 共 4 个 IK），阶段 3 收口后
    /// `Bone::update` ① 会据此进约束段（Bone.ts:376-382/402-408）。
    pub has_constraint: bool,
    /// 对齐 `_visible`（Bone.ts:77），`_onClear` = true（:102）；setter 见 [`Self::set_visible`]。
    pub visible: bool,
    /// 对齐 `_cachedFrameIndex`（Bone.ts:78），`_onClear` = -1（:103）；阶段 3 恒 -1。
    pub cached_frame_index: i32,
    /// 对齐 `_cachedFrameIndices`（Bone.ts:90），`_onClear` = null（:106）——
    /// frame index → 缓存槽位的映射表，阶段 6（P1-4）由动画侧填充，阶段 3 恒 `None`。
    pub cached_frame_indices: Option<Vec<i32>>,

    // —— D-3.3 缓存（TS 在 `_boneData` 上，update 热路径零查找）——
    /// 对齐 `_boneData.inheritTranslation`（Bone.ts:190/222/240 读）。
    pub inherit_translation: bool,
    /// 对齐 `_boneData.inheritRotation`（Bone.ts:180/200/256 读）。
    pub inherit_rotation: bool,
    /// 对齐 `_boneData.inheritScale`（Bone.ts:178 大分支条件）。
    pub inherit_scale: bool,
    /// 对齐 `_boneData.inheritReflection`（Bone.ts:269）。
    pub inherit_reflection: bool,
    /// 对齐 `_boneData.length`（阶段 6 路径约束用；`_onClear` 语义上随 data 清空）。
    pub length: f64,
    /// 运行时载荷（TS 类继承的 Rust 形态，见 [`BoneRuntime`]）。
    pub runtime: BoneRuntime,
}

/// 默认值 = `_onClear` 复位值（Bone.ts:92-107 + super TransformObject.ts:100-111）。
impl Default for Bone {
    fn default() -> Self {
        Self {
            base: TransformObject::default(), // super._onClear()（TransformObject.ts:100-111）
            idx: BoneIdx(0),                  // Rust 侧（init 预置，TS 无此字段）
            bone_name: String::new(),         // :104 `_boneData = null`（D-3.3 缓存字段方案）
            parent: None,                     // :105 `_parent = null as any`
            // Rust 侧：bone_type 是 `_boneData.type` 的缓存 → 无数据时与 `_boneData = null`
            // 对应取默认 Bone（`BoneKind::Bone::type_()` 同值）。
            bone_type: BoneType::Bone,
            transform_dirty: false,        // :98
            children_transform_dirty: false, // :99
            offset_mode: OffsetMode::Additive, // :95
            animation_pose: Transform::default(), // :96 `.identity()`
            // ★ :100 `_localDirty = true`（与 :98/:99 相反的反直觉默认：
            //   update 423 只有 localDirty 才真算矩阵，而它在每次 update 尾部 :440 重新武装）
            local_dirty: true,
            has_constraint: false, // :101（init 前恒 false；Constraint::init TS:203 置位）
            visible: true,         // :102
            cached_frame_index: -1, // :103
            cached_frame_indices: None, // :106（阶段 6 共享 ArmatureData 缓存表，阶段 3 恒 null）
            // D-3.3 缓存字段：TS 位于 boneData、_onClear 不碰；Rust 随 `_boneData = null`
            // （bone_name 清空）一并回到“未初始化”初值（Slot::on_clear 同款处理）。
            inherit_translation: false,
            inherit_rotation: false,
            inherit_scale: false,
            inherit_reflection: false,
            length: 0.0,
            runtime: BoneRuntime::Bone, // Surface._onClear（Surface.ts:63 `_bone = null` 同步语义）
        }
    }
}

impl Bone {
    /// 对齐 `Bone.init(boneData, armatureValue)`（Bone.ts:345-361）。
    ///
    /// # 参数（任务 7 调用面）
    /// - `bone_data`：只读来源，需要的字段值缓存进本骨（D-3.3）。
    /// - `armature: ArmatureIdx` = TS `armatureValue._armature`（D-3.1 不存对象）。
    /// - `existing_bones`：调用方**已推入**的骨表——按名解析父骨（TS:355
    ///   `armature.getBone(name)` 只查 `_addBone` 之前的骨？不，TS `_addBone` 在 :358，
    ///   父骨早已在 `_bones` 里，等价于此处的 `existing_bones`；F3.2 拓扑序保证父在场）。
    ///
    /// **不 push**：TS:358 `armature._addBone(this)` 把自己推入 `_bones`；Rust 的
    /// `init` 刻意差异为**只预置 `idx = existing_bones.len()`**（= 调用方随后 push
    /// 后的下标，与 TS `_addBone` 的插入位置一致），推入由调用方完成——这样 `init`
    /// 不需要 `&mut Armature`，装配循环里“建一根推一根”顺序不变。
    ///
    /// 幂等 guard（TS:346-348 `_boneData !== null`）：Rust 不持 data 引用，用
    /// `bone_name` 非空判定（`BoneData.name` 的缓存，`on_clear` 清空；与 `_boneData`
    /// 同生同灭）。
    pub fn init(&mut self, bone_data: &BoneData, armature: ArmatureIdx, existing_bones: &[Bone]) {
        if !self.bone_name.is_empty() {
            // :346-348 已 init 过直接返回
            return;
        }

        self.bone_name.clone_from(&bone_data.name); // :350 `_boneData = boneData`
        self.base.armature = Some(armature); // :351
        self.alpha = bone_data.alpha; // :352 ← alpha 来自 BoneData.alpha

        // :354-356 按名解析父骨（TS `armature.getBone`，Armature.ts:589 线性遍历）
        if let Some(parent_name) = &bone_data.parent {
            if let Some(parent_idx) = existing_bones
                .iter()
                .position(|bone| bone.bone_name == *parent_name)
            {
                self.parent = Some(BoneIdx(parent_idx as u32));
            } else {
                // F3.2：`_buildBones` 按 sortedBones 拓扑序创建（BaseFactory.ts:168-173）
                // → 父骨必已 push 在场；找不到 = 数据或调用序错误 → 告警后按根骨处理。
                log::warn!(
                    "bone {}: parent bone '{}' not found in existing bones (Bone.ts:355)",
                    self.bone_name,
                    parent_name
                );
                self.parent = None;
            }
        }

        // :358 `_addBone(this)` → Rust 刻意差异：init 不 push，只预置下标（见方法文档）
        self.idx = BoneIdx(existing_bones.len() as u32);

        // D-3.3 缓存（指南 D-3.3：build 一次、update 零查找）
        self.bone_type = bone_data.type_();
        self.inherit_translation = bone_data.inherit_translation;
        self.inherit_rotation = bone_data.inherit_rotation;
        self.inherit_scale = bone_data.inherit_scale;
        self.inherit_reflection = bone_data.inherit_reflection;
        self.length = bone_data.length;

        // :360 origin 指向 data 的 Transform（F3.3 只读 → Rust 值拷贝，与 clone 恒等；
        // Transform 为 Copy，直接赋值 = clone 一份）
        self.origin = Some(bone_data.transform);

        // —— Surface 分支（Surface.ts:442-477：super.init 之后的网格初始化）——
        match &bone_data.kind {
            BoneKind::Bone => {
                self.runtime = BoneRuntime::Bone; // 防御：复用骨时清掉旧载荷
            }
            BoneKind::Surface {
                segment_x,
                segment_y,
                geometry,
            } => {
                // TS:449-467 网格常量与缓冲、TS:469-475 `_bone`（宿主骨）解析，
                // 一并收进 SurfaceState::init（surface.rs）。
                let host = self.resolve_surface_host(existing_bones); // TS:469-475
                self.runtime = BoneRuntime::Surface(SurfaceState::init(
                    *segment_x,
                    *segment_y,
                    geometry,
                    host,
                ));
            }
        }
    }

    /// TS Surface.ts:469-475 的 `_bone`（宿主骨）解析——父是普通骨取父、
    /// 父是 Surface 继承父的 `_bone`（可能 null）、无父保持 null。
    fn resolve_surface_host(&self, existing_bones: &[Bone]) -> Option<BoneIdx> {
        let parent_idx = self.parent?; // :469 `_parent !== null`
        let parent = existing_bones.get(parent_idx.index()); // F3.2 父必在场
        let parent = match parent {
            Some(parent) => parent,
            None => {
                // 与 init 的父骨告警同源（那里已 warn），此处静默按“无宿主”处理
                return None;
            }
        };
        if parent.bone_type == BoneType::Bone {
            Some(parent_idx) // :470-471 父是普通骨 → `_bone = _parent`
        } else {
            // :473-474 父是 Surface → 继承父的 `_bone`（嵌套 Surface 时可能仍为 null）
            parent.surface_state().and_then(|state| state.bone)
        }
    }

    /// 对齐 `Bone.update(cacheFrameIndex)`（Bone.ts:365-441）——四分支脏标记机；
    /// Surface 覆盖（Surface.ts:481-586）的差异段按 `runtime` 分派。
    ///
    /// 三段结构：①分支选择（TS:366-415 / Surface:482-531 **逐字相同**）→
    /// ②脏标记消费（Bone:417-438 与 Surface:533-580 差异段分派）→ ③尾部重置（TS:440/585）。
    ///
    /// - `cache_frame_index`：阶段 3 恒 -1（P1-4 缓存帧阶段 6 接入）。
    /// - `left`：索引小于自身的骨（`split_at_mut(i)` 左半，F3.2）——父骨/宿主解析
    ///   都在其中（`update_global_transform_matrix` 的 `ancestors` 即它），见模块文档。
    /// - `right`：索引大于自身的骨（`split_first_mut` 之后的右段）——**仅约束段用**
    ///   （阶段 3 收口：链骨/后段 target，`constraint.rs` 借用方案表）。
    /// - `constraints`：本骨架约束全表（TS `armature._constraints`，Armature.ts:98），
    ///   ① 内按 `constraint.root == self.idx` 过滤（TS:378/404）。
    /// - `flip_x` / `flip_y`：`Armature.flip_x/flip_y` 下传（TS:119-120 在
    ///   `_updateGlobalTransformMatrix` 内读 armature，Rust 无 armature 引用 → 参数化）。
    ///
    /// # Panics
    /// - 与改写前相同（父骨越界 expect、P1-4 缓存帧 `unreachable!`）；新增
    ///   约束段的链骨越界/`chain ≤ root` panic 见 [`Constraint::update`]。
    pub fn update(
        &mut self,
        cache_frame_index: i32,
        left: &mut [Bone],
        right: &mut [Bone],
        constraints: &mut [Constraint],
        flip_x: bool,
        flip_y: bool,
    ) {
        // TS:411 改写的是入参 `cacheFrameIndex` 本身 → Rust 用可变局部副本同构
        let mut cache_frame_index = cache_frame_index;
        let is_surface = matches!(self.runtime, BoneRuntime::Surface(_));

        // ---------- ① 分支选择（Bone.ts:366-415 / Surface.ts:482-531 逐字相同）----------
        if cache_frame_index >= 0 && self.cached_frame_indices.is_some() {
            // —— P1-4（阶段 6）缓存帧表分支：阶段 3 `_cachedFrameIndices` 恒 None →
            // 整段不可达；结构照抄以便阶段 6 直接接（同 slot.rs ⑩）——
            // TS:366 `cacheFrameIndex >= 0 && _cachedFrameIndices !== null`
            //
            // ★ :367 读成**局部拷贝**后立刻结束借用（改写前是 `let … = self.cached_
            // frame_indices.as_mut()` 长持到 :393）：约束段（:376-382）要把整个
            // `self` 作为根骨借出（`Constraint::update(root, …)`），字段级长借会让
            // 借用检查失败。两次读之间 `cached_frame_indices` 无任何写点 → 语义等价。
            let cached_frame_index = self
                .cached_frame_indices
                .as_ref()
                .expect("分支条件已判 is_some")[cache_frame_index as usize]; // :367 本骨在该缓存帧的槽位号
            if cached_frame_index >= 0 && self.cached_frame_index == cached_frame_index {
                // :368 Same cache.
                self.transform_dirty = false; // :369
            } else if cached_frame_index >= 0 {
                // :371 Has been Cached.
                self.transform_dirty = true; // :372
                self.cached_frame_index = cached_frame_index; // :373
            } else {
                // :375
                self.run_constraints(left, right, constraints, flip_x, flip_y); // :376-382

                // :384-387 Dirty 判定（父的 `_childrenTransformDirty`——直接读字段）
                if self.transform_dirty
                    || self
                        .parent
                        .and_then(|parent| left.get(parent.index()))
                        .is_some_and(|parent| parent.children_transform_dirty)
                {
                    self.transform_dirty = true; // :388
                    self.cached_frame_index = -1; // :389
                } else if self.cached_frame_index >= 0 {
                    // :391 Same cache, but not set index yet.
                    self.transform_dirty = false; // :392
                    // :393 回填映射（重新借 `cached_frame_indices`；索引写的是
                    // **外层** `cache_frame_index`（TS:393 `cacheFrameIndex` 参数，
                    // 非内层 `cachedFrameIndex` 槽位号）——两者仅差一个 d，勿混）
                    if let Some(indices) = self.cached_frame_indices.as_mut() {
                        indices[cache_frame_index as usize] = self.cached_frame_index;
                    }
                } else {
                    // :395 Dirty.
                    self.transform_dirty = true; // :396
                    self.cached_frame_index = -1; // :397
                }
            }
        } else {
            // :401 —— ★ 阶段 3 只会走这里（cache_frame_index 恒 -1）
            self.run_constraints(left, right, constraints, flip_x, flip_y); // :402-408

            if self.transform_dirty
                || self
                    .parent
                    .and_then(|parent| left.get(parent.index()))
                    .is_some_and(|parent| parent.children_transform_dirty)
            {
                // :410 Dirty.
                cache_frame_index = -1; // :411 ★ 改写入参副本 → 让 :422 的 isCache 恒 false
                self.transform_dirty = true; // :412
                self.cached_frame_index = -1; // :413
            }
            // :410 不满足时什么都不做（transform_dirty 可能为 false → 走 :436 分支）
        }

        // ---------- ② 脏标记消费（Bone.ts:417-438 / Surface.ts:533-580 分派）----------
        if self.transform_dirty {
            self.transform_dirty = false; // :418 / Surface:534 ★ 立刻清
            self.children_transform_dirty = true; // :419 / Surface:535 ★ 向子传播

            if is_surface {
                // —— Surface.ts:537-539 新增 1/3：清矩阵缓存 flag（-1.0 < 0 → 未命中）——
                self.surface_reset_matrix_cache();
                // —— Surface.ts:541 新增 2/3：重算世界坐标顶点（无条件，不看 localDirty）——
                // 父骨只读（Surface.ts:100-130 三分支）→ 共享借用即可，随后让位给
                // 骨路径的 &mut left（NLL：本借用最后使用点就在这里）。
                let parent_ref = self.parent.and_then(|parent| left.get(parent.index()));
                self.surface_update_vertices(parent_ref);
            }

            if self.cached_frame_index < 0 {
                // :421 / Surface:543 没有可用缓存
                let is_cache = cache_frame_index >= 0; // :422 / Surface:544
                if self.local_dirty {
                    // :423 / Surface:545 ★ 只有 localDirty 才真算矩阵
                    if is_surface {
                        // Surface.ts:546（Surface 版忽略 isCache，Surface.ts:135）
                        self.surface_update_global_transform_matrix(is_cache);
                    } else {
                        // :424
                        self.update_global_transform_matrix(is_cache, left, flip_x, flip_y);
                    }
                }

                if is_cache && self.cached_frame_indices.is_some() {
                    // :427-429 / Surface:549-550 `_armatureData.setCacheFrame(...)`
                    // （ArmatureData.ts:282-301）——阶段 3 不可达：isCache 为真要求
                    // cache_frame_index ≥ 0，而活路径里 :411 在脏时必把它改写成 -1；
                    // 未脏则根本进不到本段。★ 注意分支①（Bone.ts:366-400）**不改写**
                    // 入参 cacheFrameIndex（只有 :411 改写）→ 阶段 6 打开
                    // `cached_frame_indices` 后，从分支①带着 cacheFrameIndex ≥ 0 走到
                    // 本段是**可到达路径**（届时接 setCacheFrame），勿误判恒不可达。
                    unreachable!("P1-4：阶段 6 实现缓存帧（ArmatureData::setCacheFrame）");
                }
            } else {
                // :431-432 / Surface:553-554 `_armatureData.getCacheFrame(...)`——同上
                // 不可达：cached_frame_index ≥ 0 只在①缓存分支被写入（阶段 3 恒 None）。
                unreachable!("P1-4：阶段 6 实现缓存帧（ArmatureData::getCacheFrame）");
            }

            if is_surface {
                // —— Surface.ts:556-579 新增 3/3：填 _hullCache——
                // 依赖本段 546 算出的 global/矩阵，顺序契约：清 flag → 顶点 → 矩阵 → hull。
                self.surface_update_hull_cache();
            }
        } else if self.children_transform_dirty {
            // :436 / Surface:581
            self.children_transform_dirty = false; // :437 / Surface:582
        }

        // ---------- ③ 尾部重置（:440 / Surface:585）----------
        self.local_dirty = true; // ★★ 不是 false！每次 update 结束重新武装 localDirty（F3.6）
    }

    /// `Bone::update` ① 的约束段（Bone.ts:376-382 / :402-408；Surface.ts:493-499 /
    /// :519-525 与 Bone 逐字相同 → 两分支共用本方法）。
    ///
    /// 位置契约照抄：`_hasConstraint` 为真时遍历**全部**约束、按
    /// `constraint._root === this`（:378/:404）过滤，在 **transform_dirty 消费
    /// （:417）之前**执行；`Constraint::update` 的第一件事即 `_root.updateByConstraint()`
    /// （Constraint.ts:207）。`self`（根骨自身）与 `left`/`right` 三段互不重叠
    /// → 一次调用全部借出（`constraint.rs` 模块文档借用方案表）。
    fn run_constraints(
        &mut self,
        left: &mut [Bone],
        right: &mut [Bone],
        constraints: &mut [Constraint],
        flip_x: bool,
        flip_y: bool,
    ) {
        if !self.has_constraint {
            return; // :376 / :402
        }

        for constraint in constraints.iter_mut() {
            if constraint.root == self.idx {
                // :378/:404 `constraint._root === this`；:379/:405 `constraint.update()`
                constraint.update(self, left, right, flip_x, flip_y);
            }
        }
    }

    /// 对齐 `_updateGlobalTransformMatrix(isCache)`（Bone.ts:109-330）——**解析外壳**：
    /// 按 TS:111-117（`const parent = this._parent`）/ :174-175（`isSurface`/
    /// `surfaceBone`）把 `self.parent` 在 `ancestors`（= `[0, idx)` 祖先切片，F3.2）里
    /// 解成 (parent, 宿主) 两个可变借用，再交 [`Self::update_global_transform_matrix_resolved`]
    /// 跑 220 行主体。
    ///
    /// 消费点：`update` :424（`left` 即该骨的祖先切片）、
    /// `update_by_constraint_resolved` :450（isCache 恒 true，parent/宿主由约束侧
    /// 跨区预解析——链骨祖先区间跨三段，见 `constraint.rs` 模块文档）。
    fn update_global_transform_matrix(
        &mut self,
        is_cache: bool,
        ancestors: &mut [Bone],
        flip_x: bool,
        flip_y: bool,
    ) {
        let (parent, surface_bone) = Self::resolve_parent_context(self.parent, ancestors);
        self.update_global_transform_matrix_resolved(
            is_cache,
            parent,
            surface_bone,
            flip_x,
            flip_y,
        );
    }

    /// `_updateGlobalTransformMatrix` 主体（Bone.ts:109-330 逐行）——与外壳的唯一
    /// 差异：parent/宿主为**入参**（TS 对象别名在 Rust 的显式化），其余逐行不动。
    ///
    /// - 两个调用方：[`Self::update_global_transform_matrix`]（祖先切片解析）与
    ///   [`Self::update_by_constraint_resolved`]（约束三段解析）。
    /// - `inherit` 仍由 `self.parent.is_some()`（TS:121）判定；Override（:163 关掉
    ///   inherit）时传入的借用虽已求得但不消费——与 TS「:117 已取别名、:163 后不用」同序。
    fn update_global_transform_matrix_resolved(
        &mut self,
        is_cache: bool,
        parent: Option<&mut Bone>,
        surface_bone: Option<&mut Bone>,
        flip_x: bool,
        flip_y: bool,
    ) {
        // ---- TS:111-116 局部别名（值均为 Copy，先取副本避免借用打架）----
        let origin = self.origin; // :114
        let offset = self.offset; // :115
        let animation_pose = self.animation_pose; // :116
        let mut inherit = self.parent.is_some(); // :121 `inherit = parent !== null`
        // TS:122 `let rotation = 0.0`——0 初值在所有可达路径上都先被赋值再读取，
        // Rust 延迟初始化以消 `unused_assignments`（语义等价）。
        let mut rotation: f64;

        // ---- TS:124-171 offsetMode 三分支：只决定 global 初值 + 是否允许 inherit ----
        match self.offset_mode {
            OffsetMode::Additive => {
                // :124
                if let Some(origin) = origin {
                    // :125（TS:126 的整式被原版注释掉，逐字段写）
                    // :127-129 x 与 scaleX/scaleY 恒 ±/*，不随 yDown 变号
                    self.global.x = origin.x + offset.x + animation_pose.x;
                    self.global.scale_x = origin.scale_x * offset.scale_x * animation_pose.scale_x;
                    self.global.scale_y = origin.scale_y * offset.scale_y * animation_pose.scale_y;

                    if DragonBones::Y_DOWN {
                        // :131
                        self.global.y = origin.y + offset.y + animation_pose.y;
                        self.global.skew = origin.skew + offset.skew + animation_pose.skew;
                        self.global.rotation =
                            origin.rotation + offset.rotation + animation_pose.rotation;
                    } else {
                        // :136 y/skew/rotation 对 **offset 部分**取负（pose 永远相加）
                        self.global.y = origin.y - offset.y + animation_pose.y;
                        self.global.skew = origin.skew - offset.skew + animation_pose.skew;
                        self.global.rotation =
                            origin.rotation - offset.rotation + animation_pose.rotation;
                    }
                } else {
                    // :142
                    self.global.copy_from(&offset); // :143

                    if !DragonBones::Y_DOWN {
                        // :145 取负的是 offset 的拷贝
                        self.global.y = -self.global.y;
                        self.global.skew = -self.global.skew;
                        self.global.rotation = -self.global.rotation;
                    }

                    self.global += animation_pose; // :151 pose 仍相加（Transform.add）
                }
            }
            OffsetMode::None => {
                // :154 本分支完全不看 offset，也不看 yDown
                if let Some(origin) = origin {
                    // :155
                    self.global.copy_from(&origin);
                    self.global += animation_pose; // :156
                } else {
                    // :158
                    self.global.copy_from(&animation_pose); // :159
                }
            }
            OffsetMode::Override => {
                // :162 else 分支（Override）
                inherit = false; // :163 ★ 顺手关掉继承
                self.global.copy_from(&offset); // :164

                if !DragonBones::Y_DOWN {
                    // :166 不加 animationPose、不用 origin
                    self.global.y = -self.global.y;
                    self.global.skew = -self.global.skew;
                    self.global.rotation = -self.global.rotation;
                }
            }
        }

        if inherit {
            // TS:117 + TS:174-175：parent/宿主已由调用方（外壳或约束）解析传入
            let Some(parent) = parent else {
                unreachable!("inherit ⟹ self.parent 非空（TS:121 与 :117 同源）");
            };

            // :174 `isSurface = parent._boneData.type === BoneType.Surface`
            let is_surface = parent.bone_type == BoneType::Surface;
            // :175 `surfaceBone = isSurface ? (parent as Surface)._bone : null`
            // —— 已由 resolve 同步给出（非 Surface 父恒 None）
            // :176 parentMatrix：Surface 父按当前 global 局部坐标在曲面上采样，
            // 返回 Matrix 值拷贝（消除 TS 静态 _helpMatrix 被下次采样覆盖的隐患）
            let parent_matrix = if is_surface {
                parent.get_global_transform_matrix(self.global.x, self.global.y)
            } else {
                parent.global_transform_matrix
            };

            if self.inherit_scale && (!is_surface || surface_bone.is_some()) {
                // :178 ★ 复合大分支条件（不是单纯 inheritScale！两条分支数学完全不同）
                if is_surface {
                    // :179 —— Surface 父骨子路径（笔记 §2.3.4）
                    if self.inherit_rotation {
                        // :180-182 直接读 parent.global（Surface 的 update 已写新值、
                        // Surface.ts:162 清脏 → 无需 updateGlobalTransform）
                        self.global.rotation += parent.global.rotation;
                    }

                    // :184 惰性回填宿主骨的 global
                    let surface_bone = surface_bone
                        .expect(":178 条件保证 isSurface ⟹ surfaceBone 非空（Surface.ts:471/474）");
                    surface_bone.update_global_transform();
                    // :185-186 缩放累乘宿主骨
                    let host_scale_x = surface_bone.global.scale_x;
                    let host_scale_y = surface_bone.global.scale_y;
                    self.global.scale_x *= host_scale_x;
                    self.global.scale_y *= host_scale_y;
                    // :187 ★ 就地把 global.x/y 变成世界坐标（delta=false，含 tx/ty）
                    let (x, y) =
                        parent_matrix.transform_point(self.global.x, self.global.y, false);
                    self.global.x = x;
                    self.global.y = y;
                    // :188 ★ 用已被改写的 global 再生成矩阵
                    // （Deref 借用限制：接收者先拷贝局部再调用，slot.rs 同款）
                    let global = self.global;
                    global.to_matrix(&mut self.global_transform_matrix);

                    if self.inherit_translation {
                        // :190-192 从矩阵取回世界坐标
                        self.global.x = self.global_transform_matrix.tx;
                        self.global.y = self.global_transform_matrix.ty;
                    } else {
                        // :194-196 把局部平移写回矩阵
                        self.global_transform_matrix.tx = self.global.x;
                        self.global_transform_matrix.ty = self.global.y;
                    }
                } else {
                    // :199 —— inheritScale && 普通父骨子路径（笔记 §2.3.5）
                    if !self.inherit_rotation {
                        // :200-201 惰性回填父 global（★ 唯一的写回点之一）
                        parent.update_global_transform();

                        let parent_rotation = parent.global.rotation;
                        if flip_x && flip_y {
                            rotation = self.global.rotation - (parent_rotation + PI); // :204
                        } else if flip_x {
                            rotation = self.global.rotation + parent_rotation + PI; // :207
                        } else if flip_y {
                            rotation = self.global.rotation + parent_rotation; // :210
                        } else {
                            rotation = self.global.rotation - parent_rotation; // :213
                        }

                        self.global.rotation = rotation; // :216
                    }

                    let global = self.global;
                    global.to_matrix(&mut self.global_transform_matrix); // :219
                    self.global_transform_matrix.concat(&parent_matrix); // :220 ★ this = this × parentMatrix

                    if self.inherit_translation {
                        // :222-224 ★ 从矩阵取回世界坐标（与 :240-245 方向相反！）
                        self.global.x = self.global_transform_matrix.tx;
                        self.global.y = self.global_transform_matrix.ty;
                    } else {
                        // :226-228 ★ 保留局部平移、矩阵承担父变换
                        self.global_transform_matrix.tx = self.global.x;
                        self.global_transform_matrix.ty = self.global.y;
                    }

                    if is_cache {
                        // :231-232 立刻分解回 global
                        let matrix = self.global_transform_matrix;
                        self.global.from_matrix(&matrix);
                    } else {
                        // :234-235 ★ 惰性：延后到 updateGlobalTransform()
                        self.global_dirty = true;
                    }
                }
            } else {
                // :239 —— inheritScale == false（或 Surface 且宿主为 null）的另一条
                // 数学路径（笔记 §2.3.6）
                if self.inherit_translation {
                    // :240-244 ★ 手工乘父矩阵（不写回矩阵的 tx/ty）
                    let x = self.global.x;
                    let y = self.global.y;
                    self.global.x =
                        parent_matrix.a * x + parent_matrix.c * y + parent_matrix.tx;
                    self.global.y =
                        parent_matrix.b * x + parent_matrix.d * y + parent_matrix.ty;
                } else {
                    // :246-253 不继承平移 → 只做 flip 反号
                    if flip_x {
                        self.global.x = -self.global.x;
                    }
                    if flip_y {
                        self.global.y = -self.global.y;
                    }
                }

                if self.inherit_rotation {
                    // :256-257 惰性回填父 global（★ 另一个写回点）
                    parent.update_global_transform();

                    let parent_rotation = parent.global.rotation;
                    if parent.global.scale_x < 0.0 {
                        // :259-260 判定 1：父 global.scaleX
                        rotation = self.global.rotation + parent_rotation + PI;
                    } else {
                        // :262-263
                        rotation = self.global.rotation + parent_rotation;
                    }

                    // :266 判定 2：★ 父矩阵行列式 det<0（反射），不是 global 的
                    if parent_matrix.a * parent_matrix.d - parent_matrix.b * parent_matrix.c
                        < 0.0
                    {
                        rotation -= self.global.rotation * 2.0; // :267 用当前（未被 :278 覆写）的 rotation

                        if flip_x != flip_y || self.inherit_reflection {
                            // :269 ★ 复合条件（JS `!==` 对 boolean = 异或）
                            self.global.skew += PI; // :270
                        }

                        if !DragonBones::Y_DOWN {
                            // :273-274
                            self.global.skew = -self.global.skew;
                        }
                    }

                    self.global.rotation = rotation; // :278
                } else if flip_x || flip_y {
                    // :280
                    if flip_x && flip_y {
                        rotation = self.global.rotation + PI; // :282
                    } else {
                        if flip_x {
                            rotation = PI - self.global.rotation; // :285-287
                        } else {
                            rotation = -self.global.rotation; // :288-290
                        }

                        self.global.skew += PI; // :292（单 flip 才加）
                    }

                    self.global.rotation = rotation; // :295
                }

                let global = self.global;
                global.to_matrix(&mut self.global_transform_matrix); // :298 ★ 本路径唯一矩阵生成点（无 concat）
            }
        } else {
            // :301 —— !inherit（Override 模式或无父骨），笔记 §2.3.7
            if flip_x || flip_y {
                // :302
                if flip_x {
                    self.global.x = -self.global.x; // :303-305
                }
                if flip_y {
                    self.global.y = -self.global.y; // :307-309
                }

                if flip_x && flip_y {
                    rotation = self.global.rotation + PI; // :312
                } else {
                    if flip_x {
                        rotation = PI - self.global.rotation; // :315-317
                    } else {
                        rotation = -self.global.rotation; // :318-320
                    }

                    self.global.skew += PI; // :322
                }

                self.global.rotation = rotation; // :325
            }

            let global = self.global;
            global.to_matrix(&mut self.global_transform_matrix); // :328
        }
    }

    /// TS:117（`const parent = this._parent as Bone`）与 TS:174-175
    /// （`isSurface`/`surfaceBone`）的 Rust 解析：把 `parent: BoneIdx` 在
    /// `ancestors`（索引 < 自身，F3.2）里解成可变借用。
    ///
    /// 宿主骨（`surfaceBone`）是 Surface 父的祖先（Surface.ts:469-475）→
    /// 宿主下标 < 父下标 → `split_at_mut(父)` 后宿主在左半、父在右半，
    /// 两者与调用方已切出的 `self` 三方互不重叠。
    ///
    /// `pub(crate)` 消费方：本模块的两个外壳 + `constraint.rs` 的链骨
    /// `parent < root` 分支（约束三段方案的 left 段复用，见其模块文档）。
    pub(crate) fn resolve_parent_context(
        parent: Option<BoneIdx>,
        ancestors: &mut [Bone],
    ) -> (Option<&mut Bone>, Option<&mut Bone>) {
        let Some(parent_idx) = parent else {
            return (None, None);
        };
        let parent_pos = parent_idx.index();
        // 先共享借用判定宿主（TS:174-175：isSurface 看父的 boneType，surfaceBone
        // 取父 payload；解析位置随函数入口前移，判定逻辑逐字同构）
        let host_idx = ancestors
            .get(parent_pos)
            .expect("父骨不在祖先切片中（F3.2 拓扑序被破坏，Bone.ts:355）")
            .surface_host_idx(); // `isSurface ? (parent as Surface)._bone : null`

        if let Some(host_idx) = host_idx {
            // 宿主是 surface 的祖先 → host < parent（Surface.ts:471/474 语义）：
            // split_at_mut 后宿主在左半、父在右半，互不重叠。
            let (pre, post) = ancestors.split_at_mut(parent_pos);
            let parent = &mut post[0]; // post 至少 1 元（上面共享借用已确认在界内）
            let host = pre.get_mut(host_idx.index()).expect(
                "Surface 宿主骨不在祖先切片中（Surface.ts:471/474：宿主必为 surface 的祖先）",
            );
            (Some(parent), Some(host))
        } else {
            // 顶层 Surface 无宿主（TS:175 null）或非 Surface 父
            let parent = ancestors
                .get_mut(parent_pos)
                .expect("父骨不在祖先切片中（F3.2 拓扑序被破坏，Bone.ts:355）");
            (Some(parent), None)
        }
    }

    /// 对齐 `Bone._updateAlpha()`（Bone.ts:334-341）——
    /// `Armature::advance_time` 的 alpha 传播在 bones update 之前每帧调一次
    /// （Armature.ts:328-339，`_alphaDirty` 时遍历 `_bones`）。
    ///
    /// - `parent`：本骨父骨（无父传 None）；`armature_global_alpha`：骨架
    ///   `globalAlpha`（根骨用）。
    pub fn update_alpha(&mut self, parent: Option<&Bone>, armature_global_alpha: f64) {
        self.global_alpha = if let Some(parent) = parent {
            self.alpha * parent.global_alpha // :336
        } else {
            self.alpha * armature_global_alpha // :339
        };
    }

    /// 对齐 `Bone.updateByConstraint()`（Bone.ts:445-455）——**祖先切片路径**：
    /// `ancestors = [0, idx)`（F3.2 父/宿主必在其中）。
    ///
    /// 调用链（阶段 3 收口，F3.1 已证伪「无约束」）：`Constraint::update` →
    /// `_root.updateByConstraint()`（Constraint.ts:207）——root 祖先区间
    /// `[0, root)` 单切片可构造 → 走本签名；链骨走 [`Self::update_by_constraint_resolved`]。
    ///
    /// `flip_*` 与 [`Self::update`] 同义（:450 的 `_updateGlobalTransformMatrix(true)`
    /// 读 armature flip，参数化下传）。
    ///
    /// TS:446 `localDirty = false` 时直接 return、不碰 parent → Rust 同序先判
    /// `local_dirty` 早退再 resolve（避免多余的父骨越界 panic 点）。
    pub fn update_by_constraint(&mut self, ancestors: &mut [Bone], flip_x: bool, flip_y: bool) {
        // TS:446 `localDirty = false` 时整段直接返回、不碰 parent——resolve 移到
        // 早退之后（消除多余 panic 点：父骨越界 expect 只在真要算矩阵时才可达）。
        if !self.local_dirty {
            return;
        }
        let (parent, surface_bone) = Self::resolve_parent_context(self.parent, ancestors);
        self.update_by_constraint_resolved(parent, surface_bone, flip_x, flip_y);
    }

    /// `Bone.updateByConstraint()` 主体（Bone.ts:445-455 **逐行**）——与外壳的唯一
    /// 差异：parent/宿主为**入参**。
    ///
    /// 存在的理由（约束链骨路径）：链骨的祖先区间 `[0, chain)` **跨 left/root/right
    /// 三段且含 root 自身**，在 `Bone::update`（root 自身即 `self`）里无法构成单一
    /// `&mut [Bone]` 切片 → 由 `constraint.rs::resolve_parent_host` 跨区解析成两个
    /// 显式借用传入；`:448` 脏判定、`:450` 矩阵回填（isCache 恒 true → `global.
    /// fromMatrix` 回填链，龙素材 rotation 修正来源）、`:453` 无条件置脏语义不变。
    pub(crate) fn update_by_constraint_resolved(
        &mut self,
        parent: Option<&mut Bone>,
        surface_bone: Option<&mut Bone>,
        flip_x: bool,
        flip_y: bool,
    ) {
        if self.local_dirty {
            // :446 —— 全库唯一置 localDirty = false 的地方（F3.6 的成对语义）
            self.local_dirty = false; // :447

            let parent_dirty = parent
                .as_ref()
                .is_some_and(|parent| parent.children_transform_dirty); // :448 借用而非消费
            if self.transform_dirty || parent_dirty {
                // :449/:450 ★ isCache 恒 true —— TS 虚方法多态（Surface.ts:133 覆盖
                // `_updateGlobalTransformMatrix`：按顶点拟合、忽略 isCache、无 parent
                // 参与 → 传入的借用在该分支落空）；普通骨走 Bone 主体（parent/宿主入参）
                if matches!(self.runtime, BoneRuntime::Surface(_)) {
                    self.surface_update_global_transform_matrix(true);
                } else {
                    self.update_global_transform_matrix_resolved(
                        true,
                        parent,
                        surface_bone,
                        flip_x,
                        flip_y,
                    );
                }
            }

            self.transform_dirty = true; // :453 无条件置脏（保证后续 update 还走缓存/子传播）
        }
    }

    /// 对齐 `Bone.invalidUpdate()`（Bone.ts:481-483）——**只有一行** `_transformDirty = true`。
    ///
    /// 注：`docs/guide/03-stage3.md` 3.3 转述的“并沿子传播”与原文不符——
    /// 子骨传播靠 `update` :386/:410 读 `_parent._childrenTransformDirty`，
    /// 而 `_childrenTransformDirty` 只在 `update` :419 置位（笔记 §2.7 已核对）。
    pub fn invalid_update(&mut self) {
        self.transform_dirty = true; // :482
    }

    /// 对齐 `Bone.visible` setter（Bone.ts:538-550）——变化时遍历本骨架插槽，
    /// 只刷父骨是自己的槽（`slot._updateVisible()`，Slot.ts:342 → slot.rs 公开转发）。
    ///
    /// getter = 直接读 [`Self::visible`] 字段（TS:535-537 无副作用）。
    pub fn set_visible(&mut self, visible: bool, slots: &mut [Slot]) {
        if self.visible == visible {
            // :539-541
            return;
        }

        self.visible = visible; // :543

        for slot in slots.iter_mut() {
            // :545 `armature.getSlots()`（本骨架 slots 切片）
            if slot.parent == self.idx {
                // :546 `slot.parent === this`（TS 对象身份 → Rust 索引相等）
                slot.update_visible(); // :547 `slot._updateVisible()`
            }
        }
    }

    /// 对齐 `Bone.contains(value)`（Bone.ts:496-507）——`self` 是否为 `value`
    /// 的**祖先**（`value === this` 直接 false）。
    ///
    /// TS 持对象引用沿 `value.parent` 上溯；Rust 存 `BoneIdx` → 传骨表回溯。
    #[must_use]
    pub fn contains(&self, bones: &[Bone], value: BoneIdx) -> bool {
        if value == self.idx {
            return false; // :497-499
        }

        let mut ancestor = Some(value); // :501
        while let Some(current) = ancestor {
            // :502-504 `while (ancestor !== this && ancestor !== null)`
            if current == self.idx {
                return true;
            }
            // TS 沿引用走到 null 返回 false——链上任何骨不在表里（等效断链）
            // 同样按走到 null 处理，返回 false（不 panic）。
            ancestor = bones.get(current.index()).and_then(|bone| bone.parent);
        }

        false // :506 `ancestor === this` 为假
    }

    /// 对齐 `Surface._getGlobalTransformMatrix(x, y)`（Surface.ts:165-437）的
    /// 契约分派：Surface 走三角形拟合（surface.rs 实现）；普通骨返回自身矩阵。
    ///
    /// 普通骨分支的调用方在 TS 不存在——TS 调用方（Slot.ts:584、Bone.ts:176）
    /// 先按 `boneType` 分派、普通骨直接读 `globalTransformMatrix` 不经本方法；
    /// Rust 以单一契约方法统一入口（slot.rs:714-721 已按此调用），故普通骨
    /// 返回 `globalTransformMatrix` 等价于 TS 的直接读取。
    #[must_use]
    pub fn get_global_transform_matrix(&self, x: f64, y: f64) -> Matrix {
        match &self.runtime {
            BoneRuntime::Surface(_) => self.surface_get_global_transform_matrix(x, y),
            BoneRuntime::Bone => self.global_transform_matrix,
        }
    }

    /// 对齐 TS:174-175 的宿主判定（`isSurface ? (parent as Surface)._bone : null`）：
    /// 非 Surface 恒 `None`；Surface 取载荷里的宿主骨下标（Surface.ts:469-475 解析，
    /// 宿主恒为本骨祖先 → 下标 < 自身）。
    ///
    /// `pub(crate)` 消费方：`resolve_parent_context`（本模块）与
    /// `constraint.rs::resolve_parent_host`（约束链骨的 parent/宿主跨区解析）。
    pub(crate) fn surface_host_idx(&self) -> Option<BoneIdx> {
        if self.bone_type != BoneType::Surface {
            return None; // isSurface=false → surfaceBone 恒 null
        }
        match &self.runtime {
            BoneRuntime::Surface(state) => state.bone, // `(parent as Surface)._bone`
            BoneRuntime::Bone => None, // 防御：runtime 与 bone_type 由 init 同步维护
        }
    }

    /// Surface 载荷只读访问（TS `(bone as Surface)._xxx` 的 Rust 形态）。
    #[must_use]
    pub fn surface_state(&self) -> Option<&SurfaceState> {
        match &self.runtime {
            BoneRuntime::Surface(state) => Some(state),
            BoneRuntime::Bone => None,
        }
    }

    /// Surface 载荷可变访问。
    pub fn surface_state_mut(&mut self) -> Option<&mut SurfaceState> {
        match &mut self.runtime {
            BoneRuntime::Surface(state) => Some(state),
            BoneRuntime::Bone => None,
        }
    }

    /// 对齐 `Bone._onClear()`（Bone.ts:92-107 + super）——arena dispose 路径
    /// （D-3.1：`dispose = on_clear + free.push`，任务 6/7）与池复用语义。
    ///
    /// 除 `idx` 保留（Default = 0，on_clear 不动；见方法体内注释）外，
    /// 复位值与 `_onClear`（= [`Default`]）同值。
    pub fn on_clear(&mut self) {
        self.base = TransformObject::default(); // super._onClear()（TransformObject.ts:100-111）

        self.offset_mode = OffsetMode::Additive; // :95
        self.animation_pose.identity(); // :96

        self.transform_dirty = false; // :98
        self.children_transform_dirty = false; // :99
        self.local_dirty = true; // :100 ★ 默认 true（F3.6）
        self.has_constraint = false; // :101
        self.visible = true; // :102
        self.cached_frame_index = -1; // :103
        self.bone_name = String::new(); // :104 `_boneData = null`（D-3.3 缓存字段方案）
        self.parent = None; // :105
        self.cached_frame_indices = None; // :106

        // Rust 侧（D-3.3）：随 `_boneData = null` 一并复位缓存字段与载荷——
        // bone_type 回 Bone（Surface 载荷整块 drop，对应 Surface.ts:51-64 的
        // `_onClear` 差异段：_dX/_k/… 归零、_bone = null）。idx 保留，
        // 复用方 init 按新骨表长度重写（Slot::on_clear 同款）。
        self.bone_type = BoneType::Bone;
        self.inherit_translation = false;
        self.inherit_rotation = false;
        self.inherit_scale = false;
        self.inherit_reflection = false;
        self.length = 0.0;
        self.runtime = BoneRuntime::Bone;
    }
}

impl std::ops::Deref for Bone {
    type Target = TransformObject;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl std::ops::DerefMut for Bone {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone_data(
        name: &str,
        parent: Option<&str>,
        rotation: f64,
        inherit_rotation: bool,
        inherit_scale: bool,
    ) -> BoneData {
        BoneData {
            inherit_translation: true,
            inherit_rotation,
            inherit_scale,
            inherit_reflection: true,
            name: name.to_string(),
            parent: parent.map(str::to_string),
            transform: Transform::new(0.0, 0.0, 0.0, rotation, 1.0, 1.0),
            ..BoneData::default()
        }
    }

    /// 冒烟：三级骨链 init（按名解析父骨）→ invalid_update（F3.11 工厂收尾同款）→
    /// 按 `split_at_mut` 拓扑序 update（任务 6 的调用形态）。
    ///
    /// 重点验证 TS:201 的**父骨惰性回写**：mid 有父且 inheritScale=true → 自身 update
    /// 末尾 TS:235 置 `_globalDirty`；child 的 inheritRotation=false 分支在 :201 调
    /// `parent.updateGlobalTransform()` 把 mid.global 分解成世界值并清脏——这是
    /// `update` 签名从 `Option<&Bone>` 改为 `ancestors: &mut [Bone]` 的 TS 证据
    ///（Rooster 素材同款链路）。
    #[test]
    fn update_composes_and_lazily_backfills_parent() {
        let mut bones: Vec<Bone> = Vec::new();

        let mut root = Bone::default();
        root.init(
            &bone_data("root", None, 0.5, true, true),
            ArmatureIdx(0),
            &bones,
        );
        bones.push(root);

        let mut mid = Bone::default();
        mid.init(
            &bone_data("mid", Some("root"), 0.3, true, true),
            ArmatureIdx(0),
            &bones,
        );
        bones.push(mid);

        let mut child = Bone::default();
        child.init(
            &bone_data("child", Some("mid"), 0.1, false, true), // inheritRotation=false
            ArmatureIdx(0),
            &bones,
        );
        bones.push(child);

        assert_eq!(bones[1].parent, Some(BoneIdx(0))); // TS:354-356 按名解析
        assert_eq!(bones[2].idx, BoneIdx(2)); // TS:358 → Rust 预置下标

        for bone in &mut bones {
            bone.invalid_update(); // F3.11 工厂收尾 invalid_update(null, true)
        }
        for i in 0..bones.len() {
            // 任务 6 advance_time 的借用形态（F3.2 拓扑序 + 阶段 3 约束三段拆分）
            let (left, right) = bones.split_at_mut(i);
            let (cur, tail) = right.split_first_mut().expect("split_at_mut(i) 后右段非空");
            cur.update(-1, left, tail, &mut [], false, false);
        }

        // root：无父 → TS:301-329，global = origin（Additive/yDown），不置 globalDirty
        assert!((bones[0].global.rotation - 0.5).abs() < 1e-9);
        assert!(!bones[0].global_dirty);

        // mid：有父 → TS:219-235，矩阵 rot(0.3)∘rot(0.5) 后置 globalDirty（global 仍为局部 0.3）
        // ——随后被 child 的 :201 回写分解成世界值 0.8 且清脏（★ 签名偏差的验证点）
        assert!((bones[1].global.rotation - 0.8).abs() < 1e-9);
        assert!(!bones[1].global_dirty);
        assert!((bones[1].global_transform_matrix.a - 0.8f64.cos()).abs() < 1e-9);

        // child：inheritRotation=false → TS:201-216 rotation = 局部 - 父；
        // 矩阵 = rot(0.1-0.8)∘rot(0.8) → 世界旋转 0.1（不继承父旋转的语义）
        assert!((bones[2].global.rotation - (0.1 - 0.8)).abs() < 1e-9);
        assert!((bones[2].global_transform_matrix.a - 0.1f64.cos()).abs() < 1e-9);
    }
}
