# core 真实语料压测与 JS 对拍审计

> 日期：2026-11-08
> 测试：`crates/dragonbones-core/tests/corpus_audit.rs`（`#[ignore]`，本地语料库依赖）
> 回归：`crates/dragonbones-core/tests/malformed_input.rs`
> 报告产物：`target/corpus_audit/report.md`

## 1. 目的

`assets/` 里提交的 29 套素材只是官方素材库的极小抽样。此前的 golden 只覆盖这些
手挑素材，无法回答「解析器面对上游 538 份真实 skeleton 是否稳健」。本审计把
core 的解析、构建、动画推进全链路放到本地完整语料上跑一遍，并用官方 JS 运行时
（`tools/irdump.mjs`）对结构快照做抽样对拍，把「证据面」从 29 套扩到 536 套。

审计输入是本地暂存（`.gitignore` 中 `_research/`）：

- `_research/mods/rigs/`：48 套 PvZGE mod 素材
- `_research/recovered/rigs/`：488 套从 pvzge 客户端恢复的素材
- （缺省还会追加 `assets/`，可用 `DB_CORPUS_DIRS` 覆盖）

语料本身不入库，因此该测试标记 `#[ignore]`，只在本地手动运行。

## 2. 方法

对每个 `*_ske.json`：

1. 解析：`BaseFactory::parse_dragon_bones_data` + `parse_texture_atlas_data`。
2. 结构自检：`armatureNames` 非空、`sortedBones/bones` 与 `sortedSlots/slots`
   对应、`defaultAnimation` 在 `animationNames` 内、`defaultSkin` 在 `skins` 内。
3. 构建：全部 armature 各 `build_armature` 一次再 `dispose_armature`，验证构建路径。
4. 推进：首个 armature 的每个动画（上限 64）从 0 帧起按 1/60 s 推进 120 帧，
   逐帧检查每根骨骼、每个插槽的全局矩阵 6 个分量与 alpha 有限，
   以及 `AnimationState` 的 `current_time` / `total_time` 有限。
5. 异常归一：每个 rig 包 `catch_unwind`，panic hook 静音后归类为 `panic`，
   不中断整轮。
6. JS 对拍：抽样 `DB_CORPUS_PARITY` 个 rig（默认 40，按等距抽取），调用
   `node tools/irdump.mjs` 导出 meta/structure/atlas 快照与 8 组 IR 数组；
   int 数组精确比较、float 数组按 f32 位比较。

判定为失败的类别：解析失败、构建失败、`panic`、`non_finite`、JS 快照/IR 差异。
`armatureNames` 为空的空骨架 rig（0 个 armature，如 `BonkChoy`、`ExplodeONut`、
`InfiNut`、`NunchakuZombie`、`Par0`、`Plantern`、`PlayerHouse`、`TNT` 等）按
「无可测 armature」跳过，不算失败。

## 3. 运行方式

MSYS Bash / PowerShell 都可以，但 `DB_CORPUS_DIRS` 必须传 **Windows 路径**：

```bash
env DB_CORPUS_DIRS="D:/workspace/RustProjects/DragonBonesRust/_research/mods/rigs;D:/workspace/RustProjects/DragonBonesRust/_research/recovered/rigs" \
    DB_CORPUS_PARITY=40 \
    cargo test -q -p dragonbones-core --test corpus_audit -- --ignored --nocapture
```

可选环境变量：

| 变量 | 默认 | 含义 |
|---|---|---|
| `DB_CORPUS_DIRS` | `_research/mods/rigs;_research/recovered/rigs;assets` | `;` 分隔的扫描根 |
| `DB_CORPUS_FRAMES` | `120` | 每个动画推进帧数 |
| `DB_CORPUS_PARITY` | `40` | JS 对拍抽样数，`0` 关闭 |
| `DB_CORPUS_LIMIT` | 不限 | 只跑前 N 个 rig（冒烟用） |
| `DB_CORPUS_REPORT` | `target/corpus_audit/report.md` | 报告路径 |

## 4. 结果（全量）

| 指标 | 数值 |
|---|---|
| rig 总数 | 536 |
| 通过 / 失败 | 536 / 0 |
| armature 总数 | 3452 |
| 推进动画数 | 3093 |
| 推进帧数 | 371160 |
| JS 对拍 | 39 一致 / 0 不一致 / 0 跳过 |
| 用时 | 约 23–39 s（视机器与编译缓存） |

首轮（修复前）是 523 通过 / 13 失败、JS 对拍 38 一致 / 1 不一致 / 0 跳过。
逐条失败与修复如下。

## 5. 失败根因与修复

### 5.1 `parse_z_order_frame` 负偏移 underflow（4 套）

- rig：`bonkchoy_ske`、`BobSledHEAD_ske`、`BonkChoy_ske`、`NunchakuZombie_ske`
- 现象：解析阶段 panic `attempt to subtract with overflow`。
- 根因：TS 的 `_parseZOrderFrame` 允许负 `zOrderOffset`（把槽往前排），旧 Rust 把
  offset 直接 `as usize` 饱和成 0，导致 `zOrders` 覆盖缺失，末尾
  `unchanged_index -= 1` 下溢。
- 修复：索引按 i64 计算并只写 `0..slotCount` 范围内的项；`unchanged` 长度用
  `saturating_sub`；末尾递减改用 `saturating_sub` + 缺项按 0 处理（对齐 TS
  `unchanged[--i] || 0`）；奇数长度、非有限/越界/落后游标的帧对跳过。
- 偏差登记：`docs/model_mapping.md` D-6.19。

### 5.2 子骨架显示撞上同名贴图（6 套）

- rig：`ExplodeONut_ske`、`InfiNut_ske`、`Par0_ske`、`Plantern_ske`、
  `PlayerHouse_ske`、`TNT_ske`
- 现象：推进到某个动画时 panic `unreachable!("imageDisplayData 非 Image 类型")`。
- 根因：`_build_slots` 对所有 display 类型都查图集贴图（与 TS 一致），子骨架显示
  的 `path` 恰好与图集贴图同名时 `texture_data` 变 Some，于是进入
  `geometry == null && texture != null` 分支。TS 在该分支用条件回退
  `imageDisplayData = displayData.type === Image ? displayData : rawDisplayData`，
  对非 Image 的 raw 取 `.pivot` 得到 `undefined`（后续 NaN，但该 pivot 只有图像
  渲染消费）。Rust 移植成 `unreachable!` 是移植缺陷。
- 修复：按 TS 语义取候选显示数据；pivot 仅在显示数据是 Image 时读取，否则取
  `(0, 0)`，保证矩阵与数值检查干净。

### 5.3 IK 退化姿态产生 NaN（3 套）

- rig：`future_protector_zombie_ske`、`Protector_ske`（`Death` f83，骨骼 `wire2`）、
  `Zomboss_11_ske`（`Spawn` f0，多根骨骼）
- 现象：全局矩阵出现 NaN，`AnimationState` 时间轴仍正常。
- 根因：Protector 的 IK 写在旧字段 `ik` 里
  （`ik: [{"chain": 1, "name": "bone_ik", "bone": "wire2", "target": "hat"}]`，
  而同 rig 的 `constraint` 是空数组）。IK 链骨 `wire2` 的 `scaleFrame` 让 scaleY
  从 1 线性过零到 -1，帧 83 附近 scaleY ≈ 0；`update_by_constraint_resolved`
  在 `is_cache` 下用 `Transform::from_matrix` 分解局部矩阵，`c == d == 0` 时
  `atan(-c / d)` = `atan(NaN)`，接着 `compute_b` 用被污染的 `global` 重新
  `to_matrix`，NaN 写回全局矩阵。
- 官方 TS 的 `Transform.fromMatrix` 在同样数据下同样产生 NaN（`Math.atan(0/0)`），
  属于极端姿态下的健壮性缺口，不是纯移植错误。
- 修复：`Transform::from_matrix` 增加退化矩阵保护——行列式
  `a*d - b*c == 0` 时走确定性回退：`rotation = atan2(b, a)`、
  `scale_x = hypot(a, b)`、`scale_y = 0`、`skew = 0`，保证 `to_matrix` 回写后矩阵有限。
- 偏差登记：`docs/model_mapping.md` D-6.18。

### 5.4 槽级 `actions` 丢失（1 套，JS 对拍差异）

- rig：`tornado_ske`（`structure.armatures.sandyMain.skins.default.snowyBase[1].actions`
  Rust 长度 0 / JS 1）
- 根因：TS `_parseDisplay` 依据当前 skin 中该 slot 已解析的显示数量
  （`displays.length`）决定槽级 actions 挂到哪个显示上；旧 Rust 读 `self.skin`，
  而该字段在 `parse_skin` 期间从未赋值，条件恒 false。
- 修复：`parse_display` 增加 `displays_len` 参数，由 `parse_skin` 传入已解析数量，
  条件改为 `display_index >= 0 && display_index as usize == displays_len`。
  行为恢复与 TS 一致，不属于语义偏差。

## 6. 回归测试

`crates/dragonbones-core/tests/malformed_input.rs`（4 个普通测试，随 `cargo test`
运行，无需本地语料）：

| 测试 | 覆盖 |
|---|---|
| `z_order_out_of_range_offset_does_not_underflow` | 5.1 的负偏移/越界 zOrder |
| `armature_display_sharing_texture_name_does_not_panic` | 5.2 的子骨架显示撞贴图名 |
| `malformed_toplevel_is_rejected_without_panic` | 顶层字段类型错误 / 空数据不 panic |
| `extreme_numbers_do_not_panic` | 1e308 级 scale 下构建与推进不 panic |

## 7. 已知边界

- 语料全部来自 PvZGE（PvZ2 竖版）：无 mesh/weight/deform/非 Normal blend 的样本，
  这些路径仍只有合成用例覆盖。
- JS 对拍只抽样 39 套（`DB_CORPUS_PARITY` 默认 40，536/40 等距抽样实际覆盖 39 套），
  其余 rig 的 TS 数值一致性未逐帧验证。
- 语料不在仓库内，审计测试 `#[ignore]`，不是 CI 的默认回归。
- 审计只校验「不 panic、数值有限、结构自检、抽样 JS 一致」，不校验动画观感。
