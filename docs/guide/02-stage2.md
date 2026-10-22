# 阶段 2 实现指南 — 解析器与 IR（M2 最高风险区）

> 配套 `docs/requirements.md` §3 阶段 2 + `docs/model_mapping.md` **§4.1（IR 量化事实，必读）**。
> 按任务 1→8 顺序实现，验收清单全绿后告诉我，我写阶段 3（armature 层级）指南。
> 范围外：dbbin 二进制解析（P1-1，阶段 6）、`_parserDragonBonesData` 写出路径（P1-10，阶段 6）。
> 本指南所有"实测"结论都是我对 5.7 源码 / 9 素材 JSON / 参考运行时实际验证过的。

---

## 0. 已实测事实与 requirements 修正（先读）

| # | 事实 | 影响 |
|---|---|---|
| F2.1 | **版本门**：`version` 或 `compatibleVersion` 命中 `DATA_VERSIONS = [4.0, 4.5, 5.0, 5.5, 5.6]` 之一即接受（`ObjectDataParser.ts:2294-2303`）。`2.3`/`3.0` 常量存在但**不在接受列表**；`frameRate` 默认 24、`===0` 时修正为 24（data 级与 armature 级各一次） | requirements P0-5 "支持 2.3/3.0" 不准确——2.3/3.0 数据只有 `compatibleVersion` 是新版才被接受；9 素材全 5.5，行为以版本门为准 |
| F2.2 | **`Math.round` 共 18 处**：1242, 1243, 1456, 1477, 1482, 1487, 1513, 1544, 1545, **1753–1760**, 1881 | requirements D2 清单漏了 1243、1545、1759、1760——以本清单为准，`js_round` 必须覆盖全部 18 处 |
| F2.3 | **IR 黄金已生成**：`testdata/ir/` 9 文件 / 0.8MB / 四段 `meta+structure+atlas+arrays`；`tools/irdump.mjs` + `tools/gen_ir.mjs` 已验证 9/9 | 阶段 2 验收 = Rust 解析产物与这 9 个文件全等（数组逐元素 + 结构快照 + 图集快照） |
| F2.4 | **素材动画键位实测**：`bone[].{translateFrame,rotateFrame,scaleFrame,frame}`、`slot[].{displayFrame,colorFrame}`、顶层 `frame`（仅 PlantAnimate，action 帧）、`curve`（Plant+Rooster，贝塞尔缓动）、`ffd`（仅龙）、`events`（仅 Plant，frameEvent）。**generic `"timeline"` 路径被 Surface_P1_4 使用（type=50 Surface）** | `ObjectDataParser.ts:994-1209` 的 TimelineType 分发**必须实现**，不是可选项；`zIndexFrame`/`userData`/`canvas` 9 素材均未使用（golden 覆盖不到，阶段 6 前留意） |
| F2.5 | `ActionFrame` 类在 `ObjectDataParser.ts:2432`（文件末尾小类）；`_actionFrames` 在 `_parseAnimation` 尾部 `length = 0` 清空 | 阶段 2 需要该类型；跨动画复用前必须清空 |
| F2.6 | `_modifyArray` 有 **5 个 padding 分支**，第 5 个"条件读 `timelineArray.length`、写 `colorArray`"（`ObjectDataParser.ts:2229-2235`，原文见任务 8） | 逐行镜像，**勿"修复"**——padding 影响 IR 数组长度，逐元素断言必须一致 |
| F2.7 | TypedArray store 语义：**i16/u16 = wrap（非 clamp）**，**f32 = round-to-nearest** | 用任务 1 的 `js_store_i16/i16/f32` helper，勿用饱和转换 |
| F2.8 | **rotation 连续性状态机 `_prevClockwise`/`_prevRotation` 是 parser 实例级字段、不按 timeline 重置**；`ObjectDataParser.ts:1655` 的 `prevRotation + rotation - prevRotation + PI_D*cw` 中间项在 f64 下**不一定抵消** | 照抄原表达式，**不化简、不重置**——首关键帧 `frameStart!==0` 时用到上个 timeline 残留是 JS 的真实行为（golden 会验证） |
| F2.9 | `stage` 未指定时**默认取第一个骨架**（`else if (armatureNames.length > 0)`，`ObjectDataParser.ts:2327-2331`） | 实测 Dragon 的 `meta.stage = "Dragon"`，照抄该分支 |
| F2.10 | `_parseTransform` 双分支（2041-2047）：`ROTATE/SKEW` 键 → `rotation = normalizeRadian(ROTATE×DEG_RAD)`、`skew = normalizeRadian(SKEW×DEG_RAD)`；否则 `SKEW_X/SKEW_Y` 键 → `rotation = normalizeRadian(skY×DEG_RAD)`、`skew = normalizeRadian(skX×DEG_RAD) - rotation`。`x/y ×scale`，`scaleX/scaleY` **不 ×scale** | 两分支条件是 `in` 判断不是值判断；`skew = skX − rotation` 的减法在 normalize **之后** |

---

## 1. 关键技术决策（D-2.1 – D-2.5）

### D-2.1 入口收 `serde_json::Value`，三个 helper 用精确真值表（D5 不用 derive）

`key in rawData` 在 Rust 必须区分三态：`raw.get(key) == None`（键不存在）/ `Some(Null)`（键存在值为 null）/ `Some(v)`。三个 helper 的**精确语义表**（对照 `ObjectDataParser.ts:36-92`，逐分支实现）：

```rust
// parser/js_compat.rs
pub fn js_round(v: f64) -> f64 { (v + 0.5).floor() }   // 半数向 +∞，不是 Rust round()

/// JS `+value || 0`：取数字，失败给 0
pub fn js_to_number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),   // NaN 经 ||0 兜底
        Value::String(s) => s.parse::<f64>().unwrap_or(f64::NAN), // "" → NaN, "12.5" → 12.5, "abc" → NaN
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => f64::NAN,                                        // null/数组/对象 → NaN
    }
}
// 统一出口：(js_to_number(v)).or_zero()  即 NaN → 0.0（JS `|| 0` 语义）

pub fn get_number(raw: &Value, key: &str, default: f64) -> f64 {
    match raw.get(key) {
        None => default,                                  // 键不存在
        Some(Value::Null) => default,                     // null → default（66-71 行）
        Some(v) if v == "NaN" => default,                 // 字符串 "NaN" → default
        Some(v) => js_to_number(v).or_zero(),             // else +value || 0
    }
}

pub fn get_boolean(raw: &Value, key: &str, default: bool) -> bool {
    match raw.get(key) {
        None => default,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !matches!(s.as_str(), "0" | "NaN" | "" | "false" | "null" | "undefined"),
        Some(Value::Null) => false,                        // !!null（44-60 行 else 分支）
        Some(v) => !v.is_null() && ... ,                   // !!value：0 → false，非零数 → true
    }
}

pub fn get_string(raw: &Value, key: &str, default: &str) -> String {
    match raw.get(key) {
        None => default.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),           // ⚠ String(null) = "null"，不是 default！
        Some(Value::Number(n)) => number_to_js_string(n),  // 整数无 ".0"（24 → "24"）
        Some(Value::Bool(b)) => b.to_string(),
    }
}
```

**三个最容易翻车的点**：`get_string(Some(Null)) = "null"`（JS `String(null)`）；`get_number` 的 null 和缺键都回 default；`get_boolean` 的 null 走 `!!null = false` 而非 default。任务 1 的测试必须用真值表锁死这三类。

### D-2.2 Parser = 单个可变状态结构体（镜像 TS parser 实例字段）

```rust
pub struct ObjectDataParser {
    // 上下文（解析过程中层层切换）
    data: Option<DragonBonesData>,      // _data
    armature: Option<ArmatureData>,     // _armature
    bone: Option<BoneData>,             // _bone（当前）
    geometry: Option<GeometryData>,     // _geometry
    slot: Option<SlotData>,             // _slot
    skin: Option<SkinData>,             // _skin
    mesh: Option<MeshDisplayData>,      // _mesh
    animation: Option<AnimationData>,   // _animation（当前，三基准在它身上）
    timeline: Option<TimelineData>,     // _timeline（当前写入中）
    // 帧解析状态
    frame_value_type: FrameValueType,   // Step/Int/Float（_parseTimeline 设置）
    frame_default_value: f64,           // 0.0 或 1.0（scale 帧用 1.0）
    frame_value_scale: f64,
    prev_clockwise: i32,                // rotation 连续性状态机（F2.8，不按 timeline 重置）
    prev_rotation: f64,
    action_frames: Vec<ActionFrame>,    // _actionFrames（F2.5，动画解析尾部清空）
    // IR 写入缓冲（对应 TS 的7 个 number[]，量化在 finalize_root）
    int_array: Vec<i32>, float_array: Vec<f64>,
    frame_int_array: Vec<i32>, frame_float_array: Vec<f64>,
    frame_array: Vec<i32>, timeline_array: Vec<i32>, color_array: Vec<i32>,
    // help 对象（对应 _helpMatrixA/B、_helpTransform、_helpColorTransform、_helpPoint、_helpArray）
    help_transform: Transform, help_color: ColorTransform, help_array: Vec<f64>, ...
}
```

### D-2.3 IR 相对偏移三基准（照 TS，勿自行发明）

- `animation.frame_int_offset / frame_float_offset / frame_offset` 在 **`_parseAnimation` 最开头捕获**（892-894，先于任何 action/timeline 解析）——该动画之后所有写入都相对这三个基准。
- `_parseTimeline` 的 `frame_value_offset = 当前数组长度 − animation.frame*_offset`（相对）；`timeline.offset = timelineArray 当前长度`（**绝对**）；`frameIndices` 追加到 `data.frameIndices`，`timeline.frame_indices_offset` 为绝对索引，单关键帧时 `= -1`。
- `frameArray` 内每帧写入值 = `frame_parser(...) − animation.frame_offset`（**相对**）。

### D-2.4 finalize_root 量化 = `model_mapping.md` §4.1 五步

parse 收尾把7 个缓冲交给 `DragonBonesData::finalize_root(ir)`，内部先执行 §4.1 的量化镜像（f32 / i16 wrap / u16 wrap / padding），再 `Arc::new(RootInfo)` 回填。

### D-2.5 黄金对比法

- **8 组数组**：精确相等（`==`）。int 组双方都是 wrap 后的 i12 值域整数；float 组双方都是 f32 量化后的 f64（bit 相等）。不设容差——任何不等都是真差异。
- **structure / atlas / meta 快照**：先 `norm()`（递归把所有 `Number` 重写为 `Number::from_f64(as_f64)`）消除 JS 整数字面量（`24` → u64）与 Rust 浮点（`1.0` → f64）的序列化变体差异，再 `assert_eq!(Value, Value)`（serde_json 默认 `Map` 是 BTreeMap，对象比较与键序无关，数组有序）。

---

---

## 任务 1 — `parser/js_compat.rs` 语义助手

**目标**：把 D2 表里全部 JS 数值语义收进一个模块，后续 2400 行翻译只调 helper，不裸写运算。

```rust
// crates/dragonbones-core/src/parser/js_compat.rs
pub fn js_round(v: f64) -> f64                    // (v + 0.5).floor()，18 处调用点见 F2.2
pub fn js_to_number(v: &Value) -> f64              // D-2.1 表
pub fn or_zero(v: f64) -> f64                      // NaN → 0（JS `|| 0`）
pub fn get_number(raw: &Value, key: &str, default: f64) -> f64
pub fn get_boolean(raw: &Value, key: &str, default: bool) -> bool
pub fn get_string(raw: &Value, key: &str, default: &str) -> String
pub fn has_key(raw: &Value, key: &str) -> bool     // `key in rawData`（含值为 Null 的键！）
pub fn number_to_js_string(n: &Number) -> String   // JS String(number)：整数无 ".0"
// TypedArray store 语义（F2.7，finalize_root 用）
pub fn js_store_i16(v: f64) -> i32                 // 向零截断 → mod 2^16 → i16 值域 → widens
pub fn js_store_u16(v: f64) -> i32                 // 向零截断 → mod 2^16 → u16 值域 → widens
pub fn js_store_f32(v: f64) -> f64                 // (v as f32) as f64（round-to-nearest）
```

**测试**（`tests/js_compat.rs`，真值表逐行断言）：

```rust
// get_number:  缺键→default；Null→default；"NaN"→default；""→0；"abc"→0；"12.5"→12.5；
//              true→1；false→0；0→0；数组/对象→0
// get_boolean: 缺键→default；"0"/"NaN"/""/"false"/"null"/"undefined"→false；"abc"→true；
//              Null→false；0→false；1→true
// get_string:  缺键→default；Null→"null"（不是 default！）；24→"24"；12.5→"12.5"；true→"true"
// js_round:    0.5→1；-0.5→-0（==0.0）；-1.5→-1；1.5→2；2.5→3   ← 与 Rust round()(-1.5→-2) 的差异点
// js_store_i16/-u16: 65535→-1/65535；-1→-1/65535；40000→-25536/40000；3.7→3；-3.7→-3
```

---

## 任务 2 — `parser/data_parser.rs`（DataParser.ts，309 行）

**内容**：
1. ~130 个数据键常量（`pub const ARMATURE: &str = "armature";` …，照 `DataParser.ts:45-309` 抄，顺序不变）。
2. 版本表与版本门（F2.1）：

```rust
pub const DATA_VERSIONS: [&str; 5] = ["4.0", "4.5", "5.0", "5.5", "5.6"];
// 注意：2.3/3.0 常量可定义（DATA_VERSION_2_3 等）但不进接受列表（F2.1）
pub fn is_supported_version(raw: &Value) -> bool {
    let v = get_string(raw, VERSION, ""); let cv = get_string(raw, COMPATIBLE_VERSION, "");
    DATA_VERSIONS.contains(&v.as_str()) || DATA_VERSIONS.contains(&cv.as_str())
}
```

3. string → 枚举的映射函数（`_getArmatureType` / `_getBlendMode` / `_getTweenType`? / `_getPositionMode` / `_getSpacingMode` / `_getRotateMode` / `_getAnimationBlendType` / `_getAnimationFadeOutMode`…）——**逐个对照 `DataParser.ts` 尾部实现**，注意：type 键既有字符串形态（`"Armature"`）又有数字形态（`0`），`_parseArmature`/`_parseSlot` 里的 `typeof === "string"` 双分支要保留（F2.10 同款模式）。

---

## 任务 3 — IR 黄金基建（**已交付**，你只需写 Rust 侧测试）

**已生成（我实测 9/9）**：
- `tools/irdump.mjs` — 参考运行时转储单素材四段：`meta`（版本/骨架名/frameRate/stage）、`structure`（骨架树：bones/slots/constraints/skins/animations + sortedBones 序 + timeline offset/type）、`atlas`（图集 + SubTexture region）、`arrays`（8 组 IR）
- `tools/gen_ir.mjs` — 批量驱动 → `testdata/ir/*.json`（9 文件 / 0.8MB）
- 重新生成：`node tools/gen_ir.mjs`（确定性，无随机种子）

**Rust 侧测试**（`tests/ir_golden.rs`）伪代码：

```rust
const ASSETS: &[(&str, &str)] = &[
    ("Dragon", "Dragon_ske"), ("Dragon_Blend", "Dragon_Blend_ske"),
    ("PlantAnimate", "PlantAnimate_ske"), ("PlantAnimate_compressed", "PlantAnimate_ske"),
    ("Rooster_Ani", "Rooster_Ani_ske"), ("Rooster_Blend", "Rooster_Blend_ske"),
    ("Rooster_P1", "Rooster_P1_ske"), ("Surface_P1_4", "Surface_P1_4_ske"), ("龙", "龙_ske"),
]; // tex 文件名 = ske 名替换 _ske(_compressed)?.json → _tex.json

fn parse_asset(stem: &str) -> (DragonBonesData, TextureAtlasData) {
    // 读 assets/{stem}.json → ObjectDataParser::default().parse_dragon_bones_data(&raw, 1.0)
    // 读对应 tex → parse_texture_atlas_data(&raw_tex, &mut atlas, 1.0)
}

/// D-2.5：递归把 Number 重写为 from_f64(as_f64)，消除 u64/i64/f64 序列化变体
fn norm(v: Value) -> Value;

#[test] fn ir_arrays_match_js_elementwise() {
    for (dir, stem) in ASSETS {
        let golden: Value = read(&format!("testdata/ir/{dir}.json"));
        let (data, _) = parse_asset(stem);
        let ir = &data.root.expect("finalize_root 已调用").ir;
        let g = &golden["arrays"];
        // int 组：i32 == i64（golden 经 as_i64→as_i32）；float 组：f64 精确 ==
        assert_eq!(ir.int_array, to_i32_vec(&g["intArray"]));
        assert_eq!(ir.float_array, to_f64_vec(&g["floatArray"]), "{dir} floatArray");   // bit 相等
        assert_eq!(ir.frame_int_array, ...);  assert_eq!(ir.frame_float_array, ...);
        assert_eq!(ir.frame_array, ...);      assert_eq!(ir.timeline_array, ...);
        assert_eq!(ir.color_array, ...);      assert_eq!(ir.frame_indices, ...);
    }
}

#[test] fn structure_snapshot_matches_js() {
    for (dir, stem) in ASSETS {
        let golden: Value = read(...);
        let (data, atlas) = parse_asset(stem);
        // snapshot 三件套：meta / structure / atlas（见下）
        assert_eq!(norm(snapshot_meta(&data)), norm(golden["meta"].clone()), ...);
        assert_eq!(norm(snapshot_structure(&data)), norm(golden["structure"].clone()), ...);
        assert_eq!(norm(snapshot_atlas(&atlas)), norm(golden["atlas"].clone()), ...);
    }
}
```

**`snapshot_*` 三件套**放 `tests/common/ir_snapshot.rs`：与 `tools/irdump.mjs` 的对应段**一一对应**（字段清单直接照该文件96-232 行翻译：`tf()` transform 6 元数组、`col()` color 8 元数组、`tl()` timeline 的 type/offset/frameIndicesOffset(+x,y)、bones 的 parent/type/length/alpha/inherit*/transform(+surface geometry)、slots 的 parent/displayIndex/zOrder/zIndex/alpha/blendMode/color、constraints 的 type/order/target/root/bone(+IK/Path 变体字段)、skins 的 display 列表（name/type/path/transform/pivot/actions/geometry/boundingBox/path 专属键）、animations 的 duration/playTimes/三基准/actionTimeline/zOrderTimeline/四类 timelines、atlas 的 region/frame 数组）。

> snapshot 的数值序列化注意：JS 里 `alpha: 1` 输出整数 `1`，Rust `f64` 输出 `1.0`——靠 `norm()` 消解；**靠 `norm()` 消解，不要手工把 f64 转整数**。

---

## 任务 4 — `parser/object_data_parser.rs` 骨架 + 入口

**文件**：`parser/object_data_parser.rs`（对应 `ObjectDataParser.ts` 2436 行，D7 单文件映射；`js_compat` 独立模块）。结构体字段见 D-2.2。

**入口 1**：`parse_dragon_bones_data`（`ObjectDataParser.ts:2294-2353`）伪代码：

```rust
pub fn parse_dragon_bones_data(&mut self, raw: &Value, scale: f64) -> Option<DragonBonesData> {
    if !is_supported_version(raw) { return None; }              // F2.1：console.assert 失败→None
    let mut data = DragonBonesData::default();                  // Rust 无需 borrowObject
    data.version = get_string(raw, VERSION, "");
    data.name     = get_string(raw, NAME, "");
    data.frame_rate = get_number(raw, FRAME_RATE, 24.0);
    if data.frame_rate == 0.0 { data.frame_rate = 24.0; }       // F2.1

    if has_key(raw, ARMATURE) {
        self.data = Some(data);                                 // 先挂上下文（_data）
        self.parse_array();                                     // 清空7 个 IR 缓冲
        for raw_armature in raw[ARMATURE].as_array()? {
            let armature = self.parse_armature(raw_armature, scale);
            self.data_mut().add_armature(armature);
        }
        self.modify_array();                                    // 量化 + padding + 挂 binary（任务 8）
        // stage 解析（F2.9）：
        if has_key(raw, STAGE) { data.stage = Some(get_string(raw, STAGE, "")) }
        else if !data.armature_names.is_empty() { data.stage = Some(data.armature_names[0].clone()) }
        self.data = None;
    }
    if has_key(raw, TEXTURE_ATLAS) { /* _rawTextureAtlases 延迟解析：本阶段在独立入口处理 */ }
    Some(data)
}
```

**关键时机**：`stage` 解析在 `modify_array` **之后**、`data` 移出**之前**；`finalize_root` 不在此处调用——见任务 8（IR 缓冲要先量化）。

**入口 2**：`parse_texture_atlas_data`（`2354-2420`，结构最简单，按 requirements 翻译顺序先打通）：textureAtlas 的 width/height/name/imagePath/autoSearch + `SubTexture[]` 循环（name/rotated/region×scale/frame，`frame` 只在 frameWidth>0 时创建——逐行对照）。

**本任务验收**：`parse_dragon_bones_data(Dragon_ske)` 返回 Some，`data.armatures.len()==1`，8 组 IR 缓冲长度与 `testdata/ir/Dragon.json` 一致（先对 `frameFloat=536, frame=536, timeline=828, indices=1210, int=0, color=0` 这组冒烟值）。

---
## 任务 5 — armature 静态结构（bone / slot / skin / display / constraint）

按 requirements 翻译顺序逐段翻，行号区间与已知坑：

| 区间 | 内容 | 坑 |
|---|---|---|
| `285-472` | `_parseArmature` | `frameRate` 默认取 data 级、0→24；`type` 键**字符串/数字双分支**（`typeof === "string"` → `_getArmatureType`，否则 `_getNumber`）；canvas 的 x/y/w/h **×armature.scale**；aabb ×scale；`defaultActions`/`actions` 分支；bone→slot→constraint→skin→animation 的**解析顺序**（slot 依赖 bone，skin display 的 armature 显示依赖后续回填——照抄顺序） |
| `473-510` | `_parseBone` | inherit 四键默认全 `true`（`_getBoolean(..., true)`！注意与 `_onClear` 的 false 不同——**解析缺键= true**）；transform ×scale；`surface` 类型键 |
| `511-592` | `_parseIKConstraint` / `_parsePathConstraint` | IK：`bendPositive` 默认 true？（逐行对照）；Path：`rotateMix/translateMix` 解析缺省 **1**（与 `_onClear` 的 0 不同，`model_defaults.rs` 已埋注释）；`pathSlot` 由 slot 名回查、`bones[]` 逐个回查、`root` 取第一个有效骨 |
| `593-623` | `_parseSlot` | `blendMode` **字符串/数字双分支**（`BLEND_MODE in && typeof === "string"` → `_getBlendMode` else `_getNumber`）；`color` 键存在 → `createColor + _parseColorTransform`，否则 **`DEFAULT_COLOR` 共享单例**（见偏差 D-2.6 下方）；`zOrder` 取参数不读 JSON |
| `624-664` | `_parseSkin` | `display[]` 循环 + `displayIndex` 越界补 null（`slotDisplays[i] = null` 那段边界逻辑逐行翻） |
| `665-792` | `_parseDisplay` 及子解析 | `type` 字符串（`"image"`…）与数字双分支 → `DisplayType`；Image 的 `PIVOT` 键（x,y ×scale）；Armature 显示的 `inheritAnimation` 默认 true + `actions`；Mesh → `_parseGeometry`；BoundingBox → Rectangle/Ellipse/Polygon 三分支（Polygon 顶点 ×scale）；Path 的 closed/constantSpeed |
| `793-877` | `_parseBoundingBox` / `_parsePolygonBoundingBox` | vertices ×scale |

**D-2.6（新偏差，记入 model_mapping §4）**：TS 的 `SlotData.DEFAULT_COLOR` 是**共享静态单例**，无 color 键的 slot 全部指向同一对象——运行时若原地改 slot.color 会串扰。Rust 用 `ColorTransform` 值拷贝（`SlotData::DEFAULT_COLOR` const），本项目运行时不会原地改 data 层 color，行为等价。

**本任务验收**：`parse_asset("Dragon")` 后 structure 快照的 `bones/slots/skins` 段与 golden 全等（`sortedBones` 19 项顺序、18 slots 的 parent/blendMode/color、19 displays……）。

---

## 任务 6 — animation / timeline / frame（**核心，占 60% 篇幅**）

### 6.1 `_parseAnimation`（878-1209）

```
fn parse_animation(&mut self, raw) -> AnimationData:
    animation.blend_type   = _getAnimationBlendType(get_string(BLEND_TYPE, ""))
    animation.frame_count  = get_number(DURATION, 0) as i32      // 注意 frameCount 语义是整数
    animation.play_times   = get_number(PLAY_TIMES, 1) as i32
    animation.duration     = frame_count / armature.frame_rate    // f64 除法
    animation.fade_in_time = get_number(FADE_IN_TIME, 0)
    animation.scale        = get_number(SCALE, 1)
    animation.name         = get_string(NAME, DEFAULT_NAME)，空→DEFAULT_NAME
    // ★ 三基准在此捕获（D-2.3），先于一切写入：
    animation.frame_int_offset  = self.frame_int_array.len()
    animation.frame_float_offset= self.frame_float_array.len()
    animation.frame_offset      = self.frame_array.len()
    self.animation = Some(animation)

    // 1) 顶层 frame 键 → action 帧累积（frameStart 按 DURATION 递增；仅 PlantAnimate 用到）
    //    每帧 _parse_action_data_in_frame(rawFrame, frameStart, null, null)
    // 2) zOrder 键 → parse_timeline(…, ZOrder, Step, 0, _parse_z_order_frame) → animation.z_order_timeline
    // 3) bone[] → parse_bone_timeline（6.2）
    // 4) slot[] → parse_slot_timeline（6.2）
    // 5) ffd[] → deform：skin/slot/display 名回查 → parse_timeline(…, SlotDeform, Float, 0, _parse_deform_frame)
    //    （仅龙用到；mesh 缺失时 continue）
    // 6) ik[] → parse_timeline(…, IKConstraint, Int, 2, _parse_ik_constraint_frame) → constraint timelines
    // 7) action_frames 非空 → parse_timeline(None, action_frames, "", Action, Step, 0, _parse_action_frame)
    //    → animation.action_timeline；随后 self.action_frames.clear()（F2.5）
    // 8) ★ generic timeline[]（F2.4，Surface type=50 走这里）→ 6.3 的类型分发
    //    timelineType = get_number(TYPE, Action)（数字！）；timelineName = get_string(NAME, "")
    //    按 type 分发到对应 (FrameValueType, frameValueCount, frameParser)，再按类型族
    //    addBoneTimeline / addSlotTimeline / addConstraintTimeline / addAnimationTimeline
```

**generic 分发表**（`1018-1209`，我已扫出全部 case——按此实现）：

| TimelineType | FrameValueType | count | frameParser | 归属 |
|---|---|---|---|---|
| Action | — | | （TODO 分支，跳过） | — |
| ZOrder | Step | 0 | z_order_frame | animation.zOrderTimeline |
| BoneAll | Float | 6 | bone_all_frame | bone |
| BoneTranslate / BoneRotate / BoneScale | Float | 2 | 对应 parser | bone |
| Surface | Float | 0?（**读 1090-1120 原文确认 count**） | deform_frame（Surface 帧内 vertices 写 floatArray） | bone |
| SlotDeform | Float | 0（同上需对照） | deform_frame | slot |
| SlotColor | Int | 1 | slot_color_frame | slot |
| SlotDisplay / SlotZIndex / BoneAlpha / SlotAlpha | Int（Step?）| … | single_value_frame | bone/slot（**逐行读 1018-1090 确认**） |
| AnimationProgress / Weight / Parameter | Int/Float | … | single/double_value_frame | animation |
| IKConstraint | Int | 2 | ik_frame | constraint |

> 上表标"读原文"的三处，翻译前先精读 `1018-1209`，把每个 case 的 `(valueType, count, parser)` 抄准——这是 golden 中 `timeline[].type/offset` 直接对账的地方。

### 6.2 `_parseBoneTimeline` / `_parseSlotTimeline`（1312-1428）

```
parse_bone_timeline(raw):                       // raw = bone[] 元素
    bone = armature.get_bone(get_string(NAME, ""))；不存在 → return
    self.bone = bone; self.slot = armature.get_slot(bone.name)   // ← 同名槽位（！）
    if translate_frame 键: frame_default=0, scale=1,
        parse_timeline(raw, key="translateFrame", BoneTranslate, Float, 2, double_value_frame)
        → add_bone_timeline(bone.name, tl)
    if rotate_frame 键:   frame_default=0, scale=1, parse(…, BoneRotate, Float, 2, bone_rotate_frame)
    if scale_frame 键:    frame_default=1, scale=1, parse(…, BoneScale, Float, 2, bone_scale_frame)
    if frame 键:          parse(…, BoneAll, Float, 6, bone_all_frame)
    self.bone = self.slot = None

parse_slot_timeline(raw):
    slot = armature.get_slot(...)；不存在 → return
    display_tl = parse(display_frame 键 ? "displayFrame" : "frame", SlotDisplay, Step, 0, slot_display_frame)
    color_tl   = parse(color_frame   键 ? "colorFrame"   : "frame", SlotColor, Int, 1, slot_color_frame)
    非空则 add_slot_timeline
```

注意 `FrameValueType`/`frameValueCount`/`frameDefault`/`frameScale` 是**调用 parse_timeline 前设置的实例状态**（TS 在方法里直接改字段）——Rust 同样用 `self.frame_value_type = ...` 传递，保持一致。

### 6.3 `_parseTimeline`（1210-1310）——**整个阶段 2 最重要的函数，伪代码逐行给出**

```rust
fn parse_timeline(&mut self, raw: Option<&Value>, raw_frames: Option<&Value>,
                  frames_key: &str, tl_type: TimelineType, fvt: FrameValueType,
                  frame_value_count: i32,
                  frame_parser: FrameKind,           // ← 见下方"分发方案"
                  mut timeline: Option<TimelineData>) -> Option<TimelineData>
{
    // 1) 取帧数组：raw 有 frames_key → 覆盖 raw_frames；仍无 → None；空数组 → None
    let frames = pick_frames(raw, raw_frames, frames_key)?;    // 顺序照 1215-1226
    let key_frame_count = frames.len();

    // 2) 记录三个"当前长度"基准
    let frame_int_len = self.frame_int_array.len();
    let frame_float_len = self.frame_float_array.len();
    let timeline_offset = self.timeline_array.len();

    // 3) timeline 对象（TS：传入复用 / 否则 borrow）
    let mut timeline = timeline.unwrap_or_default();
    timeline.timeline_type = tl_type;
    timeline.offset = timeline_offset as i32;
    self.frame_value_type = fvt;
    self.timeline = Some(timeline);                        // 供 frame_parser 内部使用

    // 4) 先占位后回填（TS: length += 1+1+1+1+1+keyFrameCount）
    let base = self.timeline_array.len();
    self.timeline_array.resize(base + 6 + key_frame_count, 0);
    // 5) scale/offset ×100 + round（F2.2 的 1242/1243 两处 js_round！raw 为 None 时写 100/0）
    let (scale, offset) = match raw {
        Some(r) => (js_round(get_number(r, SCALE, 1.0) * 100.0) as i32,
                    js_round(get_number(r, OFFSET, 0.0) * 100.0) as i32),
        None => (100, 0),
    };
    timeline_array[base + TIMELINE_SCALE] = scale;
    timeline_array[base + TIMELINE_OFFSET] = offset;
    timeline_array[base + TIMELINE_KEY_FRAME_COUNT] = key_frame_count as i32;
    timeline_array[base + TIMELINE_FRAME_VALUE_COUNT] = frame_value_count;

    // 6) frame_value_offset：Step→0；Int→frame_int_len − animation.frame_int_offset；Float 同理
    timeline_array[base + TIMELINE_FRAME_VALUE_OFFSET] = match fvt {
        Step => 0, Int => (frame_int_len - anim.frame_int_offset) as i32,
        Float => (frame_float_len - anim.frame_float_offset) as i32,
    };

    // 7) 单帧 vs 多帧（1267-1305，逐行）
    if key_frame_count == 1 {
        timeline.frame_indices_offset = -1;
        let v = dispatch(frame_parser, &frames[0], 0, 0) - anim.frame_offset;
        timeline_array[base + TIMELINE_FRAME_OFFSET + 0] = v as i32;
    } else {
        let total = anim.frame_count + 1;                  // 比动画多一帧
        let fi_off = data.frame_indices.len();
        data.frame_indices.resize(fi_off + total, 0);      // 占位
        timeline.frame_indices_offset = fi_off as i32;
        let (mut frame_start, mut frame_count, mut ik) = (0usize, 0usize, 0usize);
        for i in 0..total {
            if frame_start + frame_count <= i && ik < key_frame_count {
                let raw_frame = &frames[ik];
                frame_start = i;
                frame_count = if ik == key_frame_count - 1 {
                    anim.frame_count as usize - frame_start          // 最后一帧吃到动画末尾
                } else if is_action_frame(raw_frame) {               // ActionFrame 形态（F2.5）
                    self.action_frames[ik + 1].frame_start - frame_start
                } else {
                    get_number(raw_frame, DURATION, 1.0) as usize    // 默认 1
                };
                let v = dispatch(frame_parser, raw_frame, frame_start, frame_count) - anim.frame_offset;
                timeline_array[base + TIMELINE_FRAME_OFFSET + ik] = v as i32;
                ik += 1;
            }
            data.frame_indices[fi_off + i] = (ik as i64 - 1) as i32;  // ← 可为 -1！i32 而非 usize
        }
    }
    self.timeline = None;
    Some(timeline_out)                                       // 从 self.timeline 取回
}
```

**分发方案（关键 Rust 移植决策）**：TS 把 `this._parseXxxFrame` 当方法引用传（内部隐式改 `this`）。Rust 中闭包捕获 `&mut self` 与参数冲突——用 **`enum FrameKind` + `fn dispatch(&mut self, kind, raw, start, count) -> usize`** 分发（或 `fn(&mut Self, ...) -> usize` 函数指针）。推荐 enum：编译期安全、match 可穷举。

### 6.4 帧级 parser（1430-1885）

```
parse_frame(raw, start, _count):            frameArray 只写 [start]，返回 frameOffset（1 个槽位）
parse_tween_frame(raw, start, count):       ← 核心分发（1443-1497，下表逐分支）
    frame_offset = parse_frame(...)
    if count > 0:
        if curve 键:                        sample_count = count+1
            is_omited = sampling_easing_curve(curve, help_array)   // 147-221 贝塞尔采样，逐行翻
            resize +1 +1 +sample_count
            [TWEEN_TYPE] = Curve
            [EASING_OR_COUNT] = is_omited ? sample_count : -sample_count   ← 符号编码！
            for i: [CURVE_SAMPLES+i] = js_round(help_array[i] * 10000.0)    ← F2.2 1456
        else:
            tween_easing = 有 TWEEN_EASING ? get_number(..., -2) : -2       // noTween = -2
            == -2 → None(+1)；== 0 → Line(+1)
            < 0   → QuadIn(+2),  [EASING] = js_round(-easing*100)           ← 1477
            <= 1  → QuadOut(+2), [EASING] = js_round(easing*100)            ← 1482
            else  → QuadInOut(+2),[EASING] = js_round(easing*100 - 100)     ← 1487
    else:                                   count<=0 → None(+1)
    （注意：None/Line 分支只 +1 槽位，Quad/Curve 分支才写 EASING 槽——槽位数随分支变化）

parse_single_value_frame:   [value] = js_round(get_number(VALUE, frame_default) * frame_scale)   ← 1513
parse_double_value_frame:   [x] = js_round(get_number(X, default)*scale)（1544）；[y] 同（1545）
parse_action_frame / parse_z_order_frame:  逐行（zOrder 的 unchanged 压缩算法在 1579-1636，别漏）
parse_bone_all_frame:       ★ rotation 连续性状态机（下）
parse_bone_rotate_frame:    同状态机 + [skew] = js_round? 不——rotate 帧 skew 直接 DEG_RAD 不 round（1710 行）
parse_slot_color_frame:     _parse_color_transform → colorArray 8 × js_round（1753-1760）；
                            defaultColorOffset === -1 时的特殊分支逐行（1741-1789）
parse_slot_display_frame:   [1] = VALUE 键优先 else DISPLAY_INDEX；触发 action_in_frame
parse_deform_frame / ik_frame / action 帧: 逐行（1876-2036）
```

**rotation 连续性状态机**（`parse_bone_all_frame` 1644-1660 与 `parse_bone_rotate_frame` 1687-1704 **共用 self.prev_*, 不重置** — F2.8）：

```rust
fn continue_rotation(&mut self, rotation: f64, frame_start: usize, clockwise: f64) -> f64 {
    if frame_start != 0 {
        if self.prev_clockwise == 0 {
            rotation = self.prev_rotation + normalize_radian(rotation - self.prev_rotation);
        } else {
            // 到达极值时 prev_clockwise 向 0 收 1（方向由符号决定）
            if (self.prev_clockwise > 0 && rotation >= self.prev_rotation)
            || (self.prev_clockwise < 0 && rotation <= self.prev_rotation) {
                self.prev_clockwise += if self.prev_clockwise > 0 { -1 } else { 1 };
            }
            // ★ 逐字照抄，不化简（F2.8：f64 下 a+b-b ≠ b 必然）
            rotation = self.prev_rotation + rotation - self.prev_rotation + PI_D * self.prev_clockwise;
        }
    }
    self.prev_clockwise = clockwise as i32;   // BoneAll←TWEEN_ROTATE(0.0 默认)；Rotate←CLOCK_WISE(0 默认)
    self.prev_rotation = rotation;
    rotation
}
// BoneAll 帧：help_transform 从 TRANSFORM 键解析 → rotation 状态机 → 写6 个 float（x,y,rotation,skew,scaleX,scaleY）
//             最后 _parse_action_data_in_frame(raw, start, self.bone, self.slot)（events→action！）
// Rotate 帧：rotation = get_number(ROTATE,0)*DEG_RAD → 状态机；[1] = get_number(SKEW,0)*DEG_RAD（不 round 不 normalize）
```

### 6.5 `_parse_action_data_in_frame`（223-283）

frame 的 `events` 键 → ActionData 列表 → **push 进 `self.action_frames`**（带 frameStart）；`_merge_action_frame` 处理同帧合并（245-283，逐行）。这是 PlantAnimate frameEvent 的来源（golden `events` 段 60 条可对账）。

---

## 任务 7 — geometry / mesh / weights（2065-2202）

`_parse_geometry`（2065 起）伪代码骨架：

```
geometry_offset = int_array.len(); vertices_offset = float_array.len()
geometry.offset = geometry_offset; geometry.root 回填留到 finalize（勿存 data 引用！D-1.1）
int_array.resize(+4): [VERTEX_COUNT]=floor(vertices.len/2), [FLOAT_OFFSET]=vertices_offset, [WEIGHT_OFFSET]=-1
float_array.resize(+vertexCount*2) ← vertices 原样拷贝（不量化！量化在 finalize）
triangles → int_array（[TRIANGLE_COUNT] 最后回填 = floor(len/3)）
uv → float_array（紧随 vertices 之后）
weights（2106-2202，逐行）：weightCount = Math.floor(rawWeights.length - vertexCount) / 2
    ← 括号语义：floor 只包差值，再 /2 可能出 .5——照抄表达式，Rust 写
      ((raw.len() - vertex_count) as f64).floor() / 2.0 然后 as i32？（实际是整数布局，对照原文）
    int_array 写入 weight 段布局（[WEIGHT_OFFSET] 指向此处）：count/boneCount/bone indices/vertexCounts...
    vertexOffset 负数 +65536 修正（requirements 提过的坑，原文在此段内）
    权重 bones 解析：按 index 取 rawBones（_rawBones 顺序 = 解析顺序！）
```

> `_rawBones`：`_parse_bone` 时 push 的**解析顺序**骨列表（与 `armature.sorted_bones` 不同）——weights 的骨索引引用它。阶段 1 的 `BoneData` 没有这个字段，Parser 自己持有一个 `Vec<BoneData>`（或 clone 的名字列表）即可。

**本任务验收**：Rooster 的 `int=2710, float=4097`、龙的 `int=1926, float=2394`、Surface 的 `int=4, float=8` 与 golden 全等。

---

## 任务 8 — finalize 量化 + IR 收口

**`_modifyArray` 原文（2215-2293，逐行镜像 — F2.6）**：

```rust
fn modify_array(&mut self) {
    // Align：5 个分支，注意第 5 个"条件读 timelineArray、写 colorArray"
    if self.int_array.len() % 2 != 0 { self.int_array.push(0); }
    if self.frame_int_array.len() % 2 != 0 { self.frame_int_array.push(0); }
    if self.frame_array.len() % 2 != 0 { self.frame_array.push(0); }
    if self.timeline_array.len() % 2 != 0 { self.timeline_array.push(0); }
    if self.timeline_array.len() % 2 != 0 { self.color_array.push(0); }   // ← 条件是 timeline！勿"修复"
    // 量化：float 组 → f32；int 组 → i16/u16 wrap（任务 1 的 js_store_*）
    // 布局：binary buffer 按 l1..l7 拼接（P1-1 前可不产出 binary，但**数组量化必须做**）
    // frameIndices 不在 _modifyArray 内（普通 number[]，无量化）
}
```

**收口流程**：

```rust
// parse_dragon_bones_data 尾部：
self.modify_array();                                   // ← 量化在此（数据还是可变状态）
let ir = IrArrays { int_array: take(self.int_array), ..., frame_indices: take(data.frame_indices) };
data.finalize_root(ir);                                // Arc<RootInfo> 回填 parent/geometry
```

**最终测试**：`tests/ir_golden.rs` 全绿（任务 3）。

---

## 验收清单（全绿即阶段 2 完成）

```bash
# 1. 构建 + 全部测试（阶段 0/1 回归 + 新增）
cargo build && cargo test -p dragonbones-core
# 2. clippy 零警告
cargo clippy -p dragonbones-core --tests
# 3. IR 黄金可重新生成（确定性）
node tools/gen_ir.mjs
# 4. 分层断言核对
#    - ir_arrays_match_js_elementwise：9 素材 × 8 组数组精确相等
#    - structure/atlas/meta 快照全等（norm 后）
#    - js_compat 真值表（Null/"NaN"/""/"abc"/缺键五类输入）
#    - 阶段 0 的 10 项 + 阶段 1 的 55 项回归全绿
```

**新增测试合计**：`js_compat`（真值表）+ `ir_golden`（arrays/structure/atlas/meta 4 个）≈ 10+ 个用例。

**常见坑速查**（全部实测）：
1. `js_round = (x+0.5).floor()`，18 处调用点（F2.2 清单），不是 Rust `round()`。
2. `get_string(Null) = "null"`；`get_number(Null) = default`；`get_boolean(Null) = false`——三态真值表别混。
3. `has_key` 必须把"键存在值为 null"算作存在（`in` 语义）。
4. `_modifyArray` 第 5 个 padding 分支怪癖 + i16/u16 是 wrap 不是 clamp + f32 量化。
5. rotation 状态机跨 timeline 不重置 + 1655 行表达式不化简（F2.8）。
6. 三基准（frameInt/Float/FrameOffset）在 `_parseAnimation` 最开头捕获。
7. `timeline_array` 先 `resize(6+n)` 占位再按偏移回填（Rust 用 resize，勿 push 式）。
8. generic `timeline` 路径必须实现（Surface type=50，F2.4）；分发表"读原文"三处先精读。
9. 帧 parser 分发用 `enum FrameKind`（借用冲突，闭包方案不可行）。
10. `frameIndices` 可写 **-1**（i32 不是 usize）。
11. 量化只在 `modify_array`/`finalize_root` 做一次——解析中间过程全部原始 f64/i32。
12. 不用 serde derive 结构体（D5）；`stage` 默认首骨架、`frameRate` 0→24（F2.1/F2.9）。
13. `inherit*` 解析缺键默认 **true**（`_parseBone`），与 `_onClear` 的 false 相反——Default 测试别改。
14. `scaleMix/translateMix` Path 解析缺省 1 ≠ `_onClear` 0（已埋注释）。

**工期提示**：任务 6（animation/timeline/frame）约占一半工作量；先用 Dragon（无 mesh/color/ffd）打通数组对齐，再上 PlantAnimate（color+events+curve）、Rooster（mesh+weights）、龙（ffd）、Surface（generic timeline），逐素材扩绿。

---

## 完成后

跑完验收清单，把 `cargo test` 输出（尤其 `ir_golden` 四项）和 `cargo clippy` 发我。确认全绿后我写**阶段 3（armature 层级）**指南——`TransformObject/Bone/Slot/Armature` + arena 索引设计 + `NullAdapter`，验收是静姿态矩阵差分 ≤ 1e-6。
