# 阶段 4 实现指南：动画运行时

> 配套 `docs/requirements.md` §3 阶段 4。官方 TS 是行为规格，但不是 Rust 结构模板。
> 本指南要求数值公式、状态转移、时间轴顺序和可观察行为对齐 TS；所有权、arena、容器、借用和调度采用 Rust 设计，并用行为测试证明等价。
> 先完成任务 1 的架构决策与测试，再按任务 2→9 实现。全套验收通过后进行 review，再编写阶段 5 指南。
> 参考笔记位于 `target/stage4_notes/`：`clock_driver.md`、`animation.md`、`animation_state.md`、`timeline_states.md`。行号以 `DragonBonesJS/DragonBones/src/dragonBones/` 当前 5.7 源码为准。

---

## 0. 规则与已核实事实

### 0.1 哪些必须逐项对齐

下列逻辑直接影响姿态、插槽或事件边界，逐个 TS 分支核对，并使用数值/行为测试锁定：

- `BaseTimelineState` 的时间归一化、帧查表、帧跨越、easing 和 tween 公式。
- 16 个具体 TimelineState 对扁平 IR 的索引、wrap、插值、pose 写点及调用顺序。
- `AnimationState` 的 fade/playhead 状态、权重分配、timeline 分组遍历、自动淡出。
- `Animation` 的公开 API 默认值、config 归一化和写回、副作用、子动画继承及更新顺序。
- WorldClock/runtime 的时间缩放、0 早退、成员顺序、运行时 add/remove/clear 对本帧迭代的影响。
- Action Play 链。它不受事件监听器闸门影响，会创建/推进动画状态，数值相关。

“逐项对齐”不要求 Rust 代码逐行相似。保留官方表达式的算术顺序；若用不同控制结构表达同一行为，必须通过对应测试证明。

### 0.2 哪些按 Rust 语义实现

不机械移植 JS 对象引用、setter 回调、动态数组空洞、trait-object 所有权或对象池。Rust 实现采用 arena/newtype handle、唯一 owner、显式 runtime context 和安全容器。只有可观察的延迟释放/成员变更时机必须保留；对象内部如何被持有不属于验收目标。

不要因 Rust 风格偏好改变官方可观察顺序。尤其不要用无序 HashMap 遍历替代会影响 blend 首写者的顺序，也不要通过 `Rc<RefCell<_>>` 绕过 arena 的借用边界。

### 0.3 官方源码已核实的阶段 4 事实

| 编号 | 官方事实 | 实现约束 |
|---|---|---|
| F4.1 | requirements 行数与源码总行数有差异：AnimationState 1602、Animation 959、BaseTimelineState 570、TimelineState 1074、WorldClock 256、DragonBones 400 | 以 TS 原文和本指南行号为准，不以估算行数裁剪 |
| F4.2 | 帧定位使用 `frameIndices[frameIndicesOffset + floor(currentTime*frameRate)]` O(1) 查表；Bezier 在解析期采样，运行期是 easing 与曲线表线性插值 | 不实现不存在的二分或运行期 Bezier |
| F4.3 | `_fadeState` 值为 -1/0/1；`_subFadeState` 也为 -1/0/1 但含义不同；`_playheadState` 为位掩码，只有值 3 推进动画时间 | Rust 可用两个 enum 和 bitflags/newtype，不得混淆状态含义 |
| F4.4 | 17 种 TimelineType 对应 16 个具体类；BoneAlpha/SlotAlpha 共用 AlphaTimelineState；BoneAll 每帧 6 个值 | 具体类/值布局按此实现；保留官方 `MutilpleValueTimelineState` 拼写仅用于源码定位 |
| F4.5 | 时间轴值读取基址为 frame int/float 基址 + timeline offset 中的 value offset + frame index × value count；Step/Int/Float 三类 offset 来源不同 | 作为所有时间轴共用的数据读取规则 |
| F4.6 | SlotColor 间接读 colorArray；Surface/Deform header 使用 TimelineFrameValueCount（IR index 3）；Action/ZOrder/SlotDisplay 的 Step 值位于 frameArray；若干 int16 wrap 需要 `+=65536` | 按 TS 指定数组和 wrap 规则读取 |
| F4.7 | WorldClock 保留 dt 符号；Animation 层对负 dt 取正；AnimationState 的 timeScale 是反向播放入口 | 负时间只在 TS 指定层处理 |
| F4.8 | WorldClock 更新时间基准、timeScale 缩放后才做 0 早退；DragonBones 的 objects/clock/events 三段无条件按序运行 | clock 的早退不能挪到 runtime 外层 |
| F4.9 | Play Action 入 armature action 队列，无 listener 闸；Frame event 和生命周期事件按各自 listener 闸门入队；Sound action 无条件进入事件队列（TimelineState.ts:53），但仍受 actionEnabled 等外层条件约束 | Play 链本阶段真实现；保留 Sound 的 listener 闸门例外；纯事件派发可在阶段 5 完成 |
| F4.10 | start/loopComplete/complete 的触发属于 ActionTimelineState；loopComplete 有跨第 0 帧的延迟 flush | 保留触发点和相对顺序，即使事件消费留到阶段 5 |
| F4.11 | `gotoAndStopByFrame` 存在；`resetToPose` 是 config/state 字段而非独立方法；Animation 没有公开 fadeOut | Rust API 和测试按真实公开行为映射 |
| F4.12 | play 使用 All 淡出策略；fadeIn 默认 SameLayerAndGroup；`_fadeOut` 的 Single case 是空实现，Single 复用既有 state 的分支在 playConfig | 不合并这些路径 |
| F4.13 | fade 推进先于 state timeScale；默认 pauseFadeIn 会冻结 playhead 直到 fade-in 完成 | 顺序保持 |
| F4.14 | 时间归一化在 BaseTimelineState，分为 action/single-frame/普通多帧轴；普通多帧轴复制 action 时间；负时间取反后 floor；负向整数倍时刻 currentTime=duration | 分支结构与边界行为必须测试 |
| F4.15 | cache 只在 fade 完成且 cacheFrameRate>0 时启用；多状态路径传 0；当前素材通常不会进入缓存体 | 本阶段保留结构并标注 P1-4，不伪称缓存已完成 |
| F4.16 | Action 轴帧位置用 `frameArray[pos] / frameRate`，不是乘预先计算的倒数 | 保持除法表达式 |
| F4.17 | 骨架 scale 修正落在不同轴的不同位置：BoneAll blend；Translate/Surface/Deform init 读值；Rotate/Scale 不乘 | 按轴移植，不统一“优化” |
| F4.18 | float IR 已 f32 量化；int IR 有 i16/u16 wrap；parser 写侧 scale 与运行侧 `_valueScale` 互为量化/还原 | 两侧行为都要保留 |
| F4.19 | BlendState 每帧 reset；leftWeight/layerWeight/blendWeight 分配；dirty 首写者清空 pose；遍历序影响结果 | 保留确定性遍历顺序 |
| F4.20 | AnimationState 没有 fadeIn 方法、normalizedTime 或单一 timeline 字典；内部按固定组保存时间轴，并有 pose timeline 集合 | Rust 容器可调整，但组顺序和 pose fallback 必须保留 |
| F4.21 | DragonBones 5.7 事件派发仅在 advanceTime flush；全局 eventManager 只接 SOUND | 事件边界阶段 5 收口 |
| F4.22 | 阶段 3 尚缺 `_sort_z_order` 和 `Constraint::invalid_update`，阶段 4 时间轴会调用 | 在 ZOrder/IK 轴之前补齐 |
| F4.23 | IK currentA/B 与 Deform JSON 退化路径需 READ-ORIGINAL；素材未覆盖的曲线负分支仍须保留 | 先按源码保留，再用合成输入验证可达行为 |
| F4.24 | WorldClock 固定初始循环边界；新增成员本帧不推进；移除成员跳过并在压缩阶段处理；clear 通过成员关系解除，不只是清容器 | 用 runtime 调度算法实现，不要求复制 JS 空洞数组 |
| F4.25 | 根骨架挂钟会传播至现有 child；Slot 切换 child display 时也挂接；buildArmature 本身不挂钟 | 两个传播入口不可漏 |
| F4.26 | Animation 从 `ArmatureData.animations` 查数据；cacheFrame 写路径属于 P1-4 | 数据通过只读 Arc/借用访问，不把可变缓存伪装成不可变引用 |
| F4.27 | EventObject.animationState 在核心只写不读，golden 不 dump | 阶段 4 不加该字段，除非事件验收证明需要 |

### 0.4 阶段边界

阶段 4 完成动画数值、WorldClock 调度、Action Play 和帧级 golden。阶段 5 完成事件 listener dispatch、SOUND 双发、EventObject 池化及交叉动画用例矩阵。事件触发条件和入队时点若影响状态/Action，不得因消费延后而省略。

阶段 4 的事件验证走任务 1.4 固定、任务 7 落实的 seam：注册 9 类探针打开 listener 闸门 + `take_flushed_events()` 快照断言顺序，flush 不 dispatch；阶段 5 只把 flush 的“丢弃”换成 dispatch + 回收，事件产生路径不动。

---

## 1. Rust 运行时架构决策

### 1.1 唯一 owner 与 arena

由 `BaseFactory`（或本阶段新增的窄 `Runtime`，但不能出现双 owner）拥有：

```rust
struct Runtime {
    armatures: Vec<Option<Armature>>,
    clocks: ClockArena,
    // DragonBones data, event queue, parser/factory state as needed.
}

struct ClockArena {
    clocks: Vec<Option<WorldClock>>,
}

struct WorldClock {
    time: f64,
    time_scale: f64,
    system_time: f64,
    parent: Option<ClockId>,
    members: Vec<AnimatableHandle>, // stable order
}

enum AnimatableHandle {
    Armature(ArmatureIdx),
    Clock(ClockId),
}
```

设计要求：

1. `WorldClock` 不拥有 Armature，不保存 `&mut BaseFactory`，不保存 `Box<dyn IAnimatable>`。
2. Armature 与 Clock 都用 newtype handle 相互引用；容器 owner 唯一，禁止 `Rc<RefCell<_>>`。
3. `AnimatableHandle::Clock` 纳入本阶段，因为官方 WorldClock 本身实现 IAnimatable，嵌套 clock 属于其公开行为。循环嵌套必须避免递归栈溢出；测试定义官方允许的自环/环行为，不额外发明无限递归。环检测/断开属于**对官方行为的有意偏离**（官方 `add` 不防环），结论与依据登记进任务 8 的差异条目。
4. `Armature.clock` 可保留 `Option<ClockId>` 作为回指状态，但只由 runtime 关系操作修改；不能提供会绕过成员表的裸 setter。
5. handle 指向空 arena slot 时视为无效成员：推进时安全跳过并清理；dispose 要先 detach 再回收 slot。

### 1.2 RuntimeCtx：窄能力，不传万能 Factory

加入一个只在驱动/跨对象更新期间存活的上下文（名称可为 `RuntimeCtx` 或扩展 `ArenaCtx`）：

```rust
struct RuntimeCtx<'a> {
    armatures: &'a mut [Option<Armature>],
    clocks: &'a mut [Option<WorldClock>],
    // 当前 clock 的成员表由独立拆借字段/操作器提供。
}

impl RuntimeCtx<'_> {
    fn armature(&self, id: ArmatureIdx) -> Option<&Armature>;
    fn set_armature_clock(&mut self, id: ArmatureIdx, clock: Option<ClockId>);
    fn advance_animatable(&mut self, handle: AnimatableHandle, dt: f64);
}
```

签名可调整以满足 Rust 借用，但遵循以下实现模式：

- 从成员 Vec 取一个 handle 到局部值，再释放成员表借用。
- 对 Armature 使用 arena `take()` → 推进 → 放回原 slot；Clock 的调度循环由 runtime 持有，按每一步短借时钟字段和成员表，不取走整个 Clock 及其关系表。
- 正在推进的 Clock 必须仍可通过 `ClockId` 访问，其成员表/parent 关系始终可供 attach、detach、clear 和嵌套查询修改。不得把 Clock slot 置为 `None` 后再要求 context 找回该 Clock；“正在推进”与“已销毁”是不同状态。
- 推进上下文允许 `set_armature_clock`、Slot child display 接线和 Action 建立新 state；成员关系操作写回当前 clock 成员表。
- 避免持有一个跨整个推进循环的 `&mut members` 同时调用会修改该 Vec 的方法。
- 若使用 pending command queue，则每条命令须在官方对应操作时点生效；不能统一拖到整帧结束导致成员资格差异。

该上下文是 Rust 借用边界，不是 TS 对象的替代物。它只暴露当前更新路径确实需要的操作。

### 1.3 WorldClock 遍历语义

按 `WorldClock.ts:97-155` 实现语义，不照搬 `null` 空洞表示：

1. NaN→0；显式负 dt 用“当前系统时间 - 上次基准”替代；创建时初始化 system time。
2. 刷新 system time，应用 timeScale，再对缩放后的 0 早退。
3. `time += abs(passedTime)`，但成员收到带符号 passedTime。
4. 固定本轮开始边界。推进中新 add 的 handle 本轮不推进；本轮尚未到达的 handle 被 remove 后跳过；当前 handle 推进过程中 self-remove 不重复推进。
5. 遍历后安全整理成员顺序。Rust 可用 `Vec<Option<Handle>>` tombstone + `retain`/压缩，也可用快照与增量列表；选择必须通过下列行为测试，且不能用 `unsafe set_len`。
6. `clear()` 解除每个成员的回指及嵌套 clock 关系；清除后容器状态和所有 owner back-reference 一致。
7. `contains` 沿 clock parent 链查询，不把“直接成员”误当祖先包含语义。
8. 时间源可注入：`system_time` 与负 dt sentinel 依赖墙钟（TS `Date.now`）。提供可测 seam——内部 `advance_time_with(passed_time, now)`（实现名 `WorldClock::update_time`）、对外入口取真实 now，或可替换的 now 函数字段——否则任务 1 测试矩阵里 “system time 更新顺序、time 与成员 dt 符号符合 TS” 的断言无法稳定成立。

### 1.4 时间轴容器与确定性

TS 的 8 组 timeline 遍历序会影响首写者、pose reset 和 action 先后（F4.19/F4.20）。Rust 可以用强类型字段、固定数组或 enum+有序 Vec；只有当更新序被显式固定并经测试时，才可合并容器。任何 HashMap 只用于按键 lookup，不可依赖其 iteration 顺序。

AnimationState 引用使用稳定 `StateId`；骨骼/槽/约束目标用 `BoneIdx`/`SlotIdx`/`ConstraintIdx`。不要在状态机内部保存跨 arena Rust 引用。

- `ConstraintIdx` 按**创建序身份**定义（constraints 恒不重排，同阶段 3 `SlotIdx` 语义），不得以容器位置充当身份。
- 状态身份与播放顺序分开：`states: Vec<Option<AnimationState>>` 保存稳定 slot，`play_order: Vec<StateId>` 保存 TS 的 layer 排序、同层插入序和查询顺序。状态回收只从播放序列移除 ID 并释放 slot，不能通过压缩状态存储移动其他状态的身份。
- `StateId` 包含 slot 与 generation；slot 每次复用时递增 generation，查询必须同时校验 slot/generation。若初版不引入 generation，则明确禁止 slot 复用，直到 Animation owner 销毁；不能把旧 ID 重新解释为新状态。
- `last_state`、父/子状态与时间轴目标统一使用稳定 ID；不得保存 `play_order` 的位置。`get_state` 依 TS 语义倒序扫描播放序列，而非状态存储的 slot 顺序。
- 必须测试“回收一个状态不改变其他状态 ID”“复用 slot 后旧 ID 无效”“回收/新建后 layer 顺序及同层查询结果仍与 TS 一致”。

### 1.5 数值兼容的实现纪律

- 复制 JS 运算顺序，特别是除法位置、加减次序和 f64 中间值。
- 对 JS wrap/round 已有 helper（阶段 2）复用，不重复实现另一套。
- 未覆盖分支可用合成 timeline/IR 输入测试；不能以素材未触发作为删除理由。
- 遇到看似官方 bug，先 READ-ORIGINAL 并做最小差分测试；未经用户确认和差异登记不“修复”。

### 1.6 阶段 3 触碰面（任务 1 的前置改造）

Runtime/时钟重构会改动阶段 3 已提交的以下接缝，任务 1 开工即处理，完成后先确认阶段 3 回归再继续：

- **0 早退位置迁移**：阶段 3 现状在 `BaseFactory::advance_time` **最顶层** `dt == 0 → return`（当时以 WorldClock.ts:114 为据）；任务 1.4 要求 dt=0 时外层 objects/events 两段仍执行 → 该早退**下移进 `WorldClock::update_time` 内层**（clock 层早退，只跳过成员推进）。`armature_unit::advance_time_zero_is_noop_while_advance_one_drives` 的注释依据同步更新（断言本身仍应成立：挂钟成员在 dt=0 时不被推进）。
- **`ArenaCtx → RuntimeCtx` 签名迁移**：`Armature::advance_time(&mut ArenaCtx)` 的形参、`SlotUpdateCtx::world`、`Slot` 的 child clock 两处 TODO（Slot.ts:541/:549——正是任务 1.5 要落实的接线）、`BaseFactory::dispose_armature` 增加 detach。
- **推进驱动换轴**：`BaseFactory::advance_time(dt)` 从 arena 全遍历改为「`DragonBones` 三段 → 挂钟成员」驱动（对应常见错误 5）；`advance_one(idx, dt)` **保留**单骨架直驱语义（build 收尾与既有测试依赖它，即官方 `armature.advanceTime(0)` 直调形态）。
- 既有测试兼容性：`pose_golden`/`armature_unit` 不挂钟、只用 build 收尾 advance 与 `advance_one`——改造后必须**原样全绿**（任务 1 完成条件已含 workspace 回归）。

---

## 2. 任务依赖与实施步骤

执行顺序：**任务 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9**。任务 8 随手维护 TODO/差异清单。任务 7 的 golden harness 可在任务 6 批 2 完成 BoneAll 后提前搭建。

## 任务 1 — Runtime/Clock handle 调度与行为测试

目标：先证明 Rust arena 能表达官方 clock 成员资格和推进期间变更，再开始动画状态机。

前置：先完成 §1.6 的阶段 3 触碰面改造（0 早退迁移、ArenaCtx→RuntimeCtx、推进换轴），并确认阶段 3 回归全绿。

1. 设计并实现 `ClockId`、`AnimatableHandle`、`ClockArena/WorldClock` 和窄 runtime context。Armature/Clock 均由 runtime arena 拥有；禁止 trait-object ownership。
2. 提供统一关系入口：attach/detach armature、clock parent attach/detach、clear、dispose。armature 的 clock back-reference 和成员表必须原子式维护。
3. 实现 `WorldClock::update_time`（F4.8/F4.24 时间策略）与 runtime dispatch。WorldClock 保持时间策略；runtime 按 handle 解析并推进对象。
4. 接入 `DragonBones.advance_time` 三段顺序：objects flush（Rust 暂无对象池，可空操作）→ clock advance → events flush。dt=0 不跳过外层两段。事件 seam（`S4-Event`）：
   - `buffer_event` 真入队；TS:334-338 的 `indexOf` 是对象身份去重，不是字段内容去重。阶段 4 按值拥有事件时，每次产生的新事件直接追加，禁止用 `PartialEq`/`Vec::contains` 去掉同值事件；若引入事件 handle，仅对同一 handle 去重。`buffer_object` 本阶段 no-op 占位；
   - flush 点把队列**整体移入“本帧已 flush”缓冲**：`take_flushed_events() -> Vec<EventObject>`（取走即清；或 `advance_time` 直接返回本帧快照，二选一），`BaseFactory` 透传该 seam。flush 本身**不 dispatch、不调用任何 listener**，禁止“静默清空后无法验证”；
   - 每次 flush **替换**上一帧快照，不追加；本帧事件为空也必须用空快照替换。`take_flushed_events()` 取走后缓冲为空，重复取返回空；dt=0 也执行这一 flush 规则。若选直接返回快照，保持同样的逐次调用边界；
   - 注意可见性：`tests/` 是集成测试（外部 crate），`pub(crate)` 看不到——seam 用 `pub` + `#[doc(hidden)]`（或 dev-feature 门控），不要 `pub(crate)`；
   - 阶段 5 把 flush 的“移入缓冲”替换为 `dispatchDBEvent` + SOUND 双发 + 池化回收，事件产生路径与队列顺序保持不变。

   **S4-Event 验收**：分别产生的两条同值事件都保留；有事件帧之后的无事件帧快照为空；同帧重复 take 第二次为空；dt=0 能 flush 已缓冲事件且不推进骨架。listener 闸门的正/反向用例在任务 7 验证。
5. 接入 Armature clock propagation 两处：根挂钟传播至已有 child（Armature.ts:904-910）；Slot display child 切换时继承 parent clock（Slot.ts:541/549）。buildArmature 不自动挂钟。dispose/clear 必须 detach。

**必须覆盖的 clock 测试**（不依赖动画数据）：

| 用例 | 断言 |
|---|---|
| 新建/直接 attach/detach | 成员顺序、回指一致；重复 attach 幂等 |
| timeScale 0/0.5/2 | 0 早退；时间累计与传入成员 dt 符合 TS |
| dt=NaN、正数、负数 sentinel | system time 更新顺序、time 与成员 dt 符号符合 TS |
| 未挂钟与已挂钟 Armature | 只有 clock 成员被 runtime advance；`advance_one(0)` 仍跑单骨架更新 |
| 推进 A 时新增 B | B 本轮不推进，下轮按插入序推进 |
| 推进 A 时移除尚未访问的 B | B 本轮跳过，关系回指清除 |
| A 推进中移除自身 | 不崩溃、不重复推进；之后成员表可继续使用 |
| clear 多成员 | 所有 Armature/Clock back-reference 清除，之后可重新 attach |
| 根挂钟与 child 切换 | 已存在 child 递归挂接；切换旧 child 摘除，新 child 加入 |
| dispose clock member | arena slot 清空前摘除成员，下一次 advance 无 stale handle |
| 嵌套 clock/contains | parent 链查询正确；嵌套 clock 按 handle 推进且无无限递归 |

**任务完成条件**：以上测试通过；`cargo test --workspace` 阶段 0-3 回归通过；clock 设计没有未实现的公开假方法。未覆盖的循环 clock 语义写入“差异/限制”而不是留静默 TODO。

## 任务 2 — AnimationConfig/AnimationData 核对 + Animation 容器骨架

1. `AnimationConfig::clear/copy_from` 对照 `AnimationConfig.ts:279-296`：`fadeOutMode=All`、`timeScale=-100`、`playTimes=-1`、`autoFadeOutTime=-1` 等默认逐字段测试。
2. 核对 `AnimationData`/`TimelineData` 已有模型字段；不因本阶段重写 model。
3. Animation 拥有状态 slot 存储与独立 `play_order`，引用使用 §1.4 的稳定 `StateId`；采用 slot 复用时同时实现 generation 检查，否则落实禁止复用和旧 ID 失效规则。按需实现有序动画表和 blend state lookup；lookup 容器 iteration 不参与结果。
4. 先立 API 签名与类型，不留一组无调用者的 `todo!()` API；逐个由后续任务填体。签名按 `animation.md §1.3`，特别是 play 的 `play_times: i32`、`stop(Option<&str>)`、fadeIn 的参数。
5. `Animation::on_clear` 对照 `Animation.ts:68-100`：清理 states、播放顺序、动画数据绑定、blend states 与 config，复位 timeScale/inheritTimeScale/dirty 等字段；替换 Armature 清理路径中旧 `reset()+init`。同步使旧状态 ID 失效；`reset` 与 `on_clear` 的不同语义不得合并。
6. config 双借用采用值语义、本地 config 再写回或等价方案；`play_config` 的归一化写回不能丢。

## 任务 3 — BaseTimelineState 与可独立验证的时间算法

对照 `timeline_states.md §1-3` 实现五种基类职责。Rust 可通过组合/enum 表示，不必复制 TS 抽象继承树；必须保持派生轴看到相同字段和更新钩子。

- `_set_current_time` 三分支与负数/循环边界按 F4.14。
- 帧定位 O(1) 查 `frameIndices`；验证 frameCount+1 尾项、duration 精度补偿（F4.2）。
- tween frame position/duration、有限循环最后一帧不 tween，按 F4.2/F4.16。
- `get_easing_value`、曲线表插值函数按 TS 表达式；运行期不重新采样 Bezier。
- `playState`、`dirty` 状态生命周期按 TS；magic number 转 enum 时列出映射表。

**单测**：时间归一化三分支、正/负整数倍、负向边界 currentTime=duration、首/末帧查表、每类 easing、最后一帧 tween 禁止条件。

## 任务 4 — AnimationState 与 BlendState

1. 字段初始化按 `AnimationState.ts:251-311`；Rust 删除没有观测价值的临时池字段，但默认值测试和 reset 行为必须覆盖。
2. 使用 `FadeState`、`SubFadeState`、`PlayheadState` 表达官方值域；状态迁移对照 F4.3/F4.13。
3. `_advanceFadeTime` 的线性 fade、负时间取正、fade event 入队时点和 parent/actionEnabled 条件按 F4.10。
4. `advance_time` 顺序按 `AnimationState.ts:808-1070`：fade → playhead/timeScale → timeline rebuild → action 轴（在 weight early-return 前）→ 固定组更新 → fade complete/auto fade。
5. BlendState 保留每帧 reset、预算权重计算、dirty 首写者 pose 初始化和 target 去重（F4.19）；用确定性容器/循环。
6. cache path 按 F4.15 保留结构；仅实现 cacheFrameRate=0 下不会进入的分支时，须在量化/写入分支就地注释明确 P1-4 未完成及其当前不可达条件，不宣称缓存支持。
7. `fade_out` 的 longer-fade guard、progress continuity、五种 fadeOutMode 过滤逐项覆盖（F4.12）。

**单测**：fade in/out 状态转移、pauseFadeIn、fade 独立于 state timeScale、weight=0 时 action 仍执行、blend 首写者、相同 target 去重、fade-out 连续性。

## 任务 5 — Animation 顶层 API 与状态管理

对照 `animation.md §3`：

1. `advance_time`：负 dt 取正只在此层；父继承 timeScale 读取同帧 runtime 状态；blend states reset；单/多/零 state 分支和缓存 frame index 更新顺序按 TS。
2. `play_config`：查表失败、Single 复用、config normalize 写回、fade-out filtering、state 创建/插入、child animation inheritance、子动画状态创建顺序逐项移植。
3. `play`、`fade_in`、`goto_and_play_*`、`goto_and_stop_*`、`stop` 和查询 API 逐个映射 TS 默认/副作用；`play` 与 `fadeIn` 不能共用错误的默认 fadeOutMode。
4. 父子 AnimationState 用 `StateId`/owner handle，不保存跨 arena 引用。RuntimeCtx 只在需要访问其他 Armature/状态时提供窄查询/命令。
5. 子骨架 clock membership 来自任务 1 的 runtime 关系入口；Animation 不直接改 clock 成员表。

**单测**：config normalize 写回、Single 早退不改 config、play vs fadeIn 淡出范围、gotoAndStop freeze、stop 单状态/全部、get_state 倒序、isCompleted 空列表 false、child animation inheritance。

## 任务 6 — 16 类 TimelineState

依赖顺序与原 TS 名称/公式见 `timeline_states.md §3-5`。每个类先列出：输入 IR 数组与 offset 基准、每帧值数量、写入 runtime 目标、dirty 标记、结束帧行为和差异测试，再实现。

**批 0 前置**：补 `Armature::sort_z_order`（`Armature.ts:185-207`）和 `Constraint::invalid_update`（`Constraint.ts:218-224`），确保行为用例先行。

**批 1：Action/ZOrder**

- ActionTimelineState 是主要跨帧扫描轴；逐个经过的 action frame 按方向触发。
- Play action 无 listener 闸，追加到 Armature action queue；Armature 同帧消费并启动目标 animation。
- Frame/Sound/start/loopComplete/complete 的 event 条件与 enqueue 点保留；实际 dispatch 留阶段 5。
- ZOrder 轴调用 sort_z_order；Action 帧位置使用除法（F4.16）。

**批 2：BoneAll/Translate/Rotate/Scale**

- 值数量 6/2/2/2；帧值三基准链；scale 落点按 F4.17。
- 先更新 animation pose，再按 BlendState 写最终 pose；保持 normalize/末帧/pose fallback 行为。

**批 3：SlotDisplay/SlotColor/SlotZIndex/Alpha**

- SlotColor 间接索引 colorArray 和 wrap 修正；fade color 使用官方 progress 曲线。
- SlotZIndex 的 `+=` 与绝对赋值、displayController gate、displayIndex 终值行为保持。
- Alpha 双用 BoneAlpha/SlotAlpha，写对应 dirty 标记。

**批 4：Surface/Deform**

- deform header 五值和 IR index 3；same-value offset/wrap、权重/顶点边界按 TS。
- 写 Surface/DisplayFrame deform data 的分支和 geometry identity guard 保持。
- 9 素材未覆盖的 JSON 退化分支照原实现保留并用合成数据测试可达部分。

**批 5：IK/AnimationProgress/Weight/Parameters**

- IK current/result 字段严格按 TS 写点，并调用 constraint invalidation。
- Animation 三轴仅 parent state 存在时修改子状态；用合成数据覆盖素材未覆盖分支。

**每批验收**：轴级合成 IR 单测 + 涉及资产的 golden 子集；失败先核查 offset、wrap、scale、遍历序，再排查 libm。

## 任务 7 — 逐帧 golden harness 与状态行为测试

新增 `tests/golden_anim.rs`，复用阶段 0 的 9 资产 × 全动画 × 120 帧 golden。构造流程：parse → build → 显式 attach 根骨架到 runtime clock → `goto_and_play_by_time(anim, 0, -1)` → 每帧先推进后采样。

对每帧比较骨骼 13 值和槽 15 值。还需比较骨/槽名称与顺序元数据——**末态比对**（jsdump 在 120 帧跑完后才写 `meta.slotNames`）。

事件验证口径（任务 1.4 固定的 seam，逐帧执行）：

1. harness 在 armature proxy 上为需验证的 **9 类事件注册探针**，使对应 `hasDBEventListener` 为真——探针只为打开入队闸门，flush 不 dispatch，测试期间不会被调用；
   注册必须发生在 `goto_and_play_by_time` 等可能产生 fade 事件的播放入口**之前**（对照 `jsdump.mjs:103-117`），播放入口产生的事件保留到第一次 runtime flush；探针回调可直接 panic，以证明本阶段未 dispatch；
2. 每帧推进后用 `take_flushed_events()` 取本帧快照，与 golden 的 `frames[i].events` 按 `(type, name, bone, slot)` **逐帧全等**比较（`golden_events` 断言可直接复用参照；事件同帧 flush，时序与 jsdump 一致），比较后丢弃；
3. 正式 dispatch 对拍（调用真实 listener 回调）留阶段 5；阶段 4 的验收目标是事件产生条件、入队时机与队列顺序正确，且全程不调用 listener。

另加 listener 闸门反向测试：不注册 listener 时，Frame 与生命周期事件按官方各自闸门不入队；Sound action 即使没有 listener 仍入队（仍须满足 actionEnabled 等外层条件）；Play action 不经事件队列且不受 listener 开关影响。该测试不能用“事件队列总是为空”作为无 listener 场景的断言。

容差初始 `1e-9 * (1 + |want|)`；只有证实是系统 libm 差异后才按 requirements 降至 `1e-6 * (1 + |want|)`，在验收记录报告最大绝对/相对误差，不静默放宽。

额外 animation tests：

1. gotoAndStopByFrame 后时间推进姿态冻结；同帧 gotoAndPlay 恢复结果与首采样一致。
2. resetToPose 让未被动画覆盖的 bone/slot 回到 model pose；若素材无未覆盖目标，用合成骨骼/slot 测试。
3. 子骨架 inheritAnimation 的 StateId 创建、clock propagation 和父子时间缩放顺序。
4. state timeScale=2 的前 60 帧与正常前 120 帧对拍；反向状态 timeScale 覆盖负向推进。
5. WorldClock 成员变更/嵌套用例使用任务 1 的 runtime tests，不在 golden harness 里另造另一套驱动。

## 任务 8 — 收口、清理和差异登记

- 清点阶段 3 遗留 TODO：`_sort_z_order`、`Constraint::invalid_update`、Animation 清理路径、Slot child clock 接线。
- `todo!()` 在阶段 4 范围内必须为零；允许的 `TODO P1-4/P1-5/阶段5` 要有明确调用条件与不影响本阶段验收的证明。
- 检查所有新增 handle 都有 owner、失效规则和测试；不得有 stale member、未同步 back-reference 或 `unsafe` 容器长度操作。
- 更新差异登记：仅记录确实未实现/不可达/行为不等价的条目，写明证据和后续阶段，不用“Rust 实现不同”笼统带过。
- 对代码做一次逐轴 TS 原文 review，重点核对 F4.5/F4.6/F4.14/F4.17/F4.18/F4.19。

## 任务 9 — 验收

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node tools/gen_pose.mjs
```

**阶段 4 完成标准**：

1. 9 资产 × 全动画 × 120 帧 bones/slots golden 逐元素通过；报告容差和最大观测误差。
2. clock 行为测试全绿：显式成员资格、timeScale、零早退、成员 add/remove/clear、child propagation、dispose、嵌套 clock/contains 与 cycle 防护。
3. gotoAndStopByFrame、resetToPose、child animation inheritance、AnimationState timeScale 测试通过。
4. 阶段 0-3 回归全绿；clippy 无警告。
5. 阶段 4 范围内无未解释 `todo!()`；P1 cache 的保留/量化/写入分支须就地标注 P1-4 与不可达条件（见任务 4.6）。
6. 任何未实现的官方可观察行为都写入差异登记；不把“素材没覆盖”当作删实现的理由。
7. 事件 enqueue 验证（`S4-Event`）：9 类事件逐帧 `(type, name, bone, slot)` 快照与 golden 一致；同值独立事件不被吞掉、逐帧快照替换/重复 take/dt=0 flush 测试通过，listener 闸门正/反向测试通过；全程未调用 listener（正式派发对拍属阶段 5）。

## 常见错误速查

1. TS 结构不等于规格；arena/runtime 中央维护关系，不模拟任意可重入 setter。
2. 不用 `Box<dyn IAnimatable>` 保存 arena 对象；稳定 handle 才是跨子系统引用。
3. clock 成员遍历不能借用 Vec 时调用会修改该 Vec 的回调；先取 handle、释放借用，再通过 context 推进。
4. 不用 `unsafe set_len`；Vec 压缩使用安全操作并验证插入顺序。
5. 只有显式挂钟的骨架参与全局推进；buildArmature 自己不挂钟；单骨架 `advance_one(0)` 与全局 clock `advance_time(0)` 不同。
6. 负 dt 不在 WorldClock 取绝对值；Animation 层与 AnimationState 层按 F4.7 分工。
7. `play` 的 All 与 `fadeIn` 的 SameLayerAndGroup 默认不同；Single 在 `_fadeOut` 是空 case。
8. fade 先于 state timeScale；`playhead == 3` 才推进时间。
9. Action Play 无 listener 闸；必须与 event queue 分离。
10. Timeline group 顺序、BlendState reset/dirty first-writer 是数值契约。
11. 三基准 IR offset、三处 `+=65536`、运行时 `_valueScale` 还原不能省。
12. IK/Deform 的 READ-ORIGINAL 分支不能因当前素材没覆盖就删除。

## 完成后

提交验收输出、逐资产 golden 通过摘要、最大差异、clock 行为矩阵和差异登记供 review。Review 完成后再编写阶段 5 指南：事件派发、对象池、replace API 和交叉淡入/事件矩阵。
