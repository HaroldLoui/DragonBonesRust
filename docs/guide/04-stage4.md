# 阶段 4 实现指南 — 动画系统

> 配套 `docs/requirements.md` §4 阶段 4。按任务 1→8 顺序实现，验收清单全绿后告诉我，我 review 后写阶段 5 指南。
> **基准 = 官方仓库实现**（`DragonBonesJS/DragonBones/src/dragonBones/`），不以素材覆盖裁剪行为——官方有、素材没打到的路径也要照实现（能单测的配单测，不能的结构照抄+注释标注）。
> 本指南所有"实测"结论均已对官方 TS 源码逐行核对；**伴随研究笔记在本机 `target/stage4_notes/`**（`clock_driver.md` / `animation.md` / `animation_state.md` / `timeline_states.md`，含逐行伪代码与行号索引，实现时随手翻），笔记是本指南的展开版，冲突时以官方 TS 原文为准。
> **编号说明**：本指南设计决策用 `DD-4.N`（`model_mapping.md §4` 的偏差编号 `D-4.N` 已占到 D-4.31，避免撞号）；实测事实用 `F4.N`。

---

## 0. 已实测事实与 requirements 修正（先读）

| # | 事实 | 影响 |
|---|---|---|
| F4.1 | **requirements 行数是"非空行"口径**，实测（总行数）：`AnimationState.ts` **1602**（非 1445）、`Animation.ts` **959**（非 887）、`BaseTimelineState.ts` **570**（非 500）、`TimelineState.ts` **1074**（非 931）、`WorldClock.ts` **256**（非 240）、`DragonBones.ts` **400**（非 375） | 总量比 requirements 估算多约 15%，排期按 8–12 天上限走 |
| F4.2 | requirements 任务 3 三处名不副实：①**帧定位不是二分查找**——`frameIndices[frameIndicesOffset + floor(currentTime*frameRate)]` O(1) 查表（BaseTimelineState.ts:200-201）；②**运行期没有贝塞尔**——`_getCurvePoint`/`_samplingEasingCurve` 是**解析期**（ObjectDataParser.ts:134/:147），运行期只有 `_getEasingValue`(:233)/`_getEasingCurveValue`(:253) 两个纯查表线性插值函数 | 任务 3 按笔记 `timeline_states.md §2` 实现，别去找不存在的二分/递归 |
| F4.3 | **官方没有 `FadeState` 枚举**（requirements 决策 1 的值域不成立）：三个裸 `number`（AnimationState.ts:195-209）——`_fadeState` ∈ {-1 Fade in, 0 Fade complete, 1 Fade out}、`_subFadeState` ∈ {-1 Fade start, 0 Fading, 1 Fade complete}，**都没有 2 且同值不同义**；`_playheadState` 是位掩码 {2 Play, 1 Fade Play}，**只有 ===3 推进时间** | 见 DD-4.2：**两个枚举 + 一个位类型**，比较处 40 个点逐条对齐（笔记 `animation_state.md §10.2`） |
| F4.4 | **17 个 `TimelineType` 值 → 16 个具体类**：`BoneAlpha=60` 与 `SlotAlpha=24` 共用 `AlphaTimelineState`（AnimationState.ts:440/:599）；类名拼写实测是 **`MutilpleValueTimelineState`**（缺 t，勿"纠正"）；**BoneAll 是 6 值/帧**（x,y,rotation,skew,scaleX,scaleY；TimelineState.ts:314） | 批次实现按 16 类；字段命名保留官方拼写并注释 |
| F4.5 | **三基准读取链**：`abs = (frameIntOffset \| frameFloatOffset) + timelineArray[tl.offset+TimelineFrameValueOffset] + frameIndex*valueCount`，其中 `_frameValueOffset` 按 `FrameValueType` 三选一（Step=0 / Int 相对① / Float 相对②，parser:1253-1265） | 公式与逐类型表见笔记 `timeline_states.md §3`——**这是本阶段最核心的数据通路**，任务 6 全部批次共用 |
| F4.6 | 异形轴三处：**SlotColor** 间接寻址（frameIntArray 存 colorArray 下标，`+=65536` wrap 修正 :678/:699）、**Surface/Deform** 用 `TimelineFrameValueCount`（**index 3，不是 4**）取 5 元 deform 头（:527/:883，Deform 版还有 :893-897 的 wrap 修正）、**Action/ZOrder/SlotDisplay** 是 Step 型值在 `frameArray` | 三处 wrap/异形照抄，漏了就是符号位分叉 |
| F4.7 | **负时间三层接力、全库无 `.abs()`**：WorldClock 原样缩放（:110-112，`time` 只前进）→ `Animation.ts:173-175` 硬取正（注释 "Only animationState can reverse play."）→ **反向播放唯一实现点是 `AnimationState.ts:815-817` 自身带符号 timeScale** | abs 只在 Animation 层做一次；clock 层不取正 |
| F4.8 | **`WorldClock.advanceTime(0)` 早退（:114-116）只跳推进**：`_systemTime` 刷新（:108）与 timeScale 缩放（:110-112）都发生在早退**之前**；`DragonBones.advanceTime` 的三段（objects flush :305-311 → clock :313 → events flush :315-331）**无条件执行** | 阶段 3 已按此改了 `BaseFactory::advance_time(0.0)` 早退（保持）；timeScale 必须排在 0 判定前 |
| F4.9 | **Play action 与事件是两条队列、闸门不同**：`ActionType.Play` → `_bufferAction(ev, true)`（TimelineState.ts:43-49）**无监听器闸门 → 数值相关，阶段 4 必须真做**；Frame/Sound/start/loopComplete/complete 走 `bufferEvent` + `hasDBEventListener` 闸 → 纯事件可推阶段 5 | 阶段 4/5 边界的唯一分水岭（DD-4.5） |
| F4.10 | `start/loopComplete/complete` **不在 `_advanceFadeTime`**（requirements §590 表述易误导），在 `ActionTimelineState.update`（TimelineState.ts:85/:103/:111），且 `loopComplete` 推迟到跨过第 0 帧才 buffer（:156-159/:215-218/:240-244，最后 :251-257 兜底 flush） | 事件体阶段 5，但**触发逻辑结构属于 Action 轴**（任务 6 批 1 照抄结构，入队走占位） |
| F4.11 | `gotoAndStopByFrame` **官方真有**（Animation.ts:713-720 = `gotoAndPlayByFrame(name, frame, 1)` + `state.stop()`）；**`resetToPose` 不是方法**——只是 `AnimationConfig.resetToPose`（:114，默认 true）→ 拷进 `AnimationState.resetToPose`（:81）的**字段**；**`Animation` 没有公开 `fadeOut`**（公开版在 AnimationState.ts:1111，Animation 只有私有 `_fadeOut:102`） | 验收三条按此落地（任务 8）；requirements P0-9 的 "fadeOut" 指 `_fadeOut` 过滤器 |
| F4.12 | `play()` 与 `fadeIn()` 的 fadeOutMode **不同**：`play` 走 `clear()` → **All(4)**（AnimationConfig.ts:279），`fadeIn` 参数默认 **SameLayerAndGroup(3)**——"同名重播"用哪个入口决定淡出范围；`_fadeOut` 里 **`Single` 是 `// TODO` 空 case**（Animation.ts:143-151 附近），Single 真逻辑在 `playConfig:352-362`（复用既有 state 早退） | 照抄，勿把 Single 挪进 `_fadeOut` |
| F4.13 | **fade 推进在 timeScale 缩放之前**（AnimationState.ts:810-812 vs :814-817）→ 单状态 `timeScale` 不影响 fade；`pauseFadeIn` 默认 true（AnimationConfig.ts:286）→ init 起 `_playheadState=2` 冻结到淡入完成 `|=1`（:705/:749） | 顺序照抄；playhead 位语义见 F4.3 |
| F4.14 | **时间归一化只在 `BaseTimelineState._setCurrentTime`（:92-161）**，且三分支：0/1 帧轴镜像 action（:97-101）、action 自身/带 timeScale·timeOffset 的真算（:102-141）、**普通多帧轴根本不归一化、直接抄 action 结果**（:142-146）；负数**先取反再 floor**（:130-133）→ `currentPlayTimes ≥ 0` 恒成立；负向整数倍时刻 `currentTime = _duration`（正向是 0）→ 不对称落到 `frameIndices` 多分配的第 `frameCount` 项 | 分支结构照抄；`frameIndices` 有 `frameCount+1` 项承接该越界（parser:1272） |
| F4.15 | **缓存帧（P1-4）条件已实测**：`isCacheEnabled = (_fadeState===0) && cacheFrameRate>0`（:832，淡入淡出中都不缓存）；**多状态恒传 cfr=0**（Animation.ts:250 vs :230 单状态传真值）；素材不设 `armature.cacheFrameRate` → `:850-875` 整段短路 | 阶段 4 实现**结构保留、体留 `// TODO P1-4`**，但 `:832` 判断必须在（rate=0 时整段不进，零风险） |
| F4.16 | Action 轴帧位置用 **`frameArray[pos] / frameRate` 除法**（6 处；`* frameRateR` 版本被官方注释 `// Precision problem`） | Rust 必须用除法，勿"优化"成乘法 |
| F4.17 | **骨架 scale 两套落点**：BoneAll 在 `blend()` 里乘（:325/:334-343）；BoneTranslate/Surface/Deform 在 `init()` 置 `_valueScale` 读取期乘；BoneRotate/BoneScale **不乘** | 两处都要实现，别只做一处（笔记 §3.3 表） |
| F4.18 | **IR 读取三件套**：`frameFloatArray` 已是 f32 量化（与 JS Float32Array 位级同）→ Rust 按 f64 读原值即可；`frameIntArray` 是 i16/u16 wrap 后的符号扩展 → **`+=65536` 修正三处必须原样**；parser 写侧 `round(v*scale)` 与运行侧 `_valueScale`（0.01/0.0001）是**量化-还原**关系，运行侧这步不能省 | 用 f64 直读/漏还原会引入 1e-7 级偏差撞穿容差（model_mapping §4.1） |
| F4.19 | `BlendState` 在 **AnimationState.ts:1528-1601**；两层键 = `Animation.getBlendState(type→name→BlendState)`（:748-760），**每帧开头全部 `reset()`**（:188-193）；`update` 用 leftWeight(1.0 预算)/layerWeight/blendWeight 层优先分配，`dirty===1` = 首个写入者（先清零该骨 pose :894-902）；**官方不存在 `_fadeStates` 字段** | requirements"两层字典每帧 reset"正确；无 `_fadeStates` |
| F4.20 | `AnimationState` **没有 `fadeIn()` 方法**（淡入 = init 时 `_fadeState=-1/_subFadeState=-1` 自然开始）、**没有 `_normalizedTime`**、**没有 `_timelineStates` 字典——是 8 个强类型数组（:229-236）** | DD-4.3；`start` 之类事件在 init/advance 里发 |
| F4.21 | `DragonBones` 5.7 核心**没有 `dispatchEvent/addEvent/removeEvent`**——派发只有 `DragonBones.advanceTime:321 → armature.eventDispatcher.dispatchDBEvent`，`_eventManager` 只接 SOUND（:322-324） | 阶段 5 按此实现；阶段 4 只搭三段骨架（DD-4.5） |
| F4.22 | **两个前置缺口**（阶段 3 留的 TODO，本阶段必须补）：`Armature::_sort_z_order`（Armature.ts:185-207，ZOrder 轴 3 个消费点）与 `Constraint::invalid_update`（Constraint.ts:218-224，IK 轴每帧调 :980） | 任务 6 批 0 先补 |
| F4.23 | 两条 **READ-ORIGINAL** 待差分确认：① IK 轴 `_onUpdateFrame` 写 `_currentA/_currentB`（:971-972）而非 `_resultA/_resultB`，非 tween 帧 `_current*` 只在 tween 时赋值（BaseTimelineState:447-463）→ 可能读到陈旧值/初值 0.0（=weight 0、bend false）；② Deform JSON 路径退化使 `_parseDeformFrame` 头两分支不可达（dbbin 或不同）。9 素材 291 条 curve 全为长度 4 → `_curveCount` 负分支不可达 | 先照抄，差分打不过再回原文；负分支照抄不许"修复" |
| F4.24 | `WorldClock` 迭代：长度 `l` 循环前**定格** → 本帧 `add` 的成员不推进；`remove` 置 null 跳过、空洞下帧压缩；**`clear()` 不清数组**——只把成员 `clock` 置 null 触发 setter 幂等闸（:241-254）回摘 | Rust 直译不能写成 `vec.clear()`（成员回指未复位=分叉）；笔记 §2 有完整表 |
| F4.25 | `Armature._clock` 的**子骨架传播在 clock setter 里**（Armature.ts:904-910：挂钟时给已有子骨架递归挂同钟）+ `Slot._updateDisplay:549`（切换显示时挂）——**子骨架入钟只有这两个入口** | 漏掉 → 子骨架不被推进 → 数值差 |
| F4.26 | `Animation` 拿数据：`_animations` 挂 `ArmatureData.animations` **引用**（Armature.ts:290），查表失败 warn+null（:339-348）；`replaceAnimation` 允许挂任意合并子集 | Rust `Arc<DragonBonesData>` + `armature_name` 直查（阶段 6 P1-5 的 replaceAnimation 留 TODO）；运行时**写** `AnimationData.cached_frames` 的硬冲突（笔记 §4.3）→ P1-4 前不可达，届时再议 RefCell/偏差 |
| F4.27 | `EventObject.animationState` 全库**只写不读**（写点 AnimationState.ts:680/:716、Action Timeline :47；core 无读者，jsdump 也不 dump） | 阶段 4 **不建该字段**，阶段 5 若事件用例需要再建（登记于代码注释） |

---

## 1. 设计决策（DD-4.1 – DD-4.8）

### DD-4.1 模块布局（D7：1 TS 文件 → 1 Rust 模块）

```text
src/animation/mod.rs            ← 重写：Animation（Animation.ts 959 行）+ pub mod
src/animation/world_clock.rs    ← WorldClock（256）+ IAnimatable 语义（76，无对象可言→并入）
src/animation/animation_state.rs← AnimationState（1602）+ BlendState（同文件 :1528-1601）
src/animation/base_timeline_state.rs ← 5 个抽象基类（570）
src/animation/timeline_state.rs ← 16 个具体类（1074）
src/core/dragon_bones.rs        ← 常量 struct 保留，**新增实例部分**（见 DD-4.5；阶段 5 继续长）
src/armature/armature.rs        ← +clock 字段/getter/setter/子骨架传播、_sort_z_order 真身、
                                   on_clear 里 animation.reset→on_clear（R4）
src/armature/constraint.rs      ← +invalid_update（F4.22）
src/factory/base_factory.rs     ← +_dragonBones 字段注入、clock() getter、advance_time 走三段
```

`AnimationConfig`/`AnimationData`/`TimelineData` 阶段 2 已在 `model/`，本阶段**只核对不重写**（任务 2）。

### DD-4.2 fade 子状态 = 两个枚举 + 一个位类型（修正 requirements 决策 1）

```rust
/// 对齐 `_fadeState`（AnimationState.ts:199-205 注释）：值域 {-1,0,1}，**无 2**。
#[repr(i8)] enum FadeState { FadeIn = -1, FadeComplete = 0, FadeOut = 1 }
/// 对齐 `_subFadeState`（:206-209）：**同数字与 FadeState 不同义**（-1=Fade start, 0=Fading, 1=Fade complete）。
#[repr(i8)] enum SubFadeState { FadeStart = -1, Fading = 0, FadeComplete = 1 }
/// 对齐 `_playheadState`（:195-198 "Play Enabled, Fade Play Enabled"）——位掩码：
/// bit1(2)=Play（用户层），bit0(1)=Fade Play（淡入完成解锁）；**只有 3 才推进时间**（:814）。
#[repr(u8)] struct PlayheadState(u8);   // stop(): &=1；fadeOut(pause): &=2；init: pauseFadeIn→2 否则 3
```

**不许用魔法数字**，但 40 个比较点（笔记 `animation_state.md §10.2` 列全）必须逐条语义等值；`2` 只出现在 `_timelineDirty` 与位值里——`_timelineDirty` 单独用小整数枚举。

### DD-4.3 `AnimationState` 的时间轴容器 = 8 个强类型字段（修正 requirements 决策 2）

官方不是 `enum TimelineStateKind + Vec`，是 8 个强类型数组（:229-236）且 `advanceTime` 按**固定分组序**遍历（action → zOrder → bone → boneBlend → slot → slotBlend → constraint → animation，:842-1015）。Rust 跟随官方：

```rust
pub struct AnimationState {
    pub action_timeline: Option<ActionTimelineState>,
    pub z_order_timeline: Option<ZOrderTimelineState>,
    pub bone_timelines: Vec<BoneAllTimelineState>,        // 见下注
    pub bone_blend_timelines: Vec<…>, pub slot_timelines: Vec<…>,
    pub slot_blend_timelines: Vec<…>, pub constraint_timelines: Vec<…>,
    pub animation_timelines: Vec<…>,
    // + pose_timelines（resetToPose 兜底，:1022-1060）、8 数组之外的字段见笔记 §1
}
// 注：骨四类（All/Translate/Rotate/Scale）与 Blend 两类官方也是分数组
//（:408-434 等 4 组 + _boneBlendTimelines…）——照 AnimationState.ts:229-236 实测的
// 8 个数组名一一落成字段，别自造分组。
```

分组遍历序是契约（影响 BlendState 的 `dirty` 首写者与 `prevTarget` 去重，:889-892）。

### DD-4.4 借用策略（沿用阶段 3 的"拆字段 + ctx"三板斧）

1. **`Armature::advance_time` 内**：`let Self { animation, bones, slots, constraints, actions, … } = self;` 字段级解构后，`animation.advance_time(passed_time, &mut AnimCtx { bones, slots, actions, constraints, scalars…, world })` ——新增 **`AnimationCtx`**（对照 `SlotUpdateCtx` 先例，`armature/slot.rs:239`）：
   - `bones: &mut [Bone]`、`slots: &mut [Slot]`、`constraints: &mut [Constraint]`、`actions: &mut Vec<EventObject>`（Action 轴 `_bufferAction` 用）；
   - 骨架标量（写点都在时间轴/状态）：`cache_frame_index: &mut i32`、`alpha_dirty/z_index_dirty/slots_dirty: &mut bool`、`armature_idx`、`inherit_animation: bool`、`cache_frame_rate: f64`（数据侧）；
   - `world: &mut ArenaCtx`（跨骨架：子骨架 fadeIn 继承 Animation.ts:443-452、inheritTimeScale 父链 :178——父必须先推进，成员资格即数值）；
   - proxy 引用怎么办：`hasDBEventListener`/`bufferEvent` 是事件面 → 阶段 4 事件走**全局 `DragonBones` 占位**（DD-4.5），不摸 proxy，`Armature` 持 `_dragonBones: Option<ArmatureIdx?…>` 见 DD-4.5 的取舍。
2. **`Animation::play/fadeIn/gotoAnd*` 的 config 双重借用**（TS `this.playConfig(this._animationConfig)`）：Rust 用 **`mem::take(&mut self.animation_config)` → 本地填 → `play_config(&mut cfg)` → 写回**，或 `play_config(AnimationConfig) -> AnimationStateRef` 值语义。任选其一，**归一化写回行为必须保留**（playConfig 原地改 cfg：F 见笔记 `animation.md §3.2`——首播 fadeInTime 强制 0、fadeOutTime 缺省=fadeInTime、timeScale≤-100 → `1/ad.scale`、负 position `duration - (pos%dur)`…）。
3. **状态引用**：`AnimationState` 由 `Animation.animation_states: Vec<AnimationState>` 拥有；跨对象引用（`last_animation_state`、`EventObject` 若阶段 5 建字段、`_parent` 子状态）用 **`StateRef(索引)`/`(ArmatureIdx, u32)`**，禁 `Rc`（D3）。`get_state` 倒序扫描语义（:787-797）保持。
4. **`blend_states`**：`HashMap<String(type), HashMap<String(name), BlendState>>` 归 `Animation` 拥有（对应 :63 两层字典）；`BlendState.target` 是骨/槽/约束的**目标引用** → 用 `BlendTarget` 枚举（`Bone(BoneIdx)/Slot(SlotIdx)/Constraint(BoneIdx)/AnimationState(StateRef)/Surface…`，按 `getBlendState` 调用点实测收口）。

### DD-4.5 阶段 4/5 事件边界（最小占位面——数值必须真 / 事件可占位）

**阶段 4 必须真现**（缺了 golden 数值就不过）：
1. `WorldClock` 全语义（F4.8/F4.24）；
2. `Armature.clock` 字段 + getter/setter + **子骨架传播**（F4.25）+ `on_clear` 的 `_clock.remove`（Armature.ts:123-124，阶段 3 留的 TODO 正好关闭）；
3. `DragonBones` **实例部分**（不再是常量 struct）：`_clock` + `advance_time` 三段骨架——① objects flush **空队列 no-op**、② `_clock.advance_time(dt)` **真现**、③ events flush **循环体只 clear 不 dispatch**（保持同帧清空节奏，阶段 5 只填循环体）；
4. `buffer_event`（真 push + 去重，无派发也无害）/ `buffer_object`（no-op 占位）两个方法**签名存在**——调用点：AnimationState.ts:681/:717、TimelineState.ts:59/:90、Armature.ts:268；
5. **Play action 链**：Action 轴 → `armature.buffer_action(ev, true)`（阶段 3 已有）→ `Armature::advance_time` ⑦ 消费 → `animation.fade_in`（本阶段真身）——**不经事件、无闸门**（F4.9）；
6. `BaseFactory._dragonBones` 字段 + `clock()` getter（jsdump.mjs:94/101 形态）；`BaseFactory::advance_time(dt)` 改为委托 `dragon_bones.advance_time(dt)`。

**阶段 5 才做**：`dispatchDBEvent`、SOUND 双发、监听器 harness、池回收、`EventObject.animationState`（F4.27）。
**占位即数值安全的证明链**：事件派发只调回调、核心无事件→数值回路；`hasDBEventListener` 恒 false 时连 EventObject 都不构造（闸门结构照抄 TimelineState.ts:53/85/103/111、AnimationState.ts:676/:712）。

> ⚠️ **推进成员资格**（F4.8 + 阶段 3 已有行为的衔接）：`BaseFactory::advance_time(dt)` 改三段委托后，**只推进挂了 clock 的骨架**——`build_armature` 官方不自动挂钟（jsdump 是手动 `armature.clock = factory.clock`）。差分 harness 必须自己挂钟（任务 8）；阶段 3 的 `pose_golden`/`armature_unit` 只用 build 收尾 advance 与 `advance_one`，**不受影响**（`advance_time(0.0)` 仍早退 no-op）。`dispose_armature` 必须同时 `clock.remove`（成员资格泄漏）。

### DD-4.6 数据访问 = `Arc<DragonBonesData>` 直查 + 名字

- `Animation._animations` 不存引用：查表 = `data.armatures[armature_name].animations.get(name)`（失败 `log::warn!` + None，对齐 :339-348）；
- 时间轴 init 读 IR：`data.root.as_ref()` → `ir.frame_float_array/frame_int_array/frame_array/timeline_array/frame_indices/color_array`（阶段 2 已锁）；
- `AnimationData.cached_frames` 运行时写（:868/:872）= **P1-4 才开**（F4.15：rate=0 整段不进）→ 结构照抄、体 `// TODO P1-4`，届时再议 RefCell 或登记偏差。

### DD-4.7 事件结构照抄、体走占位

`_advanceFadeTime`/`ActionTimelineState` 里 `hasDBEventListener` 闸 + `buffer_event` 调用**结构逐行保留**（行号注释齐全），阶段 5 往 `DragonBones::buffer_event`→flush 填真身时零结构改动。

### DD-4.8 `Animation::on_clear`（关闭阶段 3 的 R4 TODO）

TS `_onClear`（Animation.ts:68-100）= states 逐个回收 → `_animations` 表逐 key delete（:73-75，**`:96` 的 clear 被注释**）→ `_blendStates` 逐项回收 delete → config `clear()` 保留实例（Rust 无池）→ 标量复位（timeScale=1.0、inherit=1.0、dirty=false、数组清空）。`Armature::on_clear` 把 `reset()+init` 换成 `on_clear()`。

---

## 2. 组件与字段清单（对照 TS 抄；细表在笔记）

| 组件 | 字段/方法全表在哪 | 落地时注意 |
|---|---|---|
| `WorldClock` | 笔记 `clock_driver.md §2`（字段 :49-68 已列） | `advance_time` 伪代码在笔记；F4.24 迭代语义 |
| `Animation` | `animation.md §1（18 方法+8 getter API 表 + Rust 草案）/§2 字段表` | `animationConfig` getter **每次读都 clear()**（:939-942）；`isCompleted` 空列表 **false**（:861）；`_onClear` 见 DD-4.8 |
| `AnimationState` | `animation_state.md §1 字段全表（默认=_onClear）/§6` | 8 强类型数组（DD-4.3）；`_pose_timelines`（:1022-1060）别漏 |
| `BaseTimelineState` 5 基类 | `timeline_states.md §1 职责表/§2 伪代码` | `playState` 三态 {-1 start, 0 play, 1 complete}（BaseTimelineState.ts:30-32）；`dirty` 生命周期（§2.4） |
| `BlendState` | `animation_state.md §5`（AnimationState.ts:1528-1601） | `update` 三分支 + `dirty` 语义；`_reset` 每帧（Animation.ts:188-193） |
| `AnimationConfig` | 已在 `model/animation_config.rs`（阶段 2）+ `animation.md §0/§3.2 消费点` | 核对 `clear()` 默认值与 **AnimationConfig.ts:279-296** 逐项一致 |
| 16 个时间轴类 | `timeline_states.md §4 类清单表（每帧布局/写点/绑定行号/依赖就绪度）` | §3 三基准链 + §5 Action/ZOrder 完整伪代码 |

---

## 任务 1 — `WorldClock` + 驱动链接线（requirements 顺序第 1 步）

1. `src/animation/world_clock.rs`：字段（time/timeScale/_systemTime/_animatebles/_clock）+ `advance_time`（**逐行照笔记 `clock_driver.md §2` 伪代码**：NaN→0、负 dt 自动帧间隔、`_systemTime` 先于早退刷新、timeScale 先于 0 判定、time 绝对值前进而成员收到带符号 dt、长度定格 + 前移压缩 + 尾段整理）+ `add/remove/clear/clock setter`（幂等闸 :241-254；`clear()` 不 vec.clear()——F4.24）。
   `IAnimatable`（76 行）在 Rust 是 trait：`fn advance_time(&mut self, passed_time: f64)`——`Armature` 实现之（现有方法直接挂 trait 即可）。
2. `Armature`：`clock: Option<ClockHandle>`（阶段 5 定型，先存 `Option<u32>` 句柄或直接 `Option<()>` 占位 + TODO——**要能表达"当前挂的钟"以便 setter 幂等与 on_clear 摘除**；建议阶段 4 直接让 `BaseFactory` 拥有 `WorldClock` 实例、`Armature.clock` 存 `Option<ClockIdx>` 简单新类型，阶段 5 扩成多钟）；getter/setter 按 Armature.ts:886-911 逐行（**挂钟时子骨架递归挂同钟 :904-910**，摘钟同理）；`on_clear` 的 `clock.remove`（:123-124）落地。
3. `core/dragon_bones.rs` 实例部分：`DragonBones { clock, events: Vec<EventObject>, objects: …(no-op 队列) }` + `advance_time` 三段骨架（占位面按 DD-4.5）+ `buffer_event/buffer_object`。**静态常量 struct 不动**（`VERSION/Y_DOWN/…`）。
4. `BaseFactory`：`_dragon_bones: DragonBones` 字段（jsdump:94 `new DragonBones(em)` 等价——事件 dispatcher 先喂 `NullArmatureProxy` 或空实现，阶段 5 接 eventManager）、`fn clock(&mut self) -> &mut WorldClock`（jsdump:101 用）、`advance_time(dt)` 委托三段；`dispose_armature` 摘钟。

**验收自测**：`advance_time(0.0)` 仍 no-op（阶段 3 测试绿）；`advance_one` 不变；`advance_time(dt>0)` 只推进挂钟骨架——挂钟前后各跑一次断言。

## 任务 2 — 数据面核对 + Animation 骨架 + 占位收口

1. `AnimationConfig` 核对：`clear()`/`copy_from()` 与 **AnimationConfig.ts:279-296** 逐字段 diff（含 `fadeOutMode` 默认 All、`timeScale` 默认 -100、`playTimes=-1`、`autoFadeOutTime=-1`——注意笔记提到 :304 与 config:296 的默认差异，以 `_onClear` 为准）；缺啥补啥，`tests/model_defaults.rs` 加断言。
2. `Animation` **骨架**（先字段后逻辑）：按 `animation.md §2` 字段表 + `_onClear` 逐字段；`init(armature_idx)`（:161-168 幂等）、`animations` 挂表 setter（:908-926：同引用早退 + 按插入序重建 `animation_names`）——**Armature::init 阶段 3 已调 `animation.init`，本任务补挂表**（对应 Armature.ts:290，读 `armature_data().animations`）；`on_clear`（DD-4.8）替换 `armature.rs` R4 的 `reset()+init`；`play/fadeIn/goto*/stop/reset/get_state/has_animation/get_states/is_playing/is_completed/last_*` **先留签名 + `todo!()`**（任务 5 填体），签名按 `animation.md §1.3 草案`（★`play` 第 2 参 `play_times: i32`、`stop(Option<&str>)`、`fadeIn` 6 参——阶段 3 占位签名是错位的，本任务一并改）。
3. **调用点同步改造**（签名一改就断的，全在本任务收掉）：
   - `armature/slot.rs` 子骨架分支 `child.animation.play(None, -1.0)` → `play(None, -1)`（playTimes 语义）；
   - `armature/armature.rs` action 队列三分支 `fade_in(name, -1.0)` → 官方调用形态（Animation.ts:360/368/374 实测参数——**读原文**：`childArmature.animation.fadeIn(actionData.name)` 单参默认 vs `this._animation.fadeIn(actionData.name)`，映射成 `fade_in(name, -1.0, -1, 0, "", SameLayerAndGroup)` 或提供默认包装）；
   - `factory/base_factory.rs` `_getSlotDisplay` 的 `play(None, -1.0)` 同步；
   - 其余 `animation/mod.rs` 模块头的"占位差异表"注释按实现情况收缩。
4. `EventObject`：**不建 `animationState` 字段**（F4.27），在 `event_object.rs` 注释登记。

## 任务 3 — `BaseTimelineState` 5 个抽象基类（笔记 `timeline_states.md §1/§2`）

- 5 个基类按官方（TimelineState / Tween / Single / Double / Multiple——**以 `BaseTimelineState.ts` 实测类名为准**，笔记 §1 有职责表）；
- `_set_current_time` **三分支逐行**（笔记 §2.2 伪代码 + F4.14 要点）：返回 `bool`"时间没变→整帧跳过"；
- `update` 帧定位（§2.1）：O(1) 查表（F4.2）、`frameIndices` 尾项承接（:121 `duration+0.000001` 精度补偿 → floor 越界一格）、`_on_arrive_at_frame/_on_update_frame` 钩子与 `dirty` 生命周期（§2.4）；
- 缓动两函数（§2.3）：`get_easing_value`（QuadIn/QuadOut/QuadInOut 半余弦 + `(value-progress)*easing+progress`）与 `get_easing_curve_value`（查表线性、`count>0` 判 omited、**负分支照抄不修**——F4.23）；`easing = frameArray[frameOffset+2] * 0.01`（:316）；
- `_frame_position/_frame_duration_r`（§3.2 尾）：末帧 `1.0/(anim.duration - position)`、**有限循环最后一帧强制不 tween**（:300-307/341-342）。

**单测**：手造小 `AnimationData`+IR（或取 Dragon 某轴）断言帧定位/归一化三分支/负时间/整数倍不对称（F4.14 的 `currentTime=d` 案例）/easing 三公式数值。

## 任务 4 — `AnimationState` + fade 状态机 + `BlendState`（顺序：先于 Animation 逻辑）

1. **字段全表**（笔记 `animation_state.md §1`，默认值 = `_onClear` :251-311 逐项；timeline 回池段 :251-289 结构保留）+ `init`（:725-803，含 `pauseFadeIn→playhead=2/否则 3`、layer/group 写点 :738/:744、负时间修正 :796-798 照抄——疑似 bug 也抄）。
2. **fade 子状态**（DD-4.2 两枚举 + 位类型）+ **`_advanceFadeTime` 逐行**——照抄下方伪代码（=笔记 §2.4，行号实测）：

```text
_advanceFadeTime(passedTime):                                  # AnimationState.ts:667
  isFadeOut := fade_state > 0                                  # :668 方向入口一次判定
  if sub_fade == FadeStart:                                    # :670  -1 → 0
      sub_fade = Fading                                        # :671
      eventActive = parent.is_none() && action_enabled         # :673  ★子动画状态不发 fade 事件
      if eventActive:
          ty = isFadeOut ? FADE_OUT : FADE_IN
          if armature_proxy_has_listener(ty):                  # :676  ★闸门（结构保留，阶段5接派发）
              push_buffered(EventObject{ ty, armature, /*animationState:阶段5*/ })  # :677-681
  if passedTime < 0: passedTime = -passedTime                  # :686-688  ★fade 时间永不倒退
  fade_time += passedTime                                      # :690
  if fade_time >= fade_total_time: sub_fade = FadeComplete;
      fade_progress = if isFadeOut {0.0} else {1.0}            # :692-694
  elif fade_time > 0: fade_progress = 线性（淡入 t/T，淡出 1-t/T） # :696-697 ★纯线性无 TweenType
  else: fade_progress = if isFadeOut {1.0} else {0.0}          # :699-700
  if sub_fade == FadeComplete:                                 # :703
      if !isFadeOut: playhead |= 1; fade_state = FadeComplete  # :705-706 ★只有淡入完成才复位+解锁
      eventActive 同上 → FADE_IN/FADE_OUT_COMPLETE 闸门入队    # :709-717
  # 淡出完成：fade_state 保持 FadeOut、sub 保持 FadeComplete（本函数不复位）
```

3. `advance_time(passed_time, cache_frame_rate)` **主干逐行**（笔记 §3 伪代码 = :808-1070）：**fade 先于 timeScale**（F4.13）→ `playhead==3` 才推进 → `_timeline_dirty` 结构重建（:822-828，约束段留 stage6/批5 对应处理）→ `is_blend_dirty`/`is_cache_enabled`/`weight_result` 级联（:831-840）→ **action 轴在权重早退之前**（:842-848，`weight==0` 仍发事件）→ 缓存段（F4.15：结构+`TODO P1-4`）→ **8 数组分组遍历 + BlendState 去重/首写者清零 pose**（:877-1015，分组序是契约 DD-4.3）→ 淡入完成善后 + `autoFadeOutTime>=0` 自动淡出（:1018-1067）。
4. `fadeOut(fade_out_time, pause_playhead)`（:1111-1164，笔记 §2.5）：**新淡出更长则整体忽略**（:1120-1124）、`fade_total_time = out/progress` 回写（:1162-1163 ★进度曲线连续的关键）、5 类数组 `timeline.fade_out()` + 子状态 `fadeOut(999999,true)`（:1133-1158）、`display_control=false`（:1161）。
5. **`BlendState`**（:1528-1601，笔记 §5）：字段、`update(animation_state) -> bool` 三分支（leftWeight 预算/layerWeight/blendWeight）、`dirty` 语义、`_reset` 时机；`Animation::get_blend_state(type, name, target)`（两层懒建 :748-760——本任务先给 `Animation` 加这个方法体，字典字段任务 2 已立）。
6. 时间归一化不在本文件——引用任务 3 的 `_set_current_time`（F4.14）；`currentTime` setter（:1440-1474）若被 Progress 轴用到，任务 6 批 5 再补。

## 任务 5 — `Animation` 顶层逻辑（笔记 `animation.md §3`）

1. **`advance_time(passed_time)`（:172-267，笔记 §3.1 伪代码）**：abs（F4.7，只此一处）→ `inherit_time_scale` 父链重算（:177-186，**读父骨架 `animation._inherit_time_scale`——经 `ArenaCtx` 解析父（`Armature.parent` 索引，D-3.2），父必须先推进）** → 缩放 → **每帧开头 blend_states 全 reset**（:188-193）→ 单/多/零状态三分支（:196/:233/:264——单状态传真 cfr、多状态恒 0.0 且回收压数组 + `cache_frame_index=-1`、零状态 `-1`；F4.15）→ `buffer_object` 回收点 = 占位。
2. **`play_config`（:337-481，笔记 §3.2 伪代码）逐行**：查表失败 warn+null → Single 早退（:352-362，**不改 cfg**）→ 归一化（:364-409，全部写回 cfg——注意负 position 的 `duration - (pos % duration)`）→ `_fadeOut(cfg)` → 借 state `init` → `_animation_dirty=true` + `cache_frame_index=-1` → layer 插入序 → **子骨架同名继承（:443-452，has_animation + getState==null 双闸，经 world 淡入子骨架）** → animation_timelines 子状态（:454-472，`fadeIn(k,0,1,layer,"",Single)` + `actionEnabled=false/resetToPose=false/stop()` + `addState`）→ `_last = s`。
3. **`_fadeOut(cfg)`（:102-157，笔记 §3.4）**：5 模式过滤逐行（SameLayer :110 / SameGroup :122 / SameLayerAndGroup :134-137 / All :143-151 / **Single 空 case 不补**——F4.12）。
4. 快捷方法装配（笔记 §3.3）：`play`（clear→fadeOutMode=**All** + resetToPose + fadeInTime=0 + null 三分支：defaultAnimation/暂停恢复/重播 last——:516-528）、`fadeIn`（6 参表 F 见 §3.3）、`gotoAnd*` 族（position 计算 :636/:668）、`gotoAndStop*` = gotoAndPlay(playTimes=1)+`state.stop()`。
5. 查询面照 `§1.1/§1.2` 全实现（`getState` **倒序**、`isCompleted` 空 false、`animationConfig` getter 的 clear 副作用）。

## 任务 6 — 时间轴批次（requirements 五批 + 批 0 前置）

**批 0 前置（F4.22，先做）**：
- `Armature::_sort_z_order(slot_indices, offset)`（Armature.ts:185-207 照抄——阶段 3 在 `armature.rs:217-224` 留了逐行 TODO；越界 `continue`、`z_order_dirty = !is_original`、置 `slots_dirty`）；
- `Constraint::invalid_update`（Constraint.ts:218-224 = `root.invalid_update()` + `bone.invalid_update()`）。

**批 1 — `ActionTimelineState` + `ZOrderTimelineState`**（笔记 §5 完整伪代码）：
- Action 是**唯一重写 `update()`** 的轴（除 SlotColor）：跨帧扫描对每个经过帧调 `_on_cross_frame`（正向 :153/:212、单帧 :237/:246、兜底 flush :251-257）；
- 触发四件套：`Play → buffer_action(ev,true)`（:49 **无闸，数值**）、Frame/Sound → `buffer_event`（:59，Sound 无条件 :53，阶段 5 出体）、START/LOOP_COMPLETE/COMPLETE（:85/:103/:111，`eventActive = parent.is_none() && action_enabled` :75，**loopComplete 推迟跨 0 帧**）；
- `_sort_z_order(null,0)` 在 :80（`displayControl && resetToPose`——zOrder 归位）；
- **帧位置一律 `frameArray[pos] / frame_rate` 除法**（F4.16，6 处）；
- ZOrder：`playState>=0` 守卫（:275）→ `_sort_z_order(frame_array, frame_offset+2)`（:278）→ 末帧 `_sort_z_order(null,0)`（:281）。

**批 2 — 骨骼四轴**（BoneAll/Translate/Rotate/Scale）：按笔记 §3.3 表（值个数 6/2/2/2、基准 ②、scale 落点 F4.17）→ 写 `bone.animation_pose` + `transform_dirty=true`（写点行号见 §4 表）、BoneAll 末帧 `normalize_radian`（:300-301）、pose 兜底 scale=1（:305-306）、Rotate 末帧差量归一化（:409-410）、`blend()` 里挂 `BlendState(BONE_TRANSFORM)`。**先 pose 再 blend 的分工**：`_on_update_frame` 算 `_result*`，`blend` 写 `animationPose`——照官方。

**批 3 — 槽四轴**：SlotDisplay（`set_display_index(v, true)` :647 + `playState>=0` 守卫 + pose 取 `slot_data.display_index` :644）、SlotColor（**在 update 里直接写**，fade 期 `result + (target-result)*progress^4` :773-781、稳态直赋 :798-805、两处 `color_dirty=true`；间接寻址 +65536 F4.6）、SlotZIndex（`+=`/`=` 与 blendWeight :843/:846 + `z_index_dirty=true` :851——消费链阶段 3 已通）、Alpha（**双用** BoneAlpha/SlotAlpha，`_alpha` + `alpha_dirty=true` :629，scale 0.01）；slot 轴的 `displayController` 门（:930-935：null / == state.name / == state.group）。

**批 4 — Surface/Deform**：deform 头 5 元用 `TimelineFrameValueCount`(**index 3**)（:527/:883 + F4.6）、动态 `_value_count/_deform_count/_deform_offset/_same_value_offset`（:529-532/:888-891，Deform 版 same 偏移 +65536 修正 :893-897）、blend 三分支写 `surface.deform_vertices` / `display_frame.deform_vertices`（:555-574/:921-940；JSON 路径退化分支照抄 F4.23②）、`vertices_dirty=true` 守卫 `slot.geometry_data == frame.get_geometry_data()`（:951-952）。**运行时依赖已就绪**（笔记 §7.1 表：`SurfaceState.deform_vertices`/`DisplayFrame.deform_vertices`/`update_deform_vertices` 阶段 3 都在）。

**批 5 — IK + Animation 三轴**：IK（写 `_current_a/_current_b` :971-972、pose 回 `IKConstraintData` :975-977、`constraint.invalid_update()` :980——批 0 前置；**F4.23① READ-ORIGINAL**：非 tween 帧读陈旧值的行为先照抄，差分为准）、AnimationProgress/Weight/Parameter（**`_parent.is_some()` 才写**子状态 `currentTime/weight/parameterX/Y` :1004/:1031/:1058——9 素材无子动画，结构照抄 + 单测合成数据）。

## 任务 7 — 调用点与收口核对

- `Armature::advance_time` ② `self.animation.advance_time(passed_time)` 保持（现已是真身调用点）；`AnimationCtx` 组装按 DD-4.4；
- 阶段 3 全部 `todo!()`/TODO 注释清点：`armature.rs`（`_sort_z_order` 已于批 0 完成、on_clear R4 已换）、`animation/mod.rs` 差异表收缩、`constraint.rs` invalid_update、`event_object.rs` animationState 注记保留；
- grep 清扫：`todo!(` 在 `src/animation|armature|factory` 的残留逐条列出——除明确 `TODO P1-4/P1-5/阶段5` 外必须为零。

## 任务 8 — 静态差分基建（验收工具）：`tests/golden_anim.rs`

**纯 Rust 测试，不需要新 JS 工具**（阶段 0 的 golden 已是 120 帧逐帧 dump）。逐动画文件（`tests/common/mod.rs::golden_dir_stems` 遍历 9 素材 37 动画）：

```text
for each testdata/golden/<asset>/<anim>.json:
    factory.parse(ske, key) + parse_texture(tex, key)      # key = ir_snapshot 同款推导
    idx = factory.build_armature(armature_name, key, "", key, Box::new(NullArmatureProxy::new()))
    armature.clock = factory.clock                          # jsdump.mjs:101 镜像（DD-4.5 ⚠️）
    armature.animation.goto_and_play_by_time(anim, 0.0, -1) # jsdump.mjs:117
    for i in 0..frameCount:                                 # jsdump:120-144：**先推进后采样**
        factory.advance_time(meta.dt)                       # = 1/frameRate，走 DragonBones→clock→armature
        compare frames[i].bones[name] == [13 元]            # jsdump:127-131 顺序
        compare frames[i].slots[name] == [15 元]            # jsdump:136-142 顺序
        # frames[i].events **整体跳过**（阶段 5）
    末尾：slotNames 顺序 == golden meta.slotNames（排序锚点）；frameRate/dt 一致
```

- **容差**：`|got-want| <= 1e-9 * (1.0 + |want|)` 首跑（pose_golden 同款）；失败时 panic 带**分类计数 + 最大绝对/相对误差 + 前 5 最差（素材/帧/骨或槽/字段/want/got）**——确认属 libm 级差异（`Math.cos/sin/atan2` 实现差）后降级 `1e-6*(1.0+|want|)`（requirements 硬指标）**并在指南回执里报告实测最大差**，别静默放宽容差；
- 失败诊断先行：先查表达式序（除法/加减序、F4.16/F4.18、`+=65536`、`_valueScale` 还原），再怀疑 libm；
- **单测（`tests/animation_unit.rs` 或并入）**：
  1. `gotoAndStopByFrame` 往返：`goto_and_stop_by_frame(name, k)` → 连推 N 帧数值**冻结**（playhead≠3）→ 同帧 `goto_and_play_by_frame(name, k)` 与首采样一致（往返一致）；
  2. `resetToPose`：语义 = `resetToPose=true` 时**动画未覆盖的骨/槽由 `_pose_timelines` 兜底回数据姿态**（AnimationState.ts:1022-1060 + TimelineState.ts:79-80 zOrder 归位）——用阶段 3 的 `testdata/pose/<asset>.json` 当"数据姿态"基准：播 A 动画若干帧 → `play()`（resetToPose=true）起播同名动画至淡入完成 → 断言未覆盖集合姿态 == pose 黄金；若 9 素材动画全覆盖骨架导致无差异，**用合成 data 造未覆盖骨**保证分支被测到；
  3. 子骨架继承（F3.9 单测的延伸）：inheritAnimation 子骨架在父 `play` 后 `has_animation` 命中 → 子状态被创建（`get_state` 非 None）；
  4. `timeScale`：`animation.time_scale = 2.0` 后 dt 推进翻倍（对拍 golden 前 60 帧 ≡ 正常速度前 120 帧，容差同上）——requirements 把 timeScale 用例放阶段 5，但**此处先钉数值通路**，阶段 5 用例矩阵直接复用。
- 阶段 0-3 全部回归保持绿（83 例）。

---

## 验收清单（全绿即阶段 4 完成）

```bash
cargo build && cargo test -p dragonbones-core        # 阶段 0-3 回归 + golden_anim + animation_unit
cargo clippy -p dragonbones-core --tests             # 零警告
node tools/gen_pose.mjs                              # 阶段 3 黄金不回归（确定性 9 ok）
```

**验收标准（对应 requirements §4）**：
1. **9 素材 × 全动画 × 120 帧差分**：骨 13 元 + 槽 15 元逐元素通过（容差策略见任务 8；`events` 字段阶段 5 再比）；
2. `slotNames`/`boneNames` 末态与 golden `meta` 全序一致；
3. `gotoAndStopByFrame` 往返一致（冻结 + 同帧复现）；
4. `resetToPose` 后未覆盖姿态与阶段 3 静姿态黄金一致（合成数据兜底覆盖分支）；
5. 无 `todo!()` 残留于本阶段范围（任务 7 清扫清单）；clippy 零警告；
6. 阶段 0-3 全部回归绿。

**常见坑速查**（全部实测，编号对 §0）：
1. 帧定位是 O(1) 查表不是二分；运行期缓动只有两个线性函数（F4.2）。
2. fade 枚举**值域无 2、两字段同值异义**；只有 `playhead==3` 推进时间（F4.3/DD-4.2）。
3. 17 type → 16 类（Alpha 双用）；BoneAll **6 值/帧**；类名 `MutilpleValue…` 保留拼写（F4.4）。
4. 三基准链 + 三处 `+=65536` + `_valueScale` 量化还原——IR 通路三件套（F4.5/6/18）。
5. abs 只在 `Animation.advanceTime` 做一次；反向播放 = state 带符号 timeScale（F4.7）。
6. `WorldClock(0)` 早退、timeScale 先于 0 判定、`time` 绝对值前进而成员收带符号 dt（F4.8）。
7. Play action **无监听器闸、数值相关**；其余事件可占位——阶段 4/5 分水岭（F4.9/DD-4.5）。
8. `start/loopComplete/complete` 在 Action 轴不在 fade（F4.10）；loopComplete 推迟跨 0 帧。
9. `play→All` vs `fadeIn→SameLayerAndGroup`；`Single` 在 `_fadeOut` 是官方空 case（F4.12）。
10. **fade 推进先于 timeScale**；`pauseFadeIn` 默认使 playhead 冻结到淡入完成（F4.13）。
11. 归一化三分支——普通多帧轴**抄 action 不自算**；负数先反再 floor；负向整数倍 `currentTime=d`（F4.14）。
12. `isCacheEnabled` 要求 `fade_state==FadeComplete`；多状态恒 cfr=0；缓存段结构保留 `TODO P1-4`（F4.15）。
13. Action 帧位置**除法**（官方注释 Precision problem）（F4.16）。
14. 骨架 scale 两套落点（blend 内 vs init 的 `_valueScale`），Rotate/Scale 不乘（F4.17）。
15. `BlendState` 每帧 reset、`dirty==1` 首写者清零 pose、分组遍历序是契约（F4.19/DD-4.3）。
16. `AnimationState` 无 `fadeIn()`/`_normalizedTime`/`_timelineStates` 字典（F4.20）。
17. 子骨架入钟只有 clock setter 传播与 `Slot.ts:549` 两个入口（F4.25）；`WorldClock::clear` 不能写成 `vec.clear()`（F4.24）。
18. 批 0 两前置别忘：`_sort_z_order`、`Constraint::invalid_update`（F4.22）。
19. config 双重借用用 `mem::take` 解（DD-4.4）；`play_config` 归一化**写回 cfg**的行为必须保留。
20. 差分 harness **必须挂钟**（`armature.clock = factory.clock`），否则 `advance_time(dt)` 推不动（DD-4.5 ⚠️）；采样在 advance **之后**（jsdump:120-144）。
21. IK 轴 `_currentA/B`、Deform 退化分支 = READ-ORIGINAL，先照抄后差分（F4.23）。
22. `EventObject.animationState` 阶段 4 不建（F4.27）；`buffer_event/buffer_object` 签名必须先存在（DD-4.5）。

**工期提示**：任务 4（AnimationState）与任务 6（16 类）是大头，合计占 60% 以上；任务 1/2/3 各半天到一天可并行推进。建议顺序 = 1→2→3→4→5→6→8（任务 7 随手做），其中**任务 8 的 harness 可以在任务 5 后先搭骨架**（单轴批 2 完成时就能对拍 BoneAll 素材的骨姿态，提前暴露 IR 通路问题）。

---

## 完成后

把 `cargo test` 输出（尤其 `golden_anim` 的逐素材通过情况与**实测最大差**）+ `gen_pose` 输出发我。确认全绿后我 review，然后写**阶段 5（工厂、事件与 P0 收口）**指南——`DragonBones.advanceTime` 三段真身、事件派发链、`EventObject` 池、`BaseFactory` 剩余 replace_* 方法，以及 requirements §5 的 9 格用例矩阵（fadeOut 交叉 / layer 叠加 / goto 往返 / 事件时序 / timeScale / 双骨架…）差分基建。
