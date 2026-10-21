# 阶段 6 实现指南：P1 完整度（dbbin / Path / BoundingBox / cache / replace）

> 配套 [`docs/requirements.md`](../requirements.md) §3 阶段 6。官方 TS 是行为规格，Rust 结构可不同但可观察行为必须对齐。
> 本阶段补齐 P1 全部 10 项，并关闭 D-4.35（`Slot.z_index` 精度）。阶段 5 的 212 个测试与 37+131 份 golden 必须保持全绿。
> 行号以 `DragonBonesJS/DragonBones/src/dragonBones/` 当前 5.7 源码为准；素材引用的 demo 资源位于 `DragonBonesJS/Pixi/Demos/resource/`。
> 预估 18–24 个工作日：dbbin 3–4、Path 4–5、cache 2–3、BoundingBox 2、replace 2–3、夹具/工具 3、收口 2。

## 实现检查点（2026-10-21，进行中）

已完成并入 master 的部分：

| 任务 | 状态 | 证据 |
|---|---|---|
| 任务 1 夹具/工具/zIndex | ✅ | `testdata/dbbin`、`testdata/synthetic`、`Slot.z_index: f64`（D-4.35 关闭） |
| 任务 2 dbbin | ✅ | `parser/binary_data_parser.rs`；`tests/dbbin.rs`：2 素材 IR 与 JSON 全等、9 动画×120 帧 pose 全等 |
| 任务 3 IR 打包 | ✅ | `IrArrays::to_binary`；`tests/ir_binary.rs` 8 素材与 JS `data.binary` 逐字节相等 |
| 任务 4 BoundingBox | ✅ | 三种算法 + Slot/Armature 命中；`tests/bounding_box.rs` 对 JS golden（含官方 `B.x/y` bug 锁定） |
| 任务 5 PathConstraint | ⚠️ 部分 | 合成夹具 fixed/length/tangent 121 帧对 JS 全等；仅覆盖 path slot 父骨在 root 之前、链骨父为 root 的拓扑（D-6.5） |
| 任务 6 cache | ⏳ 未完成 | 曾接入运行时 rate 但行为未对齐 JS，已整体回退；不得把“只设 rate”当完成 |
| 任务 7 replace | ⏳ 未开始 | P1-5 仍缺 |
| 任务 8 P1 审计 | ⚠️ 部分 | P1-6（ActionData/defaultActions）、P1-7 canvas、P1-8 14 BlendMode 测试已加；P1-9 审计未写 |

> 注意：`preserve_order` 已加入 serde_json feature（dbbin header 的 bone/slot 时间轴顺序依赖 JSON 对象插入序）；阶段 2–5 全量测试与 golden 仍全绿。

---

## 目录

- [0. 调研结论与阶段边界](#0-调研结论与阶段边界)
- [1. 设计决策](#1-设计决策)
- [2. 任务依赖](#2-任务依赖)
- [任务 1 — 夹具、工具与 zIndex](#任务-1--夹具工具与-zindex)
- [任务 2 — dbbin 读取](#任务-2--dbbin-读取)
- [任务 3 — IR 二进制写出](#任务-3--ir-二进制写出)
- [任务 4 — BoundingBox 与命中检测](#任务-4--boundingbox-与命中检测)
- [任务 5 — PathConstraint](#任务-5--pathconstraint)
- [任务 6 — cacheFrameRate 帧缓存](#任务-6--cacheframerate-帧缓存)
- [任务 7 — replace_* 与工厂 API](#任务-7--replace_-与工厂-api)
- [任务 8 — P1 剩余项审计](#任务-8--p1-剩余项审计)
- [任务 9 — Review 与验收](#任务-9--review-与验收)
- [常见错误速查](#常见错误速查)
- [完成后](#完成后)

---

## 0. 调研结论与阶段边界

### 0.1 P1 现状盘点（2026-10-21 实测）

| 项 | 数据/解析层 | 运行时 | 结论 |
|---|---|---|---|
| P1-1 dbbin | 无（只有 JSON `ObjectDataParser`） | 无 | **最大缺口**；本地只有 2 个 dbbin 样本 |
| P1-2 IK/Path | `ConstraintKind::Path` 字段、`parse_path_constraint`、`DisplayKind::Path` 均已实现；IK 真身阶段 3 已完成（D-4.19） | factory 对 Path warn 跳过；`Constraint` 仅 IK | 只剩 Path 运行时 |
| P1-3 BoundingBox | `BoundingBoxData` 三变体、parser 已实现 | `Slot::contains_point/intersects_segment` 恒 false/None；`Armature` 仅遍历骨架 | 算法 + 签名改造 |
| P1-4 cache | `cache_frame_rate`、`cached_frames` 字段、AnimationState 的帧号去重已有 | **无写侧**：`cachedFrameIndices` 恒 None，Bone/Slot 命中 `unreachable!` | 设计 + 逐层接线 |
| P1-5 replace | `Slot::replace_*` 原语已实现 | `BaseFactory::replace_*` 全缺 | 工厂 API + animation override |
| P1-6 UserData/ActionData/defaultActions | parser 全实现；defaultActions 已在 child build 使用；action 队列阶段 4 已接 | 基本完成 | 补解析/行为测试即可 |
| P1-7 CanvasData | parser（`ObjectDataParser.ts:306` / Rust `parse_armature`）与 model 已实现 | core 无渲染，只存数据 | 补测试 |
| P1-8 BlendMode 14 种 | parser 的 string/number 两路已实现 | core 只透传，适配层渲染 | 补 14 值测试 |
| P1-9 脏标记 | `_lockUpdate`/`*_dirty` 阶段 3/4 已逐行翻译 | — | 审计 + 针对性回归 |
| P1-10 IR 二进制写出 | 只有 `DragonBonesData.binary` 字段 | 无 | **要求文档有误名**，见 F6.8 |

另：`Slot.z_index` 目前为 `i32`（D-4.35），官方是 `number`，SlotZIndex blend 会产生小数；本阶段一并改 `f64`。

### 0.2 素材与夹具现状

- `assets/` 的 9 个素材全部为 5.5 JSON，**无** `path` 约束、`boundingBox` 显示、`canvas`、`cacheFrameRate`。
- `DragonBonesJS/Pixi/Demos/resource/`：
  - `.dbbin` 仅 2 个：`mecha_1406`、`mecha_1002_101d_show`（含同名 JSON，可直接做 JSON↔binary 差分）。
  - 两者显示类型只有 `image` + `armature`，**无 mesh**；因此 dbbin 的 geometry/weight 分支没有真实素材，需 source-review + 合成数组测试，登记 D-6.10。
  - `shizuku` 有 `canvas`；`mecha_1406/1502b`、`you_xin` 有 `ik`；`bounding_box_tester` 的包围盒是**代码里构造**的，JSON 中无 `boundingBox`。
  - **没有任何 Path 约束 / Path 显示样本**；`"cacheFrameRate"` 在全部 demo JSON 中也不存在（官方缓存由 `armature.cacheFrameRate = n` 运行时开启）。
- 结论：Path、BoundingBox、replace、cache 必须有**合成夹具**；Path 夹具需先用 JS 参考实现验证可解析、无 NaN，再生成 golden。

### 0.3 官方源码已核实的阶段 6 事实

| 编号 | 官方事实 | 实现约束 |
|---|---|---|
| F6.1 | dbbin 格式：tag `"DBDT"`（4 字节）+ `u32` headerLength + UTF-8 header JSON + 二进制负载；负载起点 `12 + headerLength`（BinaryDataParser.ts:394-417） | 头部与偏移均小端；tag 不符返回 `None` |
| F6.2 | 根 `offset` 为 12 或 14 个数字：`[intOff,intLen, floatOff,floatLen, frameIntOff,frameIntLen, frameFloatOff,frameFloatLen, frameOff,frameLen, timelineOff,timelineLen, colorOff?,colorLen?]`；长度单位为**字节**；无 color 段时 `colorArray = intArray`（:367-392） | 偏移相对负载起点；解码为 i16/f32/i16/f32/i16/u16/i16 |
| F6.3 | 二进制 `animation.duration` 是**帧数**；`playTimes` 默认 1；`offset` 是 `[frameIntOffset, frameFloatOffset, frameOffset]`（:186-205） | 不要复用 JSON 的数组长度推导 |
| F6.4 | `_parseBinaryTimeline`：`keyFrameCount == 1` → `frameIndicesOffset = -1`；否则为 `animation.frameCount + 1` 个动画帧生成 `frameIndices`，逐帧指向 `iK-1`，最后一帧的 count 用 `animation.frameCount - frameStart`（:141-184） | `frameIndices` 是全局追加数组 |
| F6.5 | 二进制 geometry：`geometry.offset = rawData.offset`；`weightOffset = intArray[offset + GeometryWeightOffset]`，`< -1` 时 `+= 65536`，`>=0` 才建 `WeightData`（:331-365） | `raw_books` 下标 → 骨名，与 JSON parser 一致 |
| F6.6 | `BinaryDataParser extends ObjectDataParser`：仅覆写 `_parseAnimation/_parseGeometry/_parseArray/parseDragonBonesData`；骨架/bone/slot/skin/display 的静态解析完全复用（:27-438） | Rust 也应复用 `ObjectDataParser`，不要复制 2960 行 |
| F6.6a | dbbin header 的 `bone`/`slot`/`constraint` 是**对象映射**，其键序决定 `frameIndices` 追加顺序；serde_json 默认 BTreeMap 会按字母序重排，导致 IR 与 JS 不等 | Cargo 的 `serde_json` 必须开 `preserve_order`（本项目已开） |
| F6.7 | header UTF-8 用自定义 `_decodeUTF8`（容错/替换非法序列），非标准 `TextDecoder`（:38-139） | 官方导出数据均为合法 UTF-8；是否复刻非法字节行为见 D-6.6 |
| F6.8 | requirements 的 P1-10 函数名 `_parserDragonBonesData` **不存在**；实际是 `ObjectDataParser._modifyArray`（:2215-2323）：把 7 组 IR 数组打包成 `data.binary`，padding 条件为四组 `length % 2` + `colorArray` 按 `timelineArray.length % 2`；**它不写 dbbin 头部** | P1-10 = IR 数组打包并存入 `data.binary`；不要误做完整 dbbin 导出 |
| F6.9 | `_parseDeformFrame` 的 weighted 分支在官方就是 `// TODO`（:1975-1990）；Rust 现有 TODO 是忠实照抄 | 不实现加权 deform；登记“官方亦未实现” |
| F6.10 | Rectangle：Cohen–Sutherland；`containsPoint` 用半宽/半高闭区间；Ellipse：缩放 y 后圆判定；Polygon：顶点 + AABB | 逐式翻译，不做“更优算法” |
| F6.11 | Rectangle `inSideB` 分支写 `intersectionPointB.y = xB`（BoundingBoxData.ts:257）——官方笔误，`y` 被赋成 `xB` | **照抄可观察行为**并写测试锁定；这是官方 bug，不是 Rust 偏差 |
| F6.12 | Slot 命中：`updateTransformAndMatrix()` → 全局矩阵求逆 → 点/端点逆变换 → BoundingBox 判定 → 交点/法线正变换回骨架坐标；`intersectsSegment` 返回 0/1/2/3/-1（Slot.ts:941-1037） | `Slot`/`Armature` 需 `&mut self`（可能更新矩阵） |
| F6.13 | PathConstraint：`dirty`、`pathOffset`、`position/spacing/rotateOffset/rotateMix/translateMix`；`init` 解析 root/target/pathSlot/bones；`update` 先更新路径顶点（`childrenTransformDirty` / `verticesDirty` / 权重骨脏），再按 `PositionMode/SpacingMode/RotateMode` 采样并改写链骨矩阵；`invalidUpdate` 为空（Constraint.ts:230-834） | 新增 `PathConstraint` 运行时结构；空 `invalidUpdate` 照抄 |
| F6.14 | 非匀速用 `curveLengths` 分段线性；匀速用 10 段采样积分（`segments[10]`）；`addCurvePosition` 用三次 Bézier 导数切线；`positions` 每 space 3 值（x,y,angle）（:357-634） | 数学表达式顺序不可“优化” |
| F6.15 | Path 的路径顶点来自 path slot 的 `GeometryData`；无 weight 时只受 pathSlot 父骨矩阵，有 weight 时按 geometryBones 权重混合（:278-348） | Rust 直接读 IR `intArray/floatArray` + `Slot.geometry_data` |
| F6.16 | cache 由 `ArmatureData.cacheFrames(rate)` 初始化：有效 rate = `max(ceil(rate * animation.scale), 1)`；`cacheFrameCount = ceil(rate*duration)+1`；为每个 animation 生成 `cachedFrames: boolean[]` 与 bone/slot 的 `indices[] = -1`（ArmatureData.ts:269-280；AnimationData.ts:235-260） | 初始化一次；`cacheFrameRate > 0` 时再次设置直接返回（官方 TODO clear） |
| F6.17 | `DragonBonesData.cachedFrames` 是扁平 `number[]`，`setCacheFrame` 追加 10 个 f64（a,b,c,d,tx,ty,rotation,skew,scaleX,scaleY）；`getCacheFrame` 反向写回（ArmatureData.ts:282-317） | 每帧缓存偏移 10 个 f64 |
| F6.18 | `Animation.advanceTime` 在 `animationDirty && cacheFrameRate>0` 时把 `AnimationData` 的 bone/slot `cachedFrameIndices` 赋给运行时 Bone/Slot；随后 `AnimationState.advanceTime(passedTime, cacheFrameRate)`（Animation.ts:205-230） | 赋值发生在 state advance 之前 |
| F6.19 | `AnimationState` 缓存：fade 完成且 `cacheFrameRate>0` 才启用；先把 `action.currentTime` 量化 `floor(t * rate*2)/(rate*2)`；算 `cacheFrameIndex=floor(t*rate)`；同 index → 不更新任何 timeline；未缓存 → 标记 `cachedFrames[index]=true`，只跳过 bone timeline（`isUpdateBoneTimeline=false`）（AnimationState.ts:832-878） | 时间量化在 zOrder 更新之前；cache index 写回 Armature |
| F6.20 | Bone/Slot 缓存分支：同槽位命中 → 不重算；已缓存槽位 → 只 `getCacheFrame`；未命中且脏 → 计算并 `setCacheFrame`；非脏但已有 cache → 回填当前 cache 索引（Bone.ts:366-438；Slot.ts:740-783） | `get_cache_frame`/`set_cache_frame` 需要每 Armature 的扁平数组 |
| F6.21 | `Armature.cacheFrameRate` setter 设置 `ArmatureData.cacheFrames(value)` 并递归给现有 child armature（Armature.ts:765-780）；Slot 切换 child 时若 child 未设 rate 则继承父 rate（Slot.ts:551-555） | Rust data 不可变 → 运行时 rate 表 + Armature 字段（D-6.1/D-6.2） |
| F6.22 | replace API：`replaceDisplay`（:720-748）、`replaceSlotDisplay`（:783-796）、`replaceSlotDisplayList`（:800-821）、`replaceSkin`（:860-900）、`replaceAnimation`（:939-984）；`replaceAnimation` 直接替换 `animation.animations` 映射并递归 child armature | Rust data 不可变 → 每 Animation 的 animation override 映射（D-6.7） |
| F6.23 | `SlotData.zIndex` / `_zIndex` 是 `number`，排序键 `zIndex * 1000 + zOrder` 为数值比较（Armature.ts:46-48） | Rust `Slot.z_index` 改 `f64`，关闭 D-4.35 |

### 0.4 阶段边界

**包含**：P1-1～P1-10、D-4.35、上述运行时/测试/夹具/工具。

**不包含**：
- 加权 deform frame（官方 TODO，F6.9）。
- Path/BoundingBox 的渲染表现（核心只做数值/命中；Bevy 适配层负责显示）。
- dbbin **写出**（官方无 dbbin exporter；P1-10 只做 IR 打包）。
- `replace_*` 的渲染对象替换（core 只换数据/DisplayFrame；渲染实体由阶段 7 处理）。
- `Armature.replacedTexture`：TS 中只被 `Armature` 自己存取、core 内无消费者，是适配层的渲染纹理对象；阶段 7 实现，登记 D-6.11。

### 0.5 本次调研的实测记录（2026-10-21）

- **dbbin 头**：用 Node 读取 `mecha_1406` / `mecha_1002_101d_show`，tag=`DBDT`、headerLength=7464/7504、负载起点=7476/7516；根 `offset` 12 项，header 结构与 JSON skeleton 相同，animation 只有 `offset` + timeline 偏移表，没有 `frame` 数组。确认 F6.1–F6.4。
- **素材缺口**：`assets/` 9 个 JSON 均无 `path`/`boundingBox`/`canvas`/`cacheFrameRate`；Pixi demo 中 dbbin 只有 2 个，显示类型只有 image/armature；`shizuku` 有 canvas；`bounding_box_tester` 的包围盒在代码里构造。
- **Path 夹具**（JS 实测）：
  - 缺 `lengths` → `_parseDisplay` 直接抛错（`rawCurveLengths.length`）；
  - `targetDisplay` 不写时默认取 target slot 名，通常找不到 Path display → 约束被静默丢弃；
  - `vertices` 长度不是 6 的倍数时 `curveCount` 非整数，`new Array(curveCount)` 或采样产生 NaN；15 个点可得到有限姿态。
- **BBox 夹具**（JS 实测）：`type:"boundingBox"` 的三种 `subType` 均能 build；`armature.containsPoint/intersectsSegment` 可直接调用；rect 的 `inSideB` 分支可观察到 `intersectionPointB.y` 被写成 `xB` 的官方 bug。
- **cache**：所有 demo JSON 无 `cacheFrameRate`，必须运行时设置；官方缓存由 `ArmatureData.cacheFrames` 初始化，不能只改一个 rate 字段。

---

## 1. 设计决策

### 1.1 dbbin：扩展现有 `ObjectDataParser`，而不是复制解析器

新增 `parser/binary_data_parser.rs`（或 `object_data_parser.rs` 内 `binary` 子模块）只负责：

```rust
pub(crate) struct BinaryArrays {
    pub int: Vec<i32>,          // i16 解码
    pub float: Vec<f64>,        // f32 解码
    pub frame_int: Vec<i32>,
    pub frame_float: Vec<f64>,
    pub frame: Vec<i32>,
    pub timeline: Vec<i32>,     // u16 解码
    pub color: Vec<i32>,
    pub frame_indices: Vec<i32>,
    pub payload: Vec<u8>,       // 整份输入字节（data.binary 语义见 D-6.7）
}
```

`ObjectDataParser` 增加 `binary: Option<BinaryContext>`：

- `parse_array()`：`binary` 存在时从 `BinaryArrays` 填 `self.int_array/...`，**不 clear 后重建**；
- `parse_animation()`：`binary` 存在时走 `parse_binary_animation`（F6.3/F6.4）；
- `parse_geometry()`：`binary` 存在时走 `parse_binary_geometry`（F6.5）；
- 收尾：binary 模式**跳过** `modify_array()` 与 `build_ir()`，用 `BinaryArrays` 直接构造 `IrArrays`；`data.binary = Some(payload)`；`data.finalize_root(ir)`；
- JSON 模式路径完全不变。

公开入口：

```rust
impl ObjectDataParser {
    pub fn parse_binary_dragon_bones_data(&mut self, bytes: &[u8], scale: f64) -> Option<DragonBonesData>;
}
impl BaseFactory {
    pub fn parse_dragon_bones_data_binary(&mut self, bytes: &[u8], name: &str) -> Option<Arc<DragonBonesData>>;
}
```

不把 `BaseFactory::parse_dragon_bones_data` 改成泛型 `&[u8]`；JSON 入口保持现状，新增 binary 入口并在文档写明对应 TS `instanceof ArrayBuffer` 分派。

### 1.2 IR 二进制写出

P1-10 只实现 `_modifyArray` 的 Rust 等价：

```rust
impl DragonBonesData {
    /// 打包 7 组 IR 数组（不是 dbbin 头部）。
    pub fn pack_binary(&mut self); // 或 parser 内调用
}
```

顺序：int i16 → float f32 → frameInt i16 → frameFloat f32 → frame i16 → timeline u16 → color i16；padding 条件照 F6.8；结果存 `self.binary`。JSON 解析收尾调用；binary 解析不调用（`data.binary` 保留输入负载）。

### 1.3 BoundingBox：数据算法 + `&mut Slot/Armature`

- `BoundingBoxData` 增加：

```rust
pub fn contains_point(&self, x: f64, y: f64) -> bool;
pub fn intersects_segment(
    &self, xa: f64, ya: f64, xb: f64, yb: f64,
    point_a: Option<&mut Point>, point_b: Option<&mut Point>,
    normal_radians: Option<&mut Point>,
) -> i32;
```

- 三种子类公式逐式翻译；官方 rectangle 的 `intersectionPointB.y = xB` 照抄（F6.11），测试锁定。
- `Slot::contains_point` / `Slot::intersects_segment` 改 `&mut self`：内部 `update_transform_and_matrix(ctx.parent)` → 矩阵求逆/正变换。当前 `Slot` 已有 `update_transform_and_matrix(&mut self, parent: &Bone)`。
- `Armature::contains_point` / `intersects_segment` 改 `&mut self`，遍历槽、按父骨查找 `ctx.parent`；交点/法线出参照旧签名（8 个 f64 入参）保留，内部转成 `Option<&mut Point>`。
- `BoundingBoxData` 的 `Rectangle` 变体没有 x/y 字段；AABB 用 `[-w/2, w/2] × [-h/2, h/2]`；Polygon 用 `vertices` 与 `x/y/width/height` AABB。

### 1.4 PathConstraint：`Constraint` 改成分派枚举 + 约束更新上下文

当前 `Constraint` 是 IK 专用单结构体。推荐：

```rust
pub struct Constraint {
    pub idx: ConstraintIdx,
    pub name: String,
    pub root: BoneIdx,
    pub order: i32,
    pub target: BoneIdx,
    pub kind: ConstraintRuntime, // Ik { weight, bend_positive } | Path(PathConstraint)
}
pub struct PathConstraint {
    pub path_slot: SlotIdx,
    pub path_display_name: String,
    pub bones: Vec<BoneIdx>,
    pub dirty: bool,
    pub path_offset: i32,
    pub position: f64, pub spacing: f64,
    pub rotate_offset: f64, pub rotate_mix: f64, pub translate_mix: f64,
    pub position_mode: PositionMode, pub spacing_mode: SpacingMode, pub rotate_mode: RotateMode,
    pub spaces: Vec<f64>, pub positions: Vec<f64>,
    pub bone_lengths: Vec<f64>, pub path_global_vertices: Vec<f64>,
    pub segments: [f64; 10],
}
```

- `Bone::update` 的约束段需要额外的 `ConstraintUpdateCtx`：`slots: &mut [Slot]`、`armature_data: &ArmatureData`、`flip_x/flip_y`。IK 只使用 bones 三切片；Path 需要 path slot 与 IR。
- `Constraint::update(root_self, left, right, slots, armature_data, flip)` 内部分派。
- `PathConstraint::init` 在 `_build_constraints` 中解析 root/target/pathSlot/bones（路径显示必须存在于 path slot 的 skin）；`root._hasConstraint = true`。
- Path 的 `update` 需要 `path_slot.geometry_data`、`geometry_bones`、`display_frame.raw_display_data`（Path 变体）；`Slot` 提供只读访问器，路径顶点更新时写 `path_global_vertices`，不修改几何。
- 链骨矩阵修改后 `bone.global.from_matrix(matrix)`；Surface 骨同 IK 分支。
- `Bone::update` 的 `slots` 参数会扩大调用点；`Armature::advance_time` 已持有 `bones`/`slots` 字段，可同时传入（两个数组不相交）。
- 借用策略：Path 更新 path slot 的父骨矩阵时用 `resolve_parent_host`（IK 已有）；链骨在 `right` 中；path slot 的父骨可能在 `left`/root/`right`，用按名解析的 `BoneIdx` + 切片检查。

### 1.5 cache：per-Armature 缓存（关闭 D-4.35 之外的新偏差）

官方缓存存储在**共享 data**（`DragonBonesData.cachedFrames`、`AnimationData.cachedFrames`、bone/slot index maps）。Rust data 是 `Arc<DragonBonesData>` 不可变（D-3.3）。选择：

```rust
// armature/cache.rs
pub(crate) struct ArmatureCache {
    pub frame_rate: f64,
    pub frames: Vec<f64>,                 // 扁平 10 f64/槽位
    pub animations: HashMap<String, AnimationFrameCache>,
}
pub(crate) struct AnimationFrameCache {
    pub cached_frames: Vec<bool>,
    pub bone_indices: HashMap<String, Vec<i32>>,
    pub slot_indices: HashMap<String, Vec<i32>>,
}
```

- `Armature` 增加 `cache: ArmatureCache` 与 `cache_frame_rate: f64`；`cache_frame_rate()` 返回字段（不再读 data）。
- `BaseFactory::set_armature_cache_frame_rate(idx, rate)`：写运行时 rate 表（供之后 build 的同类骨架继承）、调用 `Armature::set_cache_frame_rate(rate)` 递归 child；`Armature` 初始化 `AnimationFrameCache`（F6.16），并清空/复用已有缓存。
- `Animation::advance_time_with_context` 增加 `cache: &mut ArmatureCache` 参数；`animationDirty && cache_rate>0` 时把 map 中 bone/slot index 克隆到运行时对象的 `cached_frame_indices`（F6.18）。
- `AnimationState::advance_time_with_context` 增加 `animation_cache: &mut AnimationFrameCache`；实现 F6.19（量化、cache index、bool 标记、bone timeline skip）。
- `Bone::update` / `Slot::update` 增加 `frames: &mut ArmatureCache`（或只借 `&mut Vec<f64>`）：实现 F6.20 的 set/get；`SlotUpdateCtx` 增加 `cache`。
- 索引表复制到 Bone/Slot 是**克隆**而非共享引用；数值等价（D-6.3）。

**为什么不用 `Mutex` 放进 data**：保持 D-3.3 不可变数据、避免锁进入热路径与 `Clone/PartialEq` 破坏；缓存只影响“走哪条更新路径”，per-Armature 与共享 data 在相同输入下数值一致（第二实例重算而不复用第一实例的缓存槽位，行为等价、性能略差）。

差异登记（见 §1.8）：D-6.1 per-Armature cache、D-6.2 rate setter 运行时化、D-6.3 index clone。

### 1.6 replace_*：`Animation` animation override + 工厂 take-out

- `Animation` 增加 `animations_override: Option<IndexMap<String, AnimationData>>`；`animation_data(name)`/`animation_names()` 先查 override；`play_config` 不再直接 `armature_data.animations.get`，改走 `self.animation_data(name)`（注意 `self.data`/`armature_data` 的借用）。
- `replace_animation(armature, target_data, is_override)`：override=true 用目标 `defaultSkin` 的 animations 整体替换；false 合并；然后递归 child armature（遍历 `slot.child_armature` / display frames 的 `DisplayHandle::Armature`，用目标 skin 的 displays 找 child data）。
- `replace_display(slot, display_data, index)`：`slot.replace_display_data`；非空时调用工厂的 `_get_slot_display` 语义（image 显示但 raw 是 mesh → 用 `slot.mesh_display`）；空 → `replace_display(None,index)`。
- `replace_slot_display` / `replace_slot_display_list`：从 `get_armature_data`/defaultSkin 取 display(s)；list 先 `set_display_frame_count`。
- `replace_skin(armature, skin, is_override, exclude)`：逐槽 `skin.get_displays`，miss 且非 override 时回退默认皮肤——Rust 的 `SkinData.parent` 只是**骨架名**（不是对象引用），需用 `get_armature_data(skin.parent, data_name)` 取 `ArmatureData.default_skin` 再 `skins.get(name)`；override 且仍 miss → `display_frame_count = 0`；命中的每个 display 走 `replace_raw_display_data` + `replace_display`。
- 工厂方法需要 `&mut Armature`，而 Armature 在 `self.armatures` 中：统一用“`take` 出 arena → 操作 → 放回”的模式；child build 走 `_build_child_armature`（会 push arena），注意不要与 take 同时持有同一槽位。
- 补 `get_dragon_bones_data`/`get_texture_atlas_data`/`update_texture_atlases`，以及 `clear`/`remove*`（D-6.8）。

### 1.7 `Slot.z_index` 改 `f64`

- `SlotData.z_index: f64`、`Slot.z_index: f64`；parser 去掉 `as i32`；`sort_z_order` 的键改 `f64`（稳定排序，`partial_cmp` 处理 NaN 为 Equal 并 warn）。
- `SlotZIndex` timeline 去掉 `as i32` 截断；pose 分支用 `data.z_index`（已是 f64）。
- golden JSON 的整数值转 f64 后逐元素相等；`tests/sort_bones`/`pose_golden` 更新类型断言。

### 1.8 差异登记（实现后写入 `docs/model_mapping.md`）

| 编号 | 内容 | 理由 |
|---|---|---|
| **D-6.1** | 缓存表 per-Armature，而非 `DragonBonesData/AnimationData` 共享 | data 不可变（D-3.3）；数值等价，第二实例重算 |
| **D-6.2** | `ArmatureData.cacheFrameRate` setter 运行时化：Factory rate 表 + Armature 字段；不修改 data | 同上；新 build 从表继承 |
| **D-6.3** | Bone/Slot 的 `cachedFrameIndices` 从缓存表**克隆**，非引用共享 | Rust 所有权；同值 |
| **D-6.4** | `Slot/Armature::contains_point/intersects_segment` 需要 `&mut self` | 官方会 `updateTransformAndMatrix`；签名破坏性变更 |
| **D-6.5** | Path 当前为“Armature 骨循环前预推进 + 仅支持 path slot 父骨早于 root、链骨父为 root”的实现；完整 `ConstraintUpdateCtx` 泛化与任意拓扑待补 | 借用收敛验证；合成夹具覆盖常见拓扑，但不能声称 Path 100% |
| **D-6.6** | dbbin header 用标准 UTF-8 解码（非法字节直接失败或 lossy），不逐字节复刻 `_decodeUTF8` | 官方导出数据合法；非法数据行为登记 |
| **D-6.7** | `DragonBonesData.binary`：JSON 路径存打包 IR，dbbin 路径存整份输入字节 | 对齐官方两条路径的不同来源（`_modifyArray` vs `_parseArray`） |
| **D-6.8** | 数据删除 `remove*/clear` 立即释放，不进入 `pending_recycles` | 模型不池化（D-5.1） |
| **D-6.9** | `replaceAnimation` 的 animation 映射是每 Animation override，不是 data 写回 | data 不可变；语义等价 |
| **D-6.10** | 无 dbbin mesh 素材，二进制 geometry/weight 只做 source-review + 合成数组测试 | 样本限制 |
| **D-6.11** | `Armature.replacedTexture` 不实现（适配层渲染纹理） | TS 中 core 无消费者；阶段 7 由 Bevy 实体承担 |

---

## 2. 任务依赖

```text
任务 1（夹具/工具/zIndex）
   ├─→ 任务 2（dbbin）─→ 任务 3（IR 打包）
   ├─→ 任务 4（BoundingBox）
   ├─→ 任务 5（Path）
   ├─→ 任务 6（cache）
   └─→ 任务 7（replace/工厂）
任务 4/5/6/7 ─→ 任务 8（P1 审计）─→ 任务 9（验收）
```

dbbin 与 Path 可并行；cache 依赖 dbbin 之后稳定的 `data.binary`? 否，独立；但 cache 会改 `Bone/Slot/Animation` 签名，建议在 Path/BoundingBox 之后做，避免冲突。replace 依赖 `Animation`/`Slot` 稳定，放最后。

---

## 任务 1 — 夹具、工具与 zIndex

### 文件

| 文件 | 处理 |
|---|---|
| `testdata/dbbin/mecha_1406/`、`testdata/dbbin/mecha_1002_101d_show/` | 从 Pixi demos 复制 `_ske.json`、`_ske.dbbin`、`_tex.json`（不需要 png） |
| `testdata/synthetic/path/*.json` | Path 约束 + Path 显示夹具（见下） |
| `testdata/synthetic/bbox/*.json` | 三种 BoundingBox 显示 + 旋转/缩放骨 |
| `testdata/synthetic/replace/*.json` | 两 skin / 两 armature / 多 display 的换装夹具 |
| `testdata/synthetic/features/*.json` | canvas / userData / defaultActions / 14 blendMode |
| `tools/dbharness.mjs` | 增加 `createFactoryFromBinary`（Node Buffer→ArrayBuffer）、`snapshotHit` 等 helper |
| `tools/stage6dump.mjs` | 模式：`binary-ir` / `path` / `hit` / `cache` / `replace` |
| `tools/gen_stage6.mjs` | 逐 case spawn，写 `testdata/stage6/**` |
| `src/model/armature_data.rs`、`src/armature/slot.rs`、`src/parser/object_data_parser.rs`、`src/animation/timeline_states/slot.rs`、`src/armature/armature.rs` | zIndex `i32 → f64` |
| `tests/z_index.rs` | 分数 zIndex 的排序 + SlotZIndex 时间轴 |

### 夹具要点

- **dbbin**：复制 `DragonBonesJS/Pixi/Demos/resource/mecha_1406/{mecha_1406_ske.json,mecha_1406_ske.dbbin,mecha_1406_tex.json}` 与 `mecha_1002_101d_show/` 同名三件套；共 9 个动画（8+1）。
- **Path 夹具**：一个 armature、root + b1/b2/b3 骨、一个 `pathSlot`（display `type:"path"`）、armature 级 `"path":[ConstraintData]`。关键点（实测）：
  - Path display 必须同时有 `vertices` 与 `lengths`；`_parseDisplay` 对缺失 `lengths` 会崩。
  - `vertices` 长度必须是 6 的倍数（即 3 个点/曲线）；实测 15 个点可稳定运行（先跑 JS 确认不是 NaN，不要按“15 点=5 曲线”推断曲线数）；`lengths` 提供足够长的递增数组。
  - `targetDisplay` 必须显式等于 path display 的 `name`（默认取 target slot 名，通常不存在）。
  - 先跑 `stage6dump path` 验证所有骨 global 有限；若 NaN，调整 `lengths`/`position` 后再生成 golden。
  - 至少三组模式：`percent/length/tangent`、`fixed/fixed/chain`、`percent/percent/chainScale`。
- **BBox 夹具**：三个槽各一个 `type:"boundingBox"` display（rectangle/ellipse/polygon）；父骨给平移/旋转/缩放以覆盖逆矩阵；`stage6dump hit` 记录：
  - 一组网格点 × 三个槽的 `containsPoint` 结果；
  - 多条线段（含两端在内、单端在内、完全在外、穿过）的 `intersectsSegment` 返回值、A/B 交点、normalRadians；
  - 显式命中 rectangle 的 `inSideB` 分支（验证 F6.11 的 `y=xB` 行为）。
- **replace 夹具**：一个 DragonBonesData 两个 armature：
  - `replace_target`：两个 skin（`""`、`"alt"`），slot 有 2 个 image display；
  - `replace_source`：有 `idle`/`walk` 两个 AnimationData，可被 `replaceAnimation` 合并；
  - 用 `stage6dump replace` 记录每次 replace 后 `displayIndex`、`displayFrameCount`、当前 display name、animation names、末帧 pose。

### zIndex

- `SlotData.z_index: f64`、`Slot.z_index: f64`；`parse_slot` 不截断；`sort_z_order` 用 f64 键；`SlotZIndex` blend 写 f64。
- 测试：`zIndex = 1.5` 的槽排序稳定、`SlotZIndex` timeline 小数插值；更新 `D-4.35` 为已关闭。

### 验收

- 所有夹具在 JS 与 Rust 都能 parse/build；Path/BBox 无 NaN；zIndex f64 后阶段 5 全套 golden 仍全绿。

---

## 任务 2 — dbbin 读取

### 文件

| 文件 | 处理 |
|---|---|
| `src/parser/binary_data_parser.rs` | tag/header/数组解码 + `BinaryArrays` |
| `src/parser/object_data_parser.rs` | `binary` 字段；`parse_array/parse_animation/parse_geometry` 分支；`parse_binary_dragon_bones_data`；binary 收尾跳过 `modify_array/build_ir` |
| `src/parser/mod.rs` | 导出 binary 入口 |
| `src/factory/base_factory.rs` | `parse_dragon_bones_data_binary` |
| `tests/dbbin.rs` | 头/数组/IR/动画差分 |
| `tests/golden_binary.rs` | 9 个 dbbin 动画 × 120 帧 |

### 实作步骤

1. `parse_dragon_bones_data_binary(bytes)`：
   - 检查长度 ≥12、tag `DBDT`；
   - `header_len = u32_le(bytes[8..12])`；`header = JSON.parse(utf8(bytes[12..12+len]))`；
   - `payload_off = 12+len`；根 `offset` 数组校验；
   - 解码 7 组 typed array（F6.2）；`frame_indices` 初始为空；`payload` 保存剩余字节。
2. 设置 parser `binary = Some(BinaryArrays)` 后调用内部 `parse_dragon_bones_data_with_mode(&header, scale)`：
   - `parse_array` 只填数组，不清空；
   - `parse_animation` 分支：F6.3 + F6.4；
   - `parse_geometry` 分支：F6.5；
   - `parse_armature` 其余静态解析复用 JSON 路径；
   - armature 解析完后：`IrArrays { int_array: binary.int, ... frame_indices: self.frame_indices }`；`data.finalize_root(ir)`；`data.binary = Some(payload)`；**不调用** `modify_array/build_ir`。
3. `parse_binary_timeline` 伪码：

```text
parse_binary_timeline(type, offset, existing=None):
    t = existing.unwrap_or_default(); t.type=type; t.offset=offset
    key_count = timeline_array[offset + KEY_FRAME_COUNT]
    if key_count == 1: t.frame_indices_offset = -1
    else:
        total = animation.frame_count + 1
        t.frame_indices_offset = frame_indices.len()
        frame_indices.resize(len+total, 0)
        iK=0; frame_start=0; frame_count=0
        for i in 0..total:
            if frame_start+frame_count <= i && iK < key_count:
                frame_start = frame_array[animation.frame_offset + timeline_array[offset+FRAME_OFFSET+iK]]
                frame_count = if iK == key_count-1 { animation.frame_count - frame_start }
                              else { frame_array[...+iK+1] - frame_start }
                iK += 1
            frame_indices[t.frame_indices_offset + i] = iK - 1
```

4. `parse_binary_animation`：设置 `frame_int_offset/frame_float_offset/frame_offset`（raw.offset）、`duration = frame_count/frame_rate`、`play_times` 默认 1；`action`/`zOrder`/`bone`/`slot`/`constraint`/`timeline` 按 F6.3 分派；`animation.timeline` 的 `AnimationProgress` 且 blendType != None 时带 x/y（`AnimationTimelineData`）。
5. `parse_binary_geometry`：`geometry.offset = raw.offset`；按 F6.5 读 weight；`WeightData.bones` 用 `raw_bones[boneIndex]`；无 weight 时不复制顶点（运行时读 IR）。
6. `BaseFactory` 方法：解析后 `add_dragon_bones_data` 的现有逻辑（内嵌 atlas 等）复用；binary 不再 `parse_texture_atlas_data` 内嵌捕获（dbbin 头无 `textureAtlas` 时循环不跑）。

### 测试

- **头/数组**：两个 dbbin 的 tag/header/数组长度；错误 tag/截断 → None，不 panic。
- **IR 等价**：同一素材的 JSON 与 dbbin 解析后 8 组 `IrArrays` 逐元素相等（f32 已量化；容差 0 或 1e-12 断言）。
- **9 个动画**：`mecha_1406` ×8 + `mecha_1002_101d` ×1，120 帧 pose/events 与 JS `jsdump`（dbbin 输入）全等；容差按 f64 `1e-9*(1+|want|)`，另报告最大差（预期 ≤1e-12，因为 f32 IR 已量化）。
- **无 mesh 限制**：显式断言 fixture 显示类型不含 mesh；在测试注释登记 D-6.10。

### 验收

`tests/dbbin.rs` + `tests/golden_binary.rs` 全绿；header 错误路径有测试。

---

## 任务 3 — IR 二进制写出

### 文件

| 文件 | 处理 |
|---|---|
| `src/parser/object_data_parser.rs` / `src/model/dragon_bones_data.rs` | `pack_binary`：按 F6.8 padding + 打包 7 组数组 |
| `tests/ir_binary.rs` | 9 素材 Rust `data.binary` vs JS `data.binary` 字节/哈希全等 |
| `tools/irdump.mjs`（或 `stage6dump`） | 输出 JS `data.binary` 的 base64/hex |

### 步骤

1. JSON 解析收尾（`build_ir` 后）调用 `data.pack_binary()`；padding 照 `_modifyArray` 的五个条件（四组 `%2` + color 按 timeline length）。
2. 写入：int i16 LE、float f32 LE、frameInt i16、frameFloat f32、frame i16、timeline u16、color i16；长度/顺序严格按 F6.2。
3. `DragonBonesData.binary = Some(bytes)`；binary 解析路径不覆盖（保留整份输入字节，D-6.7）。
4. 测试：对 9 个素材比较 Rust 与 JS 的 `data.binary` 哈希（或逐字节）；再验证“解包→与 `IrArrays` 逐元素相等”（若实现 `unpack_ir` 测试 helper）。
5. 注意：**不要**把 `pack_binary` 称为 dbbin 写出；它没有 tag/header，不能直接给 `BinaryDataParser` 读。

### 验收

9/9 字节相等；`tests/ir_binary.rs` 全绿。

---

## 任务 4 — BoundingBox 与命中检测

### 文件

| 文件 | 处理 |
|---|---|
| `src/model/bounding_box.rs` | `contains_point` / `intersects_segment` + 三种子类算法 |
| `src/armature/slot.rs` | `contains_point` / `intersects_segment` 实装；`&mut self` |
| `src/armature/armature.rs` | 外层遍历 + 输出参数；`&mut self` |
| `tests/bounding_box.rs` | 纯算法单测 + 夹具 golden |
| `tests/golden_bbox.rs` | 点/线段查询对拍 |
| `tests/sort_bones.rs` / `pose_golden.rs` | 签名/类型回归 |

### 算法要点

- Rectangle：`contains` 用 `pX ∈ [-w/2,w/2] && pY ∈ [-h/2,h/2]`；`intersects` 用 Cohen–Sutherland；`inSideA/inSideB` 与 `intersectionCount` 语义照 `BoundingBoxData.ts:125-345`。
- Ellipse：`pY *= widthH/heightH; sqrt(pX²+pY²) <= widthH`；线段算法 F6.10。
- Polygon：射线法/绕数（照 TS），AABB 用 parse 时记录的 x/y/width/height。
- **F6.11**：rectangle `inSideB` 分支 `intersectionPointB.y = xB` 原样实现，测试锁定（不要“修复”）。
- `Slot`：`_bounding_box_data` 为空 → false/0；`update_transform_and_matrix` → 逆矩阵变换点/端点；B 交点/法线正变换；`intersectionCount` 1/2 时 A/B 同点（Slot.ts:996-1035）。
- `Armature`：遍历槽（当前排序序）；A 取最近、B 取最远；未提供出参时首个命中即 break（Rust 恒提供出参，按当前签名）。

### 测试

- 纯算法：每个类型 20+ 点/线段（含边界、内部、外部、竖直/水平、退化线段）。
- 夹具：旋转/缩放骨下的逆变换；rectangle `inSideB` bug 分支；ellipse 与 polygon；输出 A/B/normal 精确到 1e-12。
- 现有 pose/golden 全绿（签名变更只影响未使用路径）。

---

## 任务 5 — PathConstraint

### 文件

| 文件 | 处理 |
|---|---|
| `src/armature/constraint.rs` | `Constraint` 枚举 + `PathConstraint` 字段/init/update/bezier |
| `src/armature/bone.rs` | `Bone::update` 增加 `slots`/`armature_data` 约束上下文；`run_constraints` 分派 |
| `src/armature/armature.rs` | 传递 slots/armature_data；`_sort_z_order` 不相关 |
| `src/factory/base_factory.rs` | `_build_constraints` 创建 Path（不再 warn 跳过） |
| `src/armature/slot.rs` | Path 显示/几何/geometry_bones 只读访问器 |
| `tests/path_constraint.rs` | 合成夹具 + init/update 单测 |
| `tests/golden_path.rs` | 3 组模式的 pose golden |

### 实作顺序

1. `Constraint` 结构改造：保持 `idx/name/root/order/target` 公共字段；`kind: ConstraintRuntime::{Ik{weight,bend_positive}, Path(PathConstraint)}`；现有 IK 测试/调用点改访问器。
2. `ConstraintUpdateCtx`（借用来自 `Armature::advance_time`）：

```rust
pub(crate) struct ConstraintUpdateCtx<'a> {
    pub left: &'a mut [Bone],
    pub right: &'a mut [Bone],
    pub slots: &'a mut [Slot],
    pub armature_data: &'a ArmatureData,
    pub flip_x: bool,
    pub flip_y: bool,
}
```

   `Bone::update` 增加两个参数（`slots`、`armature_data`）；IK 路径只用 left/right；Path 用 slots + IR。`Bone::update` 调用方（Armature/测试）全部更新。
3. `PathConstraint::init`：按 `ConstraintKind::Path` 的 `path_slot/path_display/bones` 解析 `SlotIdx/BoneIdx`；校验 path slot 的当前 display 是 `DisplayKind::Path`；`root = first bone`。
4. `update`：
   - path slot 无 geometry/offset 不匹配 → return；
   - `root.children_transform_dirty` → `update_path_vertices`；否则 `path_slot.vertices_dirty || is_bones_update()` → 更新并清 dirty；两者都否且 `!dirty` → return；
   - 计算 `spaces`（Length/ChainScale 用 bone length+矩阵；其余填 spacing）；
   - `compute_bezier_curve(path_display, spaces_count, tangent, percent_position, percent_spacing)`；
   - 旋转/平移 mix 改链骨矩阵，写 `global.from_matrix`；`dirty=false`。
5. `update_path_vertices`：
   - 无 weight：path slot 父骨 `update_by_constraint`，用 `parent.global_transform_matrix` 变换 IR 顶点（`float_array`，`scale = armature_data.scale`）；
   - 有 weight：遍历 geometry_bones，按 weight 混合（`WeightData` IR）。
6. `compute_bezier_curve`：逐行移植 F6.13/F6.14（非匀速 curveLengths 分支、匀速 10 段积分、`add_curve_position`）；`positions` 长度 `spaceCount*3+2`；所有 `continue`/边界保持官方。
7. `invalid_update` 为空；`dirty` 由外部/时间轴重置（Path 无 timeline）。

### 测试

- 合成夹具 3 组模式各 120 帧 pose（骨 global/矩阵）与 JS 全等；`stage6dump path` 先验证无 NaN。
- `init` 失败分支：path slot 不存在/display 非 Path/骨骼列表空 → 不加入 constraints（与 TS null 返回一致）。
- 借用/边界：root/target/pathSlot 的父骨跨切片；`bones` 包含 root 之外/顺序乱序时按名解析。

### 风险

- Path 是阶段 6 最高风险项：源 600 行、借用复杂、无官方素材。若时间不足，**不得**用 stub 冒充完成；必须按 §1.8 登记未完成并保留 P1-2 未通过状态。

---

## 任务 6 — cacheFrameRate 帧缓存

### 文件

| 文件 | 处理 |
|---|---|
| `src/armature/cache.rs` | `ArmatureCache` / `AnimationFrameCache` / set/get/pack 10 f64 |
| `src/armature/armature.rs` | `cache_frame_rate: f64`、`cache: ArmatureCache`、setter/递归 child、`advance_time` 传参 |
| `src/animation/animation_state.rs` | F6.19：量化、cache index、bool、bone timeline skip |
| `src/animation/player.rs` | F6.18：animationDirty 时赋 index；`advance_time_with_context` 增加 cache 参数 |
| `src/armature/bone.rs` / `slot.rs` | F6.20：缓存分支接 set/get；签名加 cache |
| `src/factory/base_factory.rs` | `set_armature_cache_frame_rate` + rate 表（D-6.2） |
| `tests/cache.rs` / `tests/golden_cache.rs` | 无缓存 vs 有缓存 pose、100 帧差分 |

### 实现细节

1. `ArmatureCache::new(rate, armature_data)`：有效 rate = `max(ceil(rate*animation.scale),1)`；`cache_count = ceil(rate*duration)+1`；`cached_frames = vec![false; count]`；每个 bone/slot 建 `vec![-1; count]`；`frames=Vec::new()`。
2. `set_cache_frame(matrix, transform) -> i32`：push 10 个 f64，返回旧长度；`get_cache_frame(..., offset)` 读回 10 值并同步 `transform.x/y`。
3. `Animation::advance_time_with_context`：
   - 单 state 时取 `cache_frame_rate`（来自 Armature 字段）；
   - `animation_dirty && rate>0`：`bone.cached_frame_indices = Some(cache.animation.bone_indices[name].clone())`；slot 同理（仅 defaultSkin display 条件，照 :221-226）；
   - 清理 `animation_dirty=false`。
4. `AnimationState`：F6.19 的 `is_cache_enabled`、量化、`cached_frames[index]`、`is_update_bone_timeline`；把 `context.cache_frame_index` 写回 Armature（当前已有字段）。
5. `Bone/Slot::update`：F6.20 四分支；`set_cache_frame` 用当前 `FrameCache`；`get_cache_frame` 读回；`unreachable!` 删除。
6. Factory setter：`set_armature_cache_frame_rate(idx, rate)`；若 data 的 rate 已 >0 且不同，官方 setter 会因 `cacheFrames` guard 不重算（`if cacheFrameRate>0 return`）——照抄该 guard，测试锁定“第二次设置不改变缓存结构”。
7. Child 继承：`Slot::update` 子骨架切换时，若 `child.cache_frame_rate == 0` 且 parent rate !=0 → `child.set_cache_frame_rate(parent_rate)`；递归由 Armature setter 完成。

### 测试

- `Dragon/walk`、`Rooster_Ani/rooster_idle_anim` 在 `rate=24` 下 120 帧 pose 与无缓存一致；与 JS `cache_dump` golden 全等。
- 同帧重复（同 cache index）不重复更新；不同 index 走 set/get；非脏回填索引。
- `rate=0` 完全走旧路径；第二次 `set` guard；child 继承。
- 性能不作为验收，只测行为。

---

## 任务 7 — replace_* 与工厂 API

### 文件

| 文件 | 处理 |
|---|---|
| `src/factory/base_factory.rs` | `replace_display/slot_display/slot_display_list/skin/animation`；`get_dragon_bones_data`/`get_texture_atlas_data`/`update_texture_atlases`/`clear`/`remove*` |
| `src/animation/player.rs` | `animations_override`、`animation_data/names/play_config` 走 override |
| `src/armature/slot.rs` | 需要的只读 display/display_frames 访问器 |
| `tests/replace.rs` / `tests/golden_replace.rs` | API 行为 + 合成夹具对拍 |

### 实现要点

- `replace_display`：index 归一（<0 取 slot.display_index，再 <0 取 0）；`replace_display_data`；非空 → `get_slot_display_for_replace`（image raw 是 mesh → meshDisplay）；空 → `replace_display(None,index)`。
- `replace_slot_display`：`get_armature_data(armatureName, dbName).defaultSkin.get_display(slotName, displayName)`；失败 false。
- `replace_slot_display_list`：display 列表 → `set_display_frame_count(len)` → 逐 index `replace_display`（**不**清 raw data，照官方）。
- `replace_skin`：exclude 跳过；miss 回退 defaultSkin；override miss → `display_frame_count=0`；命中逐 frame `replace_raw_display_data` + `replace_display`；success 标记。
- `replace_animation`：override true 用目标 data 的 `animations`；false 合并（后写覆盖）；递归 child（遍历 `DisplayHandle::Armature` 与显示帧，按目标 skin displays 找 child data）；返回 `defaultSkin` 存在与否。
- **与 cache 的交互**：若缓存已初始化后 `replaceAnimation` 改了 animation 集合，按目标 `AnimationData` 重建/清空该 Armature 的 `AnimationFrameCache`（速率与 duration 可能不同）；官方 data 共享缓存的路径没有直接对应，登记在 D-6.1。
- `get_*`/`remove*`/`clear`：按官方 map 语义；`remove` 后对象立即 drop（D-6.8），不再 `bufferObject`。
- 借用：工厂方法统一 take armature out，操作完放回；`replace_display` 内 child build 需要 `&mut self`，take 可避免与 arena 冲突。

### 测试

- 合成夹具：replaceDisplay 单个/空；slotDisplay 命中/miss；displayList 数量；replaceSkin override/exclude/回退；replaceAnimation override/merge/child 递归。
- 记录 `displayIndex/displayFrameCount/当前 display name/animationNames`；末帧 pose 与 JS `replace_dump` 全等。
- 现有 `build_armature`/stage 5 全绿。

---

## 任务 8 — P1 剩余项审计

| 项 | 要做的事 | 测试 |
|---|---|---|
| P1-6 UserData/ActionData | 解析 `userData`（armature/bone/slot/display），`actions`/`defaultActions` 到 model；action 队列已有 | 合成 features 夹具；`get_int/float/string`；defaultActions → child armature Play |
| P1-7 CanvasData | parser 已存 `armature.canvas`；核对默认值与 scale | features 夹具断言 x/y/w/h/color/background |
| P1-8 BlendMode | parser 的字符串/数字两路；核对 14 值映射 | 14 值 fixture，`SlotData.blend_mode` 逐值 |
| P1-9 脏标记 | 审计 `_lockUpdate/invalidUpdate/slotsDirty/zIndexDirty/alphaDirty/transformDirty/childrenTransformDirty` 是否都有入口/消费 | 现有 stage 3/4 测试 + 补充路径 |
| F6.9 weighted deform | 官方 TODO；Rust 保留 TODO + 注释 | 不实现，登记 |
| D-4.35 zIndex | 任务 1 已完成 | zIndex 测试 |

不新增“看起来完整”的空实现；官方 TODO 原样保留并在差异表引用。

---

## 任务 9 — Review 与验收

### 验收命令

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node tools/gen_pose.mjs
node tools/gen_golden.mjs          # 现有 37 份零 diff
node tools/gen_matrix.mjs          # 现有 131 份零 diff
node tools/gen_stage6.mjs          # dbbin/IR/path/hit/cache/replace 全生成、重复零 diff
```

### P1 验收表

| # | 验收项 | 证据 |
|---|---|---|
| P1-1 | dbbin 解析 | `tests/dbbin.rs` + 9 动画 golden |
| P1-2 | Path 约束 | `tests/path_constraint.rs` + 3 组 path golden |
| P1-3 | BoundingBox 三种 + 命中 | `tests/bounding_box.rs` + bbox golden |
| P1-4 | cacheFrameRate | `tests/cache.rs` + cache golden |
| P1-5 | replace 系列 | `tests/replace.rs` + replace golden |
| P1-6 | UserData/ActionData/defaultActions | features 测试 + child action 回归 |
| P1-7 | CanvasData | features 测试 |
| P1-8 | 14 BlendMode | features/14 值测试 |
| P1-9 | 脏标记 | stage 3/4 回归 + 审计记录 |
| P1-10 | IR 二进制打包 | `tests/ir_binary.rs` 9/9 字节相等 |
| D-4.35 | zIndex f64 | `tests/z_index.rs`，D-4.35 关闭 |

### 交付摘要

- 逐项 P1 结果、二进制/IR 最大误差、Path/BBox/cache/replace golden 数量；
- 现有阶段 0–5 全绿证据（37+131 golden、212 测试基线不退化）；
- D-6.1～D-6.11 差异登记；
- 明确未完成项（若有）：不得用 stub 掩盖，写明原因/后续阶段。

---

## 常见错误速查

| 症状 | 根因 | 正确做法 |
|---|---|---|
| dbbin 解析崩溃/全 NaN | 偏移当成“元素数”或漏了 `12+headerLen` | 长度是字节；所有偏移相对负载起点 |
| dbbin 动画时间/帧数错 | 把 `duration` 当秒 | 二进制 `duration` 是帧数；`duration=帧数/frameRate` |
| dbbin timeline frameIndices 错位 | 忘记 `frameCount+1` 或最后一帧 count | 照 F6.4 |
| dbbin geometry 权重越界 | `weightOffset` 的 -1 哨兵/65536 wrap | 照 F6.5 |
| Node Buffer 传给 JS binary parser 失败 | `instanceof ArrayBuffer` 不认 Node Buffer | `buf.buffer.slice(byteOffset, byteOffset+byteLength)` |
| Path 骨 NaN | Path 顶点/长度布局非法，或 targetDisplay 缺省错 | 顶点长度 6 的倍数、显式 `targetDisplay`、先跑 JS 验证 |
| Path 不生效 | path slot 当前 display 不是 `Path` / root 不对 | 校验 `DisplayKind::Path` 与 `bones[0]` |
| BoundingBox 永远 false | `Slot.bounding_box_data` 未由 display 刷新 | 先 `update_display_data`/`advanceTime(0)`；检查 display 类型 |
| 命中点/法线坐标系错 | 忘记逆变换/正变换 | Slot 内逆矩阵进、正矩阵出 |
| rectangle `B.y` 奇怪 | 官方 bug `y = xB` | 照抄并测试锁定，不要修 |
| cache 不生效 | rate 只写 Armature 字段，未 init cache 表 | setter 同时建 `AnimationFrameCache` |
| cache 后 pose 错 | 量化/`isUpdateBoneTimeline` 顺序错 | 照 F6.19/F6.20，先量化再算 index |
| 第二实例 cache 数值不同 | 把 per-Armature 缓存误当共享 | 同输入应同值；差异是性能不是数值 |
| replace 后 display 错 | 用了 rawDisplayData 而非 displayData，或 index 归一错 | 照 F6.22 |
| replaceAnimation 后播不到 | `play_config` 仍读 data.animations | 走 `Animation::animation_data` override |
| zIndex 排序抖动 | f64 键 + 不稳定排序 | 稳定 `sort_by`，NaN 视为 Equal 并 warn |

---

## 完成后

1. 全量命令通过；阶段 5 的 212 测试、37 baseline、131 matrix 不退化。
2. 新增测试/夹具/工具与 `testdata/stage6/**` 入库；`gen_stage6` 可重复零 diff。
3. 更新 `docs/model_mapping.md` D-6.x、关闭 D-4.35、勘误 P1-10 名称。
4. Review 通过后编写阶段 7 指南（Bevy 适配层）。
