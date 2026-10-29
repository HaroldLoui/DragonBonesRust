# dragonbones-bevy

Bevy 0.20 adapter for `dragonbones-core`.

The adapter uses the **B2** design from `docs/design/bevy_skew_transform.md`:

- `dragonbones-core` computes each slot's authoritative `global_transform_matrix`
  (including DragonBones inheritance, constraints and Surface sampling).
- The adapter converts that matrix to `Affine3A` and writes it directly into
  Bevy's `GlobalTransform` in `PostUpdate`, after Bevy's transform propagation
  and before visibility/rendering.
- Skew is therefore preserved exactly; no vertex baking is needed for image or
  bone-space mesh slots.

## Usage

```rust,no_run
use bevy::prelude::*;
use dragonbones_bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DragonBonesPlugin)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn((
        DragonBonesArmature {
            skeleton: assets.load("Dragon_ske.json"),
            atlas: assets.load("Dragon_tex.json"),
            armature_name: "Dragon".to_owned(),
            animation: Some("stand".to_owned()),
        },
        // DragonBones data is y-down; flip once at the armature root.
        Transform::from_scale(Vec3::new(0.75, -0.75, 1.0)),
    ));
}
```

The entity carrying [`DragonBonesArmature`] is the armature root. Its `Transform`
is the armature's world placement; each slot is spawned as a child with a
`Mesh2d` + `MeshMaterial2d<DragonBonesMaterial>` and a `DragonBonesSlot` marker.

## Implemented (MVP)

- `_ske.json` / `_tex.json` asset loaders, with the atlas page image resolved
  relative to the atlas file (`imagePath`).
- Armature construction through `BaseFactory`, attached to the root clock.
- Direct `GlobalTransform` sync with `set_if_neq`.
- Image slots (atlas region + trim frame + pivot).
- Rotated atlas regions (`rotated: true`): the displayed content size swaps the
  stored region's width/height (matching TS `Slot.ts:422-424`) and UVs rotate
  with the region (`u' = 1 - v, v' = u`, matching the Phaser/Cocos adapters); the
  same mapping is used for mesh and skinned-mesh UVs.
- Non-skinned mesh slots, including FFD/deform vertex updates.
- Weighted/skinned mesh slots rebuilt in armature space every frame.
- Surface mesh displays: when the slot's parent bone is a Surface, vertices are
  warped per vertex by the Surface matrix in armature space (matching the
  reference adapters), with FFD deform applied first.
- Child-armature displays: each child armature gets a nested root entity under the
  slot entity that displays it; its world matrix is that slot's world matrix,
  visibility inherits through the parent chain, and it is released recursively
  when the parent root is despawned (`DragonBonesChildArmature` marks the nested
  root). Limitation: core builds child armatures with a null proxy, so their
  events are not forwarded as Bevy messages yet.
- Color multipliers + global alpha baked into per-vertex colors, with one shared
  `DragonBonesMaterial` per `(atlas page, blend mode)` (see below).
- DragonBones blend modes: Normal / Add / Multiply / Screen through a custom
  `Material2d` (fixed-function blend states); Overlay and other modes that need
  render-target reads fall back to Normal.
- Slot draw order via Bevy's transparent-phase z sort.
- Visibility from slot/bone visibility and display presence.
- Despawn → dispose lifecycle: despawning the armature root releases the core
  armature (through the reference implementation's deferred `dispose` buffer),
  the skeleton/atlas registrations and the mesh/material assets.
- O(1) per-slot work in the sync system: a `SlotIdx` → draw-order map is built
  once per armature per frame instead of a linear search per slot.
- Armature-level dirty check: a pose hash + root affine comparison skips the whole
  per-slot pass for idle armatures (see below and
  `cargo bench -p dragonbones-bevy`).
- Schedule-integration tests (`tests/schedule.rs`, with the real `TransformPlugin`)
  guard the direct `GlobalTransform` write against Bevy upgrades: the adapter must
  run after `TransformSystems::Propagate`, and later systems must see its values.

## Lifecycle: despawn → dispose

`DragonBonesPlugin` records an `ArmatureBinding` (root entity, slot entities,
mesh handles, atlas image, factory cache keys) for every built armature. A
`RemovedComponents<DragonBonesInstance>` system in `DragonBonesSystems::Cleanup`
runs before setup/advance and, for a despawned root:

1. removes the slot mesh assets from `Assets<Mesh>`;
2. drops one reference from the shared atlas material cache (removing the
   material with the last armature);
3. decrements the factory's `data_map` / `atlas_map` reference counts and
   removes the entries with the last armature;
4. calls `BaseFactory::dispose_armature`, whose actual `on_clear` runs at the
   start of the `advance_time` later in the same frame.

## Materials, per-slot color and blend modes

Slots only need a per-slot tint and a per-slot blend mode, not a per-slot
texture: every slot of an armature samples the same atlas page. The adapter
therefore creates **one `DragonBonesMaterial` per `(atlas page image, blend
mode)`** (`src/material.rs`) and bakes the DragonBones color multiplier + global
alpha into the mesh `ATTRIBUTE_COLOR` attribute.

`DragonBonesMaterial` is a small custom `Material2d`:

- its only bindings are the atlas page texture and sampler;
- the blend mode is forwarded through `#[bind_group_data(DragonBonesBlend)]`, so
  Bevy compiles one pipeline per blend mode and every slot sharing
  `(image, blend)` shares one material asset, bind group and draw batch;
- `specialize` replaces the color target's blend state:
  Normal `SrcAlpha/OneMinusSrcAlpha`, Add `SrcAlpha/One`, Multiply
  `Dst/OneMinusSrcAlpha`, Screen `One/OneMinusSrc` (all `BlendOperation::Add`,
  alpha component always `OVER`);
- the fragment shader (`src/dragonbones_material.wesl`) samples the atlas and
  multiplies by the vertex color, mirroring `ColorMaterial`'s output/tonemapping
  tail.

Notes:

- Overlay / Darken / Lighten / HardLight / Difference / Subtract / … need reads
  of the render target inside the shader. They currently fall back to Normal
  (the pre-blend-mode behaviour); a shader-based path is a follow-up.
- The blend factors are the straight-alpha form used by Bevy's 2D pipeline.
  Normal and Add are exact; Multiply/Screen match a premultiplied reference
  (Pixi) only where the slot is fully opaque, so soft alpha edges of those slots
  are approximate until a premultiplied shader path lands.
- A runtime blend-mode change (slot timeline) swaps the slot's material handle;
  the armature keeps each `(image, blend)` reference it ever acquired until
  disposal, which keeps the cache refcounts strictly balanced.

## Armature-level dirty check

Every frame the sync system hashes the armature pose (FNV-1a over slot order,
slot matrices, display frames, visibility, colors, interpolated deform vertices
and all bone matrices) and compares it plus the root affine with the values
stored in the `ArmatureBinding`. Unchanged armatures skip the per-slot pass
entirely. This is what makes idle characters cheap:

| static sync, `PostUpdate` only | dirty check off | dirty check on |
|---|---|---|
| 1 armature | 2.156 µs | 1.220 µs |
| 10 armatures | 11.51 µs | 5.196 µs |
| 100 armatures | 118.2 µs | 46.45 µs |

A hash is used instead of a hand-maintained dirty flag so that *every* mutation
source (animation advance, play/seek, replace APIs, moving the root) invalidates
the cache without extra bookkeeping. Disable it when you need the old behaviour:

```rust
app.insert_resource(DragonBonesSyncConfig {
    skip_clean_armatures: false,
});
```

Benchmarks (also measure 1/10/100 armatures while animating):

```text
cargo bench -p dragonbones-bevy
```

## Events

Core dispatches every animation event (`start`, `complete`, `loopComplete`,
`fadeIn*`, `frameEvent`, `soundEvent`, …) through the armature proxy. The adapter
installs `BevyArmatureProxy`, which records them in a queue; a system in
`DragonBonesSystems::Advance` drains the queue into `DragonBonesEvent` messages in
the same frame:

```rust
fn on_events(mut events: MessageReader<DragonBonesEvent>) {
    for event in events.read() {
        if event.event_type() == EventObject::COMPLETE {
            info!("animation finished for armature {:?}", event.armature);
        }
    }
}
```

`DragonBonesEvent` carries the core armature index and the complete core
`EventObject` (time, name, bone/slot names, action data, user data). Note that
core only *creates* an event when the proxy reports a listener for that type, so
`BevyArmatureProxy::has_db_event_listener` reports `true` for every type.

## Animation commands

Game code never touches the non-send runtime: it sends `DragonBonesCommand`
messages to the armature root entity. Commands are applied before the frame's
`advance_time`, so a `Play` starts animating in the same frame:

```rust
fn switch(mut commands: MessageWriter<DragonBonesCommand>, root: Entity) {
    commands.write(DragonBonesCommand::play(root, "walk"));
    commands.write(DragonBonesCommand::pause(root));
    commands.write(DragonBonesCommand::set_speed(root, 0.5));
    commands.write(DragonBonesCommand::play_config(
        root,
        DragonBonesPlayConfig::new("attack")
            .with_play_times(1)
            .with_fade_in_time(0.1)
            .with_position(0.2),
    ));
}
```

Supported actions: `Play` / `PlayConfig` (fade-in, start position, play count,
time scale) / `Stop` / `Pause` / `Resume` / `SetSpeed`. Pause and speed work by
setting the time scale of every active `AnimationState`; the demo's keyboard
animation switching uses this API.

## Not yet implemented

- Animation state machine (layer / group / transition / priority) and seeking the
  playhead of a running animation without restarting it.
- Shader-based blend modes (Overlay / Darken / Lighten / HardLight / …); they
  currently fall back to Normal.
- Replacement textures / replace-skin (换装).
- Batching / armature-level dirty flags for *animated* armatures (the sync pass
  still runs every frame while a pose changes).

## Demo

`crates/dragonbones-demo` loads one rig selected by a command-line argument:

```text
cargo run -p dragonbones-demo -- <name>
cargo run -p dragonbones-demo -- --demo <name>
cargo run -p dragonbones-demo -- --list
```

Available rigs:

```text
dragon (default) | dragon_blend | plant | rooster | long | surface
```

Animation selection is intentionally not a CLI option. The demo reads each
rig's animation names from its skeleton JSON and shows them in a Bevy UI overlay
(top-left). While the demo runs:

- press `1`-`9` to select an animation
- press `Left` / `Right` to cycle through the current rig's animations

For example:

```powershell
cargo run -p dragonbones-demo -- rooster
cargo run -p dragonbones-demo -- --demo plant
cargo run -p dragonbones-demo -- --demo long
```
