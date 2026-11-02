//! 阶段 6：BoundingBox 三种子类 + Slot/Armature 命中检测（P1-3）。
//!
//! golden 由 `tools/bbox_dump.mjs` 用 JS 参考实现生成：网格点 containsPoint、
//! 8 条线段的 intersectsSegment（含 A/B/normal 出参）。

mod common;

use common::read_value;
use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::Point;
use serde_json::Value;
use std::path::Path;

fn approx(want: Option<f64>, got: f64, context: &str) {
    let want = want.unwrap_or(f64::NAN);
    if want.is_nan() {
        assert!(got.is_nan(), "{context}: expected NaN, got {got}");
        return;
    }
    assert!(
        (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
        "{context}: want={want} got={got}"
    );
}

#[test]
fn bounding_box_hit_tests_match_js_golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let ske = read_value(&root.join("synthetic/bbox/bbox_ske.json"));
    let tex = read_value(&root.join("synthetic/synthetic_tex.json"));
    let golden = read_value(&root.join("golden_bbox/bbox.json"));

    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "bbox").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "bbox"));
    let armature_idx = factory
        .build_armature(
            "bbox_fixture",
            "bbox",
            "",
            "bbox",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build");
    factory
        .attach_armature(factory.root_clock(), armature_idx)
        .expect("attach");
    factory.advance_time(0.0);

    for expected in golden["points"].as_array().expect("points") {
        let point = expected["point"].as_array().expect("point x/y");
        let x = point[0].as_f64().unwrap();
        let y = point[1].as_f64().unwrap();
        let hit = expected["hit"].as_str();
        let got = factory.armature_mut(armature_idx).contains_point(x, y);
        assert_eq!(
            got,
            hit.is_some(),
            "armature.containsPoint({x},{y}) want={hit:?}"
        );
    }

    for (slot_name, expected) in golden["slots"].as_object().expect("slots") {
        let armature = factory.armature_mut(armature_idx);
        let slot_idx = armature.get_slot(slot_name).expect("slot idx");
        let parent_idx = armature
            .slots
            .iter()
            .find(|slot| slot.idx == slot_idx)
            .expect("slot")
            .parent;
        let point_results = expected["pointResults"].as_array().expect("pointResults");
        for result in point_results {
            let point = result["point"].as_array().expect("point");
            let x = point[0].as_f64().unwrap();
            let y = point[1].as_f64().unwrap();
            let want = result["hit"].as_bool().unwrap();
            let slot = armature
                .slots
                .iter_mut()
                .find(|slot| slot.idx == slot_idx)
                .expect("slot");
            let got = slot.contains_point(
                x,
                y,
                armature
                    .bones
                    .iter()
                    .find(|bone| bone.idx == parent_idx)
                    .expect("parent"),
            );
            assert_eq!(got, want, "{slot_name}.containsPoint({x},{y})");
        }

        for result in expected["segmentResults"]
            .as_array()
            .expect("segmentResults")
        {
            let segment = result["segment"].as_array().expect("segment");
            let vals: Vec<f64> = segment.iter().map(|v| v.as_f64().unwrap()).collect();
            let want_count = result["count"].as_i64().unwrap() as i32;
            let mut a = Point::default();
            let mut b = Point::default();
            let mut n = Point::default();
            let (got_count, got_a, got_b, got_n) = {
                let armature = factory.armature_mut(armature_idx);
                let slot = armature
                    .slots
                    .iter_mut()
                    .find(|slot| slot.idx == slot_idx)
                    .expect("slot");
                let parent = armature
                    .bones
                    .iter()
                    .find(|bone| bone.idx == parent_idx)
                    .expect("parent");
                let count = slot
                    .intersects_segment(
                        vals[0],
                        vals[1],
                        vals[2],
                        vals[3],
                        parent,
                        Some(&mut a),
                        Some(&mut b),
                        Some(&mut n),
                    )
                    .unwrap_or(0);
                (count, [a.x, a.y], [b.x, b.y], [n.x, n.y])
            };
            let context = format!("{slot_name}.intersectsSegment({vals:?})");
            assert_eq!(got_count, want_count, "{context} count");
            for (index, got) in got_a.into_iter().enumerate() {
                approx(
                    result["a"].as_array().unwrap()[index].as_f64(),
                    got,
                    &format!("{context} a[{index}]"),
                );
            }
            for (index, got) in got_b.into_iter().enumerate() {
                approx(
                    result["b"].as_array().unwrap()[index].as_f64(),
                    got,
                    &format!("{context} b[{index}]"),
                );
            }
            for (index, got) in got_n.into_iter().enumerate() {
                approx(
                    result["normal"].as_array().unwrap()[index].as_f64(),
                    got,
                    &format!("{context} normal[{index}]"),
                );
            }
        }
    }
}

#[test]
fn rectangle_in_side_b_official_typo_is_preserved() {
    // 线段起点/终点都在矩形内 → -1；单端在内（inSideA）时官方把 B.y 写成 xB。
    // 这里直接验证底层数据，避免依赖 Slot 变换。
    let data = dragonbones_core::model::BoundingBoxData {
        color: 0,
        width: 100.0,
        height: 50.0,
        kind: dragonbones_core::model::BoundingBoxKind::Rectangle,
    };
    let mut a = Point::default();
    let mut b = Point::default();
    let mut n = Point::default();
    // A 在内部，B 在右侧外部；矩形 [-50,50]x[-25,25]。
    let count = data.intersects_segment(
        0.0,
        0.0,
        100.0,
        0.0,
        Some(&mut a),
        Some(&mut b),
        Some(&mut n),
    );
    assert_eq!(count, 2);
    assert_eq!((a.x, a.y), (50.0, 0.0));
    assert_eq!((b.x, b.y), (50.0, 50.0), "F6.11: B.y is xB, not yB");
}

#[allow(dead_code)]
fn _assert_golden_is_object(value: &Value) {
    assert!(value.is_object());
}

// ---------------------------------------------------------------------------
// `Armature.intersectsSegment`（Armature.ts:490-574）的槽选择语义：
// A 出参取**距起点最近**（竖直段用 y 差、否则 x 差）；B 出参取 x 差最大者——
// 只比 x（原文如此，与 A 不对称）。Rust 签名只返回 A 交点（B 为入参占位），
// 故测试以“独立重写的 TS 选择公式”为参照，并单独验证 A/B 选槽不同时返回 A。
// ---------------------------------------------------------------------------

fn build_bbox_fixture() -> (BaseFactory, ArmatureIdx) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let ske = read_value(&root.join("synthetic/bbox/bbox_ske.json"));
    let tex = read_value(&root.join("synthetic/synthetic_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "bbox").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "bbox"));
    let armature = factory
        .build_armature(
            "bbox_fixture",
            "bbox",
            "",
            "bbox",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build");
    factory
        .attach_armature(factory.root_clock(), armature)
        .expect("attach");
    factory.advance_time(0.0);
    (factory, armature)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TsSelection {
    a: Option<Point>,
    b: Option<Point>,
    a_slot: Option<usize>,
    b_slot: Option<usize>,
}

/// `Armature.ts:508-553` 的独立重写（A: 最近，竖直段比 y；B: 只比 x 最大）。
fn ts_reference_selection(
    factory: &mut BaseFactory,
    armature: ArmatureIdx,
    segment: [f64; 4],
) -> TsSelection {
    let is_v = segment[0] == segment[2];
    let mut a_slot = None;
    let mut b_slot = None;
    let mut a_point = Point::default();
    let mut b_point = Point::default();
    let mut d_min = 0.0;
    let mut d_max = 0.0;

    let arm = factory.armature_mut(armature);
    let bones = &arm.bones;
    for (slot_index, slot) in arm.slots.iter_mut().enumerate() {
        let Some(parent) = bones.iter().find(|bone| bone.idx == slot.parent) else {
            continue;
        };
        let mut point_a = Point::default();
        let mut point_b = Point::default();
        let count = slot
            .intersects_segment(
                segment[0],
                segment[1],
                segment[2],
                segment[3],
                parent,
                Some(&mut point_a),
                Some(&mut point_b),
                None,
            )
            .unwrap_or(0);
        if count <= 0 {
            continue;
        }
        // A：竖直段用 y 差，否则 x 差；最近者（:513-527）。
        let mut d = if is_v {
            point_a.y - segment[1]
        } else {
            point_a.x - segment[0]
        };
        if d < 0.0 {
            d = -d;
        }
        if a_slot.is_none() || d < d_min {
            d_min = d;
            a_point = point_a;
            a_slot = Some(slot_index);
        }
        // B：**只**比 x 差，最大者（:531-545，原文不对称）。
        let mut d = point_b.x - segment[0];
        if d < 0.0 {
            d = -d;
        }
        if b_slot.is_none() || d > d_max {
            d_max = d;
            b_point = point_b;
            b_slot = Some(slot_index);
        }
    }

    TsSelection {
        a: a_slot.map(|_| a_point),
        b: b_slot.map(|_| b_point),
        a_slot,
        b_slot,
    }
}

fn armature_intersection(
    factory: &mut BaseFactory,
    armature: ArmatureIdx,
    segment: [f64; 4],
) -> Option<Point> {
    factory
        .armature_mut(armature)
        .intersects_segment(
            segment[0], segment[1], segment[2], segment[3], 0.0, 0.0, 0.0, 0.0,
        )
        .map(|(x, y)| Point::new(x, y))
}

fn assert_point_eq(got: Option<Point>, want: Option<Point>, context: &str) {
    match (got, want) {
        (None, None) => {}
        (Some(got), Some(want)) => {
            assert!(
                (got.x - want.x).abs() <= 1e-9 && (got.y - want.y).abs() <= 1e-9,
                "{context}: got=({}, {}) want=({}, {})",
                got.x,
                got.y,
                want.x,
                want.y
            );
        }
        _ => panic!("{context}: got={got:?} want={want:?}"),
    }
}

/// 网格线段：`intersects_segment` 的返回值必须等于 TS A 规则选中的交点。
#[test]
fn armature_intersects_segment_matches_ts_slot_a_selection() {
    let (mut factory, armature) = build_bbox_fixture();
    let mut hit_count = 0usize;
    let mut miss_count = 0usize;
    for x in (-200..=200).step_by(40) {
        for y in (-200..=200).step_by(40) {
            let segments = [
                [f64::from(x), -200.0, f64::from(x), 200.0], // 竖直（isV）
                [-200.0, f64::from(y), 200.0, f64::from(y)], // 水平
                [f64::from(x), f64::from(y), 200.0, -200.0], // 斜线
            ];
            for segment in segments {
                let want = ts_reference_selection(&mut factory, armature, segment).a;
                let got = armature_intersection(&mut factory, armature, segment);
                assert_point_eq(got, want, &format!("segment {segment:?}"));
                if want.is_some() {
                    hit_count += 1;
                } else {
                    miss_count += 1;
                }
            }
        }
    }
    assert!(hit_count > 0, "网格必须至少命中一次");
    assert!(miss_count > 0, "网格必须至少未命中一次");
}

/// A/B 选槽不同时：返回 A（A 规则最近），而不是 B（x 差最大）。
#[test]
fn armature_returns_a_not_b_when_slot_selection_differs() {
    let (mut factory, armature) = build_bbox_fixture();
    let mut found = false;
    for x in (-200..=200).step_by(20) {
        for y0 in (-200..=200).step_by(20) {
            let segment = [f64::from(x), f64::from(y0), f64::from(x), 200.0];
            let selection = ts_reference_selection(&mut factory, armature, segment);
            if selection.a_slot.is_some()
                && selection.b_slot.is_some()
                && selection.a_slot != selection.b_slot
            {
                let got = armature_intersection(&mut factory, armature, segment);
                assert_eq!(
                    got, selection.a,
                    "A/B 选槽不同时应返回 A 交点（segment {segment:?}，a_slot={:?} b_slot={:?}）",
                    selection.a_slot, selection.b_slot
                );
                assert_ne!(
                    got, selection.b,
                    "不得返回 B 交点（B 只比 x 差，与 A 不对称）"
                );
                found = true;
            }
        }
    }
    assert!(found, "夹具必须存在 A/B 选槽不同的线段（否则测试无意义）");
}

/// 竖直段（`xA === xB`）的 A 选择用 **y 差**；找到 y 差与 x 差排序不一致的线段，
/// 断言返回 y 差最近者而不是 x 差最近者（Armature.ts:513-527 的 `isV` 分支）。
#[test]
fn vertical_segment_selects_slot_by_y_distance() {
    let (mut factory, armature) = build_bbox_fixture();
    let mut found = false;
    for x in (-200..=200).step_by(10) {
        let segment = [f64::from(x), -200.0, f64::from(x), 200.0];
        // 独立收集每个槽的 A 交点与 x 差。
        let mut x_best: Option<(f64, Point)> = None;
        let mut y_best: Option<(f64, Point)> = None;
        {
            let arm = factory.armature_mut(armature);
            let bones = &arm.bones;
            for slot in arm.slots.iter_mut() {
                let Some(parent) = bones.iter().find(|bone| bone.idx == slot.parent) else {
                    continue;
                };
                let mut point_a = Point::default();
                let mut point_b = Point::default();
                let count = slot
                    .intersects_segment(
                        segment[0],
                        segment[1],
                        segment[2],
                        segment[3],
                        parent,
                        Some(&mut point_a),
                        Some(&mut point_b),
                        None,
                    )
                    .unwrap_or(0);
                if count <= 0 {
                    continue;
                }
                let x_distance = (point_a.x - segment[0]).abs();
                let y_distance = (point_a.y - segment[1]).abs();
                if x_best.is_none_or(|(best, _)| x_distance < best) {
                    x_best = Some((x_distance, point_a));
                }
                if y_best.is_none_or(|(best, _)| y_distance < best) {
                    y_best = Some((y_distance, point_a));
                }
            }
        }
        let Some((_, y_point)) = y_best else { continue };
        let Some((_, x_point)) = x_best else { continue };
        if y_point != x_point {
            let got = armature_intersection(&mut factory, armature, segment);
            assert_eq!(
                got,
                Some(y_point),
                "竖直段应按 y 差选最近 A（segment {segment:?}）"
            );
            assert_ne!(got, Some(x_point), "竖直段不应按 x 差选");
            found = true;
        }
    }
    assert!(found, "夹具必须存在竖直段上 y/x 差排序不一致的情形");
}

/// 未命中任何槽 → `None`。
#[test]
fn armature_intersects_segment_miss_returns_none() {
    let (mut factory, armature) = build_bbox_fixture();
    for segment in [
        [-10_000.0, -10_000.0, -9_900.0, -9_900.0],
        [9_000.0, 9_000.0, 9_000.0, 9_100.0],
    ] {
        let want = ts_reference_selection(&mut factory, armature, segment);
        assert_eq!(want.a, None, "参照实现也应为未命中");
        assert_eq!(
            armature_intersection(&mut factory, armature, segment),
            None,
            "未命中应返回 None（segment {segment:?}）"
        );
    }
}
