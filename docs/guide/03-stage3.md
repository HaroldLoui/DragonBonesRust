# 阶段 3 实现指南 — armature 层级与工厂装配

> 配套 `docs/requirements.md` §3 阶段 3。按任务 1→8 顺序实现，验收清单全绿后告诉我，
> 我写阶段 4（动画系统）指南。本指南所有"实测"结论均为我对 5.7 源码 / 9 素材 / 参考运行时的实际验证。

---

## 0. 已实测事实与 requirements 修正（先读）

| # | 事实 | 影响 |
|---|---|---|
| F3.1 | ~~**9 素材全部无约束**（实测 `constraint` 键不存在）~~ **勘误（阶段 3 收口实测）**：查错了键——原始 JSON 用 **`"ik"`**，`龙`（IK_L/IK_R，weight=0）与 `Rooster×3`（bone_ik/bone_ik1，weight 缺省 1.0）共 4 个 IK | ~~stub~~ **IK 已在阶段 3 实现真身**（pose 差分证明必须：`updateByConstraint → isCache=true → fromMatrix` 回填链 + weight=1 解算）；Path 仍 stub（0 素材，P1-2）。bones↔constraints 借用已解决：`Bone::update` 三段签名 `left/right/constraints`。见 `model_mapping` D-4.19 |
| F3.2 | **`_buildBones` 按 `sortedBones`（拓扑序）创建**（`BaseFactory.ts:168-173`），`Armature.advanceTime` 迭代这个创建序 | 运行时 `bones: Vec<Bone>` 天然"父索引 < 子索引" → `split_at_mut(i)` 顺序更新安全；**不是 JSON 原始序** |
| F3.3 | **`origin` 全库只读**：唯一赋值 `Bone:360 origin = boneData.transform`、`Slot:461/464` 同理，无人写 `origin.xxx` | Rust 在 init 时 `clone` 一份 `origin: Transform` 与 JS 引用语义恒等（data 解析后不可变） |
| F3.4 | `flipY = this._armature.flipY === DragonBones.yDown`（`Bone.ts:120`）——是**恒等比较**不是 `&&` | yDown 恒 true → `flipY = armature.flipY`；但照抄表达式，勿"化简"成 `&&` |
| F3.5 | **Surface 骨被真实使用**：`Surface_P1_4` 的 `surf`（segment 1×1，且有子骨） | ~~双线性插值~~ **勘误**：是"格子→对角线二分→`_getAffineTransform` 三角形拟合"（Surface.ts:66-88/165-437），静态姿态验收会打到 |
| F3.6 | `Bone::update` 是**四分支脏标记机**（`Bone.ts:365-441`）：cache 命中 / 已缓存 / 约束+脏 / 普通；`_localDirty` 在 update **尾部重置为 true**、由 `updateByConstraint` 置 false 阻止同帧重复算矩阵 | P1-4 前 `cacheFrameIndex` 恒 -1 → 永远走"普通"分支；`_localDirty` 的时序照抄，勿按直觉改成"算完置 false" |
| F3.7 | `armature._parent`（子骨架回指父插槽）在 core 只有两处消费：`Armature.ts:330` alpha 传播、`Animation.ts:177` inheritTimeScale（阶段 4） | 设计见 D-3.2，用索引解环 |
| F3.8 | `_buildSlots` 流程（`BaseFactory.ts:177-224`）：defaultSkin 与 currentSkin 合并（current 覆盖）→ 遍历 `sortedSlots` → `replaceRawDisplayData` → `replaceTextureData`（仅当 textureAtlasName 非空）→ `_getSlotDisplay` → `replaceDisplay` → 末尾 `_setDisplayIndex(slotData.displayIndex, true)` | 逐行照抄，尤其 merge 语义与最后的 `_setDisplayIndex` 带 force 参数（**勘误：第二参是 `isAnimation` 不是 force——`grep force Slot.ts` 为 0，TS:598**） |
| F3.9 | 9 素材**单骨架**（requirements §1.3）→ 子骨架（Armature 型 display）路径 golden 无覆盖 | 接口照实现 + 手写单测；差分不承诺 |
| F3.10 | 槽排序比较器 `zIndex*1000+zOrder > … ? 1 : -1`（`Armature.ts:47`，永不返回 0）；ES2019 起 Array.sort **稳定** | Rust 用 `sort_by_key(zIndex*1000+zOrder)` + **stable sort**（`sort_by_key` 即稳定）→ 平局保持原序；**勘误**：「= V8 实际行为」不成立——比较器永不返 0（不一致比较器）不受稳定排序规格覆盖，node 实测 V8 对**等键组逆序**；决策 = 保持 Rust 原序并登记 **D-4.29**（官方公开路径 z_order 恒唯一、等键不可达）。阶段 0 golden 的 `meta.slotNames` 是**排序后**槽名序（dump 在 advance 之后）→ 验收锚点（R5 收口） |
| F3.11 | `buildArmature` 收尾调 `armature.advanceTime(0.0)`（`BaseFactory.ts:713`） | Rust 装配收尾同样先 `invalidUpdate` 再 `advance_time(0.0)`，否则静姿态不算 |

---

## 1. 设计决策（D-3.1 – D-3.4）

### D-3.1 运行时容器：arena + newtype 索引（requirements D3 落地）

```rust
// factory/base_factory.rs（同时充当运行时世界——阶段 5 WorldClock 挂这里）
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct ArmatureIdx(u32);
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct BoneIdx(u32);    // 索引本 armature.bones
#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct SlotIdx(u32);    // 索引本 armature.slots

pub struct BaseFactory {
    data_map:  IndexMap<String, Arc<DragonBonesData>>,   // dragonBonesName → 阶段2产物
    atlas_map: IndexMap<String, Vec<TextureAtlasData>>,
    armatures: Vec<Option<Armature>>,   // 骨架 arena；None = 已 dispose 的空位
    free:      Vec<u32>,                // 空位复用栈（= 池语义的 arena 版，见下）
    parser:    ObjectDataParser,
    // … 3 个适配方法（D-3.4）
}

impl Armature {
    bones: Vec<Bone>,          // 创建序 = sortedBones 拓扑序（F3.2）
    slots: Vec<Slot>,
    constraints: Vec<Constraint>,   // 阶段 3 恒空（F3.1）
    data: Arc<DragonBonesData>,     // D-3.3
    armature_name: String,
    parent: Option<(ArmatureIdx, SlotIdx)>,  // D-3.2，子骨架回指
    global_alpha / alpha / lock_update / slots_dirty / z_index_dirty / alpha_dirty …,
    actions: Vec<ActionRequest>, proxy: Box<dyn ArmatureProxy>,
}
```

**池语义决策（记入 model_mapping §4 偏差）**：JS 里 `Armature/Bone/Slot` 走 `borrowObject` 池；Rust 中 Bone/Slot 被 Armature 拥有、随其生灭，**arena 空位复用**（`dispose` → `on_clear()` → `free.push(idx)`；`build` → 优先 pop 空位）等价实现"复用不重发 hash_code、dispose 时清空"语义。阶段 1 的 TypeId 池保留给阶段 5 的 `EventObject`。

### D-3.2 环引用用索引解环

`Armature ↔ Slot`（父插槽 vs 子骨架 display）成环：**双方都存纯数值索引**，不存对象引用、不存 `Rc`（D3 禁令）：

- `Slot.child_armature: Option<ArmatureIdx>`
- `Armature.parent: Option<(ArmatureIdx, SlotIdx)>`

alpha 传播读父插槽时要求"父索引 < 子索引"（父骨架先于子骨架 build → arena 序成立），用 `split_at_mut` 取；阶段 4 的 inheritTimeScale 同理解析。

### D-3.3 数据引用 = `Arc<DragonBonesData>` + 名字解析

运行时对象不借生命周期（会污染池/arena 所有权），持有：

- `Armature: Arc<DragonBonesData>` + `armature_name`（→ `data.armatures[name]` 拿 `ArmatureData`；再 `.root` 拿阶段 2 的 `Arc<RootInfo>`/IR 数组——阶段 4 时间轴直接用）
- `Bone/Slot: 自己的 name` + 初始化时解析好的 `parent: Option<BoneIdx>`；`origin` 按 F3.3 clone
- 需要 data 的热路径（update 里的 `bone_data.inherit_*`）：`Armature` 在 build 时把每根骨的解析结果缓进 `Bone` 的 `inherit_*` 字段（build 一次、update 零查找）——或 update 内 `data.armatures[..].bones[..]` 双查，二选一，**推荐前者**（字段与 BoneData 一一对应，阶段 6 前 data 不可变，无失效风险）

**数据销毁语义偏差**：JS 靠 `armatureData.parent === null` 检测数据被 factory.clear（`Armature.ts:306` 警告）；Rust 持 `Arc`，数据活到无引用——该警告分支不可达，更安全。记入 model_mapping §4。

### D-3.4 渲染契约：两个 trait + Null 实现

```rust
/// Slot 的 14 抽象方法（armature/Slot.ts 338 行起）——Bevy 适配层（阶段 7）的全部契约
pub trait SlotRenderAdapter {
    fn init_display(&mut self, value: DisplayHandle, retain: bool);
    fn dispose_display(&mut self, value: DisplayHandle, release: bool);
    fn update_display(&mut self);          // _onUpdateDisplay
    fn add_display(&mut self);  fn replace_display(&mut self, prev: DisplayHandle);
    fn remove_display(&mut self);
    fn update_z_order(&mut self);  fn update_visible(&mut self);
    fn update_blend_mode(&mut self);  fn update_color(&mut self);
    fn update_frame(&mut self);  fn update_mesh(&mut self);
    fn update_transform(&mut self);  fn identity_transform(&mut self);
}
/// IArmatureProxy + IEventDispatcher（IArmatureProxy.ts / IEventDispatcher.ts）
pub trait ArmatureProxy {
    fn db_init(&mut self, armature: ArmatureIdx); fn db_clear(&mut self);
    fn db_update(&mut self); fn dispose(&mut self, dispose_proxy: bool);
    fn add_db_event_listener(&mut self, ty: &str, f: Box<dyn Fn(&EventObject)>);
    fn has_db_event_listener(&mut self, ty: &str) -> bool;
    fn dispatch_db_event(&mut self, ty: &str, e: &EventObject);
}
// 阶段 3 交付 NullSlotAdapter / NullProxy（差分用；阶段 7 换 Bevy 实现）
```

`DisplayHandle`：TS 的 display 是引擎对象（raw/mesh/childArmature）；core 不认识它 → `DisplayHandle` 枚举占位：`Raw / Mesh / Armature(ArmatureIdx) / Null`（阶段 7 由适配层映射成 Bevy 实体）。

---

## 2. 字段清单（结构定义，对照 TS 抄）

```rust
// armature/transform_object.rs — TransformObject.ts (155 行)
pub struct TransformObject {
    pub global_transform_matrix: Matrix,   // readonly
    pub global: Transform,                 // readonly
    pub offset: Transform,                 // readonly
    pub origin: Option<Transform>,         // F3.3：init 时 clone 自 data
    pub global_dirty: bool,
    pub alpha: f64,                        // _alpha
    pub global_alpha: f64,                 // _globalAlpha
    // _armature: 不存对象，存 ArmatureIdx 或由外层传参（见任务3 借用策略）
}
// 静态 help 对象（_helpMatrix/_helpTransform/_helpPoint）→ 模块级 thread_local 或参数传入

// armature/bone.rs — Bone.ts (578 行) 字段：
//   继承 TransformObject；+ bone_name: String、parent: Option<BoneIdx>
//   offset_mode: OffsetMode、animation_pose: Transform
//   has_constraint: bool（阶段 3 恒 false）
//   local_dirty: bool、transform_dirty: bool、children_transform_dirty: bool
//   cached_frame_index / cached_frame_indices（阶段 6 前 None，走非缓存分支）
//   缓存字段: inherit_translation/rotation/scale/reflection、bone_type（D-3.3）

// armature/slot.rs — Slot.ts (1316 行) 字段：
//   继承 TransformObject；+ slot_name、parent: BoneIdx（data.parent 名解析）
//   display_index、animation_display_index、visible、blend_mode、z_order、z_index
//   display_frames: Vec<DisplayFrame>, display: Option<DisplayHandle>
//   raw_display / mesh_display: DisplayHandle、child_armature: Option<ArmatureIdx>
//   geometry_data: Option<…>（指向 display frame 内 geometry 的索引/克隆）、texture_data…
//   10+ 个脏标记：display_data_dirty/display_dirty/geometry_dirty/texture_dirty/
//     visible_dirty/blend_mode_dirty/color_dirty/z_order_dirty/transform_dirty + vertices_dirty
//   color: ColorTransform、global 已在 TransformObject

// armature/armature.rs — Armature.ts (941 行) 字段见 D-3.1
```

> Slot 的 `DisplayFrame`（`rawDisplayData / textureData / display / deformVertices`，`Slot.ts` 90-140 附近）与 `init/_updateDisplayData` 的解析流程是任务 5 的重点，**逐行对照**，这里不转述。

---

---

## 任务 1 — `armature/transform_object.rs`（155 行，最简单）

按 §2 字段清单实现 + `_on_clear()`（对照 `TransformObject._onClear` 100-111 行）+ `update_global_transform()`：

```rust
// 仅当 global_dirty 时从矩阵回填 global（TS 135-140，注意这是"读取保证"，动画侧不依赖）
pub fn update_global_transform(&mut self) {
    if self.global_dirty {
        self.global_dirty = false;
        self.global.from_matrix(&self.global_transform_matrix);
    }
}
```

静态 help 对象：TS 用类级静态 `_helpMatrix/_helpTransform/_helpPoint`（跨实例共享、非线程安全）。Rust 建议**不共享**——调用点都是局部一次调用，直接 `Transform::default()` 临时值或传参（性能无差，避免 thread_local 样板）。差异记入对照表（纯实现细节，无行为影响）。

---

## 任务 2 — 容器与索引（D-3.1 / D-3.2 落地）

`BaseFactory` arena 骨架 + `ArmatureIdx` 解析 API：

```rust
impl BaseFactory {
    pub fn new() -> Self;
    pub fn parse_dragon_bones_data(&mut self, raw: &Value, name: &str) -> …;   // 包装阶段2 parser → data_map
    pub fn parse_texture_atlas_data(&mut self, raw: &Value, name: &str) -> …;
    pub fn armature(&self, idx: ArmatureIdx) -> &Armature;
    pub fn armature_mut(&mut self, idx: ArmatureIdx) -> &mut Armature;
    pub fn build_armature(&mut self, armature_name: &str, db_name: &str, skin: &str,
                          atlas_name: &str, proxy: Box<dyn ArmatureProxy>) -> ArmatureIdx;
    pub fn dispose_armature(&mut self, idx: ArmatureIdx);  // on_clear + free.push
}
```
> **勘误（阶段 3 收口）**：`build_armature` 最终实现返回 **`Option<ArmatureIdx>`**（= JS fill 失败返回 `null` 语义）；另 `advance_one(idx, dt)` 公开（= TS 直接 `armature.advanceTime(0)`，build 收尾同款驱动），`advance_time(dt)` 对齐官方 `WorldClock.advanceTime(0)` **早退为 no-op**（`WorldClock.ts:114-116`）。

子骨架（`_buildChildArmature`，`BaseFactory.ts:254-257`）：build 时把 `parent = Some((父Idx, 槽Idx))` 写进子 Armature、`slot.child_armature = Some(子Idx)`——9 素材走不到，配单测（手造 ArmatureDisplayData 走 `_getSlotDisplay` 分支）。

---

## 任务 3 — `armature/bone.rs`（578 行，**本阶段最大单体**）

### 3.1 `init`（TS 345-361）

```rust
fn init(&mut self, bone_data: &BoneData, armature_idx: ArmatureIdx, ctx: &BuildCtx) {
    // _boneData/_armature/_alpha；parent 按 data.parent 名 → ctx.bone_index(name)（F3.2 建完索引后回填）
    self.alpha = bone_data.alpha;
    self.origin = Some(bone_data.transform.clone());   // F3.3 只读 clone
    // D-3.3 缓存: inherit_* 四旗标 + bone_type + length 按需
    ctx.armature.add_bone(self);   // 推入 bones: Vec（顺序 = sortedBones 序）
}
```

### 3.2 `_update_global_transform_matrix`（TS 109-330）——**逐行翻译，给出主干伪代码**

```rust
fn update_global_transform_matrix(&mut self, is_cache: bool, parent: Option<&Bone>, ctx: &Ctx) {
    let flip_x = armature.flip_x;
    let flip_y = armature.flip_y == DragonBones::Y_DOWN();   // F3.4 恒等比较，勿写 &&
    let mut inherit = parent.is_some();
    let mut rotation = 0.0;

    // —— offsetMode 三分支（TS 124-171）：决定 global 初值 ——
    match self.offset_mode {
        OffsetMode::Additive => { /* origin±offset±pose（yDown 决定 y/skew/rotation 的正负号）；
                                    origin==None → copy(offset) 后 !yDown 反号再 add(pose) */ }
        OffsetMode::None     => { /* origin.map(copy.add(pose)) or copy(pose) */ }
        OffsetMode::Override => { inherit = false; copy(offset); !yDown 反号; }   // ← 注意 inherit 被关掉
    }

    if inherit {
        let is_surface = parent.bone_type == BoneType::Surface;
        let surface_bone = if is_surface { parent.surface_bone() } else { None };
        let parent_matrix = if is_surface {
            parent.get_global_transform_matrix(global.x, global.y)   // 双线性（任务4）
        } else { &parent.global_transform_matrix };

        if bone.inherit_scale && (!is_surface || surface_bone.is_some()) {
            if is_surface { /* TS 179-197：rotation 累加 / surface_bone 缩放累乘 /
                               parentMatrix.transformPoint 就地写 global / toMatrix /
                               inheritTranslation 决定 tx,ty 谁覆盖谁 */ }
            else {
                if !bone.inherit_rotation { parent.update_global_transform();
                    // 四种 flip 组合决定 rotation 差分式（TS 203-214，逐条照抄） }
                global.to_matrix(mat);  mat.concat(parent_matrix);
                // inheritTranslation ? global = mat.tx/ty : mat.tx/ty = global.x/y
                if is_cache { global.from_matrix(mat); } else { self.global_dirty = true; }
            }
        } else {
            // ← inherit_scale=false 的完全另一条数学路径（TS 239-299）：
            //   平移：inheritTranslation ? 用 parentMatrix 手乘 : flipX/flipY 反号
            //   旋转：inheritRotation ? parent.updateGlobalTransform 后按 parent.scaleX<0 /
            //         det<0 两个判定累加（det<0 时 rotation -= global.rotation*2 且
            //         (flipX!=flipY || inheritReflection) → global.skew += PI；!yDown 再反号）
            //         : flipX||flipY 的三种组合（含 skew += PI）
            //   末尾 global.to_matrix(mat)
        }
    } else {
        // !inherit（Override 模式或无父骨）：flipX/flipY 反号 + rotation 三分支 + toMatrix（TS 301-329）
    }
}
```

**坑位（我逐段核对过）**：
1. **`inherit_scale` 决定走哪条大分支**（TS 178）——条件是 `inheritScale && (!isSurface || surfaceBone !== null)`，不是单纯 `inheritScale`；两条分支的数学完全不同，漏条件直接矩阵错。
2. `inherit_translation` 在两个分支里的方向相反：继承分支是"**从矩阵取回** global.x/y"（矩阵算出来的世界坐标覆盖局部），不继承分支是"**写进矩阵** tx/ty"（局部值保留、矩阵承担父变换）——语义别弄反（TS 222-229 vs 240-245）。
3. det 判定 `parentMatrix.a*d - b*c < 0` 是**父矩阵行列式**（反射判定），不是 global。
4. `updateGlobalTransform` 调用时机只在读 `parent.global.rotation` 前（惰性回填）。
5. 阶段 1 的 `Matrix::concat` 就是这里用——顺序 `global.to_matrix → concat(parentMatrix)`（先局部后父，TS 219-220）。

### 3.3 `update`（TS 365-441，非缓存路径）

```rust
pub fn update(&mut self, cache_frame_index: i32, bones: &mut [Bone], constraints: &[Constraint], …) {
    // 阶段 3：cache_frame_index 恒 -1 且 has_constraint=false → 直接走 TS 401-415 else 分支：
    //   if transform_dirty || (parent && parent.children_transform_dirty) { cache=-1; transform_dirty=true; cached=-1; }
    // if transform_dirty {
    //     transform_dirty=false; children_transform_dirty=true;
    //     if cached_index < 0 {
    //         if local_dirty { update_global_transform_matrix(false) }   // ← local_dirty 此刻恒 true（见下）
    //         // (isCache 写缓存分支 = P1-4，阶段 6)
    //     } else { /* getCacheFrame 分支 = P1-4 */ }
    // } else if children_transform_dirty { children_transform_dirty=false; }
    // local_dirty = true;   // ★ 尾部重置（F3.6），勿按直觉写成 false
}
```

- **约束钩子**（TS 376-382/402-408）：`if has_constraint { for c in constraints { if c.root == self { c.update() } } }`——~~阶段 3 结构保留、`has_constraint` 恒 false（F3.1），阶段 6 再议借用~~ **勘误：F3.1 已证伪，IK 真身随阶段 3 落地**（`constraint.rs`），借用方案 = `Bone::update(cache, left, right, constraints, flip_x, flip_y)` 三段切片（`split_at_mut` + `split_first_mut`），约束在 root 的 update 内、脏标记消费之前执行，chain 骨靠 F3.2 拓扑序同帧靠后消费。
- `update_by_constraint`（TS 445-455）：照抄（**注意 resolve 放在 `local_dirty` 早退之后**，与 TS:446-447 一致）。
- `invalid_update`（TS 481+）：**勘误**：TS:481-483 **只有一行 `_transformDirty = true`**，不置 local_dirty、不沿子传播（子骨经 update 的 `children_dirty` 链间接传播）。指南任务 6 的"照抄"结论正确、此处旧表述有误。

### 3.4 `_update_alpha`（TS 334-341）

```rust
self.global_alpha = if let Some(p) = parent { self.alpha * p.global_alpha }
                    else { self.alpha * armature.global_alpha };
```

---

## 任务 4 — `armature/surface.rs`（547 行）

- `Surface extends Bone`：Rust 用 **enum 或 `BoneKind::Surface` 载荷 + `surface_bone: BoneIdx` 字段**——阶段 1 定的 `BoneKind` 是 **data 层**；运行时建议 `Bone { kind: BoneRuntimeKind::Surface { segment_x, segment_y, bone: BoneIdx } }`（`_bone` 指向同一 armature 里的宿主骨，TS `Surface._bone`），`as_surface()` 访问。
- **核心：`get_global_transform_matrix(x, y)`（TS 165-230）**——在 surface 局部网格坐标 (x,y) 上做**双线性插值**得到该点的世界矩阵：

```rust
// 伪代码（逐行对照 TS）：
//   u = x / segmentX, v = y / segmentY   （clamp？读原文确认边界处理）
//   四角 = globalTransformMatrix 经 (0,0)/(segX,0)/(segX,segY)/(0,segY) 变换后的 4 组 (a,b,c,d,tx,ty)
//   对 tx,ty（以及 a..d？读原文：只插值 tx/ty 还是整矩阵）按 u,v 双线性混合 → 结果矩阵
//   注意：原地写 self 的临时矩阵（TS 复用 this.globalTransformMatrix？读 165-230 原文确定写入目标）
```
> **实现前先精读 `Surface.ts:165-230`**——插值目标矩阵与四角采样方式我未在本指南展开，防止转述失真；黄金数据（Surface_P1_4 静姿态）会验证。
>
> **勘误（阶段 3 收口）**：上方"双线性插值"猜测**整体不成立**——官方实现是**格子→对角线二分→每三角形 `_getAffineTransform` 拟合仿射**（Surface.ts:66-88/165-437，5 区域 Left→Right→Up→Down→Center），结果返回**静态 help 矩阵的拷贝**、不写 self；另有两处 JS 数组越界靠自动扩容（`_hullCache` 10→12、`_matrixCahce` 在 segmentY>segmentX 的 Left 分区）Rust 必须显式扩容——实测推导与正确分配公式见 `surface.rs` 模块文档（上界 = `max(TS 长, 五区域最大下标+1)`，旧笔记公式错误已修）。

- `Surface::update` 若有覆盖（读 `Surface.ts` 的 update/init 差异部分）一并翻译。
- `Bone::update_global_transform_matrix` 的 `is_surface` 分支依赖 `parent.get_global_transform_matrix(global.x, global.y)`（任务 3 已留口）。

---

## 任务 5 — `armature/slot.rs`（1316 行，第二大单体）

### 5.1 结构与 init（TS 639-677）

```rust
fn init(&mut self, slot_data: &SlotData, armature, raw_display, mesh_display, …) {
    // _slotData/_armature/_alpha；_parent = armature.getBone(slotData.parent)（名 → BoneIdx）
    // display_index = slotData.displayIndex; z_order/z_index/color ← slotData（对齐 TS init 尾部）
    // origin：显示切换时由 _updateDisplayData 赋 clone（TS 461/464，F3.3）
}
```

### 5.2 14 抽象方法 → trait（D-3.4）+ NullAdapter

按 `Slot.ts:338-372` 的 14 个 `protected abstract` 逐个搬进 `SlotRenderAdapter`（阶段 0 的 `jsdump` NullSlot 就是行为参照：`_onUpdateDisplay` 设 `_rd`、其余 no-op）。`Slot` 持 `Box<dyn SlotRenderAdapter>`。

### 5.3 `update`（TS 678-784）——脏标记状态机，逐段翻译

```rust
pub fn update(&mut self, cache_frame_index: i32, parent_bone: &Bone, …) {
    if display_data_dirty { self.update_display_data(); display_data_dirty = false; }   // 解析 displayIndex → DisplayFrame
    if display_dirty      { adapter.update_display(); display_dirty = false; }
    if geometry_dirty || texture_dirty {
        if display 为 raw/mesh { adapter.update_frame(); }   // 14方法之一
        geometry_dirty = false; texture_dirty = false;
    }
    if display.is_none() { return; }                       // ★ displayIndex=-1 提前返回（坑1）
    if visible_dirty { adapter.update_visible(); … }       // …五个 adapter 脏标记段
    if blend_mode_dirty { … } if color_dirty { … } if z_order_dirty { … }

    // mesh 分支（TS 715-740）：geometry 非空且 display==mesh 时
    //   is_skinned = weight 非空; is_surface = parent 是 Surface 骨
    //   vertices_dirty || (is_skinned && is_bones_update()) || (is_surface && parent.children_transform_dirty)
    //     → adapter.update_mesh(); 若 is_skinned||is_surface → return（跳过 transform 段，"Compatible"）
    //   is_bones_update()：逐骨查 transform_dirty（TS 原文读 _bones 循环，逐行翻）

    // 非缓存分支（cache_frame_index 恒 -1）：TS 762-766
    //   if transform_dirty || parent.children_transform_dirty { transform_dirty=true; … }
    // if transform_dirty { update_global_transform_matrix(false); adapter.update_transform(); transform_dirty=false; }
}
```

- Slot 的 `_update_global_transform_matrix`（TS 内 override，读 `Slot.ts` 940/994 附近 `updateTransformAndMatrix` 与骨矩阵 concat 的差异）→ **读原文实现**：槽的 global = origin/displayData.transform 经父骨矩阵变换（与 Bone 不同，无 inherit 分支）。
- `_update_alpha`（TS 374-382）：`global_alpha = alpha * parent._globalAlpha`，且**置 `color_dirty = true`**（副作用别漏）。
- `invalid_update`（TS 795-800）、`update_transform_and_matrix`（TS 804-809）：照抄。
- `replace_raw_display_data / replace_texture_data / replace_display / _set_display_index`（TS 813+ / 1000+）：装配用，按 `_buildSlots` 需要面实现，DisplayFrame 的 `raw_display_data/texture_data/display/deform_vertices` 结构逐行对照 TS 90-140 区域。

---
## 任务 6 — `armature/armature.rs`（941 行）

### 6.1 `advance_time`（TS 295-387，**更新顺序是契约，逐行照抄**）

```rust
pub fn advance_time(&mut self, passed_time: f64, world: &mut ArenaCtx<'_>) {
    if self.lock_update { return; }
    self.lock_update = true;
    // ① 校验 data（TS: armatureData null → warn return；Rust: Arc 已持，不可达 → 跳过，记偏差）
    // ② animation.advance_time(passed_time)          —— 阶段 4 实现，本阶段 no-op 占位
    // ③ 槽排序（TS 315-326）：
    if self.slots_dirty || self.z_index_dirty {
        self.slots.sort_by_key(|s| s.z_index as i64 * 1000 + s.z_order as i64);   // F3.10 stable！
        if self.z_index_dirty { for (i, slot) in slots.iter_mut().enumerate() { slot.set_z_order(i); } }
        self.slots_dirty = false; self.z_index_dirty = false;
    }
    // ④ alpha 传播（TS 328-339）：armature.global_alpha = alpha * (parent_slot.global_alpha | 1.0)
    //    parent 来自 D-3.2 索引（子骨架需 world 句柄传入，split_at_mut 取父）
    if self.alpha_dirty {
        self.alpha_dirty = false;
        // 自身 global_alpha 计算（父 = 父插槽.global_alpha，见 D-3.2；阶段3 顶层骨架恒1.0路径）
        for bone in bones { bone.update_alpha(self.global_alpha); }   // TS 332-338
        for slot in slots { slot.update_alpha(); }
    }
    // ⑤ 骨更新（TS 341-349）：if cache<0 || cache != prev { for i: bones[i].update(cache, …) }
    //    借用策略见下
    // ⑥ 槽更新：同循环序
    // ⑦ action 队列（TS 352-383）：非空时逐条执行（Play → 子骨架/自身 animation.fadeIn，阶段 4 接），
    //    阶段 3 结构保留、恒空；action.return_to_pool → arena 空位复用语义
    // ⑧ lock=false; proxy.db_update()
}
```

**借用策略（坑 4）**：⑤⑥ 要同时读 `bones[i]` 的 parent 与写 `bones[i]`——利用 F3.2（父索引 < 子索引）：

```rust
for i in 0..self.bones.len() {
    let (left, right) = self.bones.split_at_mut(i);
    let cur = &mut right[0];
    let parent: Option<&Bone> = cur.parent.map(|p| &left[p.0 as usize]);   // p < i 必然在 left
    cur.update(-1, parent, /*constraints*/ &[]);
}
```
（若某骨 parent > i → panic 暴露数据序问题，等于免费断言；实测 sortedBones 拓扑序保证不会。）

### 6.2 其余 API

- `invalid_update(bone_name, update_slot)`（TS 406-432）：照抄（名解析 → bone/slot.invalid_update，或全量）。
- `contains_point / intersects_segment`（TS 451-490+）：**范围决策**——requirements 放阶段 3，但真实逻辑是遍历槽交 `slot.contains_point`（内部走 BoundingBox，P1-3 阶段 6 才实现）。本阶段：**方法签名 + 遍历骨架照抄，BoundingBox 内部分返回 false 并标注 P1-3 TODO**（避免阶段 3 膨胀；验收不含它）。
- `get_bones/get_slots/get_bone/get_slot`、`_add_bone/_add_constraint`、`set clock`（TS 889-910，阶段 5 WorldClock 用，字段先留）、`flip_x/flip_y`（TS 705-730，`_flipX/_flipY` 默认 false，来源读原文确认——若来自 armatureData 或 adapter 写入，按原文）。
- `_on_clear`（TS 118-180）：全部字段复位 + 子骨架/槽/骨清空 → arena 空位复用（D-3.1）。

---

## 任务 7 — `factory/base_factory.rs`（1027 行）

```
BuildArmaturePackage { data_name, armature_name, skin, texture_arena_name, dragon_bones: Arc<…>,
                       armature: ArmatureData, skin: SkinData … }   // TS 60-108 区域字段照抄
```

装配主流程（TS 701-716 已读，伪代码）：

```rust
pub fn build_armature(&mut self, armature_name, db_name, skin_name, atlas_name, proxy) -> ArmatureIdx {
    let pkg = fill_build_armature_package(db_name, armature_name, skin_name, atlas_name)?;  // TS 110-166：
    //   解析 dataMap/armature/skin/textureAtlasName；skin 缺失时 autoSearch 回退（逐行）
    let idx = arena_alloc();                               // 空位复用优先（D-3.1）
    let armature = Armature::new(proxy, data.clone(), armature_name);
    build_bones(&pkg, &mut armature);                      // F3.2: 按 sortedBones 序
    build_slots(&pkg, &mut armature);                      // F3.8 流程逐行
    build_constraints(&pkg, &mut armature);                // stub：遍历但只登记 IK/Path 占位（F3.1）
    armature.invalid_update(None, true);                   // TS 712
    armature.advance_time(0.0, …);                         // TS 713 ← F3.11，静姿态就靠它
    idx
}
```

- `_get_slot_display`（TS 258-320）：按 `displayData.type` 分派——Image/Mesh → adapter 的 raw/mesh display；**Armature → `_buildChildArmature` + inheritAnimation + defaultActions + `animation.play()`（TS 289-310，子骨架接线，配单测）**；BoundingBox/Path → raw display 占位。`_get_texture_data` 按 atlas 名查。
- `_build_slot` 三个抽象方法 → **factory 层 trait**（D-3.4 的第三组契约）：

```rust
pub trait FactoryAdapter {   // 阶段 7 Bevy 实现
    fn build_texture_atlas_data(&self, data: Option<TextureAtlasData>, texture: …) -> TextureAtlasData;
    fn build_armature(&self, pkg: &BuildPackage) -> Armature;    // 换 proxy
    fn build_slot(&self, pkg: &BuildPackage, slot_data: &SlotData, armature: …) -> Slot;  // 注入 NullSlotAdapter
}
// 差分/Bevy 共用默认实现 NullFactoryAdapter（= 阶段 0 jsdump 的 NullFactory 语义）
```

- 其余 `replace_*` 系列（replaceDisplay/replaceTextureData/…，TS 720-990）：阶段 3 只实现 `buildArmature` 路径**调用到的**子集（`replace_raw_display_data/replace_texture_data/replace_display/_set_display_index` 已在任务 5）；公开的 `replaceSkin/replaceSlotDisplay` 等是 P1-5（阶段 6），留接口 TODO。

---

## 任务 8 — 静姿态差分基建（本阶段验收工具）

阶段 0 的 golden 是**播放动画后**的逐帧 dump，不能当静姿态基准 → 新工具 `tools/posedump.mjs`（jsdump 变体）：

```js
// tools/posedump.mjs — buildArmature → 不播放 → advance(0) → dump 静姿态
// 用法: node tools/posedump.mjs <skeJson> <texJson> <armatureName> <outJson>
// 复用 jsdump.mjs 的 PIXI 桩 / NullAtlas / NullSlot / EvProxy / NullFactory（导出或复制）
const armature = factory.buildArmature(armName, key, "", key);   // 内部已 advanceTime(0)（F3.11）
armature.clock = factory.clock;
factory._dragonBones.advanceTime(0);        // 再走一遍全局，确保锁/脏标记全收敛
const bones = {}, slots = {};
for (const b of armature.getBones()) bones[b.name] = [
    b.global.x, b.global.y, b.global.rotation, b.global.skew, b.global.scaleX, b.global.scaleY,
    b.globalTransformMatrix.a, b.globalTransformMatrix.b, b.globalTransformMatrix.c,
    b.globalTransformMatrix.d, b.globalTransformMatrix.tx, b.globalTransformMatrix.ty,
    b._globalAlpha,
];
for (const s of armature.getSlots()) slots[s.name] = [
    s.displayIndex, s.visible?1:0, s._zOrder, s._zIndex,
    s._colorTransform.alphaMultiplier, s._colorTransform.redMultiplier,
    s._colorTransform.greenMultiplier, s._colorTransform.blueMultiplier,
    s._colorTransform.alphaOffset, s._colorTransform.redOffset,
    s._colorTransform.greenOffset, s._colorTransform.blueOffset,
    s.global.x, s.global.y, s._globalAlpha,
];
// + slotNames（排序后顺序，F3.10 锚点）+ 每槽 geometry 摘要：
//   geometryData 非空时: vertexCount / triangleCount（阶段2 已锁 IR，这里验证 runtime getter 通路）
fs.writeFileSync(out, JSON.stringify({ meta:{ armature:armName, frameRate, slotNames },
                                       bones, slots }));
```
>
> **勘误（阶段 3 收口）**：
> 1. **元素顺序以 `tools/jsdump.mjs:127-142` 实际输出为准**（bone 13 元是 `x,y,rotation,scaleX,scaleY,skew,…`——skew 在第 6 位非第 4 位；slot 颜色 8 元是 `red,green,blue,alpha` × multiplier/offset——非 alpha 在前），上方草图顺序不采用，`posedump.mjs`/`pose_golden.rs` 均按 jsdump 顺序，保证与阶段 0 golden 同构。
> 2. `factory._dragonBones.advanceTime(0)` 经 `WorldClock.ts:114-116` 早退是 **no-op**（node 实测第二遍逐字节无变化）——收敛全部发生在 buildArmature 内部那遍；posedump 保留该调用仅作官方管线镜像，Rust 侧对应 `BaseFactory::advance_time(0.0)` 早退（驱动用 `advance_one`）。
> 3. 出参另含 `meta.boneNames` 与 `geometry: {槽: [vertexCount, triangleCount]}`（仅 mesh 槽；Surface_P1_4 的几何挂在**骨**上，槽级为 `{}`）。
```

批量：`tools/gen_pose.mjs`（镜像 gen_golden 的 9 素材清单）→ `testdata/pose/<asset>.json`（9 文件，预计 <100KB）。

**Rust 侧测试**（`tests/pose_golden.rs`）伪代码：

```rust
// 对 9 素材：build_armature → advance_time(0.0) → 逐骨逐槽与 golden 比对
// 数值：|got-want| <= 1e-9（JS/Rust 同 f64 表达式序，理论 bit 相等；矩阵乘法序若平台差异
//       放宽到 requirements 的 1e-6——先用 1e-9 跑，失败再降级并记录）
// 骨骼 13 元 [x,y,rotation,skew,scaleX,scaleY, a,b,c,d,tx,ty, globalAlpha]
// 插槽 15 元（同 posedump）
// slotNames：Vec<String> 全序相等（排序结果锚点，F3.10）
// 几何摘要：vertexCount/triangleCount（vertex_count() 走阶段2回填的 root，验证通路）
```

---

## 验收清单（全绿即阶段 3 完成）

```bash
cargo build && cargo test -p dragonbones-core        # 阶段 0-2 回归 + 新增
cargo clippy -p dragonbones-core --tests
node tools/gen_pose.mjs                              # 9 素材静姿态黄金（可重生成，确定性）
# 新增测试:
#   tests/pose_golden.rs      — 9 素材静姿态矩阵/alpha/displayIndex/color 全等 + slotNames 全序
#   tests/armature_unit.rs    — 子骨架接线单测（inheritAnimation/defaultActions 分支）、
#                                sort 稳定性单测（构造 zIndex 平局）、invalid_update 传播、
#                                displayIndex=-1 提前返回
```

**验收标准（对应 P0-6 / P0-7 / M3）**：
1. 9 素材 build + `advance_time(0)` 后，全部骨骼 world 矩阵（a,b,c,d,tx,ty）+ global（x,y,rotation,skew,scaleX,scaleY）+ globalAlpha 与 JS 静姿态差分 ≤ 1e-9（保底 1e-6）
2. 插槽 15 元状态（displayIndex/visible/zOrder/zIndex/color×8/global×2/alpha）全等
3. 运行时槽序 == golden `meta.slotNames`（F3.10 排序锚点）
4. Surface_P1_4 的 `surf` 子骨矩阵全等（双线性插值通路）
5. 5 个含 mesh 素材（龙/Rooster×3/Surface）的 `vertexCount/triangleCount` 与 golden IR 一致（`geometry.root` 通路）
6. 阶段 0-2 全部回归绿

**常见坑速查**（全部实测）：
1. `inherit_scale` 是 `_updateGlobalTransformMatrix` 大分支的复合条件（F3.4 坑1），两条分支数学完全不同。
2. `inherit_translation` 两分支方向相反（矩阵取回 vs 写入矩阵）。
3. `flip_y = flip_y == yDown` 恒等比较，勿化简成 `&&`（F3.4）。
4. `update` 尾部 `local_dirty = true`（F3.6），勿改成 false；四分支别合并。
5. 槽排序用 **stable** `sort_by_key(zIndex*1000 + zOrder)`（F3.10），且 z_index dirty 时回写 `_setZOrder(i)`。
6. 骨骼创建序 = `sortedBones` 拓扑序（F3.2），`split_at_mut` 依赖它——**不要**按 JSON 序建。
7. build 收尾 `invalid_update(null, true)` + `advance_time(0.0)`（F3.11），漏了静姿态不算。
8. `origin` 用 clone（F3.3），但别把 data 层字段也 clone 进运行时造成双份真相。
9. 约束 stub 期 `has_constraint` 恒 false，但**字段与 update 钩子保留**（阶段 6 直接接）。
10. Slot `displayIndex == -1` → update 提前 return（display 为 None）。
11. 子骨架路径 golden 无覆盖（F3.9），必须手写单测。
12. arena dispose = `on_clear` + 空位复用，别再往 stage-1 TypeId 池塞 Armature/Bone/Slot（记对照表偏差）。
13. `DisplayHandle` 别泄漏成引擎类型——core 只认占位枚举（渲染契约隔离，关键结论 2）。

**工期提示**：任务 3（Bone 算法）与任务 5（Slot 状态机）占大头；建议顺序 = 1→2→3→6(骨架跑通)→4→5→7→8，即先让"无槽的骨"过矩阵差分，再上槽，最后装配。

---

## 完成后

跑完验收清单，把 `cargo test` 输出（尤其 `pose_golden` 与 `armature_unit`）+ `gen_pose.mjs` 输出发我。确认全绿后我写**阶段 4（动画系统）**指南——`WorldClock/Animation/AnimationState/BaseTimelineState/TimelineState 16 帧类型/BlendState`，那是 requirements 里工期最长（8-12 天）的阶段，我会先把 `_valueArray` 三基准读取链和 fade 状态机的子状态枚举整理成伪代码。
