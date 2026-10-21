//! 阶段 6：P1-6/7/8 数据层审计（UserData / ActionData / defaultActions /
//! CanvasData / 14 BlendMode）。

mod common;

use common::read_value;
use dragonbones_core::core::enums::ActionType;
use dragonbones_core::core::enums::BlendMode;
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

#[test]
fn user_data_actions_canvas_and_blend_mode_are_parsed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let ske = read_value(&root.join("synthetic/features/features_ske.json"));
    let mut factory = BaseFactory::new();
    let data = factory.parse_dragon_bones_data(&ske, "features").expect("parse");
    let armature = data.armatures.get("features_fixture").expect("armature");

    let slot = armature.slots.get("slot").expect("slot");
    assert_eq!(slot.blend_mode, BlendMode::Add);
    assert_eq!(slot.color.alpha_multiplier, 0.8);
    assert_eq!(slot.color.red_multiplier, 0.9);

    let canvas = armature.canvas.as_ref().expect("canvas");
    assert_eq!(canvas.x, 1.0);
    assert_eq!(canvas.y, 2.0);
    assert_eq!(canvas.width, 640.0);
    assert_eq!(canvas.height, 480.0);
    assert!(canvas.has_background);
    assert_eq!(canvas.color, 255);

    assert_eq!(armature.default_actions.len(), 1);
    assert_eq!(armature.default_actions[0].action_type, ActionType::Play);
    assert_eq!(armature.default_actions[0].name, "idle");
}

#[test]
fn all_fourteen_blend_modes_parse_from_strings() {
    let names = [
        "normal", "add", "alpha", "darken", "difference", "erase", "hardlight", "invert",
        "layer", "lighten", "multiply", "overlay", "screen", "subtract",
    ];
    let expected = [
        BlendMode::Normal,
        BlendMode::Add,
        BlendMode::Alpha,
        BlendMode::Darken,
        BlendMode::Difference,
        BlendMode::Erase,
        BlendMode::HardLight,
        BlendMode::Invert,
        BlendMode::Layer,
        BlendMode::Lighten,
        BlendMode::Multiply,
        BlendMode::Overlay,
        BlendMode::Screen,
        BlendMode::Subtract,
    ];
    let mut slots = Vec::new();
    for (index, name) in names.iter().enumerate() {
        slots.push(serde_json::json!({
            "name": format!("slot_{index}"),
            "parent": "root",
            "blendMode": name,
        }));
    }
    let ske = serde_json::json!({
        "frameRate": 24,
        "name": "BlendModes",
        "version": "5.6",
        "compatibleVersion": "5.6",
        "armature": [{
            "type": "Armature",
            "name": "blend_modes",
            "frameRate": 24,
            "bone": [{ "name": "root" }],
            "slot": slots,
        }],
    });
    let mut factory = BaseFactory::new();
    let data = factory.parse_dragon_bones_data(&ske, "blend").expect("parse");
    let armature = data.armatures.get("blend_modes").expect("armature");
    for (index, expected) in expected.iter().enumerate() {
        assert_eq!(
            armature.slots.get(&format!("slot_{index}")).unwrap().blend_mode,
            *expected,
            "blend mode {index}"
        );
    }
}
