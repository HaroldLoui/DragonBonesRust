//! 阶段 6：cacheFrameRate（P1-4）与 JS 参考对拍。

mod common;

use common::read_value;
use dragonbones_core::armature::{ArmatureIdx, DisplayHandle, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::transform::Transform;
use dragonbones_core::model::{ArmatureData, DisplayData, DisplayKind};
use std::path::Path;
use std::sync::Arc;

#[test]
fn dragon_walk_cache_rate_matches_js() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let ske = read_value(&root.join("assets/Dragon_ske.json"));
    let tex = read_value(&root.join("assets/Dragon_tex.json"));
    let golden = read_value(&root.join("testdata/golden_cache/dragon_walk.json"));

    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "Dragon").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "Dragon"));
    let armature = factory
        .build_armature(
            "Dragon",
            "Dragon",
            "",
            "Dragon",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build");
    factory
        .attach_armature(factory.root_clock(), armature)
        .expect("attach");
    factory.set_armature_cache_frame_rate(armature, 24.0);
    assert_eq!(factory.armature(armature).cache_frame_rate(), 24.0);
    factory
        .armature_mut(armature)
        .animation
        .goto_and_play_by_time("walk", 0.0, -1)
        .expect("play");

    for (frame_index, expected) in golden["frames"]
        .as_array()
        .expect("frames")
        .iter()
        .enumerate()
    {
        let arm = factory.armature(armature);
        for (name, want) in expected["bones"].as_object().expect("bones") {
            let idx = arm
                .get_bone(name)
                .unwrap_or_else(|| panic!("missing bone {name}"));
            let bone = arm
                .get_bones()
                .iter()
                .find(|bone| bone.idx == idx)
                .expect("bone");
            let g = &bone.global;
            let m = &bone.global_transform_matrix;
            let got = [
                g.x, g.y, g.rotation, g.scale_x, g.scale_y, g.skew, m.a, m.b, m.c, m.d, m.tx, m.ty,
            ];
            for (index, value) in want.as_array().expect("vector").iter().enumerate() {
                let Some(want) = value.as_f64() else { continue };
                let got = got[index];
                assert!(
                    (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                    "frame {frame_index} bone {name}[{index}]: want={want} got={got}"
                );
            }
        }
        factory.advance_time(1.0 / 24.0);
    }
}

#[test]
fn cache_rate_is_inherited_by_later_build() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let ske = read_value(&root.join("assets/Dragon_ske.json"));
    let tex = read_value(&root.join("assets/Dragon_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "Dragon").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "Dragon"));
    let first = factory
        .build_armature(
            "Dragon",
            "Dragon",
            "",
            "Dragon",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("first build");
    factory.set_armature_cache_frame_rate(first, 24.0);
    let second = factory
        .build_armature(
            "Dragon",
            "Dragon",
            "",
            "Dragon",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("second build");
    assert_eq!(factory.armature(second).cache_frame_rate(), 24.0);
    assert!(!factory.armature(second).frame_cache.animations.is_empty());
}

// ---------------------------------------------------------------------------
// D-6.2 二次设置语义（Armature.ts:768-779 + ArmatureData.ts:269-278）
// ---------------------------------------------------------------------------

const CHILD_ARMATURE: &str = "ChildA";

/// 只 parse（不 build）的 Dragon 工厂：`Arc` 独占于 `data_map`，供子骨架注入。
fn dragon_factory() -> BaseFactory {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let ske = read_value(&root.join("assets/Dragon_ske.json"));
    let tex = read_value(&root.join("assets/Dragon_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "Dragon").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "Dragon"));
    factory
}

fn build_dragon(factory: &mut BaseFactory) -> ArmatureIdx {
    factory
        .build_armature(
            "Dragon",
            "Dragon",
            "",
            "Dragon",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build Dragon")
}

fn build_dragon_with_child(factory: &mut BaseFactory) -> (ArmatureIdx, ArmatureIdx) {
    inject_child_armature(factory);
    let parent = build_dragon(factory);
    let child = factory
        .armature(parent)
        .slots
        .iter()
        .find_map(|slot| match slot.display {
            Some(DisplayHandle::Armature(child)) => Some(child),
            _ => None,
        })
        .expect("父槽 display 应接线到子骨架");
    (parent, child)
}

/// 往 Dragon 的 default skin 注入一个最小子骨架显示（同 `armature_unit.rs` 的手法）：
/// 子骨架 `ChildA` 0 骨 0 槽 0 动画，用于验证 rate 递归。
fn inject_child_armature(factory: &mut BaseFactory) {
    let arc = factory
        .data_map
        .get_mut("Dragon")
        .expect("data_map[Dragon]");
    let data = Arc::get_mut(arc).expect("DragonBonesData 的 Arc 应独占（尚未 build）");
    data.add_armature(ArmatureData {
        name: CHILD_ARMATURE.into(),
        frame_rate: 24.0,
        ..Default::default()
    });

    let armature_data = data.armatures.get_mut("Dragon").expect("Dragon armature");
    let skin_name = armature_data
        .default_skin
        .clone()
        .expect("Dragon 无 default_skin");
    let candidates: Vec<(String, i32)> = armature_data
        .sorted_slots
        .iter()
        .filter_map(|name| {
            armature_data
                .slots
                .get(name)
                .map(|slot_data| (name.clone(), slot_data.display_index))
        })
        .filter(|(_, display_index)| *display_index >= 0)
        .collect();
    let skin = armature_data.skins.get_mut(&skin_name).expect("skin");
    let (slot_name, display_index) = candidates
        .into_iter()
        .find(|(name, display_index)| {
            skin.displays
                .get(name)
                .is_some_and(|list| usize::try_from(*display_index).is_ok_and(|i| i < list.len()))
        })
        .expect("找不到可注入子骨架显示的槽");
    let frame_index = usize::try_from(display_index).expect("display_index >= 0");
    skin.displays.get_mut(&slot_name).expect("slot displays")[frame_index] = Some(DisplayData {
        name: CHILD_ARMATURE.into(),
        path: CHILD_ARMATURE.into(),
        transform: Transform::default(),
        parent: skin_name,
        kind: DisplayKind::Armature {
            inherit_animation: true,
            actions: Vec::new(),
            armature: Some(CHILD_ARMATURE.into()),
        },
    });
}

fn table_rate(armature: &dragonbones_core::armature::Armature) -> f64 {
    armature
        .frame_cache
        .animations
        .values()
        .next()
        .expect("cache table should be initialized")
        .rate
}

/// 24 → 30：TS `ArmatureData.cacheFrames` 在 `cacheFrameRate > 0` 时早退且
/// **不赋值**（ArmatureData.ts:270-272）→ getter 与帧表都必须保持 24（一致）。
#[test]
fn cache_rate_second_set_keeps_getter_and_table_in_sync() {
    let mut factory = dragon_factory();
    let armature = build_dragon(&mut factory);

    factory.set_armature_cache_frame_rate(armature, 24.0);
    let first_table_rate = table_rate(factory.armature(armature));
    let first_animation_count = factory.armature(armature).frame_cache.animations.len();
    assert_eq!(factory.armature(armature).cache_frame_rate(), 24.0);
    assert_eq!(first_table_rate, 24.0);
    assert!(first_animation_count > 0);

    factory.set_armature_cache_frame_rate(armature, 30.0);
    assert_eq!(
        factory.armature(armature).cache_frame_rate(),
        24.0,
        "第二次设置 30 不得改写 getter（ArmatureData.ts:270-272 早退不赋值）"
    );
    assert_eq!(
        table_rate(factory.armature(armature)),
        first_table_rate,
        "第二次设置 30 不得重建/改写帧表"
    );
    assert_eq!(
        factory.armature(armature).frame_cache.animations.len(),
        first_animation_count,
        "frame_cache 不因二次设置重复插入"
    );

    // D-6.2 rate 表记录实际生效值 → 之后 build 的同类骨架继承 24（不是请求的 30）。
    let second = build_dragon(&mut factory);
    assert_eq!(factory.armature(second).cache_frame_rate(), 24.0);
    assert_eq!(table_rate(factory.armature(second)), first_table_rate);
}

/// Armature.ts:773-777：setter 进入外层分支后向所有 `childArmature` 递归下发
/// 同一个 value（Slot.child_armature → arena 索引）。
#[test]
fn cache_rate_setter_recurses_into_child_armature() {
    let mut factory = dragon_factory();
    let (parent, child) = build_dragon_with_child(&mut factory);
    assert_eq!(
        factory.armature(child).cache_frame_rate(),
        0.0,
        "build 收尾父 rate 仍为 0 → 子骨架尚未继承"
    );

    factory.set_armature_cache_frame_rate(parent, 24.0);
    assert_eq!(factory.armature(parent).cache_frame_rate(), 24.0);
    assert_eq!(
        factory.armature(child).cache_frame_rate(),
        24.0,
        "setter 必须递归写到 child armature（Armature.ts:773-777）"
    );

    // 二次设置：父/子都已 >0 → 均早退不赋值（ArmatureData.ts:270-272）。
    factory.set_armature_cache_frame_rate(parent, 30.0);
    assert_eq!(factory.armature(parent).cache_frame_rate(), 24.0);
    assert_eq!(factory.armature(child).cache_frame_rate(), 24.0);

    // 新 build 的父骨架按实际生效值 24 继承。
    let rebuilt = build_dragon(&mut factory);
    assert_eq!(factory.armature(rebuilt).cache_frame_rate(), 24.0);
}
