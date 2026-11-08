use std::path::{Path, PathBuf};

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, FlexDirection, JustifyContent, Node, Overflow, PositionType,
    Pressed, UiRect, Val,
};
use bevy::ui_widgets::{Activate, Button};
use bevy::window::WindowResolution;
use dragonbones_bevy::prelude::*;

/// Usage:
///   cargo run -p dragonbones-demo                 # show the first rig in load order
///   cargo run -p dragonbones-demo -- <rig>        # show <rig> first
///   cargo run -p dragonbones-demo -- --list       # list the rigs found under assets/
///
/// Every rig under `assets/<rig>/<rig>_ske.json` is loaded; the right-hand panel
/// switches between them (click a name, or press Up/Down). Inside the window,
/// 1-9 selects an animation and Left/Right cycles the current rig's animations.
fn main() {
    let rigs = match parse_cli() {
        Cli::Help => {
            print_usage();
            return;
        }
        Cli::List => {
            let rigs = discover_rigs().unwrap_or_else(|error| usage_error(&error));
            print_rig_list(&rigs);
            return;
        }
        Cli::Run(name) => {
            let rigs = discover_rigs().unwrap_or_else(|error| usage_error(&error));
            let index = match name.as_deref() {
                None => 0,
                Some(name) => resolve_rig(&rigs, name).unwrap_or_else(|| {
                    eprintln!("error: unknown rig `{name}`\n");
                    print_rig_list(&rigs);
                    std::process::exit(2);
                }),
            };
            (rigs, index)
        }
    };
    let (rigs, index) = rigs;
    if rigs.is_empty() {
        usage_error("no rigs found under assets/");
    }
    let initial = &rigs[index];
    let current = initial
        .animations
        .iter()
        .position(|animation| *animation == initial.default_animation)
        .unwrap_or(0);
    let title = format!("DragonBonesRust - {}", initial.display);
    println!(
        "discovered {} rigs, showing `{}` ({}/{})",
        rigs.len(),
        initial.dir,
        index + 1,
        rigs.len()
    );

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title,
                        resolution: WindowResolution::new(1280, 720)
                            .with_scale_factor_override(1.0),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: assets_root().to_string_lossy().into_owned(),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
        .insert_resource(DemoCatalog { rigs })
        .insert_resource(SelectedDemo {
            index,
            current,
            pending: None,
        })
        .add_plugins(DragonBonesPlugin)
        .add_observer(on_rig_activated)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                handle_rig_keys,
                apply_selection,
                switch_animation,
                update_animation_hud,
                update_rig_buttons,
            )
                .chain(),
        )
        .add_systems(Update, log_dragonbones_events)
        .run();
}

/// Prints the animation events the adapter forwards into Bevy messages.
///
/// The demo logs every event at `info` so the bridge is visible while running;
/// a real game would usually filter for `complete` / `frameEvent` / `soundEvent`.
fn log_dragonbones_events(mut events: MessageReader<DragonBonesEvent>) {
    for event in events.read() {
        info!(
            "DragonBones event: {} (armature {:?}, name {:?}, bone {:?}, slot {:?})",
            event.event_type(),
            event.armature,
            event.event.name,
            event.event.bone_name,
            event.event.slot_name,
        );
    }
}

// ---------------------------------------------------------------------------
// Rigs discovered from assets/
// ---------------------------------------------------------------------------

/// One rig found under `assets/`: the paths are relative to the asset root.
#[derive(Clone)]
struct RigEntry {
    /// Directory name under `assets/`, also the load-order key.
    dir: String,
    /// Human-readable name shown in the window and the rig panel.
    display: String,
    /// `<dir>/<file>_ske.json`
    skeleton: String,
    /// `<dir>/<file>_tex.json`
    atlas: String,
    armature: String,
    default_animation: String,
    animations: Vec<String>,
    /// Uniform scale applied to the armature root.
    scale: f32,
    /// Armature-space AABB center from the skeleton data.
    center: Vec2,
    /// Extra CLI aliases (e.g. `plant` for `PlantAnimate`).
    aliases: Vec<&'static str>,
}

/// Hand-tuned presentation defaults for the rigs that shipped with the demo.
/// Rigs that are only discovered from disk fall back to the same heuristics
/// `tools/import_rig.py` uses (largest armature, idle-ish animation, 320px fit).
struct CuratedRig {
    dir: &'static str,
    display: &'static str,
    armature: &'static str,
    default_animation: &'static str,
    scale: f32,
    center: [f32; 2],
    aliases: &'static [&'static str],
}

const CURATED_RIGS: &[CuratedRig] = &[
    CuratedRig {
        dir: "Dragon",
        display: "Dragon",
        armature: "Dragon",
        default_animation: "stand",
        scale: 0.75,
        center: [112.23, -46.99],
        aliases: &[],
    },
    CuratedRig {
        dir: "Dragon_Blend",
        display: "Dragon Blend",
        armature: "Dragon",
        default_animation: "stand",
        scale: 0.75,
        center: [112.23, -46.99],
        aliases: &[],
    },
    CuratedRig {
        dir: "PlantAnimate",
        display: "PlantAnimate",
        armature: "MegaGatlingPea",
        default_animation: "Idle",
        scale: 1.6,
        center: [8.05, -70.09],
        aliases: &["plant"],
    },
    CuratedRig {
        dir: "Rooster_Ani",
        display: "Rooster",
        armature: "armatureName",
        default_animation: "rooster_idle_anim",
        scale: 0.9,
        center: [22.16, 41.23],
        aliases: &["rooster"],
    },
    CuratedRig {
        dir: "龙",
        display: "龙",
        armature: "armatureName",
        default_animation: "stand",
        scale: 0.7,
        center: [-82.45, -241.22],
        aliases: &["long"],
    },
    CuratedRig {
        dir: "Surface_P1_4",
        display: "Surface",
        armature: "fixture",
        default_animation: "idle",
        scale: 1.0,
        center: [0.0, 0.0],
        aliases: &["surface"],
    },
    CuratedRig {
        dir: "Peashooter",
        display: "Peashooter",
        armature: "Peashooter",
        default_animation: "Idle",
        scale: 2.27,
        center: [17.91, -52.40],
        aliases: &[],
    },
    CuratedRig {
        dir: "Sunflower",
        display: "Sunflower",
        armature: "Sunflower",
        default_animation: "Idle",
        scale: 1.93,
        center: [0.89, -57.28],
        aliases: &[],
    },
    CuratedRig {
        dir: "Wallnut",
        display: "Wallnut",
        armature: "Wall-nut",
        default_animation: "Idle1",
        scale: 1.81,
        center: [-0.46, -57.50],
        aliases: &[],
    },
    CuratedRig {
        dir: "GatlingPea",
        display: "GatlingPea",
        armature: "GatlingMGP",
        default_animation: "Idle",
        scale: 1.86,
        center: [15.95, -68.10],
        aliases: &[],
    },
    CuratedRig {
        dir: "Torchwood",
        display: "Torchwood",
        armature: "Torchwood",
        default_animation: "Idle",
        scale: 1.14,
        center: [-3.17, -57.57],
        aliases: &[],
    },
    CuratedRig {
        dir: "BasicZombie",
        display: "BasicZombie",
        armature: "BasicZombie",
        default_animation: "Idle",
        scale: 1.3,
        center: [-18.05, -92.97],
        aliases: &[],
    },
    CuratedRig {
        dir: "Gargantuar",
        display: "Gargantuar",
        armature: "Gargantuar",
        default_animation: "Idle",
        scale: 0.6,
        center: [-47.79, -207.71],
        aliases: &[],
    },
    CuratedRig {
        dir: "CrazyDave",
        display: "CrazyDave",
        armature: "CrazyDave",
        default_animation: "Idle",
        scale: 0.3,
        center: [-2.92, 21.58],
        aliases: &[],
    },
    CuratedRig {
        dir: "ZombossHead",
        display: "ZombossHead",
        armature: "ZombossHead",
        default_animation: "Idle00",
        scale: 0.33,
        center: [2.04, -338.17],
        aliases: &[],
    },
    CuratedRig {
        dir: "Shovel",
        display: "Shovel",
        armature: "shovelInHand",
        default_animation: "ShovelInHand",
        scale: 1.21,
        center: [102.46, -7.58],
        aliases: &[],
    },
    CuratedRig {
        dir: "DinoRaptor",
        display: "DinoRaptor",
        armature: "Raptor",
        default_animation: "Idle",
        scale: 0.76,
        center: [30.78, -80.78],
        aliases: &[],
    },
    CuratedRig {
        dir: "AquaVine",
        display: "AquaVine",
        armature: "Aquavine",
        default_animation: "Idle",
        scale: 1.21,
        center: [15.33, -10.75],
        aliases: &[],
    },
    CuratedRig {
        dir: "ElectricPea",
        display: "ElectricPea",
        armature: "ElectricPeashooter",
        default_animation: "Idle",
        scale: 1.38,
        center: [16.33, -91.27],
        aliases: &[],
    },
    CuratedRig {
        dir: "Chomper",
        display: "Chomper",
        armature: "Chomper",
        default_animation: "Idle",
        scale: 1.31,
        center: [28.72, -85.27],
        aliases: &[],
    },
    CuratedRig {
        dir: "Repeater",
        display: "Repeater",
        armature: "Repeater",
        default_animation: "Idle",
        scale: 2.16,
        center: [11.56, -55.02],
        aliases: &[],
    },
    CuratedRig {
        dir: "Cactus",
        display: "Cactus",
        armature: "Cactus",
        default_animation: "Idle",
        scale: 1.18,
        center: [-14.94, -77.79],
        aliases: &[],
    },
    CuratedRig {
        dir: "Marigold",
        display: "Marigold",
        armature: "MariGold",
        default_animation: "Idle",
        scale: 2.04,
        center: [-3.95, -51.33],
        aliases: &[],
    },
    CuratedRig {
        dir: "MelonPult",
        display: "MelonPult",
        armature: "Melon-pult",
        default_animation: "Idle",
        scale: 1.18,
        center: [-54.14, -67.74],
        aliases: &[],
    },
    CuratedRig {
        dir: "BalloonZombie",
        display: "BalloonZombie",
        armature: "BalloonZombie",
        default_animation: "Idle",
        scale: 1.23,
        center: [-20.23, -100.92],
        aliases: &[],
    },
    CuratedRig {
        dir: "DarkKingZombie",
        display: "DarkKingZombie",
        armature: "KingVeteran",
        default_animation: "Idle",
        scale: 0.91,
        center: [20.89, -121.89],
        aliases: &[],
    },
    CuratedRig {
        dir: "GloomShroom",
        display: "GloomShroom",
        armature: "GloomShroom",
        default_animation: "Idle",
        scale: 1.46,
        center: [1.33, -74.73],
        aliases: &[],
    },
];

const IDLE_HINTS: &[&str] = &["idle", "stand", "wait", "static", "loop"];

#[derive(Resource)]
struct DemoCatalog {
    rigs: Vec<RigEntry>,
}

#[derive(Resource)]
struct SelectedDemo {
    index: usize,
    current: usize,
    /// Set by a click / Up-Down; applied by [`apply_selection`].
    pending: Option<usize>,
}

#[derive(Component)]
struct DemoRig;

#[derive(Component)]
struct AnimationHud;

#[derive(Component)]
struct RigButton(usize);

fn assets_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn discover_rigs() -> Result<Vec<RigEntry>, String> {
    let root = assets_root();
    let read_dir = std::fs::read_dir(&root)
        .map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    let mut dirs = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|error| format!("cannot list {}: {error}", root.display()))?;
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            dirs.push(entry.path());
        }
    }
    dirs.sort_by_key(|dir| {
        dir.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    let mut rigs = Vec::new();
    for dir in dirs {
        match load_rig(&dir) {
            Ok(Some(rig)) => rigs.push(rig),
            Ok(None) => {}
            Err(message) => eprintln!("warning: {message}"),
        }
    }
    Ok(rigs)
}

fn load_rig(dir: &Path) -> Result<Option<RigEntry>, String> {
    let dir_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("non-UTF-8 asset directory {}", dir.display()))?
        .to_owned();

    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    files.sort();
    let preferred = format!("{dir_name}_ske.json");
    let Some(skeleton_file) = files
        .iter()
        .find(|file| **file == preferred)
        .or_else(|| files.iter().find(|file| file.ends_with("_ske.json")))
        .cloned()
    else {
        return Ok(None);
    };
    let skeleton_stem = skeleton_file
        .strip_suffix("_ske.json")
        .ok_or_else(|| format!("unexpected skeleton name {skeleton_file}"))?;
    let atlas_file = format!("{skeleton_stem}_tex.json");
    if !files.iter().any(|file| **file == atlas_file) {
        return Err(format!(
            "{}: skeleton {skeleton_file} has no matching {atlas_file}",
            dir.display()
        ));
    }

    let skeleton_path = dir.join(&skeleton_file);
    let text = std::fs::read_to_string(&skeleton_path)
        .map_err(|error| format!("cannot read {}: {error}", skeleton_path.display()))?;
    let skeleton: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("cannot parse {}: {error}", skeleton_path.display()))?;
    let armatures = skeleton
        .get("armature")
        .and_then(|value| value.as_array())
        .filter(|armatures| !armatures.is_empty())
        .ok_or_else(|| format!("{}: skeleton has no armatures", skeleton_path.display()))?;

    let curated = CURATED_RIGS.iter().find(|rig| rig.dir == dir_name);
    let armature = pick_armature(armatures, curated.map(|rig| rig.armature));
    let armature_name = armature
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or(dir_name.as_str())
        .to_owned();
    let animations: Vec<String> = armature
        .get("animation")
        .and_then(|value| value.as_array())
        .map(|animations| {
            animations
                .iter()
                .filter_map(|animation| {
                    animation
                        .get("name")
                        .and_then(|name| name.as_str())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    let default_animation = pick_animation(&animations, curated.map(|rig| rig.default_animation));

    let (scale, center) = match curated {
        Some(curated) => (
            curated.scale,
            Vec2::new(curated.center[0], curated.center[1]),
        ),
        None => auto_presentation(armature),
    };

    Ok(Some(RigEntry {
        dir: dir_name.clone(),
        display: curated
            .map(|curated| curated.display.to_owned())
            .unwrap_or_else(|| dir_name.clone()),
        skeleton: format!("{dir_name}/{skeleton_file}"),
        atlas: format!("{dir_name}/{atlas_file}"),
        armature: armature_name,
        default_animation,
        animations,
        scale,
        center,
        aliases: curated
            .map(|curated| curated.aliases.to_vec())
            .unwrap_or_default(),
    }))
}

fn pick_armature<'a>(
    armatures: &'a [serde_json::Value],
    wanted: Option<&str>,
) -> &'a serde_json::Value {
    if let Some(wanted) = wanted
        && let Some(armature) = armatures
            .iter()
            .find(|armature| armature.get("name").and_then(|name| name.as_str()) == Some(wanted))
    {
        return armature;
    }
    armatures
        .iter()
        .max_by_key(|armature| {
            (
                armature
                    .get("animation")
                    .and_then(|value| value.as_array())
                    .map_or(0, Vec::len),
                armature
                    .get("bone")
                    .and_then(|value| value.as_array())
                    .map_or(0, Vec::len),
            )
        })
        .expect("armatures is not empty")
}

fn pick_animation(animations: &[String], wanted: Option<&str>) -> String {
    if let Some(wanted) = wanted
        && animations.iter().any(|animation| animation == wanted)
    {
        return wanted.to_owned();
    }
    let hinted = IDLE_HINTS.iter().find_map(|hint| {
        animations.iter().find(|animation| {
            let lower = animation.to_lowercase();
            lower == *hint || lower.starts_with(hint)
        })
    });
    hinted
        .or_else(|| animations.first())
        .cloned()
        .unwrap_or_default()
}

fn auto_presentation(armature: &serde_json::Value) -> (f32, Vec2) {
    let aabb = armature.get("aabb");
    let value = |name: &str| {
        aabb.and_then(|aabb| aabb.get(name))
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0) as f32
    };
    let (x, y, width, height) = (value("x"), value("y"), value("width"), value("height"));
    let center = Vec2::new(x + width / 2.0, y + height / 2.0);
    let max_dim = width.max(height).max(1.0);
    let scale = ((320.0 / max_dim).clamp(0.3, 3.0) * 100.0).round() / 100.0;
    (scale, center)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn resolve_rig(rigs: &[RigEntry], name: &str) -> Option<usize> {
    let needle = name.trim().to_lowercase();
    let normalized = normalize_name(&needle);
    if needle.is_empty() {
        return None;
    }
    rigs.iter().position(|rig| {
        rig.dir.to_lowercase() == needle
            || rig.display.to_lowercase() == needle
            || normalize_name(&rig.dir) == normalized
            || normalize_name(&rig.display) == normalized
            || rig
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&needle))
    })
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

enum Cli {
    Help,
    List,
    Run(Option<String>),
}

fn parse_cli() -> Cli {
    let mut name: Option<String> = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--demo=") {
            name = Some(value.to_owned());
            continue;
        }
        match arg.as_str() {
            "--demo" | "-d" => {
                let Some(value) = args.next() else {
                    usage_error("`--demo` requires a value");
                };
                name = Some(value);
            }
            "--list" => return Cli::List,
            "--help" | "-h" => return Cli::Help,
            other if !other.starts_with('-') => name = Some(other.to_owned()),
            other => usage_error(&format!("unknown argument `{other}`")),
        }
    }
    Cli::Run(name)
}

fn print_rig_list(rigs: &[RigEntry]) {
    println!("{} rigs under assets/ (load order):", rigs.len());
    for rig in rigs {
        println!("  {}  ({})", rig.dir, rig.display);
    }
}

fn print_usage() {
    println!(
        "\
usage:
  cargo run -p dragonbones-demo                 # show the first rig in load order
  cargo run -p dragonbones-demo -- <rig>        # show <rig> first
  cargo run -p dragonbones-demo -- --demo <rig>
  cargo run -p dragonbones-demo -- --list

rigs are discovered from assets/<rig>/<rig>_ske.json. In the window: click a rig
in the right-hand panel or press Up/Down; 1-9 selects an animation, Left/Right
cycles the current rig's animations."
    );
}

fn usage_error(message: &str) -> ! {
    eprintln!("error: {message}\n");
    print_usage();
    std::process::exit(2);
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    catalog: Res<DemoCatalog>,
    selected: Res<SelectedDemo>,
) {
    commands.spawn(Camera2d);
    let rig = &catalog.rigs[selected.index];
    spawn_rig(&mut commands, &assets, rig);

    // DragonBones data is y-down. Conjugate through F = diag(1, -1, 1) at
    // the armature root only. T = (-s*center.x, s*center.y) centers the AABB.
    let hud = hud_text(&catalog, &selected);
    commands.spawn((
        AnimationHud,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            width: Val::Px(340.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Text::new(hud),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::WHITE),
    ));

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                right: Val::Px(12.0),
                bottom: Val::Px(12.0),
                width: Val::Px(230.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                padding: UiRect::all(Val::Px(6.0)),
                overflow: Overflow::clip_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(format!("Rigs ({})", catalog.rigs.len())),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.92, 0.92)),
            ));
            for (index, rig) in catalog.rigs.iter().enumerate() {
                let selected = index == selected.index;
                panel.spawn((
                    Button,
                    Hovered::default(),
                    RigButton(index),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(18.0),
                        padding: UiRect::horizontal(Val::Px(4.0)),
                        justify_content: JustifyContent::FlexStart,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(if selected {
                        RIG_SELECTED_BG
                    } else {
                        Color::NONE
                    }),
                    children![(
                        Text::new(rig.display.clone()),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(if selected { Color::WHITE } else { RIG_TEXT }),
                    )],
                ));
            }
        });
}

fn spawn_rig(commands: &mut Commands, assets: &AssetServer, rig: &RigEntry) -> Entity {
    let animation = if rig.animations.is_empty() {
        None
    } else {
        Some(rig.default_animation.clone())
    };
    let transform = Transform::from_scale(Vec3::new(rig.scale, -rig.scale, 1.0)).with_translation(
        Vec3::new(-rig.scale * rig.center.x, rig.scale * rig.center.y, 0.0),
    );
    commands
        .spawn((
            DemoRig,
            DragonBonesArmature {
                skeleton: assets.load(rig.skeleton.clone()),
                atlas: assets.load(rig.atlas.clone()),
                armature_name: rig.armature.clone(),
                animation,
            },
            transform,
        ))
        .id()
}

fn on_rig_activated(
    activate: On<Activate>,
    buttons: Query<&RigButton>,
    mut selected: ResMut<SelectedDemo>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        selected.pending = Some(button.0);
    }
}

fn handle_rig_keys(
    keys: Res<ButtonInput<KeyCode>>,
    catalog: Res<DemoCatalog>,
    mut selected: ResMut<SelectedDemo>,
) {
    let count = catalog.rigs.len();
    if count < 2 {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        selected.pending = Some((selected.index + 1) % count);
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        selected.pending = Some((selected.index + count - 1) % count);
    }
}

fn apply_selection(
    mut commands: Commands,
    assets: Res<AssetServer>,
    catalog: Res<DemoCatalog>,
    mut selected: ResMut<SelectedDemo>,
    rigs: Query<Entity, With<DemoRig>>,
    mut windows: Query<&mut Window>,
) {
    let Some(index) = selected.pending.take() else {
        return;
    };
    if index >= catalog.rigs.len() || index == selected.index {
        return;
    }
    let rig = &catalog.rigs[index];
    for entity in &rigs {
        commands.entity(entity).despawn();
    }
    spawn_rig(&mut commands, &assets, rig);
    selected.index = index;
    selected.current = rig
        .animations
        .iter()
        .position(|animation| *animation == rig.default_animation)
        .unwrap_or(0);
    if let Ok(mut window) = windows.single_mut() {
        window.title = format!("DragonBonesRust - {}", rig.display);
    }
    info!("rig: {} ({}/{})", rig.dir, index + 1, catalog.rigs.len());
}

fn switch_animation(
    keys: Res<ButtonInput<KeyCode>>,
    catalog: Res<DemoCatalog>,
    mut selected: ResMut<SelectedDemo>,
    mut commands: MessageWriter<DragonBonesCommand>,
    instances: Query<(Entity, &DragonBonesInstance)>,
) {
    let animations = &catalog.rigs[selected.index].animations;
    if animations.is_empty() {
        return;
    }
    let Some((root, _instance)) = instances.iter().next() else {
        return;
    };

    let mut requested = None;
    if keys.just_pressed(KeyCode::ArrowRight) {
        requested = Some((selected.current + 1) % animations.len());
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        requested = Some((selected.current + animations.len() - 1) % animations.len());
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (digit, key) in DIGITS.iter().enumerate() {
        if digit < animations.len() && keys.just_pressed(*key) {
            requested = Some(digit);
        }
    }
    let Some(next) = requested else {
        return;
    };

    let animation_name = animations[next].clone();
    // Dogfood the public command API: the adapter applies the command before
    // this frame's advance, so the pose/event updates happen immediately.
    commands.write(DragonBonesCommand::play(root, animation_name.clone()));
    selected.current = next;
    info!("animation: {animation_name}");
}

fn update_animation_hud(
    catalog: Res<DemoCatalog>,
    selected: Res<SelectedDemo>,
    mut text: Query<&mut Text, With<AnimationHud>>,
) {
    let content = hud_text(&catalog, &selected);
    for mut text in &mut text {
        if text.0 != content {
            text.0 = content.clone();
        }
    }
}

fn hud_text(catalog: &DemoCatalog, selected: &SelectedDemo) -> String {
    let rig = &catalog.rigs[selected.index];
    let mut text = format!(
        "{}  ({}/{})\n\nAnimations\n",
        rig.display,
        selected.index + 1,
        catalog.rigs.len()
    );
    for (index, animation) in rig.animations.iter().enumerate() {
        let marker = if index == selected.current { ">" } else { " " };
        text.push_str(&format!("{marker} {}. {animation}\n", index + 1));
    }
    text.push_str("\n[1-9]/[Left/Right] animation\n[Up/Down] rig  [click] rig");
    text
}

const RIG_SELECTED_BG: Color = Color::srgba(0.16, 0.42, 0.75, 0.9);
const RIG_HOVERED_BG: Color = Color::srgba(0.35, 0.35, 0.35, 0.8);
const RIG_TEXT: Color = Color::srgb(0.82, 0.82, 0.82);

fn update_rig_buttons(
    selected: Res<SelectedDemo>,
    mut buttons: Query<(
        &RigButton,
        &Hovered,
        Has<Pressed>,
        &mut BackgroundColor,
        &Children,
    )>,
    mut text_colors: Query<&mut TextColor>,
) {
    for (button, hovered, pressed, mut background, children) in &mut buttons {
        let is_selected = button.0 == selected.index;
        let color = if is_selected {
            RIG_SELECTED_BG
        } else if hovered.get() || pressed {
            RIG_HOVERED_BG
        } else {
            Color::NONE
        };
        *background = BackgroundColor(color);
        if let Some(&child) = children.first()
            && let Ok(mut text_color) = text_colors.get_mut(child)
        {
            *text_color = TextColor(if is_selected { Color::WHITE } else { RIG_TEXT });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_every_rig_under_assets() {
        let rigs = discover_rigs().expect("assets/ is discoverable");
        assert!(
            rigs.len() >= 29,
            "expected at least 29 rigs, discovered {}",
            rigs.len()
        );
        for rig in &rigs {
            assert!(!rig.animations.is_empty(), "{} has no animations", rig.dir);
            assert!(
                rig.animations.contains(&rig.default_animation),
                "{}: default animation {:?} is not in {:?}",
                rig.dir,
                rig.default_animation,
                rig.animations
            );
            let skeleton = assets_root().join(&rig.skeleton);
            assert!(skeleton.is_file(), "missing {}", skeleton.display());
            let atlas = assets_root().join(&rig.atlas);
            assert!(atlas.is_file(), "missing {}", atlas.display());
        }
    }

    #[test]
    fn resolves_names_and_aliases() {
        let rigs = discover_rigs().expect("assets/ is discoverable");
        let index = |name: &str| resolve_rig(&rigs, name);
        assert_eq!(index("PlantAnimate"), index("plant"));
        assert_eq!(index("plantanimate"), index("plant"));
        assert_eq!(index("Rooster"), index("Rooster_Ani"));
        assert_eq!(index("long"), index("龙"));
        assert_eq!(index("surface"), index("Surface_P1_4"));
        assert_eq!(index("basic_zombie"), index("BasicZombie"));
        assert_eq!(index("dino_raptor"), index("DinoRaptor"));
        assert_eq!(index("melon_pult"), index("MelonPult"));
        assert_eq!(index("electric_pea"), index("ElectricPea"));
        assert_eq!(
            index("plant").map(|index| rigs[index].display.as_str()),
            Some("PlantAnimate")
        );
        assert!(index("not-a-rig").is_none());
    }

    #[test]
    fn load_order_is_sorted_by_directory() {
        let rigs = discover_rigs().expect("assets/ is discoverable");
        let actual: Vec<String> = rigs.iter().map(|rig| rig.dir.to_lowercase()).collect();
        let mut sorted = actual.clone();
        sorted.sort();
        assert_eq!(actual, sorted);
        assert_eq!(rigs[0].dir, "AquaVine");
    }
}
