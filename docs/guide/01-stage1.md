# 阶段 1 实现指南 — geom + core + model

> 配套 `docs/requirements.md` §3 阶段 1。按任务 1→7 顺序实现，跑完「验收清单」全绿后告诉我，我写阶段 2（解析器）指南。
> 本指南所有"实测"结论都是我对 5.7 源码 / bundle 实际验证过的。

---

## 0. 已实测事实与 requirements 修正（先读）

| # | 事实 | 影响 |
|---|---|---|
| F1.1 | **`BinaryOffset` 在 TS 里五组共用一个枚举、判别值大量重复**（WeigthBoneCount=0、GeometryVertexCount=0、TimelineScale=0、FramePosition=0、DeformVertexOffset=0 …）。我实测：直接翻成一个 Rust enum 编译报 **E0081 discriminant value assigned more than once** | 必须拆成 5 组模块常量（任务 3 给方案），P0-3 的"BinaryOffset 数值一一对应"以拆组方式满足 |
| F1.2 | **5.7 的 `ColorTransform` 只有 `copyFrom` / `identity`，没有"组合"方法**（全文件 67 行）。乘子/偏移的真正应用在 Slot 颜色更新（阶段 3）和 `TimelineState.ts:775` 的 fade 插值（阶段 4） | P0-1 里"ColorTransform 组合"的单测改为：字段默认值 + identity + copyFrom；组合语义留到阶段 3/4 差分覆盖 |
| F1.3 | `Transform` 字段是 `x,y,skew,rotation,scaleX,scaleY`：**`rotation` = skewY，`skew` = skewX − skewY**（`fromMatrix` 末行 `this.skew = skewX - this.rotation`）。无 skewX/skewY 字段、无 `updateTransform` 方法 | JSON 的 skX/skY 换算在 `ObjectDataParser.ts:2045-2047`：`rotation = skY`，`skew = skX − skY`（阶段 2 用；阶段 1 测试数据按此语义） |
| F1.4 | `AnimationConfig._onClear` 默认值反直觉：**`timeScale = -100`**（不是 -1）、`fadeOutMode = All(4)`、`pauseFadeIn = pauseFadeOut = true`、`fadeOutTime = -1`、`playTimes = -1`、`duration = -1` | `Default` 必须逐行照 `_onClear` 抄，不能按直觉写（任务 7 有对照测试） |
| F1.5 | `ArmatureData._onClear` 里 bones/slots/skins 的清空被**注释掉**（`// this.bones.clear();`），是 JS 池复用的遗留怪癖 | Rust 用拥有树 + Default 自然规避；不要"忠实复刻"这个注释行为 |
| F1.6 | JS 对"整数样键"（如 `"0"`、`"123"`）的 `for..in` 遍历排在字符串键之前且按数值序；`IndexMap` 是纯插入序 | 9 个素材的骨骼/插槽名均非整数样，实际无差异；登记为已知差异 |
| F1.7 | geom 差分向量我已生成：**`testdata/geom/vectors.json`（704 用例 / 224KB）**，生成器见任务 2（已实测） | 阶段 1 geom 验收 = Rust 跑这 704 个用例对齐 JS 输出 |
| F1.8 | `Matrix` 只有 `concat`（无 append/prepend）；`invert` 有 `b==0&&c==0` 快路径和奇异矩阵怪分支（`a==0\|\|d==0` 时只清 a/b/tx/ty **不清 d**） | 逐行翻译，禁止"数学习惯重写"（任务 1 坑位清单） |

**colorArray 类型**：TS 是 `Int16Array \| Uint16Array` 联合（JSON 路径实为普通 number 数组）。Rust 统一存 `Vec<i32>`（i16/u16 值域均无损），对照表标注。

---

## 任务 1 — geom（5 个文件）

**目标**：与 TS 逐行等价的底层数学，f64 全程。**不用 glam**（D1 决策）。

```
crates/dragonbones-core/src/geom/
├── mod.rs
├── matrix.rs      ← geom/Matrix.ts (355 行)
├── transform.rs   ← geom/Transform.ts (265 行)
├── point.rs       ← geom/Point.ts (93 行)
├── rectangle.rs   ← geom/Rectangle.ts (124 行)
└── color.rs       ← geom/ColorTransform.ts (67 行)
```

**字段与方法清单**（snake_case，函数顺序照 TS）：

```rust
// point.rs: Point { x: f64, y: f64 } — new(x,y) / copy_from / clear
// rectangle.rs: Rectangle { x, y, width, height: f64 } — new / copy_from / clear
// color.rs: ColorTransform { alpha_multiplier, red_multiplier, green_multiplier,
//           blue_multiplier, alpha_offset, red_offset, green_offset, blue_offset: f64 }
//           new(默认 1.0 乘 / 0 偏移) / copy_from / identity   ← 无组合方法，见 F1.2
// matrix.rs: Matrix { a, b, c, d, tx, ty: f64 }
//   identity / copy_from / copy_from_array(&[f64], offset)
//   concat(&mut self, &Matrix) / invert(&mut self)
//   transform_point(&self, x, y, delta: bool) -> (f64, f64)
//   transform_rectangle(&self, rect: &mut Rectangle, delta: bool)
// transform.rs: Transform { x, y, skew, rotation, scaleX, scaleY: f64 }
//   new / copy_from / identity / add / minus / from_matrix / to_matrix
//   + 静态 fn normalize_radian(v: f64) -> f64
```

**签名决策**（TS out-param → Rust 返回值，语义不变）：
- `transformPoint(x, y, result, delta)` → `fn transform_point(&self, x, y, delta) -> (f64, f64)`
- `transformRectangle(rectangle, delta)` → `fn transform_rectangle(&self, rectangle: &mut Rectangle, delta)`（原地改写，与 TS 一致）
- `concat` / `invert` 是改写 self 的链式方法 → `&mut self -> &mut Self`

**必须逐行翻译的坑（我逐段核对过 TS 源码）**：

1. **`concat` 的两个 `if` 分支改变了浮点加法顺序**——它们不是纯性能优化，分支决定了加法发生与否及顺序。"展开成通用公式"会得到 bit 级不同结果。照抄：
   ```
   aA = this.a*value.a; dA = this.d*value.d; txA = this.tx*value.a + value.tx; ...
   if (this.b != 0 || this.c != 0) { aA += this.b*value.c; bA += this.b*value.d; ... }
   if (value.b != 0 || value.c != 0) { bA += this.a*value.b; ...; txA += this.ty*value.c; ... }
   ```
2. **`invert` 三条路径**：`b==0&&c==0` 快路径（其中 `a==0||d==0` 时只清 a/b/tx/ty **不清 d** 的怪行为，照抄）→ `det==0` 返回单位阵 → 通用路径（注意 `k = dA*det` 先存后用的顺序）。
3. **`toMatrix` 分支**：`rotation==0` → a=1,b=0，否则 a=cos,b=sin；`skew==0` → c=−b,d=a，否则 c=−sin(skew+rotation), d=cos(skew+rotation)；scale 只在 `!=1.0` 时才乘。**不是**"标准旋转矩阵"写法。
4. **`fromMatrix`**：用 `Math.atan(b/a)`（**不是 atan2**；a=0 时 JS 得 atan(±∞)=±π/2，Rust `b/0.0` 同样得 ±inf → atan 一致）；rotation 是否在 (−π/4, π/4) 决定 scaleX 取 a/cos 还是 b/sin；`backupScaleX/Y` 符号修正两段 if；末行 `skew = skewX − rotation`。
5. **`normalize_radian`**：`(value + PI) % (2PI)`（JS `%` 是截断余数，Rust `f64 %` 语义相同）→ `value += if value > 0 { -PI } else { PI }`（严格 `> 0.0`）。
6. `transformRectangle` 的 `Math.floor/ceil` → `f64::floor()/ceil()`（语义一致），四角交换排序的 tmp 逻辑照抄。

---

## 任务 2 — geom 差分基建（向量已生成，生成器给你）

**已完成（我实测跑通）**：`testdata/geom/vectors.json` 已在仓库，704 用例：`concat=69 invert=71 transformPoint=60 transformRectangle=240 toMatrix=68 fromMatrix=137 addMinus=40 normalizeRadian=19`。

**生成器**（种子固定 `20261003`，输出确定；入库后不要随意重生成）——落盘为 `tools/geovectors.mjs`：

```js
// tools/geovectors.mjs — 用 JS 参考运行时生成 geom 差分向量 → testdata/geom/vectors.json
import { createRequire } from "node:module";
import fs from "node:fs"; import path from "node:path";
const require = createRequire(import.meta.url);
globalThis.PIXI = { Container: class {}, Texture: class {}, Rectangle: class {},
                    Sprite: class {}, Mesh: class {}, BLEND_MODES: {} };
const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

let seed = 20261003;                                        // 可复现 LCG
const rnd  = () => { seed = (seed*1103515245+12345)&0x7fffffff; return seed/0x7fffffff*4-2; };    // [-2,2)
const rndT = () => { seed = (seed*1103515245+12345)&0x7fffffff; return seed/0x7fffffff*200-100; }; // [-100,100)

const M = (a,b,c,d,tx,ty) => [a,b,c,d,tx,ty];
const T = (x,y,skew,rot,sx,sy) => [x,y,skew,rot,sx,sy];
const mkM = v => new db.Matrix(...v), outM = m => [m.a,m.b,m.c,m.d,m.tx,m.ty];
const mkT = v => new db.Transform(...v), outT = t => [t.x,t.y,t.skew,t.rotation,t.scaleX,t.scaleY];

const out = { concat:[], invert:[], transformPoint:[], transformRectangle:[],
              toMatrix:[], fromMatrix:[], addMinus:[], normalizeRadian:[] };

// 边界矩阵（identity/纯平移/90°/纯缩放/含 b,c/全零/仅 b/负缩放/极小缩放）+ 60 个随机矩阵
const mats = [M(1,0,0,1,0,0), M(1,0,0,1,10,-20), M(0,1,-1,0,5,5), M(2,0,0,3,0,0),
              M(1,0.5,-0.5,1,7,9), M(0,0,0,0,1,1), M(0,1,0,0,0,0), M(-1,0,0,-1,3,4),
              M(0.001,0,0,0.001,0,0)];
for (let i=0;i<60;i++) mats.push(M(rnd(),rnd(),rnd(),rnd(),rndT(),rndT()));

for (let i=0;i<mats.length;i++) {                          // concat: 有序对
  const b = mats[(i*7+3)%mats.length], a = mkM(mats[i]);
  out.concat.push({ a: mats[i], b, expect: (a.concat(mkM(b)), outM(a)) });
}
for (const v of [...mats, M(1e-15,2,3,4,5,6), M(0,0,0,1,0,0)]) {  // invert: 含奇异/半奇异
  const m = mkM(v); m.invert(); out.invert.push({ a: v, expect: outM(m) });
}
for (let i=0;i<30;i++) {                                   // transformPoint: delta 双分支
  const v = mats[i%mats.length];
  for (const delta of [false,true]) {
    const x = rndT()/10, y = rndT()/10, r = {x:0,y:0};
    mkM(v).transformPoint(x,y,r,delta);
    out.transformPoint.push({ m:v, x, y, delta, expect:[r.x,r.y] });
  }
}
const rects = [[0,0,100,50],[-10,-10,20,5],[3.7,-2.3,15.5,8.1],[0,0,1,1]];
for (let i=0;i<30;i++) for (const r0 of rects) for (const delta of [false,true]) {
  const rect = {x:r0[0],y:r0[1],width:r0[2],height:r0[3]};
  mkM(mats[i%mats.length]).transformRectangle(rect,delta);
  out.transformRectangle.push({ m:mats[i%mats.length], r:r0, delta,
                                expect:[rect.x,rect.y,rect.width,rect.height] });
}
// toMatrix: 覆盖 rotation==0 / skew==0 / 负缩放 / 极小 skew 分支
const tans = [T(0,0,0,0,1,1), T(1,2,0,0,2,3), T(1,2,0.3,0,1,1), T(1,2,0,0.5,1,1),
              T(1,2,0.3,0.5,2,3), T(1,2,-1.2,-0.7,-2,-3), T(0,0,Math.PI,0,1,1),
              T(5,-5,0.001,-0.001,0.5,1.5)];
for (let i=0;i<60;i++) tans.push(T(rndT()/10,rndT()/10,rnd()*Math.PI,rnd()*Math.PI,rnd(),rnd()));
for (const v of tans) { const m=new db.Matrix(); mkT(v).toMatrix(m);
  out.toMatrix.push({ t:v, expect: outM(m) }); }
// fromMatrix: 覆盖 backupScale 符号修正分支（输入含随机矩阵与 toMatrix 产物）
for (const v of [...mats.map(outM), ...out.toMatrix.map(x=>x.expect)]) {
  const t = new db.Transform(); t.fromMatrix(mkM(v));
  out.fromMatrix.push({ m:v, expect: outT(t) });
}
for (let i=0;i<40;i++) {                                   // add / minus
  const a = T(rndT()/10,rndT()/10,rnd(),rnd(),rnd(),rnd());
  const b = T(rndT()/10,rndT()/10,rnd(),rnd(),rnd(),rnd());
  const ta = mkT(a); ta.add(mkT(b));
  const tb = mkT(a); tb.minus(mkT(b));
  out.addMinus.push({ a, b, expectAdd: outT(ta), expectMinus: outT(tb) });
}
// normalizeRadian: 正负大数 / ±π / 越界 / 0
for (const x of [-100,-20,-2*Math.PI,-Math.PI-0.1,-Math.PI,-3,-1,-0.5,-0.0001,0,
                 0.0001,0.5,1,3,Math.PI-0.1,Math.PI,2*Math.PI,20,100])
  out.normalizeRadian.push({ x, expect: db.Transform.normalizeRadian(x) });

const p = "testdata/geom/vectors.json";
fs.mkdirSync(path.dirname(p), {recursive:true});
fs.writeFileSync(p, JSON.stringify(out, null, 1));
console.log("cases:", Object.entries(out).map(([k,v])=>k+"="+v.length).join(" "));
```

**Rust 侧测试**（`crates/dragonbones-core/tests/geom_golden.rs`）伪代码：

```rust
// 读 ../../testdata/geom/vectors.json（serde_json），8 类用例逐个执行：
//   concat:     let mut m = Matrix::from_arr(&c.a); m.concat(&Matrix::from_arr(&c.b));
//               assert_close(&m.to_arr(), &c.expect)
//   invert / toMatrix / fromMatrix / addMinus / normalizeRadian 同理（改写 self 后取回数组）
//   transformPoint:  m.transform_point(x, y, delta) 对比 expect
//   transformRectangle: 原地改 Rectangle，对比 [x,y,width,height]
// 断言：|got − want| ≤ 1e-9（JS 与 Rust 同为 f64、同表达式序，实际应 bit 相等；
//       transformRectangle 期望值是 floor/ceil 后的整数，自然全等）
// 每类用例单独一个 #[test]，失败信息带用例序号 + 输入数组。
// 辅助：fn from_arr(v: &[f64]) -> Matrix; fn assert_close(got: &[f64], want: &[f64], ctx: &str)
```

---

## 任务 3 — `core/enums.rs`（16 个枚举 + BinaryOffset 拆组）

**源**：`core/DragonBones.ts:27-277`。全部 `#[repr(u32)]` + **显式判别值**（JSON 数字直接互转，阶段 2 要用）。

**`BinaryOffset` 拆组方案（F1.1，E0081 的唯一解）**——它是数组下标常量，用模块常量最顺手：

```rust
pub mod binary_offset {
    pub mod weight { pub const BONE_COUNT: usize = 0; pub const FLOAT_OFFSET: usize = 1; pub const BONE_INDICES: usize = 2; }
    pub mod geometry { pub const VERTEX_COUNT: usize = 0; pub const TRIANGLE_COUNT: usize = 1;
                       pub const FLOAT_OFFSET: usize = 2; pub const WEIGHT_OFFSET: usize = 3; pub const VERTEX_INDICES: usize = 4; }
    pub mod timeline { pub const SCALE: usize = 0; pub const OFFSET: usize = 1; pub const KEY_FRAME_COUNT: usize = 2;
                       pub const FRAME_VALUE_COUNT: usize = 3; pub const FRAME_VALUE_OFFSET: usize = 4; pub const FRAME_OFFSET: usize = 5; }
    pub mod frame { pub const POSITION: usize = 0; pub const TWEEN_TYPE: usize = 1;
                    pub const TWEEN_EASING_OR_CURVE_SAMPLE_COUNT: usize = 2; pub const CURVE_SAMPLES: usize = 3; }
    pub mod deform { pub const VERTEX_OFFSET: usize = 0; pub const COUNT: usize = 1;
                     pub const VALUE_COUNT: usize = 2; pub const VALUE_OFFSET: usize = 3; pub const FLOAT_OFFSET: usize = 4; }
}
```

**其余 16 个枚举数值表（照抄进代码，同时照抄进测试断言）**：

```rust
// ArmatureType:      Armature=0 MovieClip=1 Stage=2
// BoneType:          Bone=0 Surface=1
// DisplayType:       Image=0 Armature=1 Mesh=2 BoundingBox=3 Path=4
// BoundingBoxType:   Rectangle=0 Ellipse=1 Polygon=2
// ActionType:        Play=0 Frame=10 Sound=11            ← 不连续！
// BlendMode:         Normal=0 Add=1 Alpha=2 Darken=3 Difference=4 Erase=5 HardLight=6
//                     Invert=7 Layer=8 Lighten=9 Multiply=10 Overlay=11 Screen=12 Subtract=13  ← 14 种
// TweenType:         None=0 Line=1 Curve=2 QuadIn=3 QuadOut=4 QuadInOut=5
// TimelineType:      Action=0 ZOrder=1 BoneAll=10 BoneTranslate=11 BoneRotate=12 BoneScale=13
//                     SlotDisplay=20 SlotColor=21 SlotDeform=22 SlotZIndex=23 SlotAlpha=24
//                     IKConstraint=30 AnimationProgress=40 AnimationWeight=41 AnimationParameter=42
//                     Surface=50 BoneAlpha=60                                  ← 大量不连续！
// OffsetMode:        None=0 Additive=1 Override=2                            ← 无显式值，隐式 0 起
// AnimationFadeOutMode: SameLayer=1 SameGroup=2 SameLayerAndGroup=3 All=4 Single=5  ← 从 1 起！
// AnimationBlendType:   None=0 E1D=1          AnimationBlendMode: Additive=0 Override=1
// ConstraintType:    IK=0 Path=1             PositionMode: Fixed=0 Percent=1
// SpacingMode:       Length=0 Fixed=1 Percent=2          RotateMode: Tangent=0 Chain=1 ChainScale=2
```

**测试**（`tests/enums.rs`）：每个枚举写 `assert_eq!(BlendMode::Overlay as u32, 11)` 式穷举断言（16 枚举全成员 + BinaryOffset 五组常量），注释标 TS 行号。这份测试同时是"枚举核对表"（P0-3 验收物，誊一份进 `docs/`）。

**顺带**（`core/dragon_bones.rs`，只做静态部分）：

```rust
pub struct DragonBones;   // 纯命名空间：VERSION = "5.7.000", yDown = true, debug / debugDraw
// DragonBones 实例类（_clock/_events/_objects/advanceTime/bufferEvent）依赖 WorldClock(阶段4)
// 和 EventObject(阶段5)，本阶段不实现——阶段 5 收口（P0-12）。
```

---

## 任务 4 — `core/base_object.rs`（对象池语义，P0-2）

**TS 行为精确对照**（`BaseObject.ts`，逐行核对过）：

| JS 行为 | Rust 语义 |
|---|---|
| `borrowObject`：池有 → pop，**不再** `_onClear`（return 时已清） | `pool.pop()` 直接返回 |
| `borrowObject`：池空 → `new` + 调一次 `_onClear` | `T::default()` + `on_clear()` 一次 |
| `returnToPool`：先 `_onClear` 再入池 | 入池前 `on_clear()` |
| `_isInPool` 重复入池 → console.warn | **move 语义天然杜绝**，无需字段（对照表标"编译期消除"） |
| `hashCode` 构造时分配、池复用不重发 | `hash_code: u64`，静态原子计数器，`on_clear` 不动它 → 测试锚点 |
| `setMaxCount` / `clearPool` 按 `String(constructor)` 分键 | 按 `TypeId` 分键 |
| `bufferObject`：暂存，**下一次** `advanceTime` 统一 `returnToPool` | 独立缓冲队列，`drain()` 才回池（D4：行为语义必须保留） |

**伪代码**：

```rust
pub trait Poolable: Any {
    fn on_clear(&mut self);                          // 对应 _onClear，重置为 TS 默认值
}

// thread_local 池：HashMap<TypeId, Vec<Box<dyn Poolable>>>，单线程语义（core 无并发）
fn pool_borrow<T: Poolable + Default + 'static>() -> T {
    if let Some(b) = pop(TypeId::of::<T>()) { b.downcast::<T>().unwrap() }  // 已在 return 时清过
    else { let mut o = T::default(); o.on_clear(); o }                      // 新建：default + 一次 clear
}
fn pool_return<T: Poolable + 'static>(mut obj: T) { obj.on_clear(); push(Box::new(obj)) }
fn pool_clear::<T>() / fn pool_set_max_count::<T>(n)                       // 对应 clearPool/setMaxCount

// 对应 JS 调用习惯：obj.return_to_pool() 消费 self（不可能重复归还）
pub trait PoolExt: Poolable + Sized + 'static { fn return_to_pool(self) { pool_return(self) } }
impl<T: Poolable + 'static> PoolExt for T {}

// bufferObject 缓冲队列（D4）——阶段 5 由 DragonBones.advanceTime 调 drain
pub struct ObjectBuffer(Vec<Box<dyn Poolable>>);
impl ObjectBuffer {
    fn push(&mut self, obj: impl Poolable) { /* indexOf 语义：重复 push 忽略 */ }
    fn drain(&mut self) { /* 逐个 pool_return */ }
}
```

**约束**：`Poolable` 只给**运行时/事件对象**（阶段 3+ 的 Armature/Bone/Slot、阶段 5 的 EventObject）。model 数据结构本阶段直接 `Default`（拥有树，不进池——JS 池化数据对象对输出无影响，见 F1.5；对照表标注该偏差）。

**测试**（`tests/pool.rs`）：
1. borrow → 改字段 → `return_to_pool()` → 再 borrow → **`hash_code` 相同**（证明复用）且字段已清。
2. 新建对象与池对象 `hash_code` 不同。
3. `ObjectBuffer`：push 后 drain 前该实例不在池中，drain 后能借到且状态已清；同一对象 push 两次只入一次。
4. 用 `struct Dummy { v: i32 }` 实现 `Poolable`（`on_clear` 归 0）当测试载体。

---

## 任务 5 — `model/`：11 个 TS 文件 → 11 个 Rust 模块

**核心设计决策 D-1.1（反向引用表示）**——model 是拥有树，TS 的对象引用字段在 Rust 里按下表替换。**这是阶段 1 最重要的决定**，阶段 2 解析器、阶段 3–4 运行时都建在它上面（我对全库 `.parent` 用法逐个 grep 核实）：

```rust
// 所有权（无环，全程拥有）：
// DragonBonesData ──owns──> ArmatureData ──owns──> {BoneData, SlotData, SkinData, AnimationData, ConstraintData}
// SkinData ──owns──> DisplayData    AnimationData ──owns──> TimelineData    ...
// 反向/横向引用（TS 存对象指针的字段）→ 三类替换：根共享 / 名字 / 直接拥有（下表）
```

| TS 字段（类型） | Rust 表示 | 依据（TS 实际用途） |
|---|---|---|
| `ArmatureData.parent: DragonBonesData` | `parent: Arc<RootInfo>` | 只读 `.name` / `.intArray` / null 判断（Armature:306, Animation:342, Constraint:281, Surface:91） |
| `GeometryData.data: DragonBonesData` | `root: Arc<RootInfo>` | 只读 `intArray`（vertexCount/triangleCount getter、Surface:451） |
| `BoneData.parent: BoneData\|null` | `parent: Option<String>`（骨名） | 只取 `.name` / null 判断（Bone:354）；骨名在 armature 内唯一 |
| `SlotData.parent: BoneData` | `parent: String` | 只取 `.name`（Slot:656） |
| `ConstraintData.target/root/bone`、`PathConstraintData.bones/pathSlot`、`WeightData.bones` | `String` / `Option<String>` / `Vec<String>` | sortBones 身份比较 = 名字相等（名唯一）；运行时本来就按名解析 |
| `SkinData.parent: ArmatureData` | `parent: String`（armature 名） | 只读 `.defaultSkin`（BaseFactory:862），经名字表解析 |
| `DisplayData.parent: SkinData` | `parent: String`（skin 名） | 链式取名 `.parent.parent.parent.name`（BaseFactory:259/971） |
| `AnimationData.parent: ArmatureData` | `parent: String`（armature 名） | **只出现 `.parent.parent`（→root）**（AnimationState:564, BaseTimelineState:181, TimelineState ×10）；阶段 4 经运行时 armature 句柄拿 root |
| `TextureData.parent: TextureAtlasData` | `parent: String`（图集名） | 只读 `.scale`（Slot:412），阶段 3 经 factory 解析 |
| `ArmatureDisplayData.armature`、`ArmatureData.stage/defaultSkin/defaultAnimation`、`ActionData.bone/slot`、`PathConstraintData.pathDisplayData` | `Option<String>` / `String`（名字） | 运行时均按名查找（BaseFactory.getArmatureData 等） |
| 向下引用：`ArmatureDisplayData.actions`、`SkinData.displays`、`GeometryData.weight`、`BoundingBoxDisplayData.boundingBox`、`ActionData.data`、`ArmatureData.canvas/userData` | **直接拥有**：`Vec<ActionData>` / `IndexMap<String, Vec<Option<DisplayData>>>` / `Option<Box<WeightData>>` / `Option<Box<BoundingBoxData>>` … | 拥有者容器就在旁边 |
| `AnimationData.actionTimeline/zOrderTimeline`、`boneTimelines` 等 | 拥有：`Option<Box<TimelineData>>` + `IndexMap<String, Vec<TimelineData>>` | 动画拥有自己的时间轴 |

**根共享定义**（`model/dragon_bones_data.rs`，阶段 2/3/4 的地基）：

```rust
/// IR 数组 + 根名 —— TS 中所有 "parent → DragonBonesData" 实际读取的内容（D-1.1）
pub struct RootInfo {
    pub name: String,
    pub ir: IrArrays,
}
pub struct IrArrays {
    pub int_array: Vec<i32>,          // TS Int16Array | JSON 普通数组 → i32 无损
    pub float_array: Vec<f64>,        // D1: JSON 路径 f64（dbbin 路径 f32 载入时 widens 到 f64，容差 1e-4）
    pub frame_int_array: Vec<i32>,
    pub frame_float_array: Vec<f64>,
    pub frame_array: Vec<i32>,        // TS Uint16Array → i32
    pub timeline_array: Vec<i32>,     // TS Uint16Array → i32
    pub color_array: Vec<i32>,        // TS Int16Array|Uint16Array → i32（见 F1 表注）
    pub frame_indices: Vec<i32>,
}
pub struct DragonBonesData {
    pub root: Arc<RootInfo>,          // ← TS 的 name + 8 组数组（阶段 2 解析收尾时回填 clone）
    pub auto_search: bool,
    pub frame_rate: i32,
    pub version: String,
    pub stage: Option<String>,
    pub user_data: Option<Box<UserData>>,
    pub armature_names: Vec<String>,
    pub armatures: IndexMap<String, ArmatureData>,   // Map<T> → IndexMap（插入序，D 决策3）
    pub cached_frames: Vec<f64>,      // 阶段6(P1-4) setCacheFrame 写入点：届时方法上移到本类型
                                      // （&mut self），不进 Arc<RootInfo>——阶段 1 只留字段
    pub binary: Option<Vec<u8>>,      // dbbin 专用，P1-1 前恒为 None
}
// 回填时机：解析器先建树，parseDragonBonesData 收尾时 Arc::new(root)，
//           遍历 armatures 设 parent = root.clone()；GeometryData 同步。
```

**结构体形态统一模式——"公共字段 struct + `kind` 枚举携带变体字段"**（对应 TS 的 abstract class + 子类 + `type` 判别字段，避免非法状态、公共字段不重复）：

```rust
// DisplayData（DisplayData.ts 的 5 个子类）:
pub struct DisplayData { pub name: String, pub path: String, pub transform: Transform,
                         pub parent: String /*skin 名*/, pub kind: DisplayKind }
pub enum DisplayKind {
    Image       { pivot: Point, texture: Option<String> },
    Armature    { inherit_animation: bool, actions: Vec<ActionData>, armature: Option<String> },
    Mesh        { geometry: GeometryData, texture: Option<String> },
    BoundingBox { bounding_box: Option<Box<BoundingBoxData>> },
    Path        { closed: bool, constant_speed: bool, geometry: GeometryData, curve_lengths: Vec<f64> },
}
impl DisplayData { pub fn type_(&self) -> DisplayType { /* 按 kind 映射，对应 TS 的 type 字段 */ } }

// 同模式：
//   BoneData { ...公共..., kind: BoneKind::Plain | BoneKind::Surface { segment_x, segment_y, geometry } }
//   ConstraintData { order, name, target, root, bone,
//                    kind: IK { scale_enabled, bend_positive, weight }
//                        | Path { path_slot, path_display, bones, position_mode, spacing_mode,
//                                 rotate_mode, position, spacing, rotate_offset, rotate_mix, translate_mix } }
//   BoundingBoxData { color, width, height, kind: Rectangle | Ellipse | Polygon { vertices } }
// 纯 struct（无变体）: ArmatureData SlotData SkinData AnimationData TimelineData AnimationConfig
//                     CanvasData UserData ActionData WeightData GeometryData TextureAtlasData TextureData
// TimelineData 平铺（x/y 仅 AnimationTimeline 有效，_onClear 默认 0——TS 就是同容器混存）:
//   pub struct TimelineData { pub timeline_type: TimelineType, pub offset: i32,
//                             pub frame_indices_offset: i32 /* 默认 -1! */, pub x: f64, pub y: f64 }
```

**11 个模块映射**（D7：文件名对应，字段/函数顺序照 TS）：

| TS 文件 | Rust 模块 | 主要类型 |
|---|---|---|
| `AnimationConfig.ts` | `model/animation_config.rs` | `AnimationConfig`（24 字段 + `bone_mask: Vec<String>`，clear() = Default，**timeScale=-100** 见 F1.4） |
| `AnimationData.ts` | `model/animation_data.rs` | `AnimationData` + `TimelineData` |
| `ArmatureData.ts` | `model/armature_data.rs` | `ArmatureData` + `BoneData`/`BoneKind` + `SlotData` |
| `BoundingBoxData.ts` | `model/bounding_box.rs` | `BoundingBoxData` + `BoundingBoxKind`（P1-3 才用逻辑，字段先建） |
| `CanvasData.ts` | `model/canvas_data.rs` | `CanvasData` |
| `ConstraintData.ts` | `model/constraint_data.rs` | `ConstraintData` + `ConstraintKind` |
| `DisplayData.ts` | `model/display_data.rs` | `DisplayData` + `DisplayKind` + `GeometryData` + `WeightData` |
| `DragonBonesData.ts` | `model/dragon_bones_data.rs` | `DragonBonesData` + `RootInfo` + `IrArrays` |
| `SkinData.ts` | `model/skin_data.rs` | `SkinData { name, parent: String, displays: IndexMap<String, Vec<Option<DisplayData>>> }` |
| `TextureAtlasData.ts` | `model/texture_atlas_data.rs` | `TextureAtlasData` + `TextureData`（抽象类落地为 struct——core 内单一实现） |
| `UserData.ts` | `model/user_data.rs` | `UserData { ints, floats, strings }` + `ActionData` |

**`_onClear` → `Default` 规则**：`#[derive(Default)]` 不可用时（Transform 的 scale 默认 1、ColorTransform 乘子默认 1、`frame_indices_offset = -1`、`AnimationConfig.timeScale = -100`）手写 `Default`，**逐行照 TS `_onClear` 抄**。TS 字段初值（`= []`、`= null`）与 `_onClear` 不一致时**以 `_onClear` 为准**（对象池复用走的是 `_onClear`）。

**`ArmatureData` 的两个纯逻辑方法本阶段就实现**（不依赖解析器，可直接单测）：
- `sort_bones()`：照 `ArmatureData.ts:225-265` 逐行翻——环形游标 + "等约束/等父先入"的 continue 语义；`for k in constraints` 用 IndexMap 插入序。
- `add_bone/add_slot/add_skin/add_animation/get_bone/get_slot/...`：重名 `console.warn` → Rust `log::warn!`（core 加 `log = "0.4"` 依赖），行为都是"重名忽略"。
- **不做**：`cacheFrames/setCacheFrame/getCacheFrame`（P1-4，阶段 6）。

---

## 任务 6 — `docs/model_mapping.md` 字段对照表（P0-3/P0-4 验收物）

对 11 个 model 模块逐字段建表（格式示例）：

```markdown
| TS | Rust | 变换说明 |
|---|---|---|
| `ArmatureData.type: ArmatureType` | `pub armature_type: ArmatureType` | 字段名 type 与类型名冲突，加 _type 后缀（全 crate 统一规则） |
| `ArmatureData.parent: DragonBonesData` | `pub parent: Arc<RootInfo>` | D-1.1 根共享 |
| `BoneData.parent: BoneData \| null` | `pub parent: Option<String>` | D-1.1 存骨名 |
| `AnimationConfig.timeScale: number = -100` | `pub time_scale: f64 = -100.0` | _onClear 默认值 |
```

**命名统一规则**（写进对照表开头）：TS camelCase → Rust snake_case（`skewX` 之类不存在，见 F1.3）；`type` 字段与类型名冲突时 → `kind`（变体枚举）或 `xxx_type`（单值枚举）；`_` 前缀 protected → 去前缀（Rust 无访问级别，`pub(crate)` 按需）。

---

## 任务 7 — 测试清单汇总

| 测试文件 | 内容 | 来源 |
|---|---|---|
| `tests/geom_golden.rs` | 704 用例对齐 JS（8 类，1e-9） | 任务 2，P0-1 主体验收 |
| `tests/enums.rs` | 16 枚举穷举数值 + BinaryOffset 五组常量 | 任务 3，P0-3 |
| `tests/pool.rs` | 池复用 / hash_code / buffer 延迟回收 | 任务 4，P0-2 |
| `tests/model_defaults.rs` | 全部 model 类型 `Default` == TS `_onClear` 值（重点：AnimationConfig 的 18 个反直觉默认、Transform scale=1、TimelineData.frame_indices_offset=-1、ColorTransform 乘子=1） | 任务 5，P0-4 |
| `tests/sort_bones.rs` | 手造 5 骨 + 1 IK 约束数据，断言 `sort_bones()` 拓扑序（父先于子、约束 target 先于 root） | 任务 5 |
| 阶段 0 已有 10 个测试 | 回归保持全绿 | — |

---

## 验收清单（全绿即阶段 1 完成）

```bash
# 1. 构建 + 全部测试（含阶段 0 回归）
cargo build && cargo test -p dragonbones-core

# 2. clippy 零警告
cargo clippy -p dragonbones-core --tests

# 3. geom 差分（704 用例）——若向量文件缺失可重新生成（种子固定，输出一致）
node tools/geovectors.mjs

# 4. 人工核对
#    - docs/model_mapping.md 覆盖 11 个模块全部字段
#    - tests/enums.rs 覆盖 16 枚举全成员，逐个对照 core/DragonBones.ts:27-277
```

**常见坑回顾**（全部实测）：
1. `BinaryOffset` 单枚举 → E0081 编译失败（F1.1），拆五组模块常量。
2. `concat`/`invert`/`toMatrix`/`fromMatrix` 有分支怪癖，逐行翻，禁止数学习惯重写（任务 1 坑位清单 1–4）。
3. `fromMatrix` 是 `atan(b/a)` 不是 `atan2`。
4. `Default` 抄 `_onClear` 不抄直觉（`timeScale=-100`、`frame_indices_offset=-1`）。
5. 字段引用替换查 D-1.1 表，**不要**引入 `Rc<RefCell>`（D3 禁令）；`Arc` 仅用于不可变的 `RootInfo`。
6. `type` 字段命名冲突按任务 6 规则统一，别一个模块一个风格。
7. `vectors.json` 入库后不要随手重生成（随机种子固定但 diff 噪音大）。

---

## 完成后

跑完验收清单，把 `cargo test` 和 `cargo clippy` 输出发我。确认全绿后我写 **阶段 2（解析器与 IR）** 指南——那是 requirements 标注的最高风险区（M2），会带 `js_round` 等 D2 语义助手的完整伪代码和 9 素材 IR 逐元素比对方案。
