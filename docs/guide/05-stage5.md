# 阶段 5 实现指南：事件派发、对象生命周期与 P0 收口

> 配套 [`docs/requirements.md`](../requirements.md) §3 阶段 5。官方 TS 是行为规格，但不是 Rust 结构模板。
> 本阶段只把阶段 4 的“事件入队快照”换成“真实派发 + 回收”，并补齐对象生命周期语义；**事件产生条件的路径不动**。
> 行号以 `DragonBonesJS/DragonBones/src/dragonBones/` 当前 5.7 源码为准。若本文与官方 TS 冲突，以 TS 为准并修订本文。
> 阶段 4 的验收接缝（`flushed_events` / `take_flushed_events`）在本阶段删除；若测试仍依赖它，说明迁移未完成，不能用“保留旧 seam”掩盖。
> 预估 4–6 个工作日：事件/回收 2 天，测试迁移 1 天，矩阵工具与 harness 1.5–2 天，收口 0.5–1 天。

---

## 目录

- [0. 规则与已核实事实](#0-规则与已核实事实)
- [1. Rust 运行时设计决策](#1-rust-运行时设计决策)
- [2. 任务依赖与实施顺序](#2-任务依赖与实施顺序)
- [任务 1 — 事件接口拆分与派发](#任务-1--事件接口拆分与派发)
- [任务 2 — 延迟回收与 dispose 时序](#任务-2--延迟回收与-dispose-时序)
- [任务 3 — 测试接缝迁移与 recording proxy](#任务-3--测试接缝迁移与-recording-proxy)
- [任务 4 — JS 参考矩阵生成](#任务-4--js-参考矩阵生成)
- [任务 5 — Rust 矩阵 harness](#任务-5--rust-矩阵-harness)
- [任务 6 — BaseObject 收口与差异登记](#任务-6--baseobject-收口与差异登记)
- [任务 7 — Review 与 P0 验收](#任务-7--review-与-p0-验收)
- [建议的第一步](#建议的第一步)
- [常见错误速查](#常见错误速查)
- [完成后](#完成后)

---

## 0. 规则与已核实事实

### 0.1 阶段边界

**阶段 4 已交付**：事件产生条件（9 类事件）、`Sound` 的 listener 闸门例外、事件按帧入队、`take_flushed_events()` 快照、37 个动画 golden 与事件序列对拍。事件派发本身被显式推迟。

**阶段 5 交付**：

1. `IEventDispatcher` 的 Rust 形态与全局 `eventManager`；`SOUND_EVENT` 双发。
2. `DragonBones.advanceTime` 的“对象回收 → clock → 事件派发”三段真实语义。
3. 对象延迟回收（`bufferObject` → 下一帧 flush）与 `Armature.dispose` 的一帧延迟。
4. 交叉动画矩阵的 JS 参考数据与 Rust 对拍。
5. P0 全部 14 项验收（见 §任务 7）。

**不属于本阶段**（不要顺手做）：

- `replaceDisplay` / `replaceSkin` / `replaceSlotDisplay` / `replaceAnimation`（P1-5，阶段 6）。
- `removeDragonBonesData` / `removeTextureAtlasData` / `BaseFactory::clear` 等数据删除 API（P1 完整度，阶段 6；本阶段的 `pending_recycles` 只承载 Armature）。
- dbbin 二进制解析（P1-1，阶段 6）、Path 约束（P1-2）、BoundingBox（P1-3）、`cacheFrameRate` 写侧（P1-4）。
- Bevy 适配层（阶段 7）。

### 0.2 官方源码已核实的阶段 5 事实

| 编号 | 官方事实（位置） | 实现约束 |
|---|---|---|
| F5.1 | `DragonBones.advanceTime` 固定三段：先 flush `_objects`（逐个 `returnToPool()` 后清空）→ `_clock.advanceTime` → 派发 `_events`（DragonBones.ts:304-332） | 顺序不可交换；`dt == 0` 时 clock 内层早退，但回收与派发两段仍执行 |
| F5.2 | 事件派发前检查 `armature._armatureData !== null`；`null`（已回收）则整条事件跳过，**连 SOUND 也不进全局 eventManager**（:320-325） | Rust 以 arena 槽位是否仍为 `Some` 等价判断 |
| F5.3 | 派发顺序：先 `armature.eventDispatcher.dispatchDBEvent`，再仅当 `type === SOUND_EVENT` 调 `_eventManager.dispatchDBEvent`（:321-324） | 先骨架、后全局；仅 Sound 双发 |
| F5.4 | 派发循环条件每轮重读 `_events.length`（:316）；listener 在派发中新增的事件会在同一 `advanceTime` 内继续派发 | Rust 用 `VecDeque` 的 `while let Some = pop_front` 自然保序 |
| F5.5 | 派发后 `bufferObject(eventObject)`（:327），事件对象下一帧开头才 `returnToPool` | Rust 阶段 5 在派发调用栈结束后 drop；注册差异 D-5.3 |
| F5.6 | `bufferEvent` 按**对象身份** `indexOf` 去重（:334-338）；`bufferObject` 同样按身份去重（:340-344） | Rust 无对象身份，同值事件必须都保留（D-4.33 继续有效） |
| F5.7 | `Armature.dispose()`：若 `_armatureData !== null`，只做 `_lockUpdate = true` + `_dragonBones.bufferObject(this)`（Armature.ts:265-269） | Rust 的 dispose 必须延迟到下一帧，而不是就地清场 |
| F5.8 | `Armature._onClear`（Armature.ts:122-180）：先从 clock 移除；`bone/slot/constraint/action.returnToPool`；`animation.returnToPool`；`proxy.dbClear()`；最后 `_armatureData/_proxy/_clock = null`、`_lockUpdate = false` | 延迟路径在 flush 时按此顺序执行；子骨架递归清理也在此刻发生 |
| F5.9 | 已 dispose 的骨架在 flush 前仍保有 `_armatureData`；`advanceTime` 因 `_lockUpdate` 早退（Armature.ts:295-300） | `advance_one` 必须尊重 lock；flush 前查询仍能拿到骨架 |
| F5.10 | 单/多状态路径在 state fade 完成后立即从 `_animationStates` 移除，但 `bufferObject(animationState)` 下一帧才清场（Animation.ts:199, 238） | Rust `StateArena` 立即释放；无观察面，登记 D-5.2 |
| F5.11 | 事件对象来自 `BaseObject.borrowObject(EventObject)`；`_onClear` 复位 type/name/armature/bone/slot/actionData/data（EventObject.ts:185-277；BaseObject.ts:144-156） | Rust 用 `EventObject::default()` 作为 `_onClear` 等价；不引入全局池 |
| F5.12 | `BaseObject.returnToPool` = `_onClear()` + 按 `String(constructor)` 池键入池；`_isInPool` 防重复；默认上限 3000（BaseObject.ts:42-57, 181-184） | 不移植；arena/handle 承担复用，生命周期由延迟队列承担 |
| F5.13 | `setMaxCount` / `clearPool` 是公开静态 API，只影响缓存数量（BaseObject.ts:76-131） | 不移植；登记 D-5.1（无动画可观察行为） |
| F5.14 | `IEventDispatcher`：`addDBEventListener` / `hasDBEventListener` / `removeDBEventListener` / `dispatchDBEvent`（IEventDispatcher.ts:43-108）；`IArmatureProxy extends IEventDispatcher` | Rust 拆成 `EventDispatcher` 基础 trait + `ArmatureProxy: EventDispatcher` |
| F5.15 | `removeDBEventListener(type, listener, thisObject)` 按三元组删单个 | Rust 按 `ListenerId` 句柄删单个（D-4.30，已实现） |
| F5.16 | 生命周期事件在 `AnimationState` 的 fade 推进处借 EventObject 入队（AnimationState.ts:677, 713）；Frame/Sound 在 `TimelineState.ts:44-112`；子骨架 Play action 在 `Slot.ts:568` | 事件**创建点不动**；本阶段只改消费端 |
| F5.17 | Sound action 的入队不受 listener 闸门限制（TimelineState.ts:53）；start/loopComplete/complete/frameEvent 各自受闸门 | 阶段 4 已实现；本阶段验证“无 listener 时 Sound 仍派发到 eventManager” |
| F5.18 | `Animation` 每帧按 `timeScale`/`inheritTimeScale` 推进，并 reset blendStates（Animation.ts:172-200） | 阶段 4 已实现；矩阵覆盖 |
| F5.19 | `DragonBones` 构造时注入 `eventManager`（DragonBones.ts:298-302）；`advanceTime` 不调用 listener 以外的东西 | Rust `BaseFactory` 提供 `set_event_manager`；未设置时只有骨架派发 |
| F5.20 | `Armature._onClear` 内 `proxy.dbClear()`；`advanceTime` 收尾解锁后 `proxy.dbUpdate()`（Armature.ts:386） | 派发发生在 `dbUpdate` 之后；测试可用“dbUpdate 打标 → listener 读标”锁定 |

### 0.3 设计原则

1. **移植契约，不移植机制。** JS 池化的“分配复用”用 Rust arena/`Vec` 容量复用实现；“对象身份/防重复归还”由所有权消除；只有**延迟回收的可见时序**必须保留。
2. **唯一 owner 不变。** `BaseFactory` 继续持有 armature arena、clock arena、事件队列与回收队列。不新增第二个 Runtime owner，也不把 `&mut BaseFactory` 传给 listener。
3. **事件顺序不可优化。** 事件队列是 FIFO；派发中新增事件排到队尾并同帧派发；不得改用 `HashMap`、`sort` 或“本帧快照数组”。
4. **不引入 `Rc<RefCell<_>>`。** 监听器拿不到工厂，不是缺陷，而是本阶段明确的事件契约（见 §1.6）。

---

## 1. Rust 运行时设计决策

### 1.1 三层生命周期模型

| 层 | 负责对象 | Rust 机制 | 对应 JS |
|---|---|---|---|
| 结构层 | Armature、AnimationState、Clock、Bone、Slot、Constraint | arena + newtype handle + generation（D3） | 对象引用 + 池 |
| 延迟回收层 | 已 dispose 的 Armature | `pending_recycles: Vec<PendingRecycle>`，帧首 flush | `_objects` |
| 事件层 | EventObject | `VecDeque<EventObject>`，clock 后派发，派发后 drop | `_events` + EventObject 池 |

**不复刻**：`BaseObject` 的 `TypeId`/constructor 键池、`_isInPool`、`hashCode`、`setMaxCount`/`clearPool`。现有 [`core/base_object.rs`](../../crates/dragonbones-core/src/core/base_object.rs) 的池与 `ObjectBuffer` 在运行时**零调用**，阶段 5 删除并在任务 6 登记。

### 1.2 `BaseFactory` 字段

```rust
pub struct BaseFactory {
    // 已有：data_map / atlas_map / armatures / free / clocks / root_clock /
    //       parser / auto_search ...
    /// JS `_events`：本帧待派发（FIFO）。
    pending_events: VecDeque<EventObject>,
    /// JS `_objects`：下帧开头统一回收。
    pending_recycles: Vec<PendingRecycle>,
    /// JS `_eventManager`；SOUND 双发的第二站。
    event_manager: Option<Box<dyn EventDispatcher>>,
}

pub(crate) enum PendingRecycle {
    Armature(ArmatureIdx),
    // 阶段 6 若引入事件复用，可加 Event(EventObject)；阶段 5 不预留空变体。
}
```

删除 `flushed_events` 字段与 `take_flushed_events()`。`pending_events` 从 `Vec<EventObject>` 改为 `VecDeque<EventObject>`；`RuntimeCtx` 的构造参数同步改类型，`buffer_event` 改为 `push_back`。

### 1.3 全局推进顺序

```rust
fn advance_time_with_source(&mut self, passed_time: f64, now: &mut impl FnMut() -> f64) {
    // ① JS DragonBones.ts:305-311
    self.flush_recycles();

    // ② JS :313
    let mut clock_path = Vec::new();
    if let Err(error) = self.advance_clock_with(self.root_clock, passed_time, now, &mut clock_path) {
        log::warn!("root clock advance stopped: {error}");
    }

    // ③ JS :315-331
    self.dispatch_events();
}
```

三段都无条件执行：`dt == 0` 时 clock 内层早退，但第 ①、③ 段仍发生（与阶段 4 的 `dt=0` 快照语义一致，只是第 ③ 段从“替换快照”变为“派发”）。

`advance_one(idx, dt)`（单骨架直接推进）**不执行** ①/③：JS 的 `armature.advanceTime()` 也不 flush 对象、不派发事件。事件在 `advance_one` 中仍入队，等待下一次 `BaseFactory::advance_time` 派发。`advance_clock(clock, dt)`（直接推进某个 Clock 子树）同理只做第 ② 段，不 flush、不派发——它对应单独调用 `WorldClock.advanceTime`。

### 1.4 延迟回收

```rust
fn flush_recycles(&mut self) {
    let pending = std::mem::take(&mut self.pending_recycles);
    for item in pending {
        match item {
            PendingRecycle::Armature(idx) => self.dispose_armature_now(idx),
        }
    }
}

/// JS Armature.dispose（Armature.ts:265-269）：只上锁 + 入缓冲。
pub fn dispose_armature(&mut self, idx: ArmatureIdx) {
    let Some(armature) = self.armatures.get_mut(idx.index()).and_then(Option::as_mut) else {
        log::warn!("dispose_armature: armature {} not built or already recycled", idx.0);
        return;
    };
    if armature.lock_update {
        return; // 幂等：等价 JS bufferObject 的身份去重
    }
    armature.lock_update = true;
    self.pending_recycles.push(PendingRecycle::Armature(idx));
}
```

`dispose_armature_now` 复用现有 [`dispose_armature`](../../crates/dragonbones-core/src/factory/base_factory.rs) 的 BFS（自身 + 子骨架）与 clock detach 顺序，但**必须在 flush 时刻执行**，且：

- 若槽位已是 `None`（被同一批或更早清掉），跳过；
- 后代骨架若也在本批 `pending_recycles` 中，靠“槽位已空”天然幂等；
- 清场后 `free.push`，后续 `build_armature` 可复用该槽。

**可观察差异**（必须写测试）：

1. `dispose_armature(idx)` 后、下次 `advance_time` 前：`factory.armature(idx)` 仍可查询（JS 的 `_armatureData !== null`）；
2. 同一窗口内 `advance_one(idx, dt)` 因 `lock_update` 不推进；
3. 下次 `advance_time`（含 `dt = 0`）先 flush：槽位释放、clock 成员移除、`proxy.db_clear` 已调用；
4. 同一 idx 重复 dispose 只入队一次；flush 后再次 dispose 只 warn。

### 1.5 事件派发

```rust
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

        armature
            .proxy
            .dispatch_db_event(&event.event_type, &event);

        if event.event_type == EventObject::SOUND_EVENT
            && let Some(manager) = self.event_manager.as_mut()
        {
            manager.dispatch_db_event(&event.event_type, &event);
        }
        // 阶段 5 不池化：事件在本次派发调用栈结束后 drop（D-5.3）。
    }
}
```

要点：

- **`while let` 而不是 `for`**：listener 追加的事件在队尾，下一轮继续取；顺序与 F5.4 一致。
- **派发期间不持有 `&mut Armature` 跨 manager 调用**：派发用的 `event` 是 owned 值，`armature` 借用在 `proxy` 调用后结束；manager 借用独立。
- **`Sound` 双发只在骨架仍存在时发生**（F5.2 的整体跳过）。
- `has_db_event_listener` 闸门仍留在**入队侧**（阶段 4 已有），派发侧不再判断；否则 Sound 的无闸门例外会被二次过滤。

### 1.6 监听器契约（明确登记）

官方 `addDBEventListener` 的回调拿到的 `event.armature` 是可变的骨架对象引用，listener 可以在派发中同步 `play/fadeIn/dispose`，并因 F5.4 产生同帧新事件。

Rust 侧**不**把 `&mut BaseFactory` 交给 listener：

```rust
pub type DbEventListener = Box<dyn Fn(&EventObject)>;
```

因此：

- 监听器只能观察事件、向外部发送消息（例如写入 `Arc<Mutex<Vec<EventObject>>>`、Bevy `EventWriter`）；
- JS 风格“listener 内改动画/销毁骨架”在 core 契约中不可表达；Bevy 适配层应把事件转成 Bevy `Event`，由 system 在**后续调度**中执行 `play/fadeIn/dispose`；
- 派发循环仍按 F5.4 实现“派发中追加的事件同帧派发”——这条结构保证成本为零，且 `buffer_event` 的测试 seam 可以验证；
- 这是对官方扩展 API 的有意偏差 **D-5.4**，不影响 37 个 baseline golden 与矩阵数值（没有 golden 使用 listener 改状态）。

**不要**为了复刻 listener 变更能力引入 `Rc<RefCell<BaseFactory>>` 或全局 `unsafe`；若未来确实需要同步变更，正确方向是给回调增加窄 `EventContext`（可调用 `play/dispose`），单独立项，而不是换掉 arena owner。

### 1.7 `EventDispatcher` 与 `ArmatureProxy` 拆分

官方 `IArmatureProxy extends IEventDispatcher`（IEventDispatcher.ts:43-108）。Rust 当前把 8 个方法合并成 `ArmatureProxy`，本阶段拆开：

```rust
// event/i_event_dispatcher.rs（新建）
pub type DbEventListener = Box<dyn Fn(&EventObject)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

impl ListenerId {
    /// 适配层实现 `EventDispatcher` 时发号；进程内唯一，0 保留为“未发放”哨兵。
    /// 官方由对象池/实例内部计数，Rust 交给适配层（Bevy 实现也需要它）。
    pub fn next() -> Self;
}

pub trait EventDispatcher {
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId;
    fn remove_db_event_listener(&mut self, id: ListenerId);
    fn has_db_event_listener(&mut self, ty: &str) -> bool;
    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject);
}

// armature/i_armature_proxy.rs
pub trait ArmatureProxy: EventDispatcher {
    fn db_init(&mut self, armature: ArmatureIdx);
    fn db_clear(&mut self);
    fn db_update(&mut self);
    fn dispose(&mut self, dispose_proxy: bool);
}
```

- `ListenerId`、`DbEventListener` 从 `armature::i_armature_proxy` 迁到 `event`，并在旧路径 `pub use` 以减小调用点改动；迁移完成后可删除兼容 re-export。`NullArmatureProxy` 的本地发号改用 `ListenerId::next()`；**外部适配层必须能发号**，否则 Bevy 实现无法注册 listener。
- `BaseFactory` 增加：

```rust
pub fn set_event_manager(&mut self, manager: Box<dyn EventDispatcher>);
pub fn event_manager(&self) -> Option<&dyn EventDispatcher>;
pub fn event_manager_mut(&mut self) -> Option<&mut dyn EventDispatcher>;
```

- 不设置 manager 时，SOUND 只派发到骨架 proxy（等价于应用没有全局监听器）；不 panic。
- `NullArmatureProxy` 拆成两个 impl；测试用 `RecordingProxy` / `RecordingEventManager`（任务 3）。

### 1.8 P0-2 的映射与差异登记

P0-2 原文是“`BaseObject` 对象池语义（`borrowObject` / `returnToPool` / `_onClear` / `bufferObject` 缓冲回收队列）”，验收是“生命周期与 JS 等价，无提前释放”。本阶段按**行为**而不是**同名 API** 验收：

| P0-2 子项 | Rust 承载 | 验收测试 |
|---|---|---|
| `borrowObject` 复用 | armature free 栈 / `StateArena` slot 复用 / `Vec` 容量复用 | dispose 后再 build，槽位/代际正确 |
| `returnToPool` 清场 | `on_clear` + arena/`Vec` 释放 | 清场后无 clock 成员、无回指、无残留 |
| `_onClear` | 现有各类型 `on_clear`/`Default` | 阶段 3/4 已覆盖；补事件字段复位 |
| `bufferObject` 缓冲队列 | `pending_recycles` + 帧首 `flush_recycles` | dispose 一帧延迟、dt=0 也 flush |
| 无提前释放 | 事件在派发调用栈内有效；dispose 前仍可查询 | 派发时序 + dispose 窗口测试 |

新增/修订差异登记：

| 编号 | 内容 | 理由 |
|---|---|---|
| **D-5.1** | 不提供 `BaseObject` 全局 TypeId 池、`setMaxCount`/`clearPool`/`hashCode` 公共调参 | 运行时零调用；布局/身份由 arena + 所有权承担；无动画可观察行为 |
| **D-5.2** | `AnimationState` fade 完成后立即释放（官方下一帧 `bufferObject`） | Rust 事件不存 `animationState`（D-4.20），parent/child 引用已解除；`StateId` 代际失效使旧句柄不可再解引用 |
| **D-5.3** | `EventObject` 派发后即 drop（官方下一帧回收）；Armature action 队列里的 Play/Frame/Sound 事件处理完即 drop（官方 `returnToPool`） | 回调签名 `Fn(&EventObject)` 使引用不可逃逸；克隆是独立所有权；action 事件不逃出 `advanceTime`；无观察面 |
| **D-5.4** | listener 不能同步变更运行时（官方可用 `event.armature.animation` 改状态） | §1.6；Bevy 层用事件 + 后续 system 表达；不影响 golden |
| **D-5.5** | `bufferEvent`/`bufferObject` 的对象身份去重不移植 | 值语义无身份；同值事件必须保留（D-4.33）；dispose 幂等由 `lock_update` 保证 |
| **D-5.6** | 撤销 D-4.31（dispose 提前一帧） | 本阶段实现延迟回收后，D-4.31 关闭 |

`EventObject.animationState` 继续按 D-4.20 省略：TS 会写入该引用，但 core 与 golden 都不读；本阶段的事件消费者用 `(armature, bone/slot, name)` 已足够，不为此引入跨 `StateArena` 的生命周期引用。

---

## 2. 任务依赖与实施顺序

```text
任务 1（接口 + 派发）──┬─→ 任务 3（测试接缝迁移）
任务 2（回收 + dispose）┘
任务 3 ──→ 任务 4（JS 矩阵）──→ 任务 5（Rust 矩阵）
任务 1/2/3/5 ──→ 任务 6（BaseObject 收口 + 差异）──→ 任务 7（P0 验收）
```

先完成任务 1+2 并让阶段性测试（含迁移后的 37 个 golden）全绿，再投入矩阵生成；矩阵数据量大，最后做生成器与 harness 容易掩盖前面的顺序错误。

---

## 任务 1 — 事件接口拆分与派发

### 文件

| 文件 | 处理 |
|---|---|
| `src/event/i_event_dispatcher.rs` | 新建：`EventDispatcher`、`DbEventListener`、`ListenerId` |
| `src/event/mod.rs` | 导出新类型 |
| `src/armature/i_armature_proxy.rs` | `ArmatureProxy: EventDispatcher`；迁移 `DbEventListener`/`ListenerId`；`NullArmatureProxy` 拆分 impl |
| `src/factory/base_factory.rs` | `pending_events: VecDeque`、`event_manager`、`set_event_manager`、`dispatch_events`、删除 `flushed_events`/`take_flushed_events` |
| `src/armature/runtime_ctx.rs` | `pending_events: &mut VecDeque<EventObject>`；`buffer_event` 改 `push_back` |
| `tests/event_dispatch.rs` | 新建：派发顺序、Sound 双发、已回收骨架跳过、同值事件、listener 句柄 |

### 实作步骤

1. 建立 `EventDispatcher` 基础 trait；`ArmatureProxy` 改为 supertrait。`NullArmatureProxy` 的 4 个 listener 方法移到单独 impl；其余 4 个保留在 `ArmatureProxy` impl。由于方法定义移动到 supertrait，调用点（`factory` / `armature` / 测试）可能需要显式 `use crate::event::EventDispatcher as _;`。
2. `BaseFactory` 增加 `event_manager: Option<Box<dyn EventDispatcher>>`；`Default` 为 `None`。提供 `set_event_manager` 与只读/可变 getter。
3. `pending_events` 改 `VecDeque`；`RuntimeCtx` 构造与 `buffer_event` 同步改；所有事件创建点只需把 `buffer_event` 调用保持原样（内部 push_back）。
4. `advance_time_with_source` 第 ③ 段改为 `dispatch_events()`；删除 `flushed_events` 赋值与 `take_flushed_events`。
5. `advance_time(0.0)` 也要派发已缓冲事件（F5.1 三段无条件）。
6. 保留 `#[doc(hidden)] pub fn buffer_event` 作为测试注入 seam；文档改为“下一次 `advance_time` 派发”。

### 伪码

```text
advance_time(dt):
    flush_recycles()          # 任务 2；任务 1 可先保留空实现
    advance root clock
    while event = pending_events.pop_front():
        armature = arena[event.armature]?
        if armature is None: continue          # F5.2
        armature.proxy.dispatch(type, event)
        if type == SOUND: event_manager?.dispatch(type, event)
        # 事件在调用栈结束后 drop
```

### 测试卡

`tests/event_dispatch.rs`（用 recording proxy + recording event manager，见任务 3）：

1. **派发在 clock 之后**：proxy 的 `db_update` 写入一个标志，listener 回调读该标志必须为 `true`（F5.20）。
2. **Sound 双发顺序**：骨架 proxy 记录一次、eventManager 记录一次；顺序为先骨架后全局。
3. **无 armature listener 的 Sound**：骨架 proxy 无监听器，但 eventManager 仍收到（F5.17）；证明闸门在入队侧而非派发侧。
4. **已回收骨架跳过**：构造一条指向已 flush 骨架的注入事件（`buffer_event` + `armature: Some(disposed_idx)`），派发后任何 recorder 都不应收到，且 eventManager 也不收到（F5.2）。
5. **同值事件保留**：同一帧注入两条 `(type,name)` 完全相同的事件，两条都派发（D-4.33 / D-5.5）。
6. **listener 句柄**：`add` 两个 listener，`remove` 其中一个句柄，下一次派发只剩一个；重复 remove / 未发放句柄静默忽略（D-4.30）。
7. **dt=0 仍派发**：注入事件后 `advance_time(0.0)`，listener 收到；clock 成员 `current_time` 不前进（对照任务 2 的 flush 语义）。
8. **FIFO 顺序**：注入 A、B、C，收到顺序为 A、B、C。

> 不要在本任务测试“listener 内 play 产生新事件”——当前契约不允许（§1.6）。留到未来 `EventContext` 立项。

---

## 任务 2 — 延迟回收与 dispose 时序

### 文件

| 文件 | 处理 |
|---|---|
| `src/factory/base_factory.rs` | `PendingRecycle`、`pending_recycles`、`flush_recycles`、`dispose_armature` 延迟化、`dispose_armature_now`（原 BFS 逻辑）、删除无调用者的 `buffer_object()` 占位 |
| `src/armature/armature.rs` | 核对 `lock_update` 早退与 `on_clear` 复位；更新 D-4.31 注释 |
| `tests/lifecycle.rs` | 新建：dispose 窗口、flush、幂等、子骨架递归、dt=0 |
| `tests/world_clock.rs` | dispose 相关用例改为“先 dispose 再 `advance_time(0.0)` 才释放” |
| `src/factory/base_factory.rs` 单测 | dispose/回收队列顺序 |

### 实作步骤

1. 定义 `pending_recycles`；`dispose_armature` 只上锁 + 入队（F5.7）。原 BFS + `move_armature_clock(None)` + `on_clear` + `free.push` 整体移入 `dispose_armature_now`。
2. `flush_recycles` 在 `advance_time_with_source` 第一段调用；`flush_recycles` 内按入队顺序处理（官方 `_objects` 数组顺序）。
3. `dispose_armature_now` 在清场前 detach clock；若 `proxy.db_clear` 需要 arena 上下文，保持现有实现方式。
4. 处理“已 dispose 但尚未 flush”时的查询与推进：`armature(idx)` 正常返回；`advance_one` 因 `lock_update` 早退（核对 `Armature::advance_time` 第一行）。
5. `advance_one` 绝不调用 `flush_recycles` / `dispatch_events`（与 JS `armature.advanceTime` 对齐）。
6. 更新文档：删除 `dispose_armature` 方法头“时机差异 D-4.31”的说明，改为“官方同款两段式：dispose 上锁 + 入队；帧首 flush 清场”。

### 伪码

```text
dispose_armature(idx):
    armature = arena[idx]? else warn; return
    if armature.lock_update: return          # 幂等
    armature.lock_update = true
    pending_recycles.push(Armature(idx))

flush_recycles():
    batch = take(pending_recycles)
    for item in batch:
        match item:
            Armature(idx) => dispose_armature_now(idx)

dispose_armature_now(idx):
    collect descendants BFS from arena[idx] (自身 + slot.child_armature + display_frame 子骨架)
    for current in BFS 序:
        if arena[current] is None: continue
        detach clock
        on_clear()
        free.push(current)
```

### 测试卡

`tests/lifecycle.rs`：

1. **dispose 窗口**：build → dispose → 立即 `factory.armature(idx)` 可访问、`armature.name()` 不变；`advance_one(idx, dt)` 后姿态/`current_time` 不变。
2. **flush 释放**：`advance_time(0.0)` 后槽位为 `None`；`advance_one` 静默返回；clock 成员表不再包含该 idx（`clock_contains == false`）。
3. **幂等**：连续两次 `dispose_armature`，只产生一次清场；flush 后第三次 dispose 只 warn，不 panic。
4. **子骨架递归**：父骨架带 child 时 dispose 父，flush 后父子都释放；子骨架 clock 关系移除，无 stale membership。
5. **build 复用槽位**：dispose + flush 后 `build_armature` 可复用 `free` 槽；旧 `ArmatureIdx` 在复用后仍指向新骨架（arena 无代际）→ 这是阶段 3 既有语义，测试只锁定“释放后可复用”，不假装 idx 有代际。
6. **事件与回收顺序**：在 dispose 后注入一条指向该骨架的事件，`advance_time(0.0)` 先 flush 再派发 → 事件被跳过（与任务 1 的 F5.2 测试互补，但这里验证 flush 先行）。
7. **未挂 clock 的骨架**：dispose 后 flush 不 panic，`clock=None` 保持。

> `ArmatureIdx` 本身没有 generation（阶段 3 决策）；不要在本阶段顺手改成带代际的 handle，否则会扩散到所有 API 测试。

---

## 任务 3 — 测试接缝迁移与 recording proxy

### 目标

删除 `take_flushed_events()` 后，所有原“读快照”的测试必须改为“读 listener 收到的派发序列”，同时保持原有断言强度。

### 文件

| 文件 | 处理 |
|---|---|
| `tests/common/mod.rs` | 新建 `RecordingProxy`（实现 `ArmatureProxy` + `EventDispatcher`）、`RecordingEventManager`、`drain_events()` helper |
| `tests/golden_anim.rs` | `build_golden_fixture` 改用 recording proxy；9 类 panic probe 改为记录器；删“未 dispatch”断言 |
| `tests/golden_events.rs` | 读 golden JSON，不变 |
| `tests/world_clock.rs` | 事件快照用例改为派发次数/顺序断言；`global_advance_zero_flushes_events...` 改名与语义更新 |
| `tests/animation_acceptance.rs` | Sound 用例改为“无骨架 listener 时 eventManager 收到 Sound” |
| `src/factory/base_factory.rs` 单测 | 事件 append/替换语义改为派发语义 |
| `src/armature/runtime_ctx.rs` 单测 | `buffer_event` 改为 `VecDeque` 断言 |

### `RecordingProxy` 设计

```rust
#[derive(Clone, Default)]
pub struct EventLog(Arc<Mutex<Vec<EventObject>>>);

pub struct RecordingProxy {
    log: EventLog,
    listeners: HashMap<String, Vec<(ListenerId, DbEventListener)>>,
}

impl EventDispatcher for RecordingProxy {
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
        let id = ListenerId::next();
        self.listeners.entry(ty.to_owned()).or_default().push((id, listener));
        id
    }
    fn remove_db_event_listener(&mut self, id: ListenerId) {
        for listeners in self.listeners.values_mut() {
            listeners.retain(|(candidate, _)| *candidate != id);
        }
    }
    fn has_db_event_listener(&mut self, ty: &str) -> bool {
        self.listeners.get(ty).is_some_and(|listeners| !listeners.is_empty())
    }
    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
        if let Some(listeners) = self.listeners.get(ty) {
            for (_, listener) in listeners {
                listener(event);              // 保持真实 listener 调用语义
            }
            self.log.0.lock().unwrap().push(event.clone()); // 测试同时记录
        }
    }
}
```

要点：

- **`has_db_event_listener` 必须按事件类型真实返回**，否则会错误关闭 Frame/lifecycle 闸门；注册 9 类事件即等价阶段 4 的 9 个 probe。实现时还需补齐 `ArmatureProxy` 的 `db_init/db_clear/db_update/dispose`（除 `db_update` 计数外均为 no-op）。
- 记录发生在 `dispatch_db_event`，不是 `add_db_event_listener` 时；这样“没有 listener 就不记录”与“闸门按类型”同时成立。
- 记录器 clone 事件；测试每帧 `drain` 后与 golden 比较，clone 不影响被派发对象。
- 另记 `db_update()` 调用次数/序号（`Arc<AtomicUsize>`），供任务 1 的“派发发生在 dbUpdate 之后”断言使用。
- `RecordingEventManager` 同构，只记录 SOUND；用于验证双发。
- 生成 `build_golden_fixture` 时把 `EventLog` clone 返回给测试，测试每帧 `advance_time` 后 `drain` 并与 golden `frames[i].events` 比较。

### 迁移判定

- `rg "take_flushed_events|flushed_events" crates/` 必须为 0；
- 37 个 golden 逐帧事件序列仍与既有 `testdata/golden/**` 完全一致（**不重新生成** golden；若不一致，说明派发顺序与阶段 4 的入队顺序不等价，先查 §1.5 的 FIFO/闸门）；
- 原 `golden_anim` 的“listener 不被调用”断言删除，替换为“listener 恰好被调用 N 次、序列等于 golden”。

> 本任务是“证明阶段 4 的语义在真实派发下不变”的关键；不要为了省事保留快照 seam 与派发两套路径。

---

## 任务 4 — JS 参考矩阵生成

### 目标

阶段 4 的 37 个 baseline 只覆盖“单动画从头播放”。交叉淡入、layer 叠加、同名重播、双骨架、dispose 时序需要 JS 参考数据。矩阵数据必须**紧凑**，不能把 37MB 的 baseline 复制 9 份。

### 文件

| 文件 | 处理 |
|---|---|
| `tools/dbharness.mjs` | 从 `jsdump.mjs` 抽出可复用部分：PIXI 桩、`NullAtlas/NullSlot/NullFactory/EvProxy`、`installRecorder`、`snapshotPose`、`EVENT_TYPES` |
| `tools/jsdump.mjs` | 改为调用 `dbharness.mjs`；输出必须与现有 golden **逐字节等价**（见验收） |
| `tools/matrix.mjs` | 单场景 runner：接收 case JSON（scenario/asset/anim 组合），输出一个 compact golden |
| `tools/gen_matrix.mjs` | 遍历 `tools/matrix_cases.mjs` 生成 `testdata/matrix/**` |
| `tools/matrix_cases.mjs` | 场景 × 资产 × 动画的声明式配置（唯一事实来源） |
| `testdata/matrix/**` | 生成的矩阵黄金数据（入库；体积预算见下） |

### compact schema

```jsonc
{
  "meta": {
    "scenario": "fade_cross",
    "asset": "Dragon",
    "armature": "Dragon",
    "frameRate": 24,
    "dt": 0.041666666666666664,
    "frameCount": 120,
    "checkpoints": [0, 24, 48, 72, 96, 119],
    "comparePose": true,
    "boneNames": ["..."],
    "slotNames": ["..."]
  },
  "frames": [
    {
      "i": 0,
      "states": [
        {
          "armature": 0,
          "name": "walk",
          "layer": 0,
          "weight": 1.0,
          "weightResult": 1.0,
          "currentTime": 0.0,
          "fadeProgress": 0.0,
          "playing": true,
          "completed": false
        }
      ],
      "events": [
        { "armature": 0, "type": "fadeIn", "name": "walk", "bone": null, "slot": null }
      ],
      "pose": {
        "bones": { "root": [13 个 f64] },
        "slots": { "slot": [15 个 f64] }
      }
    }
  ]
}
```

规则：

- `states` 与 `events` **每帧**记录；`pose` 只在 `meta.checkpoints` 帧出现（`comparePose=false` 的场景完全省略）。
- `states[].armature` / `events[].armature` 用“本场景内创建顺序索引”（单骨架恒 0，双骨架 0/1）。
- **脚本时序**：场景脚本在“该帧 `advanceTime` 之前”执行；golden 的帧 `i` 是第 `i` 次 advance 之后的采样。生成器与 Rust harness 必须共用这条约定，避免差一帧。
- f64 直接 JSON 数字；`f32` 量化路径（dbbin）本阶段不涉及。
- 生成器必须确定性：同一输入重复运行 `git diff --exit-code testdata/matrix` 为空。

### 场景与覆盖

| scenario | 脚本（120 帧，帧率 24） | 覆盖资产/动画 | pose | 覆盖点 |
|---|---|---|---|---|
| `fade_cross` | 播 A 30 帧 → `fadeIn(B, 0.2, -1, 0, "", All)` → 90 帧 | 7 组配对（见下） | ✔ | fade 状态机、交叉权重 |
| `layer_overlay` | `fadeIn(A, 0, -1, 0, "g1")` 与 `fadeIn(B, 0, -1, 0, "g2")`（各自返回 state 后设 `resetToPose=false`）→ 15 帧 → 105 帧 | 同 7 组 | ✔ | 同层不同 group 叠加、BlendState 首写者/叠加顺序 |
| `reset_to_pose` | 播 A 30 帧 → `fadeIn(B, 0.2, -1, 0, "", All)` 且 `resetToPose=true` → 90 帧 | 7 组 | ✔ | 未覆盖目标复位 |
| `replay` | 播 A 30 帧 → `play(A)` → 90 帧 | 37 个动画全部 | ✘ | 同名重播、All 淡出 |
| `event_dense` | 播 A 全 120 帧 | 37 个动画全部 | ✘ | 事件时序（含 Sound/Frame/lifecycle） |
| `goto_roundtrip` | `gotoAndStopByFrame(A, 6)` 5 帧 → `gotoAndPlayByFrame(A, 6)` 5 帧（`frameCount=10`） | 9 个代表动画 | ✔（检查点 0/5/9） | 时间冻结/恢复 |
| `timescale` | state `timeScale=2` 播 60 帧 | 9 个代表动画 | ✔（6 检查点） | 时间缩放 |
| `dual_armature` | 同一数据 build 两个骨架，挂同一 root clock，分别播 A/B 120 帧（单动画资产两实例播同一动画） | 9 个资产 | ✔ | 多 animatable/clock |
| `dispose` | 播 A 30 帧 → `armature.dispose()` → 30 帧 | 9 个资产 | ✘ | 回收时序、事件跳过 |

7 组配对（资产 → A/B）：`Dragon`（walk/stand）、`Dragon_Blend`（walk/jump）、`PlantAnimate`（Shoot/Idle）、`PlantAnimate_compressed`（Shoot/Idle）、`Rooster_Ani`（rooster_run_anim/rooster_idle_anim）、`Rooster_Blend`（同）、`Rooster_P1`（同）。`龙` 与 `Surface_P1_4` 只有单动画，只进入 `replay/event_dense/timescale/dual/dispose`。

**覆盖口径**：requirements 的“9 素材 × 全动画 × 全用例”按笛卡尔积是约 3000 个 120 帧转储。矩阵按**语义覆盖**执行：`replay`/`event_dense` 覆盖全部 37 个动画；需要“两个动画”的场景覆盖全部可配对资产（7 组）＋单动画资产的退化用例；`goto`/`timescale` 取 9 个代表动画。理由：状态机语义由场景而不是素材决定，重复转储只增加体积；若 review 要求扩大，优先把 `fade_cross` 扩到全部可配对动画，而不是复制 `event_dense`。

**体积预算**：`testdata/matrix` 目标 ≤ 20 MB。若超预算，优先把 `replay/event_dense` 的 `pose` 保持省略、减少 `checkpoints` 到 4 个，而不是删场景。

### JS runner 要点

- 复用 `jsdump` 的 PIXI 桩与 `EvProxy`；事件记录器**直接作为 `addDBEventListener` 回调**（不需要 DragonBones 内部 `_events` 快照），因此记录的是真实派发。
- `states` 采集：遍历 `armature.animation._animationStates`（TS `private`，仅测试工具读取；`jsdump.mjs` 已直接访问 `_colorTransform`/`_globalAlpha` 等 `@internal` 字段，属同类做法），读取 `name/layer/weight/currentTime/isPlaying/isCompleted` 与 `_fadeProgress/_weightResult`（后两者是 TS `@internal public`，Rust 对应 `fade_progress/weight_result`）。不要为了采集去改官方 bundle。
- 双骨架场景需要两个 armature 实例，但 `NullFactory._buildArmature` 每次 new `EvProxy`；记录器要按 armature 分别挂 9 类 listener，并在 `states` 中标注索引。
- `dispose` 场景：在指定帧调用 `armature.dispose()`，**不要**手动清空对象；观察后续帧的 `states`（应为空）与事件（不再出现）。
- `goto_roundtrip` 的“同帧恢复”比较：`gotoAndStopByFrame(A, 6)` 后取一次 `advanceTime(0)` 的 pose；再 `gotoAndPlayByFrame(A, 6)` 后同样 `advanceTime(0)` 取 pose，二者必须相等（等价阶段 4 的 `advance_one(0)` 往返测试）。
- `gen_matrix.mjs` 复用 `gen_golden.mjs` 的 spawn 结构；失败退出码非 0。

### 验收

1. 重构后的 `jsdump.mjs` + `gen_golden.mjs` 重跑 37 个 golden，与仓库现有文件 **git diff 为空**。
2. `node tools/gen_matrix.mjs` 成功、耗时可接受、重复运行零 diff。
3. `testdata/matrix` 体积 ≤ 20MB；每个 scenario 至少覆盖上表资产。
4. 抽查 `fade_cross/Dragon`：30 帧后短时间内出现两个 state，二者 `weightResult` 互补（淡入期间和 ≈ 1.0），淡入完成后只剩一个 `weight=1.0` 的 state。

---

## 任务 5 — Rust 矩阵 harness

### 文件

| 文件 | 处理 |
|---|---|
| `tests/golden_matrix.rs` | 新建：按 scenario 驱动、逐帧 states/events、检查点 pose |
| `tests/common/mod.rs` | 矩阵读取与元数据 helper（可复用 `golden_path` 风格，新增 `matrix_path`） |
| `src/animation/player.rs` | 需要时暴露只读状态查询：`states()` 已有；确认 `weight_result/fade_progress/is_playing/is_completed` 可读 |
| `src/factory/base_factory.rs` | 双骨架/clock attach 的公开 API 已够用；不新增测试专用 API |

### harness 结构

```rust
fn run_scenario(case: &MatrixCase) {
    let mut factory = build_factory(case.asset);
    let log = EventLog::default();
    let mut armatures = vec![build_armature(
        &mut factory,
        case.asset,
        RecordingProxy::new(log.clone()),
    )];
    if case.dual_armature {
        armatures.push(build_armature(
            &mut factory,
            case.asset,
            RecordingProxy::new(log.clone()),
        ));
        // 两个都 attach 到同一个 root clock
    }
    install_probes(&mut factory, &armatures, &log); // 9 类 listener 都注册以打开闸门

    for step in 0..case.frame_count {
        apply_script(case, step, &mut factory, &armatures); // 到帧点执行 play/fadeIn/goto/dispose
        factory.advance_time(case.dt);
        let got_events = log.drain();
        compare_events(&golden.frames[step], &got_events, &armatures);
        compare_states(&golden.frames[step], &factory, &armatures);
        if golden.is_checkpoint(step) {
            compare_pose(&golden.frames[step], &factory, &armatures);
        }
    }
}
```

### 逐场景断言

| scenario | 断言 |
|---|---|
| `fade_cross` | states 的 name/layer 精确；weight/weightResult/currentTime/fadeProgress `1e-9*(1+|want|)`；事件序列 `(type,name,bone,slot)` 全等；检查点 pose 全骨/槽 13/15 值 |
| `layer_overlay` | 两个 state 同时存在；同一 BlendState 目标“首写者重置、次写者按 `weightResult` 叠加”；事件序列；检查点 pose |
| `reset_to_pose` | 未覆盖骨/槽回到数据姿态（与阶段 4 的 `resetToPose` 测试口径一致）；状态时间；事件 |
| `replay` | state 被替换后 `play_order` 只剩新 state；`fadeOut` 事件在正确帧；事件序列 |
| `event_dense` | 每帧事件序列全等；特别是 `frameEvent` 与 `soundEvent` 的先后 |
| `goto_roundtrip` | 冻结帧 states/pose 不变；恢复帧与 golden 一致 |
| `timescale` | state `timeScale=2` 下 60 帧 states/pose 与 JS 参考一致；事件帧号按缩放后时间 |
| `dual_armature` | 两个 armature 的 states/events 分别对拍；clock 推进顺序一致 |
| `dispose` | dispose 帧之后该 armature 的 states 为空；事件不再出现；另一骨架继续推进（若场景包含） |

### 容差与报告

- 数值：`1e-9 * (1 + |want|)`；事件/枚举/索引/布尔：精确相等。
- 每个 scenario 输出最大绝对/相对 pose 误差、事件总数、首个不一致帧；
- 失败信息包含 `scenario/asset/frame/target/index`，便于定位。

### 负向测试

- 人工改一条 golden 的 weight 或事件顺序，确认 harness 会失败（不提交改动，仅在实现期验证断言不是空转）。
- `comparePose=false` 的场景必须真的不读 `pose`（用缺失字段的 golden 跑一次，确认不 panic）。

---

## 任务 6 — BaseObject 收口与差异登记

### 文件

| 文件 | 处理 |
|---|---|
| `src/core/base_object.rs` | 删除 TypeId 池、`ObjectBuffer`、`Poolable`/`PoolExt`、`borrow_object`/`pool_*` |
| `src/core/mod.rs` | 移除导出 |
| `tests/pool.rs` | 删除：池 API 本身随 D-5.1 移除；生命周期行为由 `tests/lifecycle.rs` 覆盖 |
| `src/event/event_object.rs` | 模块头更新：`Default` 即 `_onClear`；不再提“阶段 5 进池” |
| `src/armature/slot.rs` / `src/factory/base_factory.rs` / `src/animation/player.rs` | 清理“TypeId 池/阶段 5 接池”注释，改为 arena/回收队列指针 |
| `docs/model_mapping.md` | 更新 D-4.1/D-4.2/D-4.3 的“池”表述；标注 D-4.31 已由 D-5.6 关闭；补 D-5.1~D-5.6 |

### 实作步骤

1. 全仓 `rg "BaseObject|ObjectBuffer|borrow_object|return_to_pool|pool_"` 确认除 `base_object.rs`/`pool.rs`/注释外无引用。
2. 删除模块与测试；`cargo check --workspace` 确认无残留。
3. 更新注释与 `model_mapping.md`；每条差异写清：官方行为、Rust 做法、理由、测试证据。
4. 保留 `EventObject` 的 `Default` 文档为“等价 `_onClear`”，不要为了“池复用”改字段类型或加 `clear()`；阶段 6 若 profiling 证明事件分配是热点，再引入 typed `Vec<EventObject>` 复用，并单独登记。

### 验收

- `rg "BaseObject|ObjectBuffer" crates/dragonbones-core/src` 为 0（注释中的历史说明改为“见 D-5.1”，不再引用已删类型）；
- `cargo test --workspace` 全绿；
- `docs/model_mapping.md` 中 D-4.1/2/3、D-4.31、D-5.x 自洽，无“池仍存在”的旧描述。

---

## 任务 7 — Review 与 P0 验收

### Review 清单

1. `advance_time` 三段顺序、`dt=0` 行为、`advance_one` 不 flush/不派发。
2. 派发循环：FIFO、跳过已回收骨架、Sound 双发顺序、空 `event_manager`。
3. `dispose` 窗口与 flush 顺序；`lock_update` 幂等；子骨架递归；clock detach。
4. 事件闸门仍只在入队侧；Sound 例外未被派发侧二次过滤。
5. 全部 `take_flushed_events`/`flushed_events` 已删除；测试使用 recording proxy。
6. 矩阵数据可复现；容差与失败信息符合；负向测试有效。
7. 已删除的池没有残留引用；`model_mapping.md` 差异自洽。

### 完整验收命令

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node tools/gen_pose.mjs
node tools/gen_golden.mjs        # 结果必须与入库 golden 零 diff
node tools/gen_matrix.mjs        # 结果必须与入库 matrix 零 diff
```

### P0 验收表

| # | 验收项 | 证据 |
|---|---|---|
| P0-1 | geom 5 类 | `tests/geom_golden.rs`（阶段 1） |
| P0-2 | 对象生命周期/缓冲回收 | `tests/lifecycle.rs` + `tests/event_dispatch.rs` + D-5.1~D-5.6 |
| P0-3 | 全部枚举 | `tests/enums.rs` |
| P0-4 | model 字段 | `tests/model_defaults.rs` |
| P0-5 | JSON → IR | `tests/ir_golden.rs` |
| P0-6 | 骨架层级变换 | `tests/pose_golden.rs` |
| P0-7 | Slot 全逻辑 | `tests/pose_golden.rs` + `golden_anim` |
| P0-8 | 16 类 TimelineState | `golden_anim` 37×120（阶段 4） |
| P0-9 | Animation/AnimationState 状态机 | `golden_matrix` fade/layer/replay/timescale |
| P0-10 | mesh 变形 | `golden_anim` 含 mesh 素材 + `pose_golden` |
| P0-11 | 事件系统（产生/入队/派发） | `tests/event_dispatch.rs` + `golden_matrix/event_dense` |
| P0-12 | WorldClock + 顶层驱动（回收/clock/派发顺序） | `tests/world_clock.rs`（迁移后）+ `tests/lifecycle.rs` |
| P0-13 | BaseFactory 装配 | `tests/armature_unit.rs` + build 路径 |
| P0-14 | 差分测试基建 | 37 baseline + matrix 全绿；`gen_golden`/`gen_matrix` 可复现 |

### 交付摘要

- 逐 scenario / 逐资产结果：帧数、事件数、最大绝对/相对 pose 误差；
- Clock/dispose 行为矩阵；
- 事件派发矩阵（骨架/全局、Sound 双发、跳过已回收）；
- P0 14 项逐项证据；
- D-5.1~D-5.6 差异登记；
- 已知未覆盖：listener 同步变更（D-5.4）、事件池（D-5.3）；两者都不影响本阶段 golden。

---

## 建议的第一步

**1A：事件接口与派发骨架（先不接回收）**

1. 新建 `event/i_event_dispatcher.rs`：`EventDispatcher`、`ListenerId`（含 `next()`）、`DbEventListener`；
2. `ArmatureProxy: EventDispatcher`，`NullArmatureProxy` 拆 impl；
3. `pending_events` 改 `VecDeque`，`advance_time` 尾段改 `dispatch_events`，删除 `flushed_events`/`take_flushed_events`；
4. 写 `tests/event_dispatch.rs` 的 FIFO / Sound 双发 / `dt=0` 三条；
5. 此时 37 golden 的事件断言会因接缝删除而编译失败——先做任务 3 的 `RecordingProxy` 再迁 golden，不要临时加回快照 seam。

**1B：回收与 dispose**：`PendingRecycle` + `flush_recycles` + `dispose_armature` 延迟化；跑 `tests/lifecycle.rs` 与迁移后的 `world_clock`。

**1C：矩阵**：先做 `dbharness.mjs` 重构并用“37 golden 零 diff”锁住，再做 `matrix.mjs`/`gen_matrix.mjs`，最后写 Rust harness。

每个子卡以“测试描述预期 → 实现 → `cargo test --workspace` + clippy”闭环；事件与回收的顺序错误会污染后面所有数值，不要跳过 1A/1B 直接做矩阵。

---

## 常见错误速查

| 症状 | 根因 | 正确做法 |
|---|---|---|
| 事件在 `advance_time` 结束前被清空/丢失 | 用 `for` 遍历快照或提前 `clear()` | `while let pop_front`；派发完再自然 drop |
| listener 只收到第一帧事件 | 派发后忘记继续取队列 | F5.4：队尾追加也要同帧派发 |
| Sound 没有进全局 manager | 骨架已被 flush 清理（F5.2 整条跳过） | 先确认骨架仍在 arena；或场景本身合法地跳过 |
| Sound 被派发两次到骨架 | 把入队闸门和派发当同一层 | 闸门只在入队侧；派发侧不再过滤 |
| dispose 后同帧查询 panic/找不到骨架 | dispose 仍就地清场 | 延迟到 `flush_recycles`；`armature(idx)` 在窗口内仍可用 |
| dispose 后 `advance_one` 仍推进 | `lock_update` 未设置/未检查 | dispose 只上锁；`Armature::advance_time` 首行早退 |
| `dt=0` 时事件不派发/对象不回收 | 把三段塞进 clock 的 0 早退 | 0 早退只在 clock 内层（F5.1） |
| 矩阵 golden 体积失控 | 每帧存全量 pose | 只有 `checkpoints` 存 pose；`replay/event_dense` 不存 pose |
| listener 内 play 编译不过 | 回调只有 `&EventObject` | 这是 D-5.4 的明确契约；事件转给 Bevy/外部驱动 |
| `take_flushed_events` 残留 | 迁移未完成 | `rg` 清零；用 recording proxy |

---

## 完成后

1. 全量命令通过（§任务 7）。
2. 提交内容：事件派发、延迟回收、矩阵数据与 harness、BaseObject 删除、`model_mapping.md` 差异更新、本指南勘误（若有）。
3. 阶段 5 review 通过后，再编写阶段 6 指南（dbbin、Path、BoundingBox、cache、replace API）。
