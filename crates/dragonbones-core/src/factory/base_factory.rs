//! `factory/BaseFactory.ts`（1027 行）——任务 7：装配主流程。
//!
//! 阶段 3 无 TS 的 3 个 `protected abstract`（BaseFactory.ts:324-326）：**Null 语义内联**
//! （= `tools/jsdump.mjs` 的 `NullFactory`：proxy 由调用方注入、槽走 [`NullSlotAdapter`]、
//! 图集建空壳），见文件尾 `TODO 阶段7：FactoryAdapter`（D-3.4 第三组契约取舍）。
//!
//! `BaseFactory` 同时充当**运行时世界**（D-3.1：Armature arena、Clock arena 和事件队列）。
//!
//! 主要对齐点（行号基准 = `DragonBonesJS/DragonBones/src/dragonBones/factory/BaseFactory.ts`）：
//! - [`BuildArmaturePackage`]：1017-1027
//! - [`BaseFactory::parse_dragon_bones_data`] / [`BaseFactory::parse_texture_atlas_data`]：353-373 / 402-409
//! - [`BaseFactory::fill_build_armature_package`]：110-166（skin 回退两怪癖）
//! - [`BaseFactory::_build_bones`] / [`BaseFactory::_build_slots`] / [`BaseFactory::_build_constraints`]：168-173 / 177-224 / 226-252
//! - [`BaseFactory::_build_child_armature`] / [`BaseFactory::_get_slot_display`]：254-256 / 258-322
//! - [`BaseFactory::build_armature`]：701-716（F3.11 收尾 `invalidUpdate` + `advanceTime(0.0)`）

use crate::animation::world_clock::{
    AnimatableHandle, ClockArena, ClockError, ClockId, WorldClock, wall_clock_seconds,
};
use crate::armature::{
    Armature, ArmatureIdx, ArmatureProxy, Bone, Constraint, DisplayHandle, NullArmatureProxy,
    NullSlotAdapter, RuntimeCtx, RuntimeCtxError, Slot, SlotIdx,
};
use crate::core::enums::ConstraintType;
use crate::event::{EventDispatcher, EventObject};
use crate::model::{
    ActionData, ArmatureData, DisplayData, DisplayKind, DragonBonesData, SkinData,
    TextureAtlasData, TextureData,
};
use crate::parser::ObjectDataParser;
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// 对齐 `BuildArmaturePackage`（BaseFactory.ts:1017-1027，字段照抄；普通类不进池，
/// `buildArmature` 每次 new——笔记 A.1）。
///
/// Rust 形态与 TS 的差异（均为 D-3.3 名字解析/所有权约束）：
/// - `data` 存 `Arc<DragonBonesData>`（共享、解析后不可变）；
/// - `armature: ArmatureData`（对象引用）→ 存 `armature_name`，经 [`Self::armature`] 惰性解析；
/// - `skin` 存**克隆**：fill ④ 怪癖允许 skin 来自**另一份** DragonBonesData（不改写
///   `dataName`/`data`，笔记 A.3-④），单纯名字引用覆盖不了跨 data 场景。
pub struct BuildArmaturePackage {
    /// TS `dataName`（:1021，初始 `""`；fill ② autoSearch 命中时被改写成实际命中的
    /// key——`_getSlotDisplay` 与递归子骨架都用**改写后**的名字，:130/:138）。
    pub data_name: String,
    /// TS `textureAtlasName`（:1022；fill :139 **原样透传**调用方入参，不做归一——
    /// 空串归一在 caller `buildArmature` 703 完成）。
    pub texture_atlas_name: String,
    /// TS `data: DragonBonesData`（:1023；本文件内未直接读，供子类/诊断——Rust 供
    /// [`Self::armature`] 解析与子骨架递归）。
    pub data: Arc<DragonBonesData>,
    /// TS `armature: ArmatureData`（:1024；`_buildBones/_buildSlots/_buildConstraints`
    /// 全部读 `dataPackage.armature.*`）。
    pub armature_name: String,
    /// TS `skin: SkinData | null`（:1025，初始 null；fill 成功时**通常**非 null——
    /// `armatureData.defaultSkin` 自身为 null 时保持 None → `_buildSlots` 180 早退）。
    pub skin: Option<SkinData>,
}

impl BuildArmaturePackage {
    /// `dataPackage.armature`（TS 对象引用 → Rust 名字解析，D-3.3）。
    ///
    /// # Panics
    /// fill 已验证 `armature_name` 必存在于 `data.armatures`（data 解析后不可变，
    /// F3.3）；不存在只可能是 fill 与本结构被外部拼装错误。
    #[must_use]
    pub fn armature(&self) -> &ArmatureData {
        self.data
            .armatures
            .get(&self.armature_name)
            .expect("BuildArmaturePackage.armature_name not in data.armatures (fill guard)")
    }
}

/// 对齐 `BaseFactory`（BaseFactory.ts:46）——阶段 3 直接落地为可实例化结构
/// （TS 是 abstract + 3 抽象方法；Null 语义内联见文件尾 TODO 阶段7）。
///
/// 字段照抄 TS:52-57（见各字段注释）；`BaseFactory` 同时是 arena 运行时世界（D-3.1）。
pub struct BaseFactory {
    /// 对齐 `autoSearch`（TS:52，`@private`，默认 false）——fill ②/④ 与
    /// `_getTextureData` ② 段的回退总闸。
    pub auto_search: bool,
    /// 对齐 `_dragonBonesDataMap`（TS:54）——`dragonBonesName → 阶段2产物`（D-3.3 Arc 共享）。
    /// 用 `IndexMap` 保插入序 = JS `for-in` 的 V8 字符串键序（笔记 A.3-②：autoSearch
    /// 首命中决定 dataName 改写，序不同会得到不同的骨架/皮肤）。
    pub data_map: IndexMap<String, Arc<DragonBonesData>>,
    /// 对齐 `_textureAtlasDataMap`（TS:55）——**value 是数组**（笔记 A.2-4：同名可挂
    /// 多个 atlas，`_getTextureData` 86-91 逐个遍历）→ `IndexMap<String, Vec<TextureAtlasData>>`。
    pub atlas_map: IndexMap<String, Vec<TextureAtlasData>>,
    /// 骨架 arena（D-3.1）：`None` = 已 dispose 的空位。
    pub armatures: Vec<Option<Armature>>,
    /// 空位复用栈（D-3.1 池语义的 arena 版）：`dispose_armature` push、
    /// [`BaseFactory::alloc_armature_slot`] 挑 `> min` 的取出。
    pub free: Vec<u32>,
    /// Runtime Clock arena, uniquely owned beside the armature arena.
    clocks: ClockArena,
    /// Default/root clock corresponding to `DragonBones.clock`.
    root_clock: ClockId,
    /// Events waiting for the next runtime dispatch (JS `_events`, FIFO).
    pending_events: VecDeque<EventObject>,
    /// Objects to recycle at the start of the next global advance (JS `_objects`).
    pending_recycles: Vec<PendingRecycle>,
    /// Global event manager (JS `_eventManager`); SOUND double-dispatch target.
    event_manager: Option<Box<dyn EventDispatcher>>,
    /// 对齐 `_dataParser`（TS:57；TS 构造可注入 DataParser，Rust 恒 `ObjectDataParser`）。
    /// TS 的 `static _objectParser/_binaryParser` 惰性单例（:47-48）→ Rust 每工厂一个
    /// 实例（笔记 A.2-1 允许；JS 共享语义在单工厂场景等价）。
    pub parser: ObjectDataParser,
    // TODO 阶段 6（P1-5）: `BinaryDataParser`（TS:354 `rawData instanceof ArrayBuffer`
    //   强制走二进制 parser）——Rust 侧无此实现（阶段 2 只做 JSON 路径）。
}

/// Deferred recyclables, mirroring JS `DragonBones._objects` (`bufferObject`).
///
/// Armature disposal is the only deferred object in stage 5; `EventObject`
/// instances are dropped after their dispatch call stack ends (D-5.3).
pub(crate) enum PendingRecycle {
    Armature(ArmatureIdx),
}

/// One prevalidated Armature clock move used to make tree updates atomic.
#[derive(Debug, Clone, Copy)]
struct ArmatureClockMove {
    armature: ArmatureIdx,
    from: Option<ClockId>,
    to: Option<ClockId>,
}

impl Default for BaseFactory {
    fn default() -> Self {
        let mut clocks = ClockArena::new();
        let root_clock = clocks.insert(WorldClock::new(0.0));

        Self {
            auto_search: false,
            data_map: IndexMap::new(),
            atlas_map: IndexMap::new(),
            armatures: Vec::new(),
            free: Vec::new(),
            clocks,
            root_clock,
            pending_events: VecDeque::new(),
            pending_recycles: Vec::new(),
            event_manager: None,
            parser: ObjectDataParser::default(),
        }
    }
}

impl BaseFactory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Default WorldClock corresponding to `DragonBones.clock`.
    #[must_use]
    pub fn root_clock(&self) -> ClockId {
        self.root_clock
    }

    /// Creates an unattached custom clock.
    pub fn create_clock(&mut self, time: f64) -> ClockId {
        self.clocks.insert(WorldClock::new(time))
    }

    /// Looks up a clock by its arena handle.
    #[must_use]
    pub fn clock(&self, id: ClockId) -> Option<&WorldClock> {
        self.clocks.get(id)
    }

    /// Mutates public clock controls such as `time_scale` without exposing its relationships.
    pub fn clock_mut(&mut self, id: ClockId) -> Option<&mut WorldClock> {
        self.clocks.get_mut(id)
    }

    /// Attaches an Armature and currently displayed descendants to a clock.
    /// Newly built Armatures remain unattached until this is called.
    pub fn attach_armature(
        &mut self,
        clock: ClockId,
        armature: ArmatureIdx,
    ) -> Result<(), RuntimeCtxError> {
        self.set_armature_clock_tree(armature, Some(clock))
    }

    /// Detaches an Armature and currently displayed descendants from their clocks.
    pub fn detach_armature(&mut self, armature: ArmatureIdx) -> Result<(), RuntimeCtxError> {
        self.set_armature_clock_tree(armature, None)
    }

    /// Attaches a child clock to a parent clock. Cycles are rejected.
    pub fn attach_clock(&mut self, parent: ClockId, child: ClockId) -> Result<(), ClockError> {
        self.clocks.attach_clock(parent, child)
    }

    /// Detaches a child clock from its parent, if attached.
    pub fn detach_clock(&mut self, child: ClockId) -> Result<bool, ClockError> {
        self.clocks.detach_clock(child)
    }

    /// Clears direct Clock members and synchronizes all back-references.
    ///
    /// Collects and validates the whole direct-member set (including every
    /// displayed descendant tree) before clearing anything, so an inconsistent
    /// relationship cannot leave a partially cleared clock.
    pub fn clear_clock(&mut self, clock: ClockId) -> Result<(), RuntimeCtxError> {
        let members = self
            .clocks
            .get(clock)
            .ok_or(ClockError::InvalidClockId)?
            .active_members();

        // Validate direct back-references before changing any relationships.
        for handle in &members {
            let is_consistent = match *handle {
                AnimatableHandle::Armature(id) => self
                    .armatures
                    .get(id.index())
                    .and_then(Option::as_ref)
                    .is_some_and(|armature| armature.clock() == Some(clock)),
                AnimatableHandle::Clock(id) => self.clocks.parent(id) == Some(clock),
            };
            if !is_consistent {
                return Err(ClockError::InconsistentRelationship.into());
            }
        }

        // Build one plan for all Armature trees first; `visited` keeps a shared
        // descendant from being planned twice.
        let mut visited = HashSet::new();
        let mut plan = Vec::new();
        let mut direct_clocks = Vec::new();
        for handle in &members {
            match *handle {
                AnimatableHandle::Armature(id) => {
                    plan.extend(self.collect_armature_clock_plan(id, None, &mut visited)?);
                }
                AnimatableHandle::Clock(id) => direct_clocks.push(id),
            }
        }
        self.validate_armature_clock_plan(&plan)?;
        self.apply_armature_clock_plan(&plan)?;

        for id in direct_clocks {
            // A displayed child armature may already have detached this clock.
            let detached = self.clocks.detach_clock(id)?;
            debug_assert!(detached, "direct clock member was validated");
        }

        let remaining = self
            .clocks
            .clear_clock(clock)
            .map_err(RuntimeCtxError::Clock)?;
        debug_assert!(
            remaining.is_empty(),
            "all direct members were detached above"
        );
        Ok(())
    }

    /// Checks direct and transitive membership by following owner back-references.
    #[must_use]
    pub fn clock_contains(&self, clock: ClockId, value: AnimatableHandle) -> bool {
        if self.clocks.get(clock).is_none() || value == AnimatableHandle::Clock(clock) {
            return false;
        }

        let mut ancestor = match value {
            AnimatableHandle::Armature(id) => self
                .armatures
                .get(id.index())
                .and_then(Option::as_ref)
                .and_then(Armature::clock),
            AnimatableHandle::Clock(id) => self.clocks.parent(id),
        };
        let mut visited = HashSet::new();
        while let Some(id) = ancestor {
            if id == clock {
                return true;
            }
            if !visited.insert(id) {
                return false;
            }
            ancestor = self.clocks.parent(id);
        }
        false
    }

    /// Moves a whole displayed-Armature tree to `new_clock` atomically.
    ///
    /// The official setter (`Armature.ts:904-910`) mutates node by node and can
    /// leave a partially updated tree if a later relationship is inconsistent.
    /// Rust first collects the exact move list (preserving the TS early-out:
    /// a node already on `new_clock` does not recurse into its children), then
    /// validates every membership/back-reference and the target clocks' token
    /// capacity before mutating anything. Execution cannot fail after that
    /// validation; if it ever does, already-applied moves are compensated in
    /// reverse order.
    fn set_armature_clock_tree(
        &mut self,
        root: ArmatureIdx,
        new_clock: Option<ClockId>,
    ) -> Result<(), RuntimeCtxError> {
        let current_clock = self
            .armatures
            .get(root.index())
            .and_then(Option::as_ref)
            .ok_or(RuntimeCtxError::InvalidArmature)?
            .clock();

        // The official clock setter returns immediately when the value is unchanged.
        if current_clock == new_clock {
            return self.move_armature_clock(root, new_clock);
        }

        let mut visited = HashSet::new();
        let plan = self.collect_armature_clock_plan(root, new_clock, &mut visited)?;
        self.validate_armature_clock_plan(&plan)?;
        self.apply_armature_clock_plan(&plan)
    }

    /// Builds the ordered move list for [`Self::set_armature_clock_tree`].
    ///
    /// Uses the same stack/child order as the TS setter so the resulting
    /// membership order is unchanged; no arena state is mutated.
    fn collect_armature_clock_plan(
        &self,
        root: ArmatureIdx,
        new_clock: Option<ClockId>,
        visited: &mut HashSet<ArmatureIdx>,
    ) -> Result<Vec<ArmatureClockMove>, RuntimeCtxError> {
        let mut plan = Vec::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }

            let armature = self
                .armatures
                .get(id.index())
                .and_then(Option::as_ref)
                .ok_or(RuntimeCtxError::InvalidArmature)?;
            let from = armature.clock();

            if from == new_clock {
                plan.push(ArmatureClockMove {
                    armature: id,
                    from,
                    to: new_clock,
                });
                continue; // TS setter early-outs and does not recurse through this node.
            }

            let children: Vec<_> = armature
                .slots
                .iter()
                .rev()
                .filter_map(|slot| slot.child_armature)
                .collect();
            plan.push(ArmatureClockMove {
                armature: id,
                from,
                to: new_clock,
            });
            pending.extend(children);
        }
        Ok(plan)
    }

    /// Validates every planned move against the current arena before any write:
    /// the back-reference must equal the recorded `from`, the old clock must
    /// own the member, the target clock must exist and not already own it, and
    /// each target clock must be able to allocate the required member tokens.
    fn validate_armature_clock_plan(
        &self,
        plan: &[ArmatureClockMove],
    ) -> Result<(), RuntimeCtxError> {
        let mut token_demand: HashMap<ClockId, u64> = HashMap::new();

        for planned in plan {
            let armature = self
                .armatures
                .get(planned.armature.index())
                .and_then(Option::as_ref)
                .ok_or(RuntimeCtxError::InvalidArmature)?;
            if armature.clock() != planned.from {
                return Err(ClockError::InconsistentRelationship.into());
            }

            if let Some(old_clock) = planned.from {
                let old = self
                    .clocks
                    .get(old_clock)
                    .ok_or(ClockError::InvalidClockId)?;
                if !old.contains_member(AnimatableHandle::Armature(planned.armature)) {
                    return Err(ClockError::InconsistentRelationship.into());
                }
            }

            if planned.from != planned.to
                && let Some(new_clock) = planned.to
            {
                let new = self
                    .clocks
                    .get(new_clock)
                    .ok_or(ClockError::InvalidClockId)?;
                if new.contains_member(AnimatableHandle::Armature(planned.armature)) {
                    return Err(ClockError::InconsistentRelationship.into());
                }
                *token_demand.entry(new_clock).or_insert(0) += 1;
            }
        }

        for (clock, demand) in token_demand {
            let target = self.clocks.get(clock).ok_or(ClockError::InvalidClockId)?;
            if !target.can_attach_count(demand) {
                return Err(ClockError::MemberTokenExhausted.into());
            }
        }

        Ok(())
    }

    /// Applies a validated plan. A failure here would mean the arena changed
    /// between validation and execution; compensate the moves already applied
    /// so callers never observe a half-updated tree.
    fn apply_armature_clock_plan(
        &mut self,
        plan: &[ArmatureClockMove],
    ) -> Result<(), RuntimeCtxError> {
        for (applied, planned) in plan.iter().enumerate() {
            if let Err(error) = self.move_armature_clock(planned.armature, planned.to) {
                log::error!("clock relationship update failed after prevalidation: {error}");
                for rollback in plan[..applied].iter().rev() {
                    if let Err(rollback_error) =
                        self.move_armature_clock(rollback.armature, rollback.from)
                    {
                        log::error!("clock relationship rollback failed: {rollback_error}");
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }

    fn move_armature_clock(
        &mut self,
        armature: ArmatureIdx,
        new_clock: Option<ClockId>,
    ) -> Result<(), RuntimeCtxError> {
        let current_clock = self
            .armatures
            .get(armature.index())
            .and_then(Option::as_ref)
            .ok_or(RuntimeCtxError::InvalidArmature)?
            .clock();
        let mut runtime = RuntimeCtx::new(
            &mut self.armatures,
            &mut self.clocks,
            &mut self.pending_events,
            None,
            current_clock,
        );
        runtime.move_armature_clock(armature, new_clock)
    }

    // ------------------------------------------------------------------
    // parse / 数据缓存（TS:353-373 / 402-409 / 481-493 / 570-576）
    // ------------------------------------------------------------------

    /// 对齐 `parseDragonBonesData(rawData, name, scale = 1.0)`（BaseFactory.ts:353-373）。
    ///
    /// - `tools/jsdump.mjs:96` 调用形态：`factory.parseDragonBonesData(JSON.parse(ske), key)`
    ///   ——`name` 即缓存 key（ske/tex 共用同一 key，jsdump:91）；`scale` 取 TS 默认 1.0。
    /// - TS:354 按 `instanceof ArrayBuffer` 选 binary parser → Rust 无二进制路径（字段 TODO）。
    /// - TS:357-366 内嵌图集循环：`_buildTextureAtlasData(null,null)` + `parseTextureAtlasData(null, …)`
    ///   ——Rust 经 [`ObjectDataParser`] 的 `raw_texture_atlases` 机制等价（raw 传 `Value::Null`），
    ///   false（无内嵌图集）即 break。
    /// - `name` 归一：TS `name = null → data.name`（:482）；Rust 入参是 `&str` 无法表 null →
    ///   **空串视作“未指定”**落 `data.name`（记偏差；jsdump 恒传非空 key）。
    ///
    /// 返回 map 中实际缓存的实例（同名冲突时是已存在的旧实例，见 [`Self::add_dragon_bones_data`]）。
    pub fn parse_dragon_bones_data(
        &mut self,
        raw: &Value,
        name: &str,
    ) -> Option<Arc<DragonBonesData>> {
        let scale = 1.0; // TS:353 scale 参数默认 1.0
        let dragon_bones_data = self.parser.parse_dragon_bones_data(raw, scale)?;

        // TS:357-366 内嵌图集 while 循环
        loop {
            let mut texture_atlas_data = TextureAtlasData::default(); // TS:358 `_buildTextureAtlasData(null, null)`
            if self
                .parser
                .parse_texture_atlas_data(&Value::Null, &mut texture_atlas_data, scale)
            {
                // TS:359-360 成功 → addTextureAtlasData（同 name 缓存）
                self.add_texture_atlas_data(texture_atlas_data, (!name.is_empty()).then_some(name));
            } else {
                break; // TS:363 returnToPool + break（Rust：空壳直接丢弃）
            }
        }

        // TS:368-370 非 null 才入 map；TS:372 返回实例
        Some(self.add_dragon_bones_data(dragon_bones_data, (!name.is_empty()).then_some(name)))
    }

    /// 对齐 `BaseFactory.parseDragonBonesData` 的 `ArrayBuffer` 分支（TS:354 → `BinaryDataParser`）。
    ///
    /// dbbin 头部不含 `textureAtlas`，因此不做 JSON 路径的内嵌图集 while 循环；
    /// 图集仍由调用方用 `parseTextureAtlasData` 单独提供。
    pub fn parse_dragon_bones_data_binary(
        &mut self,
        bytes: &[u8],
        name: &str,
    ) -> Option<Arc<DragonBonesData>> {
        let scale = 1.0;
        let dragon_bones_data = self.parser.parse_binary_dragon_bones_data(bytes, scale)?;
        Some(self.add_dragon_bones_data(dragon_bones_data, (!name.is_empty()).then_some(name)))
    }

    /// 对齐 `parseTextureAtlasData(rawData, textureAtlas, name, scale = 1.0)`（BaseFactory.ts:402-409）。
    ///
    /// - `tools/jsdump.mjs:97` 调用形态：`factory.parseTextureAtlasData(JSON.parse(tex), null, key)`
    ///   ——`textureAtlas`（渲染贴图对象）在 core 无对应物 → 不收参数（TS:405 的
    ///   `_buildTextureAtlasData(data, texture)` 绑定渲染资源是阶段 7 适配层的事，见文件尾 TODO）。
    /// - 返回 `bool`：解析成功与否（TS 返回 atlas 实例、忽略 parse 结果；Rust 按
    ///   [`ObjectDataParser::parse_texture_atlas_data`] 的 bool 透传——`false` = raw 为空且
    ///   无内嵌图集，此时仍按 TS:406 无条件入 map（空壳条目，查询行为与缺席等价）。
    pub fn parse_texture_atlas_data(&mut self, raw: &Value, name: &str) -> bool {
        let scale = 1.0; // TS:402 scale 参数默认 1.0
        let mut texture_atlas_data = TextureAtlasData::default(); // TS:403 `_buildTextureAtlasData(null, null)`
        let parsed = self
            .parser
            .parse_texture_atlas_data(raw, &mut texture_atlas_data, scale);
        // TS:405 `_buildTextureAtlasData(textureAtlasData, textureAtlas || null)`——
        // 渲染资源绑定点，core 无贴图对象 → no-op（阶段 7 FactoryAdapter 接）
        self.add_texture_atlas_data(texture_atlas_data, (!name.is_empty()).then_some(name)); // TS:406
        parsed
    }

    /// 对齐 `addDragonBonesData(data, name = null)`（BaseFactory.ts:481-493）。
    ///
    /// - TS:482 `name = name ?? data.name`（Rust：`name = None` → `data.name`；
    ///   parse 包装层把空串归一成 None，见 [`Self::parse_dragon_bones_data`]）。
    /// - TS:484 同名且**同一对象**→静默 return；:488 同名不同对象 → `warn` 且**不覆盖**
    ///   （Rust 值语义无对象身份 → 内容相等视作“同对象”静默；不等则 warn 保留旧的）。
    ///
    /// 返回 map 中该 key 实际持有的实例。
    pub fn add_dragon_bones_data(
        &mut self,
        data: DragonBonesData,
        name: Option<&str>,
    ) -> Arc<DragonBonesData> {
        let key = name.unwrap_or(&data.name).to_string(); // :482
        if let Some(existing) = self.data_map.get(&key) {
            if **existing == data {
                return Arc::clone(existing); // :484-486 同名同实例 → 静默
            }
            log::warn!("Can not add same name data: {}", key); // :488（不覆盖）
            return Arc::clone(existing);
        }
        let stored = Arc::new(data);
        self.data_map.insert(key, Arc::clone(&stored)); // :492
        stored
    }

    /// 对齐 `addTextureAtlasData(data, name = null)`（BaseFactory.ts:570-576）。
    ///
    /// - TS:571 `name = name ?? data.name`；:572 名字不在 map 时先建空数组（**同名多 atlas
    ///   允许**——`_getTextureData` 逐个遍历，笔记 A.2-4）；:573 `indexOf < 0` 才 push
    ///   （按对象身份判重 → Rust 值等价：同内容重复 push 无行为差异，判重更严，记偏差）。
    pub fn add_texture_atlas_data(&mut self, data: TextureAtlasData, name: Option<&str>) {
        let key = name.unwrap_or(&data.name).to_string(); // :571
        let texture_atlas_list = self.atlas_map.entry(key).or_default(); // :572（等价“无则建空表”）
        if !texture_atlas_list.contains(&data) {
            // :573 indexOf < 0（身份 → 值，见方法文档）
            texture_atlas_list.push(data); // :574
        }
    }

    // ------------------------------------------------------------------
    // _isSupportMesh / _getTextureData（TS:80-108）
    // ------------------------------------------------------------------

    /// 对齐 `_isSupportMesh()`（BaseFactory.ts:80-82，恒 true）——Mesh 显示回退闸
    /// （`_getSlotDisplay` 278-283：false 时 mesh 退回 rawDisplay）。
    /// TODO 阶段7（FactoryAdapter）：作为适配层可覆写的 hook（文件尾）。
    fn is_support_mesh(&self) -> bool {
        true
    }

    /// 对齐 `_getTextureData(textureAtlasName, textureName)`（BaseFactory.ts:84-108）。
    ///
    /// 两段式：① 指定名字下的**所有** atlas 逐个 `getTexture`（:85-92，同名多 atlas）；
    /// ② `factory.autoSearch` 为 true 时遍历**全表**且只看 `autoSearch === true` 的
    /// atlas（:94-105）。返回克隆（D-3.3 值语义——`Slot::replace_texture_data` 收值）。
    /// miss 返回 `None` 是合法结果（TS:210 会把 null 写进 frame.textureData）。
    fn _get_texture_data(
        &self,
        texture_atlas_name: &str,
        texture_name: &str,
    ) -> Option<TextureData> {
        // ① :85-92
        if let Some(texture_atlas_list) = self.atlas_map.get(texture_atlas_name) {
            for texture_atlas_data in texture_atlas_list {
                if let Some(texture_data) = texture_atlas_data.get_texture(texture_name) {
                    return Some(texture_data.clone());
                }
            }
        }

        // ② :94-105（注释原文：Will be search all data, if the autoSearch is true.）
        if self.auto_search {
            for texture_atlas_list in self.atlas_map.values() {
                for texture_atlas_data in texture_atlas_list {
                    if texture_atlas_data.auto_search
                        && let Some(texture_data) = texture_atlas_data.get_texture(texture_name)
                    {
                        return Some(texture_data.clone());
                    }
                }
            }
        }

        None // :107
    }

    /// [`Slot::new`] 的 `texture_atlas_scale` 入参：TS 按当前纹理所属图集的对象引用取
    /// `_textureData.parent.scale`（逐纹理，Slot.ts:412，喂给 pivot 缩放
    /// `_pivotX/Y`），Rust `Slot` 只有单一缓存值（`slot.rs` 字段文档既定：
    /// “一次 build 只用一个图集”）→ 按贴图解析的同序
    /// （atlasName → dataName，与 `_build_slots` 的 texture 通道一致）取首个命中图集的
    /// scale；都未命中 → 1.0（`TextureAtlasData` 默认 scale）。
    ///
    /// **pivot scale 场景（不改行为，同款标注见 `slot.rs::texture_atlas_scale` 字段
    /// 文档的三场景 + 阶段 6 修法）**：`first()` 取的是“同名列表首个”，而实际贴图可能
    /// 来自别的 atlas——① `_getTextureData` ② 段（autoSearch 全表扫，:94-105）命中
    /// 别的 data 的 atlas；② 同名多 atlas 下命中第 2+ 个不同 scale 的 atlas；
    /// ③ 解析失败空壳（:406 无条件入 map，scale=1.0）占列表首位。三处都会让本函数
    /// 返回的 scale ≠ `_textureData.parent.scale`，pivot 随之取错——阶段 6 随
    /// `replace_texture_data` 按帧记 scale 收口。
    fn texture_atlas_scale(&self, texture_atlas_name: &str, data_name: &str) -> f64 {
        for key in [texture_atlas_name, data_name] {
            if let Some(texture_atlas_list) = self.atlas_map.get(key)
                && let Some(texture_atlas_data) = texture_atlas_list.first()
            {
                return texture_atlas_data.scale;
            }
        }
        1.0
    }

    // ------------------------------------------------------------------
    // fill（TS:110-166）
    // ------------------------------------------------------------------

    /// TS:114-135 的 ①② 段：定位骨架所在的 DragonBonesData（含 autoSearch 回退两道闸）。
    ///
    /// - **① 117-122 精确命中**：仅当 `dragonBonesName` 非空 **且** key 存在时才查
    ///   `getArmature`；data 命中但骨架 miss → `armatureData` 仍 null（② 闸只看
    ///   armature 是否命中，不看 data 是否取到）。
    /// - **② 124-135 autoSearch 回退**：外层闸 `armatureData === null && (名字空 || factory.autoSearch)`；
    ///   内层闸（127）`名字空 || data 自身 autoSearch`。名字空 → 扫**全部** data；
    ///   名字非空 → 只扫标记了 autoSearch 的 data。命中即 `dragonBonesName = k`
    ///   **改写 dataName**（130）并 break——for-in 遍历序 = 插入序（V8 字符串键），
    ///   `IndexMap` 保序等价。
    ///
    /// 返回 `(实际命中的 data, 改写后的 dataName)`；成功时保证 `armature_name` 在该 data 中。
    fn resolve_dragon_bones_data(
        &self,
        dragon_bones_name: &str,
        armature_name: &str,
    ) -> Option<(&Arc<DragonBonesData>, String)> {
        // TS:114-115
        let mut data: Option<&Arc<DragonBonesData>> = None;
        let mut data_name: Option<String> = None;
        let mut armature_found = false;

        // ① TS:117-122
        if !dragon_bones_name.is_empty()
            && let Some(dragon_bones_data) = self.data_map.get(dragon_bones_name)
        {
            data = Some(dragon_bones_data); // :119
            data_name = Some(dragon_bones_name.to_string());
            if dragon_bones_data.get_armature(armature_name).is_some() {
                // :120
                armature_found = true; // :120-121（等价 `armatureData !== null`）
            }
        }

        // ② TS:124-135 autoSearch 回退（两道闸，见方法文档）
        if !armature_found && (dragon_bones_name.is_empty() || self.auto_search) {
            for (k, candidate) in &self.data_map {
                // TS:126 `dragonBonesData = map[k]`（循环内无条件改写局部变量；
                // 仅当最终未命中时该残留值被 137 挡住不用 → Rust 省略）
                if dragon_bones_name.is_empty() || candidate.auto_search {
                    // :127 内层闸
                    if candidate.get_armature(armature_name).is_some() {
                        // :128
                        data = Some(candidate);
                        data_name = Some(k.clone()); // :130 `dragonBonesName = k` 改写 dataName
                        armature_found = true;
                        break; // :131
                    }
                }
            }
        }

        // TS:137 `armatureData !== null` 才继续；否则 :165 return false
        if !armature_found {
            return None;
        }
        Some((
            data.expect("armature_found ⟹ data 已填（①/② 段）"),
            data_name.expect("armature_found ⟹ dataName 已填"),
        ))
    }

    /// 对齐 `_fillBuildArmaturePackage(...)`（BaseFactory.ts:110-166）。
    ///
    /// ①② 段见 [`Self::resolve_dragon_bones_data`]；此处 ③④：
    /// - **③ 137-142**：填 4 字段 + `skin` 先置 null。
    /// - **④ 144-156 skin 解析（怪癖，逐行）**：
    ///   - :145 按 **skin 名**在本 armature 的 skins 表查（`getSkin`）；
    ///   - :146-155 **skin 缺失且 `factory.autoSearch` 时**：把 `skinName` 当作**骨架名**
    ///     在所有 DragonBonesData 里找 `getArmature(skinName)`，取那份骨架的
    ///     **defaultSkin**（不是同名 skin！）——`skinName` 允许填另一个骨架名共享皮肤
    ///     （TS:672 文档语义）。附带后果：此分支**不改写 dataName/data**（data 与 skin
    ///     可能来自不同 DragonBonesData）；找到骨架即 break（即使其 defaultSkin 为 null）；
    ///     内层**无** autoSearch 闸（只有外层 `this.autoSearch`）。
    /// - **158-160**：仍 null → `armatureData.defaultSkin`（可能仍是 null → `_buildSlots` 180 早退）。
    /// - 返回 false → caller `buildArmature` 704 warn + null（Rust `None`）。
    fn fill_build_armature_package(
        &self,
        dragon_bones_name: &str,
        armature_name: &str,
        skin_name: &str,
        texture_atlas_name: &str,
    ) -> Option<BuildArmaturePackage> {
        let (data, data_name) = self.resolve_dragon_bones_data(dragon_bones_name, armature_name)?;
        let armature_data = data.armatures.get(armature_name).expect("resolve guard"); // TS:141

        // TS:142 `dataPackage.skin = null`
        let mut skin: Option<SkinData> = None;

        // ④ TS:144-156
        if !skin_name.is_empty() {
            skin = armature_data.get_skin(skin_name).cloned(); // :145
            if skin.is_none() && self.auto_search {
                // :146（仅 skin 仍 null 时回退）
                for candidate in self.data_map.values() {
                    // :147 全表扫（内层无 autoSearch 闸）
                    if let Some(skin_armature_data) = candidate.get_armature(skin_name) {
                        // :149 把 skinName 当骨架名
                        // :151 取那份骨架的 defaultSkin（可为 null → 保持 None 继续走 158）
                        skin = skin_armature_data
                            .default_skin
                            .as_deref()
                            .and_then(|default_name| skin_armature_data.get_skin(default_name))
                            .cloned();
                        break; // :152 找到骨架即 break
                    }
                }
            }
        }

        if skin.is_none() {
            // TS:158-160 → armatureData.defaultSkin
            skin = armature_data
                .default_skin
                .as_deref()
                .and_then(|default_name| armature_data.get_skin(default_name))
                .cloned();
        }

        Some(BuildArmaturePackage {
            data_name,                                          // TS:138
            texture_atlas_name: texture_atlas_name.to_string(), // TS:139 原样透传
            data: Arc::clone(data),                             // TS:140（Rust Arc，D-3.3）
            armature_name: armature_name.to_string(),           // TS:141
            skin,                                               // TS:142 + 144-160
        })
    }

    /// 对齐 `getArmatureData(name, dragonBonesName = "")`（BaseFactory.ts:627-634）——
    /// 内部复用 fill 的 ①② 段（TS:629 传 skinName=""/atlas=""，skin 解析结果被丢弃）。
    pub fn get_armature_data(&self, name: &str, dragon_bones_name: &str) -> Option<&ArmatureData> {
        let (data, _data_name) = self.resolve_dragon_bones_data(dragon_bones_name, name)?;
        data.armatures.get(name)
    }

    // ------------------------------------------------------------------
    // arena（D-3.1 / D-3.2）
    // ------------------------------------------------------------------

    /// 空位分配（D-3.1 池语义的 arena 版 + D-3.2 子>父不变量）。
    ///
    /// 规则：在 `free` 里挑**第一个 `> min_exclusive`** 的空位（取出时保持其余元素顺序），
    /// 没有则 `push` 新号。
    ///
    /// **不变量（D-3.2）**：子骨架必须分配到**大于父骨架 idx** 的槽位——父骨架先于子骨架
    /// build（子 build 发生在父的 `_buildSlots` 内），`min_exclusive = 父 idx` 保证
    /// arena 序里父 < 子；alpha 传播 / inheritTimeScale 等跨骨架索引依赖该序
    /// （`docs/guide/03-stage3.md` D-3.2，`RuntimeCtx` 访问契约）。
    ///
    /// 顶层 build 传 `min_exclusive = u32::MAX`（`build_armature`）作为**无约束哨兵**：
    /// 视作"任意空位皆可"→ 复用 `free` 栈的第一个空位，栈空才 push 新号——
    /// 对齐指南 D-3.1 "build → 优先 pop 空位"的池语义（JS `borrowObject(Armature)`
    /// 同样优先复用；顶层骨架无父序约束，复用旧号不破坏子>父不变量）。
    pub fn alloc_armature_slot(&mut self, min_exclusive: u32) -> ArmatureIdx {
        let position = if min_exclusive == u32::MAX {
            // 顶层：无序约束 → 任意空位（保持 free 内部顺序，确定性）
            (!self.free.is_empty()).then_some(0)
        } else {
            self.free.iter().position(|&idx| idx > min_exclusive)
        };
        if let Some(position) = position {
            let idx = self.free.remove(position); // 保持其余空位顺序（确定性）
            return ArmatureIdx(idx);
        }
        let idx = self.armatures.len() as u32;
        self.armatures.push(None);
        ArmatureIdx(idx)
    }

    /// 对齐 `Armature.dispose()`（Armature.ts:265-270）：**只上锁 + 入缓冲队列**。
    ///
    /// 官方 `dispose` 不做清场：`_lockUpdate = true` 后 `_dragonBones.bufferObject(this)`，
    /// 真正 `_onClear` 由下一次全局 `DragonBones.advanceTime` 开头的 `_objects` flush
    /// 执行（Armature.ts:122-180）。Rust 的 [`Self::flush_recycles`] 承担同一时机。
    ///
    /// 可观察效果（测试见 `tests/lifecycle.rs`）：
    /// - 本帧内 `armature(idx)` 仍可查询、仍挂在 clock 成员表里，但 `advance_one`
    ///   因 `lock_update` 早退；
    /// - 下一次 `advance_time`（含 `dt = 0`）先 flush：BFS 子骨架、clock detach、
    ///   `on_clear`（含 `proxy.db_clear`）、槽位置 `None`、`free.push`；
    /// - 重复 dispose 在本帧内静默幂等（等价官方 `bufferObject` 的身份去重）。
    ///
    /// `proxy.dispose(true)` 仍不由本方法调用：TS core 从不回调 proxy.dispose，
    /// 该入口在引擎显示对象侧（`PixiArmatureDisplay.dispose(disposeProxy=true)`）；
    /// 阶段 7 Bevy 代理由其显示实体入口自行调用。
    pub fn dispose_armature(&mut self, idx: ArmatureIdx) {
        let Some(armature) = self.armatures.get_mut(idx.index()).and_then(Option::as_mut) else {
            log::warn!(
                "dispose_armature: armature {} not built or already recycled",
                idx.0
            );
            return;
        };
        if armature.lock_update {
            return; // 幂等：等价 JS bufferObject 的身份去重
        }
        armature.lock_update = true;
        self.pending_recycles.push(PendingRecycle::Armature(idx));
    }

    /// 对齐 `Armature._onClear` + arena 空位复用（D-3.1 / D-5.6）。
    ///
    /// `on_clear()`（含 `proxy.dbClear()`，Armature.ts:147-149）→ 槽置 `None` →
    /// `free.push(idx)`。子骨架递归释放（Slot.ts:279 `eachDisplay.dispose()` 的
    /// arena 侧承担）：BFS 收集自身 + 全部子骨架 display（`child_armature` 与各
    /// `display_frames[].display = Armature(_)`）后统一清场，去重防环。
    ///
    /// 只由 [`Self::flush_recycles`] 调用；直接调用会绕过官方的一帧延迟。
    fn dispose_armature_now(&mut self, idx: ArmatureIdx) {
        if self.armatures.get(idx.index()).is_none_or(Option::is_none) {
            log::warn!("dispose_armature_now: armature {} already recycled", idx.0);
            return;
        }

        // ① BFS 收集自身 + 子骨架后代（去重防环）
        let mut seen: Vec<u32> = Vec::new();
        let mut pending: Vec<u32> = vec![idx.0];
        while let Some(current) = pending.pop() {
            if seen.contains(&current) {
                continue;
            }
            seen.push(current);
            if let Some(armature) = self.armatures[current as usize].as_ref() {
                for slot in &armature.slots {
                    if let Some(child) = slot.child_armature {
                        // Slot.ts:511/548 运行时回指
                        pending.push(child.0);
                    }
                    for display_frame in &slot.display_frames {
                        if let Some(DisplayHandle::Armature(child)) = display_frame.display {
                            // frame 持有的子骨架（每帧一个独立实例，TS:289 每帧递归 build）
                            pending.push(child.0);
                        }
                    }
                }
            }
        }

        // ② 逐个 on_clear + 置 None + free.push（BFS 序：自身先于后代 ≈ TS
        //    `bufferObject` 的入队序；后代可能已被上一轮清掉 → take 失败即跳过）
        for current in seen {
            let current_idx = ArmatureIdx(current);
            if self
                .armatures
                .get(current_idx.index())
                .is_none_or(Option::is_none)
            {
                continue;
            }

            let detach_result = {
                let mut runtime = RuntimeCtx::new(
                    &mut self.armatures,
                    &mut self.clocks,
                    &mut self.pending_events,
                    None,
                    None,
                );
                runtime.move_armature_clock(current_idx, None)
            };
            if let Err(error) = detach_result {
                log::warn!(
                    "dispose_armature: failed to detach armature {} from its clock: {error}; forcing clock=None",
                    current
                );
            }

            // Recovery path: even if the relationship was already inconsistent,
            // remove any residual Clock membership and force `clock = None`
            // before releasing the arena slot. Otherwise the clock would keep a
            // stale handle and the armature would stay permanently stuck.
            {
                let mut runtime = RuntimeCtx::new(
                    &mut self.armatures,
                    &mut self.clocks,
                    &mut self.pending_events,
                    None,
                    None,
                );
                if !runtime.force_detach_armature_clock(current_idx) {
                    log::warn!(
                        "dispose_armature: armature {} unavailable during clock cleanup",
                        current
                    );
                }
            }

            let Some(slot) = self.armatures.get_mut(current as usize) else {
                continue;
            };
            let Some(mut armature) = slot.take() else {
                continue; // 已是空位
            };
            // 延迟清场时 `lock_update` 已由 dispose 置 true；`on_clear` 会复位
            // false（Armature.ts:158），与官方 `_onClear` 收尾一致。
            armature.on_clear(); // = returnToPool 时点的 _onClear（含 proxy.dbClear）
            self.free.push(current); // D-3.1 空位复用栈
        }
    }

    /// 按索引读 arena（guide 任务 2 API）。
    ///
    /// # Panics
    /// 越界或空位（`None` = 已 dispose，D-3.1）；判空请直接读 `self.armatures`。
    #[must_use]
    pub fn armature(&self, idx: ArmatureIdx) -> &Armature {
        self.armatures
            .get(idx.index())
            .and_then(Option::as_ref)
            .expect("armature arena slot empty (never built or disposed)")
    }

    /// 按索引取可变 arena 槽（guide 任务 2 API）。
    ///
    /// # Panics
    /// 同 [`Self::armature`]。
    pub fn armature_mut(&mut self, idx: ArmatureIdx) -> &mut Armature {
        self.armatures
            .get_mut(idx.index())
            .and_then(Option::as_mut)
            .expect("armature arena slot empty (never built or disposed)")
    }

    // ------------------------------------------------------------------
    // _buildBones / _buildSlots / _buildConstraints（TS:168-252）
    // ------------------------------------------------------------------

    /// 对齐 `_buildBones(dataPackage, armature)`（BaseFactory.ts:168-173）。
    ///
    /// - 遍历 **`sortedBones`（拓扑序，F3.2）**——创建序 = 拓扑序，**不是 JSON 原始序**；
    ///   `Armature::advance_time` 的 `split_at_mut(i)` 顺序更新依赖“父索引 < 子索引”。
    /// - TS:170 按 `boneData.type` 选池对象 `Bone`/`Surface`——Rust 单 `Bone` 结构 +
    ///   [`Bone::init`] 内按 `BoneKind` 分派 Surface 载荷（bone.rs 既定）。
    /// - TS 无 add 调用（注册在 `Bone.init` 内 TS:358 `_addBone`）→ Rust 刻意差异：
    ///   `init` 不 push（只预置 idx），显式 `armature.add_bone`（bone.rs 方法文档，二选一）。
    fn _build_bones(&self, data_package: &BuildArmaturePackage, armature: &mut Armature) {
        let armature_data = data_package.armature(); // TS:169 dataPackage.armature.sortedBones
        for bone_name in &armature_data.sorted_bones {
            let Some(bone_data) = armature_data.bones.get(bone_name) else {
                // sorted_bones 由 addBone/sortBones 维护 → 正常必命中（防御）
                log::warn!(
                    "bone `{}` in sortedBones missing from bones map (BaseFactory.ts:169)",
                    bone_name
                );
                continue;
            };
            let mut bone = Bone::default(); // TS:170 借池对象（Rust：Default = _onClear 复位态）
            bone.init(bone_data, armature.idx, &armature.bones); // TS:171（不 push，idx 预置）
            armature.add_bone(bone); // TS:358 `_addBone` → Rust 显式 push
        }
    }

    /// 对齐 `_buildConstraints(dataPackage, armature)`（BaseFactory.ts:226-252）。
    ///
    /// **F3.1 已证伪**：`龙`（IK_L/IK_R）与 `Rooster_Ani/Blend/P1`（bone_ik/bone_ik1）
    /// 原始 JSON 含 `"ik"` 数组，共 4 个 IK；Path 素材为 0。照抄原文要点：
    /// - 遍历 `armature.constraints`（IndexMap 插入序 = TS `for…in` 键序）——
    ///   TS 此处**无 order 排序**，`Armature._addConstraint`（Armature.ts:227-231）
    ///   亦仅 push → 运行期更新序 = 数据序；
    /// - switch（:229）IK 与 **default** 两分支都建 `IKConstraint`（Rust 的
    ///   `ConstraintType` 只有两变体 → IK 单分支等价）、Path 分支（:235-239）
    ///   建 `PathConstraint` 真身——阶段 3 无素材 → warn 跳过（P1-2 阶段 6 接，
    ///   不进 `armature.constraints`、不标 `has_constraint`）。
    fn _build_constraints(&self, data_package: &BuildArmaturePackage, armature: &mut Armature) {
        let armature_data = data_package.armature(); // TS:227
        for (name, constraint_data) in &armature_data.constraints {
            match constraint_data.type_() {
                ConstraintType::IK => {
                    // TS:231-234 `borrowObject(IKConstraint)` + `init` + `_addConstraint`
                    let mut constraint = Constraint::default(); // 借池对象 → Rust Default（= _onClear 态）
                    constraint.init(constraint_data, &mut armature.bones); // 含 TS:203 root._hasConstraint = true
                    armature.add_constraint(constraint); // TS `_addConstraint`
                }
                ConstraintType::Path => {
                    // TS:235-239 的 Path 分支：无素材（"path" 键全无）→ warn 跳过
                    log::warn!(
                        "constraint `{}` (type Path) skipped: Path 阶段 6（P1-2）实现（BaseFactory.ts:235-239；F3.1 勘误：Path 素材 0 个）",
                        name
                    );
                }
            }
        }
    }

    /// 对齐 `_buildSlots(dataPackage, armature)`（BaseFactory.ts:177-224）——逐行
    /// 翻译（笔记 A.5，F3.8 流程契约）。
    ///
    /// **借用策略（任务约定报告项）**：`data_package` 自持 `Arc<DragonBonesData>` 与
    /// skin 克隆（独立于 `armature` 的本地借用）——循环里 `slot_data`/`display_data`
    /// 借自 `data_package`（共享），push/replace 的对象是 `armature`（可变），两者不相交；
    /// `DisplayData`/`SlotData` 按值 `clone` 传给 `Slot::replace*`（D-3.3 值语义，
    /// `slot.rs` DisplayFrame 文档既定）→ **无需**把数据先收集进临时 Vec。
    ///
    /// 要点（与 TS 行行对应）：
    /// 1. **180-182 早退**：`currentSkin` 或 `defaultSkin` 任一为 null → 整个函数返回，
    ///    **连 `_buildSlot` 循环都不进**（该 armature 一个槽都不建；能触发的是
    ///    `armatureData.defaultSkin === null` 的数据）。
    /// 2. **184-195 合并 = 按槽名整组覆盖**（:193 `skinSlots[k] = displays`，**不是**逐
    ///    元素 merge）：default 先全拷；current 的同名 key 整组替换（current 覆盖），
    ///    没出现的槽保留 default。TS `currentSkin !== defaultSkin` 是对象身份短路
    ///    （纯优化）→ Rust 用**值不等**判定：同对象 ⟹ 克隆值相等 ⟹ 同样跳过；
    ///    不同对象但内容相等时“整组覆盖等值数组”无行为差异（后续消费方全按值读）。
    /// 3. **197-199**：遍历 `sortedSlots`（槽创建序 = 排序序，非 skin 键序）；
    ///    `displayDatas` 按 `slotData.name in skinSlots` 判定。
    /// 4. **202-219 逐帧**：`set_display_frame_count` → `replace_raw_display_data`
    ///    （`None` 触发 defaultSkin 回填，Slot.ts:825-833）→ atlasName 非空才
    ///    `replace_texture_data`（:208-211）→ `_get_slot_display` → `replace_display(display, i)`
    ///    （:214）→ **:217 `replaceDisplay(null)` 漏传 index**（本函数唯一漏参调用，
    ///    笔记 A.5-8）→ Rust 传 `-1` 保留原语义：index 归一成
    ///    `displayIndex < 0 ? 0 : displayIndex`（build 期 displayIndex = -1 → **写 frame 0**）
    ///    且 `0 === -1` 假 → **不置 displayDirty**。
    /// 5. **222 `_setDisplayIndex(slotData.displayIndex, true)`**：第二参是 **isAnimation**
    ///    （不是 force——笔记 A.5-9 行号勘误）：先同步 `_animationDisplayIndex` 并以其
    ///    作去重闸，再 clamp（`displayFrames` 长度 0 → 恒 -1，即“槽不在任何皮肤”时
    ///    displayIndex 落 -1，`Slot.update` 提前返回）。
    fn _build_slots(&mut self, data_package: &BuildArmaturePackage, armature: &mut Armature) {
        // TS:178-182 早退（要点 1）
        let Some(current_skin) = data_package.skin.as_ref() else {
            return;
        };
        let armature_data = data_package.armature();
        let Some(default_skin) = armature_data
            .default_skin
            .as_deref()
            .and_then(|default_name| armature_data.get_skin(default_name))
        else {
            return; // TS:179-180 `defaultSkin === null`
        };

        // TS:184-195 skinSlots 合并（要点 2）
        let mut skin_slots: IndexMap<&str, &[Option<DisplayData>]> = IndexMap::new();
        for (slot_name, displays) in &default_skin.displays {
            skin_slots.insert(slot_name.as_str(), displays.as_slice()); // :185-188（引用无深拷贝）
        }
        if current_skin != default_skin {
            // :190 TS 对象身份短路 → Rust 值不等（等价性见方法文档要点 2）
            for (slot_name, displays) in &current_skin.displays {
                skin_slots.insert(slot_name.as_str(), displays.as_slice()); // :191-194 整组替换
            }
        }

        // TS:199 `_buildSlot`（三抽象之一）入参所需的图集缩放（见 texture_atlas_scale 文档）
        let texture_atlas_scale =
            self.texture_atlas_scale(&data_package.texture_atlas_name, &data_package.data_name);

        // TS:197 遍历 sortedSlots
        for slot_name in &armature_data.sorted_slots {
            let Some(slot_data) = armature_data.slots.get(slot_name) else {
                // sorted_slots 由 addSlot 维护 → 正常必命中（防御）
                log::warn!(
                    "slot `{}` in sortedSlots missing from slots map (BaseFactory.ts:197)",
                    slot_name
                );
                continue;
            };

            // TS:198 `slotData.name in skinSlots ? skinSlots[slotData.name] : null`
            let display_datas = skin_slots.get(slot_name.as_str()).copied();

            // TS:199 `_buildSlot(...)`——NullFactory `new NullSlot` + `s.init(..., {}, {})`
            // （笔记 A.12）→ Rust `Slot::new` + NullSlotAdapter 内联（文件尾 TODO 阶段7）
            let slot = Slot::new(
                slot_data,
                armature,
                texture_atlas_scale,
                DisplayHandle::Raw, // 对应 jsdump NullSlot 的 raw 显示占位
                DisplayHandle::Mesh,
                Box::new(NullSlotAdapter),
            );
            let slot_idx = slot.idx; // `Slot::init` 预置（= push 后的下标）
            armature.add_slot(slot); // TS:665 `armature._addSlot(this)`（init 内）→ Rust 显式 push
            if !armature
                .slots
                .last()
                .is_some_and(|pushed| pushed.idx == slot_idx)
            {
                // add_slot 拒绝重名（防御；sortedSlots 正常无重复 → 极少触发）
                continue;
            }

            if let Some(display_datas) = display_datas {
                // 安全论证：本段用**位置下标** `armature.slots[slot_idx.index()]`（:698-752
                // 共 5 处）而非按名查槽——build 期 slots 恒创建序（本循环就是唯一写入方、
                // 首次 advance 排序之前），`Slot::init` 预置的 `idx == 在数组中的位置`，
                // 按 idx 下标即按位置；**运行期** `Armature::advance_time` ③ 会重排 slots
                // （zIndex*1000+zOrder 稳定排序）→ 重排后 `idx` 不再等于位置，任何运行期
                // 路径都不可再用 `slots[idx.index()]` 位置下标（须按 `get_slot` 名查/存 idx）。
                // TS:201
                // TS:202 扩容（新槽 `_displayFrames` 为空 → 0→N）
                armature.slots[slot_idx.index()].set_display_frame_count(display_datas.len());

                for (frame_index, display_data) in display_datas.iter().enumerate() {
                    let index = frame_index as i32;
                    // TS:205（displayData 可为 null → Slot 内部从 defaultSkin 回填，
                    // Slot.ts:825-833）
                    armature.slots[slot_idx.index()]
                        .replace_raw_display_data(display_data.clone(), index);

                    if let Some(display_data) = display_data {
                        // ---- TS:207-211 贴图（双通道的“显式通道”）----
                        // `DisplayFrame.getTextureData()`（Slot.ts:112-138）build 期
                        // `frame.displayData` 恒 null → 实际优先级 = frame.textureData
                        //（通道2，本处写）→ rawDisplayData.texture（通道3，TS:264-266/274-276
                        // 由 `_getSlotDisplay` 回填进 data 层、与 rawDisplayData 同一对象）。
                        let texture_data = if !data_package.texture_atlas_name.is_empty() {
                            self._get_texture_data(
                                &data_package.texture_atlas_name,
                                &display_data.path,
                            )
                        } else {
                            None // :208 atlasName 为空 → 完全不写（保持 null）
                        };
                        let texture_data = match texture_data {
                            Some(texture_data) => Some(texture_data), // 通道2 命中 → 优先
                            None => match &display_data.kind {
                                // 通道3：TS 在 `_getSlotDisplay` 264-266（Image）/274-276
                                // （Mesh）把 `_getTextureData(dataName, path)` 写进 data 层
                                // `displayData.texture`——Rust data 层不可变（D-3.3）且
                                // `DisplayData.texture` 是无消费方的名字字段（parser 恒 None）
                                // → 不回填，在此按原优先级等价补查。注意 TS:259/265 的怪癖：
                                // `_getTextureData` 第一参吃的是 **dataName**（不是 atlasName）。
                                DisplayKind::Image { .. } | DisplayKind::Mesh { .. } => self
                                    ._get_texture_data(&data_package.data_name, &display_data.path),
                                // 其它类型 TS 也不回填（只有 Image/Mesh 分支写 texture）
                                _ => None,
                            },
                        };
                        // TS:210 `slot.replaceTextureData(textureData, i)`（miss 也会写 null）
                        armature.slots[slot_idx.index()].replace_texture_data(texture_data, index);

                        // TS:213-214
                        let display =
                            self._get_slot_display(data_package, display_data, armature, slot_idx);
                        armature.slots[slot_idx.index()].replace_display(display, index);
                    } else {
                        // TS:217 `slot.replaceDisplay(null)` **漏传 index**（要点 4）：
                        // -1 → 写 frame 0 且不置 displayDirty——逐字照抄，勿“修正”成 i
                        armature.slots[slot_idx.index()].replace_display(None, -1);
                    }
                }
            }

            // TS:222（要点 5；F3.8 末位 _setDisplayIndex(..., true)）
            armature.slots[slot_idx.index()].set_display_index(slot_data.display_index, true);
        }
    }

    /// 对齐 `_buildChildArmature(dataPackage, _slot, displayData)`（BaseFactory.ts:254-256）。
    ///
    /// 参数映射（TS:255）：`armatureName = displayData.path`、
    /// `dragonBonesName = dataPackage.dataName`、**`skinName` 恒 `""`**（子骨架永远用
    /// 默认皮肤）、`textureAtlasName = dataPackage.textureAtlasName`（继承父包）。
    ///
    /// - `_slot` 参数被 TS 完全忽略（下划线命名）→ Rust 不收。
    /// - 递归公开 `buildArmature`（内部再跑 fill/bones/slots/constraints +
    ///   `invalidUpdate` + `advanceTime(0.0)`，笔记 A.7）。
    /// - **子骨架不额外 advance**（F3.11：顶层 build 收尾的 advance 只在各自 build 内发生；
    ///   之后由 `BaseFactory::advance_time` 的 clock 派发统一推进全部骨架）。
    /// - proxy：TS `NullFactory._buildArmature` 每次 `new EvProxy()`（笔记 B.5）→
    ///   Rust [`NullArmatureProxy`]。
    /// - Rust 侧附加 `min_exclusive = 父 idx`：**子骨架 idx > 父 idx**（D-3.2 不变量，
    ///   TS 靠对象引用无此约束）。
    fn _build_child_armature(
        &mut self,
        data_package: &BuildArmaturePackage,
        display_data: &DisplayData,
        parent_idx: ArmatureIdx,
    ) -> Option<ArmatureIdx> {
        self.build_armature_inner(
            &display_data.path,
            &data_package.data_name,
            "",
            &data_package.texture_atlas_name,
            Box::new(NullArmatureProxy::new()),
            parent_idx.0,
        )
    }

    /// 对齐 `_getSlotDisplay(dataPackage, displayData, slot)`（BaseFactory.ts:258-322）——
    /// 按 `displayData.type` 分派（笔记 A.8 逐 case 核对）。
    ///
    /// 签名差异：`dataPackage` 收**非空**引用——build 路径恒非空；TS `=== null` 的
    /// 三级回退（:259 `displayData.parent.parent.parent.name`）归 `replace_*` 阶段 6
    /// （P1-5），且 D-1.1 把 parent 链压成名字引用，对象链在 Rust 无法表达（见文件尾 TODO）。
    /// `slot` 拆成 `armature: &mut Armature`（TS 经 `slot.armature`）+ `slot_idx`
    /// （TS:298 `eventObject.slot = slot`）。
    ///
    /// - **Image/Mesh**（:262-285）：只回填 data 层 `displayData.texture`（Rust 不可回填，
    ///   等价实现在 `_build_slots` 的 texture 通道 3，见彼处注释）+ 返回 raw/mesh handle。
    /// - **Armature**（:287-312）：递归 `_buildChildArmature` → `inheritAnimation` 赋值 →
    ///   `inheritAnimation === false` 才走“自己播”（action 优先级 :293：display 自带 →
    ///   否则子骨架 `defaultActions`；有 action → 逐条 `actionDataToInstance` +
    ///   `_bufferAction(_, false)` 头插；无 action → `animation.play()` :303）→
    ///   返回 `DisplayHandle::Armature(child)`（:310 `display = childArmature` 本体——
    ///   真正的解包在 `Slot._updateDisplay` Slot.ts:510-513）。
    /// - **BoundingBox**（:314-315）与 **default**：`break` → `None`（display 保持 null）。
    ///   **不存在 `case DisplayType.Path`**（笔记 A.8 READ-ORIGINAL）：Path 落 default
    ///   返回 null——照抄，勿自加分支（否则凭空多出一个 display）。
    fn _get_slot_display(
        &mut self,
        data_package: &BuildArmaturePackage,
        display_data: &DisplayData,
        armature: &mut Armature,
        slot_idx: SlotIdx,
    ) -> Option<DisplayHandle> {
        // TS:259-260（dataName 见签名文档；Rust 侧消费点在 `_build_slots` texture 通道 3）
        match &display_data.kind {
            DisplayKind::Image { .. } => {
                // TS:264-266 texture 回填（缓存副作用，Rust 无对应写点——见上）
                // TS:268 `display = slot.rawDisplay`
                Some(armature.slots[slot_idx.index()].raw_display)
            }

            DisplayKind::Mesh { .. } => {
                // TS:274-276 同上 texture 回填；TS:278-283
                let slot = &armature.slots[slot_idx.index()];
                Some(if self.is_support_mesh() {
                    slot.mesh_display
                } else {
                    slot.raw_display
                })
            }

            DisplayKind::Armature {
                inherit_animation,
                actions,
                armature: _,
            } => {
                // TS:287-312
                let Some(child_idx) =
                    self._build_child_armature(data_package, display_data, armature.idx)
                else {
                    // TS:290 null 检查：build 失败 → display 保持 null →
                    // 调用方 `_buildSlots:214` `replaceDisplay(null, i)`（显式 i）
                    return None;
                };

                if let Some(child) = self.armatures[child_idx.index()].as_mut() {
                    child.inherit_animation = *inherit_animation; // TS:291

                    if !child.inherit_animation {
                        // TS:293 action 来源优先级：display 自带 → 否则子骨架 defaultActions
                        let picked: Vec<ActionData> = if !actions.is_empty() {
                            actions.clone()
                        } else {
                            child.armature_data().default_actions.clone()
                        };

                        if !picked.is_empty() {
                            // TS:295-300 逐条借 EventObject（Rust 值语义 → `default()`，
                            // 池借出 `_onClear` 等价）+ 填充 + 头插 buffer
                            for action in &picked {
                                let mut event_object = EventObject::default();
                                EventObject::action_data_to_instance(
                                    action,
                                    &mut event_object,
                                    armature.idx,
                                    &armature.bones,
                                    &armature.slots, // 装配期部分槽（TS getSlot 同样只查到已建槽）
                                );
                                event_object.slot = Some(slot_idx); // TS:298 `eventObject.slot = slot`
                                event_object.slot_name =
                                    Some(armature.slots[slot_idx.index()].slot_name.clone()); // D-4.20 名字冗余
                                armature.buffer_action(event_object, false); // TS:299 append=false → 头插
                            }
                        } else {
                            // TS:303 没有任何 action → 播子骨架默认动画
                            //（阶段 3 `Animation::play` no-op 占位，阶段 4 接）
                            child.animation.play(None, -1);
                        }
                    }
                }

                // TS:307 `armatureDisplayData.armature = childArmature.armatureData`——
                // data 层写回不复刻：Rust data 不可变（D-3.3），且该字段全库只写不读
                // （TS 内无读方；grep `ArmatureDisplayData` 仅 parser 初始化 + 本行写入）
                // → 零行为影响。
                Some(DisplayHandle::Armature(child_idx)) // TS:310 `display = childArmature`
            }

            // TS:314-315 `case DisplayType.BoundingBox: break;` → display 保持 null
            DisplayKind::BoundingBox { .. } => None,

            // TS:317-318 default → null；**不存在 Path case**（Path=4 落 default，
            // 笔记 A.8 READ-ORIGINAL——照抄，勿自加）
            DisplayKind::Path { .. } => None,
        }
    }

    // ------------------------------------------------------------------
    // buildArmature（TS:701-716）
    // ------------------------------------------------------------------

    /// 对齐 `buildArmature(armatureName, dragonBonesName, skinName, textureAtlasName)`
    /// （BaseFactory.ts:701-716）。
    ///
    /// # 参数（与 guide 草图的偏差见返回值文档）
    /// - `armature_name`：骨架数据名（TS:701 第 1 参）。
    /// - `db_name`：DragonBonesData 缓存名（TS `dragonBonesName = ""`；空 = fill ② 全表扫）。
    /// - `skin_name`：皮肤名（TS `skinName = ""`；**允许填另一个骨架名共享其皮肤**，TS:672）。
    /// - `atlas_name`：贴图集缓存名（TS `textureAtlasName = ""`）。
    /// - `proxy`：注入的引擎代理（D-3.4 调用方注入——TS 经抽象 `_buildArmature` 建
    ///   proxy/display，jsdump `NullFactory._buildArmature` 里 `new EvProxy()`；
    ///   Rust 把该差异抬到签名，Null 语义由 caller 传 [`NullArmatureProxy`]）。
    ///
    /// **签名偏差（guide 任务 7 草图无 proxy 参数 + 返回裸 idx → 按 D-3.4 注入 proxy +
    /// JS `buildArmature` 失败返回 `null` 的语义）**：返回 `Option<ArmatureIdx>`。
    /// TS 的 `|| ""` 入参归一（:703）在 Rust 无 null/undefined，天然等价。
    ///
    /// 流程 = TS:701-716 + F3.11（**不挂 clock**——TS 本方法不把骨架塞进 WorldClock，
    /// 调用方自行 `armature.clock = factory.clock`，笔记 A.10）。
    pub fn build_armature(
        &mut self,
        armature_name: &str,
        db_name: &str,
        skin_name: &str,
        atlas_name: &str,
        proxy: Box<dyn ArmatureProxy>,
    ) -> Option<ArmatureIdx> {
        // 顶层：无父骨架 → 传 u32::MAX 作**无约束哨兵**（≠"严格 > 恒 push 新号"）：
        // alloc 对哨兵特判为“复用 free[0]，栈空才 push 新号”，见 alloc 文档 :430-433
        self.build_armature_inner(
            armature_name,
            db_name,
            skin_name,
            atlas_name,
            proxy,
            u32::MAX,
        )
    }

    /// [`Self::build_armature`] 的公共体（子骨架递归入口 `_buildChildArmature`
    /// BaseFactory.ts:254-256 用 `min_exclusive = 父 idx` 保证子 > 父，D-3.2）。
    fn build_armature_inner(
        &mut self,
        armature_name: &str,
        db_name: &str,
        skin_name: &str,
        atlas_name: &str,
        proxy: Box<dyn ArmatureProxy>,
        min_exclusive: u32,
    ) -> Option<ArmatureIdx> {
        // TS:702-703 fill（失败 → :704 warn + return null）
        let Some(data_package) =
            self.fill_build_armature_package(db_name, armature_name, skin_name, atlas_name)
        else {
            // TS:704 的 `dragonBonesName !== null ? dragonBonesName : ""` 拼串怪癖不复刻
            log::warn!("No armature data: {}, {}", armature_name, db_name);
            return None;
        };

        // TS:708 `_buildArmature`（抽象）→ NullFactory：`borrowObject(Armature)` + `new EvProxy()`
        // + `arm.init(pkg.armature, p, p, this._dragonBones)`（jsdump.mjs:101 的 arm.init 形态：
        // display 与 proxy 是同一对象、非 null）。Rust：proxy 由参数注入；display 取
        // `Armature(idx)` = “自己的显示对象”占位——**不能用 Null**：子骨架槽在
        // `Slot._updateDisplay` 里 `_display = childArmature.display`（Slot.ts:512），
        // 若为 Null → 槽 display 为 None → `Slot.update` 698-700 提前返回（跳过姿态段），
        // 与 TS（EvProxy 非 null）不等价。
        let idx = self.alloc_armature_slot(min_exclusive); // 顶层：u32::MAX 哨兵 → 复用 free[0]（栈空才新号）
        let mut armature = Armature::new(
            idx,
            Arc::clone(&data_package.data),
            data_package.armature_name.clone(),
            proxy,
            DisplayHandle::Armature(idx),
        );

        // TS:709-711 固定顺序
        self._build_bones(&data_package, &mut armature);
        self._build_slots(&data_package, &mut armature);
        self._build_constraints(&data_package, &mut armature);

        // TS:712-713（F3.11：先 invalidUpdate(null, true) 再 advanceTime(0.0)，
        // 漏了静姿态不算——guide 坑 7）
        self.armatures[idx.index()] = Some(armature); // 入 arena（此刻槽必空：新号或 free 取出）
        self.armature_mut(idx).invalid_update(None, true); // TS:712
        self.advance_one(idx, 0.0); // TS:713 `armature.advanceTime(0.0)`（F3.11）
        Some(idx)
    }

    /// 单骨架推进（take-out → [`Armature::advance_time`] → 放回）：
    /// 推进期间 `armatures[idx]` 为 `None`，满足 [`RuntimeCtx`] 的“自身不可经 world 访问”
    /// 借用契约（armature/mod.rs 模块文档）。
    ///
    /// `passed_time = 0.0` 照样走完整主循环（`Armature::advanceTime` 无 0 值早退）
    /// ——**直接驱动单骨架 = TS `armature.advanceTime(0)`**（build 收尾 BaseFactory.ts:713
    /// 同款）：消费脏标记 / 槽排序 / 静姿态收敛。全局 [`Self::advance_time`] 不是
    /// “全局 no-op”：WorldClock 内层在缩放后 0 早退（**成员不推进**），但外层
    /// objects flush 与 events 快照替换两段仍执行（04-stage4.md:175,198；
    /// WorldClock.ts:114 只负责 clock 内层）。要“跑一遍骨架更新”就调本方法。
    pub fn advance_one(&mut self, idx: ArmatureIdx, passed_time: f64) {
        let Some(slot) = self.armatures.get_mut(idx.index()) else {
            return; // 越界：防御跳过（alloc 保证不存在）
        };
        let Some(mut armature) = slot.take() else {
            return; // 空位（已 dispose）
        };
        let current_clock = armature.clock();
        let mut world = RuntimeCtx::new(
            &mut self.armatures,
            &mut self.clocks,
            &mut self.pending_events,
            Some(idx),
            current_clock,
        );
        armature.advance_time(passed_time, &mut world);
        // `world` 无 Drop：其对 `self.armatures` 的借用在最后一次使用（上一行）后由 NLL 结束
        if let Some(slot) = self.armatures.get_mut(idx.index()) {
            *slot = Some(armature);
        }
    }

    /// 对齐 `DragonBones.advanceTime`（DragonBones.ts:304-332）→ `WorldClock.advanceTime`
    /// 的三段顺序（对象回收 flush → root Clock → 本帧 events flush）。对象池 flush
    /// 暂为空操作；阶段 4 的 event flush 保存快照，不 dispatch listener。
    ///
    /// 不在此层对 `passed_time == 0` 早退：WorldClock 必须先刷新 system-time，缩放后
    /// 再早退；外层的 object/event 两段仍运行。
    pub fn advance_time(&mut self, passed_time: f64) {
        let mut now = wall_clock_seconds;
        self.advance_time_with_source(passed_time, &mut now);
    }

    /// Deterministic wall-clock seam for integration tests.
    #[doc(hidden)]
    pub fn advance_time_with(&mut self, passed_time: f64, now: f64) {
        let mut now_source = || now;
        self.advance_time_with_source(passed_time, &mut now_source);
    }

    /// Advances one clock subtree directly, mirroring the root-clock segment of
    /// [`Self::advance_time`] (`WorldClock.ts:97-155`). Only this clock and its
    /// nested members advance; the object/event flush phases are not run.
    ///
    /// This is the Rust equivalent of calling `WorldClock.advanceTime` on a
    /// standalone clock. The time source is the wall clock, identical to
    /// [`Self::advance_time`].
    pub fn advance_clock(&mut self, clock: ClockId, passed_time: f64) -> Result<(), ClockError> {
        let mut now = wall_clock_seconds;
        let mut clock_path = Vec::new();
        self.advance_clock_with(clock, passed_time, &mut now, &mut clock_path)
    }

    fn advance_time_with_source(&mut self, passed_time: f64, now: &mut impl FnMut() -> f64) {
        // ① JS DragonBones.ts:305-311: recycle objects buffered by the previous frame.
        self.flush_recycles();

        // ② JS :313
        let mut clock_path = Vec::new();
        if let Err(error) =
            self.advance_clock_with(self.root_clock, passed_time, now, &mut clock_path)
        {
            log::warn!("root clock advance stopped: {error}");
        }

        // ③ JS :315-331: dispatch this frame's events (after the clock).
        self.dispatch_events();
    }

    /// Returns objects buffered by `dispose_armature` (`JS _objects` flush).
    fn flush_recycles(&mut self) {
        let pending = std::mem::take(&mut self.pending_recycles);
        for item in pending {
            match item {
                PendingRecycle::Armature(idx) => self.dispose_armature_now(idx),
            }
        }
    }

    /// Dispatches queued events in FIFO order (`DragonBones.ts:315-331`).
    ///
    /// - `armature` missing/None → skip the whole event, including the SOUND
    ///   second dispatch (`:320`);
    /// - armature proxy first, then the global event manager for SOUND (`:321-324`);
    /// - `while let pop_front` mirrors the official loop's dynamic length: events
    ///   appended while dispatching are visited in the same `advance_time`.
    fn dispatch_events(&mut self) {
        while let Some(event) = self.pending_events.pop_front() {
            let Some(armature_idx) = event.armature else {
                continue; // 防御：官方事件恒带 armature
            };
            let Some(armature) = self
                .armatures
                .get_mut(armature_idx.index())
                .and_then(Option::as_mut)
            else {
                continue; // JS :320 `_armatureData === null` → 整条跳过（含 SOUND）
            };

            armature.proxy.dispatch_db_event(&event.event_type, &event);

            if event.event_type == EventObject::SOUND_EVENT
                && let Some(manager) = self.event_manager.as_mut()
            {
                manager.dispatch_db_event(&event.event_type, &event);
            }
            // 阶段 5 不池化：事件在本次派发调用栈结束后 drop（D-5.3）。
        }
    }

    /// Visits the fixed-start member snapshot in order, resolving each token
    /// immediately before dispatch so runtime detach/reattach takes effect now.
    fn visit_clock_members_with(
        &mut self,
        clock_id: ClockId,
        member_time: f64,
        mut advance: impl FnMut(&mut Self, AnimatableHandle, f64),
    ) -> Result<(), ClockError> {
        let snapshot = self
            .clocks
            .get(clock_id)
            .ok_or(ClockError::InvalidClockId)?
            .member_snapshot();

        for (index, token) in snapshot {
            let handle = self
                .clocks
                .get(clock_id)
                .and_then(|clock| clock.active_member_at(index, token));
            if let Some(handle) = handle {
                advance(self, handle, member_time);
            }
        }

        if let Some(clock) = self.clocks.get_mut(clock_id) {
            clock.compact();
        }
        Ok(())
    }

    fn advance_clock_with(
        &mut self,
        clock_id: ClockId,
        passed_time: f64,
        now: &mut impl FnMut() -> f64,
        clock_path: &mut Vec<ClockId>,
    ) -> Result<(), ClockError> {
        if clock_path.contains(&clock_id) {
            // 官方 WorldClock.add()（WorldClock.ts:192-197）不检查环：环会让
            // advanceTime 无限递归。Rust 在此跳过当前递归路径以避免栈溢出；
            // 这是对官方的有意限制（D-4.33），不是 AnimationState graph 的
            // D-4.32。
            log::warn!("clock cycle encountered while advancing {:?}", clock_id);
            return Ok(());
        }

        let member_time = self
            .clocks
            .get_mut(clock_id)
            .ok_or(ClockError::InvalidClockId)?
            .update_time(passed_time, now());
        let Some(member_time) = member_time else {
            return Ok(());
        };

        clock_path.push(clock_id);

        let result = self.visit_clock_members_with(clock_id, member_time, |runtime, handle, dt| {
            match handle {
                AnimatableHandle::Armature(armature) => {
                    if runtime
                        .armatures
                        .get(armature.index())
                        .is_none_or(Option::is_none)
                    {
                        // Stale handle: remove it from this Clock and continue safely.
                        if let Some(clock) = runtime.clocks.get_mut(clock_id) {
                            clock.detach(handle);
                        }
                    } else {
                        runtime.advance_one(armature, dt);
                    }
                }
                AnimatableHandle::Clock(child) => {
                    if runtime.clocks.get(child).is_none() {
                        // ClockArena 不提供删除，合法 ClockId 永不失效；此分支只
                        // 防御外部拼装的无效 id（正常流程不可达）。
                        if let Some(clock) = runtime.clocks.get_mut(clock_id) {
                            clock.detach(handle);
                        }
                    } else if let Err(error) =
                        runtime.advance_clock_with(child, dt, now, clock_path)
                    {
                        log::warn!("nested clock advance stopped: {error}");
                    }
                }
            }
        });

        clock_path.pop();
        result
    }

    /// Buffers a new owned event for the next global-advance dispatch.
    #[doc(hidden)]
    pub fn buffer_event(&mut self, event: EventObject) {
        self.pending_events.push_back(event);
    }

    /// Sets the global event manager used for SOUND double dispatch
    /// (`DragonBones.ts:322-324`). Unset means only the armature proxy receives
    /// the event.
    pub fn set_event_manager(&mut self, manager: Box<dyn EventDispatcher>) {
        self.event_manager = Some(manager);
    }

    #[must_use]
    pub fn event_manager(&self) -> Option<&(dyn EventDispatcher + 'static)> {
        self.event_manager.as_deref()
    }

    pub fn event_manager_mut(&mut self) -> Option<&mut (dyn EventDispatcher + 'static)> {
        self.event_manager.as_deref_mut()
    }
}

// TODO 阶段 7（D-3.4 第三组契约）: `FactoryAdapter` trait（对齐 TS:324-326 的
//   `_buildTextureAtlasData / _buildArmature / _buildSlot` 三个 abstract，笔记 A.12）：
//   `fn build_texture_atlas_data(&self, data: Option<TextureAtlasData>) -> TextureAtlasData`、
//   `fn build_armature(&self, pkg) -> (Box<dyn ArmatureProxy>, DisplayHandle)`、
//   `fn build_slot(&self, …) -> Slot`——阶段 3 内联 Null 语义
//   （NullArmatureProxy 注入 / DisplayHandle::Armature(idx) / NullSlotAdapter /
//   空壳 TextureAtlasData = jsdump NullFactory 行为），阶段 7 换 Bevy 实现时再泛型化。

// TODO P1-5（阶段 6）: replace_* 系列（均不在 buildArmature 路径上，笔记 A.11）：
//   - `replace_display(slot, display_data, display_index = -1)`（TS:720-748，@private，
//     注意 733-741 Image 数据但帧 raw 是 Mesh → 换 meshDisplay 的怪癖）
//   - `replace_slot_display(...)`（TS:783-796）
//   - `replace_slot_display_list(...)`（TS:800-821）
//   - `replace_skin(armature, skin, isOverride, exclude)`（TS:860-900，miss 回落
//     `skin.parent.defaultSkin`、isOverride 清帧、不写 replaceTextureData）
//   - `replace_animation(armature, armatureData, isOverride)`（TS:939-984，递归子骨架）
//   - `get_all_dragon_bones_data / get_all_texture_atlas_data`（TS:988-996，字段已 pub 可直读）
//   - `get_dragon_bones_data(name)`（TS:456-458）：`_dragonBonesDataMap` 按名取——
//     Rust 侧 `data_map.get(name)` 直读即可，包装方法待阶段 6 与 replace_* 一起补
//   - `get_texture_atlas_data(name)`（TS:545-547）：返回**数组**（同名多 atlas，
//     笔记 A.2-4）——Rust `atlas_map.get(name)` 直读，包装方法同批补
//   - `update_texture_atlases`（TS:424-433）、`clear/remove*`（TS:647-666 / 516-524 /
//     599-610）：数据对象删除 API 属 P1（阶段 6）；Armature 回收已由
//     `pending_recycles` 承担（D-5.6）。

#[cfg(test)]
mod runtime_clock_tests {
    use super::BaseFactory;
    use crate::animation::world_clock::{AnimatableHandle, ClockError, ClockId};
    use crate::armature::{
        Armature, ArmatureIdx, DisplayHandle, NullArmatureProxy, RuntimeCtxError, Slot,
    };
    use crate::model::DragonBonesData;
    use std::sync::Arc;

    fn insert_armature(factory: &mut BaseFactory, idx: ArmatureIdx) -> ArmatureIdx {
        let name = format!("clock-tree-test-{}", idx.0);
        let armature = Armature::new(
            idx,
            Arc::new(DragonBonesData::default()),
            name,
            Box::new(NullArmatureProxy::new()),
            DisplayHandle::Null,
        );
        if factory.armatures.len() <= idx.index() {
            factory.armatures.resize_with(idx.index() + 1, || None);
        }
        factory.armatures[idx.index()] = Some(armature);
        idx
    }

    fn link_child(factory: &mut BaseFactory, parent: ArmatureIdx, child: ArmatureIdx) {
        let slot = Slot {
            child_armature: Some(child),
            ..Slot::default()
        };
        factory.armatures[parent.index()]
            .as_mut()
            .expect("parent armature")
            .slots
            .push(slot);
    }

    fn armature_members(factory: &BaseFactory, clock: ClockId) -> Vec<AnimatableHandle> {
        factory
            .clocks
            .get(clock)
            .expect("clock exists")
            .active_members()
    }

    #[test]
    fn members_added_removed_and_self_removed_during_visit_obey_snapshot_boundary() {
        let mut factory = BaseFactory::new();
        let clock_id = factory.root_clock();
        let first = AnimatableHandle::Armature(ArmatureIdx(10));
        let removed = AnimatableHandle::Armature(ArmatureIdx(11));
        let added = AnimatableHandle::Armature(ArmatureIdx(12));
        let clock = factory.clocks.get_mut(clock_id).expect("root clock");
        clock.attach(first).expect("attach first");
        clock.attach(removed).expect("attach second");

        let mut visited = Vec::new();
        factory
            .visit_clock_members_with(clock_id, 1.0, |factory, handle, _| {
                visited.push(handle);
                if handle == first {
                    let clock = factory.clocks.get_mut(clock_id).expect("root clock");
                    assert!(clock.detach(removed), "remove member not yet visited");
                    clock.attach(added).expect("add member during visit");
                    assert!(clock.detach(first), "self-remove current member");
                }
            })
            .expect("visit stable initial member snapshot");

        assert_eq!(visited, [first]);
        assert_eq!(
            factory
                .clocks
                .get(clock_id)
                .expect("root clock")
                .active_members(),
            [added]
        );

        visited.clear();
        factory
            .visit_clock_members_with(clock_id, 1.0, |_, handle, _| visited.push(handle))
            .expect("visit next tick");
        assert_eq!(visited, [added]);
    }

    fn inconsistent_tree() -> (
        BaseFactory,
        ArmatureIdx,
        ArmatureIdx,
        ClockId,
        ClockId,
        ClockId,
    ) {
        let mut factory = BaseFactory::new();
        let parent = insert_armature(&mut factory, ArmatureIdx(0));
        let child = insert_armature(&mut factory, ArmatureIdx(1));
        link_child(&mut factory, parent, child);

        let old_clock = factory.create_clock(0.0);
        let rogue = factory.create_clock(0.0);
        let target = factory.create_clock(0.0);
        factory
            .attach_armature(old_clock, parent)
            .expect("attach parent tree");
        factory
            .attach_armature(rogue, child)
            .expect("move child to rogue clock");
        // 注入不一致：child 回指 rogue，但 rogue 成员表没有它。
        assert!(
            factory
                .clocks
                .get_mut(rogue)
                .expect("rogue clock")
                .detach(AnimatableHandle::Armature(child))
        );

        (factory, parent, child, old_clock, rogue, target)
    }

    #[test]
    fn failed_tree_attach_leaves_every_clock_relationship_untouched() {
        let (mut factory, parent, child, old_clock, rogue, target) = inconsistent_tree();

        let error = factory
            .attach_armature(target, parent)
            .expect_err("inconsistent descendant must fail prevalidation");
        assert_eq!(
            error,
            RuntimeCtxError::Clock(ClockError::InconsistentRelationship)
        );

        assert_eq!(factory.armature(parent).clock(), Some(old_clock));
        assert_eq!(factory.armature(child).clock(), Some(rogue));
        assert_eq!(
            armature_members(&factory, old_clock),
            vec![AnimatableHandle::Armature(parent)]
        );
        assert!(armature_members(&factory, target).is_empty());
    }

    #[test]
    fn failed_clear_clock_does_not_partially_detach_members() {
        let (mut factory, parent, child, clock, rogue, _target) = inconsistent_tree();

        let error = factory
            .clear_clock(clock)
            .expect_err("inconsistent descendant must abort the whole clear");
        assert_eq!(
            error,
            RuntimeCtxError::Clock(ClockError::InconsistentRelationship)
        );

        assert_eq!(factory.armature(parent).clock(), Some(clock));
        assert_eq!(factory.armature(child).clock(), Some(rogue));
        assert!(armature_members(&factory, clock).contains(&AnimatableHandle::Armature(parent)));
    }

    #[test]
    fn dispose_force_clears_inconsistent_clock_membership() {
        let (mut factory, parent, child, clock, rogue, _target) = inconsistent_tree();

        factory.dispose_armature(parent);
        factory.advance_time_with(0.0, 100.0);

        assert!(factory.armatures[parent.index()].is_none());
        assert!(factory.armatures[child.index()].is_none());
        assert!(factory.free.contains(&parent.0));
        assert!(factory.free.contains(&child.0));
        assert!(!armature_members(&factory, clock).contains(&AnimatableHandle::Armature(parent)));
        assert!(!armature_members(&factory, rogue).contains(&AnimatableHandle::Armature(child)));
    }

    #[test]
    fn direct_clock_advance_matches_nested_member_advance() {
        let mut nested = BaseFactory::new();
        let root = nested.root_clock();
        let nested_child = nested.create_clock(5.0);
        nested
            .attach_clock(root, nested_child)
            .expect("attach nested clock");
        nested.advance_time_with(0.25, 100.0);

        let mut direct = BaseFactory::new();
        let direct_child = direct.create_clock(5.0);
        direct
            .advance_clock(direct_child, 0.25)
            .expect("advance standalone clock");

        let nested_time = nested.clock(nested_child).expect("nested clock").time;
        let direct_time = direct.clock(direct_child).expect("direct clock").time;
        assert!((nested_time - direct_time).abs() < 1e-12);
        assert!((direct_time - 5.25).abs() < 1e-12);
        assert_eq!(
            direct.advance_clock(ClockId(99), 0.1),
            Err(ClockError::InvalidClockId)
        );
    }

    #[test]
    fn nested_clock_member_receives_scaled_time() {
        let mut factory = BaseFactory::new();
        let root = factory.root_clock();
        let child = factory.create_clock(1.0);
        factory.attach_clock(root, child).expect("attach child");

        factory.clock_mut(root).expect("root").time_scale = 0.5;
        factory.advance_time_with(1.0, 100.0);
        assert!((factory.clock(root).expect("root").time - 0.5).abs() < 1e-12);
        // 成员 clock 收到的是 root 缩放后的 dt（0.5），自身 timeScale=1 再加一次。
        assert!((factory.clock(child).expect("child").time - 1.5).abs() < 1e-12);
    }
}
