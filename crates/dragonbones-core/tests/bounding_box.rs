//! 阶段 6：BoundingBox 三种子类 + Slot/Armature 命中检测（P1-3）。
//!
//! golden 由 `tools/bbox_dump.mjs` 用 JS 参考实现生成：网格点 containsPoint、
//! 8 条线段的 intersectsSegment（含 A/B/normal 出参）。

mod common;

use common::read_value;
use dragonbones_core::armature::NullArmatureProxy;
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

        for result in expected["segmentResults"].as_array().expect("segmentResults") {
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
