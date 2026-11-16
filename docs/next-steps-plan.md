# 后续方案规划（v2）

> 日期：2026-11-16 · 基线：`main` `2bab88a`（含 demo 预览器 `d42af3a`）
> 性质：规划文档，未提交。
> v2 依据 2026-11-16 代码审计修订：A 拆为 A1/A2、修正“core 换装已就绪”的结论、C1 改为兼容 Bevy 传播规则的设计、E2 改为多图集模型、G1 修正语料 CI 依赖，并新增 §7 core 完成度评估。
> 排序原则：先能用于实际游戏，再谈长期性能库；core 只收口“游戏运行时必需”，性能/架构优化另列。

## 0. 结论速览

| 顺序 | 方案 | 收益 | 成本 | 建议 |
|---|---|---|---|---|
| 1 | A1. 同图集换装 / 替换纹理链（core + adapter） | 皮肤、武器、受击换图，产品可用性核心 | S–M（3–4 天） | 先做 |
| 2 | B. 播放控制 API 补全（seek / loop / layer / fade + slot/armature 命令） | 把 demo 临时实现沉淀成正式 API | S（2–3 天） | 与 A1 并行或先做 |
| 3 | D. child armature 事件转发 | 嵌套骨架事件完整性 | S（2–3 天） | A1 后 |
| 4 | C2. bone 只读查询 / 挂点 MVP | 特效、UI、武器跟随 | S（1–2 天） | 有需求就做 |
| 5 | G1. CI 语料策略 + bench smoke | 防回退 | S | 并行做 |
| 6 | A2. 跨图集换装（per-slot 图集身份） | 多 tex 皮肤 / 多图集素材 | M（3–5 天） | 素材或皮肤需要时 |
| 7 | C1. bone 实体 + attachment sync | 大量挂点、精确 affine | M（3–4 天） | C2 不够时 |
| 8 | E. 渲染长尾（color offset / 多图集渲染 / blend） | 素材兼容性 | M–L | 按素材触发 |
| 9 | F. 性能量化 → GPU skinning / 合批 | 多角色场景上限 | S→L | 先量化再决定 |
| 10 | H. 内容与工具（更多 rig、校验、demo 拆分） | 开发效率 | S | 有余力就做 |

## 1. 现状基线

**core**

- DragonBones 5.5 JSON + dbbin 解析；IR / 黄金数据 / 姿态 / 矩阵 / 事件差分齐全（`tests/golden_*`、`ir_golden`、`dbbin`）。
- 37 个 TS 文件全部有 Rust 映射（`BaseObject` 对象池按已知偏差 D-4.1 刻意不移植，其余均有对应实现或等价路径）；IK / Path 约束、BoundingBox、`cacheFrameRate`、脏标记语义、`UserData` / `ActionData`、子骨架构建与动作播放均已落地。
- 换装 API 在**数据层**落地：`BaseFactory::replace_display / replace_slot_display(_list) / replace_skin / replace_animation`，`Slot::replace_texture_data(_with_atlas_scale) / replace_display(_data)`（`tests/replace.rs`）。
- 语料审计：本地 `_research/` 中 541 个 `*_ske.json`，536 个可测（空骨架跳过）全部通过；JS 对拍默认抽样 40 个（报告记录 39 一致 / 0 不一致）。`tests/corpus_audit.rs` 为 `#[ignore]`，依赖 `_research/` + Node + `DragonBonesJS` 子模块。
- 已知未收口：replace 路径不解析 `texture_data`；`_replaceTextureAtlasData` / `update_texture_atlases` / 数据删除 API / `get_*` 包装仍是 TODO；子骨架 proxy 仍为 `NullArmatureProxy`；`AnimationState::set_current_time` 仍是 `pub(crate)`。

**适配层（`dragonbones-bevy`）**

- 资产加载（`_ske.json` / `_tex.json` / `_tex.png`），每 slot 一个 mesh 实体，世界矩阵直写 `GlobalTransform`（保留 skew）。
- blend：Normal / Add / Multiply / Screen（固定管线混合）；Overlay / Darken / Lighten / HardLight / Difference / Subtract 等回退 Normal。
- rotated atlas region、Surface mesh 顶点变形、child armature 嵌套、动画事件 → Bevy message、`DragonBonesCommand`（play / play_config / stop / pause / resume / set_speed）、脏检查与共享材质。
- 已有换装基础：sync 通过 `content_revision` 指纹发现运行时 `replace_texture_data` / `replace_skin` 并重建 mesh（`tests/lifecycle.rs` 已覆盖 slot 级 `replace_texture_data`）。
- **已知缺口**：`AtlasAsset` 单图且一个 armature 只用一个 atlas image；`replace_display` / `replace_skin` 的渲染数据链未闭环；颜色只有乘子；child armature 无事件；无换装/seek/layer/group/tint 命令；bone 不是 ECS 实体；`DragonBonesPlayConfig::time_scale` 默认 1.0 会覆盖播放状态。

**demo**

- 预览器：可点击动画列表、暂停/调速/循环、进度条 seek、F1 骨骼 gizmos、x9 压测、状态行（`feat/demo-previewer` → `d42af3a`）。
- 压测实测：9 个 PlantAnimate 实例 + gizmos ≈ **142–151 fps**（RTX 4080，1296×731 窗口）。
- `crates/dragonbones-demo/src/main.rs` 已 1918 行，继续叠加换肤/播放控制 UI 前建议先拆模块。

**性能基线（2026-11-08）**

| 指标 | 数值 |
|---|---|
| parse batch29 / parser_only | 56.3 ms / 28.4 ms |
| advance Dragon / Plant / Rooster | 1.599 / 4.169 / 6.206 µs |
| 每帧分配 | 0 / 2 / 0 |

**CI**：fmt、clippy、test（Linux 窗口依赖）三个 job；无 wasm、无 bench 回归；`corpus_audit` 不在 CI。

## 2. 缺口清单

| 缺口 | 证据 | 影响 | 归属 |
|---|---|---|---|
| replace 不刷新纹理数据 | `replace_skin` / `replace_slot_display` / `replace_display` 只改 display data + handle；`_get_texture_data` 只在 build 调用（[base_factory.rs:1448](../crates/dragonbones-core/src/factory/base_factory.rs#L1448)）；适配层渲染读 `slot.texture_data`（[render.rs:284](../crates/dragonbones-bevy/src/render.rs#L284)） | 换 skin / 换显示后纹理与 UV 不变，`content_revision` 也不 bump | core（A1） |
| 单图集 armature | 所有 slot 共用一个 `atlas_texture` / `atlas_size`（[runtime.rs:467](../crates/dragonbones-bevy/src/runtime.rs#L467)）；`AtlasAsset` 单 `Handle<Image>` | 跨 tex.json 的皮肤/素材无法加载 | adapter（A2） |
| 无 per-slot 图集身份 | `DisplayFrame` 只有 `texture_atlas_scale`，没有 atlas id；`_get_texture_data` 只返回 texture + scale | 多图集时 slot 找不到正确 image | core（A2） |
| 无运行中 seek / loop / layer 命令 | demo 用 `PlayConfig::with_position` 重启；`set_current_time` 为 `pub(crate)` | 播放状态被重置，demo 速度会悄悄回 1.0 | core + adapter（B） |
| bone 不可挂载 | adapter 只 spawn slot 实体 | 特效/粒子/UI 无法跟骨骼 | adapter（C） |
| child 骨架无事件 | 子骨架注入 `NullArmatureProxy`（[base_factory.rs:1530](../crates/dragonbones-core/src/factory/base_factory.rs#L1530)） | 嵌套骨架 complete/frameEvent 丢失 | core + adapter（D） |
| 颜色 offset 丢失 | `color_from_transform` 只读 multiplier（[math.rs:45](../crates/dragonbones-bevy/src/math.rs#L45)） | 受击闪白/变暗无法表达 | adapter（E） |
| slot tint / visible / armature alpha / flip 无命令 | `DragonBonesCommand` 只有播放动作 | 隐藏武器、受击闪、朝向切换都需绕过 API 直接改 core | adapter（B） |
| 跨骨架排序 / ysort 未定义 | `slot_z` 只按骨架内 order（[math.rs:56](../crates/dragonbones-bevy/src/math.rs#L56)）；root z 由用户管 | 多个骨架同 z 时前后不保证 | adapter（E） |
| blend 长尾回退 | `blend.rs` 测试显式断言回退 | Overlay 类素材观感不一致（当前语料几乎不用） | adapter（E） |
| 数据管理 API 缺失 | `update_texture_atlases`、`get_dragon_bones_data`、`get_texture_atlas_data`、`remove_*`、`clear` 未实现；`tests/replace.rs` 只断言 data 名字 | 运行时热更/卸载/清理无从下手 | core（A2/G） |
| 语料 CI 依赖不成立 | `_research/` 被 `.gitignore` 忽略且约 2GB，另需 Node + `DragonBonesJS` | CI 无法直接跑 536 语料 | 工程（G1） |
| 性能上限未量化 | 只有 9 实例、小 mesh 数据 | 无法判断 GPU skinning / 合批的投入回报 | 工程（F） |
| demo 单文件膨胀 | `main.rs` 1918 行 | 再加 UI 会难以维护 | 工程（H） |

## 3. 候选方案

### 3.1 A1. 同图集换装 / 替换纹理链（先做）

**目标**：让游戏在运行时安全地换皮肤、换贴图、换显示对象，不重建骨架；先把**同一张 atlas 图**的路径做完整、可验证。

**设计要点**

- core 补渲染数据链：
  - 在 `replace_skin` / `replace_slot_display` / `replace_display` 的 Image/Mesh 分支里，用 `_get_texture_data` 解析 `TextureData`，经 `slot.replace_texture_data_with_atlas_scale` 写入对应帧；不能只改 display handle。
  - 公开 `replace_display` 目前没有 data name，无法解析纹理：新增带 `db_name` 的内部/公开变体，或让 `replace_slot_display` / `replace_skin` 负责解析；`tests/replace.rs` 增加 texture/UV 断言。
  - 对 Armature display：A1 明确返回“不支持”错误（当前 `replace_display` 对 Armature 分支传 `None`，会静默变空槽）；运行时建子骨架留给 A2/D。
  - 采用“先全量校验，再逐槽应用”，而不是失败后回滚：core 的 `replace_skin` 本身是逐槽应用，回滚语义不自然；校验所有 slot 的 skin display 和纹理都能解析后再改。
- adapter 命令：
  - `ReplaceSkin { skin }`（A1 限同一 skeleton + 同一 atlas；跨源在 A2 扩展为 `{ data, atlas, skin }`）；
  - `ReplaceDisplay { slot, display }`；
  - `ReplaceTexture { image }`，要求与当前 atlas data 的宽高一致（热更/视频贴图场景）；尺寸不同或要换图集时用 A2 的 `ReplaceAtlas`。
- 复用现有 `content_revision` / hash 触发 mesh 重建；材质缓存 key 仍为 `(image, blend)`，同图集不涉及旧 key 回收。`DragonBonesMaterials::len` 改为公开 diagnostic 或 crate 内单测，供集成测试断言引用平衡。

**成本**：S–M（约 3–4 天，含 core 修复、adapter 命令、demo 按钮、测试）。
**风险**：纹理解析缺失导致“换 skin 不换图”；替换到空 display / 子骨架 display 时语义要显式；材质引用计数在跨图集时才真正复杂。
**验收**

- 新增 `tests/replace_commands.rs`：用 `assets/Wallnut` 的多帧 slot（`front0 → front3`）断言换显示后 mesh UV / region 更新、`texture_data` 跟随；
- core `tests/replace.rs` 增加每帧 `texture_data` 断言（不再只看 display name）；
- demo「换肤/换显示」按钮可用；换装后材质/资产引用平衡可断言；
- 全量测试 + 536 语料审计保持全绿。

### 3.2 A2. 跨图集换装（按需求）

**目标**：皮肤/素材使用独立 `_tex.json` 时也能换装；顺带修复“多图集素材无法加载”。

**设计要点**

- 先纠正模型：DragonBones 的 `TextureAtlasData` 是**一个 tex.json 一个 `imagePath`**（[texture_atlas_data.rs:11](../crates/dragonbones-core/src/model/texture_atlas_data.rs#L11)），多图集 = 多个 tex.json；不是给单个 `AtlasAsset` 塞多个 page。
- core：给 `DisplayFrame` 增加图集身份（如 `texture_atlas_name` / index），`_get_texture_data` 返回值带上命中图集；`replace_*` 路径同样写回。补 `update_texture_atlases` 与 atlas 引用生命周期（比 A1 多出的核心工作）。
- adapter：`DragonBonesArmature` 支持多个 atlas（`Vec<Handle<AtlasAsset>>` 或新的 `AtlasSet` 资产）；factory 注册时把同一 key 下的多个 atlas 放进 `atlas_map` 的 `Vec`；slot 渲染按图集身份选 image / atlas_size，材质 key 变成 `(image, blend)` 的逐 slot 查找与引用计数。
- 与 A1 共用命令层；A1 的 `ReplaceSkin` 在 A2 扩展 `source_skeleton / atlas`，并校验纹理全部命中。

**成本**：M（约 3–5 天，core + adapter + 双页合成测试）。
**风险**：图集引用计数与 `data_map` / `atlas_map` 生命周期；同名 texture 跨页冲突；材质 key 回收。
**验收**

- 合成两页 atlas + 皮肤测试；真实多 tex 素材（有则加）加载与换装；
- 换装后 slot 的 image / UV / 材质 key 正确，旧图集引用归零；
- A1 测试全绿，无回归。

### 3.3 B. 播放控制 API 补全（先做）

**目标**：把 demo 里的临时实现沉淀成正式 API，覆盖 seek、循环、层、过渡，以及游戏常用的 slot/armature 控制。

**设计要点**

- core：
  - 公开 `AnimationState::seek_to(time)`（基于现有 `set_current_time`），并定义语义：夹取/取模规则、**不派发被跳过区间的 frameEvent**、缓存帧索引失效、`current_play_times` 处理；`Animation::seek` 按 name/layer 找状态。
  - 复用已有 `goto_and_play_by_time/frame/progress`（[player.rs:616](../crates/dragonbones-core/src/animation/player.rs#L616)）处理“重启动画”路径，二者语义分开。
- adapter：
  - `Seek { position }` / `SeekProgress { fraction }`：命令在 advance 前应用，随后对目标 armature 补一次 `advance_one(0.0)`，保证暂停态也立即换 pose；
  - `SetLoop { play_times }`、`SetLayer` / `SetGroup`、`FadeIn` / `FadeOut`；
  - `SetSlotTint { slot, color }`、`SetSlotVisible`、`SetArmatureAlpha`、`SetFlip`；
  - `DragonBonesPlaybackState` 补 `looping` / `layer` / `group`；`DragonBonesPlayConfig` 的 `time_scale` / `fade_in_time` 改为 `Option`（`None` = 继承 playback state），避免每次 `PlayConfig` 把速度写回 1.0。
- demo：进度条点击改发 `Seek`（暂停时保持暂停、不重置 fade/speed）；切动画时不再丢速度。

**成本**：S（约 2–3 天）。
**风险**：seek 的事件策略与缓存帧路径要定死；`SetLayer/Group` 改变活动状态顺序后需重排 `play_order`；`SetLoop` 中途修改 `play_times` 要与 `current_play_times` 一致。
**验收**

- `tests/player.rs` 覆盖：暂停 seek、回退 seek 不派发中间事件、循环次数、seek 后 `current_time`；
- demo 点进度条/切动画后状态行速度与实际一致；
- golden 不动。

### 3.4 C. 骨骼挂点与 ECS 集成

**目标**：游戏可以把特效、粒子、UI、武器挂到 bone 上。

**设计要点**

- **C2（推荐先做）**：只读查询 + 挂点 MVP：
  - `DragonBonesInstance` 暴露 `bone_world(name) -> Option<Affine3A>`（或系统资源查询），游戏自行搬运；
  - 或提供 `DragonBonesBoneAttachment { bone: Entity, offset }` 组件，由 sync 之后的小系统把附件 `GlobalTransform` 写成 `bone_global * offset`；附件实体不要挂在 bone 下，避免被 Bevy propagation 抢写。
- **C1（按需）**：为指定 bone spawn 实体，但**不能按 v1 的“每帧写 bone GlobalTransform”实现**：
  - Bevy 0.20 的 `propagate_recursive` 只在 `Transform` 变化 / `GlobalTransform` 新增时重算子节点；直接写 bone 的 `GlobalTransform` 不会把变更传给挂在其下的实体，armature root 移动时还会覆盖写入。
  - 可行方案：① bone 实体作为 root（不进 propagation），adapter 写其 `GlobalTransform`，附件由独立 attachment sync 系统写世界矩阵（精确、无 skew 损耗，推荐）；② bone 实体按骨架层级挂载，adapter 写 local `Transform` 让 propagation 计算——但 DragonBones 有 skew，`Transform` 的 TRS 分解会失真；③ 只给用户点名的 bone spawn 实体，避免每骨架几十个实体。
- 按需 spawn：`DragonBonesArmature` 加 `bones: Vec<String>` 或 `DragonBonesBoneSet` 组件，默认不 spawn。

**成本**：C2 S（约 1–2 天），C1 M（约 3–4 天）。
**风险**：bone 实体数量与 ECS 迭代成本；GlobalTransform 直写与 propagation 的时序；skew 分解。
**验收**

- C2：父骨每帧移动时挂点不落后一帧；多实例压测帧率下降 < 10%；
- C1：旋转/移动父骨后附件世界矩阵正确（含非均匀缩放用例）；x9 压测下降 < 15%。

### 3.5 D. child armature 事件转发

**目标**：嵌套骨架的 `complete` / `frameEvent` / `soundEvent` 也能到 Bevy message。

**设计要点**

- core：在 `BaseFactory` 上增加 `proxy_factory: Option<Box<dyn Fn() -> Box<dyn ArmatureProxy>>>` + setter；`_build_child_armature` 用它替代硬编码 `NullArmatureProxy`（[base_factory.rs:1530](../crates/dragonbones-core/src/factory/base_factory.rs#L1530)）。这比改 `build_armature` 签名波及面小。
- adapter：`BevyArmatureProxy` 创建时共享同一个 `DragonBonesEventQueue`；事件按 `ArmatureIdx` 查 `bindings` 映射到子 root 实体（`DragonBonesChildArmature` 已有父槽/父骨架信息）。
- 生命周期：父骨架 dispose 时清空子骨架事件队列；顶层 despawn 后不再收到子事件。

**成本**：S（约 2–3 天，含测试）。
**风险**：factory 新字段在测试构造路径的默认值；子骨架事件与父骨架事件的消费顺序。
**验收**

- `tests/events.rs` 增加父→子嵌套骨架的 loopComplete / frameEvent 断言；
- 顶层 despawn 后无子骨架事件；现有事件测试不回归。

### 3.6 E. 渲染长尾

**目标**：素材兼容性与观感完整。

**子项与优先级**

1. **颜色 offset**（低–中）：共享材质下不能为每个 slot 加 uniform（会打破合批），应把 offset 编码进第二个顶点属性（或扩展顶点色为两组），在 `dragonbones_material.wesl` 里乘加；同时适配层要拿到 offset 的 API（见 B 的 slot tint 命令）。
2. **多图集渲染**（中）：与 A2 一起做，不要单独立项；核心是 per-slot 图集身份 + 逐 slot image/atlas_size。
3. **跨骨架排序 / ysort**（中）：`slot_z` 只保证骨架内顺序；游戏需要按 root z 或屏幕 Y 排序时，adapter 应提供可插拔排序策略（至少文档化“root z 由用户负责”）。
4. **blend 长尾**（低）：Overlay/Darken 等需要读 dst，固定管线无法表达；需要 shader 采样屏幕纹理或近似；当前语料 43748 normal / 430 add，按素材触发。
5. **贴图热重载**（低）：与 A1 的 `ReplaceTexture` 合并。

**成本**：offset S–M；多图集 M（并入 A2）；blend 长尾 M–L。
**风险**：WESL/材质改动会触及 golden 之外的渲染回归，需要截图对比基线。
**验收**：合成素材测试（offset 动画、两页 atlas、Overlay 素材各一），截图与官方 runtime 对比。

### 3.7 F. 性能：先量化，再决定 GPU skinning

**目标**：确定多角色/大 mesh 的真实瓶颈，避免盲目投入。

- **已有数据**：`docs/optimization-roadmap.md` §1.4 / §3.2 已有 CPU advance / sync profile，结论是“当前素材 CPU 蒙皮绝对值小于一帧预算，GPU skinning 暂缓”。F 不要重复测 CPU benchmark。
- **第一步（S）**：demo 增加可配置压测（N 实例、大 mesh rig、关闭 gizmos），记录 **draw calls、GPU 帧时间、顶点上传量、CPU sync**；`cargo run --release` 跑 10/50/100/200。
- **第二步（视结果）**：
  - 瓶颈在 CPU advance/sync：优化 timeline 查表、sync 脏检查；
  - 瓶颈在 draw call：优先合批 / 材质合并 / 减少 slot 实体，而不是直接上 GPU skinning；
  - 瓶颈在顶点上传：GPU skinning（bone matrices → storage buffer，vertex shader 蒙皮）；
  - 瓶颈在填充率：材质/贴图合并，减少 overdraw。
- **成本**：量化 S；GPU skinning L（1–2 周，含 WESL 与正确性验证）。
- **风险**：core 位一致路径不能动；GPU 蒙皮会引入浮点差异（只影响渲染）。
- **验收指标建议**：1080p、100 个中等骨架 ≥ 60 fps（RTX 3060 级）；单实例 CPU advance 不超过当前 1.5×；记录 draw call 预算。

### 3.8 G. 工程化：CI 扩展与发布准备

**目标**：防回退、可分发。

1. **CI 语料策略（S）**：`cargo test --workspace` 已覆盖 golden；`corpus_audit` 依赖本地 `_research/`（2GB、被 `.gitignore` 忽略）+ Node + `DragonBonesJS`，`submodules: recursive` 拿不到语料。建议：
   - PR：跑 fmt/clippy/test + `assets/` 的 29 套 smoke（`tests/assets_smoke.rs` 已有基础）；
   - nightly：self-hosted runner 或下载 corpus artifact 跑 536；报告 `target/corpus_audit/report.md` 入库做趋势。
2. **bench 回归（S–M）**：同机器 smoke + 宽松阈值告警，或 nightly 记录趋势；不要把绝对值写死。
3. **wasm build（M）**：`wasm32-unknown-unknown` + `wasm-bindgen`；处理 `NonSend<DragonBonesRuntime>`（wasm 单线程通常可行）、资源路径、WESL 后端；产出静态 demo 页。
4. **crates.io 发布准备（M）**：workspace 版本、README/docs.rs、feature 拆分（core 与 bevy 适配层分开）、`cargo publish --dry-run`。

**风险**：CI 时间与成本；wasm 的 Bevy 特性组合；发布后 semver 约束。
**验收**：PR 上跑 fmt/clippy/test + wasm build；nightly 跑 corpus 与 bench smoke。

### 3.9 H. 内容与工具

- **先拆 demo**（S）：`main.rs` 按 asset catalog / UI / commands / debug overlay 拆成模块；否则 A/B 加 UI 会变成新的技术债。
- **导入更多 rig**（S）：29 → 50+，优先覆盖 mesh/FFD/blend/多图集/Path/IK；注意素材版权仅开发参考。
- **资产校验工具**（S）：检查缺贴图、空 display、异常 transform、骨骼重名、循环引用；输出 HTML/JSON 报告。
- **预览器增强**（S–M）：进度条拖动（`PointerDrag`）、逐帧步进、动画混合对比、截图导出。

## 4. 推荐路线

**Sprint 1（1–2 周）**：

1. B 的 core 部分（`seek_to` + 语义测试）与 adapter 命令；
2. A1（core 纹理链 + 同图集换装 + demo 按钮）；
3. E2 的 per-slot 图集身份做 0.5 天设计 spike，决定 A2 是否紧随。

理由：B 最便宜且能修掉 demo 现存的“速度回 1.0”；A1 的真正成本在 core 纹理链，先把坑暴露出来。

**Sprint 2（1–2 周）**：

- D（child 事件）与 C2（只读挂点/attachment MVP）二选一先做，另一个紧随；
- G1 语料 CI 策略 + bench smoke；
- 素材需要跨 tex 皮肤时插入 A2。

**Sprint 3（1–3 周）**：

- F 第一步压测量化；数据支持再决定合批/GPU skinning；
- G3 wasm demo；
- E 的 color offset / 跨骨架排序（按素材需求）。

**低优先/按需**：C1 大量挂点、blend 长尾、H 的素材与工具、G4 发布。

## 5. 量化验收（跨方案）

- 正确性：全量测试 + 536 语料审计 + JS 对拍保持全绿；新增能力必须有独立测试。
- 性能：单实例 advance 不超过基线 1.5×；100 实例 1080p ≥ 60 fps（F 目标）；记录 draw call 数。
- 工程质量：fmt/clippy `-D warnings`/test 全绿；新增公开 API 有文档注释与示例。
- demo：每个新能力在预览器里可点、可验证，避免“只有测试没有演示”。
- core 边界：所有“渲染资源绑定”留在 adapter；core 只负责数据、动画、替换语义与图集身份。

## 6. 开放问题

1. 素材版权：`assets/` 仅开发参考；对外发布/网页 demo 需要替换为自制或授权素材。
2. Bevy 升级节奏：0.20 固定；升级会波及 `ui_widgets`、picking、WESL，需要单独评估。
3. `NonSend<DragonBonesRuntime>` 与多线程：目前主线程执行；wasm/多线程方案需要重新评估。
4. 换装语义：A1 限定同图集，A2 才支持跨图集；皮肤带子骨架 display 时 A1 明确报错还是建子骨架，需要定稿。
5. 子骨架 proxy 注入：优先用 factory 上的 `proxy_factory` 字段，避免改 `build_armature` 签名。
6. 多图集同名 texture：跨页重名时以图集身份优先，还是按名字首命中，需要在 A2 定稿。

## 7. core 完成度评估（2026-11-16）

### 7.1 按什么口径算“完成”

- **原始规格完成**：`requirements.md` 的 100% 定义——37 个 TS 文件等价实现，8 个素材 × 全部动画 × ≥120 帧，f64 容差 `1e-6`、dbbin 容差 `1e-4` 的差分全绿。
- **产品级 core 完成**：在原始规格之上，补齐游戏运行时需要的替换/图集/事件/查询边界，并且测试能锁住。

按第一个口径，**core 已经完成**：P0 清单已关闭，P1 的数据层部分基本关闭，golden / 语料审计全绿。现在感觉“还没完”，主要是 scope 从“回放动画”扩到了“游戏运行时”。

### 7.2 剩余工作与估算

| 项目 | 内容 | 估算（专职） |
|---|---|---|
| 原始规格差分 | 37 TS 映射、8 素材 golden、536 语料审计 | **已完成** |
| A1 core | replace 路径解析 `texture_data` + 测试（Wallnut 多帧 fixture） | 1–1.5 天 |
| A2 core | per-slot 图集身份 + `update_texture_atlases` + 引用生命周期 | 2–3 天 |
| B core | `seek_to` 公开 + 事件/缓存语义 + 测试 | 1 天 |
| D core | `proxy_factory` 注入子骨架 | 1 天 |
| 数据管理 API | `get_*` / `remove_*` / `clear` / `get_bone_by_display` 等包装 | 1 天 |
| Armature display 运行时替换 | 建子骨架或明确报错；先报错可省 1–2 天 | 1–2 天 |
| 回归与语料重跑 | 新测试 + 全量 + corpus | 2 天 |
| **小计（产品级 core）** | | **9–13 天 ≈ 2 周** |
| 外部阻塞验证 | mesh dbbin（缺样本）、Path percent 与官方差异（无素材） | 有样本 1–3 天；无样本登记为已知偏差 |
| 可选架构项 | `FactoryAdapter` trait、`BaseFactory: Send + Sync` | 2–3 天，非必需 |

兼职按 ×2 估算；`A2` 与 `C1` 可按素材/需求延后，不影响 core v1.0。

### 7.3 结论与冻结建议

- **不要再把 adapter 的需求叫“core 没写完”**：替换纹理、多图集、child proxy、seek 都是 core 与 adapter 的边界能力，各自有明确归属（见 §2）；core 本体没有“大重写”剩余。
- 建议把 **core v1.0** 冻结在：A1 + B（core 部分）+ D（core 部分）+ 数据管理 API + 回归全绿。预计 2 周内可结算。
- A2、C1、E、F 属于 v1.x / 产品化，按需求推进；`FactoryAdapter` / `Send + Sync` 只在 profile 或 wasm/多线程需求出现时再做。

## 8. v1 → v2 修订记录

- A 拆为 A1（同图集）/ A2（跨图集），修正“core 换装 API 已就绪”的结论：数据层就绪，渲染层缺 texture_data 解析。
- C1 改为兼容 Bevy 0.20 transform propagation 的设计；C2 提前为 MVP。
- B 补 seek 语义、`PlayConfig` 继承播放状态、slot/armature 命令；缩小 core 工作量。
- E2 从“多页 AtlasAsset”改为“多图集（多个 tex.json）+ per-slot 图集身份”。
- G1 修正语料 CI 依赖（`_research/` + Node，不是 submodule）。
- 新增 §7 core 完成度评估与冻结建议。

## 9. 实施进展（2026-11-16）

按用户确认的范围实现到 §3.6（E 只做 color offset、多图集渲染、跨骨架排序；不含 blend 长尾与热重载）；C1 按 §3.4 的“C2 优先”策略暂缓。

| 方案 | 状态 | 已落地内容 |
|---|---|---|
| A1 | ✅ | core `replace_display` / `replace_slot_display` / `replace_skin` 现在解析并写回 `texture_data`（含帧级图集归属）；Bevy `ReplaceSkin` / `ReplaceDisplay` / `ClearDisplay` 命令（同图集贴图热替换由 A2 的 `ReplaceAtlas` 覆盖）；demo `F2` 循环多帧 slot 显示 |
| A2 | ✅ | `TextureAtlasRef`（cache key + page index）随 `DisplayFrame` 保存；`DragonBonesExtraAtlases` 多 tex.json 注册；逐 slot 图集 image / 尺寸 / 材质；`update_texture_atlases`、`get_*`、`remove_*`、`clear_data`；`ReplaceAtlas` 运行时换页并刷新 slot 纹理；`ReplaceSkinFrom` 支持跨 data 皮肤（source skeleton + atlas 集） |
| B | ✅ | `AnimationState::seek_to` + `Animation::seek`；Bevy `Seek` / `SeekProgress` / `SetLoop` / `SetLayer` / `SetGroup` / `FadeIn` / `FadeOut` / `SetSlotTint` / `SetSlotVisible` / `SetArmatureAlpha` / `SetFlip`；`PlayConfig::time_scale` 改为 `Option`（未设置时继承 playback state） |
| C | 🟡 | C2 已完成：`DragonBonesRuntime::bone_transform` 只读查询 + `DragonBonesBoneAttachment` 附件同步（不依赖 Bevy 层级传播）；C1“每根 bone 一个实体”暂缓（C2 覆盖常规特效/武器挂点） |
| D | ✅ | `BaseFactory::proxy_factory` 注入子骨架 proxy；adapter 子骨架共享根事件队列；`DragonBonesEvent.armature` 携带子骨架索引 |
| E | 🟡 | color offset 用 per-slot 材质 uniform 实现（非零 offset 才创建专用材质）；多图集渲染随 A2 完成；`DragonBonesSorting::armature_z_step` 提供跨骨架 z 分带；blend 长尾、贴图热重载按约定不做 |

**验证**

- `cargo test --workspace`：全绿（core + adapter + demo 编译）。
- `cargo clippy --workspace --all-targets -- -D warnings`：全绿。
- `cargo fmt --all --check`：全绿。
- 语料审计：536/536 通过，JS 对拍 39 一致 / 0 不一致（`DB_CORPUS_PARITY=40`）。
- 新增测试：`tests/replace.rs`（core 纹理刷新）、`tests/atlas_ref.rs`、`tests/seek.rs`、`tests/proxy_factory.rs`、`tests/replace_commands.rs`、`tests/multi_atlas.rs`、`tests/bone_attachment.rs`、`tests/color_offset.rs`、`tests/sorting.rs`、`tests/cross_data_skin.rs`。
- 未做：C1（按 §3.4 策略暂缓）、blend 长尾、贴图热重载、GPU 截图基线对比。color offset 的 WESL 路径已由 demo `F3` 实机确认，headless 测试覆盖 material asset 数据路径。
