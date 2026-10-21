//! 阶段 6：P1-5 replaceDisplay / replaceSlotDisplay(List) / replaceSkin。

mod common;

use common::read_value;
use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

fn build_features() -> (BaseFactory, ArmatureIdx) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/synthetic");
    let ske = read_value(&root.join("features/features_ske.json"));
    let tex = read_value(&root.join("synthetic_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "features").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "features"));
    let armature = factory
        .build_armature(
            "features_fixture",
            "features",
            "",
            "features",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build");
    factory
        .attach_armature(factory.root_clock(), armature)
        .expect("attach");
    (factory, armature)
}

fn display_names(factory: &BaseFactory, armature: ArmatureIdx) -> Vec<String> {
    let slot = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == "slot")
        .expect("slot");
    slot.display_frames
        .iter()
        .map(|frame| {
            frame
                .display_data
                .as_ref()
                .or(frame.raw_display_data.as_ref())
                .map(|display| display.name.clone())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn replace_display_and_slot_display() {
    let (mut factory, armature) = build_features();
    let slot_idx = factory.armature(armature).get_slot("slot").expect("slot");
    assert_eq!(display_names(&factory, armature), ["img", "img2"]);

    let img2 = factory
        .get_armature_data("features_fixture", "features")
        .and_then(|armature| {
            armature
                .default_skin
                .as_deref()
                .and_then(|name| armature.get_skin(name))
        })
        .and_then(|skin| skin.get_display("slot", "img2"))
        .cloned()
        .expect("img2");
    factory.replace_display(armature, slot_idx, Some(img2), 0);
    assert_eq!(display_names(&factory, armature), ["img2", "img2"]);

    assert!(factory.replace_slot_display(
        "features_fixture",
        "features",
        "slot",
        "img",
        armature,
        slot_idx,
        0,
    ));
    let slot = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == "slot")
        .unwrap();
    assert_eq!(
        slot.display_frames[0]
            .display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some("img2")
    );

    factory.replace_display(armature, slot_idx, None, 0);
    let slot = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == "slot")
        .unwrap();
    assert!(slot.display_frames[0].display_data.is_none());
}

#[test]
fn replace_slot_display_list_resets_all_frames() {
    let (mut factory, armature) = build_features();
    let slot_idx = factory.armature(armature).get_slot("slot").expect("slot");
    factory.replace_slot_display_list("features_fixture", "features", "slot", armature, slot_idx);
    assert_eq!(display_names(&factory, armature), ["img", "img2"]);
}

#[test]
fn replace_skin_override_and_exclude() {
    let (mut factory, armature) = build_features();
    let alt = factory
        .get_armature_data("features_fixture", "features")
        .and_then(|armature| armature.get_skin("alt"))
        .cloned()
        .expect("alt skin");
    assert!(factory.replace_skin(armature, "features", &alt, true, &[]));
    let raw = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == "slot")
        .unwrap()
        .display_frames[0]
        .raw_display_data
        .as_ref()
        .map(|display| display.name.as_str());
    assert_eq!(raw, Some("altimg"));

    let (mut factory, armature) = build_features();
    assert!(!factory.replace_skin(armature, "features", &alt, true, &["slot"]));
    assert_eq!(display_names(&factory, armature), ["img", "img2"]);
}
#[test]
fn replace_animation_override_replaces_map() {
    let (mut factory, armature) = build_features();
    assert!(factory.replace_animation(armature, "features", "alt_source", true));
    let animation = &factory.armature(armature).animation;
    assert!(animation.has_animation("wave"));
    assert!(!animation.has_animation("idle"));
    assert_eq!(animation.animation_data("wave").unwrap().duration, 0.5);
}
