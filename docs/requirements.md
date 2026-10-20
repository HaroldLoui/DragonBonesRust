# DragonBones Rust 运行时 — 需求文档与实施指南

> 版本：v1.0（2026-10-03）
> 状态：调研完成，待实施
> 关联目录：`DragonBonesJS/`（官方 JS 参考实现）、`assets/`（待播放素材）、`src/`（当前为空壳）

---

## 目录

- [0. 概述](#0-概述)
- [1. 调研结论（已验证的技术事实）](#1-调研结论已验证的技术事实)
- [2. 需求清单](#2-需求清单)
- [3. 分阶段实施计划](#3-分阶段实施计划)
- [4. 关键移植决策（D1–D7）](#4-关键移植决策d1d7)
- [5. 风险登记册与待验证问题](#5-风险登记册与待验证问题)
- [6. 资料索引](#6-资料索引)
- [7. 里程碑总表](#7-里程碑总表)

---

## 0. 概述

### 0.1 项目目标

用 Rust **完全复刻** DragonBones 官方 JS 运行时（`DragonBonesJS/DragonBones`）的核心部分，并提供一个 Bevy 适配层，最终在 Bevy 中播放 `assets/` 目录下的 9 个素材动画。

### 0.2 "100% 复刻"的验收定义

以 **JS 版运行时的实际行为为规格（spec）**，通过逐帧数值差分测试验收：

- 9 个素材 × 全部动画 × ≥120 帧
- 骨骼世界变换、插槽状态（displayIndex / color / zOrder / zIndex / visible）、事件序列
- `f64` 字段容差 `1e-6`，dbbin（f32）路径容差 `1e-4`，整数/枚举/索引必须完全相等

差分测试全绿 = 100%。任何主观的"看起来一样"都不作为验收依据。

### 0.3 范围声明

**包含：**

- `DragonBonesJS/DragonBones/src/dragonBones/` 下全部 37 个 TS 文件的等价 Rust 实现
- 一个 Bevy 适配层（`dragonbones-bevy`）

**不包含：**

- 6 个 JS 平台适配器（Egret / Hilo / Phaser / Pixi / ThreeJS / Cocos）
- 仓库的 build 脚本、JS demo 页面
- 滤镜类功能（ColorFilter / BlurFilter 在 core 中并不存在）

### 0.4 技术选型结论

| 项 | 结论 |
|---|---|
| Bevy 版本 | **0.19.1 stable** 开发；0.20 正式发布后只迁移适配层（core 与 Bevy 完全解耦） |
| Rust edition | 2024（`Cargo.toml` 已设置） |
| 数值精度 | core 全程 `f64`，仅在渲染边界降 `f32` |
| 数据结构 | arena + `newtype` 索引，禁用 `Rc<RefCell>` |
| JSON 解析 | `serde_json::Value` + 宽松 helper，不用 derive 结构体 |
| 验收方式 | Node 参考环境 + 黄金数据差分测试 |

---

## 1. 调研结论（已验证的技术事实）

以下结论均经实际核查或运行验证，不是推测。

### 1.1 核心运行时盘点

`DragonBonesJS/DragonBones/src/dragonBones/` 共 **37 个 TS 文件 / 16679 行**（实代码 11131 行 / 注释 5547 行 / 空行 1682 行），按目录分 9 个模块：

| 模块 | 文件数 | 行数 | 作用 |
|---|---|---|---|
| `parser/` | 3 | 2760 | JSON / dbbin → 内部扁平 IR。`ObjectDataParser.ts` 2436 行，是最重的单文件 |
| `animation/` | 6 | 4079 | `Animation` / `AnimationState` / 16 个具体 TimelineState / 5 个抽象基类 / `WorldClock` / `BlendState` |
| `armature/` | 7 | 4130 | `Armature` / `Bone` / `Slot` / `Surface` / `Constraint(IK+Path)` / `TransformObject` / `IArmatureProxy` |
| `model/` | 11 | 2758 | 数据模型：`ArmatureData` `BoneData` `SurfaceData` `SlotData` `SkinData` `DisplayData` `AnimationData` `AnimationConfig` `BoundingBoxData` `ConstraintData` `TextureAtlasData` `CanvasData` `UserData` `DragonBonesData` |
| `factory/` | 1 | 951 | `BaseFactory` 装配逻辑（与渲染无关） |
| `geom/` | 5 | 862 | `Matrix` (333) / `Transform` (248) / `Point` (93) / `Rectangle` (124) / `ColorTransform` (64) |
| `core/` | 2 | 553 | `BaseObject` 对象池 (178) + `DragonBones` 全局入口与全部枚举 (375) |
| `event/` | 2 | 386 | `EventObject` (277) + `IEventDispatcher` (109) |

**关键结论 1：核心运行时零 DOM / 浏览器依赖。** 全仓库检索确认：无 `window` / `document` / `HTMLImageElement` / `Audio` / `requestAnimationFrame`，仅使用 `console.warn/assert/info` 与 `Date.now`。核心是纯逻辑代码（数学、数组、插值、状态机），1:1 移植无障碍。

**关键结论 2：渲染已被完全隔离。** 所有与引擎相关的调用都通过三组接口收敛：

1. `Slot` 的 14 个抽象方法
2. `BaseFactory` 的 3 个抽象方法
3. `IArmatureProxy` + `IEventDispatcher`

这三组接口就是 Bevy 适配层需要实现的**全部**契约。

**关键结论 3：解析器产出扁平 IR，时间轴按偏移读取。** JSON 模式下 `ObjectDataParser` 同样会把数据编译成 `intArray / floatArray / frameArray / timelineArray / colorArray` 五组扁平数组，所有 `BaseTimelineState` 通过 `BinaryOffset` 枚举定义的偏移读取（`core/DragonBones.ts:27-55`）。**这个中间表示是整个复刻的地基，必须逐元素对齐。**

### 1.2 适配层契约

**`Slot` 需要实现的 14 个抽象方法**（`armature/Slot.ts`）：

```
_initDisplay(value, isRetain)     _disposeDisplay(value, isRelease)
_onUpdateDisplay()                _addDisplay()
_replaceDisplay(value)            _removeDisplay()
_updateZOrder()                   _updateVisible()
_updateBlendMode()                _updateColor()
_updateFrame()                    _updateMesh()
_updateTransform()                _identityTransform()
```

**`BaseFactory` 需要实现的 3 个抽象方法**（`factory/BaseFactory.ts`）：

```
_buildTextureAtlasData(textureAtlasData, textureAtlas)
_buildArmature(dataPackage)
_buildSlot(dataPackage, slotData, armature)
```

**`IArmatureProxy`**（`armature/IArmatureProxy.ts:36-92`）：

```
dbInit(armature) / dbClear() / dbUpdate() / dispose(disposeProxy)
readonly armature / readonly animation
```

加上 `IEventDispatcher` 的 4 个方法：`addDBEventListener` / `hasDBEventListener` / `removeDBEventListener` / `dispatchDBEvent`。

**参考实现规模**：`Pixi/8.x/src/dragonBones/pixi/` 仅 4 个文件 607 行代码。Bevy 适配层的**代码量**量级相当，但渲染实现复杂度更高（见 §5 风险 R1/R2/R3）。

### 1.3 素材盘点（assets/）

| 素材 | 动画 | mesh 变形 | blend 模式 | 图集 |
|---|---|---|---|---|
| 龙 | stand | ✔ (14 顶点) | — | 自有 png |
| Dragon_Blend | stand, walk, jump, fall | — | **add, multiply, overlay, screen** | 共用 Dragon_tex.png |
| Dragon | stand, walk, jump, fall | — | — | Dragon_tex.png |
| PlantAnimate_ske_compressed | Idle, Shoot, Death, Food, Water, Random | — | — | PlantAnimate_tex.png |
| PlantAnimate_ske | 同上 | — | — | PlantAnimate_tex.png |
| Rooster_Ani | 5 个 rooster_* 动画 | ✔ (8) | — | Rooster_Ani_tex.png |
| Rooster_Blend | 同上 | ✔ (8) | **add** | 共用 Rooster_Ani_tex.png |
| Rooster_P1 | 同上 | ✔ (8) | — | 共用 Rooster_Ani_tex.png |
| Surface_P1_4 | idle | ✔ (3) | — | 共用 Dragon_tex.png |

补充事实：

- 全部为 **DragonBones 5.5 数据格式**，单骨架、单页图集
- `PlantAnimate_ske_compressed.json`（37KB）与 `PlantAnimate_ske.json`（251KB）**键集合与帧计数完全一致**，前者只是移除了空白字符的同一格式，无需专门处理逻辑
- 文件名含非 ASCII 字符（中文），需验证 Bevy 资源加载路径处理（风险 R7）
- 仓库 `Pixi/Demos/` 下还有大量额外素材，含 `.dbbin` 二进制样本，可用于 P1 阶段测试

### 1.4 外部环境

| 项 | 实测结果 |
|---|---|
| Bevy 版本 | `0.20.0-rc.2`（2026-09-28）、stable `0.19.1` |
| Bevy 0.19.1 关键 API | `bevy::mesh::Mesh2d`、`bevy::sprite_render::Material2d`（含 `specialize` / `fragment_shader` / `vertex_shader` / `alpha_mode` / `depth_bias`）、`ColorMaterial`、`AlphaMode2d`、`MeshMaterial2d` |
| Node | v22.18.0 已安装 |
| JS 参考运行时 | `DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js`（728KB UMD）**已实测可在 Node 中直接 require 并完整驱动** |
| crates.io 现状 | `rusty_dragonbones` 0.3.0 仅 354 行，是骨架，无参考价值；`bevy_spine` 等仅覆盖 Spine |
| 许可证 | MIT（`DragonBonesJS/LICENSE`），衍生实现需保留版权声明 |

---

## 2. 需求清单

> 编号规则：P0 = 核心可播放（必须），P1 = 完整度（核心 100% 范围），P2 = Bevy 适配。

### 2.1 P0 — 核心可播放

| # | 需求 | 验收标准 |
|---|---|---|
| P0-1 | `geom` 5 个类：`Matrix` / `Transform` / `Point` / `Rectangle` / `ColorTransform`，含 `updateTransform` 的 skew + scale 组合语义 | 单元测试对齐 TS 输出 |
| P0-2 | `BaseObject` 对象池语义（`borrowObject` / `returnToPool` / `_onClear` / `bufferObject` 缓冲回收队列） | 生命周期与 JS 等价，无提前释放 |
| P0-3 | 全部枚举（`core/DragonBones.ts:27-277`）：`BinaryOffset` `ArmatureType` `BoneType` `DisplayType` `BoundingBoxType` `ActionType` `BlendMode(14)` `TweenType` `TimelineType(17)` `OffsetMode` `AnimationFadeOutMode` `AnimationBlendType` `AnimationBlendMode` `ConstraintType` `PositionMode` `SpacingMode` `RotateMode` | 数值一一对应 |
| P0-4 | `model/` 11 个数据模型的字段与结构 | 与 TS 字段一一对应 |
| P0-5 | JSON 解析器 → 扁平 IR，支持数据版本 `2.3 / 3.0 / 4.0 / 4.5 / 5.0 / 5.5 / 5.6` | 9 个素材的 IR 数组与 JS **逐元素相等** |
| P0-6 | 骨架层级变换：`inheritTranslation / inheritRotation / inheritScale / inheritReflection` + `flipX / flipY` + `Alpha` 传播 | 姿态矩阵差分 ≤ 1e-6 |
| P0-7 | `Slot` 全逻辑：displayIndex、color、zOrder、zIndex、visible、alpha、deformVertices、DisplayFrame、14 个抽象方法的 Rust 等价物（trait） | 差分通过 |
| P0-8 | **16 个具体 TimelineState + 5 个抽象基类 + `BlendState`**<br>具体：`ActionTimelineState` `ZOrderTimelineState` `BoneAllTimelineState` `BoneTranslateTimelineState` `BoneRotateTimelineState` `BoneScaleTimelineState` `SurfaceTimelineState` `AlphaTimelineState` `SlotDisplayTimelineState` `SlotColorTimelineState` `SlotZIndexTimelineState` `DeformTimelineState` `IKConstraintTimelineState` `AnimationProgressTimelineState` `AnimationWeightTimelineState` `AnimationParametersTimelineState`<br>抽象：`TimelineState` `TweenTimelineState` `SingleValueTimelineState` `DoubleValueTimelineState` `MutilpleValueTimelineState` | 逐帧数值差分 |
| P0-9 | `Animation` + `AnimationState` 状态机：`playConfig` / `play` / `fadeIn` / `stop` / `fadeOut` / `gotoAndPlayByTime\|Frame\|Progress` / `gotoAndStopByTime\|Frame\|Progress` / layer / group / weight / boneMask / resetToPose / additive / autoFadeOutTime / fadeTotalTime / `AnimationFadeOutMode` 5 种 | 交叉淡化场景差分通过 |
| P0-10 | mesh 变形：`Surface` 骨骼、`SurfaceTimelineState`、`DeformTimelineState`、权重蒙皮、`_updateMesh` 数学公式 | 5 个含 mesh 素材差分通过 |
| P0-11 | 事件系统：`EventObject`（`frameEvent` / `soundEvent` / action）、缓冲与在 `advanceTime` 末尾派发 | 事件序列与 JS 完全一致 |
| P0-12 | `WorldClock` + `DragonBones.advanceTime` 顶层驱动（含对象回收与事件派发顺序） | 时间推进行为一致 |
| P0-13 | `BaseFactory` 装配：`_fillBuildArmaturePackage` / `_buildBones` / `_buildSlots` / `_buildConstraints` / `_getSlotDisplay` / `buildArmature` | `buildArmature` 后骨架结构一致 |
| P0-14 | **差分测试基建**（详见 §3 阶段 0） | 9 素材 × 全动画 × ≥120 帧全绿 |

### 2.2 P1 — 完整度

| # | 需求 | 备注 |
|---|---|---|
| P1-1 | dbbin 二进制解析（`parser/BinaryDataParser.ts` 379 行），含 UTF-8 解码、TypedArray 视图、f32/f64 差异 | 可用 `Pixi/Demos/.../mecha_*_ske.dbbin` 测试 |
| P1-2 | IK 约束 + Path 约束（`armature/Constraint.ts` 717 行） | 当前 assets 无 IK，但属"100%"范围 |
| P1-3 | BoundingBox：`containsPoint` / `intersectsSegment`，矩形 / 椭圆 / 多边形三种 | `model/BoundingBoxData.ts` 654 行 |
| P1-4 | `cacheFrameRate` 帧缓存（`AnimationState` 的 `cachedFrameIndices`、`Bone/Slot` 的缓存命中路径） | 影响行为路径，不只是性能 |
| P1-5 | 换装与替换 API：`replaceSkin` / `replaceDisplay` / `replaceSlotDisplay` / `replaceSlotDisplayList` / `replaceAnimation` / `replacedTexture` | `BaseFactory` 公开 API |
| P1-6 | `UserData` / `ActionData` / `defaultActions` / 子骨架动作播放 | |
| P1-7 | `CanvasData`（`"canvas"` 键）解析 | core 只存数据，无渲染 |
| P1-8 | 14 种 `BlendMode` 的数据透传（渲染效果归适配层） | |
| P1-9 | 脏标记逻辑：`_lockUpdate` `invalidUpdate` `_slotsDirty` `_zIndexDirty` `_alphaDirty` `_transformDirty` `_childrenTransformDirty` 等 | 逐行对照 |
| P1-10 | `ObjectDataParser` 的二进制**写出**路径（`_parserDragonBonesData`，约 `ObjectDataParser.ts:2229-2400`） | 用于自测：写出再读回应等价 |

### 2.3 P2 — Bevy 适配

| # | 需求 |
|---|---|
| P2-1 | `DragonBonesPlugin` + 自定义 `AssetLoader`（`_ske.json` / `_tex.json` / `_tex.png` → Bevy Asset） |
| P2-2 | 实体映射：骨架根实体 + 每 Slot 一个子实体 |
| P2-3 | 每帧同步：骨架世界矩阵 → 网格顶点（含 skew 剪切，见 R2） |
| P2-4 | 颜色同步：`ColorTransform` → 顶点色（乘子 + 偏移） |
| P2-5 | zOrder / zIndex → 绘制顺序（见 R3） |
| P2-6 | blend 模式 → 自定义 `Material2d`（见 R1） |
| P2-7 | 事件 → Bevy `Event` / `EventWriter` |
| P2-8 | 时间驱动：接入 Bevy `Time` + `SystemSet` 调度 |
| P2-9 | Demo：播放 `assets/` 下 9 个素材动画，覆盖 blend 与 mesh 用例 |
| P2-10 | Bevy 0.20 迁移预案（只动 `dragonbones-bevy`） |

---

## 3. 分阶段实施计划

每个阶段统一格式：**目标 / 任务分解 / 关键技术决策 / 交付物 / 验收标准 / 常见坑**。

### 阶段 0 — 工程基建与差分测试环境

**目标**：在写任何 Rust 业务代码之前，先把"100%"从口号变成可执行的验收标准，同时验证参考环境可用。

**任务分解**

1. 创建 Cargo workspace，三个 crate：
   ```
   crates/dragonbones-core/     纯逻辑，禁止依赖 bevy
   crates/dragonbones-bevy/     适配层（本阶段只建空壳）
   crates/dragonbones-demo/     可执行 demo（本阶段只建空壳）
   ```
2. 实现 `tools/jsdump.mjs`：Node 参考转储工具（完整可用代码见下方）
3. 定义**转储协议**（两侧必须完全一致的 JSON 结构）
4. 生成并入库 `testdata/golden/*.json` 黄金数据
5. 搭建 `cargo test` → 调用 node 生成/读取黄金数据的测试脚手架
6. 预研任务 Q1 / Q3（见 §5）

**关键技术决策**

- JS 参考 bundle 顶层引用 `PIXI`，必须先打桩再 require
- 对象池按 `String(constructor)` 分键，自定义子类**必须覆盖静态 `toString()`**，否则 `borrowObject` 抛错
- 转储必须排除 `hashCode` 等与实现相关的字段

**交付物**

- `tools/jsdump.mjs`
- `testdata/golden/*.json`（9 个素材的逐帧黄金数据）
- workspace 骨架 + 空测试

**验收标准**

- `node tools/jsdump.mjs assets/Dragon_ske.json` 能输出 19 骨骼 / 18 插槽骨架的 120 帧数据
- 9 个素材全部转储成功，无异常

**JS 参考环境完整代码**（已实测跑通）

```js
// tools/jsdump.mjs
globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
}; // 仅为满足 bundle 顶层引用，core 不会真正用到

const fs = require("fs");
const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

// 池按 String(constructor) 分键，必须覆盖静态 toString（否则 borrowObject 抛错）
class NullAtlas extends db.TextureAtlasData {
  static toString() { return "NullAtlas"; }
  _onClear() { super._onClear(); }
  createTexture() { return new db.TextureData(); }
}

// 实现 Slot 的 14 个抽象方法，全部 no-op（我们要的是核心逻辑，不是渲染）
class NullSlot extends db.Slot {
  static toString() { return "NullSlot"; }
  _initDisplay() {}
  _disposeDisplay() {}
  _onUpdateDisplay() { this._rd = this._display || this._rawDisplay; }
  _addDisplay() {}
  _replaceDisplay() {}
  _removeDisplay() {}
  _updateZOrder() {}
  _updateVisible() {}
  _updateBlendMode() {}
  _updateColor() {}
  _updateFrame() {}
  _updateMesh() {}
  _updateTransform() {}
  _identityTransform() {}
}

// IArmatureProxy + IEventDispatcher 实现
class EvProxy {
  constructor() { this._armature = null; this._l = {}; }
  dbInit(a) { this._armature = a; }
  dbClear() {}
  dbUpdate() {}
  dispose() {}
  get armature() { return this._armature; }
  get animation() { return this._armature.animation; }
  addDBEventListener(t, f) { (this._l[t] = this._l[t] || []).push(f); }
  hasDBEventListener(t) { return (this._l[t] || []).length > 0; }
  removeDBEventListener() {}
  dispatchDBEvent(t, e) { (this._l[t] || []).forEach(f => f(e)); }
}

// 实现 BaseFactory 的 3 个抽象方法
class NullFactory extends db.BaseFactory {
  static toString() { return "NullFactory"; }
  _buildTextureAtlasData(ta) {
    return ta || db.BaseObject.borrowObject(NullAtlas);
  }
  _buildArmature(pkg) {
    const arm = db.BaseObject.borrowObject(db.Armature);
    const p = new EvProxy();
    arm.init(pkg.armature, p, p, this._dragonBones);
    return arm;
  }
  _buildSlot(pkg, slotData, armature) {
    const s = db.BaseObject.borrowObject(NullSlot);
    s.init(slotData, armature, {}, {});
    return s;
  }
}

// ---- 主流程 ----
const em = new EvProxy();
const factory = new NullFactory();
factory._dragonBones = new db.DragonBones(em);

factory.parseDragonBonesData(
  JSON.parse(fs.readFileSync(process.argv[2], "utf8")), "Dragon");
factory.parseTextureAtlasData(
  JSON.parse(fs.readFileSync(process.argv[3], "utf8")), null, "Dragon");

const armature = factory.buildArmature("Dragon", "Dragon");
// ... 逐帧 armature.advanceTime(1/24)，dump bones/slots/events
```

**转储协议**

```jsonc
{
  "frames": [
    {
      "i": 0,
      "bones": { "<boneName>": [x, y, rotation, scaleX, scaleY, skew, a, b, c, d] },
      "slots": {
        "<slotName>": [displayIndex, visible, zOrder, zIndex,
                       rM, gM, bM, aM, rO, gO, bO, aO,
                       globalX, globalY]
      },
      "events": [ { "type": "...", "name": "...", "bone": "...", "slot": "..." } ]
    }
  ]
}
```

**常见坑**

- `require` bundle 时 `PIXI` 未定义会在模块顶层就崩（`PixiArmatureDisplay extends PIXI.Container`）
- `buildArmature` 默认 `textureAtlasName=""`，而图集按显式名字注册 → 需显式传第 4 参或置 `factory.autoSearch = true`
- `hasDBEventListener` 必须实现，否则 `AnimationState._advanceFadeTime` 抛 TypeError

**工期**：0.5 天

---

### 阶段 1 — geom + core + model

**目标**：搭起与 TS 一一对应的类型系统与底层数学，为解析层和运行层提供地基。

**任务分解**

| TS 源文件 | 行数 | Rust 目标 |
|---|---|---|
| `geom/Matrix.ts` | 333 | `src/geom/matrix.rs` |
| `geom/Transform.ts` | 248 | `src/geom/transform.rs` |
| `geom/Point.ts` | 93 | `src/geom/point.rs` |
| `geom/Rectangle.ts` | 124 | `src/geom/rectangle.rs` |
| `geom/ColorTransform.ts` | 64 | `src/geom/color.rs` |
| `core/BaseObject.ts` | 178 | `src/core/base_object.rs` |
| `core/DragonBones.ts` | 375 | `src/core/enums.rs` + `src/core/dragon_bones.rs` |
| `model/*.ts`（11 个） | 2758 | `src/model/*.rs` |

**关键技术决策**

1. **`geom` 5 类自己实现，不用 `glam`。** `Transform` 同时持有 `x/y/skew/scaleX/scaleY/rotation`，`Matrix` 是 2D 仿射（a/b/c/d/tx/ty），与 `glam` 的语义不一致，强转会引入偏差。全部字段名与 TS 保持一致（`skewX` → `skew_x`）。
2. **对象池先做语义，后做性能。** 阶段 1 只实现：
   ```rust
   trait Poolable { fn on_clear(&mut self); }
   // 显式缓冲队列，镜像 DragonBones.bufferObject / 下帧统一回收
   ```
   性能型 free-list 池（`TypeId` + `Vec<T>`）推迟到 P1 阶段，避免过早优化带来的生命周期 bug。
3. **`Map<T>` → `IndexMap` 或 `Vec<(String, T)>`。** TS 的 `Map<T>` 是插入有序的对象字典；解析与遍历顺序会影响结果（如 `_cacheBones` 遍历），**必须保持插入顺序**，推荐 `indexmap::IndexMap`。
4. **枚举用 `#[repr(u32)]` + TS 数值对齐**，例如 `BlendMode::Overlay = 11`，方便与 JSON 中的数字形式直接互转。

**交付物**

- `crates/dragonbones-core/src/{geom,core,model}/` 全部模块
- 单元测试：矩阵乘法、`Transform.updateTransform` 的 skew 语义、`ColorTransform` 组合

**验收标准**

- `geom` 单测对齐 TS（构造相同输入，比较输出到 1e-12）
- 11 个 model 类型字段与 TS 一一对应（写一个对照表进 `docs/` 或注释）
- 全部枚举数值与 `core/DragonBones.ts:27-277` 逐个核对

**常见坑**

- `Transform.updateTransform` 处理的是 `skewX/skewY` 与 `scaleX/scaleY` 的组合，直觉写法（当成 rotation）会错
- `Matrix.append` / `prepend` 的顺序与 `glam` 相反，务必按 TS 逐行翻译
- `ColorTransform` 的"乘子 + 偏移"组合顺序（先乘后加，且与 `_globalAlpha` 相乘的时机）容易搞反

**工期**：3–5 天

---

### 阶段 2 — 解析器与 IR

**目标**：把 JSON 编译成与 JS 完全一致的扁平 IR。**这是整个项目最重要的分层验收点**——IR 对齐了，后续动画逻辑才有可靠的对照基准。

**任务分解**

| TS 源文件 | 行数 | 说明 |
|---|---|---|
| `parser/DataParser.ts` | 309 | 全部数据键常量 + 版本表 |
| `parser/ObjectDataParser.ts` | 2436 | 核心解析器，按函数逐个翻译 |
| （P1）`parser/BinaryDataParser.ts` | 379 | dbbin 读取 |

翻译顺序建议（依赖由浅入深）：

1. `_getBoolean` / `_getNumber` / `_getString` 三个 helper（`ObjectDataParser.ts:36-92`）
2. `parseTextureAtlasData`（结构最简单，先打通）
3. `parseDragonBonesData` → bone / slot / skin / display
4. animation → timeline → frame（占 `ObjectDataParser.ts` 约 60% 篇幅）
5. mesh / weights / `slotPose` / `bonePose`（蒙皮解析，最易错）
6. `_parserDragonBonesData`（IR 编译，P1-10）

**关键技术决策**

1. **不用 serde derive 结构体。** 严格 struct 会因"字段缺省 / 值为字符串 / 版本差异"大面积失败。入口收 `serde_json::Value`，全程用 helper 读取：
   ```rust
   fn get_number(raw: &Value, key: &str, default: f64) -> f64 {
       match raw.get(key) {
           None => default,
           Some(v) if v.is_null() || v == "NaN" => default,
           Some(v) => js_to_number(v),   // NaN / "" / null → 0
       }
   }
   ```
2. **镜像 `key in rawData` 语义**：键不存在 → 用默认值；键存在但值为 `null` → 也进分支。Rust 里要区分 `raw.get(key) == None` 与 `Some(Null)`。
3. **IR 用 `Vec<i32>` / `Vec<f64>` / `Vec<u16>`，严格按 `BinaryOffset` 偏移布局。**
4. **JSON 路径的 `floatArray` 是 f64**（JS `number[]`），**dbbin 路径是 `Float32Array`（f32）**——两条路径的黄金数据要分别生成。

**交付物**

- `crates/dragonbones-core/src/parser/`
- IR 比对测试：`assert_ir_eq!(rust_ir, golden_ir)`

**验收标准**

- **9 个素材的 5 组 IR 数组与 JS 逐元素完全相等**（f64 容差 1e-12 或直接 bit 相等）
- 数据版本 5.5 全键通过；其他版本至少能解析不 panic

**常见坑**

- **`Math.round` 是"半数向 +∞"**（`Math.round(-0.5) === -0`），Rust 的 `f64::round()` 是"远离零"（`-0.5 → -1`）。IR 编译里有 10+ 处 `Math.round`（`ObjectDataParser.ts:1242, 1456, 1477, 1482, 1487, 1513, 1544, 1753-1758, 1881`），必须实现 `fn js_round(x: f64) -> f64 { (x + 0.5).floor() }`
- `+value || 0`：JS 里 `+""` 是 0、`+"abc"` 是 NaN、`NaN || 0` 是 0，整体等价于"取数字，失败给 0"
- 蒙皮解析里 `vertexOffset < 0` 时 `+= 65536` 的越界修正（`PixiSlot.ts:191` 也重复出现）
- `Math.floor(rawVertices.length / 2)` 一类的整数下标计算

**工期**：5–8 天

---

### 阶段 3 — armature 层级

**目标**：实现骨架、插槽、约束的数据结构与**姿态求值**（不含动画驱动），做到"给定时间轴数据能算出正确的世界矩阵"。

**任务分解**

| TS 源文件 | 行数 | 说明 |
|---|---|---|
| `armature/TransformObject.ts` | 154 | `global` / `globalTransformMatrix` / `local` / `localMatrix` |
| `armature/Bone.ts` | 530 | 层级更新、脏标记、缓存帧索引、`_updateAlpha` |
| `armature/Surface.ts` | 547 | 网格骨骼，`_getGlobalTransformMatrix(x, y)` 双线性插值 |
| `armature/Slot.ts` | 1203 | DisplayFrame、deformVertices、14 抽象方法改写为 trait |
| `armature/Armature.ts` | 886 | 主循环、槽排序、alpha 传播、action 队列、点/线段相交 |
| `armature/Constraint.ts` | 717 | IK + Path（P1 阶段可先留 stub，但接口先立） |
| `factory/BaseFactory.ts` | 951 | 装配逻辑（`_buildBones` / `_buildSlots` / `_buildConstraints` / `_getSlotDisplay`） |

**关键技术决策**

1. **arena + newtype 索引**（D3）：
   ```rust
   #[derive(Copy, Clone, PartialEq, Eq, Hash)]
   struct BoneIdx(u32);
   #[derive(Copy, Clone, PartialEq, Eq, Hash)]
   struct SlotIdx(u32);

   struct Armature {
       bones: Vec<Bone>,
       slots: Vec<Slot>,
       constraints: Vec<Constraint>,
       // ...
   }
   ```
   `Slot.parent: BoneIdx`、`Bone.armature: ArmatureIdx`、`Armature._parent: Option<SlotIdx>`。禁止 `Rc<RefCell>`。
2. **14 个抽象方法 → Rust trait**：
   ```rust
   trait SlotRenderAdapter {
       fn init_display(&mut self, value: DisplayHandle, retain: bool);
       fn update_display(&mut self);
       fn add_display(&mut self);
       fn replace_display(&mut self, prev: DisplayHandle);
       fn remove_display(&mut self);
       fn update_z_order(&mut self);
       fn update_visible(&mut self);
       fn update_blend_mode(&mut self);
       fn update_color(&mut self);
       fn update_frame(&mut self);
       fn update_mesh(&mut self);
       fn update_transform(&mut self);
       fn identity_transform(&mut self);
       fn dispose_display(&mut self, value: DisplayHandle, release: bool);
   }
   ```
   阶段 3 先提供 `NullAdapter`（对应差分测试环境），阶段 7 换成 Bevy 实现。
3. **更新顺序严格照抄 `Armature.advanceTime`**（`armature/Armature.ts:394-466`）：
   ```
   lock → animation.advanceTime → 槽排序 → alpha 传播
        → bones[i].update(cacheFrameIndex) × N
        → slots[i].update(cacheFrameIndex) × N
        → 执行 action 队列 → 解锁 → proxy.dbUpdate()
   ```
4. **`BaseFactory` 是纯装配逻辑，不放渲染代码**，可直接进 core。

**交付物**

- `crates/dragonbones-core/src/{armature,factory}/`
- `NullAdapter` 实现

**验收标准**

- `buildArmature()` 后调用 `advanceTime(0.0)`，静姿态下所有骨骼的 `global` 矩阵（a/b/c/d/tx/ty）与 JS 差分 ≤ 1e-6
- 5 个含 mesh 素材的顶点数、UV、三角索引与 JS 一致

**常见坑**

- `Bone.update(cacheFrameIndex)` 里有 4 条分支（缓存命中 / 有缓存 / 有约束 / 脏），漏一条会导致增量更新与全量更新结果不同
- `inheritReflection` 影响 `scaleX/scaleY` 的符号传播，容易漏
- 约束的执行时机在 `Bone.update` 内部（`constraint._root === this` 时才更新），不是统一阶段
- 槽排序比较器（`Armature.ts:47`）返回 `1` 或 `-1`（永不返回 0），平局时比较不一致 → 见风险 R5

**工期**：5–8 天

---

### 阶段 4 — 动画系统

**目标**：实现完整的时间轴驱动与动画状态机，这是代码量最大、语义最复杂的阶段。

**任务分解**

| TS 源文件 | 行数 | 内容 |
|---|---|---|
| `animation/WorldClock.ts` | 240 | 时钟、`animatebles` 列表、`timeScale` |
| `animation/IAnimatable.ts` | 76 | trait |
| `animation/Animation.ts` | 887 | 播放入口、多状态管理、blendStates 重置 |
| `animation/AnimationState.ts` | 1445 | 状态机核心（fade / layer / group / weight / boneMask）+ `BlendState` |
| `animation/BaseTimelineState.ts` | 500 | 5 个抽象基类，帧搜索与插值 |
| `animation/TimelineState.ts` | 931 | 16 个具体时间轴 |
| `model/AnimationConfig.ts` | 337 | 播放配置 |
| `model/AnimationData.ts` | 351 | 动画数据 + `TimelineData` |

**任务分解顺序**

1. `WorldClock` + `IAnimatable`（最简单，先打通顶层驱动）
2. `AnimationConfig` + `AnimationData`
3. `BaseTimelineState` 5 个抽象基类（帧二分查找、`TweenType` 6 种缓动、贝塞尔曲线采样 `_getCurvePoint` / `_samplingEasingCurve`）
4. 按依赖顺序实现具体时间轴：
   - 第一批：`ActionTimelineState`（驱动整条时间轴）、`ZOrderTimelineState`
   - 第二批：`BoneAll` / `BoneTranslate` / `BoneRotate` / `BoneScale`
   - 第三批：`SlotDisplay` / `SlotColor` / `SlotZIndex` / `Alpha`（BoneAlpha 与 SlotAlpha 共用）
   - 第四批：`Surface` / `Deform`（mesh 相关）
   - 第五批：`IKConstraint` / `AnimationProgress` / `AnimationWeight` / `AnimationParameters`
5. `AnimationState` 状态机（fade 四态 `_fadeState` / `_subFadeState` / `_playheadState`、layer 权重分配、`_advanceFadeTime`）
6. `Animation` 顶层（`playConfig` / `fadeIn` / `fadeOutMode` 5 种 / `getBlendState`）
7. `BlendState` 权重累加

**关键技术决策**

1. **状态机的子状态用显式枚举**，不要用魔法数字：
   ```rust
   enum FadeState { None = -1, Fading = 0, Faded = 1, Complete = 2 }
   ```
   JS 里大量 `if (this._fadeState > 0 && this._subFadeState > 0)` 这类数字比较，翻译时必须回查 `AnimationState.ts` 中的注释确认语义。
2. **时间轴状态对象**：阶段 4 先用 `enum TimelineStateKind { Action(ActionTimelineState), ... }` + `Vec<TimelineStateKind>`，避免 trait object 的动态分发与借用问题；性能优化留到后期。
3. **`advanceTime` 中的时间归一化**（`BaseTimelineState.ts:120-210` 的 `currentPlayTimes` / `currentTime` 计算）用 `f64`，注意 JS 的 `Math.floor` 对负数的行为（与 Rust `floor` 一致，可直接用）。

**交付物**

- `crates/dragonbones-core/src/animation/`

**验收标准**

- 单动画播放：9 素材 × 全动画 × 120 帧差分通过
- `gotoAndStopByFrame` 往返一致
- `resetToPose` 后姿态与初始姿态一致

**常见坑**

- `_advanceFadeTime` 里会调用 `hasDBEventListener`，事件 trait 必须先就位
- `AnimationState.advanceTime` 的缓存帧路径（`cacheFrameRate > 0`）分支复杂，先让 `cacheFrameRate = 0` 走非缓存路径，缓存留到 P1-4
- `Animation.advanceTime` 对 `passedTime < 0` 取绝对值（只有 animationState 能倒放）
- `blendStates` 是两层字典（type → name → BlendState），每帧开头先 `reset()`
- 5 种 `AnimationFadeOutMode` 的过滤条件各不相同（`SameLayer` / `SameGroup` / `SameLayerAndGroup` / `All` / `Single`）

**工期**：8–12 天

---

### 阶段 5 — 工厂、事件与 P0 收口

**目标**：补齐事件流与顶层入口，完成 P0 全部验收项。

**任务分解**

1. `event/EventObject.ts`（277 行）+ `event/IEventDispatcher.ts`（109 行）
2. `core/DragonBones.ts` 的 `advanceTime`：对象回收 → clock 推进 → 事件派发（frameEvent 派发到 armature，soundEvent 额外派发到全局 eventManager）→ 回收 EventObject
3. `BaseFactory` 剩余方法：`replaceDisplay` / `replaceSlotDisplay(List)` / `replaceSkin` / `replaceAnimation`（P1-5 的一半，可视情况推到 P1）
4. action / defaultActions / 子骨架 `ActionType.Play` 的处理（`Armature.ts` 的 `_actions` 队列）
5. 补齐差分用例矩阵：

| 用例 | 覆盖点 |
|---|---|
| 单动画播放 ×9 素材 | 基线 |
| `fadeOut` + `fadeIn` 交叉 | fade 状态机 |
| 同 layer 叠加两动画 | layer 权重 |
| `gotoAndStopByFrame` 往返 | 时间跳转 |
| `resetToPose` | 归位 |
| 同名动画重播 | `fadeOutMode` |
| 事件密集动画 | 事件时序 |
| `timeScale = 0.5 / 2.0` | 时间缩放 |
| 双骨架同时播放 | WorldClock 多 animatable |

**交付物**

- 完整的 `dragonbones-core`
- 差分测试套件全绿

**验收标准**

- **P0 全部 14 项通过**
- 差分测试 9 素材 × 全动画 × 全用例矩阵全绿

**工期**：3–5 天

---

### 阶段 6 — 完整度收口（P1）

**目标**：把 P1 的 10 项全部补齐，达到"核心运行时 100%"。

**任务分解**

| 子项 | 对应源文件 | 工期 |
|---|---|---|
| dbbin 二进制解析 | `parser/BinaryDataParser.ts` 379 行 | 2–3 天 |
| IK 约束 | `Constraint.ts` 的 `IKConstraint`（67-229 行） | 1–2 天 |
| Path 约束 | `Constraint.ts` 的 `PathConstraint`（230-717 行） | 2–3 天 |
| BoundingBox 三种 + 点/线段相交 | `model/BoundingBoxData.ts` 654 行 + `Armature/Slot` 的相交方法 | 1–2 天 |
| `cacheFrameRate` 帧缓存 | `AnimationState` 的 `cachedFrameIndices` 路径 + `Bone/Slot` 缓存命中 | 1–2 天 |
| 换装 API | `BaseFactory` 的 replace 系列 | 1 天 |
| UserData / ActionData | `model/UserData.ts` 157 行 | 0.5 天 |
| CanvasData 解析 | `model/CanvasData.ts` 46 行 | 0.5 天 |
| IR 二进制写出路径 | `ObjectDataParser.ts:2229-2400` | 1 天 |

**关键技术决策**

- **dbbin 路径的数值是 f32**，黄金数据要单独生成，容差放宽到 `1e-4`
- 二进制解析用 `byteorder` + 显式对齐检查；UTF-8 解码要对照 `BinaryDataParser._decodeUTF8`（自定义实现，非标准 `String.fromCharCode`）

**验收标准**

- P1 全部 10 项通过
- 用 `Pixi/Demos/` 下的 `.dbbin` 素材跑通差分

**工期**：4–6 天

---

### 阶段 7 — Bevy 适配层

**目标**：把已验证的核心接入 Bevy，播放 `assets/` 下的素材动画。

> **前置**：先完成 §5 的预研问题 Q1–Q3，再动手。

**任务分解**

#### 7.1 资产加载（P2-1）

- 两个自定义 `AssetLoader`：
  - `SkeletonAsset` ← `_ske.json`
  - `AtlasAsset` ← `_tex.json`，内部嵌套加载 `_tex.png` → `Image`
- 依赖 `serde_json`，复用 core 的解析器
- 验证中文文件名路径（Q3）

#### 7.2 实体映射（P2-2）

```
ArmatureRoot (实体)
  ├─ DragonBonesArmature { skeleton: Handle<SkeletonAsset>, armature: ArmatureIdx }
  ├─ Transform (骨架世界变换)
  └─ Slot 实体 ×N
       ├─ Mesh2d (quad)
       ├─ MeshMaterial2d<DragonBonesMaterial>
       └─ DragonBonesSlot { slot: SlotIdx }
```

#### 7.3 变换同步（P2-3）— 核心技术决策

**问题**：Bevy 的 `Transform` 是 TRS 分解（translation + rotation + scale），**无法表达剪切（skew）**。而 DragonBones 的 `Transform` 同时有 `skewX/skewY`，当两者不等时就是剪切矩阵。

**推荐方案 A：顶点烘焙**

```
每帧：slot 的局部仿射矩阵 (a,b,c,d,tx,ty) 直接乘进 quad 的 4 个顶点坐标
     实体 Transform 保持 identity
     骨架根实体承担世界变换
```

- 优点：完全绕开 TRS 分解损失，天然支持 skew；每帧重建 4 顶点开销极小
- 缺点：失去 GPU 实例化；实体层级只用于组织，不参与渲染变换
- 这正是 Pixi 适配层的做法的等价形式（`PixiSlot._updateTransformV4` 直接写 display 的矩阵）

**备选方案 B**：直接写 `GlobalTransform` 的 affine 分量，绕过 transform propagation。复杂度高（要处理传播系统的覆盖时机），不推荐首选。

#### 7.4 颜色同步（P2-4）

`ColorTransform`（乘子 + 偏移）烘进**顶点色**：

```
vertexRGBA = texture * multiplier + offset
alpha 还要乘 _globalAlpha
```

优点：免去 material uniform 同步，每 slot 不同色天然支持。

#### 7.5 zOrder 排序（P2-5）

- 方案 1（首选）：正交相机下 `slot 的 GlobalTransform.z = -index * 0.001`，依赖 Bevy 透明队列按 z 排序 → **需先验证 Q1**
- 方案 2：自定义 phase sort key
- 需要覆盖的风险 R5：槽排序比较器平局行为

#### 7.6 blend 模式（P2-6）

用 `Material2d::specialize` 按模式特化管线（需先验证 Q2）：

| 模式 | wgpu BlendState | 可行性 |
|---|---|---|
| Normal | `SrcAlpha / OneMinusSrcAlpha` | ✔ |
| Add | `One / One` | ✔ |
| Multiply | `Src(Dst) / Zero` | ✔ |
| Screen | `One / OneMinusSrc` | ✔ |
| **Overlay** | 固定因子表达不了（分段非双线性） | ✘ 需自定义 shader 或近似 |
| Darken / Lighten / Difference / HardLight / Invert / Erase / Subtract | 部分可表达 | 逐个评估 |

**建议**：P2 先交付 Normal / Add / Multiply / Screen 四种（覆盖 assets 实际用到的 3 种 + Normal），Overlay 单独立项。

#### 7.7 事件与调度（P2-7 / P2-8）

```rust
// 事件
#[derive(Event, Clone)]
pub struct DragonBonesEvent { pub kind: EventKind, pub name: String, ... }

// 调度
SystemSet::DragonBones  // 在 transform propagation 之前推进动画
```

- 时间源：`Res<Time>` 的 `delta_secs()`
- 顶层入口：镜像 `DragonBones.advanceTime`，先 drain 待回收对象，再推进 clock，最后派发事件

#### 7.8 Demo（P2-9）

- 播放 9 个素材，键盘切换动画
- 专门展示：mesh 变形（`Surface_P1_4`）、blend 模式（`Dragon_Blend` / `Rooster_Blend`）

**交付物**

- `crates/dragonbones-bevy/`
- `crates/dragonbones-demo/`

**验收标准**

- 9 个素材全部能播放，动画切换、事件触发正常
- mesh 变形与 blend 效果与浏览器中的 DragonBones 官方 demo 目视一致
- 无系统 panic、无每帧分配尖峰

**常见坑**

- 骨骼/插槽实体数量多时，每帧全量同步会有开销 → 用脏标记只更新 dirty slot
- mesh 顶点缓冲重建要用 `RenderAssetUsages` / `BufferVec` 复用，避免每帧新建 `Mesh`
- Bevy 的 2D 相机是 Y 向上，DragonBones 数据默认 `yDown = true`（`DragonBones.ts:289`）→ 需要一次 Y 轴翻转，**只在根实体做一次，不要每层都翻**

**工期**：7–14 天

---

## 4. 关键移植决策（D1–D7）

| # | 决策 | 理由与做法 |
|---|---|---|
| **D1** | **core 全程 f64，仅渲染边界降 f32** | JS 全程 f64；只有 f64 才能把差分容差收到 1e-9。注意 JSON 路径 `floatArray` 是 f64、dbbin 路径是 f32，两条路径分别建黄金数据 |
| **D2** | **必须镜像的 JS 数值语义**（见下表） | 这是最容易静默出错的地方，IR 层就能拦住大部分 |
| **D3** | **arena + newtype 索引，禁用 `Rc<RefCell>`** | JS 里 Armature↔Slot↔Bone 是环状引用 + 池化复用；`Rc<RefCell>` 有运行时 borrow panic 风险且与池化语义冲突 |
| **D4** | **对象池保留生命周期语义，性能池后置** | `bufferObject()` → 下一帧统一回收是**行为语义**（事件对象在派发后才失效），即使直接 drop 也必须保留这个缓冲队列 |
| **D5** | **解析器不用 serde derive 结构体** | 严格 struct 会因字段缺省 / 字符串数字 / 版本差异大面积失败；用 `Value` + helper 逐行对照翻译 |
| **D6** | **扁平 IR 是地基，必须逐元素对齐** | 先对齐 IR，动画逻辑才有可靠对照基准。这是最重要的分层验收点 |
| **D7** | **每个 TS 文件 → 一个 Rust 模块，命名保持对应** | `_updateDisplayData` → `_update_display_data`，函数顺序不变，便于 diff 走查与搜索定位 |

### D2：必须镜像的 JS 数值语义对照表

| JS 行为 | 出现位置 | Rust 正确写法 |
|---|---|---|
| `Math.round(x)` 半数**向 +∞**（`Math.round(-0.5) === -0`） | `ObjectDataParser.ts:1242, 1456, 1477, 1482, 1487, 1513, 1544, 1753-1758, 1881` | `fn js_round(x: f64) -> f64 { (x + 0.5).floor() }`<br>**不是** `x.round()`（Rust 是远离零，`-0.5` 会差 1） |
| `+value \|\| 0`：`NaN` / `""` / `null` → 0 | `_getNumber` `ObjectDataParser.ts:66-77` | 自定义 `js_to_number` |
| `value === "NaN"`、`value === null` → default | 同上 | 逐分支照抄 |
| `_getBoolean` 字符串真值表：`"0" "NaN" "" "false" "null" "undefined"` → false | `ObjectDataParser.ts:36-64` | 逐分支照抄 |
| `key in rawData`：键不存在才用默认值，键存在值为 null 也进分支 | 全解析器 | 区分 `get(key) == None` 与 `Some(Null)` |
| `instanceof Array` 类型分派 | `ObjectDataParser.ts:1897` | `Value::is_array()` |
| 槽排序比较器返回 `1` 或 `-1`（**永 not 0，平局不一致**） | `Armature.ts:47` | 需实测 V8 TimSort 结果，用 zOrder 用例覆盖 |
| `Math.floor` 对负数 = 向下取整 | 多处 | 与 Rust `floor` 一致，可直接用 |
| `typeof value === "string"` 分派 | `ObjectDataParser.ts:39, 82, 291, 476, 602, 671, 797, 1906` | `Value::is_string()` |

---

## 5. 风险登记册与待验证问题

### 5.1 风险登记册

| ID | 风险 | 等级 | 对策 |
|---|---|---|---|
| **R1** | Overlay（及 Darken / HardLight / Difference 等）blend 模式无法用 wgpu 固定混合因子表达（分段非双线性函数） | 中 | P2 先交付 Normal / Add / Multiply / Screen（覆盖 assets 实际用到的 3 种）；Overlay 走自定义 shader 路线单独立项 |
| **R2** | Bevy `Transform` 是 TRS 分解，**无法表达 skew 剪切** | 高 | 阶段 7.3 方案 A（顶点烘焙），第一周先做技术验证 |
| **R3** | Bevy 0.19.1 透明 2D 队列排序语义未确认 | 中 | Q1 实测；备选自定义 sort key |
| **R4** | JS 数值语义（`Math.round` 半数方向等）静默偏差 | 高 | D2 对照表 + IR 逐元素断言（阶段 2 就能拦住） |
| **R5** | 槽位排序比较器平局时返回 `-1`（不一致），V8 TimSort 与 Rust 排序结果可能不同 | 中 | zOrder 时间轴专项用例；必要时实现为"按 (zIndex*1000+zOrder, 原始索引)"的全序比较 |
| **R6** | `AnimationState` 的 `cacheFrameRate` 缓存路径复杂（`cachedFrameIndices`） | 中 | 用 `cacheFrameRate` 素材专项覆盖（P1-4） |
| **R7** | 非 ASCII 资源文件名（中文名素材） | 低 | 阶段 0 就验证 Bevy 资源加载 |
| **R8** | Bevy 0.20 RC → 正式版 API 变动 | 低 | core 与 bevy 层完全解耦，迁移只动 `dragonbones-bevy` |

### 5.2 待验证问题

| ID | 问题 | 影响阶段 | 验证方法 |
|---|---|---|---|
| **Q1** | Bevy 0.19.1 `Transparent2d` 的排序键是否包含 z？ | 阶段 7.5（R3） | 写最小 demo：两个同位置不同 z 的半透明 quad，看遮挡关系 |
| **Q2** | `Material2d::specialize` 是否允许 per-material 覆盖 `BlendState`？ | 阶段 7.6（R1） | 读 `bevy_sprite` 源码 `Material2dPipeline::specialize` |
| **Q3** | Bevy 0.19.1 资源加载对中文路径的支持 | 阶段 0 / 7.1（R7） | `asset_server.load("中文名_tex.png")` 最小测试 |
| **Q4** | 通读 `DragonBonesJS/docs/DragonBones_5.0_data_format_zh.md` | 阶段 2 | 数据格式权威文档，解析前必读 |

---

## 6. 资料索引

| 用途 | 位置 |
|---|---|
| 数据格式规范（权威） | `DragonBonesJS/docs/DragonBones_5.0_data_format_zh.md` |
| 全部枚举定义 | `DragonBones/src/dragonBones/core/DragonBones.ts:27-277` |
| `BinaryOffset` IR 偏移定义 | 同上 `DragonBones.ts:27-55` |
| 宽松解析 helper | `parser/ObjectDataParser.ts:36-92` |
| 数据键常量全表 | `parser/DataParser.ts`（`protected static readonly` 约 130 个） |
| 主循环与更新顺序 | `armature/Armature.ts` 的 `advanceTime()`（动画 → 排序 → alpha → 骨/槽更新 → action → `dbUpdate`） |
| 对象池与缓冲回收 | `core/BaseObject.ts`、`core/DragonBones.ts:304-344` |
| Slot 14 抽象方法 | `armature/Slot.ts` |
| Factory 3 抽象方法 | `factory/BaseFactory.ts` |
| 适配层接口 | `armature/IArmatureProxy.ts`、`event/IEventDispatcher.ts` |
| 渲染参考实现（4 文件 607 行） | `Pixi/8.x/src/dragonBones/pixi/` |
| JS 参考运行时（可 Node require） | `Pixi/Demos/8.x/libs/dragonBones/dragonBones.js` |
| JS 素材（含 `.dbbin`） | `Pixi/Demos/8.x/resources/` |
| Bevy API | <https://docs.rs/bevy/0.19.1>：`bevy::mesh::Mesh2d`、`bevy::sprite_render::Material2d` |

### 6.1 TS 源文件 → Rust 模块映射表

| TS 文件 | 行数 | Rust 模块 |
|---|---|---|
| `core/BaseObject.ts` | 178 | `core/base_object.rs` |
| `core/DragonBones.ts` | 375 | `core/enums.rs` + `core/dragon_bones.rs` |
| `geom/Matrix.ts` | 333 | `geom/matrix.rs` |
| `geom/Transform.ts` | 248 | `geom/transform.rs` |
| `geom/Point.ts` | 93 | `geom/point.rs` |
| `geom/Rectangle.ts` | 124 | `geom/rectangle.rs` |
| `geom/ColorTransform.ts` | 64 | `geom/color.rs` |
| `event/EventObject.ts` | 277 | `event/event_object.rs` |
| `event/IEventDispatcher.ts` | 109 | `event/dispatcher.rs` |
| `factory/BaseFactory.ts` | 951 | `factory/base_factory.rs` |
| `parser/DataParser.ts` | 309 | `parser/data_parser.rs` |
| `parser/ObjectDataParser.ts` | 2436 | `parser/object_data_parser.rs` |
| `parser/BinaryDataParser.ts` | 379 | `parser/binary_data_parser.rs` |
| `armature/Armature.ts` | 886 | `armature/armature.rs` |
| `armature/Bone.ts` | 530 | `armature/bone.rs` |
| `armature/Surface.ts` | 547 | `armature/surface.rs` |
| `armature/Slot.ts` | 1203 | `armature/slot.rs` |
| `armature/Constraint.ts` | 717 | `armature/constraint.rs` |
| `armature/TransformObject.ts` | 154 | `armature/transform_object.rs` |
| `armature/IArmatureProxy.ts` | 93 | `armature/i_armature_proxy.rs` |
| `animation/Animation.ts` | 887 | `animation/player.rs` |
| `animation/AnimationState.ts` | 1445 | `animation/animation_state.rs` |
| `animation/BaseTimelineState.ts` | 500 | `animation/timeline_state.rs`（基类时间/帧/tween 算法） |
| `animation/TimelineState.ts` | 931 | `animation/timeline_state.rs`（共享类型）+ `animation/timeline_states/*`（16 个具体轴） |
| `animation/WorldClock.ts` | 240 | `animation/world_clock.rs` |
| `animation/IAnimatable.ts` | 76 | `animation/world_clock.rs`（`AnimatableHandle`；无独立 trait 模块） |
| `model/AnimationConfig.ts` | 337 | `model/animation_config.rs` |
| `model/AnimationData.ts` | 351 | `model/animation_data.rs` |
| `model/ArmatureData.ts` | 654 | `model/armature_data.rs` |
| `model/BoundingBoxData.ts` | 654 | `model/bounding_box.rs` |
| `model/CanvasData.ts` | 46 | `model/canvas_data.rs` |
| `model/ConstraintData.ts` | 96 | `model/constraint_data.rs` |
| `model/DisplayData.ts` | 188 | `model/display_data.rs` |
| `model/DragonBonesData.ts` | 199 | `model/dragon_bones_data.rs` |
| `model/SkinData.ts` | 108 | `model/skin_data.rs` |
| `model/TextureAtlasData.ts` | 168 | `model/texture_atlas_data.rs` |
| `model/UserData.ts` | 157 | `model/user_data.rs` |

---

## 7. 里程碑总表

| 里程碑 | 阶段 | 内容 | 工期（专职） | 验收 |
|---|---|---|---|---|
| **M0** | 阶段 0 | workspace + `jsdump.mjs` + 黄金数据 + CI | 0.5 天 | 9 素材全部转储成功 |
| **M1** | 阶段 1 | geom + core + model | 3–5 天 | 单元测试 + 枚举核对表 |
| **M2** | 阶段 2 | 解析器 → IR | 5–8 天 | **IR 逐元素相等**（9 素材） |
| **M3** | 阶段 3 | armature 层级 | 5–8 天 | 静姿态矩阵差分 ≤ 1e-6 |
| **M4** | 阶段 4 | 动画系统 | 8–12 天 | 单动画逐帧差分 |
| **M5** | 阶段 5 | 工厂 + 事件 + P0 收口 | 3–5 天 | **P0 全绿（14 项）** |
| **M6** | 阶段 6 | 完整度 P1 | 4–6 天 | **P1 全绿（10 项）** |
| **M7** | 阶段 7 | Bevy 适配 + demo | 7–14 天 | **P2 全绿（10 项）** |
| | | **合计** | **36–59 天** | |

\* 专职估算；兼职约 ×2。**M2（解析器）与 M4（动画系统）是风险集中区**，建议各预留 50% buffer。

### 建议的第一步

在写任何 Rust 业务代码之前，先做**阶段 0** —— 把 `tools/jsdump.mjs` 完整写出来，跑通 9 个素材的黄金数据转储。这一步同时完成三件事：

1. 验证参考环境确实可用（本调研已预验证，剩协议定义）
2. 把"100%"从口号变成可执行的验收标准
3. 逼自己先看透数据结构与更新顺序
