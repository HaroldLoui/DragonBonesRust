//! `armature/Armature.ts`（941 行）——骨架运行时容器。
//!
//! 阶段 3：字段 + 查询/注册类小方法，以及任务 6 的逻辑体——`advance_time` 主循环、
//! `invalid_update`、`on_clear`、`contains_point` / `intersects_segment`
//! （均对齐 `armature/Armature.ts` 逐行翻译）。
//!
//! 借用策略（指南任务 6 坑 4）：骨架被 `take` 出 arena 后经 [`crate::armature::ArenaCtx`]
//! 访问**其他**骨架；`advance_time` 内部用字段级解构（`let Self { … } = self`）把
//! bones/slots/constraints/actions/animation/proxy 拆成互不冲突的切片借用。

use super::bone::Bone;
use super::constraint::Constraint;
use super::i_armature_proxy::ArmatureProxy;
use super::slot::{Slot, SlotUpdateCtx};
use super::{ArenaCtx, ArmatureIdx, BoneIdx, DisplayHandle, SlotIdx};
use crate::animation::Animation;
use crate::core::enums::ActionType;
use crate::event::EventObject;
use crate::geom::Point;
use crate::model::{ArmatureData, DragonBonesData};
use serde_json::Value;
use std::sync::Arc;

/// 对齐 `Armature`（Armature.ts:42）。
///
/// 字段默认值逐行对齐 `_onClear`（Armature.ts:122-181，见各字段注释）。
/// TS 的 `_armatureData: ArmatureData` 在 Rust 拆成 `data: Arc<DragonBonesData>` +
/// `armature_name`（D-3.3 名字解析，`armature_data()` 即 `data.armatures[name]`）。
pub struct Armature {
    /// 本骨架在 factory arena 中的索引（D-3.1；`proxy.db_init(idx)` 用）。
    pub idx: ArmatureIdx,
    /// 骨架名（→ `data.armatures[name]`，D-3.3）。
    pub armature_name: String,
    /// 阶段 2 产物根（D-3.3：运行时不借生命周期，`Arc` 持有；data 解析后不可变）。
    pub data: Arc<DragonBonesData>,
    /// 对齐 `inheritAnimation`（Armature.ts:63），`_onClear` = **true**（Armature.ts:155）。
    pub inherit_animation: bool,
    /// 对齐 `userData`（Armature.ts:67），`_onClear` = null（Armature.ts:156）。
    pub user_data: Option<Value>,
    /// 对齐 `_lockUpdate`（Armature.ts:71），`_onClear` = false（:158）。
    pub lock_update: bool,
    /// 对齐 `_slotsDirty`（Armature.ts:72），`_onClear` = **true**（:159）。
    pub slots_dirty: bool,
    /// 对齐 `_zOrderDirty`（Armature.ts:73），`_onClear` = false（:160）。
    pub z_order_dirty: bool,
    /// 对齐 `_zIndexDirty`（Armature.ts:77），`_onClear` = false（:161）。
    pub z_index_dirty: bool,
    /// 对齐 `_alphaDirty`（Armature.ts:81），`_onClear` = **true**（:162）。
    pub alpha_dirty: bool,
    /// 对齐 `_flipX`（Armature.ts:82），`_onClear` = false（:163）；只由 setter 写（:705-715）。
    pub flip_x: bool,
    /// 对齐 `_flipY`（Armature.ts:83），`_onClear` = false（:164）。
    pub flip_y: bool,
    /// 对齐 `_cacheFrameIndex`（Armature.ts:87），`_onClear` = -1（:165）；阶段 3 恒 -1（P1-4 阶段 6）。
    pub cache_frame_index: i32,
    /// 对齐 `_alpha`（Armature.ts:88），`_onClear` = 1.0（:166）。
    pub alpha: f64,
    /// 对齐 `_globalAlpha`（Armature.ts:92），`_onClear` = 1.0（:167）。
    pub global_alpha: f64,
    /// 对齐 `_bones`（Armature.ts:93）——创建序 = `sortedBones` 拓扑序（F3.2）。
    pub bones: Vec<Bone>,
    /// 对齐 `_slots`（Armature.ts:94）——初始 = `sortedSlots` 序，`advanceTime` 里按
    /// `zIndex*1000+zOrder` 稳定排序（F3.10）。
    pub slots: Vec<Slot>,
    /// 对齐 `_constraints`（Armature.ts:98）——阶段 3 收口（F3.1 已证伪「无约束」）：
    /// 龙/Rooster×3 共 4 个 IK 经 `_build_constraints` 登记；Path 素材 0 个。
    pub constraints: Vec<Constraint>,
    /// 对齐 `_actions`（Armature.ts:99）——action 队列（阶段 3 结构保留、恒空）。
    pub actions: Vec<EventObject>,
    /// 对齐 `_animation`（Armature.ts:104，init 时创建）。
    pub animation: Animation,
    /// 对齐 `_proxy`（Armature.ts:105，init 时写入）。
    pub proxy: Box<dyn ArmatureProxy>,
    /// 对齐 `_display`（Armature.ts:106）——子骨架槽显示切换时读
    /// `childArmature.display`（Slot.ts:512）。
    pub display: DisplayHandle,
    /// 对齐 `_parent`（Armature.ts:120，子骨架回指父插槽）——D-3.2 索引解环。
    pub parent: Option<(ArmatureIdx, SlotIdx)>,
    // TODO 阶段 5（P0-12）: `_clock: WorldClock`（Armature.ts:116）、`_dragonBones`（:115）
    // TODO 阶段 6（P1-5）: `_replaceTextureAtlasData`（:110）、`_replacedTexture`（:111）
}

impl Armature {
    /// 构造 + `init`（TS `Armature.init(armatureData, proxy, display, dragonBones)`，
    /// Armature.ts:274-291：写 data/proxy/display、`proxy.dbInit(this)`、
    /// `animation.init(this)`）。
    ///
    /// Rust 无池：构造即初始化（`_armatureData !== null` 幂等 guard 不再需要，
    /// 记 `model_mapping.md` §4 池语义偏差）。
    #[must_use]
    pub fn new(
        idx: ArmatureIdx,
        data: Arc<DragonBonesData>,
        armature_name: impl Into<String>,
        proxy: Box<dyn ArmatureProxy>,
        display: DisplayHandle,
    ) -> Self {
        let armature_name = armature_name.into();
        let mut armature = Self {
            idx,
            data,
            armature_name,
            inherit_animation: true,   // _onClear Armature.ts:155
            user_data: None,           // :156
            lock_update: false,        // :158
            slots_dirty: true,         // :159
            z_order_dirty: false,      // :160
            z_index_dirty: false,      // :161
            alpha_dirty: true,         // :162
            flip_x: false,             // :163
            flip_y: false,             // :164
            cache_frame_index: -1,     // :165
            alpha: 1.0,                // :166
            global_alpha: 1.0,         // :167
            bones: Vec::new(),         // :168
            slots: Vec::new(),         // :169
            constraints: Vec::new(),   // :170
            actions: Vec::new(),       // :171
            animation: Animation::new(), // :173
            proxy,                     // :174
            display,                   // :175
            parent: None,              // :180
        };
        armature.proxy.db_init(idx); // Armature.ts:288
        armature.animation.init(idx); // Armature.ts:289-290（`animations` 数据表挂接阶段 4）
        armature
    }

    /// 对齐 `Armature.armatureData` getter（Armature.ts:806-808）。
    ///
    /// # Panics
    /// `build` 后 `armature_name` 必存在于 `data.armatures`；不存在说明装配时
    /// 数据被换（对应 TS `_armatureData === null` 的已 dispose 警告分支，
    /// Rust 持 `Arc` 不可达，见 `model_mapping.md` §4 偏差）。
    #[must_use]
    pub fn armature_data(&self) -> &ArmatureData {
        self.data
            .armatures
            .get(&self.armature_name)
            .expect("armature data not found (dangling armature_name)")
    }

    /// 对齐 `Armature.name` getter（Armature.ts:791-793）。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.armature_name
    }

    /// 对齐 `Armature.cacheFrameRate` getter（Armature.ts:765-767，读 data）。
    /// TODO 阶段 6（P1-4）：setter（:768-780 `armatureData.cacheFrames` + 子骨架递归）。
    #[must_use]
    pub fn cache_frame_rate(&self) -> f64 {
        self.armature_data().cache_frame_rate
    }

    /// 对齐 `Armature.getBone(name)`（Armature.ts:589，线性遍历按名找）。
    #[must_use]
    pub fn get_bone(&self, bone_name: &str) -> Option<BoneIdx> {
        self.bones
            .iter()
            .position(|b| b.bone_name == bone_name)
            .map(|i| BoneIdx(i as u32))
    }

    /// 对齐 `Armature.getSlot(name)`（Armature.ts:631-639，线性按名找）。
    ///
    /// 返回**创建序身份** `slot.idx`（TS 返回 Slot 对象本身）——不可返回数组位置：
    /// 槽排序（`advance_time` ③）后位置 ≠ 身份（见该处 `SlotIdx` 语义注）。
    #[must_use]
    pub fn get_slot(&self, slot_name: &str) -> Option<SlotIdx> {
        self.slots
            .iter()
            .find(|s| s.slot_name == slot_name)
            .map(|s| s.idx)
    }

    /// 对齐 `Armature.getBones()`（Armature.ts:677-679）。
    #[must_use]
    pub fn get_bones(&self) -> &[Bone] {
        &self.bones
    }

    /// 对齐 `Armature.getSlots()`（Armature.ts:692）。
    #[must_use]
    pub fn get_slots(&self) -> &[Slot] {
        &self.slots
    }

    /// 对齐 `Armature._bufferAction(action, append)`（Armature.ts:235-242）。
    ///
    /// 偏差：TS 按对象身份 `indexOf` 去重；Rust 事件为值语义、每次都是新值，
    /// 直接入队（与 D-4.1 同源）。`append=false` 头插（子骨架动作用，Slot.ts:571）。
    pub fn buffer_action(&mut self, action: EventObject, append: bool) {
        if append {
            self.actions.push(action);
        } else {
            self.actions.insert(0, action);
        }
    }

    /// 对齐 `flipX` setter（Armature.ts:708-715：变化时 `invalidUpdate()`）。
    pub fn set_flip_x(&mut self, value: bool) {
        if self.flip_x != value {
            self.flip_x = value;
            self.invalid_update(None, false);
        }
    }

    /// 对齐 `flipY` setter（Armature.ts:729-736）。
    pub fn set_flip_y(&mut self, value: bool) {
        if self.flip_y != value {
            self.flip_y = value;
            self.invalid_update(None, false);
        }
    }

    // TODO 阶段 4（结构预留，本阶段不实现）：对齐 `Armature._sortZOrder(slotIndices, offset)`
    //（Armature.ts:185-207）——按 `armatureData.sortedSlots` 序重发 zOrder：
    // `isOriginal = slotIndices === null`；`_zOrderDirty || !isOriginal` 时循环取
    // `slotIndex = isOriginal ? i : slotIndices[offset + i]`，下标越界（`<0 || >= l`）
    // **continue 跳过**，命中的槽 `slot._setZOrder(i)`；循环后置 `_slotsDirty = true`、
    // `_zOrderDirty = !isOriginal`。消费方 = ZOrder 时间轴 `TimelineState.ts:80/278/281`
    //（阶段 4 接 TimelineState 时实现）。越界跳过会让个别槽保留旧 zOrder → 届时复核
    // D-4.29 等键可达性（见 `advance_time` ③ 排序处注释）。

    /// 对齐 `Armature._addBone(value)`（Armature.ts:211-215）。
    ///
    /// TS 按对象身份 `indexOf` 去重；Rust 骨骼按创建序推入（`Bone::init` 已预置
    /// `idx = bones.len()`），重名说明数据异常 → `log::warn!` 后忽略本次推入。
    pub fn add_bone(&mut self, bone: Bone) {
        if self.bones.iter().any(|b| b.bone_name == bone.bone_name) {
            log::warn!("duplicate bone `{}` in armature `{}`", bone.bone_name, self.armature_name);
            return;
        }
        self.bones.push(bone);
    }

    /// 对齐 `Armature._addSlot(value)`（Armature.ts:219-223），同上（`Slot::init` 预置 idx）。
    pub fn add_slot(&mut self, slot: Slot) {
        if self.slots.iter().any(|s| s.slot_name == slot.slot_name) {
            log::warn!("duplicate slot `{}` in armature `{}`", slot.slot_name, self.armature_name);
            return;
        }
        self.slots.push(slot);
    }

    /// 对齐 `Armature._addConstraint(value)`（Armature.ts:227-231）。
    pub fn add_constraint(&mut self, constraint: Constraint) {
        if self.constraints.iter().any(|c| c.name == constraint.name) {
            log::warn!(
                "duplicate constraint `{}` in armature `{}`",
                constraint.name,
                self.armature_name
            );
            return;
        }
        self.constraints.push(constraint);
    }

    /// 对齐 `Armature.advanceTime(passedTime)`（`armature/Armature.ts:295-387`）——
    /// 骨架主循环。顺序契约（TS 逐行，笔记 B2「更新顺序契约」）：
    /// 锁 → animation（:313）→ 槽排序（:315-326）→ alpha 传播（:328-339）→
    /// bones（:343-345）→ slots（:347-349）→ actions（:352-383）→ 解锁 + `dbUpdate`（:385-386）。
    ///
    /// 与 TS 的已记偏差：
    /// - `:302/:306` 两个 warn-return 不可达——Rust 持 `Arc<DragonBonesData>`，
    ///   `_armatureData === null` / 数据被 factory.clear 的分支不存在
    ///   （`model_mapping.md` §4 数据销毁语义偏差）→ 不复刻，直接继续。
    /// - `:313` `animation.advanceTime` 阶段 3 为 no-op 占位（阶段 4 接 fade 状态机）。
    /// - `:377-378` 附近非 Play action 的事件派发（bufferEvent）阶段 5——本阶段
    ///   队列恒空，结构保留 + TODO。
    ///
    /// 借用方案：字段级解构（`let Self { … } = self`）后各段只碰拆出的字段；
    /// `cache_frame_rate()` / `idx` 依赖 `&self`（`armature_data()` 读 data），在
    /// 解构前先算成局部量（值在一次 advance 内不变，data 不可变 F3.3）。
    ///
    /// # Panics
    /// - 骨骼父索引越界（`left.get(…)` 失败）：违反 F3.2 sortedBones 拓扑序
    ///   （父索引 < 子索引），属数据装配错误。
    /// - 插槽父骨索引越界（`bones.get(…)` 失败）：`Slot::init` 按名解析必成功
    ///   （Slot.ts:662 "Never;" 分支），越界同属数据错误。
    pub fn advance_time(&mut self, passed_time: f64, world: &mut ArenaCtx<'_>) {
        if self.lock_update {
            return; // :296-298 重入/已 dispose 锁
        }
        self.lock_update = true; // :300 上锁（TS 中 :302/:306 的 warn-return 不解锁——
        // Rust 该两分支不可达，见方法文档偏差）

        let prev_cache_frame_index = self.cache_frame_index; // :311 先存旧缓存帧号
        // 局部量预取：解构后 `self` 不可用（`cache_frame_rate()` 读 `armature_data()`）
        let cache_frame_rate = self.cache_frame_rate();
        let armature_idx = self.idx;

        // 字段级解构借用（指南任务 6 借用策略）：避免 self 整体借用冲突
        let Self {
            lock_update,
            slots_dirty,
            z_index_dirty,
            alpha_dirty,
            alpha,
            global_alpha,
            parent,
            cache_frame_index,
            bones,
            slots,
            constraints,
            actions,
            animation,
            proxy,
            flip_x,
            flip_y,
            ..
        } = self;

        // ---- ② 更新动画（:312-313）----
        // 阶段 3 no-op 占位；TS 内部会改 `_cacheFrameIndex`、可能置
        // `_alphaDirty` / `_zIndexDirty`（阶段 4 接入后顺序不变）。
        animation.advance_time(passed_time);

        // ---- ③ 排序插槽（:315-326）----
        if *slots_dirty || *z_index_dirty {
            // :316 比较器 `_onSortSlots`（Armature.ts:46-48）：`a._zIndex*1000+a._zOrder >
            // b.… ? 1 : -1`——**永不返回 0** 的不一致比较器：等键时返回 -1 而非 0。
            // ES2019 稳定排序的规格保证只覆盖 `compare == 0` 的情形，这里不适用；实测
            //（node，n=5/100/5000 全等键复现）V8 对**等键组实际逆序**。
            // 偏差 D-4.29（记 model_mapping）：Rust 保留**稳定** `sort_by_key`（等键保持
            // 原序）。官方公开路径等键**不可达** → 两侧零行为差：z_order 恒唯一
            //（parser 按槽序 0..n-1，object_data_parser.rs:579-583；本分支 `:318-322`
            // `_setZOrder` 回写 0..n-1；ZOrder 时间轴按 data 序经 `_sortZOrder` 重发，
            // TimelineState.ts:278）。
            // 阶段 5/6 接 `_sortZOrder` 的越界 continue（Armature.ts:192-194）时**复核**
            // ——跳过越界下标会让个别槽保留旧 zOrder，可能与新写回的序号撞键。
            // ★ 同处登记 `SlotIdx` 语义：`Slot.idx` = **创建序身份**（`Slot::init` 预置），
            // 本次重排后数组位置会变、身份不可变（官方对象引用语义）——**任何**重排后
            // 不可把 `SlotIdx` 当位置下标；位置访问必须按 `slot.idx` 扫描（下方 ④ alpha
            // 传播 / ⑦ action 队列即两处按此实现，`get_slot` 同返身份）。build 期
            // `slots[idx]` 因恒创建序而安全（factory 装配段）。
            slots.sort_by_key(|s| i64::from(s.z_index) * 1000 + i64::from(s.z_order));

            if *z_index_dirty {
                // :318-322 zIndex 变化 → 重发 zOrder 序号（`set_z_order` 会置各槽
                // `z_order_dirty`，本帧随后的 slots.update 刷给引擎）
                for (i, slot) in slots.iter_mut().enumerate() {
                    slot.set_z_order(i as i32); // :320
                }
            }

            *slots_dirty = false; // :324
            *z_index_dirty = false; // :325
        }

        // ---- ④ alpha 传播（:328-339）----
        if *alpha_dirty {
            *alpha_dirty = false; // :329
            // :330 子骨架继承**父插槽**的 globalAlpha（不是父骨架，笔记 B3.6）；
            // D-3.2 索引解环——`s` 是创建序身份（SlotIdx）→ 按 `slot.idx` 扫描
            //（见 ③ 处 `SlotIdx` 语义注），不可用 `slots[s.index()]` 位置取；
            // 找不到防御 1.0（父插槽不在 arena / 身份失配），等价于 TS「无父」路径
            //（TS `_parent` 恒非 null，Rust 侧为防御）。
            let parent_slot_alpha = match *parent {
                Some((p, s)) => world
                    .armature(p)
                    .and_then(|a| a.slots.iter().find(|s2| s2.idx == s))
                    .map_or(1.0, |slot| slot.global_alpha),
                None => 1.0,
            };
            *global_alpha = *alpha * parent_slot_alpha; // :330
            let armature_global_alpha = *global_alpha;

            // :332-334 骨骼 alpha——父索引 < 自身（F3.2 拓扑序）→ split_at_mut 左半取父；
            // 骨按创建序更新，父先于子被写入 global_alpha（与 TS 同序）
            for i in 0..bones.len() {
                let (left, right) = bones.split_at_mut(i);
                let cur = &mut right[0];
                let parent_bone = cur.parent.map(|p| {
                    left.get(p.index())
                        .expect("parent bone index out of range (F3.2 sortedBones order violated)")
                });
                cur.update_alpha(parent_bone, armature_global_alpha); // Bone.ts:334-341
            }

            // :336-338 插槽 alpha——父骨不可变借用（⑥ 的可变更新在后面分两阶段，
            // bones/slots 字段分离无冲突）
            for slot in slots.iter_mut() {
                let parent_bone = bones
                    .get(slot.parent.index())
                    .expect("slot parent bone missing (F3.2 / Slot.ts:662 Never branch)");
                slot.update_alpha(parent_bone); // Slot.ts:374-381（可能置 color_dirty）
            }
        }

        // ---- ⑤⑥ 骨骼与插槽更新（:341-350）----
        if *cache_frame_index < 0 || *cache_frame_index != prev_cache_frame_index {
            // :343-345 骨骼——创建序 = sortedBones 拓扑序（F3.2）→ split_at_mut 顺序更新安全。
            // 三段拆分（阶段 3 约束收口）：left = [0,i) 父区、cur = i、tail = (i,n) 后段
            //（约束链骨/target 区，`Bone::update` 与 `constraint.rs` 模块文档借用方案表）
            for i in 0..bones.len() {
                let (left, right) = bones.split_at_mut(i);
                let (cur, tail) = right
                    .split_first_mut()
                    .expect("split_at_mut(i) 后右段非空（i < bones.len()）");
                cur.update(
                    *cache_frame_index,
                    left,
                    tail,
                    constraints,
                    *flip_x,
                    *flip_y,
                );
            }

            // :347-349 插槽——父骨随槽变化 → 每槽构造一次 SlotUpdateCtx
            for slot in slots.iter_mut() {
                let mut slot_ctx = SlotUpdateCtx {
                    parent: bones
                        .get(slot.parent.index())
                        .expect("slot parent bone missing (F3.2 / Slot.ts:662 Never branch)"),
                    bones,
                    actions,
                    armature: armature_idx,
                    // TS 循环内读 `armature.cacheFrameRate`（Slot.ts:552）；data
                    // advance 期间不可变 → 解构前预取等价
                    cache_frame_rate,
                };
                slot.update(*cache_frame_index, &mut slot_ctx, world);
            }
        }

        // ---- ⑦ action 队列（:352-383）----
        if !actions.is_empty() {
            // TODO 阶段 5（P0-12）：非 Play 分支一旦在此派发事件（Frame/Sound），监听器
            // 可能同步 `buffer_action` → 届时把本段改 **drain-后处理**形态（官方 `for..of`
            // 按索引迭代、迭代中 push 的新 action 同轮就被吃到；Rust `actions.iter()`
            // 期间 Vec 被改会 panic）。实测本版官方 `advanceTime` 对非 Play **不派发**、
            // 只 `:379 returnToPool`——Frame/Sound 事件在 TimelineState.ts:51-60 走
            // `_dragonBones.bufferEvent` 直发不经 `_actions`；阶段 3 队列恒空。
            for action in actions.iter() {
                if let Some(action_data) = action.action_data.as_ref() {
                    // :355-376 只处理 Play；非 Play 分支官方无派发（见上 TODO；
                    // 原注「TS :377-378 附近派发」失实，已按实测更正）
                    if action_data.action_type == ActionType::Play {
                        if let Some(slot_idx) = action.slot {
                            // :357-361 动作挂在插槽 → 播该槽的子骨架
                            //（slot_idx = 创建序身份 → 按 `slot.idx` 扫描，见 ③ 语义注）
                            if let Some(child_idx) = slots
                                .iter()
                                .find(|s2| s2.idx == slot_idx)
                                .and_then(|slot| slot.child_armature)
                                && let Some(child_armature) = world.armature_mut(child_idx)
                            {
                                // fadeIn 默认 fadeInTime = -1.0（Animation.ts:568）
                                child_armature.animation.fade_in(&action_data.name, -1.0);
                            }
                        } else if let Some(bone_idx) = action.bone {
                            // :363-372 挂在骨骼 → 该骨名下所有槽的子骨架
                            for slot in slots.iter() {
                                if slot.parent == bone_idx
                                    && let Some(child_idx) = slot.child_armature
                                    && let Some(child_armature) = world.armature_mut(child_idx)
                                {
                                    child_armature.animation.fade_in(&action_data.name, -1.0);
                                }
                            }
                        } else {
                            // :373-374 无目标 → 播自身
                            animation.fade_in(&action_data.name, -1.0);
                        }
                    }
                }
                // :379 `action.returnToPool()`——Rust 无池：action 随迭代结束 drop
                //（D-4.1 池语义偏差；阶段 5 接 TypeId 池后再议）
            }
            actions.clear(); // :382 `actions.length = 0`
        }

        // ---- ⑧ 解锁 + 通知代理（:385-386）----
        *lock_update = false; // :385
        proxy.db_update(); // :386 引擎侧渲染/提交（在解锁之后）
    }

    /// 对齐 `Armature.invalidUpdate(boneName, updateSlot)`（`armature/Armature.ts:406-432`）——
    /// 强制特定骨骼（或全体）及其拥有的插槽在下一帧重算变换/显示。
    ///
    /// 注：`Bone::invalid_update` **只置自身 `transform_dirty`**（Bone.ts:481-483，
    /// bone.rs 已核对）——不沿子骨传播；子骨靠 `Bone::update` 读父骨
    /// `children_transform_dirty` 链在下一帧间接变脏（`docs/guide/03-stage3.md`
    /// 3.3 转述的“沿子传播”与 TS 原文不符，以 TS 为准）。
    pub fn invalid_update(&mut self, bone_name: Option<&str>, update_slot: bool) {
        if let Some(bone_name) = bone_name.filter(|name| !name.is_empty()) {
            // :407 指名分支（`boneName !== null && length > 0`）
            if let Some(bone_idx) = self.get_bone(bone_name) {
                // :408-410
                self.bones[bone_idx.index()].invalid_update();

                if update_slot {
                    // :412-418 只脏该骨直接拥有的槽（`slot.parent === bone`）
                    for slot in self.slots.iter_mut() {
                        if slot.parent == bone_idx {
                            slot.invalid_update();
                        }
                    }
                }
            }
        } else {
            // :421 不指名（None 或空串）→ 全体
            for bone in self.bones.iter_mut() {
                bone.invalid_update(); // :422-424
            }

            if update_slot {
                for slot in self.slots.iter_mut() {
                    slot.invalid_update(); // :427-429
                }
            }
        }
    }

    /// 对齐 `Armature._onClear`（`armature/Armature.ts:122-181`）——arena dispose 路径
    /// （D-3.1：`dispose = on_clear + free.push`，factory 的 `dispose_armature` 调用）。
    ///
    /// 副作用顺序照抄：先逐对象清理（:123-153），再字段复位（:155-180）。
    /// 池语义偏差（D-3.1 / D-3.3，记 `model_mapping.md` §4）：
    /// Bone/Slot 的 `returnToPool` → `on_clear()` 复位后 `Vec::clear`（随 arena
    /// 回收 drop）；Constraint 现为 IK 真身（阶段 3 收口，F3.1 勘误）——`Vec::clear` +
    /// `Default` = 约束 `_onClear` 复位值（Constraint.ts:49-54/:84-91，constraint.rs
    /// `Default` 实现）；EventObject / Animation 的回池 → drop / `reset()`（见各行注释）。
    pub fn on_clear(&mut self) {
        // TODO 阶段 5（P0-12）：`:123-124` clock remove——`_clock` 字段未落地
        // （WorldClock 归阶段 5）。

        for bone in self.bones.iter_mut() {
            bone.on_clear(); // :127-129 `bone.returnToPool()`
        }

        for slot in self.slots.iter_mut() {
            slot.on_clear(); // :131-133 `slot.returnToPool()`（含显示对象销毁分发）
        }

        // :135-137 `constraint.returnToPool()`——Rust 无池：Constraint 随 clear drop
        //（TS `_onClear` 复位字段 → `Default` 同值，constraint.rs 方法文档）
        self.constraints.clear();

        // :139-141 `action.returnToPool()` → drop（D-4.1 池语义偏差）
        self.actions.clear();

        // :143-145 `_animation.returnToPool()` → `Animation._onClear()`
        //（Animation.ts:68-100：回池 states、清 `_animations/_blendStates/
        // _animationConfig`、`timeScale = 1.0`、`_inheritTimeScale = 1.0` 等）——
        // Rust 现用 `reset()`（≈ Animation.ts:280-289，欠清 timeScale /
        // _inheritTimeScale 等）+ `init(idx)` 重挂 armature 回指（TS :173 把
        // `_animation` 置 null，Rust 不可 null；“清空 + 回指”≈ 池清空后再借出的
        // 状态，D-3.1 偏差）。
        // TODO 阶段 4：Animation 补 `on_clear()` 后替换此处调用（对照
        // Animation.ts:68-100）。不改运行时行为。
        self.animation.reset();
        self.animation.init(self.idx);

        self.proxy.db_clear(); // :147-149 代理清场（引擎侧）

        // TODO 阶段 6（P1-5）：`:151-153` `_replaceTextureAtlasData.returnToPool()`
        // ——字段未落地。

        // :155-180 字段复位（逐行对齐 `_onClear` 默认值）
        self.inherit_animation = true; // :155
        self.user_data = None; // :156
        self.lock_update = false; // :158
        self.slots_dirty = true; // :159
        self.z_order_dirty = false; // :160
        self.z_index_dirty = false; // :161
        self.alpha_dirty = true; // :162
        self.flip_x = false; // :163
        self.flip_y = false; // :164
        self.cache_frame_index = -1; // :165
        self.alpha = 1.0; // :166
        self.global_alpha = 1.0; // :167
        self.bones.clear(); // :168（成员已 on_clear；drop = 随 arena 回收）
        self.slots.clear(); // :169
        self.constraints.clear(); // :170（幂等照抄 :135-137 已清）
        self.actions.clear(); // :171（幂等照抄 :139-141 已清）
        // :172 `_armatureData = null`——Rust：`data: Arc` 保活（§4 数据销毁语义
        // 偏差），`armature_name` 清空使 `armature_data()` 不可达（≈ TS null 解
        // 引用崩溃；`Slot::on_clear` 清 `slot_name` 同款处理）。
        self.armature_name.clear();
        // :173 `_animation = null`——见上 :143-145（reset + 回指）。
        // :174 `_proxy = null`——Rust `Box<dyn>` 不可 null：`db_clear()` 已清场，
        // box 保活（D-3.1 偏差；复用走 `Armature::new` 重挂 proxy）。
        self.display = DisplayHandle::Null; // :175
        // TODO 阶段 6（P1-5）：`:176` `_replaceTextureAtlasData = null`、
        // `:177` `_replacedTexture = null`——字段未落地。
        // TODO 阶段 5（P0-12）：`:178` `_dragonBones = null`——字段未落地。
        // TODO 阶段 5（P0-12）：`:179` `_clock = null`——`_clock` 字段未落地
        //（WorldClock 归阶段 5，与 `:123-124` remove TODO 同批）。
        self.parent = None; // :180
    }

    /// 对齐 `Armature.containsPoint(x, y)`（`armature/Armature.ts:451-459`）——
    /// 按当前（已排序）插槽顺序找首个命中自定义包围盒的槽。
    ///
    /// 签名偏差（阶段 3 冻结、暂不改）：TS 返回**命中的 Slot**（Armature.ts:454
    /// `return slot`，:458 无命中 `null`）；Rust 返回 `bool`（命中即 true，:454 的
    /// 命中槽被丢弃）。TODO 阶段 6（P1-3）：实现 BoundingBox 时改签名返回
    /// `Option<SlotIdx>`（登记 TODO，签名现冻结不动）。
    ///
    /// TODO P1-3（阶段 6）：`Slot::contains_point` 内部 BoundingBox 未实现（恒
    /// false）→ 整体暂恒 false；遍历骨架照抄，阶段 6 接 BoundingBox 后无需改本方法。
    #[must_use]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        for slot in &self.slots {
            // :452-453 按当前（已排序）顺序
            if slot.contains_point(x, y) {
                return true; // :454 TS 返回首个命中的 slot；Rust 签名 bool → 命中即 true
            }
        }
        false // :458
    }

    /// 对齐 `Armature.intersectsSegment(…)`（`armature/Armature.ts:490-574`）——
    /// 遍历插槽找线段与自定义包围盒的相交：A 交点取距起点最近、B 交点取最远。
    ///
    /// 签名偏差（骨架既定、任务 6 冻结）：TS 的 3 个可选出参对象
    /// （intersectionPointA/B、normalRadians）在 Rust 并成 8 个 f64——
    /// `(x_c, y_c)` / `(x_d, y_d)` 即 intersectionPointA / B，作为入参下传给槽
    /// （槽命中时覆写，Slot.ts:1004-1026）；normalRadians 无对应参数（阶段 6 扩
    /// 签名时补）。返回值 = TS `:573 return intSlotA` 的坐标化：首个（起点→终点
    /// 方向）命中槽的 A 交点；无命中 `None`。TS `:548-551`「A/B 出参均未提供 →
    /// 首个命中即 break」分支因 Rust 恒“提供”出参而不复刻。
    ///
    /// TODO P1-3（阶段 6）：`Slot::intersects_segment` 内部 BoundingBox 未实现
    ///（恒 None）→ 整体暂恒 None；遍历/累计骨架照抄，阶段 6 无需改本方法结构。
    #[allow(clippy::too_many_arguments)] // TS 形参即 8 个坐标，照抄签名
    #[must_use]
    pub fn intersects_segment(
        &self,
        x_a: f64,
        y_a: f64,
        x_b: f64,
        y_b: f64,
        x_c: f64,
        y_c: f64,
        x_d: f64,
        y_d: f64,
    ) -> Option<(f64, f64)> {
        let is_v = x_a == x_b; // :496 线段是否竖直（A 点距离度量用 y 差）
        let mut d_min = 0.0; // :497
        let mut d_max = 0.0; // :498
        let mut int_x_a = 0.0; // :499
        let mut int_y_a = 0.0; // :500
        let mut int_x_b = 0.0; // :501
        let mut int_y_b = 0.0; // :502
        let mut int_slot_a: Option<usize> = None; // :505（TS 存槽引用；Rust 存下标）
        let mut int_slot_b: Option<usize> = None; // :506

        for (i, slot) in self.slots.iter().enumerate() {
            // :509 TS 把调用方的出参对象透传给每个槽；Rust 用局部 Point 同构
            //（初值来自入参，槽命中时覆写——Slot.ts:1007-1025 写回世界坐标）
            let mut point_a = Point { x: x_c, y: y_c };
            let mut point_b = Point { x: x_d, y: y_d };
            let intersection_count = slot.intersects_segment(
                x_a,
                y_a,
                x_b,
                y_b,
                Some(&mut point_a),
                Some(&mut point_b),
                None, // normalRadians：骨架签名无对应参数（见方法文档偏差）
            );
            if intersection_count.is_some_and(|count| count > 0) {
                // :510

                // :512-527 A 点：距起点距离最小者（竖直线段用 y 差，否则 x 差）
                let mut d = if is_v { point_a.y - y_a } else { point_a.x - x_a };
                if d < 0.0 {
                    d = -d;
                }
                if int_slot_a.is_none() || d < d_min {
                    d_min = d;
                    int_x_a = point_a.x;
                    int_y_a = point_a.y;
                    int_slot_a = Some(i);
                }

                // :530-546 B 点：距起点距离最大者（d 只用 x——原文如此，与 A 不对称）
                let mut d = point_b.x - x_a;
                if d < 0.0 {
                    d = -d;
                }
                if int_slot_b.is_none() || d > d_max {
                    d_max = d;
                    int_x_b = point_b.x;
                    int_y_b = point_b.y;
                    int_slot_b = Some(i);
                }
            }
        }

        // :555-571 TS 把累计值写回 intersectionPointA/B、normalRadians 出参——
        // Rust 返回值只承载 A 交点（B 点累计无出参可写，签名偏差见方法文档），
        // 读一次避免 dead-store 告警。
        let _int_b = (int_slot_b, int_x_b, int_y_b);

        // :573 `return intSlotA` → 首个命中槽的 A 交点
        int_slot_a.map(|_| (int_x_a, int_y_a))
    }
}
