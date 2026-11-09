use std::path::{Path, PathBuf};

use bevy::picking::Pickable;
use bevy::picking::events::PointerClick;
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
use dragonbones_bevy::DragonBonesRuntime;
use dragonbones_bevy::prelude::*;

/// Usage:
///   cargo run -p dragonbones-demo                 # show the first rig in load order
///   cargo run -p dragonbones-demo -- <rig>        # show <rig> first
///   cargo run -p dragonbones-demo -- --list       # list the rigs found under assets/
///
/// Every rig under `assets/<rig>/<rig>_ske.json` is loaded; the right-hand panel
/// switches between them (click a name, or press Up/Down). The left-hand panel
/// lists the current rig's animations and plays whichever one you click; below
/// it are preview controls with a clickable seek bar. Space pauses, 1-9 and
/// Left/Right cycle animations, `[`/`]` change speed, F1 toggles the bone
/// gizmos, and M adds eight extra instances of the current rig.
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
            pending_animation: None,
        })
        .init_resource::<Previewer>()
        .init_resource::<TimelineSnapshot>()
        .init_resource::<AnimationPanelState>()
        .add_plugins(DragonBonesPlugin)
        .add_observer(on_rig_activated)
        .add_observer(on_animation_activated)
        .add_observer(on_timeline_click)
        .add_observer(on_play_pause_activated)
        .add_observer(on_speed_activated)
        .add_observer(on_loop_activated)
        .add_observer(on_debug_activated)
        .add_observer(on_stress_activated)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                handle_rig_keys,
                apply_selection,
                handle_preview_keys,
                handle_animation_keys,
                apply_animation_request,
                sync_animation_panel,
                refresh_stress_clones,
                update_animation_hud,
                update_rig_buttons,
                update_animation_buttons,
                update_control_labels,
                update_timeline,
                update_preview_status,
                draw_debug_gizmos,
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
        display: "Long",
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
    /// Set by an animation click / key; applied by [`apply_animation_request`].
    pending_animation: Option<usize>,
}

/// Preview-only state: playback controls that live above the adapter commands.
#[derive(Resource)]
struct Previewer {
    paused: bool,
    speed: f64,
    looping: bool,
    debug_overlay: bool,
    stress: bool,
    /// Smoothed frames per second, purely for the status line.
    fps: f32,
}

impl Default for Previewer {
    fn default() -> Self {
        Self {
            paused: false,
            speed: 1.0,
            looping: true,
            debug_overlay: false,
            stress: false,
            fps: 0.0,
        }
    }
}

/// Latest playback position of the first animation state, refreshed every frame.
#[derive(Resource, Default)]
struct TimelineSnapshot {
    animation: String,
    current: f64,
    total: f64,
}

/// Which rig the animation panel was built for; rebuilt when the rig changes.
#[derive(Resource, Default)]
struct AnimationPanelState {
    rig: Option<usize>,
}

#[derive(Component)]
struct DemoRig;

/// The rig rendered by the main (commandable and seeded) instance.
#[derive(Component)]
struct MainRig;

/// Which catalog entry a rendered rig entity shows (also on stress copies).
#[derive(Component)]
struct VisualRig(usize);

/// Extra copy spawned by the stress toggle.
#[derive(Component)]
struct StressRig;

#[derive(Component)]
struct AnimationHud;

#[derive(Component)]
struct RigButton(usize);

#[derive(Component)]
struct AnimationList;

#[derive(Component)]
struct AnimationButton(usize);

#[derive(Component)]
struct TimelineTrack;

#[derive(Component)]
struct TimelineFill;

#[derive(Component)]
struct TimelineLabel;

#[derive(Component)]
struct PreviewStatusText;

#[derive(Component)]
struct PlayPauseButton;

#[derive(Component)]
struct SpeedButton(i8);

#[derive(Component)]
struct LoopButton;

#[derive(Component)]
struct DebugButton;

#[derive(Component)]
struct StressButton;

const PANEL_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.93);
const BUTTON_BG: Color = Color::srgba(0.22, 0.24, 0.28, 1.0);
const BUTTON_HOVER_BG: Color = Color::srgba(0.35, 0.38, 0.44, 1.0);
const TIMELINE_BG: Color = Color::srgba(0.15, 0.17, 0.21, 1.0);
const TIMELINE_FILL: Color = Color::srgba(0.28, 0.55, 0.95, 1.0);
const ANIM_SELECTED_BG: Color = Color::srgba(0.16, 0.42, 0.75, 0.9);
const ANIM_TEXT: Color = Color::srgb(0.82, 0.82, 0.82);
const BONE_LINE: Color = Color::srgb(0.15, 0.85, 0.9);
const BONE_JOINT: Color = Color::srgb(1.0, 0.45, 0.2);
const SLOT_JOINT: Color = Color::srgb(0.9, 0.3, 0.9);

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
in the right-hand panel or press Up/Down; click an animation in the left-hand
panel (or 1-9 / Left-Right) to play it; Space pauses, `[`/`]` changes speed,
F1 toggles the bone gizmos and M adds eight stress-test copies."
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
    let main = spawn_rig(&mut commands, &assets, rig, selected.index);
    commands.entity(main).insert(MainRig);

    // Left: clickable animation list for the current rig.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                width: Val::Px(300.0),
                height: Val::Px(430.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                padding: UiRect::all(Val::Px(6.0)),
                overflow: Overflow::clip_y(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|panel| {
            panel.spawn((
                AnimationHud,
                Text::new(hud_text(&catalog, &selected)),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.92, 0.92)),
                Node {
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                },
            ));
            panel.spawn((
                AnimationList,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(1.0),
                    ..default()
                },
            ));
        });

    // Bottom-left: playback controls + seek bar + status line.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Button,
                        Hovered::default(),
                        PlayPauseButton,
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("Pause"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                    row.spawn((
                        Button,
                        Hovered::default(),
                        SpeedButton(-1),
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("-"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                    row.spawn((
                        Button,
                        Hovered::default(),
                        SpeedButton(1),
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("+"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                    row.spawn((
                        Button,
                        Hovered::default(),
                        LoopButton,
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("Loop"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                    row.spawn((
                        Button,
                        Hovered::default(),
                        DebugButton,
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("F1"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                    row.spawn((
                        Button,
                        Hovered::default(),
                        StressButton,
                        Node {
                            height: Val::Px(20.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_BG),
                        children![(
                            Text::new("x9"),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        )],
                    ));
                });

            panel.spawn((
                TimelineTrack,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(16.0),
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(TIMELINE_BG),
                children![
                    (
                        TimelineFill,
                        Pickable::IGNORE,
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(TIMELINE_FILL),
                    ),
                    (
                        TimelineLabel,
                        Pickable::IGNORE,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(4.0),
                            top: Val::Px(2.0),
                            ..default()
                        },
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ),
                ],
            ));

            panel.spawn((
                PreviewStatusText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.75, 0.8)),
            ));
        });

    // Right: rig list.
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
            BackgroundColor(PANEL_BG),
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
                        ANIM_SELECTED_BG
                    } else {
                        Color::NONE
                    }),
                    children![(
                        Text::new(rig.display.clone()),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(if selected { Color::WHITE } else { ANIM_TEXT }),
                    )],
                ));
            }
        });
}

fn spawn_rig(
    commands: &mut Commands,
    assets: &AssetServer,
    rig: &RigEntry,
    rig_index: usize,
) -> Entity {
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
            VisualRig(rig_index),
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

fn on_animation_activated(
    activate: On<Activate>,
    buttons: Query<&AnimationButton>,
    mut selected: ResMut<SelectedDemo>,
) {
    if let Ok(button) = buttons.get(activate.entity) {
        selected.pending_animation = Some(button.0);
    }
}

/// Seek bar: `PointerClick` on the track node reports a position inside it.
fn on_timeline_click(
    click: On<PointerClick>,
    tracks: Query<(&ComputedNode, &UiGlobalTransform), With<TimelineTrack>>,
    snapshot: Res<TimelineSnapshot>,
    preview: Res<Previewer>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    let Ok((computed, transform)) = tracks.get(click.entity) else {
        return;
    };
    if snapshot.total <= 0.0 || snapshot.animation.is_empty() {
        return;
    }
    let Ok(root) = main.single() else {
        return;
    };
    let (_, _, center) = transform.to_scale_angle_translation();
    let left = center.x - computed.size.x / 2.0;
    let fraction = ((click.pointer.position.x - left) / computed.size.x).clamp(0.0, 1.0) as f64;
    let position = fraction * snapshot.total;
    let play_times = if preview.looping { -1 } else { 1 };
    commands.write(DragonBonesCommand::play_config(
        root,
        DragonBonesPlayConfig::new(snapshot.animation.clone())
            .with_play_times(play_times)
            .with_position(position),
    ));
    if preview.paused {
        commands.write(DragonBonesCommand::pause(root));
    }
    info!(
        "seek: {} to {position:.2}s ({:.0}%)",
        snapshot.animation,
        fraction * 100.0
    );
}

fn on_play_pause_activated(
    activate: On<Activate>,
    buttons: Query<&PlayPauseButton>,
    mut preview: ResMut<Previewer>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    toggle_pause(&mut preview, &main, &mut commands);
}

fn on_speed_activated(
    activate: On<Activate>,
    buttons: Query<&SpeedButton>,
    mut preview: ResMut<Previewer>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    adjust_speed(&mut preview, button.0, &main, &mut commands);
}

fn on_loop_activated(
    activate: On<Activate>,
    buttons: Query<&LoopButton>,
    mut preview: ResMut<Previewer>,
    snapshot: Res<TimelineSnapshot>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    if buttons.get(activate.entity).is_err() {
        return;
    }
    toggle_loop(&mut preview, &snapshot, &main, &mut commands);
}

fn on_debug_activated(
    activate: On<Activate>,
    buttons: Query<&DebugButton>,
    mut preview: ResMut<Previewer>,
) {
    if buttons.get(activate.entity).is_ok() {
        preview.debug_overlay = !preview.debug_overlay;
        info!(
            "preview: gizmos {}",
            if preview.debug_overlay { "on" } else { "off" }
        );
    }
}

fn on_stress_activated(
    activate: On<Activate>,
    buttons: Query<&StressButton>,
    mut preview: ResMut<Previewer>,
) {
    if buttons.get(activate.entity).is_ok() {
        preview.stress = !preview.stress;
        info!(
            "preview: stress {}",
            if preview.stress { "on" } else { "off" }
        );
    }
}

fn toggle_pause(
    preview: &mut Previewer,
    main: &Query<Entity, With<MainRig>>,
    commands: &mut MessageWriter<DragonBonesCommand>,
) {
    preview.paused = !preview.paused;
    info!(
        "preview: {}",
        if preview.paused { "paused" } else { "playing" }
    );
    if let Ok(root) = main.single() {
        commands.write(if preview.paused {
            DragonBonesCommand::pause(root)
        } else {
            DragonBonesCommand::resume(root)
        });
    }
}

fn adjust_speed(
    preview: &mut Previewer,
    direction: i8,
    main: &Query<Entity, With<MainRig>>,
    commands: &mut MessageWriter<DragonBonesCommand>,
) {
    let factor = if direction > 0 { 2.0 } else { 0.5 };
    preview.speed = (preview.speed * factor).clamp(0.125, 8.0);
    info!("preview: speed {:.3}x", preview.speed);
    if let Ok(root) = main.single() {
        commands.write(DragonBonesCommand::set_speed(root, preview.speed));
    }
}

fn toggle_loop(
    preview: &mut Previewer,
    snapshot: &TimelineSnapshot,
    main: &Query<Entity, With<MainRig>>,
    commands: &mut MessageWriter<DragonBonesCommand>,
) {
    preview.looping = !preview.looping;
    info!(
        "preview: loop {}",
        if preview.looping { "on" } else { "off" }
    );
    if let Ok(root) = main.single()
        && !snapshot.animation.is_empty()
    {
        let play_times = if preview.looping { -1 } else { 1 };
        commands.write(DragonBonesCommand::play_config(
            root,
            DragonBonesPlayConfig::new(snapshot.animation.clone())
                .with_play_times(play_times)
                .with_position(snapshot.current),
        ));
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

fn handle_preview_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut preview: ResMut<Previewer>,
    snapshot: Res<TimelineSnapshot>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    if keys.just_pressed(KeyCode::Space) {
        toggle_pause(&mut preview, &main, &mut commands);
    }
    if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::Minus) {
        adjust_speed(&mut preview, -1, &main, &mut commands);
    }
    if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::Equal) {
        adjust_speed(&mut preview, 1, &main, &mut commands);
    }
    if keys.just_pressed(KeyCode::KeyL) {
        toggle_loop(&mut preview, &snapshot, &main, &mut commands);
    }
    if keys.just_pressed(KeyCode::F1) {
        preview.debug_overlay = !preview.debug_overlay;
        info!(
            "preview: gizmos {}",
            if preview.debug_overlay { "on" } else { "off" }
        );
    }
    if keys.just_pressed(KeyCode::KeyM) {
        preview.stress = !preview.stress;
        info!(
            "preview: stress {}",
            if preview.stress { "on" } else { "off" }
        );
    }
}

fn handle_animation_keys(
    keys: Res<ButtonInput<KeyCode>>,
    catalog: Res<DemoCatalog>,
    mut selected: ResMut<SelectedDemo>,
) {
    let animations = &catalog.rigs[selected.index].animations;
    if animations.is_empty() {
        return;
    }
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
    if let Some(next) = requested {
        selected.pending_animation = Some(next);
    }
}

fn apply_animation_request(
    catalog: Res<DemoCatalog>,
    mut selected: ResMut<SelectedDemo>,
    preview: Res<Previewer>,
    main: Query<Entity, With<MainRig>>,
    mut commands: MessageWriter<DragonBonesCommand>,
) {
    let Some(next) = selected.pending_animation.take() else {
        return;
    };
    let Some(animation) = catalog.rigs[selected.index].animations.get(next) else {
        return;
    };
    let Ok(root) = main.single() else {
        return;
    };
    let play_times = if preview.looping { -1 } else { 1 };
    commands.write(DragonBonesCommand::play_config(
        root,
        DragonBonesPlayConfig::new(animation.clone()).with_play_times(play_times),
    ));
    selected.current = next;
    info!("animation: {animation}");
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
    let main = spawn_rig(&mut commands, &assets, rig, index);
    commands.entity(main).insert(MainRig);
    selected.index = index;
    selected.pending_animation = None;
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

/// Rebuild the left-hand animation list whenever the selected rig changes.
fn sync_animation_panel(
    mut commands: Commands,
    catalog: Res<DemoCatalog>,
    selected: Res<SelectedDemo>,
    mut state: ResMut<AnimationPanelState>,
    panels: Query<(Entity, Option<&Children>), With<AnimationList>>,
) {
    if state.rig == Some(selected.index) {
        return;
    }
    let rig = &catalog.rigs[selected.index];
    for (list, children) in &panels {
        if let Some(children) = children {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
        for (index, animation) in rig.animations.iter().enumerate() {
            let is_selected = index == selected.current;
            commands.entity(list).with_child((
                Button,
                Hovered::default(),
                AnimationButton(index),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(18.0),
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(if is_selected {
                    ANIM_SELECTED_BG
                } else {
                    Color::NONE
                }),
                children![(
                    Text::new(animation.clone()),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(if is_selected { Color::WHITE } else { ANIM_TEXT }),
                )],
            ));
        }
    }
    state.rig = Some(selected.index);
}

/// Keep eight extra copies of the current rig around while the stress toggle is on.
fn refresh_stress_clones(
    mut commands: Commands,
    assets: Res<AssetServer>,
    catalog: Res<DemoCatalog>,
    selected: Res<SelectedDemo>,
    preview: Res<Previewer>,
    clones: Query<(Entity, &VisualRig), With<StressRig>>,
) {
    let wanted = if preview.stress {
        STRESS_COPIES.len()
    } else {
        0
    };
    if wanted == 0 {
        for (entity, _) in &clones {
            commands.entity(entity).despawn();
        }
        return;
    }
    let matches = clones.iter().count() == wanted
        && clones.iter().all(|(_, visual)| visual.0 == selected.index);
    if matches {
        return;
    }
    for (entity, _) in &clones {
        commands.entity(entity).despawn();
    }
    let rig = &catalog.rigs[selected.index];
    let base = Vec3::new(-rig.scale * rig.center.x, rig.scale * rig.center.y, 0.0);
    for (offset, _) in STRESS_COPIES {
        let entity = spawn_rig(&mut commands, &assets, rig, selected.index);
        let transform = Transform::from_scale(Vec3::new(rig.scale, -rig.scale, 1.0))
            .with_translation(base + offset);
        commands.entity(entity).insert((StressRig, transform));
    }
}

const STRESS_COPIES: [(Vec3, ()); 8] = [
    (Vec3::new(-210.0, -120.0, 0.0), ()),
    (Vec3::new(0.0, -120.0, 0.0), ()),
    (Vec3::new(210.0, -120.0, 0.0), ()),
    (Vec3::new(-210.0, 120.0, 0.0), ()),
    (Vec3::new(0.0, 120.0, 0.0), ()),
    (Vec3::new(210.0, 120.0, 0.0), ()),
    (Vec3::new(-210.0, 0.0, 0.0), ()),
    (Vec3::new(210.0, 0.0, 0.0), ()),
];

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
    let animation = rig
        .animations
        .get(selected.current)
        .map(String::as_str)
        .unwrap_or("-");
    format!(
        "{}  ({}/{})\n{} animations, playing: {animation}",
        rig.display,
        selected.index + 1,
        catalog.rigs.len(),
        rig.animations.len()
    )
}

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
            ANIM_SELECTED_BG
        } else if hovered.get() || pressed {
            BUTTON_HOVER_BG
        } else {
            Color::NONE
        };
        *background = BackgroundColor(color);
        if let Some(&child) = children.first()
            && let Ok(mut text_color) = text_colors.get_mut(child)
        {
            *text_color = TextColor(if is_selected { Color::WHITE } else { ANIM_TEXT });
        }
    }
}

fn update_animation_buttons(
    selected: Res<SelectedDemo>,
    mut buttons: Query<(
        &AnimationButton,
        &Hovered,
        Has<Pressed>,
        &mut BackgroundColor,
        &Children,
    )>,
    mut text_colors: Query<&mut TextColor>,
) {
    for (button, hovered, pressed, mut background, children) in &mut buttons {
        let is_selected = button.0 == selected.current;
        let color = if is_selected {
            ANIM_SELECTED_BG
        } else if hovered.get() || pressed {
            BUTTON_HOVER_BG
        } else {
            Color::NONE
        };
        *background = BackgroundColor(color);
        if let Some(&child) = children.first()
            && let Ok(mut text_color) = text_colors.get_mut(child)
        {
            *text_color = TextColor(if is_selected { Color::WHITE } else { ANIM_TEXT });
        }
    }
}

fn update_control_labels(
    preview: Res<Previewer>,
    play_pause: Query<&Children, With<PlayPauseButton>>,
    loop_button: Query<&Children, With<LoopButton>>,
    mut texts: Query<&mut Text>,
) {
    for children in &play_pause {
        if let Some(&child) = children.first()
            && let Ok(mut text) = texts.get_mut(child)
        {
            let wanted = if preview.paused { "Resume" } else { "Pause" };
            if text.0 != wanted {
                text.0 = wanted.to_owned();
            }
        }
    }
    for children in &loop_button {
        if let Some(&child) = children.first()
            && let Ok(mut text) = texts.get_mut(child)
        {
            let wanted = if preview.looping { "Loop" } else { "Once" };
            if text.0 != wanted {
                text.0 = wanted.to_owned();
            }
        }
    }
}

fn update_timeline(
    runtime: NonSend<DragonBonesRuntime>,
    instances: Query<&DragonBonesInstance>,
    mut snapshot: ResMut<TimelineSnapshot>,
    mut fills: Query<&mut Node, With<TimelineFill>>,
    mut labels: Query<&mut Text, With<TimelineLabel>>,
) {
    let mut current = 0.0;
    let mut total = 0.0;
    let mut animation = String::new();
    if let Some(instance) = instances.iter().next()
        && let Some(armature) = runtime.armature(instance.armature)
        && let Some((_, state)) = armature.animation.states().next()
    {
        current = state.current_time();
        total = state.total_time();
        animation = state.animation_name.clone();
    }
    let fraction = if total > 0.0 {
        (current / total).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for mut node in &mut fills {
        node.width = Val::Percent(fraction as f32 * 100.0);
    }
    let label = format!(
        "{animation}   {current:.2} / {total:.2}s  ({:.0}%)",
        fraction * 100.0
    );
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
    snapshot.animation = animation;
    snapshot.current = current;
    snapshot.total = total;
}

fn update_preview_status(
    time: Res<Time>,
    runtime: NonSend<DragonBonesRuntime>,
    instances: Query<&DragonBonesInstance>,
    mut preview: ResMut<Previewer>,
    mut texts: Query<&mut Text, With<PreviewStatusText>>,
) {
    let delta = time.delta_secs();
    if delta > 0.0 {
        let instant = 1.0 / delta;
        preview.fps = if preview.fps > 0.0 {
            preview.fps * 0.9 + instant * 0.1
        } else {
            instant
        };
    }
    let bones = instances
        .iter()
        .next()
        .and_then(|instance| runtime.armature(instance.armature))
        .map_or(0, |armature| armature.bones.len());
    let status = format!(
        "fps {:.0} | speed {:.3}x | loop {} | {} | bones {bones} | gizmos {} | stress {}",
        preview.fps,
        preview.speed,
        if preview.looping { "on" } else { "off" },
        if preview.paused { "paused" } else { "playing" },
        if preview.debug_overlay { "on" } else { "off" },
        if preview.stress { "on" } else { "off" },
    );
    for mut text in &mut texts {
        if text.0 != status {
            text.0 = status.clone();
        }
    }
}

fn draw_debug_gizmos(
    mut gizmos: Gizmos,
    preview: Res<Previewer>,
    runtime: NonSend<DragonBonesRuntime>,
    rigs: Query<(&DragonBonesInstance, &GlobalTransform)>,
) {
    if !preview.debug_overlay {
        return;
    }
    for (instance, root) in &rigs {
        let Some(armature) = runtime.armature(instance.armature) else {
            continue;
        };
        for bone in &armature.bones {
            let matrix = &bone.base.global_transform_matrix;
            let joint = root
                .transform_point(Vec3::new(matrix.tx as f32, matrix.ty as f32, 0.0))
                .truncate();
            gizmos.circle_2d(Isometry2d::from_translation(joint), 1.8, BONE_JOINT);
            if let Some(parent) = bone.parent {
                let parent_matrix = &armature.bones[parent.index()].base.global_transform_matrix;
                let parent_joint = root
                    .transform_point(Vec3::new(
                        parent_matrix.tx as f32,
                        parent_matrix.ty as f32,
                        0.0,
                    ))
                    .truncate();
                gizmos.line_2d(parent_joint, joint, BONE_LINE);
            }
        }
        for slot in &armature.slots {
            let matrix = &slot.global_transform_matrix;
            let joint = root
                .transform_point(Vec3::new(matrix.tx as f32, matrix.ty as f32, 0.0))
                .truncate();
            gizmos.circle_2d(Isometry2d::from_translation(joint), 1.2, SLOT_JOINT);
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

    #[test]
    fn every_animation_panel_entry_is_individually_playable() {
        let rigs = discover_rigs().expect("assets/ is discoverable");
        for rig in &rigs {
            for (index, animation) in rig.animations.iter().enumerate() {
                assert!(
                    !animation.is_empty(),
                    "{}: animation {index} has an empty name",
                    rig.dir
                );
            }
            let expected = rig
                .animations
                .iter()
                .position(|animation| *animation == rig.default_animation)
                .unwrap_or(0);
            assert!(expected < rig.animations.len());
        }
    }
}
