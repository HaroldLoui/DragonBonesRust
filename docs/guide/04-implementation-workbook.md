# 阶段 4 实现工作簿（源码对照版）

> 本文是 [`04-stage4.md`](04-stage4.md) 的开发拆分，不替代它。
> 表里的 Rust 签名是建议的落点；伪码描述可观察顺序，不是可以直接编译的代码。
> 官方行为引用均指仓库内 `DragonBonesJS/DragonBones/src/dragonBones/` 的 5.7 源码。
> 若本文与官方 TS 或 `04-stage4.md` 冲突，以两者为准，并修订本文。

## 开发顺序与工作方式

严格按原指南的任务顺序推进：

```text
任务 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9
```

每个子项先写行为测试，再实现；先合成小输入锁定边界，再跑相关素材 golden。每完成一个子项，运行对应测试和 `cargo test --workspace`。任务 1 是架构地基，完成它并确认阶段 0–3 回归全绿后才进入动画状态机。

本工作簿把两类内容分开：

- **TS 已核实行为**：带官方文件/行号，可作为实现断言。
- **Rust 建议结构**：可以调整，但必须用测试证明行为相同；不要把建议类型误认为 TS 结构。

---

## 任务 1 — Clock arena、关系和运行时驱动

### 1.1 先定唯一 owner

**推荐由现有 `BaseFactory` 继续作为唯一 runtime owner**：它已经拥有 `armatures` arena；阶段 4 在同一处增加 Clock arena、根 Clock handle 和事件队列。不要同时引入另一个拥有 Armature arena 的 `Runtime`。

这个选择对应现状与规格：

- 当前 `BaseFactory` 是 armature arena owner（`factory/base_factory.rs:78-104`）。
- 官方 `DragonBones` 持有唯一默认/root `WorldClock` 和对象/事件队列（`core/DragonBones.ts:286-353`）。Rust 可以把这些运行时资源集中由 `BaseFactory` 持有；`BaseFactory::advance_time` 编排官方三段顺序。`core::DragonBones` 的静态常量不需要因此再变成第二个 owner。
- `WorldClock` 只保存成员 handle、parent handle 和时间字段；不拥有 Armature，不持 `BaseFactory` 引用，不用 trait object 保存 animatable。

**建议在任务 1 开始就确定的字段：**

```rust
pub struct BaseFactory {
    // 已有字段：data_map、atlas_map、armatures、free、parser……
    clocks: ClockArena,
    root_clock: ClockId,
    pending_events: Vec<EventObject>,
    flushed_events: Vec<EventObject>,
}
```

因此 `BaseFactory::default/new` 需要显式创建 root clock（`time = 0.0`，`system_time = 当前墙钟秒数`）。`build_armature` 仍不自动把新 Armature 挂上 root clock；调用方显式 attach。

### 1.2 官方时钟事实（先按这几条写测试）

来源：`animation/WorldClock.ts:49-83,97-154,168-254`，以及 `core/DragonBones.ts:293-353`。

1. `WorldClock(time = 0)` 的 `time` 是累计动画时间（秒）；构造时另用 `Date.now() * 0.001` 初始化 `_systemTime`。二者不是同一个量。
2. `advanceTime` 先把 NaN 改成 0，读取当前墙钟；负 `passedTime` 用 `currentTime - _systemTime` 替换；然后刷新 `_systemTime`；只有 `timeScale !== 1` 才乘 `timeScale`；缩放后等于 0 才早退。
3. 非零时钟更新：负 passedTime 执行 `time -= passedTime`，非负执行 `time += passedTime`；成员收到的 passedTime 保留符号。
4. 成员循环的 `l` 在循环开始时固定为当时的数组长度。循环中新追加的成员不进入本轮；已在列表中的成员被 remove 后变 null，轮到它时跳过；循环之后才压缩空位。
5. `add` 按对象身份幂等；`remove` 把本轮成员位置设 null 并清 back-reference；`clear` 对每个直接成员清除其 clock back-reference。若成员是子 Clock，只解除它与 parent Clock 的关系，不会清空子 Clock 自己的成员。
6. `contains` 是沿 `value.clock` 向上查祖先，不等价于只查直接成员；`value === this` 返回 false。
7. 官方允许尝试创建 Clock 自环/环，但不做递归保护；递归推进环会无限递归。Rust 必须防栈溢出，并在差异登记里说明采用的保护策略。
8. 官方 `DragonBones.advanceTime` 顺序固定为对象池 flush → root clock advance → event flush/dispatch。阶段 4 只实现顺序和可观测 flush 快照，不调用 listener。

**时间源细节：**每个官方 WorldClock 自己在 `advanceTime` 调用时取一次 `Date.now()`。公开 `advance_time` 应在每次 clock dispatch 时读取墙钟。指南允许提供 `advance_time_with(passed_time, now)` 的固定 now 测试 seam；嵌套 Clock 的正 dt 测试可用同一个 now，若要验证嵌套负时间 sentinel 的逐次采样，则另加可控 `FnMut() -> f64` 时间源 seam。不要把构造参数 `time` 当成 now。

推荐 API 分层：正常 `advance_time(passed_time)` 内部读取 `SystemTime` 墙钟秒数；纯时间算法放在 `advance_time_with(passed_time, now)`，由确定性测试传入 now。若集成测试要直接调用该 seam，用 `pub` + `#[doc(hidden)]`，或把纯算法测试放在同模块私有单测。对嵌套 Clock，生产路径应在每次 Clock 调用时读取一次 now；不要把一次采样的 now 当成所有嵌套层永久共享的时间源。

### 1.3 文件与职责

| 文件 | 新建/修改 | 要做的事 |
|---|---|---|
| `src/animation/world_clock.rs` | 新建 | `ClockId`、`AnimatableHandle`、`WorldClock`、`ClockArena`、成员表和 Clock 时间策略。只管时间/关系数据，不直接推进 Armature |
| `src/animation/mod.rs` | 修改 | 声明并导出 `world_clock` 类型 |
| `src/armature/runtime_ctx.rs` | 新建 | 窄 arena context：跨 Armature 查询、clock membership 原子迁移、event 入队 |
| `src/armature/mod.rs` | 修改 | 注册并 re-export `RuntimeCtx`；移除旧 `ArenaCtx` 定义 |
| `src/armature/armature.rs` | 修改 | 增加 `clock: Option<ClockId>`；`advance_time` 使用 RuntimeCtx；实现 `_sort_z_order`；清理路径配合 factory detach |
| `src/armature/slot.rs` | 修改 | `SlotUpdateCtx` 携带父 Armature clock；Slot 更新使用 RuntimeCtx；child 切换时 detach/inherit clock |
| `src/armature/constraint.rs` | 修改 | 增加 IK 的 `invalid_update`，按 root + chain bone 两处置脏 |
| `src/factory/base_factory.rs` | 修改 | owner 初始化/root clock；`advance_one` 构造 RuntimeCtx；关系入口、clock dispatch、三段式全局 advance、事件快照 seam、dispose detach |
| `src/armature/runtime_ctx.rs` | 内含单测 | 验证当前骨架不可经 context 反查、clock 迁移原子性和 event append |
| `tests/world_clock.rs` | 新建 | clock 时间、成员变更、contains/嵌套、传播、clear/dispose 行为 |
| 模块内单测（`armature/runtime_ctx.rs` / `factory/base_factory.rs`） | 内含单测 | event 入队、快照替换、重复 take 和 dt=0 flush；集成侧由 `tests/world_clock.rs` / `tests/golden_anim.rs` 覆盖 |
| `tests/armature_unit.rs` | 修改 | 更新过时的全局早退约定；保留 `advance_one` 单骨架直接推进测试 |

### 1.4 类型 / 方法草案

类型放置和可见性可调整；核心是 handle、owner 和关系更新不能分散。

```rust
// animation/world_clock.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatableHandle {
    Armature(ArmatureIdx),
    Clock(ClockId),
}

pub struct WorldClock {
    pub time: f64,
    pub time_scale: f64,
    system_time: f64,
    parent: Option<ClockId>,
    members: Vec<Option<MemberEntry>>, // insertion order + tombstone
    next_member_token: u64, // 从 1 递增；clear/compact 后不重置
}

struct MemberEntry {
    token: MemberToken,
    handle: AnimatableHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MemberToken(u64);

enum AttachResult {
    AlreadyPresent(MemberToken),
    Added(MemberToken),
}

enum ClockError {
    InvalidClockId,
    Cycle,
    InconsistentRelationship,
    MemberTokenExhausted,
}

impl WorldClock {
    pub fn new(time: f64) -> Self; // time 初始化累计时钟；system_time 另取当前墙钟
    #[doc(hidden)]
    pub fn new_with_system_time(time: f64, system_time: f64) -> Self; // 测试用确定性构造
    pub(crate) fn attach(&mut self, handle: AnimatableHandle) -> Result<AttachResult, ClockError>;
    pub(crate) fn detach(&mut self, handle: AnimatableHandle) -> bool; // 置 tombstone，不改其他顺序
    pub(crate) fn clear_members(&mut self) -> Vec<AnimatableHandle>;
    pub(crate) fn update_time(&mut self, passed_time: f64, now: f64) -> Option<f64>;
    pub(crate) fn member_snapshot(&self) -> Vec<(usize, MemberToken)>; // O(n) snapshot
    pub(crate) fn active_member_at(&self, index: usize, token: MemberToken) -> Option<AnimatableHandle>; // O(1)
    pub(crate) fn compact(&mut self);
}

pub struct ClockArena { slots: Vec<Option<WorldClock>> }

impl ClockArena {
    pub fn new() -> Self;
    pub fn insert(&mut self, clock: WorldClock) -> ClockId; // 只分配 identity，不设置 parent/member 关系
    pub fn get(&self, id: ClockId) -> Option<&WorldClock>;
    pub fn get_mut(&mut self, id: ClockId) -> Option<&mut WorldClock>;
    pub(crate) fn parent(&self, id: ClockId) -> Option<ClockId>;
    pub(crate) fn attach_clock(&mut self, parent: ClockId, child: ClockId) -> Result<(), ClockError>;
    pub(crate) fn detach_clock(&mut self, child: ClockId) -> Result<bool, ClockError>;
    pub(crate) fn clear_clock(&mut self, id: ClockId) -> Result<Vec<AnimatableHandle>, ClockError>;
}
```

`ClockArena::insert` 返回的是**新 Clock 自己的 ID**，不是 parent ID。新 Clock 默认 `parent = None`；要嵌套时另调关系入口 `attach_clock(parent_id, child_id)`，同时把 `Clock(child_id)` 加入 parent 的 members，并将 `child.parent = Some(parent_id)`。根 Clock 只 insert，不 attach 到其它 Clock。

`MemberToken` 是**一次成员关系的身份**，不是 Armature/Clock 的身份：

- token 在单个 `WorldClock` 内唯一且单调递增；跨 Clock 可以重复，所以需与 `ClockId` 组合才是全局唯一的 membership key。
- 创建 Clock 时 `next_member_token = 1`。attach 若发现同一 handle 仍有 active entry，返回 `AlreadyPresent(existing_token)`，不递增、不追加。
- attach 新关系（包括同一 handle 先 detach 后重新 attach）时，取当前 `next_member_token` 分配给新 entry，再用 `checked_add(1)` 更新计数；若耗尽则返回 `ClockError::MemberTokenExhausted`，不静默回绕。
- detach 把旧 entry 变成 tombstone；compact 删除 tombstone；二者都不重置计数。这样旧 token 永远不会匹配后续重挂的同一 handle。

伪码：

```text
attach(handle):
    if active entry with this handle exists:
        return AlreadyPresent(entry.token)

    token = MemberToken(next_member_token)
    next_member_token = checked_add(next_member_token, 1)
        or return MemberTokenExhausted without inserting
    append MemberEntry { token, handle }
    return Added(token)
```

`MemberEntry.token` 让 remove 后在同一轮重新 attach 的关系变成新条目：旧本轮 token 仍失效，新 token 在初始边界之外。这样固定循环边界不会因只检查 `contains(handle)` 而错误地推进新挂接成员。Clock slot 若不使用 generation，则移除后不复用 slot；旧 `ClockId` 永远不能指向另一个新 Clock。

### 1.4 RuntimeCtx 文件和方法

Rust 文件：`src/armature/runtime_ctx.rs`；`armature/mod.rs` 注册并导出该类型。它是借用视图，不是第二个 owner；`BaseFactory` 仍拥有所有 arena 与事件队列。

```rust
pub struct RuntimeCtx<'a> {
    armatures: &'a mut [Option<Armature>],
    clocks: &'a mut ClockArena,
    pending_events: &'a mut Vec<EventObject>,
    current_armature: Option<ArmatureIdx>,
    current_clock: Option<ClockId>,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCtxError {
    #[error("armature handle is invalid or currently taken out of the arena")]
    InvalidArmature,
    #[error(transparent)]
    Clock(ClockError),
}

impl RuntimeCtx<'_> {
    pub(crate) fn new<'a>(
        armatures: &'a mut [Option<Armature>],
        clocks: &'a mut ClockArena,
        pending_events: &'a mut Vec<EventObject>,
        current_armature: Option<ArmatureIdx>,
        current_clock: Option<ClockId>,
    ) -> RuntimeCtx<'a>;

    pub fn armature(&self, id: ArmatureIdx) -> Option<&Armature>;
    pub fn armature_mut(&mut self, id: ArmatureIdx) -> Option<&mut Armature>;
    pub fn current_clock(&self) -> Option<ClockId>;
    pub(crate) fn move_armature_clock(
        &mut self,
        id: ArmatureIdx,
        new_clock: Option<ClockId>,
    ) -> Result<(), RuntimeCtxError>;
    pub(crate) fn buffer_event(&mut self, event: EventObject);
}
```

`RuntimeCtxError` derives `thiserror::Error`; its `Clock` variant carries `ClockError`. The constructor is crate-private because only the arena owner should assemble the context; public APIs expose only the narrow read/query behavior.

各字段/方法的职责：

- `armatures` 是 arena 的借用切片；正在推进的骨架已经 `take()`，所以 `current_armature` 的查询显式返回 None。其它 Armature 只能通过 `ArmatureIdx` 查询。
- `clocks` 只借用 Factory 持有的 ClockArena。Context 不持有一个跨整个 members 循环的 `&mut Vec`。
- `pending_events` 由 Factory 持有；`buffer_event` 只追加新 owned EventObject，不按字段相等去重，也不 dispatch。
- `current_armature/current_clock` 是这次被推进 Armature 的身份和 clock 快照。Slot 要给新 child 继承时钟时，不从 arena 反查当前自身。
- `move_armature_clock` 只迁移一个当前在 arena 中的 Armature。它先校验旧关系/新 Clock，再加入新 Clock、摘除旧 Clock，最后改 `Armature.clock`；若中途失败要回滚新 membership。当前正在推进的 Armature 不在 arena 中，应由 Factory 在 take 前/放回后处理。
- `RuntimeCtx` 不负责遍历或推进 Clock；成员调度归 Factory/runtime driver。

当前落地的 `move_armature_clock` 返回 `Result`，对“目标当前被 take 出 arena”“ClockId 无效”“成员/back-reference 不一致”等情况给出错误。Slot 更新遇到关系错误应记录诊断，不能留下半更新关系。

当前代码已完成 RuntimeCtx、Factory Clock 关系 API、Armature/Slot clock propagation、dispose detach、root Clock 调度和事件 flush seam。全局调度用固定 `(index, token)` 快照，避免推进时新增成员本帧误推进；对应关系与快照测试已通过。Task 6 前置 `_sort_z_order` 和 `Constraint::invalid_update` 已实现并由 `armature_unit` 验证。

`Armature` 文件要新增 `pub(super) clock: Option<ClockId>`（初值 None）和只读 `clock()` getter。字段不能公开写。`SlotUpdateCtx` 新增 `clock: Option<ClockId>`，由 `Armature::advance_time` 在解构字段前复制 `self.clock` 后传给每个 Slot。

### 1.5 当前调用链迁移

1. `BaseFactory` 增加 `clocks: ClockArena`、`root_clock: ClockId`、event buffers；自定义 `Default` 创建 `WorldClock::new(0.0)` 作为 root。
2. `BaseFactory::advance_one` 保留单骨架 take/advance/put；构造 RuntimeCtx 后传给 Armature。全局 `advance_time` 现在执行 objects placeholder → root Clock → events snapshot。
3. `Armature::advance_time` 与 Slot 更新都使用 RuntimeCtx；`SlotUpdateCtx` 带当前父骨架 clock。
4. Slot child 切换、Factory 根挂钟传播、Clock attach/detach/clear、dispose detach 都走统一关系更新并更新 back-reference。
5. 运行时钟的零早退位于 `WorldClock::update_time`；外层 dt=0 仍 flush event snapshot。`advance_one(0)` 继续直接推进单骨架。

Task 1 验收：`cargo test --workspace` 回归及 `world_clock` / `armature_unit` / runtime event seam 行为用例通过。Task 6 的 `_sort_z_order` 与 `Constraint::invalid_update` 已在建立具体 timeline 前实现。

---

## 任务 1 伪码：时间更新与成员快照

WorldClock 时间换算严格按 `WorldClock.ts:97-123`：

```text
if passed_time is NaN: passed_time = 0
current_time = now
if passed_time < 0: passed_time = current_time - system_time
system_time = current_time
if time_scale != 1: passed_time *= time_scale
if passed_time == 0: return None
if passed_time < 0: time -= passed_time
else: time += passed_time
return Some(passed_time)
```

以下成员快照使用当前实现的 `(index, token)` 组合，避免对每个 token 再线性搜索：

```rust
let snapshot = clock.member_snapshot(); // O(n)，一次性抓取初始边界
for (index, token) in snapshot { // 顺序稳定；推进期间新增成员不在 snapshot
    let Some(handle) = clock.active_member_at(index, token) else {
        continue;
    };
    advance_handle(handle, member_dt);
}
clock.compact(); // O(n)，顺序稳定
```

成员被新增时拿到新 token，不能出现在本轮 `snapshot` 中；成员被移除时旧 token 失效。即使 clear 后重挂的成员复用了旧的 Vec index，token 也不同，因此不会在本轮误推进。自移除是在当前 Armature 调用期间将当前 token tombstone，回来后不重复推进。

#### 环处理策略（明确标注偏离）

官方 `WorldClock.add` 不检查环。推荐 Rust 的关系入口拒绝自环和 parent 链环，返回 `ClockError::Cycle`，并在 dispatch 时保留 visited-path 防御，避免损坏数据导致栈溢出。这是对“可建立循环 Clock 关系”的有意限制，须在任务 8 登记；测试应断言拒绝后 parent/member/back-reference 均未改变。

#### clock 与 Armature 关系

```text
attach_armature(clock, armature):
    if armature.clock == clock and clock contains armature: return
    pending_stack = [armature]
    visited = empty set
    while pending_stack not empty:
        current = pop pending_stack
        if current already visited: continue
        if current.clock == clock and clock contains current:
            continue
        detach current from its old clock first
        append current to new clock
        set current.clock = new clock
        // TS setter is depth-first in slot order; reverse-push to preserve it.
        for child in current.slots in reverse order:
            push child onto pending_stack

Slot child changed during armature update:
    old child: clear parent relation; detach from parent's clock; reset animation if inheritAnimation
    new child: set parent relation; inherit parent's clock; then follow existing animation inheritance path

dispose_armature(id):
    detach clock relation before arena slot take/None/free-list reuse
    run existing descendant disposal and on_clear
```

TS 依据：`Armature.ts:122-125` 清场先 remove clock；`:886-910` clock setter 在更新自身 membership 后传播到现有 child；`Slot.ts:537-580` child 切换先拆旧 child，再给新 child parent 和 clock；`BaseFactory.buildArmature` 不自动挂 clock。Rust 用 worklist/visited 传播，避免意外 child-armature 环导致递归栈溢出；保持传播发生的实际时点，不在 build 阶段提前 attach。

#### contains 和 clear

```text
clock_contains(clock, value):
    if value == Clock(clock): return false
    ancestor = value 的 clock back-reference
    while ancestor exists and ancestor != clock:
        ancestor = ancestor.parent
    return ancestor == clock

clear_clock(clock):
    逐个取得直接成员 handle，释放成员表借用
    Armature 成员：回指置 None，旧成员 token tombstone
    Clock 成员：child.parent 置 None，旧成员 token tombstone
    保留成员顺序整理/压缩由 clock traversal policy 决定
```

若 clear 发生在推进回调中，已捕获的本轮 tokens 逐个检查是否仍有效，后续已清除成员跳过。清除一个父 Clock 不清空子 Clock 自己的成员。

#### 全局 advance 与 Event seam

TS 依据 `core/DragonBones.ts:304-338`：先遍历对象回收队列并清空，再调 root clock，最后逐条处理事件；`bufferEvent` 的 `indexOf` 是对象身份判重。

```text
BaseFactory.advance_time_with(dt, now):
    flush_objects()                    // 本阶段 no-op，仍占有固定调用位置
    advance_clock(root_clock, dt, now)
    flushed_events = take(pending_events)  // 替换上次快照，即使为空也替换
    // 阶段 4 不 dispatch、不调用 proxy listener

buffer_event(event):
    append event                        // Rust 每次产生新 owned EventObject；不能按字段相等吞事件

take_flushed_events():
    return take(flushed_events)         // 第二次调用返回空
```

Stage 4 `pending_events` 和 `flushed_events` 可以由 `BaseFactory` 持有；若抽成字段子结构，仍只能由同一 owner 管理。`dt == 0` 仍进入 objects/clock/events 三段：clock 刷新 system time 后早退，events 把本帧 pending 替换为快照。

### 1.6 阶段 3 接缝和前置方法

原指南 §1.6 的接缝状态：

1. **已接线**：`ArenaCtx` → `RuntimeCtx`，迁移 `Armature::advance_time`、`SlotUpdateCtx::world` 和 `BaseFactory::advance_one`；当前 Armature 仍 take 出 arena。
2. **已接线**：Armature clock 字段、Slot child clock detach/inherit、dispose 前 detach；RuntimeCtx 的关系迁移方法校验并同步成员表与 back-reference。
3. **已接线**：全局 `BaseFactory::advance_time` 改为 root Clock 成员驱动；零早退位于 Clock 层，dt=0 的外层事件 flush 保留。
4. **已接线**：Factory Armature/Clock attach、detach、clear、contains API，以及根 clock 对已显示 child 的传播。
5. **Task 6 前置，已实现**：`Armature::sort_z_order(slot_indices, offset)`，对照 `Armature.ts:185-207`。
6. **Task 6 前置，已实现**：`Constraint::invalid_update(bones)`，按 `Constraint.ts:218-224` 使 root 与可选 chain bone invalid。

### 1.7 Task 1 测试卡

`tests/world_clock.rs`：

- 构造传入 `time` 后累计时钟等于该值；`system_time` 独立初始化。
- attach/detach、重复 attach、跨 clock 转挂：成员顺序与 Armature/Clock back-reference 一致。
- timeScale `0 / 0.5 / 2`、NaN、正 dt、负 dt sentinel；验证 system_time 先后、clock time 累加和成员 dt 符号。
- root clock 只推进其成员；未挂成员不动；`advance_one(0)` 仍推进单 Armature 更新。
- 本轮新增成员下一轮才推进；移除尚未到达成员本轮跳过；self-remove 不重复推进；压缩保序。
- 尚未轮到的成员在本轮被 remove 后又重新 attach：旧 token 失效，新 token 下轮才推进。
- clear 多个 Armature/Clock 成员后所有直接关系解除；子 Clock 自己的 members 保留。
- `contains` 覆盖直接成员、后代、无关对象、查询自身。
- root clock propagation 到已有 child；Slot 切换旧 child detach、新 child inherit。
- dispose 先 detach 再释放 slot；无 stale membership。
- 自环/环关系按记录的 Rust cycle policy 安全拒绝。

测试成员动态修改可用私有 scheduler test seam / 驱动闭包模拟；不要为测试给生产运行时引入 `Box<dyn IAnimatable>`。

事件 seam 的模块内单测（`runtime_ctx.rs` / `base_factory.rs`）+ `tests/world_clock.rs`：两条新建同值事件都保留；空帧替换旧快照；同帧第二次 take 为空；dt=0 flush 已缓冲事件且不推进骨架；listener gate 的逐帧整链验证在 `tests/golden_anim.rs`。

完成 Task 1 后运行 `cargo test --workspace`，确认原阶段 0–3 全绿再继续。

---

## 任务 2 — AnimationConfig 与 Animation 容器骨架

### 文件

| 文件 | 处理 |
|---|---|
| `src/model/animation_config.rs` | 现有字段和 `Default/clear/copy_from` 已实现；逐字段对 `AnimationConfig.ts:277-339`，主要补完整测试，不默认重写 |
| `src/model/animation_data.rs` | 现有 `AnimationData/TimelineData` 字段已在；核对覆盖即可，不重构模型 |
| `src/animation/mod.rs` | 从当前占位模块整理为子模块/导出 |
| `src/animation/player.rs` | Animation 主体与 API（避免 module-inception） |
| `src/animation/state_id.rs` | 新建稳定 StateId 与 generation 管理 |
| `src/armature/armature.rs` | `Armature::new` 挂接动画数据表；`on_clear` 使用 `Animation::on_clear` |
| 模块内单测（`src/animation/state_id.rs` / `src/animation/player.rs`） | 状态存储、清理语义与 play_order 顺序测试 |

### 实作步骤

1. 对照 `Animation.ts:68-100` 完成 `on_clear`：states、动画数据表、blend states、config、timeScale、inheritTimeScale、dirty、last state 等都复位。TS 细节：`_animationNames/_animationStates` 清空，`_animations/_blendStates` 清空，config 回收，`timeScale=1`。
2. 对照 `Animation.ts:280-289` 单独实现 `reset`。它清 states/config/dirty/last state，但不等于 `on_clear`；不能合并。
3. `AnimationConfig` 默认值包括 `fadeOutMode=All`、`playTimes=-1`、`timeScale=-100`、`fadeInTime/fadeOutTime/autoFadeOutTime=-1`；测试 `copy_from` 所有字段及 bone mask。
4. Animation 从所属 `ArmatureData.animations` 建 lookup 和 names；查询 map 的遍历顺序不能替代 TS 状态播放顺序。
5. `states: StateArena<AnimationState>`（`state_id.rs` 的稳定 slot 存储）配 `play_order: Vec<StateId>` 存 layer 排序/同层顺序。`StateId { slot, generation }`；释放不移动其它 state，slot 复用递增 generation。
6. 先立真实 API 类型，不留无人调用的空实现。尤其 `play` 第二参是 `play_times: i32`，不是 fade seconds。

测试：默认值、`copy_from`、状态释放不改其他 ID、slot 复用后旧 ID 失效、同层插入/查询顺序、reset vs on_clear。

---

## 任务 3 — TimelineState 共用时间算法

### 文件

| 文件 | 处理 |
|---|---|
| `src/animation/timeline_state.rs` | 新建；Rust 可用组合实现 TS `BaseTimelineState.ts` 里的 Timeline/Tween/Single/Double/Multiple 五层职责 |
| 模块内单测（`src/animation/timeline_state.rs`） | 纯时间算法边界测试 |

官方定位：源码文件名 `BaseTimelineState.ts`，其中基类名 `TimelineState`；具体 `ActionTimelineState` 在 `TimelineState.ts` 中覆写 update。不要把 action 的跨帧扫描误做成所有 timeline 共用的普通逻辑。

### 方法拆分

```rust
fn set_current_time(&mut self, passed_time: f64) -> bool;
fn init_timeline_data(&mut self, armature: ..., state: ..., timeline: ...);
fn update(&mut self, passed_time: f64);
fn update_frame_index(&mut self); // O(1) frameIndices 查表
fn get_easing_value(...);
fn get_easing_curve_value(...);
fn update_tween(&mut self);
```

按 `BaseTimelineState.ts:92-160,163-224,232-358` 拆任务：

1. `_setCurrentTime` 三分支：有 action 且 ≤1 帧；action 无/局部 scale-offset；普通多帧复制 action timeline 时间。
2. 普通分支保留 TS 的 `passedTime *= _timeScale` 后加 `_timeOffset * animation.duration` 次序；有限循环到头与负时间的 floor/remainder；负向整数倍 currentTime 为 duration。
3. `init/update` 使用 IR timeline offset，读取 frameCount、frame value offset、timeScale/timeOffset；帧定位是 `frameIndices[frameIndicesOffset + floor(currentTime * frameRate)]`。
4. tween frame position 使用 `_frameArray[frameOffset] * frameRateR`；ActionTimelineState 帧 event 的 frame time 使用除法 `frameArray[pos] / frameRate`，二者别“统一优化”。
5. easing 和已采样 curve table 按源码算术表达式插值；不运行时重新采样 Bezier。
6. 有限循环最后一帧不 tween；处理尾帧 duration 精度补偿、playState/frameIndex/dirty 生命周期。

测试：三类时间归一化、正负整数倍、边界 currentTime、O(1) 首末帧查表、各 easing、曲线端点/线性插值、末帧禁止 tween。Action 跨帧和 lifecycle event 单独在任务 6 测。

---

## 任务 4 — AnimationState 与 BlendState

### 文件

| 文件 | 处理 |
|---|---|
| `src/animation/animation_state.rs` | 新建；对齐 `AnimationState.ts` |
| `src/animation/blend_state.rs` | 新建 Rust 文件；官方 BlendState 实际定义在 `AnimationState.ts:1528-1601` |
| `src/animation/state_id.rs` | 父/子/last state 等引用都换成 StateId/owner ID |
| 模块内单测（`src/animation/animation_state.rs` / `src/animation/blend_state.rs`） | 默认值、状态转移、fade、权重测试 |

### 实作步骤

1. 字段默认与初始化对照 `AnimationState.ts:194-335,725-804`；Rust 可去掉无观测价值的池字段，但初始值和 reset 行为要锁测。
2. 三类状态分别表达：`_fadeState ∈ {-1,0,1}`、`_subFadeState ∈ {-1,0,1}`、`_playheadState` 位掩码。`playhead == 3` 才推进 `_time`。
3. 按 `AnimationState.ts:667-720` 实现 fade：生命周期事件仅 root state 且 `actionEnabled` 并通过相应 listener gate 才入队；fade 取负 passedTime 的正值；fade complete 的状态/事件点按原序。
4. `advance_time` 按 `AnimationState.ts:808-1070` 保序：fade → playhead/timeScale → rebuild timelines → action timeline（weight early return 之前）→ z-order → bone timelines → bone blend timelines → slot timelines → slot blend timelines → constraints → animation timelines → pose timeline cleanup → auto fade。
5. `weight == 0` 仍先执行 action timeline，然后才 early return。
6. pose timeline 集合负责未被动画覆盖目标的 reset fallback；fade 完成后移除 pose timelines。
7. BlendState 对照 `AnimationState.ts:1553-1600`：每帧 reset，按 state 更新 `leftWeight/layerWeight/blendWeight`；`dirty` 首写者重置 pose，后续 writer 按 blend 叠加；相同 BlendState target 在 bone loop 中按顺序去重。
8. cache 仅 fade 完且 `cacheFrameRate > 0` 才进入；多 state 路径给 timeline 传 0。cache 量化/写入分支须就地标注 P1-4 与当前不可达条件。
9. fadeOut：负时间归 0；longer-fade guard；progress continuity；通知 timeline fadeOut；displayControl 清除。五种模式筛选在 Animation 层实施，Single 在 `_fadeOut` 分支为空。

测试：pauseFadeIn、fade 与 state timeScale 独立、weight=0 action、事件入队 gate、blend 首写者/权重预算/同 target、progress continuity、longer-fade guard、五种 fadeOutMode。

### 4.1 当前实现检查点（2026-10-19）

`docs/archive/04-task4-handoff-2026-10-16.md` 是本轮历史交接记录。AnimationState/FadeState/BlendState 核心、action-before-zero-weight、fade 完成清理、auto-fade、cache 单状态入口和固定组 cursor 遍历已落地；本轮 `cargo test --workspace` 全绿。

后续任务 6 已消费 `TimelineDirty::RebuildAll/RefreshTargets`，建立具体 targets 并执行各轴数值 blend / pose reset；最新状态见 §6.1 和 §7.1。`docs/archive/04-task4-handoff-2026-10-16.md` 保留 2026-10-16 的历史停止点记录，不代表当前 timeline 仍是占位。

---

## 任务 5 — Animation 顶层 API 与管理

### 文件

- 主体：`src/animation/player.rs`（对应 `Animation.ts`）。
- 交接：`armature.rs`/`slot.rs`/`factory/base_factory.rs` 中所有旧 placeholder 调用点。
- 测试：模块内单测（`src/animation/player.rs`）。

### 实作顺序

1. `advance_time` 对负 dt 取正；继承 parent animation 的 `_inheritTimeScale`；每帧 reset blend states；准确处理 1/多/0 states 与 cache frame 顺序（`Animation.ts:172-267`）。
2. `play_config` 逐段移植：animation lookup 失败；Single 同层同 data state early return；config normalize **写回**；fade-out；state 初始化和 layer 插入；child inheritance；animationTimelines 子状态创建和插入重排（`:337-481`）。
3. `play` 与 `fade_in` 独立保留 TS 默认值：`play` 先 clear config 且 All；`fadeIn` 默认 SameLayerAndGroup（`:506-580`）。
4. `goto_and_play_by_time/frame/progress`、`goto_and_stop_by_time/frame/progress`、`stop`、getState/hasAnimation/isPlaying/isCompleted 等按实际公开行为映射（`:599-868`）。
5. `Animation::get_state` 倒序扫 `play_order`；空状态 `is_completed` 为 false。
6. RuntimeCtx 只用于 parent/child Armature 等跨 arena 访问；Animation 不直接写 Clock membership。

测试：config 写回；Single early return 不改 config；play vs fadeIn 的 fade 范围；gotoAndStop freeze；stop 单个/全部；getState 倒序；空列表 completed=false；child animation inheritance/state 创建顺序。

### 5.1 当前实现检查点（2026-10-19）

Animation API 既有实现已逐项 review 并补上 AnimationProgress/Weight/Parameter 子状态创建、父子 `StateId` 关系、同层 play-order 重排、auto/public fade-out 子状态传播、parent 回收时 child 安全解除，以及 `play()` 空名 resume/replay 和 getState 查询测试。Animation child axes 已由任务 6 驱动；37 个逐帧 goldens 与 workspace 测试通过。子骨架 clock/animation inheritance 走 Armature + RuntimeCtx。

---

## 任务 6 — 16 类具体 TimelineState

### 推荐文件

```text
src/animation/timeline_states/mod.rs        构造/分发与组顺序
src/animation/timeline_states/action.rs      ActionTimelineState
src/animation/timeline_states/z_order.rs     ZOrderTimelineState
src/animation/timeline_states/bone.rs        BoneAll/Translate/Rotate/Scale
src/animation/timeline_states/slot.rs        SlotDisplay/Color/ZIndex/Alpha
src/animation/timeline_states/surface.rs     SurfaceTimelineState
src/animation/timeline_states/deform.rs      DeformTimelineState
src/animation/timeline_states/constraint.rs  IKConstraintTimelineState
src/animation/timeline_states/animation.rs   Progress/Weight/Parameters
src/animation/timeline_states/*.rs           各模块内单测：合成 IR 逐轴行为
```

可以按 Rust 可读性调整文件合并，但保留可逐个 review 的 TS 类名、输入 offset、目标和更新顺序。Axis 名称和批次详见原指南任务 6。

### 每个轴开工前的六项卡片

```text
[ ] timeline type、IR 数组类型及 offset 基准
[ ] frame value count；Step/Int/Float 分别取哪个 offset
[ ] target handle/BlendState key 与 dirty flag
[ ] init/read/tween/blend 的 scale 修正位置
[ ] pose fallback、结束帧、wrap、display/geometry gate
[ ] 合成 IR 单测及受影响资产 golden 子集
```

### 按批实现与源码检查点

1. **Action / ZOrder**（`TimelineState.ts:27-287`）：ActionTimelineState 自己处理 frame crossing、正反向、跨 loop、区间播放与 start/loopComplete/complete；frame time 保留除法。Play action 无 listener gate，写 armature action queue；Frame/Sound 事件走 `_dragonBones.bufferEvent`，Frame 和生命周期事件各自 listener gate，Sound action 例外。ZOrder 调 `Armature::sort_z_order`。
2. **BoneAll / Translate / Rotate / Scale**（`:291-500`）：值数 `6/2/2/2`；每轴独立处理 normalize、pose 和 scale。BoneAll 在 blend 乘 armature scale；Translate 在 init 读值时乘；Rotate/Scale 不乘。
3. **SlotDisplay / SlotColor / SlotZIndex / Alpha**（`:591-853`）：SlotColor 先经 frameIntArray 间接索引 colorArray，负 i16 offset 加 65536；乘子值与 offset 值采用不同比例，fade color 的 progress 四次方。SlotZIndex 保留累加和 absolute blend 两路径；displayController gate 在 slot timelines 更新处。AlphaTimelineState 同时用于 BoneAlpha/SlotAlpha。
4. **Surface / Deform**（`:504-590,858-956`）：header/value offset 用 `TimelineFrameValueCount`（IR index 3）；same-value offset、负值 wrap、vertex range、geometry identity guard 按 TS。Surface 与 Deform 的 scale/value layout 不可合并臆测。
5. **IK / AnimationProgress / Weight / Parameters**（`:960-1073`）：IK 当前结果写 `currentA/currentB`（bend/weight）并调用 constraint invalidation；三个子动画 timeline 只在 target state 有 parent 时改写 state。

每批先做合成 IR 单测，再跑相关 golden 子集。核对优先级：offset → wrap → scale 落点 → 状态组遍历 → 最后才考虑 libm。

### 6.1 当前实现检查点（2026-10-20）

TimelineDirty rebuild、骨骼/插槽/约束/animation target 创建、pose fallback 与 fixed group runtime 更新已经接线；Action/ZOrder 保留独立更新路径。`golden_anim` 以 root clock 显式驱动 37 个现有动画 golden、逐帧比对 120 帧骨/槽 pose 和 enqueue event 快照，目前全绿。另有合成 line-tween header 边界单测，覆盖最后一个 line keyframe 不含 easing header 的布局。

补充合成验证：`timeline_states::action::tests::sound_bypasses_listener_gate_while_frame_and_play_do_not_enter_event_queue` 锁住无 listener 时 Sound 仍入 event queue，而 Frame 被 gate、Play 进 armature action queue。

任务 7 的补课用例已落地：`resetToPose` 未覆盖目标（`Dragon` 骨 + `PlantAnimate` 插槽的真实素材端到端，另有 `animation_state.rs` 合成用例）、state timeScale=2 的 60-vs-120 帧（含半步对照）、reverse timeScale 端到端；另补父子时间缩放组合用例。详见 §7.1。

---

## 任务 7 — golden harness 与 enqueue 验证

### 文件

| 文件 | 处理 |
|---|---|
| `tests/golden_anim.rs` | 9 资产 × 全动画 × 120 帧测试（已落地 37 个 golden） |
| `tests/common/mod.rs` | 现有 golden 读取/容差 helper；阶段 4 未单独抽 `anim_snapshot` |
| 模块内单测（`animation/player.rs` / `animation_state.rs` / `timeline_states/*.rs`） | listener gate 正反向、同值 event、不 dispatch、gotoAndStop/child/timeScale 场景；resetToPose 合成用例已落地（真实素材端到端见 `tests/animation_acceptance.rs`） |

### harness 逐帧顺序

参照 `tools/jsdump.mjs:91-145`：构造 factory/DragonBones → parse → build → 显式 `armature.clock = factory.clock` → 为 9 类 event 注册 listener → `gotoAndPlayByTime` → 每帧先 `advanceTime(dt)` 再采样。

Rust 阶段 4 映射：注册 9 类 panic probe 必须在 goto/play 之前；显式 attach 到 root clock；每帧 `BaseFactory::advance_time` 后调用 `take_flushed_events()`；只比较快照，不执行 proxy 的 dispatch。这样可比对事件生成/入队时点，同时证明本阶段从未调用 listener。

每帧对比 bones 13 值、slots 15 值和 events `(type,name,bone,slot)`；名称与顺序元数据按原指南在 120 帧末态比较。容差和最大误差报告按原指南，不以“看起来相同”验收。

### 7.1 当前实现检查点（2026-10-20）

`tests/golden_anim.rs` 已按指南建成：37 个现有动画 golden，显式 root-clock attach，播放前安装 9 类 panic probe，逐帧运行后取替换式 event snapshot，并逐元素检查 13 bone / 15 slot 值与 `(type,name,bone,slot)` 事件。名称/顺序元数据在末帧检查；每个 golden 输出最大绝对和相对 pose 误差。另有无 listener 的 Shoot 测试验证 Frame/lifecycle gate；合成 Action 单测验证 Sound 仍入队、Play 进入 action queue。相关测试当前全绿。

额外测试现状：任务 7 的五项补课已全部落地——gotoAndStop 冻结与同帧 gotoAndPlay 往返（`goto_and_stop_by_frame_freezes_pose_and_play_round_trip`）、`resetToPose` 未覆盖 bone/slot（`animation_state.rs` 合成 + `animation_acceptance.rs` 真实素材：`Dragon` walk→stand 骨复位并与"从零播放 stand"整姿态对照、`PlantAnimate` Shoot→Idle 插槽 displayIndex 复位及关闭配置的残留对照）、子骨架 StateId 创建/clock 传播/父子时间缩放组合（`child_animation_composes_inherited_parent_time_scale_once`）、state timeScale=2 的 60-vs-120 帧（含半步对照）、reverse timeScale 端到端。事件侧 listener gate、Sound 例外、Play action 不走 event queue 仍由模块内单测/golden 覆盖。

---

## 任务 8 — Review 与差异登记

复核 `animation/**`、`armature/armature.rs`、`armature/slot.rs`、`armature/constraint.rs`、`factory/base_factory.rs`：

- 阶段 3 的 sort-z-order、constraint invalidation、Animation 清理和 Slot child clock 接线均已收口。
- 阶段 4 范围没有未解释 `todo!()`/空方法；P1-4 cache 保留/量化/写入分支就地标注 P1-4 与不可达条件。
- 新增 Handle 有唯一 owner、有效性/复用策略和测试；无 stale membership/back-reference；没有 `unsafe set_len`。
- 每个轴按官方公式 review F4.5/F4.6/F4.14/F4.17/F4.18/F4.19。
- cycle guard 等有意偏离登记：官方事实、Rust 限制、测试行为和理由一致。

---

## 任务 9 — 验收

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node tools/gen_pose.mjs
```

交付摘要包含：逐资产/逐动画 golden 结果、最大绝对和相对误差、clock 行为矩阵、动画 API/state 测试结果、差异登记。所有命令及原指南阶段完成标准通过后才结束阶段 4。

### 9.1 当前验收检查点（2026-10-20）

当前验收：`cargo build --workspace`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `node tools/gen_pose.mjs` 均通过；pose 生成结果 9 ok / 0 fail。`golden_anim` 37 个动画 × 120 帧全通过，跨所有输出最大绝对 pose 差 `1.137e-13`、最大相对 pose 差 `3.085e-14`，严格使用 `1e-9 * (1 + |want|)` 容差。Clock、listener gate、Sound 例外、Play action 和 pose golden 均全绿。任务 7 的五项补课用例已全部覆盖（见 §7.1），其中 `resetToPose` 用真实素材做端到端对拍；收尾新增 3 个端到端用例后 workspace 共 205 个测试全绿、clippy 零警告。Task 8 差异登记补充 D-4.32（animation state graph cycle guard）；P1-4 cachedFrames 写侧仍明确保留未实现，不影响当前 37 个 pose/event golden。阶段 4 收尾改动（3 个用例 + 本文档修订）在工作区，待用户决定是否提交。

---

## 你可以先做的第一张代码卡

**1A：构造与基本 Clock 关系**，暂不实现递归推进。

1. 新建 `src/animation/world_clock.rs`，先实现 `ClockId`、`AnimatableHandle`、`WorldClock::new(time)`、`ClockArena`。
2. 在 `BaseFactory` 中建立 Clock arena 和 root Clock；root 的 `time` 从 0 开始，`system_time` 独立取当前时钟。
3. 实现 `attach_armature` / `detach_armature` / `attach_clock` / `detach_clock` 的原子关系更新。
4. 增加 4 个小测试：初始 time 与 system_time 分开；attach 成员次序与回指；重复 attach 幂等；跨 clock 转挂先清旧关系。
5. 跑 `cargo test --workspace`。递归调度、dt 计算和事件 seam 留到 1B/1C。

每个子卡都以“测试能描述预期 → 实现 → 全量回归”为闭环；如果你卡在 Rust 借用边界，带上正在写的最小代码和编译错误，我们就聚焦解决那一处。
