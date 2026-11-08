//! 阶段 6（P1-5）：`Slot._updateDisplayData` 的图集缩放按**当前帧贴图**解析
//! （`Slot.ts:412` `_textureData.parent.scale`）。
//!
//! 合成场景：同一 atlas key 下挂两个 atlas——首个（build 级 `first()` 会取到）
//! 无纹理、scale = 1；第二个持有全部显示贴图、scale = 2。旧实现用 build 级单值
//! 取首个 atlas 的 scale（= 1）→ pivot 取错；逐帧登记后 pivot 必须用命中图集的 2。
//!
//! 同时覆盖外部 `replace_texture_data`（无图集归属）的清零回退语义。

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use common::read_value;
use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::Point;
use dragonbones_core::geom::rectangle::Rectangle;
use dragonbones_core::model::{DisplayKind, TextureAtlasData, TextureData};

const KEY: &str = "Dragon";
const BUILD_LEVEL_SCALE: f64 = 1.0;
const HIT_ATLAS_SCALE: f64 = 2.0;
const REGION_WIDTH: f64 = 10.0;
const REGION_HEIGHT: f64 = 20.0;
const PIVOT: Point = Point { x: 0.25, y: 0.75 };

fn texture(name: &str) -> TextureData {
    TextureData {
        rotated: false,
        name: name.into(),
        region: Rectangle::new(0.0, 0.0, REGION_WIDTH, REGION_HEIGHT),
        parent: String::new(),
        frame: None,
    }
}

/// 解析 Dragon 数据后注入：
/// - 一个带非零 pivot 的 Image 显示（默认 skin 的 `displayIndex` 帧）；
/// - 两个同名 atlas（首个无贴图 + scale=1，第二个持全部显示贴图 + scale=2）。
///
/// 返回 `(factory, 槽名, displayIndex, armature)`。
fn build_with_two_atlases() -> (BaseFactory, String, i32, ArmatureIdx) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let ske = read_value(&root.join("assets/Dragon/Dragon_ske.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, KEY).is_some());

    let (slot_name, display_index, texture_names) = {
        let arc = factory.data_map.get_mut(KEY).expect("data_map[Dragon]");
        let data = Arc::get_mut(arc).expect("DragonBonesData 的 Arc 应独占（尚未 build）");
        let armature_data = data.armatures.get_mut(KEY).expect("Dragon armature");
        let skin_name = armature_data
            .default_skin
            .clone()
            .expect("Dragon 无 default_skin");

        // ① 收集默认皮肤所有 Image/Mesh 显示的贴图名。
        let mut texture_names = BTreeSet::new();
        {
            let skin = armature_data.skins.get(&skin_name).expect("skin");
            for displays in skin.displays.values() {
                for display in displays.iter().flatten() {
                    if matches!(
                        display.kind,
                        DisplayKind::Image { .. } | DisplayKind::Mesh { .. }
                    ) {
                        texture_names.insert(display.path.clone());
                    }
                }
            }
        }

        // ② 选第一个“当前显示帧是 Image”的槽，注入非零 pivot。
        let sorted_slots = armature_data.sorted_slots.clone();
        let mut chosen = None;
        for name in sorted_slots {
            let Some(slot_data) = armature_data.slots.get(&name) else {
                continue;
            };
            let index = slot_data.display_index;
            if index < 0 {
                continue;
            }
            let skin = armature_data.skins.get_mut(&skin_name).expect("skin");
            let Some(displays) = skin.displays.get_mut(&name) else {
                continue;
            };
            let Some(Some(display)) = displays.get_mut(index as usize) else {
                continue;
            };
            if let DisplayKind::Image { pivot, .. } = &mut display.kind {
                *pivot = PIVOT;
                chosen = Some((name, index));
                break;
            }
        }
        let (slot_name, display_index) = chosen.expect("找不到可注入 pivot 的 Image 槽");
        (slot_name, display_index, texture_names)
    };

    // ③ 同名多 atlas：首个空壳（build 级 `first()` 取 1），第二个才是真实命中（2）。
    let empty_atlas = TextureAtlasData {
        name: KEY.into(),
        scale: BUILD_LEVEL_SCALE,
        ..TextureAtlasData::default()
    };
    factory.add_texture_atlas_data(empty_atlas, Some(KEY));

    let mut real_atlas = TextureAtlasData {
        name: KEY.into(),
        scale: HIT_ATLAS_SCALE,
        ..TextureAtlasData::default()
    };
    for name in texture_names {
        real_atlas.add_texture(texture(&name));
    }
    factory.add_texture_atlas_data(real_atlas, Some(KEY));

    let armature = factory
        .build_armature(KEY, KEY, "", KEY, Box::new(NullArmatureProxy::new()))
        .expect("build Dragon");
    factory.advance_one(armature, 0.0);
    (factory, slot_name, display_index, armature)
}

fn slot_pivot(factory: &BaseFactory, armature: ArmatureIdx, slot_name: &str) -> (f64, f64, f64) {
    let slot = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == slot_name)
        .expect("找不到目标槽");
    // 第三个值 = Slot 的 build 级回退 scale（帧级值在 display_frames 上）。
    (slot.pivot_x, slot.pivot_y, slot.texture_atlas_scale)
}

#[test]
fn pivot_uses_current_frame_texture_atlas_scale() {
    let (factory, slot_name, display_index, armature) = build_with_two_atlases();
    let armature_data_scale = factory.armature(armature).armature_data().scale;
    let slot = factory
        .armature(armature)
        .slots
        .iter()
        .find(|slot| slot.slot_name == slot_name)
        .expect("找不到目标槽");

    // 帧级登记：命中图集是第二个 atlas（scale = 2），不是 build 级 first() 的 1。
    assert_eq!(
        slot.display_frames[display_index as usize].texture_atlas_scale,
        Some(HIT_ATLAS_SCALE),
        "当前帧贴图应来自第二个 atlas（scale=2）"
    );
    assert_eq!(
        slot.texture_atlas_scale, BUILD_LEVEL_SCALE,
        "build 级回退值应是首个 atlas 的 scale（1）——用例必须能区分两者"
    );
    assert_eq!(slot.display_index, display_index);
    assert!(slot.texture_data.is_some(), "当前帧应有贴图");

    // `pivot = displayPivot * region * (atlas.scale * armature.scale)`（Slot.ts:427-428）。
    let expected_scale = HIT_ATLAS_SCALE * armature_data_scale;
    assert!(
        (slot.pivot_x - PIVOT.x * REGION_WIDTH * expected_scale).abs() <= 1e-12,
        "pivot_x: got={} want={}",
        slot.pivot_x,
        PIVOT.x * REGION_WIDTH * expected_scale
    );
    assert!(
        (slot.pivot_y - PIVOT.y * REGION_HEIGHT * expected_scale).abs() <= 1e-12,
        "pivot_y: got={} want={}",
        slot.pivot_y,
        PIVOT.y * REGION_HEIGHT * expected_scale
    );
}

/// 外部显式 `replace_texture_data`（不提供图集归属）→ 帧级 scale 清空，
/// pivot 回退 build 级单值（旧行为；TS 侧对应“外部直接换 TextureData”路径）。
#[test]
fn plain_replace_texture_data_falls_back_to_build_level_scale() {
    let (mut factory, slot_name, _display_index, armature) = build_with_two_atlases();
    let armature_data_scale = factory.armature(armature).armature_data().scale;

    {
        let slot = factory
            .armature_mut(armature)
            .slots
            .iter_mut()
            .find(|slot| slot.slot_name == slot_name)
            .expect("找不到目标槽");
        slot.replace_texture_data(Some(texture("replacement")), -1);
        assert_eq!(
            slot.display_frames[slot.display_index as usize].texture_atlas_scale, None,
            "外部 replace_texture_data 应清掉帧级图集归属"
        );
    }
    factory.advance_one(armature, 0.0);

    let (pivot_x, pivot_y, fallback_f64) = slot_pivot(&factory, armature, &slot_name);
    assert_eq!(fallback_f64, BUILD_LEVEL_SCALE, "slot 的 build 级 scale");
    let fallback_scale = BUILD_LEVEL_SCALE * armature_data_scale;
    assert!(
        (pivot_x - PIVOT.x * REGION_WIDTH * fallback_scale).abs() <= 1e-12,
        "pivot_x 应回退 build 级 scale: got={pivot_x}"
    );
    assert!(
        (pivot_y - PIVOT.y * REGION_HEIGHT * fallback_scale).abs() <= 1e-12,
        "pivot_y 应回退 build 级 scale: got={pivot_y}"
    );
}
