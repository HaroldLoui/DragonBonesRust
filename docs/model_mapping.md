# model 字段对照表（TS → Rust）

> 配套 `docs/guide/01-stage1.md` 任务 6（P0-3 / P0-4 验收物）；阶段 3 起扩展收录
> `armature/`、`factory/`、`event/` 运行时层（§3.12+，见 `docs/guide/03-stage3.md`）。
> 对照源：`DragonBonesJS/DragonBones/src/dragonBones/`（数据版本 5.7.000）。
> Rust 落地：`crates/dragonbones-core/src/`。

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

**错误风格适用范围（阶段 4 起的受控变更）**：阶段 4 起，runtime 关系 API（`ClockArena`/`WorldClock`/`RuntimeCtx`/`StateArena`）统一用 `thiserror` + `Result` 表达可恢复的关系错误（`ClockError`/`RuntimeCtxError`/`StateArenaError`）；核心动画数据路径（model/geom/parser 读取、timeline 数值计算）继续沿用阶段 1-3 的 `Option`/`expect`/`log::warn!` 约定。新增错误类型必须与所在层同风格并登记。这是对早期“全库无 Result”约定的**受控变更**，不是全库改造。

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

### 3.12 `TransformObject.ts` → `armature/transform_object.rs`

阶段 3 新增模块挂入 `src/lib.rs`（`armature/*` 7 文件、`event/`、`factory/`、`animation/` 占位）；
`geom/mod.rs` 补顶层 `pub use`（`ColorTransform`/`Matrix`/`Point`/`Rectangle`/`Transform`）供运行时层直引。

**索引与句柄**（定义于 `armature/mod.rs`，放 armature 层避免 `armature → factory` 反向依赖）：

| Rust | 说明 |
|---|---|
| `ArmatureIdx(u32)` / `BoneIdx(u32)` / `SlotIdx(u32)` | newtype 索引（D-3.1），`.index()` → `usize`；`ArmatureIdx` 指 `BaseFactory.armatures` 槽位，`None` 空位 = 已 dispose；`BoneIdx` 创建序 = `sortedBones` 拓扑序（F3.2，父 < 子） |
| `DisplayHandle` | 占位枚举 `Null` / `Raw`（`_rawDisplay`，Slot.ts:248）/ `Mesh`（Slot.ts:249）/ `Armature(ArmatureIdx)`（Slot.ts:510-513 分支）——渲染契约隔离（关键结论 2 / D-3.4），阶段 7 映射实体 |
| `ArenaCtx<'a>` | `{ armatures: &'a mut Vec<Option<Armature>>, self_idx }`——推进期间自身槽位为 `None`（借用契约），`armature()/armature_mut()` 按索引取（D-3.2 解环；父 idx < 子 idx） |

字段（默认值 = `_onClear`，TransformObject.ts:100-111）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `globalTransformMatrix` | `global_transform_matrix: Matrix` | identity |
| `global` | `global: Transform` | identity |
| `offset` | `offset: Transform` | identity |
| `origin` | `origin: Option<Transform>` | `None`（F3.3：init 时 clone 自 data） |
| `userData` | `user_data: Option<Value>` | `None` |
| `_globalDirty` | `global_dirty` | `false`（与 alpha 相反） |
| `_alpha` | `alpha` | `1.0` |
| `_globalAlpha` | `global_alpha` | `1.0` |
| `_armature` | `armature: Option<ArmatureIdx>` | `None`（D-3.1 存索引不存对象） |

方法：`update_global_transform()`（惰性回填 `global ← 矩阵分解`，TransformObject.ts:135-140）。
TS 继承（`Bone/Slot extends`）落成 `base: TransformObject` 组合 + `Deref`/`DerefMut` 透传。

### 3.13 `Bone.ts` → `armature/bone.rs`

跨模块契约字段（**勿改名**）：`base` / `idx` / `bone_name` / `parent` / `bone_type` / `transform_dirty` /
`children_transform_dirty`（`event`/`slot`/`armature`/`factory` 依赖，bone.rs 结构文档）。
默认值逐行对齐 `_onClear`（Bone.ts:92-107 + super）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| —（super） | `base: TransformObject` | 见 3.12 |
| —（Rust 侧） | `idx: BoneIdx` | `0`（init 预置 = push 后下标） |
| `_boneData`（名字缓存） | `bone_name: String` | `""`（兼作 init 幂等判定） |
| `_parent` | `parent: Option<BoneIdx>` | `None` |
| `_boneData.type`（D-3.3 缓存） | `bone_type: BoneType` | `BoneType::Bone` |
| `_transformDirty` | `transform_dirty` | `false` |
| `_childrenTransformDirty` | `children_transform_dirty` | `false` |
| `offsetMode` | `offset_mode: OffsetMode` | `OffsetMode::Additive`（:95） |
| `animationPose` | `animation_pose: Transform` | identity（:96） |
| `_localDirty` | `local_dirty` | **`true`**（:100 ★ 反直觉默认：`update` 尾部 :440 重置为 true、仅 `updateByConstraint` :447 置 false，F3.6） |
| `_hasConstraint` | `has_constraint` | `false`（:101；`Constraint::init` TS:203 只标根骨） |
| `_visible` | `visible` | `true`（:102） |
| `_cachedFrameIndex` | `cached_frame_index` | `-1`（:103，阶段 3 恒 -1） |
| `_cachedFrameIndices` | `cached_frame_indices: Option<Vec<i32>>` | `None`（:106，阶段 6 P1-4） |
| `_boneData.inherit*` 四旗标（D-3.3 缓存） | `inherit_translation/rotation/scale/reflection: bool` | 全 `false` |
| `_boneData.length`（D-3.3 缓存） | `length: f64` | `0.0` |
| `Surface extends Bone`（TS 类继承） | `runtime: BoneRuntime` | `BoneRuntime::Bone`（Surface 载荷见 3.14） |

方法要点：`init`（TS:345-361，不 push 只预置 `idx`，D-4.14）、`update` 四分支脏标记机（TS:365-441，
`left/right/constraints` 三段骨表）、`update_global_transform_matrix`（TS:109-330 offsetMode 三分支 +
inherit 大分支两数学路径）、`update_by_constraint[_resolved]`、`invalid_update`（仅置自身
`transform_dirty`，Bone.ts:481-483，指南 3.3 勘误）、`set_visible`、`contains`、`on_clear`；
其余字段与分支逐行见源码注释。

### 3.14 `Surface.ts` → `armature/surface.rs`

形态：`BoneRuntime::Surface(SurfaceState)`（TS `Surface extends Bone` 类继承的 Rust 形态）。
默认值 = `_onClear`（Surface.ts:51-64）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `surfaceData.segmentX/segmentY`（D-3.3 缓存） | `segment_x` / `segment_y` | `0` |
| `_boneData.geometry`（D-3.3 缓存） | `geometry: GeometryData` | `GeometryData::default()` |
| `_bone`（宿主骨，Surface.ts:469-475 解析） | `bone: Option<BoneIdx>` | `None`（:63） |
| `_dX` / `_dY` | `d_x` / `d_y` | `0.0`（:54/:55） |
| `_k` / `_kX` / `_kY` | `k` / `k_x` / `k_y` | `0.0`（:56-:58） |
| `_vertices` / `_deformVertices` | `Vec<f64>` | `[]` |
| `_hullCache` | `hull_cache: Vec<f64>` | `[]`（init 显式分配 **12**——TS `length = 10`（:463）却写 `[10]/[11]`（:303-304/578-579），JS 靠自动扩容存活） |
| `_matrixCahce`（原文拼写） | `matrix_cahce: Vec<Cell<f64>>` | `[]`（init 按上界扩容——`segmentY > segmentX` 时 TS Left 分支（:194）写超长度式（:462），JS 自动扩容、Rust 显式） |

越界扩容处理：上述两处 JS 数组越界 Rust 显式扩容，元素值与扩容后的 JS 数组逐元素一致；
`geometry.root` 缺失时 warn + 空顶点缓冲（记 §4 D-4.27）。

### 3.15 `Slot.ts` → `armature/slot.rs`

主字段（默认值 = `_onClear`，Slot.ts:261-330 + super；10 个 dirty 标志默认全 `false` 不逐行列）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `_slotData`（名字缓存） | `slot_name: String` | `""`（:319，兼作 init 幂等/未初始化判定） |
| `_parent` | `parent: BoneIdx` | `BoneIdx(0)`（非 Option 契约——数据必有父骨，未初始化槽由 init 重写，:328） |
| `_displayController` | `display_controller: Option<String>` | `None`（:294） |
| `_visible` | `visible` | `true`（:306） |
| `_blendMode` | `blend_mode: BlendMode` | `BlendMode::Normal`（:307） |
| `_displayIndex` / `_animationDisplayIndex` | 同名 | `-1` / `-1`（:308/:309） |
| `_zOrder` / `_zIndex` | 同名 | `0` / `0`（:310/:311） |
| `_cachedFrameIndex` | `cached_frame_index` | `-1`（:312，阶段 3 恒 -1） |
| `_pivotX` / `_pivotY` | 同名 | `0.0` / `0.0`（:313/:314） |
| `_localMatrix` | `local_matrix: Matrix` | identity（:315） |
| `_colorTransform` | `color_transform: ColorTransform` | identity（:316，Rust 方法名 `clear()`，D-4.13） |
| `_displayFrames` | `display_frames: Vec<DisplayFrame>` | `[]`（:317） |
| `_geometryBones` | `geometry_bones: Vec<Option<BoneIdx>>` | `[]`（:318，`None` = TS null） |
| `_displayFrame` | `display_frame: Option<usize>` | `None`（:320，TS 对象引用 → 下标，D-4.16） |
| `_geometryData` / `_boundingBoxData` / `_textureData` | `Option<…>`（**值克隆**，D-3.3） | 全 `None`（:321-:323） |
| `_rawDisplay` / `_meshDisplay` | `DisplayHandle` | `Null` / `Null`（:324/:325） |
| `_display` | `display: Option<DisplayHandle>` | `None`（:326） |
| `_childArmature` | `child_armature: Option<ArmatureIdx>` | `None`（:327，D-3.2 索引解环） |
| `_cachedFrameIndices` | `cached_frame_indices: Option<Vec<i32>>` | `None`（:329，阶段 6） |
| —（Rust 侧 D-3.3 缓存） | `data: Arc<DragonBonesData>` / `armature_name: String` / `texture_atlas_scale: f64` / `adapter: Box<dyn SlotRenderAdapter>` | 默认 Arc / `""` / `1.0` / `NullSlotAdapter` |

`DisplayFrame`（Slot.ts:27-139，默认 = `_onClear` Slot.ts:38-44，`derive(Default)` 等价）：
`raw_display_data` / `display_data` / `texture_data: Option<…>`（值克隆）全 `None`、
`display: Option<DisplayHandle>` = `None`（`Armature(idx)` = 子骨架）、`deform_vertices: Vec<f64>` = `[]`。

`SlotUpdateCtx<'a>`（自身骨架不在 arena 的借用契约，任务 6）：
`parent: &Bone` / `bones: &[Bone]` / `actions: &mut Vec<EventObject>` / `armature: ArmatureIdx` /
`cache_frame_rate: f64`。

`SlotRenderAdapter`：TS `Slot.ts:332-348` 的 14 个 `protected abstract` 方法 → trait（D-3.4，
阶段 7 换 Bevy 实现，**签名冻结**）；`NullSlotAdapter` = jsdump `NullSlot` 全 no-op（偏差见 D-4.21）。
方法清单（`init/dispose/update/add/replace/remove display` + `zOrder/visible/blendMode/color/frame/mesh`
+ `transform/identityTransform`），详见 `slot.rs` trait 定义。

### 3.16 `Armature.ts` → `armature/armature.rs`

字段默认值逐行对齐 `_onClear`（Armature.ts:122-181；**无 `Default`**——`Armature::new` 字面量即该组值）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| —（Rust 侧） | `idx: ArmatureIdx` | alloc 分配（`proxy.db_init(idx)` 用） |
| `_armatureData` | `data: Arc<DragonBonesData>` + `armature_name: String` | D-3.3 拆两字段（`on_clear` 清空 name，:172） |
| `inheritAnimation` | `inherit_animation` | `true`（:155） |
| `userData` | `user_data: Option<Value>` | `None`（:156） |
| `_lockUpdate` | `lock_update` | `false`（:158） |
| `_slotsDirty` | `slots_dirty` | `true`（:159） |
| `_zOrderDirty` / `_zIndexDirty` | 同名 | `false` / `false`（:160/:161） |
| `_alphaDirty` | `alpha_dirty` | `true`（:162） |
| `_flipX` / `_flipY` | `flip_x` / `flip_y` | `false` / `false`（:163/:164） |
| `_cacheFrameIndex` | `cache_frame_index` | `-1`（:165，阶段 3 恒 -1） |
| `_alpha` / `_globalAlpha` | 同名 | `1.0` / `1.0`（:166/:167） |
| `_bones` / `_slots` / `_constraints` / `_actions` | `Vec<…>` | `[]`（:168-:171） |
| `_animation` | `animation: Animation` | 阶段 4 实体化（`animation/player.rs`，字段见 §3.21；`on_clear` 走 `Animation::on_clear`，:173 的 `reset()+init` 已被替换） |
| `_proxy` | `proxy: Box<dyn ArmatureProxy>` | `Armature::new` 注入（`Box` 不可 null：`db_clear()` 清场、box 保活，D-3.1 池语义偏差；:174） |
| `_display` | `display: DisplayHandle` | build 注入 `Armature(idx)`（:175；Slot.ts:512 读子骨架 display） |
| `_parent` | `parent: Option<(ArmatureIdx, SlotIdx)>` | `None`（:180，D-3.2 索引回指） |
| `_clock` / `_dragonBones` | 未落地 | 阶段 5（P0-12）TODO |
| `_replaceTextureAtlasData` / `_replacedTexture` | 未落地 | 阶段 6（P1-5）TODO |

方法：`advance_time`（主循环顺序契约：锁 → animation → 槽排序 → alpha → bones → slots → actions →
解锁 + `dbUpdate`）、`invalid_update`、`on_clear`、`add_bone/slot/constraint`（名字判重 warn+忽略，D-4.14）、
`buffer_action`、`contains_point` / `intersects_segment`（签名偏差 D-4.26）。

### 3.17 `Constraint.ts` → `armature/constraint.rs`

TS 抽象基类 `Constraint`（:27-66）+ `IKConstraint`（:67-228）合体单结构体，`kind` 承担
`BaseFactory._buildConstraints` 的 switch 分派（BaseFactory.ts:229）；`PathConstraint`（:230-834）
**结构在、不执行**（factory 对 Path 数据 warn 跳过，0 素材，真身留 P1-2 阶段 6）。
默认值 = `_onClear`（Constraint.ts:84-91 + 基类 :49-54）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `_constraintData.name` | `name: String` | `""`（兼作 init 幂等判定） |
| `_root`（:193 按名解析） | `root: BoneIdx` | `BoneIdx(0)`（`Bone::update` 按 `root == self.idx` 过滤） |
| `order`（数据登记；运行期不排序） | `order: i32` | `0` |
| `_target`（:192） | `target: BoneIdx` | `BoneIdx(0)`（`sortBones.wait_constraint` 保证 `idx < root`） |
| `_bone`（:194，`None` = 单骨 `_computeA`） | `bone: Option<BoneIdx>` | `None` |
| `_weight` | `weight: f64` | `1.0`（:89；阶段 5 时间轴改写） |
| `_bendPositive` | `bend_positive` | `false`（:88） |
| `type`（BaseFactory.ts:229 switch） | `kind: ConstraintType` | `ConstraintType::IK` |
| `_scaleEnabled`（TS:72/:87/:198 整行注释） | 不设字段 | 忽略（对齐 TS 现状，D-4.19） |

### 3.18 `IArmatureProxy.ts` / `EventObject.ts` → `armature/i_armature_proxy.rs` / `event/event_object.rs`

`ArmatureProxy` trait（IArmatureProxy.ts:36-92 + IEventDispatcher.ts:43-108 两接口合并，D-3.4）
8 方法：`db_init` / `db_clear` / `db_update` / `dispose` / `add_db_event_listener` /
`remove_db_event_listener` / `has_db_event_listener` / `dispatch_db_event`
（`thisObject` 用闭包捕获表达，`DbEventListener = Box<dyn Fn(&EventObject)>`）。
`NullArmatureProxy`（= jsdump `EvProxy`，`derive(Default)`）：`armature: Option<ArmatureIdx>` = `None`、
`listeners: HashMap<String, Vec<DbEventListener>>` = `{}`。

`EventObject` 默认 = `_onClear`（event_object.rs，阶段 3 最小骨架）：

| TS | Rust | 默认值（= `_onClear`） |
|---|---|---|
| `time` | `time: f64` | `0.0` |
| `type` | `event_type: String` | `""`（§1 `type` 规则加 `event_` 前缀） |
| `name` | `name: String` | `""` |
| `armature` | `armature: Option<ArmatureIdx>` | `None` |
| `bone` / `slot` | `Option<BoneIdx>` / `Option<SlotIdx>` | `None`（TS 对象引用 → 本骨架内索引，D-4.20） |
| `actionData` | `action_data: Option<ActionData>` | `None`（TS 引用 → 克隆，D-4.20） |
| `data` | `data: Option<Box<UserData>>` | `None` |
| `animationState` | 未落地 | 阶段 4/5 TODO（类型尚不存在） |

另含事件名 9 常量（EventObject.ts:46-134）与 `action_data_to_instance`（:139-159）。

### 3.19 `BaseFactory.ts` → `factory/base_factory.rs`

| TS | Rust | 默认值 / 说明 |
|---|---|---|
| `autoSearch`（:52） | `auto_search: bool` | `false` |
| `_dragonBonesDataMap`（:54） | `data_map: IndexMap<String, Arc<DragonBonesData>>` | `{}`（保插入序 = V8 `for-in` 字符串键序） |
| `_textureAtlasDataMap`（:55，value 是数组） | `atlas_map: IndexMap<String, Vec<TextureAtlasData>>` | `{}`（同名多 atlas） |
| —（D-3.1 新增，运行时世界） | `armatures: Vec<Option<Armature>>` | `[]`；`None` = 已 dispose 空位 |
| —（D-3.1 新增） | `free: Vec<u32>` | `[]` 空位复用栈（`dispose` push、`alloc_armature_slot` 挑 `> min`，子 idx > 父 idx） |
| `_dataParser`（:57） | `parser: ObjectDataParser` | 每工厂一个实例（TS `static` 惰性单例） |
| `_dragonBones`（:56） | 未落地 | 阶段 5（P0-12）TODO |
| `BuildArmaturePackage`（:1017-1027） | `data_name` / `texture_atlas_name` / `data: Arc<…>` / `armature_name` / `skin: Option<SkinData>` | `skin` 存**克隆**（fill ④ 怪癖允许跨 data），其余为名字 + Arc |

主要入口：`parse_dragon_bones_data` / `parse_texture_atlas_data`、
`build_armature(armature_name, db_name, skin_name, atlas_name, proxy) -> Option<ArmatureIdx>`
（多 proxy 参数 + `Option` 返回，D-4.15；收尾 `invalid_update(null, true)` + `advance_time(0.0)`，F3.11）、
`dispose_armature`（只上锁 + 入 `pending_recycles`；帧首 flush 做 BFS 子骨架递归 + `on_clear` + `free.push`，D-5.6）、`advance_time`（flush → 遍历 arena → 派发，
阶段 5 WorldClock 接管）、`alloc_armature_slot`。

### 3.20 `Animation.ts` → `animation/player.rs`（阶段 3 为 `animation/mod.rs` 占位；阶段 4 实体化见 §3.21）

`Animation { armature: Option<ArmatureIdx> }` = `None`；`advance_time` / `play` / `fade_in` 全 no-op
返回 `None`、`reset` / `stop` 空实现——阶段 4 接 fade 状态机（返回类型 `AnimationState` 阶段 4 才有）。

### 3.21 阶段 4 运行时（`animation/**`、`armature/runtime_ctx.rs`）

阶段 4 新增运行时对象不复刻 TS 的自引用/闭包所有权；`BaseFactory` 仍是唯一 owner（`armatures` + `clocks` + 事件队列）。字段默认值以源码为准，细节见 `animation/world_clock.rs`、`player.rs`、`animation_state.rs`、`blend_state.rs`、`state_id.rs`、`armature/runtime_ctx.rs`：

| 类型 | 关键字段 / 默认值 | 说明 |
|---|---|---|
| `ClockId(u32)` / `AnimatableHandle` | `ClockId` = clock arena 下标；`AnimatableHandle::{Armature(ArmatureIdx), Clock(ClockId)}` | 稳定 handle 替代 TS 对象引用（D-3.1/D-3.2） |
| `WorldClock` | `time`（构造入参）、`time_scale = 1.0`、`system_time =` 构造时墙钟、`parent: Option<ClockId> = None`、`members: Vec<Option<MemberEntry>> = []`、`next_member_token = 1` | 成员表保留插入序 + tombstone；`update_time` 是时间算法 seam |
| `ClockArena` | `slots: Vec<Option<WorldClock>>` | `insert` 只分配 ID；父子关系走 `attach_clock`（拒绝 `ClockError::Cycle`） |
| `RuntimeCtx<'a>` | `armatures: &'a mut [Option<Armature>]`、`clocks: &'a mut ClockArena`、`pending_events: &'a mut Vec<EventObject>`、`current_armature/current_clock: Option<_>` | 推进期借用视图；当前骨架已 take，反查自身返回 `None`；`buffer_event` 只追加 |
| `RuntimeCtxError` / `ClockError` / `StateArenaError` | `thiserror::Error` + `Result` | 见 §1「错误风格适用范围」 |
| `Animation` | `armature: Option<ArmatureIdx> = None`、`states: StateArena<AnimationState>`、`play_order: Vec<StateId> = []`、`last_state: Option<StateId> = None`、`blend_states: HashMap<BlendStateKey, BlendState> = {}`、`pending_child_animations: Vec<String> = []`、`time_scale = 1.0`、`inherit_time_scale = 1.0`、`animation_dirty/cache_frame_invalidated = false` | `play_order` 承载 TS 播放顺序；`pending_child_animations` 是 D-4.36 的延迟入口 |
| `StateId` / `StateArena<T>` | `StateId { slot: u32, generation: u32 }`；arena slots + free list | 回收不移动其它 ID；复用递增 generation，旧 ID 失效 |
| `AnimationState` | `name/animation_name/group = ""`、`layer = 0`、`action_enabled/additive/display_control/reset_to_pose = false`、`blend_type: AnimationBlendType = None`、`play_times = 1`、`current_play_times = 0`、`time_scale = 1.0`、`parameter_x/y = 0.0`、`position = 0.0`、`duration = 0.0`、`weight = 1.0`、`fade_time = 0.0`、`time = 0.0`、`fade_progress = 0.0`、`weight_result = 0.0`、`fade_state = FadeIn`、`sub_fade_state = Start`、`playhead_state = STOPPED`、`timeline_dirty = RebuildAll`、六组 timeline Vec = `[]`、`pose_timeline_ids = []`、`parent/id/armature = None` | 组顺序与 pose fallback 见 `animation_state.rs`；`bone_mask: Vec<String>` |
| `BlendStateKey` / `BlendState` | key = `{ kind: BlendStateKind, target_name: String }`；state = `{ dirty: u32 = 0, layer = 0, left_weight/layer_weight/blend_weight = 0.0 }` | key 只做 lookup，不决定 writer 顺序 |
| `TimelineInstance` | `id`、`base: BaseTimelineState`、`timeline_type`、`target: TimelineTarget`、`blend_key: Option<BlendStateKey>`、IR offset/值缓冲字段 | 具体轴见 `animation/timeline_states/*`；细节以源码为准 |

接线要点：`BaseFactory::advance_time` 执行 objects（本阶段 no-op）→ root clock → events 快照；`take_flushed_events()` 取走即清；Action 的 Play 直接进 armature action 队列。完整签名与默认值以源码为准。

---

## 4. 与 TS 的已知语义偏差

| # | TS 行为 | Rust 处理 | 原因 |
|---|---|---|---|
| D-4.1 | 全局对象池 `BaseObject`（`borrowObject` / `returnToPool` / `_isInPool` / `setMaxCount` / `clearPool` / `hashCode`） | **不移植**：无 TypeId 池；复用由 armature `free` 栈、`StateArena` 槽位、`Vec` 容量承担；生命周期语义由 `pending_recycles` 承担（D-5.1） | 运行时零调用；类型擦除池与所有权方向相反；对动画无数值影响 |
| D-4.2 | `bufferObject` 的 `indexOf` 身份去重 | `PendingRecycle` + `dispose_armature` 的 `lock_update` 幂等；事件派发后直接 drop | 值语义无对象身份；dispose 重复调用由 lock 消除（D-5.5），同值事件必须都保留（D-4.33） |
| D-4.3 | model 对象经 `returnToPool` 复用、`_onClear` 清空 | 拥有树 + `Default` | model 不进池（F1.5）；默认值仍逐行对齐 `_onClear` |
| D-4.4 | `SlotData.color` 清空后为 `null` | `ColorTransform::default()` | 避免 `null` 状态；值与 `createColor()` 一致 |
| D-4.5 | `ArmatureData.parent` 清空后为 `null` | `Option<Arc<RootInfo>>` | 见第 2 节注 |
| D-4.6 | `ArmatureData._onClear` 注释掉 `bones.clear()` 等 | 不复刻该注释怪癖 | 拥有树无池复用，直接清空 |
| D-4.7 | 整数样字符串键 `for..in` 排序规则 | `IndexMap` 纯插入序 | 9 素材无整数样键，登记为已知差异（F1.6） |
| D-4.8 | `AnimationData.cacheFrames` / `ArmatureData.setCacheFrame` 与 bone/slot cachedFrameIndices 写侧 | 保留 cache 结构（单状态 cache interval / cache frame index 计算分支）；`cachedFrames` 与 bone/slot `cachedFrameIndices` 写侧仍未实现 | P1-4；pose 与 37 个动画 golden 不依赖缓存写路径 |
| D-4.9 | `BinaryOffset` 单个 `const enum` 五组共用、判别值大量重复 | 五组 `binary_offset::*` 模块常量（`usize`） | 单 Rust enum 触发 E0081（实测）；下标语义用 const（enum 的类型区分在 `as usize` 使用点丢失，见 `01-stage1.md` 任务 3）；值与 TS 逐项一致，`tests/enums.rs` 守护 |
| D-4.10 | Armature/Bone/Slot/Constraint 走 TypeId 池（`borrowObject` / `returnToPool`） | 不进池：`BaseFactory.armatures: Vec<Option<Armature>>` + `free` 空位栈——`dispose = 上锁 + 入回收队列`，帧首 flush 时 `on_clear + free.push`；build 经 `alloc_armature_slot` 优先复用空位，且 `min_exclusive` 保证**子 idx > 父 idx** | 指南 D-3.1/D-5.6；Bone/Slot/Constraint 随 `Armature.bones/slots/constraints` 生灭（`_onClear` 副作用照抄） |
| D-4.11 | `Armature.ts:302 / :306` 两处 warn-return（`_armatureData === null` / `_parent === null`） | 不复刻，`advance_time` 上锁后直接继续 | Rust 持 `Arc<DragonBonesData>`，数据活到无引用（`on_clear` 清 `armature_name` 使 `armature_data()` 不可达）→ 分支不可达（指南 D-3.3，armature.rs 方法文档） |
| D-4.12 | `TransformObject` 静态 `_helpMatrix/_helpTransform/_helpPoint`（TransformObject.ts:37-39；Surface.ts:186 等同款） | 调用点局部临时值 | 不复刻 `thread_local` 样板（纯实现细节）；值拷贝天然消除“静态被下次调用覆盖”的别名隐患（transform_object.rs 模块文档） |
| D-4.13 | TS `ColorTransform.identity()` | Rust 侧方法名 `clear()`（Point/Rectangle 同名；Matrix/Transform 仍 `identity()`） | 阶段 1 遗留待记项（geom 落地时统一命名，无行为差异） |
| D-4.14 | `Bone.init:358 _addBone(this)` / `Slot.init:665 _addSlot(this)`（init 内 push 自身，按对象身份 `indexOf` 去重） | `init` 不 push、只预置 `idx = len()`，由调用方 `armature.add_bone/add_slot` 推入；去重改成**按名字**判重（warn + 忽略本次推入） | Rust 无法在 `&mut self` 的 `init` 内移动自身入容器；装配循环“建一根推一根”顺序不变（bone.rs / slot.rs 方法文档、`_build_bones`/`_build_slots` 注） |
| D-4.15 | 指南任务 7 草图 `build_armature(armatureName, dragonBonesName, skinName, textureAtlasName)` 返回裸 idx | 多 `proxy: Box<dyn ArmatureProxy>` 参数、返回 `Option<ArmatureIdx>` | D-3.4 调用方注入（jsdump `NullFactory._buildArmature` 在内部 `new EvProxy()` 的等价迁移）；`None` = JS `null`（fill 失败/数据缺失）（base_factory.rs 方法文档） |
| D-4.16 | 对象身份比较：`_displayFrame` 对象引用、`display === _rawDisplay` / `_meshDisplay` 等 `!==` | 值比较/下标：`display_frame: Option<usize>`、`DisplayHandle` 值相等 | 同值 ⟹ 同变换/同几何/同纹理 → 刷与不刷结果一致，已论证无行为差异（slot.rs `DisplayFrame`/`update_display_data` 文档） |
| D-4.17 | `Slot._onClear:279` 递归 `eachDisplay.dispose()`（子骨架）与 `Armature._onClear:123-124` `_clock.remove` | `Slot::on_clear` 只做本地复位 + adapter 分发；子骨架递归由 `BaseFactory` 的帧首 `flush_recycles`/`dispose_armature_now` BFS 实现，clock detach 在 factory 释放 arena slot 前完成 | `on_clear` 拿不到 `RuntimeCtx`（阶段 3 借用契约）；唯一关系 owner 为 `BaseFactory`，详见阶段 4 任务 1 / 阶段 5 任务 2 |
| D-4.18 | `Slot.ts:541 / :549` 子骨架 clock 继承、`:554` `cacheFrameRate` 写回 data（`armatureData.cacheFrames` setter） | child clock detach/inherit 已由 RuntimeCtx 关系入口接线；cacheFrameRate 对 cachedFrames 的写侧仍未实现 | clock 行为经 `world_clock`/`armature_unit` 测试；cachedFrames 留 P1-4 |
| D-4.19 | 指南 F3.1「9 素材全部无约束」实测为错误事实；TS `Constraint.ts:72/:87/:198` `_scaleEnabled` 整行注释（TODO） | **IK 约束阶段 3 提前实现（F3.1 勘误）**：原始 JSON 键是 `constraint` 之外的 **`"ik"`**——`龙`（IK_L/IK_R，weight=0）与 `Rooster_Ani/Blend/P1`（bone_ik/bone_ik1，weight 缺省 1.0）共 4 个 IK 走真身；Path 仍为 stub（0 素材，真身留 P1-2）；同时忽略 `scale_enabled` | 静姿态差分证明必须真身——否则 rotation 回填链（`updateByConstraint` → `isCache=true` → `fromMatrix`）缺失，龙素材 rotation 必错；`scale_enabled` 忽略 = 对齐 TS 现状（constraint.rs 模块文档） |
| D-4.20 | `EventObject.bone/slot` 存对象引用、`actionData` 存引用、含 `animationState` 字段 | `bone/slot` 存本骨架内索引（`Option<BoneIdx>` / `Option<SlotIdx>`）+ 冗余名字；`action_data` 存克隆；不存 `animation_state` | D-3.2 / D-1.1；`animationState` 虽在 TS 写入，但核心与 golden 不读取（F4.27），因此不引入跨 StateArena 的生命周期引用；监听器修改克隆 `UserData` 不会写回 data |
| D-4.21 | jsdump `NullSlot._onUpdateDisplay` 的 `this._rd = this._display \|\| this._rawDisplay` 赋值 | `NullSlotAdapter::update_display` 纯 no-op | 该字段全库无消费者；Rust adapter 不持有 Slot 状态（trait 方法不带状态参，D-3.4 签名冻结）——阶段 7 扩 trait 签名时再议（slot.rs trait/adapter 文档） |
| D-4.22 | `_getSlotDisplay:307` `armatureDisplayData.armature = childArmature.armatureData`（data 层写回，`_buildChildArmature` 递归链上） | 不复刻 | Rust data 层不可变（D-3.3 / F3.3）；该字段 TS 只写不读（grep 零消费者：仅 parser 初始化 + 本行写入）→ 零行为影响（base_factory.rs `_get_slot_display` 注） |
| D-4.23 | `parseDragonBonesData:482` `name = name ?? data.name`（`null` 表“未指定”） | 空串视作“未指定”落 `data.name`（`(!name.is_empty()).then_some(name)`） | Rust 入参 `&str` 无 null/undefined（jsdump 恒传非空 key，base_factory.rs:125 记偏差） |
| D-4.24 | `addTextureAtlasData:573` `indexOf < 0` 按对象身份判重 | `Vec::contains` 值判重 | 值等价：同内容重复 push 无行为差异，判重更严（base_factory.rs:204 记偏差）；同名多 atlas 允许语义不变 |
| D-4.25 | `buildArmature:704` fill 失败 warn 的 `dragonBonesName !== null ? dragonBonesName : ""` 拼串怪癖 | 直接 `warn!("No armature data: {}, {}", armature_name, db_name)` | 纯日志格式，无行为影响（base_factory.rs:952 不复刻） |
| D-4.26 | `Armature.intersectsSegment` 的 3 个可选出参对象（intersectionPointA/B、normalRadians）+ `:548-551`「A/B 出参均未提供 → 首个命中即 break」分支 | 3 个出参对象并成 8 个 f64 入参（循环内局部 `Point` 承接、命中覆写）；`:548-551` 分支不复刻；返回值 = `:573 return intSlotA` 的坐标化 `Option<(f64, f64)>`（无命中 `None`） | 签名既定（任务 6 冻结）：Rust 恒“提供”出参；`normalRadians` 无对应参数，阶段 6 扩签名时补（armature.rs 方法文档） |
| D-4.27 | `Surface.init:451` 读 geometry 的 IR 根（TS 两跳对象引用，null 即崩溃） | `geometry.root` 缺失 → warn + 空顶点缓冲（`_update_vertices` / 曲面采样对空缓冲防御性跳过） | `finalize_root` 未跑属坏数据；TS 该路径读 undefined 得 NaN 污染矩阵，Rust 保持矩阵不动（surface.rs `SurfaceState::init` 记对照表偏差） |
| D-4.28 | 病态配置 `target == chain`（Constraint.ts:173 改写 ikGlobal 后 :176 再读） | 不复刻特殊处理（无等价路径） | 本库 4 个 IK 的 target 均为独立效果骨、`sortBones.wait_constraint` 保证 `idx < root` → 不可达（constraint.rs 模块文档，登记见阶段 3 报告） |
| D-4.29 | 槽排序比较器 `Armature.ts:46-48` `a…>b…?1:-1` 永不返回 0；V8 对等键组实测逆序 | Rust 稳定 `sort_by_key(zIndex*1000+zOrder)`，等键保持原序 | 官方公开路径 `z_order` 恒唯一；等键不可达。ZOrder timeline 的越界索引继续按 TS `continue` 处理，golden 验证完整路径 |
| D-4.30 | `IEventDispatcher.removeDBEventListener(type, listener, thisObject)` 按三元组删单个；jsdump `EvProxy` 的 remove 是 no-op | `EventDispatcher` 基础 trait（`ArmatureProxy: EventDispatcher`）；`add_db_event_listener` 返回 `ListenerId`、`remove_db_event_listener(id)` 按句柄删单个；`ListenerId::next()` 供适配层发号 | `Box<dyn Fn>` 无身份可比；句柄化是 Rust 惯用形态（i_armature_proxy.rs / event/i_event_dispatcher.rs 文档） |
| D-4.31 | `Armature.dispose:265-270` 只 `_lockUpdate=true` + `_dragonBones.bufferObject(this)`，真正 `_onClear` 由 DragonBones **下一帧** flush | **阶段 5 已对齐（D-5.6）**：`BaseFactory::dispose_armature` 只上锁 + 入 `pending_recycles`；下一次 `advance_time` 帧首 `flush_recycles` 才 BFS 清场 + clock detach + `free.push` | `advance_one` 因 `lock_update` 早退；dispose 窗口内仍可查询（base_factory.rs / armature.rs） |
| D-4.32 | TS `AnimationTimelines` 通过对象引用建立 parent/child 状态图，不防御数据环 | Rust 在关联前检查祖先链并拒绝会形成环的 StateId 关系；parent 回收时解除 child parent 并标记 child 可回收 | 属于避免递归/失效 StateId 关系的安全限制；正常 acyclic golden 行为全等，异常循环配置目前无官方素材/验收输入 |
| D-4.33 | `DragonBones.bufferEvent` 按**对象身份** `indexOf` 去重（`DragonBones.ts:334-338`） | 事件按值拥有，`buffer_event` 每次产生新事件直接入 `VecDeque`，**不做内容去重**；两条同值独立事件都必须保留并各派发一次（S5-Event） | 值语义没有对象身份可比；按内容去重会吞掉同值事件（`runtime_ctx.rs` / `base_factory.rs` / `tests/event_dispatch.rs`） |
| D-4.34 | clock 嵌套**自环/环**：官方 `WorldClock.add` 不防环，递归推进即无限递归（`WorldClock.ts:168-254`） | 关系入口 `attach_clock` 拒绝自环/祖先环（`ClockError::Cycle`）；clock 关系查询/推进保留 visited 防护 | 防止栈溢出；异常循环配置无官方素材，正常 acyclic 行为全等（world_clock.rs / base_factory.rs；与 D-4.32 的 AnimationState graph 环分开） |
| D-4.35 | 运行时 `Slot.z_index` 为 `i32`（TS `_zIndex` 为 number；SlotZIndex blend 可产生小数、排序用原值） | 每步 SlotZIndex blend 后以 `(value * blend_weight) as i32` 截断为整数 | 阶段 3 字段形态；素材无 SlotZIndex 数据；阶段 6 接真实 zIndex 时间轴时复核/改 f64（timeline_states/slot.rs） |
| D-4.36 | 子骨架 animation inheritance 的状态创建：官方在 `playConfig` 内**同步**调用 `child.animation.fadeIn`（`Animation.ts:443-452`） | Rust 在 `play_config` 将请求压入 `pending_child_animations`，同一帧 `Armature::advance_time` 尾部通过 `process_child_animation_requests` 排空（`player.rs` / `armature.rs`） | 借用边界（play 路径不持 `RuntimeCtx`）；arena 序父<子保证同帧推进；`play()` 返回瞬间子状态尚未创建是该偏差的唯一可观察面 |
| D-5.1 | 全局 `BaseObject` TypeId 池、`setMaxCount`/`clearPool`/`hashCode` 公共调参 | 删除；对象复用由 arena/free 栈与 `Vec` 容量承担，生命周期语义由 `pending_recycles` 承担 | 运行时零调用；无动画可观察行为（`core/base_object.rs` 已删除，`tests/pool.rs` 已移除） |
| D-5.2 | `AnimationState` fade 完成后从活动表移除，但下一帧才 `bufferObject` 清场（Animation.ts:199,238） | `StateArena` 立即释放 + generation 失效 | 事件不存 `animationState`（D-4.20），parent/child 引用已解除；无观察面 |
| D-5.3 | `EventObject` 派发后下一帧 `returnToPool`；Armature action 队列事件处理完 `returnToPool` | 派发调用栈结束后直接 drop；action 事件随 `advanceTime` 结束 drop | 回调签名 `Fn(&EventObject)` 使引用不可逃逸，克隆是独立所有权；无观察面 |
| D-5.4 | listener 可通过 `event.armature.animation` 同步改状态/销毁骨架 | core 监听器只有 `Fn(&EventObject)`，不能取得 `&mut BaseFactory`；Bevy 层转成事件后由后续 system 执行变更 | 避免 `Rc<RefCell<BaseFactory>>` 与借用冲突；不影响数值 golden（无 golden 用 listener 改状态） |
| D-5.5 | `bufferEvent` / `bufferObject` 的对象身份去重 | 值语义不去重（同值事件保留）；dispose 重复调用由 `lock_update` 幂等 | D-4.33 的延续 |
| D-5.6 | `Armature.dispose` 的“上锁 + 下一帧清场” | `dispose_armature` = 上锁 + `pending_recycles.push(Armature)`；帧首 `flush_recycles` 做 BFS/clock detach/`on_clear`/`free.push` | 撤销 D-4.31 的一帧提前偏差（`tests/lifecycle.rs`） |

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
| `tests/pose_golden.rs` | 阶段 3：9 素材静姿态差分——骨 13 元 / 槽 15 元（前 4 元整数全等）/ `slotNames` 全序（F3.10 排序锚点）/ geometry 双向 / frameRate，容差 `1e-9*(1+\|want\|)`（不放宽 1e-6） → P0-6 / P0-7 / M3 |
| `tests/armature_unit.rs` | 子骨架接线（F3.9 / D-3.2）、槽排序稳定性（R5 / F3.10）、`invalid_update` 传播（指南 3.3 转述勘误）、`displayIndex == -1` 提前返回（坑 10）、子骨架 clock 切换边界、zOrder/zIndex 重排、约束 invalid_update |
| `tests/golden_anim.rs` | 阶段 4：37 个动画 × 120 帧 bones/slots 逐帧差分（容差 `1e-9*(1+\|want\|)`）+ 每帧事件 `(type,name,bone,slot)` 快照 + 9 类 listener 探针不 dispatch |
| `tests/world_clock.rs` | clock 行为矩阵：显式成员、timeScale/零早退/NaN/负 sentinel、成员 add/remove/clear/self-remove、嵌套/contains/环拒绝、dispose、dt=0 事件 flush |
| 模块内 animation 单测（`animation/*.rs`、`timeline_states/*.rs`） | state/blend/player/timeline 数值与状态单测：fade/playhead、blend 预算、play_config、时间归一化/easing、各轴合成 IR、Action/Sound gate |
