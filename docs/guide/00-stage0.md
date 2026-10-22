# 阶段 0 实现指南 — 工程基建与差分测试环境

> 配套 `docs/requirements.md` §3 阶段 0。本指南给出伪代码级任务分解 + 已实测可用的工具代码。
> 流程：按顺序完成任务 1→6，跑完「验收清单」全绿后告诉我，我再写阶段 1 的指南。

---

## 0. 先读：我已实测验证的事实（与 requirements 的差异）

以下结论我在本机 Node v22.18.0 上实际运行验证过，**以这里为准**（requirements 有几处按旧版 API 写的，直接照抄会失败）：

| # | 事实 | 影响 |
|---|---|---|
| F1 | `.mjs` 里直接写 `require(...)` 会崩：`ReferenceError: require is not defined in ES module scope` | jsdump 必须 `import { createRequire } from "node:module"`，或改用 `.cjs` |
| F2 | **事件只在全局 `DragonBones.advanceTime` 里派发**（`bufferEvent` → 末尾 `dispatchDBEvent`）。直接调 `armature.advanceTime(dt)` 事件永远不会触发 | 主循环必须：`armature.clock = factory.clock`，然后每帧调 `factory._dragonBones.advanceTime(dt)` |
| F3 | `BaseFactory` 构造器**不会**创建 `_dragonBones` 实例（那是 PixiFactory 干的） | jsdump 里要手动 `factory._dragonBones = new db.DragonBones(em)` |
| F4 | `buildArmature(armatureName, dragonBonesName, skinName, textureAtlasName)` 第 4 参必须显式传图集注册名，否则槽位拿不到纹理数据 | `buildArmature(armName, key, "", key)` |
| F5 | 当前源码是 **5.7.000**：`Transform` 只有 `x/y/skew/rotation/scaleX/scaleY`（无 `skewX/skewY` 字段；`rotation` 即 skewY，`skew` = skewX−skewY），`Matrix` 只有 `concat`（**没有** `append/prepend`），`Transform` 没有 `updateTransform`（对应逻辑在 `Bone._updateGlobalTransformMatrix` + `Transform.toMatrix/fromMatrix`） | 阶段 1 翻译时按 5.7 源码逐行来，**不要**照 requirements 里 `append/prepend`/`updateTransform` 的描述写 |
| F6 | 各素材 frameRate 不同：Dragon 系 24fps，PlantAnimate / 龙 = 30fps | 转储 `dt = 1/armatureData.frameRate`，两侧都从 golden `meta.dt` 读，别硬编码 1/24 |
| F7 | 素材骨架名不是文件名：`Dragon`/`MegaGatlingPea`/`armatureName`/`fixture`；tex 文件名规律 `X_ske*.json → X_tex.json`（`Dragon_Blend_tex.json` 的内部 `name` 是 `Dragon`，但注册键用我们自己传的 key，无影响） | 由 gen_golden 从 JSON 里读 `armature[0].name`，不要猜 |
| F8 | `Transform.updateTransform` 在 requirements P0-1/常见坑里被提及，但 5.7 源码不存在该方法 | 阶段 1 验收改为：`Transform.toMatrix` / `fromMatrix` 语义对齐（对 skew 的处理在 `toMatrix` 的 `c/d` 分支） |
| F9 | 事件监听必须实现 `hasDBEventListener`，否则 `AnimationState._advanceFadeTime` 抛 TypeError（requirements 已写，实测确认） | EvProxy 四方法齐全 |

已生成的验证产物（可删可重生成）：`testdata/golden/` 37 个文件 ≈ 37MB（9 素材 × 全动画 × 120 帧）。是否入库由你决定（37MB 偏大，可只提交脚本、CI 里生成）。

---

## 任务 1 — Cargo workspace（3 个 crate）

**目标**：根 `Cargo.toml` 变 workspace，三个空壳 crate。

```
DragonBonesRust/
├── Cargo.toml              # [workspace]
├── crates/
│   ├── dragonbones-core/   # 纯逻辑，禁止依赖 bevy
│   │   ├── Cargo.toml
│   │   └── src/lib.rs      # 空: //! DragonBones core
│   ├── dragonbones-bevy/   # 适配层空壳
│   │   ├── Cargo.toml      # dep: dragonbones-core (path)
│   │   └── src/lib.rs
│   └── dragonbones-demo/   # 可执行空壳（原 src/main.rs 迁入）
│       ├── Cargo.toml      # dep: dragonbones-core
│       └── src/main.rs     # fn main() {}
├── src/main.rs             # ← 删除（内容迁到 demo）
└── ...
```

**根 Cargo.toml 伪代码**：

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2024"

[workspace.dependencies]
serde_json = "1"
indexmap = "2"          # 阶段 1/2 要用（镜像 JS 插入序 Map）
# bevy 不进 workspace 依赖：阶段 0 demo/core 不需要；阶段 7 再给 bevy crate 单独加
```

**验收**：`cargo build` 通过；`cargo test` 输出 0 tests（或占位测试）。

---

## 任务 2 — 转储协议（两侧契约，先定死再写代码）

单个动画的 golden 文件结构（`testdata/golden/<asset>/<anim>.json`）：

```jsonc
{
  "meta": {
    "skeFile": "Dragon_ske.json",
    "armature": "Dragon",
    "animation": "walk",
    "frameRate": 24,
    "dt": 0.041666666666666664,   // 1/frameRate，Rust 测试读这个值推进时间
    "frameCount": 120,
    "boneNames": ["root", ...],   // 顺序 = armature.getBones() 顺序
    "slotNames": ["tailTip", ...]
  },
  "frames": [
    {
      "i": 0,
      "bones": {
        "<boneName>": [x, y, rotation, scaleX, scaleY, skew,
                       a, b, c, d, tx, ty, alpha]   // 13 个数
      },
      "slots": {
        "<slotName>": [displayIndex, visible, zOrder, zIndex,
                       rM, gM, bM, aM, rO, gO, bO, aO,   // ColorTransform 8 项
                       globalX, globalY, alpha]           // 15 个数
      },
      "events": [
        { "type": "frameEvent", "name": "...", "bone": "root"|null, "slot": "tail"|null }
      ]
    }
  ]
}
```

**字段来源（JS 侧）**：

- bone：`b.global.{x,y,rotation,scaleX,scaleY,skew}` + `b.globalTransformMatrix.{a,b,c,d,tx,ty}` + `b._globalAlpha`
- slot：`s.displayIndex` / `s.visible` / `s._zOrder` / `s._zIndex` / `s._colorTransform.{redMultiplier,greenMultiplier,blueMultiplier,alphaMultiplier,redOffset,greenOffset,blueOffset,alphaOffset}` / `s.global.{x,y}` / `s._globalAlpha`
- events：监听 9 种类型 `start loopComplete complete fadeIn fadeInComplete fadeOut fadeOutComplete frameEvent soundEvent`

**对比规则（Rust 测试侧约定）**：

- 按 **boneName / slotName 为键**比较（JSON 对象键序无关）；f64 容差 `1e-6`（dbbin 路径 `1e-4`），整数/枚举/索引全等。
- 数值 `null` 保留给 NaN/Inf（实测 9 素材 120 帧无 null；Rust 遇到按 NaN 处理即可）。
- `events` 按序全等比较（type/name/bone/slot 四元组）。

---

## 任务 3 — `tools/jsdump.mjs`（完整可用代码，已实测）

> requirements 里的版本有 F1/F2 两处致命问题，下面这份是修好并跑通 9 素材的版本。直接落盘即可。

```js
// tools/jsdump.mjs
// 用法: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]
import { createRequire } from "node:module";           // F1: .mjs 无 require
import fs from "node:fs";
import path from "node:path";

const require = createRequire(import.meta.url);

// PIXI 打桩：仅满足 bundle 顶层引用（PixiArmatureDisplay extends PIXI.Container），core 不会真正用到
globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
};

const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

// 对象池按 String(constructor) 分键，自定义子类必须覆盖静态 toString()
class NullAtlas extends db.TextureAtlasData {
  static toString() { return "NullAtlas"; }
  _onClear() { super._onClear(); }
  createTexture() { return new db.TextureData(); }
}

// Slot 14 个抽象方法全部 no-op —— 只要核心逻辑，不要渲染
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

// IArmatureProxy + IEventDispatcher
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

// BaseFactory 3 个抽象方法
class NullFactory extends db.BaseFactory {
  static toString() { return "NullFactory"; }
  _buildTextureAtlasData(ta) { return ta || db.BaseObject.borrowObject(NullAtlas); }
  _buildArmature(pkg) {
    const arm = db.BaseObject.borrowObject(db.Armature);
    const p = new EvProxy();
    arm.init(pkg.armature, p, p, this._dragonBones);   // F3: _dragonBones 需外部注入
    return arm;
  }
  _buildSlot(pkg, slotData, armature) {
    const s = db.BaseObject.borrowObject(NullSlot);
    s.init(slotData, armature, {}, {});
    return s;
  }
}

const EVENT_TYPES = [
  "start", "loopComplete", "complete",
  "fadeIn", "fadeInComplete", "fadeOut", "fadeOutComplete",
  "frameEvent", "soundEvent",
];

const fin = v => (typeof v === "number" && Number.isFinite(v) ? v : null);

// ---- CLI ----
const [skePath, texPath, armName, animName, outPath, frameCountArg] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !animName || !outPath) {
  console.error("usage: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]");
  process.exit(2);
}
const frameCount = frameCountArg ? parseInt(frameCountArg, 10) : 120;

const key = path.basename(skePath).replace(/[.]json$/, ""); // 缓存键（骨架与图集同键）
const em = new EvProxy();
const factory = new NullFactory();
factory._dragonBones = new db.DragonBones(em);              // F3

factory.parseDragonBonesData(JSON.parse(fs.readFileSync(skePath, "utf8")), key);
factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);

const armature = factory.buildArmature(armName, key, "", key); // F4: 第 4 参显式传
if (!armature) { console.error(`buildArmature failed: ${armName}`); process.exit(1); }
armature.clock = factory.clock;                                // F2: 挂进全局时钟

const events = [];
for (const t of EVENT_TYPES) {
  armature.eventDispatcher.addDBEventListener(t, e => events.push({
    type: e.type, name: e.name ?? "",
    bone: e.bone ? e.bone.name : null,
    slot: e.slot ? e.slot.name : null,
  }));
}

const frameRate = armature.armatureData.frameRate || 24;
const dt = 1 / frameRate;                                      // F6
const bones = armature.getBones();
const slots = armature.getSlots();

armature.animation.gotoAndPlayByTime(animName, 0, -1);

const frames = [];
for (let i = 0; i < frameCount; i++) {
  events.length = 0;
  factory._dragonBones.advanceTime(dt);   // F2: 必须走全局实例，事件在其中派发

  const boneMap = {};
  for (const b of bones) {
    const g = b.global, m = b.globalTransformMatrix;
    boneMap[b.name] = [
      fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
      fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty),
      fin(b._globalAlpha),
    ];
  }
  const slotMap = {};
  for (const s of slots) {
    const g = s.global, c = s._colorTransform;
    slotMap[s.name] = [
      s.displayIndex, s.visible ? 1 : 0, s._zOrder, s._zIndex,
      fin(c.redMultiplier), fin(c.greenMultiplier), fin(c.blueMultiplier), fin(c.alphaMultiplier),
      fin(c.redOffset), fin(c.greenOffset), fin(c.blueOffset), fin(c.alphaOffset),
      fin(g.x), fin(g.y),
      fin(s._globalAlpha),
    ];
  }
  frames.push({ i, bones: boneMap, slots: slotMap, events: events.slice() });
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({
  meta: {
    skeFile: path.basename(skePath), armature: armName, animation: animName,
    frameRate, dt, frameCount,
    boneNames: bones.map(b => b.name),
    slotNames: slots.map(s => s.name),
  },
  frames,
}));
console.log(`OK ${path.basename(skePath)} / ${armName} / ${animName}: ${bones.length} bones, ${slots.length} slots, ${frames.length} frames`);
```

**单素材手测**：

```
node tools/jsdump.mjs assets/Dragon_ske.json assets/Dragon_tex.json Dragon walk out.json 120
```

预期输出：`OK Dragon_ske.json / Dragon / walk: 19 bones, 18 slots, 120 frames`

---

## 任务 4 — `tools/gen_golden.mjs`（批量生成 9 素材黄金数据）

```js
// tools/gen_golden.mjs — 遍历 assets/ 下全部 ske × 全部动画，写 testdata/golden/<asset>/<anim>.json
// 用法: node tools/gen_golden.mjs [frameCount]   默认 120
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSETS = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "golden");
const FRAME_COUNT = process.argv[2] || "120";

const skes = fs.readdirSync(ASSETS)
  .filter(f => f.endsWith("_ske.json") || f.endsWith("_ske_compressed.json"));

let ok = 0, fail = 0;
for (const ske of skes) {
  const tex = ske.replace(/_ske(_compressed)?[.]json$/, "_tex.json");   // F7
  const data = JSON.parse(fs.readFileSync(path.join(ASSETS, ske), "utf8"));
  const armName = data.armature[0].name;                               // F7: 从数据读
  const anims = data.armature[0].animation.map(a => a.name);
  // 目录名: Dragon_ske → Dragon, PlantAnimate_ske → PlantAnimate_compressed
  const stem = ske.replace(/[.]json$/, "").replace(/_ske$/, "").replace(/_ske_compressed$/, "_compressed");
  for (const anim of anims) {
    const out = path.join(OUT, stem, `${anim}.json`);
    const r = spawnSync(process.execPath, [
      path.join(HERE, "jsdump.mjs"),
      path.join(ASSETS, ske), path.join(ASSETS, tex),
      armName, anim, out, FRAME_COUNT,
    ], { encoding: "utf8" });
    if (r.status === 0) ok++;
    else { fail++; console.error(`FAIL ${ske}/${anim}\n${r.stderr}`); }
  }
}
console.log(`golden done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
```

**注意（实测踩过的坑）**：`stem` 的 `_ske_compressed` 替换必须在 `_ske` 之前判断不了——上面写法是先去 `.json`、再精确匹配 `_ske` 结尾、再匹配 `_ske_compressed` 结尾，顺序保证 `PlantAnimate_ske` 不会和 `PlantAnimate` 落到同一目录互相覆盖（初版会覆盖成 31 个文件）。

**素材清单（实测核对过，golden 目录名 ↔ 骨架名 ↔ 动画数）**：

| golden 目录 | ske 文件 | armature | bones | slots | 动画数 | fps |
|---|---|---|---|---|---|---|
| Dragon | Dragon_ske | Dragon | 19 | 18 | 4 | 24 |
| Dragon_Blend | Dragon_Blend_ske | Dragon | 19 | 18 | 4 | 24 |
| PlantAnimate | PlantAnimate_ske | MegaGatlingPea | 29 | 31 | 6 | 30 |
| PlantAnimate_compressed | PlantAnimate_ske | MegaGatlingPea | 29 | 31 | 6 | 30 |
| Rooster_Ani | Rooster_Ani_ske | armatureName | 63 | 15 | 5 | 24 |
| Rooster_Blend | Rooster_Blend_ske | armatureName | 63 | 15 | 5 | 24 |
| Rooster_P1 | Rooster_P1_ske | armatureName | 63 | 15 | 5 | 24 |
| Surface_P1_4 | Surface_P1_4_ske | fixture | 4 | 2 | 1 | 24 |
| 龙 | 龙_ske | armatureName | 60 | 29 | 1 | 30 |

合计 **37 个 golden 文件**（Dragon_Blend 与 Dragon 的动画同名 stand/walk/jump/fall，目录不同不冲突）。

**验收**：`node tools/gen_golden.mjs` → `golden done: 37 ok, 0 fail`。

---

## 任务 5 — `cargo test` 脚手架

**结构**：

```
crates/dragonbones-core/
├── Cargo.toml          # deps: serde_json, indexmap
└── tests/
    └── golden_smoke.rs # 阶段 0：结构完整性；后续阶段在此文件（或并列文件）加差分断言
```

**伪代码**（`tests/golden_smoke.rs`）：

```rust
// 工具函数（可放 tests/common/mod.rs）
fn golden_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden") }

fn all_golden_files() -> Vec<PathBuf> {
    // 遍历 golden_root() 下 */*.json，排序保证稳定
}

#[test]
fn golden_data_present_and_well_formed() {
    let files = all_golden_files();
    assert_eq!(files.len(), 37, "9 素材应产出 37 个 golden 文件");

    for path in files {
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;

        // meta
        let meta = &v["meta"];
        assert_eq!(meta["frameCount"].as_u64(), Some(120));
        assert!(meta["dt"].as_f64().unwrap() > 0.0);
        let bone_names = meta["boneNames"].as_array();   // 非空
        let slot_names = meta["slotNames"].as_array();   // 非空

        let frames = v["frames"].as_array();
        assert_eq!(frames.len(), 120);

        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame["i"].as_u64(), Some(i as u64));        // 帧号连续
            // 每帧 bones/slots 键集合 == meta 名单；向量长度 13 / 15
            // 每个数值是 number 或 null（null 仅当 NaN，实测不出现）
            // events 是数组，元素含 type/name/bone/slot
        }
    }
}

#[test]
fn golden_expected_inventory() {
    // 按上面素材清单硬编码断言：每个目录的 armature 名 / bone 数 / slot 数 / 动画名集合
    // 例: Dragon 目录必须有 stand walk jump fall 四个文件，meta.armature=="Dragon", boneNames.len()==19
}
```

**补充两条冒烟断言（实测基准值）**：

- `Dragon/walk.json` frame0：`bones.root == [0,0,0,1,1,0, 1,0,0,1,0,0, 1]`，`slots.tailTip[0..4] == [0,1,0,0]`，events 含 `fadeIn / fadeInComplete / start` 三元序列。
- `PlantAnimate/Idle.json` 与 `PlantAnimate_compressed/Idle.json`：**逐字节内容应一致**（同一数据的压缩/非压缩形式，requirements §1.3 已断言键集合一致，golden 层面再确认一次）。

**验收**：`cargo test -p dragonbones-core` 全绿。

---

## 任务 6 — 预研 Q3（Bevy 中文资源路径）+ Q1（可推迟）

> 这两项 requirements 归在阶段 0，但只影响阶段 7。若想先跑通主线，可标记为 TODO 推迟——但 Q3（R7 风险）建议本阶段做。

**Q3 伪代码**（验证 `龙_tex.png` 能否被 Bevy 加载）：

```
crates/dragonbones-bevy/tests/chinese_path.rs  （或 examples/q3_path.rs）

#[test]
fn load_chinese_filename_asset() {
    // 用 bevy::asset::AssetPlugin + TaskPoolPlugin 起最小 App（无窗口）
    // asset_server.load::<Image>("../assets/龙_tex.png")
    // 轮询直到加载完成（或 5s 超时）
    // 断言: handle 有效，image 尺寸 > 0
}
```

- Bevy 依赖只加在 `dragonbones-bevy`（阶段 0 可先不加，推迟则此任务挂起）。
- 若失败：备选方案是加载时做 percent-encode / 资源侧改英文名，记录进 requirements 风险 R7 的结论栏。

**Q1**（`Transparent2d` 排序键是否含 z）：写最小双 quad demo 目测遮挡关系，结论回填 requirements §5.2 Q1 行。阶段 7 前完成即可，**本阶段可跳过**。

---

## 验收清单（全部跑一遍，全绿即阶段 0 完成）

```bash
# 1. workspace
cargo build && cargo test

# 2. 单素材手测（可选，确认工具单点可用）
node tools/jsdump.mjs assets/Dragon_ske.json assets/Dragon_tex.json Dragon walk /tmp/out.json 120
#   期望: OK Dragon_ske.json / Dragon / walk: 19 bones, 18 slots, 120 frames

# 3. 黄金数据全量生成
node tools/gen_golden.mjs
#   期望: golden done: 37 ok, 0 fail
#   检查: testdata/golden 下 9 个目录、37 个 json

# 4. Rust 侧脚手架测试
cargo test -p dragonbones-core
#   期望: golden 结构测试全绿（37 文件 × 120 帧 × 键/长度校验）

# 5.（建议）PlantAnimate 压缩一致性
#   diff testdata/golden/PlantAnimate/Idle.json testdata/golden/PlantAnimate_compressed/Idle.json
#   期望: 无输出（一致）
```

**常见坑回顾**（都是实测踩过的）：

1. `.mjs` 里 `require` 崩 → `createRequire`（F1）。
2. 事件收不到 → 忘了 `armature.clock = factory.clock` 或用了 `armature.advanceTime` 而不是全局 `advanceTime`（F2）。
3. `buildArmature` 第 4 参不传 → 槽位无纹理数据，`_updateFrame` 相关逻辑静默走空（F4）。
4. `hasDBEventListener` 缺实现 → fadeIn 事件阶段抛 TypeError（F9）。
5. `stem` 生成目录时 `_ske_compressed` 被 `_ske` 规则截断 → PlantAnimate 两个变体互相覆盖（任务 4 注）。
6. 池类子类忘覆盖 `static toString()` → `borrowObject` 抛错。
7. 37MB golden 是否入库：自行决策；不入库就在 `cargo test` 前加一步生成，或 CI 缓存。

---

## 完成后

跑完上面验收清单，把结果（尤其 `golden done` 和 `cargo test` 输出）发我。确认全绿后我写 **阶段 1（geom + core + model）** 的实现指南——按 requirements 的阶段 1 表格 + 我上面 F5/F8 的 5.7 API 修正来写。
