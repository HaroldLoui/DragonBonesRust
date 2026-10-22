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
`Mesh2d` + `MeshMaterial2d<ColorMaterial>` and a `DragonBonesSlot` marker.

## Implemented (MVP)

- `_ske.json` / `_tex.json` asset loaders, with the atlas page image resolved
  relative to the atlas file (`imagePath`).
- Armature construction through `BaseFactory`, attached to the root clock.
- Direct `GlobalTransform` sync with `set_if_neq`.
- Image slots (atlas region + trim frame + pivot).
- Non-skinned mesh slots, including FFD/deform vertex updates.
- Weighted/skinned mesh slots rebuilt in armature space every frame.
- Color multipliers + global alpha via `ColorMaterial` (offsets ignored).
- Slot draw order via Bevy's transparent-phase z sort.
- Visibility from slot/bone visibility and display presence.

## Not yet implemented

- Add/Multiply/Screen/Overlay blend modes (currently normal alpha blending).
- Rotated atlas regions (`rotated: true`); no current asset uses them.
- Surface mesh vertex deformation for mesh displays (Surface image slots work
  through the core's surface matrix).
- Child-armature slot displays and replacement textures.
- Event-to-Bevy event forwarding.
- Dirty-slot optimization; the adapter syncs all slots every frame because a
  moving root makes Bevy rewrite child `GlobalTransform`s.

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
