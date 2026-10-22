use bevy::prelude::*;
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::ui::widget::Text;
use bevy::ui::{BackgroundColor, Node, PositionType, UiRect, Val};
use bevy::window::WindowResolution;
use dragonbones_bevy::DragonBonesRuntime;
use dragonbones_bevy::prelude::*;

/// Usage:
///   cargo run -p dragonbones-demo -- [--demo <name>]
///   cargo run -p dragonbones-demo -- <name>
///   cargo run -p dragonbones-demo -- --list
///
/// In the running demo, press 1-9 to select an animation, or Left/Right to
/// cycle through the current rig's animation list. The animation HUD is a
/// Bevy UI overlay, so animation selection is not a CLI concern.
fn main() {
    let selected = parse_demo_choice();
    let title = format!("DragonBonesRust - {}", selected.demo.name);

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
                    file_path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../assets")
                        .to_string_lossy()
                        .into_owned(),
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
        .insert_resource(selected)
        .add_plugins(DragonBonesPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (switch_animation, update_animation_hud).chain())
        .run();
}

const DEMO_NAMES: &[&str] = &[
    "dragon",
    "dragon_blend",
    "plant",
    "rooster",
    "long",
    "surface",
];

#[derive(Resource)]
struct SelectedDemo {
    demo: DemoChoice,
    current: usize,
}

#[derive(Component)]
struct AnimationHud;

struct DemoChoice {
    name: &'static str,
    skeleton: &'static str,
    atlas: &'static str,
    armature: &'static str,
    default_animation: &'static str,
    animations: Vec<String>,
    /// Uniform scale applied to the armature root.
    scale: f32,
    /// Armature-space AABB center from the skeleton data.
    center: Vec2,
}

fn parse_demo_choice() -> SelectedDemo {
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
            "--list" => {
                println!("available demos: {}", DEMO_NAMES.join(", "));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other if !other.starts_with('-') => {
                name = Some(other.to_owned());
            }
            other => usage_error(&format!("unknown argument `{other}`")),
        }
    }

    let demo_name = name.as_deref().unwrap_or("dragon");
    let mut demo = demo_choice(demo_name);
    demo.animations = animation_names_for(&demo);
    if demo.animations.is_empty() {
        usage_error(&format!(
            "could not read animations for `{demo_name}` from assets/{}",
            demo.skeleton
        ));
    }
    let current = demo
        .animations
        .iter()
        .position(|animation| animation == demo.default_animation)
        .unwrap_or(0);
    SelectedDemo { demo, current }
}

fn animation_names_for(demo: &DemoChoice) -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .join(demo.skeleton);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value): Result<serde_json::Value, _> = serde_json::from_str(&text) else {
        return Vec::new();
    };
    value
        .get("armature")
        .and_then(|value| value.as_array())
        .and_then(|armatures| {
            armatures.iter().find(|armature| {
                armature.get("name").and_then(|name| name.as_str()) == Some(demo.armature)
            })
        })
        .and_then(|armature| armature.get("animation"))
        .and_then(|animation| animation.as_array())
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
        .unwrap_or_default()
}

fn demo_choice(name: &str) -> DemoChoice {
    match name {
        "dragon" => DemoChoice {
            name: "Dragon",
            skeleton: "Dragon_ske.json",
            atlas: "Dragon_tex.json",
            armature: "Dragon",
            default_animation: "stand",
            animations: Vec::new(),
            scale: 0.75,
            center: Vec2::new(112.23, -46.99),
        },
        "dragon_blend" => DemoChoice {
            name: "Dragon Blend",
            skeleton: "Dragon_Blend_ske.json",
            atlas: "Dragon_Blend_tex.json",
            armature: "Dragon",
            default_animation: "stand",
            animations: Vec::new(),
            scale: 0.75,
            center: Vec2::new(112.23, -46.99),
        },
        "plant" => DemoChoice {
            name: "PlantAnimate",
            skeleton: "PlantAnimate_ske.json",
            atlas: "PlantAnimate_tex.json",
            armature: "MegaGatlingPea",
            default_animation: "Idle",
            animations: Vec::new(),
            scale: 1.6,
            center: Vec2::new(8.05, -70.09),
        },
        "rooster" => DemoChoice {
            name: "Rooster",
            skeleton: "Rooster_Ani_ske.json",
            atlas: "Rooster_Ani_tex.json",
            armature: "armatureName",
            default_animation: "rooster_idle_anim",
            animations: Vec::new(),
            scale: 0.9,
            center: Vec2::new(22.16, 41.23),
        },
        "long" => DemoChoice {
            name: "龙",
            skeleton: "龙_ske.json",
            atlas: "龙_tex.json",
            armature: "armatureName",
            default_animation: "stand",
            animations: Vec::new(),
            scale: 0.7,
            center: Vec2::new(-82.45, -241.22),
        },
        "surface" => DemoChoice {
            name: "Surface",
            skeleton: "Surface_P1_4_ske.json",
            atlas: "Surface_P1_4_tex.json",
            armature: "fixture",
            default_animation: "idle",
            animations: Vec::new(),
            scale: 1.0,
            center: Vec2::ZERO,
        },
        other => usage_error(&format!(
            "unknown demo `{other}` (available: {})",
            DEMO_NAMES.join(", ")
        )),
    }
}

fn print_usage() {
    println!(
        "\
usage:
  cargo run -p dragonbones-demo -- [--demo <name>]
  cargo run -p dragonbones-demo -- <name>
  cargo run -p dragonbones-demo -- --list

demos:
  {}

In the window: 1-9 selects an animation, Left/Right cycles animations.",
        DEMO_NAMES.join(", ")
    );
}

fn usage_error(message: &str) -> ! {
    eprintln!("error: {message}\n");
    print_usage();
    std::process::exit(2);
}

fn switch_animation(
    keys: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedDemo>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    instances: Query<&DragonBonesInstance>,
) {
    if selected.demo.animations.is_empty() {
        return;
    }
    let Some(instance) = instances.iter().next() else {
        return;
    };

    let mut next = selected.current;
    let mut requested = None;
    if keys.just_pressed(KeyCode::ArrowRight) {
        requested = Some((next + 1) % selected.demo.animations.len());
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        requested =
            Some((next + selected.demo.animations.len() - 1) % selected.demo.animations.len());
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
        if digit < selected.demo.animations.len() && keys.just_pressed(*key) {
            requested = Some(digit);
        }
    }
    let Some(next_index) = requested else {
        return;
    };
    next = next_index;

    let animation_name = selected.demo.animations[next].clone();
    let played = {
        let Some(armature) = runtime
            .factory
            .armatures
            .get_mut(instance.armature.index())
            .and_then(Option::as_mut)
        else {
            return;
        };
        armature
            .animation
            .play(Some(animation_name.as_str()), -1)
            .is_some()
    };
    if played {
        runtime.factory.advance_one(instance.armature, 0.0);
        selected.current = next;
        info!("animation: {animation_name}");
    }
}

fn update_animation_hud(
    selected: Res<SelectedDemo>,
    mut text: Query<&mut Text, With<AnimationHud>>,
) {
    let content = hud_text(&selected);
    for mut text in &mut text {
        if text.0 != content {
            text.0 = content.clone();
        }
    }
}

fn hud_text(selected: &SelectedDemo) -> String {
    let mut text = String::from("Animations\n");
    for (index, animation) in selected.demo.animations.iter().enumerate() {
        let marker = if index == selected.current { ">" } else { " " };
        text.push_str(&format!("{marker} {}. {animation}\n", index + 1));
    }
    text.push_str("\n[1-9] select   [Left/Right] cycle");
    text
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, selected: Res<SelectedDemo>) {
    commands.spawn(Camera2d);
    let demo = &selected.demo;
    let animation = demo.animations.get(selected.current).cloned();

    // DragonBones data is y-down. Conjugate through F = diag(1, -1, 1) at
    // the armature root only. T = (-s*center.x, s*center.y) centers the AABB.
    let transform = Transform::from_scale(Vec3::new(demo.scale, -demo.scale, 1.0))
        .with_translation(Vec3::new(
            -demo.scale * demo.center.x,
            demo.scale * demo.center.y,
            0.0,
        ));
    commands.spawn((
        DragonBonesArmature {
            skeleton: assets.load(demo.skeleton),
            atlas: assets.load(demo.atlas),
            armature_name: demo.armature.to_owned(),
            animation,
        },
        transform,
    ));

    commands.spawn((
        AnimationHud,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            width: Val::Px(400.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Text::new(hud_text(&selected)),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
    ));
}
