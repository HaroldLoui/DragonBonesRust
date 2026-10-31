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

#[test]
fn replace_animation_recurses_into_child_armature() {
    let (mut factory, armature) = build_features();
    let child = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == "child_slot")
        .and_then(|slot| slot.child_armature)
        .expect("child armature");
    assert!(
        factory
            .armature(child)
            .animation
            .has_animation("child_wave")
    );
    assert!(factory.replace_animation(armature, "features", "alt_source", true));
    assert!(factory.armature(child).animation.has_animation("alt_wave"));
    assert!(
        !factory
            .armature(child)
            .animation
            .has_animation("child_wave")
    );
}

// ---------------------------------------------------------------------------
// replace_* 索引归一：负索引 → displayIndex（再负则 0）、非负越界 → 静默返回
//（Slot.ts:814-819 / 844-849 / 864-869 / 884-889，四处同构）。
// ---------------------------------------------------------------------------

fn slot_mut(
    factory: &mut BaseFactory,
    armature: ArmatureIdx,
) -> &mut dragonbones_core::armature::Slot {
    factory
        .armature_mut(armature)
        .slots
        .iter_mut()
        .find(|slot| slot.slot_name == "slot")
        .expect("slot")
}

fn default_skin_display(
    factory: &BaseFactory,
    slot_name: &str,
    index: usize,
) -> Option<dragonbones_core::model::DisplayData> {
    factory
        .get_armature_data("features_fixture", "features")
        .and_then(|armature| {
            armature
                .default_skin
                .as_deref()
                .and_then(|name| armature.get_skin(name))
        })
        .and_then(|skin| skin.get_displays(slot_name))
        .and_then(|displays| displays.get(index).cloned().flatten())
}

/// `replace_raw_display_data`：负索引解析、defaultSkin 回填（Slot.ts:825-833）、
/// 越界静默返回。
#[test]
fn replace_raw_display_data_negative_fallback_and_out_of_range() {
    let (mut factory, armature) = build_features();
    let display_index = {
        let slot = slot_mut(&mut factory, armature);
        assert!(slot.display_index >= 0);
        slot.display_index as usize
    };
    let default_display =
        default_skin_display(&factory, "slot", display_index).expect("defaultSkin display");
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

    // 先改掉当前帧，确认回填确实生效。
    slot_mut(&mut factory, armature).replace_raw_display_data(Some(img2.clone()), -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .raw_display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some("img2")
    );

    // 传 null + 负索引 → 从 defaultSkin 的 [displayIndex] 回填（Slot.ts:826-831）。
    slot_mut(&mut factory, armature).replace_raw_display_data(None, -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .raw_display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some(default_display.name.as_str()),
        "replaceRawDisplayData(null) 应按 displayIndex 回填 defaultSkin"
    );

    // 非负越界 → 静默返回，不改任何帧（Slot.ts:817-819）。
    let frame_count = slot_mut(&mut factory, armature).display_frames.len();
    slot_mut(&mut factory, armature).replace_raw_display_data(Some(img2), 99);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames.len(),
        frame_count
    );
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .raw_display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some(default_display.name.as_str())
    );

    // 负索引解析到 displayIndex（不是 0）：切到帧 1 后写 -1。
    slot_mut(&mut factory, armature).set_display_index(1, true);
    let img = factory
        .get_armature_data("features_fixture", "features")
        .and_then(|armature| {
            armature
                .default_skin
                .as_deref()
                .and_then(|name| armature.get_skin(name))
        })
        .and_then(|skin| skin.get_display("slot", "img"))
        .cloned()
        .expect("img");
    slot_mut(&mut factory, armature).replace_raw_display_data(Some(img), -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[1]
            .raw_display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some("img"),
        "负索引应解析为当前 displayIndex（=1）"
    );
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[0]
            .raw_display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some(default_display.name.as_str()),
        "负索引不得写 frame 0"
    );
}

/// `replace_texture_data` / `replace_display_data` / `replace_display` 的负索引
/// 与越界归一（同一 helper，Slot.ts 四个 replace* 同构）。
#[test]
fn other_replace_negative_and_out_of_range_indices() {
    let (mut factory, armature) = build_features();
    let display_index = {
        let slot = slot_mut(&mut factory, armature);
        assert!(slot.display_index >= 0);
        slot.display_index as usize
    };

    // ---- replace_texture_data ----
    // 该夹具的图集无贴图（build 后 textureData 为 null）→ 合成一个 TextureData 覆盖。
    let texture = dragonbones_core::model::TextureData {
        rotated: false,
        name: "replacement".into(),
        region: dragonbones_core::geom::rectangle::Rectangle::new(0.0, 0.0, 10.0, 10.0),
        parent: String::new(),
        frame: None,
    };
    slot_mut(&mut factory, armature).replace_texture_data(Some(texture.clone()), -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .texture_data
            .as_ref()
            .map(|texture| texture.name.as_str()),
        Some("replacement")
    );
    slot_mut(&mut factory, armature).replace_texture_data(None, 99);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .texture_data
            .as_ref()
            .map(|texture| texture.name.as_str()),
        Some("replacement"),
        "越界 index 应静默返回"
    );

    // ---- replace_display_data ----
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
    slot_mut(&mut factory, armature).replace_display_data(Some(img2), -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .display_data
            .as_ref()
            .map(|display| display.name.as_str()),
        Some("img2")
    );
    slot_mut(&mut factory, armature).replace_display_data(None, 99);
    assert!(
        slot_mut(&mut factory, armature).display_frames[display_index]
            .display_data
            .is_some(),
        "越界 index 应静默返回"
    );

    // ---- replace_display ----
    let raw_display = slot_mut(&mut factory, armature).raw_display;
    slot_mut(&mut factory, armature).replace_display(None, -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index].display,
        None
    );
    slot_mut(&mut factory, armature).replace_display(Some(raw_display), 99);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index].display,
        None,
        "越界 index 应静默返回"
    );
    slot_mut(&mut factory, armature).replace_display(Some(raw_display), -1);
    assert_eq!(
        slot_mut(&mut factory, armature).display_frames[display_index].display,
        Some(raw_display)
    );
}
