# Bevy 龙骨 Skew Transform 组件：调研与方案

> 状态：调研 / 设计评审稿（2026-10-21）。本文只给方案、接口草图与验证计划，不含实现代码。
>
> **实现状态更新（2026-10-21）**：最终**没有**创建 `bevy_skew_transform` 通用 crate；`dragonbones-bevy` 已按本文 B2 路径实现（直写 `GlobalTransform`），image / 非蒙皮 mesh / 蒙皮 mesh 均可渲染。本文保留作为设计依据与未来通用组件参考。
>
> 目标：在 `crates/` 下增加一个可复用的、支持 2D skew（剪切）的 Bevy Transform 组件，供后续 `dragonbones-bevy` 适配层直接使用。

---

## 0. 结论先行

1. **Bevy 的 `Transform` 不能表示 shear，但 `GlobalTransform` 可以。**
   Bevy 0.19/0.20 的 `GlobalTransform` 内部就是 `Affine3A`，`Mesh2d` 渲染、2D 视锥裁剪、picking 都直接读完整 affine 矩阵。真正缺失的只是“局部 TRS 组件 + 传播系统”这一层，而不是渲染层。
2. **因此不需要为了 skew 回到“顶点烘焙”**（requirements 阶段 7.3 的方案 A）。
   只要能把带 shear 的局部矩阵组合成 `GlobalTransform`，标准 2D 渲染管线就能正确显示。
3. **推荐拆成两条路径，共用一个 crate：**
   - **通用组件路径（B1）**：新增 `SkewTransform` 组件 + 自定义传播，把结果写入 `GlobalTransform`。适合一般 Bevy 实体、调试、编辑器、简单层级。
   - **龙骨适配路径（B2，适配层首选）**：`dragonbones-core` 已经算好了每根骨/每个 slot 的 world matrix，适配层直接把它写进 `GlobalTransform`，不再经过 Bevy 的局部层级推导。这样能原样保留 DragonBones 的 `inheritTranslation/Rotation/Scale/Reflection`、IK 约束、Surface 等语义。
4. **`SkewTransform` 不应该试图“替换全局的 Bevy Transform”**。Bevy 的 `Mesh2d` 通过 required component 强制带 `Transform`，第三方插件也默认 TRS；我们能做的是让 skew 实体以 `SkewTransform` 为局部权威、`GlobalTransform` 为最终输出，并明确它和普通 `Transform` 的边界。
5. **先做 0.5–1 天的技术 spike**：验证 `PostUpdate` 中“在 `TransformSystems::Propagate` 之后、`VisibilitySystems::CheckVisibility` 之前写 `GlobalTransform`”能同帧影响渲染/裁剪，再落 crate。
6. **独立复核结论**：`GlobalTransform` 的写调度是 sound 的（main schedule → extract，无额外帧延迟）；真正脆的是“自定义 ChildOf 层级传播”。适配层应直接写每个 slot 的 GT、**每帧全量同步**（root 移动会把 slot 的 GT 冲掉，不能只同步 dirty slot），并显式排到 `VisibilitySystems::UpdateFrusta` 之前；z 排序只在 `AlphaMode2d::Blend` 生效。详见 §4.3、§5.2、§7。

**版本建议**：本地已升级到 **rustc 1.99.1**，满足 Bevy 0.20.0 的 MSRV **1.97.1**，所以新 crate 直接对齐 **Bevy 0.20.0**。0.19.1 与 0.20 的 transform 架构相同；0.20 的 Mesh2d 改成了 retained extraction（按 `Changed<GlobalTransform>` 重新提取），因此写 `GlobalTransform` 时要用 `set_if_neq` 避免无意义变更。**设计不需要改，只换依赖版本。**

---

## 1. 事实核查

### 1.1 Bevy 侧

| 事实 | 证据 |
|---|---|
| `Transform` = `translation: Vec3 + rotation: Quat + scale: Vec3`，required `GlobalTransform + TransformTreeChanged` | `bevy_transform-0.19.1/src/components/transform.rs:69-105, 69-76` |
| `GlobalTransform` 内部是 `Affine3A`，渲染/裁剪读取完整仿射（含 shear） | `components/global_transform.rs:60, 118-122, 256-292` |
| `Mesh2d` 提取时把 `GlobalTransform.affine()` 作为 `world_from_local` 送进 shader | `bevy_sprite_render-0.19.1/src/mesh2d/mesh.rs:268-309`；0.20 同样如此 |
| 2D 裁剪用 `transform.affine()` 做 AABB/Sphere 变换 | `bevy_camera-0.19.1/src/visibility/mod.rs:748-843` |
| `TransformPlugin` 在 `PostStartup`/`PostUpdate` 的 `TransformSystems::Propagate` 集合里传播 | `bevy_transform-0.19.1/src/plugins.rs:22-48` |
| `VisibilitySystems::CalculateBounds / VisibilityPropagate / CheckVisibility` 被配置在 `TransformSystems::Propagate` 之后；`UpdateFrusta` 也读 `GlobalTransform`，却只被约束在 `CheckVisibility` 之前，和我们的写系统无序 | `bevy_camera-0.19.1/src/visibility/mod.rs:495-535` |
| 2D `calculate_bounds_2d` 只从 `Mesh2d`/`Sprite` 的局部尺寸算 `Aabb`，不读 `GlobalTransform` | `bevy_sprite-0.19.1/src/lib.rs:82-87, 119-217` |
| 仅 `AlphaMode2d::Blend` 的透明 phase 按 `world_from_local.translation.z` 排序；Opaque/AlphaMask 是 binned phase，忽略 z | `bevy_sprite_render-0.19.1/src/mesh2d/material.rs:981, 994-1058` |
| 0.20 的 Mesh2d 只对 `Changed<GlobalTransform>` 的实体重新提取 | Bevy 0.20 `extract_2d_meshes`（docs.rs 源码） |
| `GlobalTransform` 文档明确说“不能直接 mutate”，由 Bevy 管理 | `components/global_transform.rs:19-41` |

**关键判断**：`GlobalTransform` 直接写是“架构上可行、但不是 Bevy 公开契约”的集成方式。它依赖：
- 写发生在 `TransformSystems::Propagate` 之后（否则被覆盖）；
- 写发生在 `VisibilitySystems::CheckVisibility` / 渲染提取之前（否则晚一帧）；
- 不依赖 Bevy 后续再基于 `Transform` 重算这个实体。

这三点都可以用 `configure_sets` 明确表达，但需要在升级 Bevy 时做回归测试。

### 1.2 DragonBones 侧

core 的 `Transform` 与 Bevy 的 TRS 不是同一套参数化：

- `Transform { x, y, skew, rotation, scale_x, scale_y }`，见 [transform.rs](../../crates/dragonbones-core/src/geom/transform.rs:20-27)。
- `skew = skX - skY`，`rotation = skY`；当 `skX == skY` 时 `skew == 0`，两轴正交。解析器的换算见 [object_data_parser.rs](../../crates/dragonbones-core/src/parser/object_data_parser.rs:3478-3484) 和 `docs/guide/01-stage1.md` F1.3。
- 矩阵公式（列向量视角）：

```text
x_axis = ( cos(rotation) * scale_x,  sin(rotation) * scale_x )
y_axis = (-sin(rotation + skew) * scale_y, cos(rotation + skew) * scale_y)
```

对应 core 的 [`Transform::to_matrix`](../../crates/dragonbones-core/src/geom/transform.rs:92-121)。`skew` 的几何含义是“Y 轴相对 X 轴垂直方向的额外夹角”，不是 CSS `skewX(a)` 的 `tan(a)` 剪切因子；两者可互相换算，但不要混用字段语义。

- core 每个 `Bone`/`Slot` 都通过 `TransformObject` 持有：
  - `global: Transform`（矩阵的惰性分解值）；
  - `global_transform_matrix: Matrix`（**权威 world affine**，相对 armature 空间）。
  字段是 `pub` 的，见 [transform_object.rs](../../crates/dragonbones-core/src/armature/transform_object.rs:20-24)。
- 这个 world matrix 已经包含了 DragonBones 特有的：
  - `inheritTranslation` / `inheritRotation` / `inheritScale` / `inheritReflection` 分支；
  - IK / Path 约束对骨矩阵的改写；
  - Surface 父骨的曲面采样；
  - flipX/flipY。
  见 [bone.rs](../../crates/dragonbones-core/src/armature/bone.rs:526-806) 的 `_updateGlobalTransformMatrix` 对齐实现。
- slot 的 world matrix 是 `local_matrix ∘ parent_bone.global_transform_matrix`，见 [slot.rs](../../crates/dragonbones-core/src/armature/slot.rs:794-822)。

**因此对龙骨适配层来说，最可靠的输入不是“在 Bevy 侧重新组合局部 TRS”，而是 core 已经算好的 `global_transform_matrix`。**

### 1.3 顶点空间：一个容易踩的适配层细节

Pixi 参考实现并不是所有 slot 都走同一种变换：

- 普通 image / 非蒙皮 mesh：顶点在 slot 局部空间，显示对象用 slot 的 global transform 摆放。
- 蒙皮 mesh / Surface mesh：`_updateMesh` 已经用骨骼 world matrix 把顶点算到 **armature 空间**，然后调用 `_identityTransform()`，即显示对象本身不再施加 slot 变换。
- Pixi 的 `_updateTransformV4` 还对 pivot 做了 `x = global.x - (a*pivotX + c*pivotY)` 的修正。

参考 `DragonBonesJS/Pixi/8.x/src/dragonBones/pixi/PixiSlot.ts` 的 `_updateFrame`、`_updateMesh`、`_updateTransformV4`。

**推论**：Bevy 适配层如果直接把 slot 的 world matrix 写进 `GlobalTransform`，必须同时确认该 slot 的 mesh 顶点位于哪个空间：
- 顶点是 slot 局部（普通 image / 非蒙皮 mesh）→ `GlobalTransform = armature_world * slot.global_transform_matrix`；
- 顶点已经是 armature 空间（蒙皮 / Surface）→ slot 的显示变换应为 identity，只需 armature root 的 world transform；
- pivot / 子纹理偏移要么烘进顶点，要么在 local matrix 里等价修正。

这不是 `SkewTransform` 组件本身的问题，但它决定了 B2 路径的实现正确性，必须写进适配层验收用例。

### 1.4 现有测试资源

`testdata/geom/vectors.json` 是从 JS 参考运行时生成的 704 个 geom 用例：
- `toMatrix` 68 例，其中 65 例 `skew != 0`；
- `fromMatrix` 137 例，其中 125 例输出 `skew != 0`。

这比自行构造测试强得多，可以直接作为新 crate 的黄金数据：`SkewTransform::to_affine` 与 `from_affine` 用 f32 容差和这些 f64 期望值比对，确保参数化、符号、分支裁剪与 DragonBones 官方一致。

---

## 2. 方案对比

| 方案 | 做法 | 优点 | 缺点 | 结论 |
|---|---|---|---|---|
| **A. 顶点烘焙**（requirements 7.3 当前方案） | 每帧把 slot 局部仿射乘进 quad/mesh 顶点，实体 `Transform` 保持 identity | 不改 Bevy 传播；天然支持任意 shear；适配层实现直接 | 每帧改顶点/重建 mesh；失去 GPU 实例化/批处理；实体层级不参与渲染；裁剪/picking 仍按 identity 或需要额外处理；对蒙皮 mesh 还要另算 | 作为兜底/特例（顶点本来就在 armature 空间的蒙皮 mesh）保留，不作为主方案 |
| **B1. `SkewTransform` + 自定义传播** | 新组件存局部 skew TRS，插件按层级组合，写 `GlobalTransform` | 标准渲染/裁剪/picking 直接可用；不碰顶点；组件可复用；支持 Bevy 侧编辑 | 要自定义传播；写 `GlobalTransform` 依赖调度顺序；与 Bevy `Transform`/第三方 TRS 假设有边界；不能表达 DB 的 inherit/surface 特例（除非适配层自己处理） | **通用 crate 采用**；适配层只在简单层级/调试用 |
| **B2. core world matrix 直接写 `GlobalTransform`** | 适配层每帧把 `slot.global_transform_matrix` 转成 `Affine3A` 写 `GlobalTransform` | 最精确；复用 core 全部继承/约束/Surface 语义；无需自定义层级传播；代码量小；同帧生效 | “GlobalTransform 不应直接写”的版本敏感集成；不能自动带动普通 Bevy 子实体；仍需处理顶点空间/pivot；需要调度约束 | **龙骨适配层首选** |
| **C. 双实体 R·S·R 分解** | 每根骨拆成两个实体，用两层 `Transform` 的 rotation/scale 组合出 shear | 只用标准 `Transform` 传播 | 实体数翻倍；pivot/层级/动画/裁剪都复杂；DB 已有 world matrix，纯属绕路 | 不推荐 |
| **D. 自定义 shader / Material2d 传矩阵** | 顶点 shader 里额外乘一个矩阵 | 可完全绕开 `GlobalTransform` | 需要自定义提取/实例数据/pipeline；丢失标准 batch；等于自建渲染层 | 不推荐，除非将来做批量优化 |
| **E. patch/fork Bevy** | 改 `Transform` 为 affine | 语义最干净 | 维护成本高；与生态脱节 | 不推荐 |

**推荐组合**：
- 新增 `crates/bevy_skew_transform`，实现 B1 的通用组件与数学；
- `crates/dragonbones-bevy` 的 slot/bone 渲染走 B2；
- 蒙皮/Surface 这类“顶点已在 armature 空间”的 mesh 继续使用 A 的顶点策略（或直接把 `GlobalTransform` 设为 armature root 的 world transform，效果等价于 identity slot 变换）。

---

## 3. 推荐 crate 结构

```text
crates/
  dragonbones-core/                 # 已存在，纯 f64，不依赖 bevy
  bevy_skew_transform/              # 新增：通用 skew 组件 + 数学 + 调度
    Cargo.toml
    src/
      lib.rs
      math.rs                       # SkewTransform、Affine2/Affine3A 转换、分解
      plugin.rs                     # SkewTransformPlugin、系统、SystemSet
      dragonbones.rs                # feature "dragonbones"：core Transform/Matrix 互转
      prelude.rs
    tests/
      math_golden.rs                # 复用 testdata/geom/vectors.json
      hierarchy.rs
      schedule.rs
    examples/
      skew_quad.rs                  # 技术 spike / 目视验证
  dragonbones-bevy/                 # 后续阶段 7
    # 依赖 bevy_skew_transform（feature "dragonbones"）
    # 自己的 B2 sync 系统 + slot/bone 实体映射 + 材质/颜色/排序
```

### 3.1 依赖与 feature

建议只依赖 Bevy 的叶子 crate，而不是完整 `bevy`，减少编译面：

```toml
[dependencies]
bevy_app = "0.20"
bevy_camera = "0.20"        # 只为 VisibilitySystems 排序
bevy_ecs = "0.20"
bevy_math = "0.20"
bevy_reflect = "0.20"
bevy_transform = "0.20"
dragonbones-core = { path = "../dragonbones-core", optional = true }
serde = { version = "1", features = ["derive"], optional = true }

[features]
default = ["bevy_reflect"]
bevy_reflect = ["dep:bevy_reflect", "bevy_ecs/bevy_reflect", "bevy_math/bevy_reflect"]
dragonbones = ["dep:dragonbones-core"]
serde = ["dep:serde", "bevy_math/serialize"]
```

说明：
- `bevy_camera` 只用于拿到 `VisibilitySystems`，让插件自动把传播排在 `CheckVisibility` 之前；否则用户很容易漏排序。
- `dragonbones` 默认关闭，保持 crate 通用；适配层启用即可。core 不反向依赖 bevy，无环。
- 若 0.20 遇到兼容问题，可临时锁回 0.19.1，把版本号统一换掉即可；API 设计不需要改。

### 3.2 命名

建议包名/目录：`bevy_skew_transform`（Rust crate 名）。
- 它是一个通用 Bevy 组件，不绑定龙骨数据格式；
- `dragonbones` 只是可选转换 feature；
- 后续如果想独立发布/拆仓，不需要改名。
- 如果团队更希望项目内命名，可改为 `dragonbones-bevy-transform`，但会显得只服务于本项目，降低复用价值。

---

## 4. `SkewTransform` 组件设计（B1）

### 4.1 类型与语义

```rust
#[derive(Component, Clone, Copy, Debug, PartialEq, Reflect)]
#[require(GlobalTransform)]
pub struct SkewTransform {
    /// 局部平移；xy 参与 2D 变换，z 只用于 2D 透明排序。
    /// 在 B1 层级里 z 是局部值、逐层相加；B2 适配层用绝对 world z，不要重复叠加 root z。
    pub translation: Vec3,
    /// 绕 +Z 的旋转，弧度。局部 X 轴的绝对角度。
    pub rotation: f32,
    /// 局部 X/Y 缩放；允许负数（镜像）。
    pub scale: Vec2,
    /// DragonBones 语义：Y 轴相对“与 X 轴垂直”方向的额外夹角，弧度。
    /// 局部 Y 轴方向 = (rotation + skew + PI/2)。
    pub skew: f32,
}
```

设计取舍：

- **只做 2D**：`rotation` 是绕 Z 的标量，`skew` 只作用于 XY 平面。DragonBones 是 2D，不需要把 3D shear 混进来；3D 需求请直接用 `Affine3A`。
- **`skew` 用标量而不是 `skew_x/skew_y` 两个字段**：与 core `Transform` 一一对应，避免和 CSS 的 `skewX/skewY` 语义混淆。需要从数据文件的 `skX/skY` 构造时，再提供显式 helper：
  `from_dragonbones_skew_xy(skew_x, skew_y)`，内部按官方解析器做 `rotation = skY`、`skew = skX - skY`。
- **`translation: Vec3`**：保留 Bevy 的 z 排序通道。线性部分忽略 z。
- **`scale: Vec2`**：和 DB 一致；不引入 z scale。
- **`#[require(GlobalTransform)]`**：组件本身即可作为传播输出。不要 `require(Transform)`：
  - 纯 skew 实体不必被 Bevy 的内置传播碰；
  - `Mesh2d`/`Sprite` 会自动插入 `Transform`，那是它们的 required component，我们把它当占位符并忽略；
  - 如果强行 require，会让 `Transform` 看起来像权威数据，制造两个真相来源。
- 提供 `Default = IDENTITY`、`IDENTITY` 常量、`from_xy/from_translation/from_rotation/from_scale/from_skew`、`with_*`。
- 提供 `to_affine2()`、`to_affine3a()`、`to_matrix3()`、`local_x()`、`local_y()`、`transform_point()`。
- 提供与普通 Bevy `Transform` 的桥接：
  - `From<Transform>`：取 `translation`、Z 轴 rotation、`scale.xy`，`skew = 0`；
  - `to_transform_lossy()`：只回写 TRS，丢弃 skew；文档明确它是 lossy，不能反向作为权威。
- 提供 `then(child) -> Affine3A` 之类的组合方法；**不要**试图让两个 `SkewTransform` 相除/相加后还留在同一参数化里。两个 skew TRS 矩阵相乘一般不再是 `R·U` 形式，组合结果用 affine 表示，需要时再分解。

矩阵公式（Bevy 列向量约定）：

```text
x_axis = (cos(r) * sx,        sin(r) * sx,        0)
y_axis = (-sin(r + skew) * sy, cos(r + skew) * sy, 0)
z_axis = (0, 0, 1)
translation = (tx, ty, tz)
```

注意 core 的 `Matrix` 是行向量存储（`tx/ty` 在第三行），转成 Bevy 时要做布局映射：`Affine3A` 的 x/y 列是 `(a,b)`/`(c,d)`，translation 是 `(tx,ty)`；不要直接把 6 个 f64 按数组顺序塞进 `Mat4`。

### 4.2 从仿射矩阵到 `SkewTransform`

- 任何可逆 2D 仿射都可以分解成 `R(rotation) · U`，其中 `U` 是对角/上三角形式（这正是 DB 的 6 参数参数化）。**奇异点是 `skew = ±90°`**（`det = sx*sy*cos(skew) = 0`），不是 ±180°；±180° 是反射（负 scale）的特例。
- 分解并不唯一（负缩放、±π 分支、接近 ±90° 的退化）。**不要直接把 core `Transform::from_matrix` 当作公开分解 API**：它用 `atan(b/a)`（不是 `atan2`）加 `backupScaleX/Y` 启发式，只在 DB 自己的“已有旧 scale 符号”上下文中自洽，作为通用 API 会丢象限/选错分支。公开的 `try_from_affine2` 应该用 `atan2` + QR/极分解的规范约定，并明确文档“只保证重建矩阵一致，不保证字段唯一”。
- 建议提供：
  - `try_from_affine2(affine: Affine2) -> Result<Self, DecomposeError>`（或 `Option`），拒绝 `det ≈ 0`，文档写清楚“不保证与旧字段逐一相等，只保证重建矩阵一致”；
  - `#[cfg(feature = "dragonbones")] fn from_dragonbones(transform: &dragonbones_core::geom::Transform) -> Self`，直接逐字段 f64→f32，不做矩阵分解（这是适配层唯一需要的入口）；
  - `#[cfg(feature = "dragonbones")] fn to_dragonbones(&self) -> dragonbones_core::geom::Transform`，方便回写/调试。
- 不要把 `rotation/skew` 归一化到 `(-π, π]`：DB 动画的 rotation 可以超过 ±π，归一化会改变 blend/pose 数值。也不要通过 `Quat` 往返（会归一化并丢失无界角度），直接算 `sin_cos`。
- 对 f32 的分解误差要接受：core 是 f64，渲染边界才降 f32。黄金测试应以“重建矩阵误差”为准，而不是逐字段 bit 相等；Bevy 侧容差建议从 core 的 1e-9 放宽到 ~1e-6 相对误差。

### 4.3 插件与调度

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SkewTransformSystems {
    Propagate,
}

pub struct SkewTransformPlugin;
```

`build()` 里：

- `PostStartup`：在 `TransformSystems::Propagate` 之后跑一次，保证第一帧正确。
- `PostUpdate`：
  - `after(TransformSystems::Propagate)`：这样“skew 子树外侧的普通 Bevy 祖先”的 `GlobalTransform` 已经是本帧最新值，可以用作 base；
  - `before(VisibilitySystems::UpdateFrusta)` 与 `before(VisibilitySystems::CheckVisibility)`：`UpdateFrusta` 会读相机 `GlobalTransform`，且只被约束在 `CheckVisibility` 之前，和我们的写系统无序；不显式排序会产生执行顺序歧义（开 `ReportExecutionOrderAmbiguities` 时可见），也可能在相机带 skew 时读到旧值。另一个选择是 `.ambiguous_with(VisibilitySystems::UpdateFrusta)`，但显式 `before` 更稳。
  - `VisibilitySystems::CalculateBounds` 不需要显式排序：2D 的 `calculate_bounds_2d` 只从 `Mesh2d`/`Sprite` 的局部尺寸算 `Aabb`，不读 `GlobalTransform`。显式 `before` 也无害。
  - `VisibilitySystems::VisibilityPropagate` 不读 `GlobalTransform`，不需要排序。
- 不要放在 `Last`：`Last` 在 `CheckVisibility` 之后，裁剪/`ViewVisibility` 会读旧值；虽然 render extraction 还在 `Last` 之后，但已经晚了。
- 若用户没有添加 `VisibilityPlugin`，引用这些 SystemSet 只是创建空集合，不会报错。

> 单写者/叶子契约（重要）：v1 的 B1 传播假设“一个 `SkewTransform` 实体只由本系统写 `GlobalTransform`”，并且 skew 子树连续。混合树会出两类问题：
> 1. 普通 `Transform` 子实体在 skew 父实体之下时，Bevy 会在我们的写系统之前用父实体的占位 `Transform` 算它，导致一帧滞后；
> 2. 没有 `Transform` 的 skew 父实体会截断 Bevy 的内置传播（serial `propagate_recursive` 在父节点缺 `Transform` 时直接返回；parallel 版本的 `NodeQuery` 要求 `Transform`），导致它的普通后代永远不会被 Bevy 更新。
> 因此：要么整个子树都是 `SkewTransform`，要么不要让普通 `Transform` 实体夹在中间；挂特效/粒子请用适配层显式写 `GlobalTransform`，不要依赖 Bevy 自动传播。
>
> 还要处理 `RemovedComponents<SkewTransform>`：组件被移除后，如果没有别的写入者，实体会保留最后一帧的 `GlobalTransform`，看起来“卡住”。插件应监听移除并把 `GlobalTransform` 复位（或至少 `warn!` 要求用户手动处理）。v1 也可以选择“只支持实体生命周期内一直带 `SkewTransform`”并在文档里写清楚。

> 可选的更强集成：把 `SkewTransformSystems::Propagate` 直接嵌套进 Bevy 的 `TransformSystems::Propagate` 集合，并用 `.after(propagate_parent_transforms)` / `.after(sync_simple_transforms)` 排序。这样所有 `after(TransformSystems::Propagate)` 的第三方系统都能看到 skew 结果，而不只是 visibility/render。代价是依赖 Bevy 内部 system 函数名，升级时更脆弱。v1 建议先用独立的 set + 显式 `before(VisibilitySystems::...)`，把兼容性风险降到最低；等确有第三方消费者需求再切换。

传播算法（v1 用简单的“向上找祖先再自根向下乘”）：

```text
for each entity with SkewTransform:
    chain = []
    current = entity
    loop:
        chain.push(local[current])
        if parent exists and parent has SkewTransform:
            current = parent
        else:
            base = parent.GlobalTransform.affine() or IDENTITY
            break
    global = base
    for local in chain.reverse():
        global = global * local.to_affine3a()
    write GlobalTransform(global) if changed
```

实现注意：`Query<&mut GlobalTransform, With<SkewTransform>>` 和 `Query<&GlobalTransform, Without<SkewTransform>>` 可以共存，因为 `With`/`Without` 可证明不重叠；如果未来查询条件更复杂，改用 `ParamSet` 或两阶段 gather，避免借用冲突。

优点：
- 不需要 unsafe、不需要 `GlobalSkewTransform` 缓存组件、不依赖 Bevy 的传播内部实现；
- 龙骨骨架通常几十到几百个实体、深度有限，O(N·depth) 完全可接受。

代价/约束：
- 每个实体重复走祖先链；大规模场景可后续优化（dirty-tree 标记或 `GlobalSkewTransform` 缓存）。
- **不能只按 `Changed<SkewTransform>` 做脏更新**：base 祖先的 `GlobalTransform` 可能因根节点移动而变化，而子实体的 `SkewTransform` 没变；漏更新会让子实体停在旧位置。v1 全量每帧重算正好规避这个坑；将来做 dirty 优化时，必须把 base/root 变化纳入脏集合。
- **skew 子树必须连续**：一旦向上遇到没有 `SkewTransform` 的祖先，就把它的 `GlobalTransform` 当作 base。不要把“普通 `Transform` 实体”夹在 skew 子树中间；它的 `GlobalTransform` 是 Bevy 用占位 `Transform` 算出来的，不是 skew 结果。
- `SkewTransform` 实体上的 `Transform` 是占位符，**不参与计算**。如果要与普通 Bevy 层级混用，只在 skew 子树的根之上放普通 `Transform`。
- `transform: Vec3` 的 z 语义：在 B1 层级里作为**局部 z** 逐层相加（与矩阵乘法的 z 平移一致）；B2 适配层使用**绝对 world z**（`root_z + sort_z`），不要把两者叠加。建议在文档/类型注释里明确“z 是局部还是世界”，避免适配层重复加 root z。

写 `GlobalTransform`：

- 用 `GlobalTransform::from(Mat4::from(affine))`（或等价构造）；
- 用 `Mut::set_if_neq` / 先比较再写，避免每帧无变化也触发 change detection；0.20 的 retained extraction 对 `Changed<GlobalTransform>` 敏感，这点尤其重要；
- z 分量来自各层 `translation.z` 的累加；2D 排序读 `GlobalTransform.translation().z`，与 shear 无关。

### 4.4 与 `Transform` / 第三方生态的边界

**能工作**：
- `Mesh2d` / `Sprite` 渲染：提取 `GlobalTransform.affine()`；
- 2D 视锥裁剪：`Aabb` 用 affine 变换；
- mesh picking / ray cast：picking 读 `GlobalTransform.affine()`；
- z 排序、可见性继承（`Visibility`/`InheritedVisibility` 不依赖 `Transform`）。

**不能工作 / 需要避免**：
- `GlobalTransform::compute_transform()`、`rotation()`、`scale()`、`to_isometry()`、`reparented_to()` 在含 shear 时文档明确说结果无效；任何第三方插件如果对 skew 实体调用这些，会得到错误值。文档应写“**永远不要分解 `GlobalTransform`，读 affine()**”。
- `Transform` 不再权威：读 `Transform` 的插件（动画、gizmo、编辑器属性面板、部分物理/拾取实现）看到的是占位 identity，不是 skew 结果。若需要兼容，可另做可选的 lossy `Transform` 同步（只写 translation/rotation/scale，丢弃 skew），但不要让它反向覆盖 `SkewTransform`。
- `Transform::set_parent_in_place` / `remove_parent_in_place` 内部走 `reparented_to` / `compute_transform`，会丢 shear；skew 实体上要换父子关系请直接用 `ChildOf`/`add_child` 并重算。
- 非 skew 子实体不会自动继承 skew 变换（见 4.3 连续子树约束）。如果要把普通 Bevy 实体挂到骨上，需要适配层显式写它的 `GlobalTransform`，或也给它一个 `SkewTransform`。
- **z 排序只在 `AlphaMode2d::Blend` 的透明 phase 生效**：`Opaque2d` / `AlphaMask2d` 是 binned phase，不读 z。requirements 里的 `-index*0.001` 方案必须确认 DB slot 都在 Blend；否则要自定义 phase sort key。另外 z 越大越后画，方向要和 DB 的 zOrder/displayIndex 对齐。
- `bevy_picking` 在 `PreUpdate` 读取上一帧的 `GlobalTransform`，有和其它 transform 一样的 1 帧延迟；龙骨命中检测应优先用 core 已有的 `containsPoint` 等实现。picking 本身对 shear 是安全的（用 `affine()`/inverse），但 y-down 根节点的负行列式会翻转三角形绕序，backface culling 可能把面剔除；需要测试/关闭 backface。
- Bevy 的 UI 已经改用独立的 `UiGlobalTransform(Affine2)`；`Mesh2d` 不读它，所以本方案不影响 UI。若将来要把 skew 用于 UI，需要另做。
- 3D shear 不支持；本组件只处理 XY 平面。

---

## 5. 龙骨适配层路径（B2，首选）

### 5.1 调度

在 `PostUpdate` 增加一个 set：

```text
TransformSystems::Propagate
    → DragonBonesSystems::SyncGlobalTransforms
    → VisibilitySystems::UpdateFrusta / CheckVisibility
    → render extraction
```

（`CalculateBounds` 的 2D 实现不读 `GlobalTransform`，不需要显式排序。）

这样：
- armature root 的 `Transform` 在 `Update` 改了，本帧也能反映到 slot；
- 裁剪和渲染同帧看到新矩阵；
- 不依赖 `TransformPlugin` 给 slot 算出的占位值。

core 的 `advanceTime` 应在 `Update` 或 `PostUpdate` 中早于这个 sync set 的地方完成；sync 系统只读取 core 已经算好的 `global_transform_matrix`，不再做 DB 数学。

### 5.2 同步逻辑（按 slot 类型分支）

对每个需要渲染的 slot：

```text
root = armature_root_entity.GlobalTransform.affine()   // 含 y-down 翻转/摆位
if slot 是普通 image / 非蒙皮 mesh:
    local = affine3_from_dragonbones_matrix(slot.global_transform_matrix)
    world = root * local
    // pivot/顶点空间按 1.3 的规则处理
    slot.GlobalTransform = world (+ slot z)
else if slot 是蒙皮 mesh / Surface mesh:
    // core 的 update_mesh 已把顶点算到 armature 空间
    slot.GlobalTransform = root (+ slot z)  // 等价于 identity slot transform
else:
    // 子骨架/空 slot 等按适配层策略
```

`affine3_from_dragonbones_matrix` 就是把 core 的 `Matrix`（行向量布局）转成 Bevy 的 `Affine3A`（列向量布局），并补 z。这个 helper 可以放在 `bevy_skew_transform` 的 `dragonbones` feature 里，保证只有一份矩阵布局转换代码。

对 `GlobalTransform` 的写操作建议提供一个薄的扩展 trait（`GlobalTransformExt`）或自由函数，例如：

```rust
pub trait GlobalTransformExt {
    fn from_affine3a(affine: Affine3A) -> GlobalTransform;
    fn set_affine3a(&mut self, affine: Affine3A);
}
```

这样 B2 系统和 B1 插件都通过同一个入口写 affine，避免两处各自拼 `Mat4` 时出现布局/顺序偏差。

- **每帧同步所有 slot，不要只同步 dirty slot**：这是最容易踩的坑。root `Transform` 一动，Bevy 的内置传播会把所有 slot 的 `GlobalTransform` 重写成 `root_TRS * identity`；如果我们的同步因为“slot 自身没 dirty”而跳过它，该 slot 就会塌回 root 位置/identity。几百个 slot 每帧一次 affine 乘法很便宜；如果一定要做 dirty 优化，必须把“root/父级变动”纳入脏集合。
- 写前用 `set_if_neq` 比较，避免无变化时触发 change detection / 0.20 retained re-extraction 抖动。
- 不要在同一个实体上同时启用 `SkewTransformPlugin` 的传播和 B2 同步；两者都写 `GlobalTransform`，语义会打架。二选一，或让 B2 实体不带 `SkewTransform`。
- **slot 扁平挂在 armature root 下**，不要镜像骨层级：B2 已经用 core world matrix 算好了每个 slot 的全局变换，Bevy 层级只用于 `ChildOf` 生命周期/可见性继承。把 slot 挂在 bone 实体下会让内置传播先算一遍 `bone_TRS * identity`，虽然会被我们覆盖，但增加无谓工作并制造“混合层级”误解。
- **y-down**：core 的 `DragonBones::Y_DOWN` 恒为 true，数据是 y-down。只在 armature root 做一次 Y 翻转（`Transform::from_scale(Vec3::new(1., -1., 1.))` 或 root 矩阵），不要每层翻。B2 的 `root` 矩阵天然带这个翻转，slot local matrix 保持 core 原值即可。注意负行列式会翻转 picking 的三角形绕序（见 4.4）。
- **pivot**：Pixi 对 image slot 的 pivot 做了显示位置修正；Bevy 适配层必须在 quad 顶点坐标或 local matrix 里等价处理，否则会出现整体偏移。建议把 pivot 烘进每个 image slot 的 quad 顶点（一次性），这样 `global_transform_matrix` 可以直接作为 `GlobalTransform`。
- **Surface / 蒙皮 mesh 的顶点空间**：core 的 `SurfaceState::vertices` 注释明确是“世界坐标顶点”（[surface.rs](../../crates/dragonbones-core/src/armature/surface.rs:60)）；Pixi 对这类 mesh 走 `_identityTransform()`。Bevy 适配层不要再把 slot world matrix 乘到这些顶点上，否则二次变换。上文的“slot 变换 = armature root”分支就是对应处理。
- **z 排序**：core 的 `z_index`/`z_order` 决定 slot 先后；写 `GlobalTransform` 时把 z 设为**绝对 world z**（`root_z + sort_z`），不要再把 root z 和绝对 slot z 叠加两次。`Transparent2d`（仅 Blend）的 sort key 读 `translation().z`，不受 shear 影响；Opaque/AlphaMask 不读 z（见 4.4）。

### 5.3 为什么 B2 比 B1 更适合龙骨

- DragonBones 的 `inheritScale=false`、`inheritRotation=false`、`inheritReflection`、IK、Surface 等语义，core 已经逐行对齐 JS；在 Bevy 侧用普通父子 `Transform` 重新推导会重复实现，且容易偏差。
- core 的 `global_transform_matrix` 是 f64 权威值；直接降 f32 写 `GlobalTransform`，数值路径最短。
- 不需要给每根骨/slot 维护 `SkewTransform` + 自定义传播；适配层只做“读矩阵 → 转换 → 写组件”。
- 对蒙皮/Surface 这类顶点已经 world 化的 mesh，B2 的“slot 变换 = armature root”正好对应 Pixi 的 `_identityTransform()`，不会二次变换。

### 5.4 代价

- B2 是版本敏感的 `GlobalTransform` 写操作，需要 spike + 回归测试。
- Bevy 的普通子实体不会自动跟随 bone；要挂特效/粒子到骨上，需要额外同步（给子实体写 `GlobalTransform`，或也使用 B1）。
- `Transform` 仍是 `Mesh2d` 的 required component，但只是占位；不要在 B2 实体上同时用 Bevy 动画/物理改 `Transform`。

---

## 6. 验证计划

### 6.1 技术 spike（先做，0.5–1 天）

1. 最小 App：`DefaultPlugins` + `TransformPlugin` + 自定义系统（`PostUpdate`，`after(TransformSystems::Propagate)`，`before(VisibilitySystems::CheckVisibility)`）。
2. 一个 `Mesh2d` quad，`GlobalTransform` 写成 45° 剪切矩阵；确认屏幕上显示为平行四边形，而不是被 silently 丢掉。
3. 在同一帧的 `CheckVisibility` 之后（或用一个 `PostUpdate` 末尾系统）读取 `GlobalTransform.affine()`，确认没有被内置传播覆盖。
4. 放两个不同 z 的 skew quad，确认 Blend 透明排序使用 z、Opaque/AlphaMask 不受 z 影响且裁剪不误剔除。
5. 在同一个 App 里移动 armature root，保持某个 slot 不变，确认它的 `GlobalTransform` 仍被重写为 `new_root * slot_matrix`（验证“不能只同步 dirty slot”）。
6. 优先在 Bevy 0.20.0 跑；0.20 的 retained extraction 要确认因 `Changed<GlobalTransform>` 重新提取。如需兼容旧项目，再补跑 0.19.1（当前 rustc 1.99.1 两者都能跑）。

### 6.2 数学黄金测试

- 读取 `testdata/geom/vectors.json` 的 `toMatrix` / `fromMatrix`；
- 把 f64 期望值转成 f32 容差（建议 `1e-4 * (1 + |expected|)`，具体按坐标量级调）；
- 验证：
  - `SkewTransform → Affine3A` 与 JS `toMatrix` 一致；
  - `Affine3A → SkewTransform` 重建矩阵一致（不要求字段逐一相等）；
  - `skew = 0`、`skew = ±π`、负 scale、`|skew|` 接近 π/2、`rotation` 接近 ±π/4 的边界分支。
- 现有 65/125 个非零 skew 用例已经覆盖了大部分实际分支。

### 6.3 层级/调度测试

- 构造 root(normal Transform) → skew parent → skew child，`app.update()` 后比较 `GlobalTransform.affine()` 与手算 `base * local1 * local2`。
- 构造 root(skew) + 多个 children，验证深度/顺序无关。
- 在 `PostUpdate` 加一个 `.after(SkewTransformSystems::Propagate).before(VisibilitySystems::CheckVisibility)` 的测试系统，断言读到的是新矩阵。
- 同时挂 `Transform` + `SkewTransform`（模拟 `Mesh2d` 的 required component），确认最终 `GlobalTransform` 是 skew 结果。
- 用 `set_if_neq` 测试：输入不变时 `GlobalTransform` 不产生 `Changed`。
- 如果加 `VisibilityPlugin`，确认没有 schedule ambiguity；显式 `.before(VisibilitySystems::UpdateFrusta)`，并测试相机带/不带 skew 两种情况。
- 移除 `SkewTransform` 后，确认 `GlobalTransform` 不会停留在最后一帧（或按文档明确该限制）。
- 混合层级测试：普通 `Transform` 子实体挂在 skew 父实体下、以及无 `Transform` 的 skew 父实体（会截断内置传播），确认行为与文档一致（serial 和 multi_threaded 两种构建都跑）。

### 6.4 适配层验收

- 9 个素材：在每个采样帧，把 core 的 `slot.global_transform_matrix` 与 Bevy `slot.GlobalTransform.affine()` 对比（f32 容差）；蒙皮/Surface 分支单独验证“顶点空间 + identity slot”。
- 目视对比：`Surface_P1_4`（mesh 变形）、`Dragon_Blend` / `Rooster_Blend`（blend + skew 数据）、PlantAnimate（静态 `skX != skY` 的 ±180° 特例）。
- 根节点移动 + 静态 slot：移动 armature root 后只 update 一帧，slot 的 `GlobalTransform` 必须是 `new_root * slot_matrix`，不能是旧值或 identity（验证 5.2 的“每帧全量同步”）。
- z/alpha mode：分别用 `AlphaMode2d::Blend` / `Opaque` / `AlphaMask` 建 slot，确认只有 Blend 受 z 影响；和 DB 的 zOrder/displayIndex 顺序对齐。
- Surface / 蒙皮：确认顶点是 armature 空间、slot 变换没有被二次施加；对比 Pixi 的 `_identityTransform`。
- 9 素材中静态 skew 差异只有 PlantAnimate 的两处 `skX - skY = ±180°`；真正的一般 shear 主要来自合成/时间轴/未来素材，因此必须有合成 golden，不能只靠现有 assets。
- y-down + shear 的 picking：对负行列式 quad 做一次 ray hit / backface 测试。

---

## 7. 风险与坑

| # | 风险 | 等级 | 对策 |
|---|---|---|---|
| 1 | `GlobalTransform` 直接写不是 Bevy 公开契约，升级可能失效 | 高 | spike 覆盖；把写系统集中到一个小模块；Bevy 升级时跑回归；关注上游是否提供 custom transform 扩展点 |
| 2 | 写晚于渲染提取 / 被 `TransformSystems::Propagate` 覆盖 | 高 | `after(TransformSystems::Propagate)` + `before(VisibilitySystems::CheckVisibility)`；测试断言同帧可见 |
| 3 | `UpdateFrusta` 读相机 `GlobalTransform`，和写系统无序 | 中 | 显式 `.before(VisibilitySystems::UpdateFrusta)`；或 `.ambiguous_with` 并接受歧义警告；开 `ReportExecutionOrderAmbiguities` 验证 |
| 4 | skew 子树混入普通 `Transform` 实体，导致 base 不对/一帧滞后 | 高 | 文档写“skew 子树连续、单写者”约束；debug 模式下检测并 warning；适配层 B2 不依赖该层级 |
| 5 | 无 `Transform` 的 skew 父实体截断内置传播，普通后代永远不更新 | 高 | 不要依赖“缺 Transform 就退出传播”的 emergent 行为；普通后代显式写 GT 或也加 `SkewTransform`；serial/multi_threaded 都测 |
| 6 | `Mesh2d` 强制 `Transform`，占位 identity 被第三方插件当成真值 | 中 | 文档明确 `SkewTransform` 权威；可选 lossy `Transform` 同步；适配层 B2 实体不要交给 Bevy 动画/物理 |
| 7 | 第三方插件对 `GlobalTransform` 调 `compute_transform/rotation/scale` | 中 | 文档列出不兼容 API；picking 用 core 命中测试；必要时给插件提供 affine-aware 替代 |
| 8 | B2 只同步 dirty slot，root 移动后 slot 塌回 root/identity | **高** | 每帧同步所有 slot；若做 dirty 优化，必须把 root/parent 变化纳入；P0 测试 |
| 9 | 蒙皮/Surface mesh 顶点已在 armature 空间，直接写 slot 矩阵会二次变换 | 高 | 适配层按 1.3/5.2 分支；Pixi 参考 `_identityTransform`；Surface `vertices` 明确是世界坐标；验收用例覆盖 |
| 10 | pivot 未等价处理导致整体偏移 | 中 | 把 pivot 烘进顶点或 local matrix；与 Pixi 对比 |
| 11 | f64→f32 精度 | 低 | core 保持 f64；只在渲染边界降 f32；Bevy 侧容差 ~1e-6 相对误差 |
| 12 | `skew` 接近 ±90° 奇异、负 scale、±180° 反射分支导致分解不稳定 | 中 | 保留语义字段，不做不必要的矩阵分解；`try_from_affine` 用 `atan2` 规范分解并拒绝 `det≈0`；不要把 core `from_matrix` 当公开 API；测试边界 |
| 13 | 0.20 retained extraction 因每帧写 `GlobalTransform` 产生抖动 | 中 | `set_if_neq`；但在 B2 里不要为了省变更而漏同步（见 8）；测试 `Changed` 次数 |
| 14 | z 排序只在 Blend phase 生效 | 中 | 确认 DB slot 的 AlphaMode；Opaque/AlphaMask 需要自定义 phase sort；验证 z 方向 |
| 15 | y-down 根翻转导致负行列式，picking 三角形绕序/backface 出错 | 中 | 测试 sheared + y-flipped quad 的 ray hit；按需关闭 backface |
| 16 | Bevy 0.19.1 vs 0.20.0 版本选择 | 中 | 已升级 rustc 1.99.1（满足 0.20 MSRV 1.97.1），直接用 0.20.0；保留 0.19.1 兼容分支作为回退 |
| 17 | 自定义传播的 O(N·depth) 性能 | 低 | 龙骨规模可接受；先正确；有 profile 再加 `GlobalSkewTransform`/dirty tree |
| 18 | `GlobalTransform` 写入与 `bevy_picking` 的 1 帧延迟 | 低 | 与 Bevy 自身 transform 一致；命中检测用 core |
| 19 | `TransformTreeChanged` / static optimization 与自定义传播互不影响，但初学者会误以为有关 | 低 | 文档说明自定义传播不依赖它；不要复用 `Transform` 的脏标记 |
| 20 | 移除 `SkewTransform` 后 `GlobalTransform` 卡在最后值 | 低 | 监听 `RemovedComponents<SkewTransform>` 复位/警告；或文档限定生命周期 |

---

## 8. 里程碑与验收

| 阶段 | 内容 | 预计 | 验收 |
|---|---|---|---|
| M0 | 技术 spike：写 `GlobalTransform` 显示 shear quad；0.19/0.20 调度验证 | 0.5–1 天 | 目视 + 矩阵断言；裁剪/排序正常 |
| M1 | `bevy_skew_transform` 数学层：类型、转换、分解、golden 测试 | 1–2 天 | `testdata/geom/vectors.json` 全部通过；边界用例 |
| M2 | 插件/传播/调度/change detection 测试 | 1–2 天 | 层级、同帧可见、`set_if_neq`、无 ambiguity |
| M3 | `dragonbones` feature：core `Transform`/`Matrix` 互转、B2 sync helper、适配层原型 | 1 天 | 至少 1 个素材的 slot matrix 与 core 对齐 |
| M4 | 阶段 7 适配层正式接入（不在本次范围） | 7–14 天（requirements 原估） | 9 素材、blend、mesh、事件 |

---

## 9. 待拍板

1. **Bevy 版本**：已确认本地 rustc 1.99.1，直接上 Bevy 0.20.0；是否仍要保留 0.19.1 的兼容 feature 分支？
2. **crate 名字/范围**：`bevy_skew_transform`（推荐，通用）还是 `dragonbones-bevy-transform`（项目专用）？
3. **适配层主路径**：是否接受 B2（core world matrix 直写 `GlobalTransform`）作为 slot 渲染主路径，B1 只作为通用/调试能力？
4. **是否更新 requirements 7.3**：当前文档写的是“方案 A 顶点烘焙 + 备选 B 直接写 GlobalTransform”；如果认可本方案，应把 7.3 改为“B2 主路径 + B1 通用组件 + A 仅用于蒙皮/Surface 顶点空间”，并更新 R2 的对策。

### 9.1 决策记录（2026-10-21）

- **Rust / Bevy**：本地 rustc 1.99.1，满足 Bevy 0.20.0 的 MSRV 1.97.1；新 crate 直接对齐 0.20.0。
- **crate 命名 / 是否做通用组件**：**暂不实现 `bevy_skew_transform`**；只做 `crates/dragonbones-bevy` 的 B2 适配层。后续其他项目确实需要通用 2D skew 组件时，再按“默认零龙骨依赖 + optional `dragonbones` feature”的约束抽成通用 crate。
- **适配层路径**：B2 直写 `GlobalTransform` 已实现；每帧全量同步所有 slot（root 移动会冲掉子 GT）；no SkewTransform dependency。
- **文档更新**：`requirements.md` 7.3 已按实现改为 B2；7.4/7.5/R2/常见坑同步更新。`bevy_skew_transform` 保留为未来可选项，不再是本阶段交付物。

---

## 10. 参考

- 本地 Bevy 0.19.1 源码（0.20 的 transform 架构相同；实现/spike 时以 0.20 源码复核）：
  - `bevy_transform-0.19.1/src/components/transform.rs`
  - `bevy_transform-0.19.1/src/components/global_transform.rs`
  - `bevy_transform-0.19.1/src/plugins.rs`
  - `bevy_transform-0.19.1/src/systems.rs`
  - `bevy_camera-0.19.1/src/visibility/mod.rs`
  - `bevy_sprite_render-0.19.1/src/mesh2d/mesh.rs`
- 本地 DragonBones core：
  - [transform.rs](../../crates/dragonbones-core/src/geom/transform.rs)
  - [transform_object.rs](../../crates/dragonbones-core/src/armature/transform_object.rs)
  - [bone.rs](../../crates/dragonbones-core/src/armature/bone.rs)
  - [slot.rs](../../crates/dragonbones-core/src/armature/slot.rs)
- 本地 JS 参考：
  - `DragonBonesJS/DragonBones/src/dragonBones/geom/Transform.ts`
  - `DragonBonesJS/Pixi/8.x/src/dragonBones/pixi/PixiSlot.ts`
- 外部资料：
  - Bevy 0.20 发布说明：<https://bevy.org/news/bevy-0-20/>
  - Bevy 讨论：shear/affine transform 的限制与 workaround：<https://github.com/bevyengine/bevy/discussions/14622>、<https://github.com/bevyengine/bevy/discussions/4822>、<https://github.com/bevyengine/bevy/discussions/19513>
  - `bevy_mod_transform2d`（2D Transform 代理，写入 `Transform`，不支持 skew）：<https://github.com/tim-blackbird/bevy_mod_transform2d>
