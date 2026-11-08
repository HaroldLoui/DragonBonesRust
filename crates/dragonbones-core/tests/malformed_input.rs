//! 畸形/极端输入的健壮性回归测试（方案 A 语料审计的产物）。
//!
//! 上游 536 套真实素材暴露出三类数据在官方运行时里“能跑但语义退化”的输入，
//! 而 Rust 移植在 debug 构建下会直接 panic：
//!
//! 1. `zOrder` 时间轴里槽位偏移为负/越界，导致 `_parseZOrderFrame` 尾部
//!    `unchangedIndex` 下溢（官方 TS 靠 JS 数组负下标返回 `undefined` 静默容忍）；
//! 2. 子骨架（`type: armature`）display 的 `path` 与图集贴图同名，使
//!    `Slot.updateDisplayData` 走进 `imageDisplayData` 条件回退分支（官方 TS
//!    对非 Image 显示取 `.pivot` 得 `undefined`，Rust 移植曾写成 `unreachable!`）；
//! 3. 极端数值（`1e308` 级缩放）不应让解析/推进崩溃。
//!
//! 这些用例不依赖 `_research/` 素材库，全部为内联合成 JSON。

use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use serde_json::{Value, json};

const DT: f64 = 1.0 / 60.0;

fn skeleton_with(animation: Value) -> Value {
    json!({
        "frameRate": 24,
        "name": "malformed",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "Main",
            "aabb": { "x": 0, "y": 0, "width": 100, "height": 100 },
            "bone": [{ "name": "root" }],
            "slot": [
                { "name": "s1", "parent": "root" },
                { "name": "s2", "parent": "root" }
            ],
            "skin": [{
                "slot": [
                    { "name": "s1", "display": [{ "type": "image", "name": "s1" }] },
                    { "name": "s2", "display": [{ "type": "image", "name": "s2" }] }
                ]
            }],
            "animation": [{
                "duration": 1,
                "name": "anim",
                "zOrder": { "frame": [{ "duration": 0, "zOrder": animation }] }
            }]
        }]
    })
}

fn texture_atlas(sub_textures: Value) -> Value {
    json!({
        "name": "malformed",
        "imagePath": "malformed_tex.png",
        "width": 64,
        "height": 64,
        "SubTexture": sub_textures
    })
}

/// 解析 + 构建 + 推进 `frames` 帧；返回工厂供调用方检查矩阵。
fn parse_build_advance(
    skeleton: &Value,
    atlas: Option<&Value>,
    frames: usize,
) -> (BaseFactory, ArmatureIdx) {
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(skeleton, "malformed")
        .expect("skeleton parses");
    if let Some(atlas) = atlas {
        assert!(
            factory.parse_texture_atlas_data(atlas, "malformed"),
            "atlas parses"
        );
    }
    let idx = factory
        .build_armature(
            "Main",
            "malformed",
            "",
            "malformed",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("armature builds");
    let _ = factory.attach_armature(factory.root_clock(), idx);
    factory
        .armature_mut(idx)
        .animation
        .goto_and_play_by_time("anim", 0.0, -1);
    for _ in 0..frames {
        factory.advance_time(DT);
    }
    (factory, idx)
}

fn assert_bones_finite(factory: &BaseFactory, idx: ArmatureIdx) {
    for bone in factory.armature(idx).get_bones() {
        let matrix = &bone.base.global_transform_matrix;
        for value in [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty] {
            assert!(
                value.is_finite(),
                "bone `{}` matrix has non-finite value: {matrix:?}",
                bone.bone_name
            );
        }
    }
}

/// `zOrder` 里 `slotIndex + offset` 越界：旧实现把负偏移饱和成 `usize` 后
/// 覆盖缺失，尾部 `unchangedIndex -= 1` 触发 attempt to subtract with overflow。
#[test]
fn z_order_out_of_range_offset_does_not_underflow() {
    let skeleton = skeleton_with(json!([0, 0, 5, -3]));
    let atlas = texture_atlas(json!([
        { "name": "s1", "x": 0, "y": 0, "width": 16, "height": 16 },
        { "name": "s2", "x": 16, "y": 0, "width": 16, "height": 16 }
    ]));
    let (factory, idx) = parse_build_advance(&skeleton, Some(&atlas), 5);
    assert_bones_finite(&factory, idx);
}

/// 子骨架 display 的 `path` 与图集贴图同名：旧实现在
/// `Slot::update_display_data` 的 `unreachable!("imageDisplayData 非 Image 类型")` 处 panic。
#[test]
fn armature_display_sharing_texture_name_does_not_panic() {
    let mut skeleton = skeleton_with(json!([0, 0, 1, 0]));
    let armature = &mut skeleton["armature"][0];
    // 把 s1 的显示换成子骨架显示，path/name 与图集里的 `s1` 贴图同名。
    armature["skin"][0]["slot"][0]["display"] = json!([{
        "type": "armature",
        "name": "s1",
        "path": "s1"
    }]);
    // 补一个同名子骨架，让 build 阶段能解析到 child armature。
    skeleton["armature"].as_array_mut().unwrap().push(json!({
        "type": "Armature",
        "frameRate": 24,
        "name": "s1",
        "aabb": { "x": 0, "y": 0, "width": 10, "height": 10 },
        "bone": [{ "name": "b" }],
        "skin": [{
            "slot": [{
                "name": "b-slot",
                "display": [{ "type": "image", "name": "s1" }]
            }]
        }]
    }));
    let atlas = texture_atlas(json!([
        { "name": "s1", "x": 0, "y": 0, "width": 16, "height": 16 }
    ]));
    let (factory, idx) = parse_build_advance(&skeleton, Some(&atlas), 5);
    assert_bones_finite(&factory, idx);
}

/// 顶层结构损坏（`armature` 不是数组 / 缺失）时解析应返回 `None`，不得 panic。
#[test]
fn malformed_toplevel_is_rejected_without_panic() {
    let mut parser = dragonbones_core::parser::ObjectDataParser::new();
    assert!(
        parser
            .parse_dragon_bones_data(&json!({ "armature": "not-an-array" }), 1.0)
            .is_none()
    );
    assert!(
        parser
            .parse_dragon_bones_data(&json!({ "name": "empty" }), 1.0)
            .is_none()
    );
}

/// 极端数值不应在解析/推进阶段崩溃（不承诺数值有限：数据本身越界）。
#[test]
fn extreme_numbers_do_not_panic() {
    let mut skeleton = skeleton_with(json!([0, 0, 1, 0]));
    skeleton["armature"][0]["animation"][0]["bone"] = json!([{
        "name": "root",
        "scaleFrame": [{
            "duration": 0,
            "x": 1.0e308,
            "y": 1.0e308
        }]
    }]);
    let _factory = parse_build_advance(&skeleton, None, 2);
}
