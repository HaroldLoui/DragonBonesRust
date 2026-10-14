# model 字段对照表（TS → Rust）

> 配套 `docs/guide/01-stage1.md` 任务 6，是 P0-3 / P0-4 的验收物。
> 对照源：`DragonBonesJS/DragonBones/src/dragonBones/model/*.ts`（数据版本 5.7.000）。
> Rust 落地：`crates/dragonbones-core/src/model/`。

---

## 1. 命名与表示统一规则

| 规则 | 说明 | 例 |
|---|---|---|
| 字段/方法名 | TS camelCase → Rust snake_case | `frameRate` → `frame_rate`，`fadeOutTweenType` → `fade_out_tween_type` |
| `_` 前缀 protected | 去掉前缀，按需 `pub` / `pub(crate)` | `_onClear` → `Default` / `on_clear` |
| `type` 单值枚举字段 | 字段名与类型名冲突时 → `xxx_type` | `ActionData.type` → `action_type`，`ArmatureData.type` → `armature_type` |
| `type` 变体判别字段 | 字段名与类型名冲突时 → `kind` + `type_()` 方法 | `DisplayData.type` → `DisplayData.kind: DisplayKind` + `DisplayData::type_()` |
| 抽象类 + 子类 | → 公共字段 struct + `kind` 变体枚举（避免非法状态） | `BoneData`/`SurfaceData` → `BoneData { kind: BoneKind }` |
| `Map<T>` | 插入有序字典 → `indexmap::IndexMap` | `bones` / `slots` / `skins` / `animations` |
| `Array<number>`（整数） | → `Vec<i32>` | `sortedBones` 之外的整数列表 |
| `Array<number>`（浮点） | → `Vec<f64>`（D1：core 全程 f64） | `curveLengths` / `cachedFrames` |
| `@internal` / `@private` | 不影响可见性策略：core 内统一 `pub`（同 crate 使用） | — |

---

## 2. D-1.1 反向/横向引用替换表

TS 对象引用字段在 Rust 里按“根共享 / 名字 / 直接拥有”三类替换；**不使用 `Rc<RefCell>`（D3）**。

| TS 字段（类型） | Rust 表示 | 依据 / 备注 |
|---|---|---|
| `ArmatureData.parent: DragonBonesData` | `Option<Arc<RootInfo>>` | `RootInfo` 打包 `name` + 8 组 IR 数组，只读共享 |
| `GeometryData.data: DragonBonesData` | `Option<Arc<RootInfo>>` | 只读 `intArray` |
| `AnimationData.parent: ArmatureData` | `String`（骨架名） | 运行时经名字解析 |
| `SkinData.parent: ArmatureData` | `String`（骨架名） | 只读 `.defaultSkin` 等 |
| `DisplayData.parent: SkinData` | `String`（皮肤名） | 链式取名 |
| `TextureData.parent: TextureAtlasData` | `String`（图集名） | 只读 `.scale` |
| `BoneData.parent: BoneData \| null` | `Option<String>`（父骨名） | 骨名在骨架内唯一 |
| `SlotData.parent: BoneData` | `String`（父骨名） | 只取 `.name` |
| `ConstraintData.target/root: BoneData` | `String`；`bone` → `Option<String>` | 身份比较即名字相等 |
| `PathConstraintData.pathSlot: SlotData` | `String` | — |
| `PathConstraintData.pathDisplayData: PathDisplayData` | `Option<String>` | 名字引用 |
| `PathConstraintData.bones: BoneData[]` | `Vec<String>` | — |
| `WeightData.bones: BoneData[]` | `Vec<String>` | — |
| `ArmatureDisplayData.armature: ArmatureData \| null` | `Option<String>` | 名字引用 |
| `ImageDisplayData.texture` / `MeshDisplayData.texture` | `Option<String>`（纹理名） | 纹理本体由图集拥有 |
| `ArmatureData.stage/defaultSkin/defaultAnimation` | `Option<String>` | 名字引用 |
| `ActionData.bone/slot` | `Option<String>` | 名字引用 |
| `ActionData.data` | `Option<Box<UserData>>` | 直接拥有 |
| `SkinData.displays: Map<Array<DisplayData\|null>>` | `IndexMap<String, Vec<Option<DisplayData>>>` | 直接拥有（`null` 保留） |
| `GeometryData.weight: WeightData \| null` | `Option<Box<WeightData>>` | 直接拥有 |
| `BoundingBoxDisplayData.boundingBox` | `Option<Box<BoundingBoxData>>` | 直接拥有 |
| `AnimationData.actionTimeline/zOrderTimeline` | `Option<Box<TimelineData>>` | 直接拥有 |
| 其余向下的时间轴/动作列表 | `Vec<...>` / `IndexMap<String, Vec<TimelineData>>` | 直接拥有 |

> 唯一与指南的偏差：`ArmatureData.parent` 用 `Option<Arc<RootInfo>>`（TS 里 `_onClear` 置
> `null`），用 `Option` 更诚实；解析收尾由 `DragonBonesData::finalize_root` 统一回填。

---

## 3. 逐模块字段表

### 3.1 `AnimationConfig.ts` → `animation_config.rs`

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `pauseFadeOut` | `pause_fade_out` | `true` |
| `fadeOutMode` | `fade_out_mode` | `AnimationFadeOutMode::All` |
| `fadeOutTweenType` | `fade_out_tween_type` | `TweenType::Line` |
| `fadeOutTime` | `fade_out_time` | `-1.0` |
| `actionEnabled` | `action_enabled` | `true` |
| `additive` | `additive` | `false` |
| `displayControl` | `display_control` | `true` |
| `pauseFadeIn` | `pause_fade_in` | `true` |
| `resetToPose` | `reset_to_pose` | `true` |
| `fadeInTweenType` | `fade_in_tween_type` | `TweenType::Line` |
| `playTimes` | `play_times` | `-1` |
| `layer` | `layer` | `0` |
| `position` | `position` | `0.0` |
| `duration` | `duration` | `-1.0` |
| `timeScale` | `time_scale` | **`-100.0`** |
| `weight` | `weight` | `1.0` |
| `fadeInTime` | `fade_in_time` | `-1.0` |
| `autoFadeOutTime` | `auto_fade_out_time` | `-1.0` |
| `name` / `animation` / `group` | `name` / `animation` / `group` | `""` |
| `boneMask` | `bone_mask: Vec<String>` | `[]` |

方法：`clear()`（= `_onClear`）、`copy_from()`。

### 3.2 `AnimationData.ts` → `animation_data.rs`

`TimelineData`（含 `AnimationTimelineData`，字段平铺）：

| TS | Rust | 默认值 |
|---|---|---|
| `type` | `timeline_type` | `TimelineType::BoneAll` |
| `offset` | `offset` | `0` |
| `frameIndicesOffset` | `frame_indices_offset` | **`-1`** |
| `x` / `y`（仅 AnimationTimeline） | `x` / `y` | `0.0` |

`AnimationData`：

| TS | Rust | 默认值 |
|---|---|---|
| `frameIntOffset` / `frameFloatOffset` / `frameOffset` | 同名 snake | `0` |
| `blendType` | `blend_type` | `AnimationBlendType::None` |
| `frameCount` / `playTimes` | `frame_count` / `play_times` | `0` |
| `duration` / `fadeInTime` / `cacheFrameRate` | snake | `0.0` |
| `scale` | `scale` | `1.0` |
| `name` | `name` | `""` |
| `cachedFrames` | `cached_frames: Vec<bool>` | `[]` |
| `boneTimelines` / `slotTimelines` / `constraintTimelines` / `animationTimelines` | `IndexMap<String, Vec<TimelineData>>` | `{}` |
| `boneCachedFrameIndices` / `slotCachedFrameIndices` | `IndexMap<String, Vec<i32>>` | `{}` |
| `actionTimeline` / `zOrderTimeline` | `Option<Box<TimelineData>>` | `None` |
| `parent` | `parent: String` | `""` |

方法：`add_{bone,slot,constraint,animation}_timeline`、`get_*`、`get_*_cached_frame_indices`。
`cacheFrames`（P1-4）未实现，留待阶段 6。

### 3.3 `ArmatureData.ts` → `armature_data.rs`

`BoneData`（+ `SurfaceData` 变体）：

| TS | Rust | 默认值 |
|---|---|---|
| `inheritTranslation` / `inheritRotation` / `inheritScale` / `inheritReflection` | snake | 全 `false` |
| `type` | `kind: BoneKind` + `type_()` | `BoneKind::Bone` |
| `length` | `length` | `0.0` |
| `alpha` | `alpha` | `1.0` |
| `name` | `name` | `""` |
| `transform` | `transform: Transform` | identity |
| `userData` | `user_data: Option<UserData>` | `None` |
| `parent` | `parent: Option<String>` | `None` |
| `SurfaceData.segmentX/segmentY` | `BoneKind::Surface { segment_x, segment_y }` | `0` |
| `SurfaceData.geometry` | `BoneKind::Surface { geometry }` | `GeometryData::default()` |

`SlotData`：

| TS | Rust | 默认值 |
|---|---|---|
| `blendMode` | `blend_mode` | `BlendMode::Normal` |
| `displayIndex` / `zOrder` / `zIndex` | snake | `0` |
| `alpha` | `alpha` | `1.0` |
| `name` | `name` | `""` |
| `color` | `color: ColorTransform` | 乘子 1 / 偏移 0 |
| `userData` | `user_data` | `None` |
| `parent` | `parent: String` | `""` |

常量：`SlotData::DEFAULT_COLOR`、`create_color()`。

`ArmatureData`：

| TS | Rust | 默认值 |
|---|---|---|
| `type` | `armature_type` | `ArmatureType::Armature` |
| `frameRate` / `cacheFrameRate` | snake (`f64`) | `0.0` |
| `scale` | `scale` | `1.0` |
| `name` | `name` | `""` |
| `aabb` | `aabb: Rectangle` | default |
| `animationNames` | `animation_names: Vec<String>` | `[]` |
| `sortedBones` / `sortedSlots` | `Vec<String>`（名字） | `[]` |
| `defaultActions` / `actions` | `Vec<ActionData>` | `[]` |
| `bones` / `slots` / `constraints` / `skins` / `animations` | `IndexMap<String, _>` | `{}` |
| `defaultSkin` / `defaultAnimation` | `Option<String>` | `None` |
| `canvas` / `userData` | `Option<CanvasData>` / `Option<UserData>` | `None` |
| `parent` | `Option<Arc<RootInfo>>` | `None` |

方法：`sort_bones`、`add_bone/slot/constraint/skin/animation/action`、`get_bone/slot/constraint/skin/mesh/animation`、`set_root_info`。
`cacheFrames` / `setCacheFrame` / `getCacheFrame`（P1-4）未实现。

### 3.4 `BoundingBoxData.ts` → `bounding_box.rs`

| TS | Rust | 默认值 |
|---|---|---|
| `type` | `kind: BoundingBoxKind` + `type_()` | `BoundingBoxKind::Rectangle` |
| `color` / `width` / `height` | 同名 | `0x000000` / `0.0` / `0.0` |
| `PolygonBoundingBoxData.x/y/vertices` | `BoundingBoxKind::Polygon { x, y, vertices }` | `0.0` / `[]` |

`containsPoint` / `intersectsSegment`（P1-3）未实现。

### 3.5 `CanvasData.ts` → `canvas_data.rs`

| TS | Rust | 默认值 |
|---|---|---|
| `hasBackground` | `has_background` | `false` |
| `color` | `color: i32` | `0x000000` |
| `x` / `y` / `width` / `height` | 同名 (`f64`) | `0.0` |

### 3.6 `ConstraintData.ts` → `constraint_data.rs`

| TS | Rust | 默认值 |
|---|---|---|
| `order` | `order: i32` | `0` |
| `name` | `name` | `""` |
| `type` | `kind: ConstraintKind` + `type_()` | `ConstraintKind::IK` |
| `target` / `root` | `String` | `""` |
| `bone` | `Option<String>` | `None` |
| `IKConstraintData.scaleEnabled/bendPositive/weight` | `ConstraintKind::IK { .. }` | `false` / `false` / `1.0` |
| `PathConstraintData.pathSlot` | `ConstraintKind::Path { path_slot }` | `""` |
| `pathDisplayData` | `path_display: Option<String>` | `None` |
| `bones` | `Vec<String>` | `[]` |
| `positionMode` / `spacingMode` / `rotateMode` | 同名 | `Fixed` / `Fixed` / `Chain` |
| `position` / `spacing` / `rotateOffset` / `rotateMix` / `translateMix` | snake | `0.0` |

### 3.7 `DisplayData.ts` → `display_data.rs`

`GeometryData`：

| TS | Rust | 默认值 |
|---|---|---|
| `isShared` / `inheritDeform` | snake | `false` |
| `offset` | `offset: i32` | `0` |
| `data` | `root: Option<Arc<RootInfo>>` | `None` |
| `weight` | `weight: Option<Box<WeightData>>` | `None` |

方法：`clear()`、`share_from()`、`vertex_count()`、`triangle_count()`。

`DisplayData`（+ 5 子类变体）：

| TS | Rust | 默认值 |
|---|---|---|
| `type` | `kind: DisplayKind` + `type_()` | `DisplayKind::Image` |
| `name` / `path` | 同名 | `""` |
| `transform` | `transform: Transform` | identity |
| `parent` | `parent: String` | `""` |
| `ImageDisplayData.pivot/texture` | `Image { pivot, texture }` | `Point::default()` / `None` |
| `ArmatureDisplayData.inheritAnimation/actions/armature` | `Armature { .. }` | `false` / `[]` / `None` |
| `MeshDisplayData.geometry/texture` | `Mesh { .. }` | default / `None` |
| `BoundingBoxDisplayData.boundingBox` | `BoundingBox { .. }` | `None` |
| `PathDisplayData.closed/constantSpeed/geometry/curveLengths` | `Path { .. }` | `false` / `false` / default / `[]` |

`WeightData`：

| TS | Rust | 默认值 |
|---|---|---|
| `count` / `offset` | 同名 (`i32`) | `0` |
| `bones` | `Vec<String>` | `[]` |

方法：`add_bone`。

### 3.8 `DragonBonesData.ts` → `dragon_bones_data.rs`

`RootInfo` / `IrArrays`（新引入的共享根，对应 TS `name` + 8 组 TypedArray）：

| TS | Rust |
|---|---|
| `name` | `RootInfo.name: String` |
| `intArray` | `IrArrays.int_array: Vec<i32>` |
| `floatArray` | `IrArrays.float_array: Vec<f64>` |
| `frameIntArray` | `IrArrays.frame_int_array: Vec<i32>` |
| `frameFloatArray` | `IrArrays.frame_float_array: Vec<f64>` |
| `frameArray` | `IrArrays.frame_array: Vec<i32>` |
| `timelineArray` | `IrArrays.timeline_array: Vec<i32>` |
| `colorArray` | `IrArrays.color_array: Vec<i32>` |
| `frameIndices` | `IrArrays.frame_indices: Vec<i32>` |

`DragonBonesData`：

| TS | Rust | 默认值 |
|---|---|---|
| `name` | `name` | `""` |
| — | `root: Option<Arc<RootInfo>>` | `None` |
| `autoSearch` | `auto_search` | `false` |
| `frameRate` | `frame_rate: f64` | `0.0` |
| `version` | `version` | `""` |
| `stage` | `stage: Option<String>` | `None` |
| `userData` | `user_data: Option<UserData>` | `None` |
| `armatureNames` | `armature_names: Vec<String>` | `[]` |
| `armatures` | `IndexMap<String, ArmatureData>` | `{}` |
| `cachedFrames` | `cached_frames: Vec<f64>` | `[]` |
| `binary` | `binary: Option<Vec<u8>>` | `None` |

方法：`add_armature`、`get_armature`、`finalize_root`。

### 3.9 `SkinData.ts` → `skin_data.rs`

| TS | Rust | 默认值 |
|---|---|---|
| `name` | `name` | `""` |
| `displays` | `displays: IndexMap<String, Vec<Option<DisplayData>>>` | `{}` |
| `parent` | `parent: String` | `""` |

方法：`add_display`、`get_display`、`get_displays`。

### 3.10 `TextureAtlasData.ts` → `texture_atlas_data.rs`

`TextureAtlasData`：

| TS | Rust | 默认值 |
|---|---|---|
| `autoSearch` | `auto_search` | `false` |
| `width` / `height` | 同名 | `0.0` |
| `scale` | `scale` | `1.0` |
| `name` / `imagePath` | `name` / `image_path` | `""` |
| `textures` | `IndexMap<String, TextureData>` | `{}` |

方法：`add_texture`、`get_texture`、`copy_from`。

`TextureData`：

| TS | Rust | 默认值 |
|---|---|---|
| `rotated` | `rotated` | `false` |
| `name` | `name` | `""` |
| `region` | `region: Rectangle` | default |
| `parent` | `parent: String` | `""` |
| `frame` | `frame: Option<Rectangle>` | `None` |

方法：`copy_from`。

### 3.11 `UserData.ts` → `user_data.rs`

`UserData`：

| TS | Rust | 默认值 |
|---|---|---|
| `ints` | `ints: Vec<i32>` | `[]` |
| `floats` | `floats: Vec<f64>` | `[]` |
| `strings` | `strings: Vec<String>` | `[]` |

方法：`add_int/add_float/add_string`、`get_int/get_float/get_string`。

`ActionData`：

| TS | Rust | 默认值 |
|---|---|---|
| `type` | `action_type` | `ActionType::Play` |
| `name` | `name` | `""` |
| `bone` / `slot` | `Option<String>` | `None` |
| `data` | `Option<Box<UserData>>` | `None` |

---

## 4. 与 TS 的已知语义偏差

| # | TS 行为 | Rust 处理 | 原因 |
|---|---|---|---|
| D-4.1 | 对象池 `_isInPool` 重复入池检测 | 无该字段 | move 语义下同一对象不可能被两次归还（编译期消除） |
| D-4.2 | `bufferObject` 的 `indexOf` 去重 | `ObjectBuffer::push` 直接入队 | 同上，push 消费所有权 |
| D-4.3 | model 对象经 `returnToPool` 复用、`_onClear` 清空 | 拥有树 + `Default` | model 不进池（F1.5）；默认值仍逐行对齐 `_onClear` |
| D-4.4 | `SlotData.color` 清空后为 `null` | `ColorTransform::default()` | 避免 `null` 状态；值与 `createColor()` 一致 |
| D-4.5 | `ArmatureData.parent` 清空后为 `null` | `Option<Arc<RootInfo>>` | 见第 2 节注 |
| D-4.6 | `ArmatureData._onClear` 注释掉 `bones.clear()` 等 | 不复刻该注释怪癖 | 拥有树无池复用，直接清空 |
| D-4.7 | 整数样字符串键 `for..in` 排序规则 | `IndexMap` 纯插入序 | 9 素材无整数样键，登记为已知差异（F1.6） |
| D-4.8 | `AnimationData.cacheFrames` / `ArmatureData.setCacheFrame` | 未实现 | P1-4，阶段 6 |
| D-4.9 | `BinaryOffset` 单个 `const enum` 五组共用、判别值大量重复 | 五组 `binary_offset::*` 模块常量（`usize`） | 单 Rust enum 触发 E0081（实测）；下标语义用 const（enum 的类型区分在 `as usize` 使用点丢失，见 `01-stage1.md` 任务 3）；值与 TS 逐项一致，`tests/enums.rs` 守护 |

### 4.1 阶段 2 必读：IR 量化事实（实测，修正 requirements D1）

requirements D1 说"JSON 路径 `floatArray` 是 f64、dbbin 路径是 f32"——**只对 parser 内部中间态成立**。
实测（`ObjectDataParser._modifyArray()`，`ObjectDataParser.ts:2229+`，JSON 路径 parse 收尾也会执行）：
用参考运行时 parse `Dragon_ske.json` 后——

| 字段 | JS 实际类型 | 说明 |
|---|---|---|
| `data.floatArray` / `data.frameFloatArray` | **Float32Array** | `number[]` 被重解释为 TypedArray，f64 值量化为 f32（实测 frameFloatArray 536 个值全部 f32 精确） |
| `data.intArray` / `data.frameIntArray` / `data.colorArray` / `data.frameArray` | Int16Array | 赋值按 i16 wrap |
| `data.timelineArray` | Uint16Array | 赋值按 u16 wrap |
| `data.binary` | ArrayBuffer（**非 null**） | JSON 路径也构建 binary buffer |

**结论：两路径的运行时值精度相同（float=f32、int=i16/u16）**，D1 的"JSON 路径 f64"仅指
`_modifyArray` 之前的 parser `number[]` 中间态。

**阶段 2 落点**：在 `DragonBonesData::finalize_root(ir)` 收尾时逐行镜像 `_modifyArray`：

1. `float_array` / `frame_float_array`：`v as f32 as f64`（f32 量化，Rust `as f32` 与
   JS `Float32Array` 存储同为 round-to-nearest）
2. `int_array` / `frame_int_array` / `color_array`：`v as i16 as i32`；`frame_array` /
   `timeline_array`：`v as u16 as i32`（wrap 语义）
3. **奇数长度补 0 的对齐 padding**（镜像 `_modifyArray` 的 5 个 align 分支，含"条件读
   `timelineArray.length`、写 `colorArray`"的怪癖）——padding 发生在全部解析之后，
   不影响已存偏移，但**影响 IR 数组长度**，逐元素断言必须一致
4. `frame_indices` 不经 `_modifyArray`（普通 `number[]`），无量化、无 padding
5. `binary` 暂不复刻（`Option::None`），P1-10 再定

**不做的后果**：阶段 2 IR 逐元素黄金断言直接失败；阶段 4 运行时读 f32 IR，若用 f64 原值
会引入 1e-7 级偏差、可能累积超出 1e-6 差分容差。

---

## 5. 测试锚点

| 测试 | 覆盖 |
|---|---|
| `tests/model_defaults.rs` | 全部 model `Default` == TS `_onClear`（含 `kind` 变体：Surface / Path 约束 / 5 种 Display / SkinData） |
| `tests/sort_bones.rs` | `sort_bones` 拓扑序（父先于子、约束 target 先于 root） |
| `tests/enums.rs` | 16 枚举 + `BinaryOffset` 五组常量数值 |
| `tests/geom_golden.rs` | geom 704 用例对齐 JS |
| `tests/pool.rs` | 池复用 / `hash_code` 保留 / 缓冲延迟回收 / 上限 |
| `tests/ir_golden.rs` | 阶段 2：9 素材 × 8 组 IR 数组 bit 级全等 + meta/structure/atlas 快照（`norm()` 后） |
| `tests/js_compat.rs` | JS 语义真值表：`js_round` 半数向 +∞ / 三 helper 三态 / TypedArray wrap |
