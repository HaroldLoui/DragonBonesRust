# DragonBones 素材来源与导入

本仓库 `assets/` 里的 DragonBones 素材来自若干公开工程。本文记录每个来源的可用性、如何再导入
更多素材，以及导入时必须遵守的三件套规范。

## 结论：`Gzh0821/pvzge_web` 已删除，但素材没有消失

`assets/PlantAnimate/PlantAnimate_ske.json`（超级机枪豌豆，armature `MegaGatlingPea`）出自已删除的
[`Gzh0821/pvzge_web`](https://github.com/Gzh0821/pvzge_web)（现在返回 404）。它的素材实际上是
PvZ2 Gardendless（pvzge）客户端里未修改的 DragonBones 5.5 导出物。这个仓库本身没有顶层 `assets/` 目录
——原始龙骨 JSON 全部嵌在 `docs/assets/resources/import/<pack>.json` 的 `dragonBonesJson` 字段里，
所以“找素材”应该直接找素材源，而不是找这个仓库。当前可用的路径如下：

| 来源 | 状态 | 内容 | 获取难度 |
| --- | --- | --- | --- |
| [`PVZ2-Gardendless-Plus-Team/PlantsVsZombies2GardendlessPlus.Art`](https://github.com/PVZ2-Gardendless-Plus-Team/PlantsVsZombies2GardendlessPlus.Art) @ `ecbcf8b` | ✅ 公开可用 | 1287 个 rig 目录 / 538 份唯一 skeleton，全部 `version=5.5`；每 rig 一个目录：`skeleton.json` + `atlas.json` + `_textures/<uuid>.png`（git-lfs） | **推荐**：文件即三件套原料，无需解包 |
| [pvzge 官方 Release](https://github.com/Gzh0821/pvzg_site/releases)（`pvzge-0.16.8.zip` 1.2GB / `pvzge-lite-0.16.8.zip` 385MB） | ✅ 可下载 | Tauri + Cocos Creator 3.8.4 web build；DragonBones 在 `assets/resources/import/**/*.json` 的 `dragonBonesJson` 字段，贴图在 `assets/resources/native/`（avif/webp/png） | 需要解包 + 解析 Cocos 序列化格式 |
| [`Dey410/extract-pvzge-gpnext` Releases](https://github.com/Dey410/extract-pvzge-gpnext/releases) | ✅ 可下载 | 已提取的 `GardendlessResources-0.16.8+*.zip`（608MB），游戏 `docs/` 运行资源 | 贴图可能已是 ASTC，需再转 PNG |
| [pvzge 官方示例 mod 包](https://pvzge.com/guide/mod/gp-next-examples.html) | ✅ 可下载 | `pvzge.entities.zip` 等，仅脚本 + 64×64 单骨骼示例 | 没有真实素材 |
| `Gzh0821/pvzge_web` 本体 | ⚠️ 仓库 404，但 [Software Heritage 有完整归档](https://archive.softwareheritage.org/api/1/snapshot/b402fb0cdef5fc2eea56a8f23241aa2f2f802d44/) | 5 次 full visit（最新 2026-10-18），snapshot `b402fb0c…`，HEAD `0e4a0534`；目录树可列出、单文件 blob 可取回（匿名 API 有 429 限流） | 注意：该仓库**没有顶层 `assets/`**，龙骨 JSON 嵌在 `docs/assets/resources/import/<pack>.json` 里 |
| [play.pvzge.com](https://play.pvzge.com) 线上构建 | ✅ 仍在线 | `assets/resources/import/` 下 1058 个 Cocos pack 可直接下载（约 68MB），489 个含龙骨；`ske/tex` 就是 `DragonBonesAsset._dragonBonesJson` 原文 | `native/` 贴图路径 404，需从 `Twig6943/PVZGE_Android` 等镜像按 uuid 回填 |

> 交叉验证：`.Art` 仓库里 `plants_PeashooterPLANT/skeleton.json` 与本仓库 `PlantAnimate_ske.json`
> 是同一份数据——`skeleton.name` 同为 `PlantAnimate`，同样 6 条动画
> `Idle / Shoot / Death / Food / Water / Random`。因此 `.Art` 就是 `pvzge_web` 素材的上游，
> 不是二次转换产物。

社区 mod（Another-Timeline、Impzane、Reflourished 等）实测都是**纯数据 mod**（只含关卡/数值 JSON），
不含龙骨；`pvzge.entities.zip` 等官方示例也只验证了 mod 资源目录约定。

## 三件套规范（导入前必读）

`dragonbones-bevy` 的 `AtlasAssetLoader` 把 `tex.json` 的 `imagePath` 解析为**相对 tex.json 所在目录**，
所以 `assets/` 下每个 rig 一个同名目录，`imagePath` 保持纯文件名即可：

```text
assets/<Name>/<Name>_ske.json      # DragonBones skeleton（version 5.5）
assets/<Name>/<Name>_tex.json      # 图集描述，imagePath 必须等于 "<Name>_tex.png"
assets/<Name>/<Name>_tex.png       # 图集贴图，尺寸必须与 tex.json 的 width/height 一致
```

- 少数 rig 共用一张贴图（如 `Dragon_Blend` 的 `imagePath` 是 `Dragon_tex.png`、
  `Rooster_Blend` 是 `Rooster_Ani_tex.png`）：在各自目录里放一份同名 PNG 副本即可，
  `imagePath` 不要写路径。

- `_ske.json` 里的 `armature[].name` 才是骨架名，和文件名无关（例如
  `Peashooter/Peashooter_ske.json` 的 `skeleton.name` 是 `PlantAnimate`，armature 是 `Peashooter`）。
- 多 armature 文件很常见（一个 JSON 里既有植物本体，也有装饰/弹道）；demo 只需选一个 armature，
  但 `assets_smoke` 会把文件内全部 armature 都装配一遍。
- 图集贴图不要带 trim/rotation 之外的二次加工；PNG 尺寸与 `tex.json` 的 `width/height` 不一致会导致
  UV 偏移。

## 本仓库已集成的 PvZ2 Gardendless 素材

| demo `--demo` | 文件 / armature | bones | slots | 动画数 | 图集 |
| --- | --- | --- | --- | --- | --- |
| `peashooter` | Peashooter / Peashooter | 26 | 38 | 6 | 512×512 |
| `sunflower` | Sunflower / Sunflower | 21 | 28 | 7 | 256×512 |
| `wallnut` | Wallnut / Wall-nut | 19 | 25 | 12 | 512×1024 |
| `gatlingpea` | GatlingPea / GatlingMGP | 48 | 57 | 7 | 512×512 |
| `torchwood` | Torchwood / Torchwood | 20 | 29 | 6 | 2048×2048 |
| `basic_zombie` | BasicZombie / BasicZombie | 30 | 26 | 4 | 1024×512 |
| `gargantuar` | Gargantuar / Gargantuar | 70 | 81 | 5 | 1024×1024 |
| `crazy_dave` | CrazyDave / CrazyDave | 80 | 77 | 11 | 2048×1024 |
| `zomboss_head` | ZombossHead / ZombossHead | 22 | 17 | 10 | 1024×2048 |
| `shovel` | Shovel / shovelInHand | 2 | 1 | 1 | 512×512 |
| `dino_raptor` | DinoRaptor / Raptor | 46 | 50 | 9 | 512×512 |
| `aquavine` | AquaVine / Aquavine | 48 | 33 | 7 | 512×512 |
| `electric_pea` | ElectricPea / ElectricPeashooter | 27 | 31 | 7 | 512×1024 |
| `chomper` | Chomper / Chomper | 38 | 44 | 16 | 1024×512 |
| `repeater` | Repeater / Repeater | 32 | 53 | 7 | 512×1024 |
| `cactus` | Cactus / Cactus | 94 | 99 | 12 | 512×512 |
| `marigold` | Marigold / MariGold | 28 | 27 | 7 | 1024×1024 |
| `melon_pult` | MelonPult / Melon-pult | 25 | 27 | 5 | 1024×1024 |
| `balloon_zombie` | BalloonZombie / BalloonZombie | 39 | 34 | 13 | 512×1024 |
| `dark_king_zombie` | DarkKingZombie / KingVeteran | 33 | 42 | 6 | 1024×2048 |
| `gloom_shroom` | GloomShroom / GloomShroom | 42 | 44 | 7 | 512×512 |

选型覆盖：最小骨架（`shovel` 2 骨）、常规植物/僵尸、多 armature 文件（`basic_zombie` 12 个、
`gargantuar` 5 个、`aquavine` 10 个、`cactus` 12 个、`marigold` 8 个）、94 骨的大型骨架
（`cactus`）、80 骨的人形 NPC（`crazy_dave`）、超长 AABB（`zomboss_head`）。

## 本次采集的本地暂存（未入库，`.gitignore` 忽略 `_research/`）

| 目录 | 内容 | 来源 |
| --- | --- | --- |
| `_research/mods/rigs/` | **48 套三件套齐全**的 rig（271 armature / 1204 动画 / 35MB 贴图），含植物、僵尸、恐龙、Boss、UI | `.Art` 仓库 `ecbcf8b` |
| `_research/mods/manifest.json` / `catalog/plus_art_index.json` | 48 套的机器可读清单 + 1287 套全量索引（rig→skeleton/armature/动画/贴图） | 同上 |
| `_research/pvzge_client/official_rig_inventory.json` / `coverage_vs_art.md` | 官方 0.16.8 客户端 1210 rig 的寻址清单与 `.Art` 覆盖对照 | `pvzge-lite-0.16.8.zip` |
| `_research/recovered/rigs/` | **488 套** `ske`+`tex`（3,186 armature / 7,934 动画），13 套带 PNG；其余缺贴图 | live `play.pvzge.com` 构建 |

需要更多素材时，从上面任一处复制到 `assets/` 并用 `tools/import_rig.py` 规范化即可。

其中 `aquavine` 与 `electric_pea` 只存在于官方 `pvzge-lite-0.16.8` 包里（`.Art` 未收录），
是从官方客户端解包得到的：Cocos 资源包里的 `DragonBonesAsset._dragonBonesJson`，贴图是带
`auxl` alpha 的 AVIF，必须用 libavif `avifdec` 转 PNG（ffmpeg 解码会丢 alpha）。

这些素材也参与 `crates/dragonbones-core/tests/assets_smoke.rs`（解析/装配/推进冒烟）和
`crates/dragonbones-core/tests/ir_binary.rs`（与官方 TS 运行时 `data.binary` 字节对拍）的回归。

## 再导入更多素材

`.Art` 仓库的 1287 个 rig 都可以用同一套流程导入：

```powershell
# 1. 浅克隆（git-lfs 拉贴图，完整 checkout 约 1.6GB）
gh repo clone PVZ2-Gardendless-Plus-Team/PlantsVsZombies2GardendlessPlus.Art -- --depth 1

# 2. 用仓库根的 _index.json 找到目标 rig 目录，例如 plants_ChomperPLANT
#    目录内：skeleton.json、atlas.json、_textures/<cocos-uuid>.png

# 3. 复制并改名，自动写入 assets/<Target>/<Target>_{ske,tex}.json + _tex.png
python tools/import_rig.py --rig-dir <clone>/plants_ChomperPLANT --target Chomper

# 4. 无需改 demo 代码：启动时自动扫描 assets/<Target>/，新目录会出现在 UI 面板里
#    （想默认展示它：cargo run -p dragonbones-demo -- <Target>）

# 5. 生成黄金数据并跑测试
node tools/gen_ir_binary.mjs
cargo test --workspace
```

`tools/import_rig.py` 会完成命名规范化、`imagePath` 改写、AABB 居中/缩放计算和 match 分支生成。

## 版权

`.Art` 仓库的素材版权归 PopCap Games / PopCap Games (Shanghai) / Electronic Arts 所有，
上游仓库明确说明“仅供开发参考、不由该仓库授权”。本仓库引用这些素材仅用于 DragonBones
运行时的开发与测试对拍，demo 不用于商业分发。
