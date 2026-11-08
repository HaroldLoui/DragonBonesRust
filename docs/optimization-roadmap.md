# DragonBonesRust 优化路线图

> 状态：规划稿（2026-10-22，2026-10-29 更新）。
>
> 原则：**先度量，再优化**。core 的数值路径已有 JS 黄金数据保护，先不要为了“看起来更快”改语义；优先处理适配层已经明确的热点和产品完整度缺口。

当前判断：

- **代码/性能上最大的空间在 Bevy 适配层**：每 slot 一个 mesh/material、每帧全量同步、CPU 蒙皮/FFD。
- **产品上最大的缺口**：动画状态机/事件、blend mode、换装/child armature、编辑与调试体验。

> **实现状态（2026-10-29）**：P0 与首批 P1 项已落地——
> - **§3 benchmark 基线 + CPU profile**：core 25 个 + 适配层 16 个 criterion benchmark（含 rig 对比与 baseline，见 §3 实测数字）；rig 对比定位到动画场景的适配层开销主要在 CPU 蒙皮/FFD（§1.4、§3.2）；
> - 1.1 slot 索引缓存（`sync.rs` 每 armature 每帧一次 `SlotIdx → order` 映射，去掉每 slot 线性查找）；
> - 1.2 armature 级脏标记（pose hash + root affine 比较，静态角色跳过全量 slot sync，见 §1.2）；
> - 1.3 的 ① 共享材质 + 顶点色（一个 atlas page 一个材质，per-slot 颜色写 `ATTRIBUTE_COLOR`），以及 ②④ 自定义 `Material2d`：按 `(atlas image, blend mode)` 共享材质，Normal / Add / Multiply / Screen 走固定管线混合状态；
> - 1.5 `despawn → dispose` 生命周期与资产/注册回收（`runtime.rs` 的 `ArmatureBinding` + `DragonBonesSystems::Cleanup`）；
> - 1.6 直写 `GlobalTransform` 的 schedule 集成测试（真实 `TransformPlugin`）。
>
> 仍待做：1.4 GPU skinning（本轮 profile 后成为下一个优先项）、1.3 的 ③ 合批、§3.3 的 GPU 侧 profile，以及 §4 的产品项（Overlay 等需要读目标像素的 blend mode 也还在等 shader 方案）。

---

## 1. Bevy 适配层性能

### 1.1 去掉 O(N²) 的 slot 查找（✅ 已实现 2026-10-25）

当前 [`sync.rs`](../crates/dragonbones-bevy/src/sync.rs) 对每个 slot 实体执行：

```rust
armature.slots
    .iter()
    .enumerate()
    .find(|(_, slot)| slot.idx == slot_ref.slot)
```

当前 31–63 个 slot 还不明显，但多骨架/大骨骼时会放大成 O(slot²)。

建议：

- 构建 slot 实体时缓存 `SlotIdx -> (order, Slot)`，或至少缓存 `SlotIdx -> order`；
- 每帧直接按索引取 `armature.slots[order]`；
- `DragonBonesInstance.slots` 目前没有真正用于更新/清理，可以承担这个索引和生命周期管理职责；
- root affine 的 `HashMap` 可以缓存到 armature 实例上，只在新增/移除 armature 时重建。

**已实现**：`sync.rs` 每帧为每个 armature 建一次 `HashMap<SlotIdx, usize>`（来自 core 的 z 排序结果），slot 侧 O(1) 取 `armature.slots[order]`；root affine 仍每帧从 root 实体读取（O(armatures)，成本可忽略）。

### 1.2 全量同步 vs 脏标记（✅ 已实现 2026-10-29）

当前**每帧同步所有 slot** 是刻意的：root `Transform` 移动时 Bevy 的内置传播会把所有子实体的 `GlobalTransform` 重写成占位值，只同步“dirty slot”会让未 dirty 的 slot 塌回 root/identity。

后续可以做的不是“按 slot 的 Changed 判断”，而是：

- **armature 级脏标记**：动画推进、root 移动、slot 显示变化、骨架换装时标记；否则跳过整个 armature；
- 每帧先确认 root `GlobalTransform` 与上一帧相同，且 `animation` 时间未变化，再决定是否全量写；
- 对静态角色可以做到“只在首帧 + root 变化时同步”。

**已实现**：没有把“标记”散落到各个可能修改骨架的地方（那样容易漏），而是在 [`sync.rs`](../crates/dragonbones-bevy/src/sync.rs) 里每帧算一个 **pose hash**（FNV-1a）+ root affine，与 `ArmatureBinding` 里上次同步的值比较；相同就整 armature 跳过。pose hash 覆盖 sync 读取的全部输入：slot 顺序/`SlotIdx`、slot 矩阵、`display_frame`、可见性、`ColorTransform`、`global_alpha`、插值后的 FFD `deform_vertices`、以及全部骨骼矩阵与可见性（蒙皮 mesh 与父骨骼可见性依赖它们）。任何来源的变换（动画推进、play/seek、replace API、用户改 root transform）都会改变 hash 或 root affine，因此不会漏刷新。

- 开关：`DragonBonesSyncConfig { skip_clean_armatures }`（默认 `true`），置 `false` 可强制回到“每帧全量写”；
- 测试：`crates/dragonbones-bevy/tests/lifecycle.rs` 用“篡改 slot `GlobalTransform`”作为探针，验证干净 armature 不重写、动画推进后会重同步、关闭开关后每帧都写。

静态（不推进动画）只跑 `PostUpdate` 的 A/B 实测（2026-10-29 复核；RTX 4080 / i7-14700K，criterion，`cargo bench -p dragonbones-bevy`）。`sync/baseline`（有资产、无 armature）测得一次 `PostUpdate` 的固定开销约 3.2 µs（含 `Material2dPlugin` 在 headless 下的记账系统）；因此下表用 **10→100 armature 的增量斜率**给出每个 armature 的 sync 成本，避免固定开销干扰 A/B 对比：

| 场景 | 关闭脏标记 | 开启脏标记 | 收益 |
|---|---|---|---|
| 每个 idle armature 的 sync 增量 | 1.35 µs | 0.51 µs | −62% |

原始读数（10 / 100 armature）：关闭 41.0 / 162.6 µs，开启 33.0 / 78.6 µs。单个 armature 的一次 `PostUpdate`（约 27 µs）被固定开销主导，不参与对比。

动画中的 armature 每帧都会 dirty，脏标记无法跳过整骨架；Dragon（18 slot）每个 armature 约 3.9 µs（含 core 1.45 µs），Rooster（15 slot，含蒙皮 mesh）约 11.6 µs（含 core 6.3 µs）——蒙皮路径见 §1.4 的 profile 结论。

### 1.3 材质与 draw call

当前 [`render.rs`](../crates/dragonbones-bevy/src/render.rs) 为每个 slot 创建：

- 一个 `Mesh2d`；
- 一个 `ColorMaterial`（现已改为按 `(atlas page, blend mode)` 共享的 `DragonBonesMaterial`）；
- 一次独立的渲染实例候选。

slot 数多时会带来 material 资产数量、draw call 和 batching 压力。建议按收益排序：

1. **共享材质 + 顶点色**（✅ 已实现）：同一个 atlas 的所有 slot 共用一个材质，把 per-slot 颜色烘进 `ATTRIBUTE_COLOR`。`ColorMaterial` 的 shader 已经支持 `VERTEX_COLORS`，这样不再需要每 slot 一个 material。
2. **按 atlas / blend 分组**（✅ 已实现）：一个 `(atlas page, blend mode)` 一个材质；blend mode 通过自定义 `Material2d` 的 `specialize` 区分（同一 blend 的 slot 共享一套 pipeline + material + bind group）。
3. **合批/合并 mesh**：如果 Bevy 的 batching 仍不能合并，可把同一 atlas、同一 z 区间的 slot 合并成一个 mesh；代价是动态顶点更新，需要和“直写 GlobalTransform”的方案做权衡。
4. **自定义 `Material2d`**（✅ 已实现 2026-10-29）：`DragonBonesMaterial` 统一支持 Normal/Add/Multiply/Screen，per-vertex color 继续走 `ATTRIBUTE_COLOR`；blend mode 放进 `AsBindGroup::Data` 参与 pipeline key。Overlay 等需要读 dst 的模式仍需 shader 级混合，暂回退 Normal。

### 1.4 蒙皮与 FFD 的 GPU 化

当前 weighted mesh 在 [`render.rs`](../crates/dragonbones-bevy/src/render.rs) 里每帧 CPU 计算：

```text
for each vertex:
    for each bone weight:
        position += bone_matrix * local_position * weight
```

然后把新的 `ATTRIBUTE_POSITION` 写回 `Mesh` 资产。当前素材只有几百个顶点，CPU 足够，但这是最典型的上限。

**profile 结论（2026-10-29，`cargo bench -p dragonbones-bevy`）**：见 §3.2 的 rig 对比——animated Dragon 约 2.5 µs/armature（18 slot，图集 quad 为主），animated Rooster 约 5.3 µs/armature（15 slot，含 weighted mesh，另有 63 骨骼进入 pose hash）。按 slot 折算，蒙皮 rig 的同步成本约为图片 rig 的 **2.6 倍**，且这部分全部在 CPU（float 循环 + 每帧 `Assets<Mesh>::get_mut` + 顶点重传）。据此下一步优先 **GPU skinning**，而不是先做合批：骨骼矩阵进 storage buffer，顶点 shader 按权重混合，mesh 资产不再逐帧变更。

可选路线：

- **GPU skinning**：骨骼矩阵上传 uniform/storage buffer，顶点 shader 按权重混合；
- **bone matrix texture/SSBO**：避免每帧改 mesh 资产；
- **FFD/deform 数据放 vertex buffer/texture**：避免每帧 `Assets<Mesh>::get_mut`；
- 继续 CPU 更新时：复用 `Vec` 缓冲、避免每帧分配；注意 `Mesh::insert_attribute` 与 render extraction 的时序，必要时使用自定义 render phase / `BufferVec` 管理动态顶点；
- 对重建中的 mesh 维护正确的 `Aabb`，或谨慎使用 `NoFrustumCulling`。

**已实现（2026-10-29）**：CPU 侧先做低风险优化——`WeightedMeshData` / `UnskinnedMeshData` 持有 per-slot 复用的 `positions` scratch buffer，`write_positions` 用 `Mesh::attribute_mut` 原地写顶点，稳态下每帧不再分配 `Vec`、不再替换整个 attribute。实测（`cargo bench -p dragonbones-bevy`，animated）：Rooster 10 armature 140.2 → 133.9 µs（−4.5%），Dragon 不变（其 slot 多为图集 quad，不走蒙皮路径）。

**GPU skinning 暂缓（有据可依）**：profile 显示当前素材每个 armature 的 CPU 蒙皮成本约 5.3 µs（Rooster 15 slot / 63 骨骼），绝对值仍小于一帧预算；而 GPU skinning 需要自定义 vertex shader 复刻 `mesh2d` 的内部 bindings（版本敏感），并把骨骼矩阵做成 storage buffer。等蒙皮顶点数/角色数上来、或 §3.3 的 GPU 侧 profile 证明顶点重传是瓶颈时再做；届时优先在 `DragonBonesMaterial` 上加 vertex shader 分支，而不是另起材质。

### 1.5 生命周期与内存（✅ 已实现 2026-10-25）

当前 `DragonBonesRuntime` 持有 `BaseFactory`，但 root 实体 despawn 时没有对应的 `dispose_armature`；`DragonBonesInstance.slots` 也没有清理逻辑。建议：

- 在 `DragonBonesInstance` 上使用 `OnRemove`/`RemovedComponents` 标记 dispose；
- 下一帧 `advance_time` 时由 core 的延迟回收机制真正释放 arena 对象；
- 同步清理 mesh/material 资产，避免长生命周期 `Assets` 增长；
- 多 armature 共享同一 skeleton/atlas 时，注册缓存需要有引用计数或明确的 ownership；
- 资产加载失败时输出明确错误，而不是静默空转。

**已实现**：`runtime.rs` 为每个 armature 保存 `ArmatureBinding`（root 实体、slot 实体、mesh handle、atlas image、factory 缓存 key）；`DragonBonesSystems::Cleanup` 通过 `RemovedComponents<DragonBonesInstance>` 在 setup/advance 之前回收 mesh/material 资产、递减 `data_map`/`atlas_map` 引用计数并调用 `dispose_armature`（真正的 `on_clear` 由同一帧稍后的 `advance_time` flush）。slot 实体是 root 的 Bevy children（`linked_spawn`），随 root 递归 despawn。

### 1.6 `GlobalTransform` 直写的版本风险（✅ 已有 schedule 集成测试 2026-10-29）

B2 路径依赖：

- `GlobalTransform` 可写，且渲染/裁剪读取完整 affine；
- 写系统必须排在 `TransformSystems::Propagate` 之后、visibility/render extraction 之前；
- `GlobalTransform` 直写不是 Bevy 的公开契约。

建议：

- 增加 schedule 集成测试：同帧可见、裁剪正常、无 order ambiguity；
- Bevy 升级 checklist 中固定检查 `TransformSystems::Propagate`、`VisibilitySystems` 和 `GlobalTransform`；
- 关注上游是否提供官方的 custom/affine local transform 扩展点，出现后可以迁移；
- 文档明确：含 shear 的 GT 不能调用 `compute_transform/rotation/scale`。

**已实现**：`crates/dragonbones-bevy/tests/schedule.rs` 安装了真实 `TransformPlugin`（headless，无渲染），用三个测试锁住关键契约：

1. root 移动后，propagation 会重写子实体的 `GlobalTransform`，适配层必须在**之后**重新写入 slot 的世界矩阵——测试断言 slot 世界矩阵 == root affine × slot 局部矩阵（若 sync 早于 propagate 就会塌成 root affine 而失败）；
2. 蒙皮 slot（Rooster_Ani 的 weighted mesh）的世界矩阵只等于 root affine + 绘制顺序 z；
3. 排在 `DragonBonesSystems::SyncTransforms` 之后的其他 system 能看到适配层写入的值（`Probe` system 断言）。

仍未覆盖：headless 环境下的 visibility/culling 与 render extraction（需要 RenderPlugin/相机），以及 order-ambiguity 审计（Bevy 内部系统本身存在允许的 ambiguity）。Bevy 升级时仍需按 checklist 人工核对这三条。

---

## 2. core 性能

core 以数值正确性优先，不建议在 profile 之前做微优化。可优先关注：

### 2.1 解析与加载

- 当前 JSON 路径使用 `serde_json::Value` + helper，启动/加载期成本较高；
- 如果加载时间重要，可优先走 dbbin/IR，或为 JSON 做更直接的解析路径；
- 图集/骨架解析结果可以缓存，多个 armature 共享同一份 `Arc<DragonBonesData>`。

**2026-11-08 方案 B 实测**：`parse_dragon_bones_data_impl` 里 armature 数组原先先 `.cloned()`
深拷贝整棵 `Value` 树，改为借用 `&[Value]` 后（数据语义不变），同机会话内
`parse/*/object_data_parser`：Dragon 198.7 → 87.0 µs（−56%）、PlantAnimate
1 186.7 → 842.6 µs（−29%）。剩余成本集中在 `dispatch_frame` 的逐 keyframe
`Value` 哈希查找（约 650 ns/keyframe；BoneRotate/Translate/Scale 三类占语料全部
keyframe 的 87%），继续重写 ROI 低，按本节建议优先走 dbbin/IR。

同日第二轮：曲线帧采样原先每帧对 `curve` 做约 14 次二分 Bezier 求值
（`sampling_easing_curve`，约 113 ns/采样），改为按 `(curve, sample_count)` 缓存采样结果
+ `curve/samples` scratch 复用后（缓存按解析器实例隔离，上限 1024 项、单条 ≤256 采样；
29 套素材 21 198 个曲线帧只有 160 组唯一键，命中率极高），`load/batch29/parser_only`
50.3 → **28.4 ms**、`load/batch29/serde_json+parser` 81.5 → **56.3 ms**
（相对最初基线 72.7 / 105.3 ms 为 **−61% / −46%**）。此时完整加载耗时约一半在
`serde_json`（≈29.4 ms），继续压缩需 typed deserialize 或改走 dbbin/IR；
`dispatch_frame` 的逐 keyframe `Value` 访问（约 650 ns/keyframe）仍在，但占比已降到次要。

### 2.2 每帧分配

- `AnimationState`、timeline、BlendState、事件队列在动画推进中可能产生临时 Vec；用 `SmallVec`、复用 scratch buffer、预分配容量；
- 关注 [`timeline_states/update.rs`](../crates/dragonbones-core/src/animation/timeline_states/update.rs) 和 `animation/player.rs` 的热路径；
- 事件在无监听器时可以更早短路，避免构造无用事件对象。

**2026-11-08 方案 B 已完成主要部分**：`BlendStateKey.target_name` 改 `Arc<str>`
（每帧 blend key 克隆不再分配）、`AnimationPlayer` 复用 `state_ids_scratch`
快照、`advance_time_with_context` 借用 animation name、action timeline 改为
`Arc<DragonBonesData>` 借用 actions、`update_groups` 用 `mem::take` 暂借
`state.name/group`、子动画 timeline 结果改为 `mem::take` 而非 clone、world clock
的 path/member 快照用 `BaseFactory` 上的 scratch Vec 复用。结果：稳态
`advance_time` 每帧分配 Dragon **0**、Rooster_Ani **0**、PlantAnimate **2**
（来自显示切换子骨架构建的偶发尖峰，非稳态）。同机会话内 advance 基准
（HEAD → 改后）：Dragon 3.25 → 1.97 µs、PlantAnimate 7.47 → 4.70、
Rooster_Ani 11.70 → 7.29 µs（约 −37%～−40%）。

### 2.3 `Slot` 的渲染适配器抽象

B2 路径不使用 `SlotRenderAdapter` 回调，但 core 仍为每个 `Slot` 保存 `Box<dyn SlotRenderAdapter>`，并走虚调用。可以考虑：

- feature-gate 掉渲染适配器，或提供不装箱的 Null 实现；
- 如果未来仍要支持多引擎适配器，再恢复该层抽象。

### 2.4 `BaseFactory` 的 Send/Sync

`BaseFactory` 包含 `Box<dyn ArmatureProxy>` 和 `Box<dyn EventDispatcher>`，这些 trait object 没有 `Send` 约束，导致 Bevy 侧只能用 `NonSend` 资源，系统固定在主线程。

如果多 armature 并行推进成为瓶颈，可以考虑：

- 给相关 trait 加 `Send + Sync`；
- 或拆分纯数据 core 与适配器状态，让动画推进可以并行；
- 这是结构性改动，应先 profile 确认收益。

### 2.5 矩阵与三角函数

`Transform::to_matrix` / `from_matrix` 在骨骼更新中频繁触发。可以在 profile 后考虑：

- 缓存 `sin/cos`；
- 使用 `mul_add`；
- 减少不必要的 `Transform -> Matrix -> Transform` 往返；
- 但要保持与 JS 参考实现的数值一致性，改动需跑完黄金数据测试。

**2026-11-08 实验（未采用）**：实现 `Matrix::set_from_transform_concat` /
`set_concat`（与 to_matrix+concat、copy_from+concat 逐位等价，含位等价单测）并
做微基准；但跨 crate 基准下融合版反而更慢（10.6 → 16.9 ns），整帧 advance
也无可见差异——收益被内联/寄存器分配吞掉，已回退。矩阵相位只占整帧约 30%
（Dragon bones+slots ≈0.56 / 1.97 µs），下一波应优先动画相位（60–70%）。

**2026-11-08 实验（已采用，收益在噪声内）**：`Transform::from_matrix` 的两个角度与
`Transform::to_matrix` 的非零 rotation/skew 分支改用 `f64::sin_cos`，一次调用同时得到
正余弦，省掉一次角参数规约；数值与分开 `sin`/`cos` 一致，golden/pose/IR 与 536 套语料
全部通过。advance 复测（A/B 同会话）Dragon 2.019→2.039 µs、PlantAnimate 4.621→4.554 µs、
Rooster 7.23 µs 上下波动，差异均在 ±1.5% 噪声内，未能量化出稳定收益——与“矩阵相位约 30%、
其中三角函数只是一小部分”的判断一致。保留该改动（不增加复杂度且语义等价），矩阵相位
的下一波优化应以减少 to_matrix/concat 调用次数为主，而不是三角函数本身。

### 2.6 内存布局与预分配

- bones/slots/timeline 向量按数据规模预分配；
- IR 的 float 数组已经被 f32 量化，core 用 f64 承载；如果内存成为问题，可评估内部 f32 存储 + 边界转换，但会触及黄金容差；
- 当前素材规模较小，优先级低于适配层。

---

## 3. 性能基线（✅ 已建立 2026-10-29）

基准用 `criterion`（workspace 依赖），数据复用 `assets/` 的现有 rig。两侧都已可重复运行：

```text
cargo bench -p dragonbones-core    # 25 个：parse 17 / assemble 3 / advance 5
cargo bench -p dragonbones-bevy    # 16 个：baseline 2 + static A/B × {1,10,100}（6）+ advance-only × {Dragon,Rooster} × {1,10}（4）+ animated × {Dragon,Rooster} × {1,10}（4）
```

> ⚠️ 只在**机器静默**时记录基线：并发 rustc 负载会让 criterion 数字通胀 1.2–1.9× 并产生 severe outliers。criterion 会在 `target/criterion/` 留 baseline，被污染的 run 会让后续 comparison 误报 regression。

### 3.1 core（i7-14700K，单核，criterion median）

| benchmark | median |
|---|---|
| `advance/Dragon/factory_advance_time`（1/60 s） | 3.327 µs（10-29: 3.314） |
| `advance/PlantAnimate/factory_advance_time` | 4.099 µs（4.047） |
| `advance/Rooster_Ani/factory_advance_time` | **6.312 µs**（6.933，−8.6%） |
| `advance/Dragon/factory_advance_one` | 3.320 µs（3.233） |
| `advance/Rooster_Ani/factory_advance_one` | **6.376 µs**（6.868，−5.9%） |
| `assemble/*/factory_parse_build_armature`（ske→atlas→build） | 227.7 µs / 797.5 µs / 1.136 ms（Dragon / Plant / Rooster） |
| `parse/*/serde_json+object_data_parser`（完整解析） | 343.9 µs / 1.078 ms / 1.897 ms（Dragon / Plant / Rooster） |
| `parse/*/object_data_parser`（Value 已解析） | 192.1 µs / 724.4 µs / 1.010 ms |
| `parse/assets/*` 吞吐 | Dragon 约 50 MiB/s、龙 71.3 MiB/s、Rooster 约 61.7 MiB/s、PlantAnimate 32.0 MiB/s |

**2026-10-31 复测**（同机静默）：bone 目标查找由 `bones.iter().find(|b| b.idx == target)` 改为下标直取（`BoneIdx` 是创建序 = `armature.bones` 下标，未排序），Rooster（63 骨）的 advance 由 6.90/6.78 µs 降到 6.31/6.38 µs，Dragon/Plant 在噪声内（golden 全绿，无数值变化）。slot 目标查找保持线性：槽会按 zOrder 重排，下标 ≠ `SlotIdx`，需要每帧重建位置表；当前 rig 槽数 ≤18，收益不足以抵消复杂度。`cached_frame_indices` 共享化（审计建议 `Arc`）未做：官方该表是共享**可变**缓存（Bone.ts:393 / Slot.ts:712 回填），Rust 按骨架克隆以避免生命周期与并发问题，且只在 `animation_dirty` 时挂载，不是每帧分配，故记录为已知取舍（D-6.3）。

**读数**：动画推进是 3–7 µs/帧量级，解析/装配是百微秒到毫秒量级。对“加载后一直播”的游戏用法，瓶颈明确在适配层（sync/材质/蒙皮），解析只在场景切换时付一次；若要缩短加载，优先走 dbbin/IR（见 2.1）。

#### 3.1.1 方案 B 补充基线（2026-11-08，`cargo bench -p dragonbones-core --bench baseline` / `--bench allocations`）

批量加载（`assets/` 全部 29 套，单进程串行；`baseline` 组，`--sample-size 10`）：

| benchmark | median | 吞吐 |
|---|---|---|
| `load/batch29/serde_json_only`（只把 ske 解析成 `Value`） | 30.3 ms | 110.4 MiB/s |
| `load/batch29/parser_only`（`Value` 已就绪，只跑 parser） | **72.7 ms** | 46.0 MiB/s |
| `load/batch29/serde_json+parser`（完整加载路径） | **105.3 ms** | 31.8 MiB/s |
| `load/batch29/single/Dragon` | 326.0 µs | 53.1 MiB/s |
| `load/batch29/single/PlantAnimate` | 1.053 ms | 33.8 MiB/s |
| `load/batch29/single/Rooster_Ani` | 1.795 ms | 66.1 MiB/s |

dbbin 二进制入口（`dbbin` 组，只测 `parse_binary_dragon_bones_data`）：

| benchmark | median | 吞吐 |
|---|---|---|
| `dbbin/mecha_1002_101d_show_ske` | 174.6 µs | 57.8 MiB/s |
| `dbbin/mecha_1406_ske` | 187.3 µs | 124.1 MiB/s |

分配基线（`cargo bench -p dragonbones-core --bench allocations`，计数 allocator；`adv/f` = 稳态 `advance_time(1/60)` 每帧）：

| rig | parse allocs / bytes | build allocs / bytes | adv allocs/frame | adv B/frame |
|---|---|---|---|---|
| Dragon | 6 248 / 646 KB | 3 955 / 473 KB | 57 | 367 |
| PlantAnimate | 13 518 / 1.56 MB | 8 527 / 1.11 MB | 58 | 11 362 |
| Rooster_Ani | 28 090 / 4.28 MB | 15 832 / 2.41 MB | 99 | 1 345 |
| 29 套 assets 全量加载 | 1 189 204 / 145.6 MB | — | — | — |

**读数**：解析路径里 serde_json 只占约 29%（30.3/105.3 ms），**parser 本身占约 69%**；因此 §2.1 的收益不能只靠换 JSON 后端，必须同时削减 parser 的逐字段 `Value` 访问与临时分配（单个 rig 平均 4 万次分配、5 MB）。动画推进的每帧分配仍为几十到一百次，§2.2 有明确目标（先归零 Dragon/PlantAnimate 的稳态分配）。

> 测量注意：本表在非静默机器上采一次，绝对值只用于本轮改动的前后对比；与 10-29 基线（advance/Dragon 3.327 µs）的差异在机器波动范围内。

### 3.2 Bevy 适配层（无渲染 headless `App`）

`sync/baseline/*`（有资产、无 armature）测 `PostUpdate` 固定开销；`sync/static/*` 只跑 `PostUpdate`（不推进动画）；`sync/animated/*` 每次先 `advance_time(1/60)` 再同步；`advance/*` 只推进动画（纯 core 成本）。2026-10-29 复测：

| 场景 | 耗时 |
|---|---|
| `sync/baseline`（无 armature，跑一次 `PostUpdate`） | 3.2 µs |
| static 10 / 100 armature，脏标记关 | 41.0 µs / 162.6 µs（增量 1.35 µs/armature） |
| static 10 / 100 armature，脏标记开 | 33.0 µs / 78.6 µs（增量 0.51 µs/armature） |
| animated Dragon 1 / 10 armature | 30.6 µs / 65.8 µs（3.9 µs/armature） |
| advance-only Dragon 1 / 10 | 1.48 µs / 14.5 µs（1.45 µs/armature） |
| animated Rooster 1 / 10 armature | 36.0 µs / 140.2 µs（11.6 µs/armature） |
| advance-only Rooster 1 / 10 | 6.36 µs / 62.8 µs（6.27 µs/armature） |

**2026-10-31 修复后复测（P1 技术债修复）**：本次机器比 10-29 慢，用同机 `advance-only Dragon 10`（27.0 µs vs 基线 14.5 µs，≈1.86×）与 core `advance_time`（5.6 µs vs 3.31 µs，≈1.69×）归一化，折算系数取 ≈1.75×：

| 场景（归一化后） | 10-29 基线 | 修复前（本会话实测） | 修复后 |
|---|---|---|---|
| static 100 armature，脏标记开 | 78.6 µs | ≈116 µs | **≈88 µs** |
| static 10 armature，脏标记开 | 33.0 µs | ≈38 µs | **≈28 µs** |
| static 100 armature，脏标记关 | 162.6 µs | ≈223 µs | ≈221 µs（该路径本就逐槽同步） |

修复点：`slot_orders` 只对 dirty armature（以及有 child armature 的父 armature）构建，无 child 的常见场景完全跳过子骨架解析；同时把 `held_materials` 快照 clone 与 `new_materials` 提交表删掉，改为直接读写 binding 的材质键集。剩余约 10–12% 开销来自新增的内容指纹（`hash.rs`，纯数值、无字符串），它换来 `replace_texture_data`/`replace_skin` 这类“不改 display 索引只改内容”的运行时替换能当帧刷新。

**2026-10-31 C6（每帧分配 / 内容版本号）复测**（同机 `advance-only Dragon 10` = 14.4 µs，等于 10-29 基线，无需归一化）：

| 场景 | 10-29 基线 | C6 前（本会话） | C6 后 |
|---|---|---|---|
| static 10 armature，脏标记开 | 33.0 µs | 31.8 µs | **32.8 µs** |
| static 100 armature，脏标记开 | 78.6 µs | 99.3 µs | **81.9 µs（+4%）** |
| static 100 armature，脏标记关 | 162.6 µs | 259.0 µs | 199.5 µs（+23%） |
| animated Dragon 1 armature | 30.6 µs | 28.3 µs | 28.8 µs |

改动：事件队列从 `Arc<Mutex<Vec<_>>>` 改为 NonSend `Rc<RefCell<_>>`（core 工厂本来就在 NonSend 资源里，每事件不再加锁）；sync 的 7 个 `HashMap/HashSet/Vec` 改为 `Local<SyncScratch>` 跨帧复用；更关键的是去掉每帧逐 slot 的内容哈希——core `Slot` 增加 `content_revision`（Rust 侧簿记，`_updateDisplayData` 检测到 display/geometry/texture 内容变化时自增，**不改变 TS 语义/数值**），Bevy 侧只 hash 这个计数器，`replace_texture_data`/`replace_skin` 的当帧刷新能力保持不变。`skip_off` 路径仍有约 23% 的额外成本（每槽都走完整同步 + revision 指纹 + 可见性/材质逻辑），默认 `skip_clean_armatures = true` 的热路径已回到基线 ±4%。

**读数**：`animated` 减去 `advance` 得到适配层每 armature 的 sync 成本——Dragon（18 slot，图集 quad 为主）约 2.5 µs，Rooster（15 slot，含 weighted mesh）约 5.3 µs，**按 slot 折算蒙皮路径贵约 2.6 倍**，即 CPU 蒙皮/FFD 是动画场景下适配层的主要开销（见 §1.4 的 GPU skinning 决策）。core `advance_time` 在 Rooster（63 骨骼）上 6.3 µs/armature，已超过适配层 sync，属于 §2 的 core 动画推进本身。

### 3.3 待补的基线

1. **渲染 profile**（🟡 CPU 侧已完成 2026-10-29）：用 rig 对比（Dragon 图片 slot vs Rooster 蒙皮 slot）定位到动画场景的适配层开销主要在 CPU 蒙皮/FFD（§3.2）；GPU 侧（Tracy / GPU capture：draw call、fill rate）仍待做，需要带渲染器的 profile 环境；
2. **内存 profile**：armature/mesh/material 生命周期（1.5 已补 dispose，仍需量化长期运行是否平稳）；
3. **回归门槛**：目前 CI 只保证 benchmark 能编译（`clippy --all-targets`），数字仍靠手动记录；后续可加 benchmark job 或定期存档 `target/criterion` 结果。

---

### 3.4 方案 B 前后对照（2026-11-08）

> 所有数字均为**同机、同一会话内背靠背测量**（HEAD stash 后立即复测），
> 绝对值随 CPU 状态漂移，只用于对比；`advance` 用 criterion 20 samples。

解析（`cargo bench -p dragonbones-core --bench parse`，只取 parser 相位）：

| benchmark | HEAD | 方案 B 后 | 变化 |
|---|---|---|---|
| `parse/Dragon/object_data_parser` | 198.7 µs | 87.0 µs | −56% |
| `parse/PlantAnimate/object_data_parser` | 1 186.7 µs | 842.6 µs | −29% |

批量加载（`cargo bench -p dragonbones-core --bench baseline`，29 套 assets 全量）：

| benchmark | 最初基线 | 方案 B 后 | 变化 |
|---|---|---|---|
| `load/batch29/serde_json_only` | 30.3 ms | 29.4 ms | ≈0% |
| `load/batch29/parser_only` | 72.7 ms | 28.4 ms | −61% |
| `load/batch29/serde_json+parser` | 105.3 ms | 56.3 ms | −46% |

动画推进（`cargo bench -p dragonbones-core --bench advance`）：

| benchmark | HEAD | 方案 B 后 | 变化 |
|---|---|---|---|
| `advance/Dragon/factory_advance_time` | 3.25 µs | 1.97 µs | −39% |
| `advance/Dragon/factory_advance_one` | 6.08 µs | 3.53 µs | −42% |
| `advance/PlantAnimate/factory_advance_time` | 7.47 µs | 4.70 µs | −37% |
| `advance/Rooster_Ani/factory_advance_time` | 11.70 µs | 7.29 µs | −38% |
| `advance/Rooster_Ani/factory_advance_one` | 11.56 µs | 7.02 µs | −39% |

每帧分配（`cargo bench -p dragonbones-core --bench allocations`）：

| rig | 改前 adv allocs/frame | 改后 adv allocs/frame |
|---|---|---|
| Dragon | 57 | **0** |
| PlantAnimate | 58 | 2（显示切换尖峰） |
| Rooster_Ani | 99 | **0** |

剩余热点（临时相位插桩，已移除）：动画时间轴更新占整帧 60–70%（Dragon
≈1.74 µs、PlantAnimate ≈1.81 µs），矩阵相位 bones+slots 占约 30%；
加载侧剩余成本约一半在 `serde_json`（≈29.4 ms），其余为 `dispatch_frame` 的逐帧
`Value` 访问。进一步收益需要动 `update_groups` 的逐 timeline 混合状态查表、
`dispatch_frame` 的逐帧 `Value` 访问或 typed deserialize，风险高于本轮。

语料审计（`crates/dragonbones-core/tests/corpus_audit.rs`，536 套）在本轮解析优化后
从 30.0 s 降到 **27.7 s**，仍保持 536/536 通过、JS 对拍 39 一致 / 0 不一致。

## 4. 产品层优化

### 4.1 动画运行时能力

当前 demo 是“播放一个动画 + UI 切换”，产品化还需要：

- `DragonBonesPlayer` / `AnimationPlayer` 风格的组件 API：play/pause/stop/seek/speed/loop/fade（✅ 部分实现 2026-10-29）：`DragonBonesCommand` message + `DragonBonesPlayConfig`，支持 play / play_config（fade_in、起始 position、time_scale、play_times）/ stop / pause / resume / set_speed，命令在 `DragonBonesSystems::Advance` 里先于本帧 advance 应用；运行中 seek（不重启动画改播放头）与 `AnimationPlayer` 式时间轴拖动仍待做；
- 动画状态机：layer、group、fade in/out、transition、priority；
- 动画事件到 Bevy `Message` 的桥接（✅ 已实现 2026-10-29）：`BevyArmatureProxy` 把 core 派发的每个 `EventObject` 记录到共享队列，`forward_dragonbones_events` 转成 `DragonBonesEvent` message（start / complete / loopComplete / fadeIn* / frameEvent / soundEvent，携带 armature 索引与完整事件字段）；参见下文 §4.1 说明；
- 多 armature 同时播放、独立时间缩放；
- 与 Bevy `Time`、pause、fixed update 的统一调度。

**事件桥接实现细节**（2026-10-29）：core 只在 `proxy.has_db_event_listener(type)` 为真时才创建/缓冲事件，因此 `BevyArmatureProxy::has_db_event_listener` 恒返回 `true`（"Bevy 桥永久监听"）；普通监听器注册仍委托给 `NullArmatureProxy`。事件在同一帧的 `DragonBonesSystems::Advance` 内被转发，消费方用 `MessageReader<DragonBonesEvent>` 读取。集成测试见 `crates/dragonbones-bevy/tests/events.rs`（loopComplete、armature 归属、队列不重放）。

**动画命令 API 实现细节**（2026-10-29）：游戏代码不接触 non-send 的 `DragonBonesRuntime`，而是向 armature root 实体发 `DragonBonesCommand` message；`apply_dragonbones_commands` 在 `Update` 的 `Advance` 集合里先于 `advance_dragonbones` 运行，因此 play/stop 当帧生效。pause/resume/set_speed 通过设置当前所有 `AnimationState::time_scale` 实现（Pause=0，Resume=1，速度可再调）；`play_config` 暴露 core `AnimationConfig` 的 fade_in_time / position / time_scale / play_times。demo 的键盘切动画已改为使用该 API（dogfooding）。测试见 `crates/dragonbones-bevy/tests/player.rs`。

### 4.2 渲染完整度

按用户可见度排序：

- **blend mode**（🟡 部分完成，2026-10-29）：Normal / Add / Multiply / Screen 已由自定义 `Material2d` 支持——用固定管线混合状态实现，材质按 `(atlas image, blend)` 共享（一套 pipeline 对应一种 blend）；Overlay（以及 Darken / Lighten / HardLight / Difference / Subtract 等需要读目标像素的模式）仍回退为 Normal，等待 shader 级混合方案；
- **rotated atlas region**（✅ 已实现 2026-10-29）：`rotated: true` 的图集区域按参考适配器语义渲染——内容尺寸取旋转后的 region（宽高互换，仅无 trim frame 时，对齐 TS `Slot.ts:422-424`），UV 用 `u' = 1 - v, v' = u` 旋转（对齐 `PhaserSlot.ts:174-182` / `CocosSlot.ts:221-229`）；贴图/蒙皮 mesh 的 UV 都走同一个 `atlas_uv` 映射。实现见 `render.rs`，测试见 `tests/rotated_atlas.rs`（合成 rotated 骨架 + 图集，端到端断言顶点与 UV）；
- **Surface mesh** 的逐顶点变形（✅ 已实现 2026-10-29）：父骨是 Surface 的 mesh display 现在按参考适配器语义渲染——slot 用 armature-space（identity local transform），每个顶点经 `Surface._getGlobalTransformMatrix(x, y)` 变换到 armature 空间，并叠加 FFD deform（对齐 `EgretSlot.ts:504-522`）；Surface 晶格本身由 core 计算，adapter 每帧按骨架 dirty 重算。实现见 `render.rs` 的 `surface_positions` / `update_surface_positions`，测试见 `tests/surface_mesh.rs`（合成 surface 骨架，静态 warp + 随父骨动画逐帧更新）；
- **child armature** 与嵌套骨架（✅ 已实现 2026-10-29）：core 在 build 时通过 `_buildChildArmature` 建子骨架；adapter 为每个子骨架实例在“显示它的 slot 实体”下 spawn 一个嵌套 root 实体（`DragonBonesChildArmature` 组件记录父 slot/父骨架），子骨架世界矩阵 = 该 slot 的世界矩阵（按 core slot 数据独立解析，递归支持多层嵌套），可见性继承 `父骨架可见 && 父 slot 可见`；despawn 顶层 root 时递归释放整个子树的 mesh/material/注册引用（core 的 dispose BFS 负责 arena）。实现见 `runtime.rs`（spawn/cleanup）与 `sync.rs`（子骨架仿射解析），测试见 `tests/child_armature.rs`（合成 Parent→Child 骨架，断言嵌套实体/世界矩阵/子树资产回收）；**已知限制**：core 为子骨架注入的是 `NullArmatureProxy`，故子骨架的动画事件暂不会转发到 Bevy `Message`（需要 core 开放 proxy 注入才能补齐），child root 也不支持单独 despawn（随父 root 生命周期）；
- replace texture / replace skin / 换装；
- 更精确的 zOrder / zIndex 排序；
- 颜色 offset（当前只支持乘子）；
- 多 atlas、多材质、贴图热重载。

### 4.3 ECS 集成与编辑体验

- slot/bone 实体层级与生命周期管理；
- 把特效、粒子、UI 挂到 bone 上；
- 骨骼/插槽调试视图（pivot、AABB、z 顺序、当前动画）；
- 动画列表改成可点击 UI（现在只支持键盘）；
- 编辑器内预览：时间轴拖动、动画混合、换肤；
- 资产导入/校验工具：检查缺失贴图、空显示、异常 transform。

### 4.4 发布与工程质量

- CI：`fmt` / `clippy` / `test` / wasm build / golden 回归；
- benchmark 回归；
- 截图对比（和官方 DragonBones demo 对齐）；
- `crates.io` 发布、docs.rs、semver 与 Bevy 升级策略；
- 示例工程 / WebAssembly demo；
- submodule 初始化已经写进 README，CI 需要处理 `--recurse-submodules`。

---

## 5. 建议优先级

| 优先级 | 事项 | 预期收益 | 成本 |
|---|---|---|---|
| P0 | 建 benchmark/profile 基线（✅ core + 适配层 benchmark 已完成）；清理 despawn 生命周期（✅ 已完成） | 避免盲目优化和内存泄漏 | 低 |
| P1 | slot 索引缓存，去掉 O(N²) 查找；共享 material + 顶点色（✅ 已完成） | 直接降低多骨架开销和 draw 成本 | 中 |
| P1 | 自定义 `Material2d`，支持 blend + 统一材质 | 产品完整度 + 批处理 | 中 |
| P1 | GPU skinning / FFD | 大 mesh、多角色时收益最大 | 高 |
| P2 | 动画状态机、事件桥接、换装、child armature | 产品可用性 | 高 |
| P2 | 调试视图、可点击 UI、编辑器预览 | 开发和美术效率 | 中 |
| P3 | parser/动画热路径微优化、`Send` 化、内存布局 | 取决于 profile 结果 | 中高 |

如果目标是“先能用于实际游戏”，建议按 **生命周期清理 → 共享材质/顶点色 → 动画事件/状态机 → blend mode** 的顺序；如果目标是“长期高性能库”，再投入 **GPU skinning、合批、core `Send` 化和 benchmark 回归**。
