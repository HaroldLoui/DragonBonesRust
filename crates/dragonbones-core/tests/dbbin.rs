//! 阶段 6：dbbin 读取与 JSON 路径的等价性。
//!
//! 两个官方样本同时提供 `_ske.json` 与 `_ske.dbbin`；JSON 路径已由阶段 2 的
//! IR golden 对 JS 验证，因此 binary 解析只要与 JSON 解析逐元素相等即证明
//! 头/数组/时间轴/几何分支正确。

mod common;

use common::ir_snapshot::{read_json, tex_stem};
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::model::IrArrays;

use std::path::PathBuf;

const ASSETS: [(&str, &str); 2] = [
    ("mecha_1406", "mecha_1406_ske"),
    ("mecha_1002_101d_show", "mecha_1002_101d_show_ske"),
];

fn dbbin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/dbbin")
}

fn load_pair(dir: &str, stem: &str) -> (BaseFactory, BaseFactory, String) {
    let root = dbbin_root().join(dir);
    let ske = read_json(&root.join(format!("{stem}.json")));
    let tex = read_json(&root.join(format!("{}.json", tex_stem(stem))));
    let bytes = std::fs::read(root.join(format!("{stem}.dbbin"))).expect("read dbbin");
    let armature = ske["armature"][0]["name"].as_str().expect("armature").to_owned();

    let mut json = BaseFactory::new();
    assert!(json.parse_dragon_bones_data(&ske, stem).is_some());
    assert!(json.parse_texture_atlas_data(&tex, stem));
    let mut binary = BaseFactory::new();
    assert!(binary.parse_dragon_bones_data_binary(&bytes, stem).is_some());
    assert!(binary.parse_texture_atlas_data(&tex, stem));
    (json, binary, armature)
}

fn ir_of(factory: &BaseFactory, stem: &str) -> IrArrays {
    factory
        .data_map
        .get(stem)
        .expect("data")
        .root
        .as_ref()
        .expect("root")
        .ir
        .clone()
}

#[test]
fn dbbin_ir_matches_json_ir_for_both_samples() {
    for (dir, stem) in ASSETS {
        let (json, binary, _) = load_pair(dir, stem);
        let json_ir = ir_of(&json, stem);
        let binary_ir = ir_of(&binary, stem);
        assert_eq!(json_ir.int_array, binary_ir.int_array, "{dir}: int");
        assert_eq!(json_ir.float_array, binary_ir.float_array, "{dir}: float");
        assert_eq!(
            json_ir.frame_int_array, binary_ir.frame_int_array,
            "{dir}: frameInt"
        );
        assert_eq!(
            json_ir.frame_float_array.len(),
            binary_ir.frame_float_array.len(),
            "{dir}: frameFloat length"
        );
        for (index, (want, got)) in json_ir
            .frame_float_array
            .iter()
            .zip(&binary_ir.frame_float_array)
            .enumerate()
        {
            assert!(
                want.to_bits() == got.to_bits() || (want - got).abs() <= 1e-12,
                "{dir}: frameFloat[{index}] want={want} got={got}"
            );
        }
        assert_eq!(json_ir.frame_array, binary_ir.frame_array, "{dir}: frame");
        assert_eq!(
            json_ir.timeline_array, binary_ir.timeline_array,
            "{dir}: timeline"
        );
        assert_eq!(json_ir.color_array, binary_ir.color_array, "{dir}: color");
        assert_eq!(
            json_ir.frame_indices, binary_ir.frame_indices,
            "{dir}: frameIndices"
        );
    }
}

#[test]
fn dbbin_build_and_advance_match_json_pose() {
    for (dir, stem) in ASSETS {
        let (mut json, mut binary, armature) = load_pair(dir, stem);
        let anims: Vec<String> = json
            .get_armature_data(&armature, stem)
            .expect("armature data")
            .animations
            .keys()
            .cloned()
            .collect();
        assert!(!anims.is_empty(), "{dir}: animations");
        for anim in anims {
            let json_idx = build_json(&mut json, &armature, stem);
            let binary_idx = build_json(&mut binary, &armature, stem);
            json.armature_mut(json_idx)
                .animation
                .goto_and_play_by_time(&anim, 0.0, -1)
                .expect("json play");
            binary
                .armature_mut(binary_idx)
                .animation
                .goto_and_play_by_time(&anim, 0.0, -1)
                .expect("binary play");
            for frame in 0..120 {
                let dt = 1.0 / 24.0;
                json.advance_time(dt);
                binary.advance_time(dt);
                compare_pose(&format!("{dir}/{anim} frame {frame}"), &json, json_idx, &binary, binary_idx);
            }
        }
    }
}

fn build_json(factory: &mut BaseFactory, armature: &str, stem: &str) -> dragonbones_core::armature::ArmatureIdx {
    let idx = factory
        .build_armature(armature, stem, "", stem, Box::new(NullArmatureProxy::new()))
        .expect("build armature");
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("attach");
    idx
}

fn compare_pose(
    context: &str,
    a: &BaseFactory,
    a_idx: dragonbones_core::armature::ArmatureIdx,
    b: &BaseFactory,
    b_idx: dragonbones_core::armature::ArmatureIdx,
) {
    let a_armature = a.armature(a_idx);
    let b_armature = b.armature(b_idx);
    assert_eq!(a_armature.bones.len(), b_armature.bones.len());
    for (a_bone, b_bone) in a_armature.bones.iter().zip(&b_armature.bones) {
        assert_eq!(a_bone.bone_name, b_bone.bone_name);
        let value = |bone: &dragonbones_core::armature::Bone| {
            let g = &bone.global;
            let m = &bone.global_transform_matrix;
            [
                g.x, g.y, g.rotation, g.scale_x, g.scale_y, g.skew, m.a, m.b, m.c, m.d, m.tx, m.ty,
                bone.global_alpha,
            ]
        };
        let a_values = value(a_bone);
        let b_values = value(b_bone);
        for (index, (want, got)) in a_values.iter().zip(b_values).enumerate() {
            assert!(
                (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                "{context} bone `{}`[{index}]: want={want} got={got}",
                a_bone.bone_name
            );
        }
    }
    for (a_slot, b_slot) in a_armature.slots.iter().zip(&b_armature.slots) {
        assert_eq!(a_slot.slot_name, b_slot.slot_name);
        assert_eq!(a_slot.display_index, b_slot.display_index);
        assert!((a_slot.z_index - b_slot.z_index).abs() <= 1e-12);
    }
}

#[test]
fn dbbin_header_rejects_invalid_input() {
    let mut parser = dragonbones_core::parser::ObjectDataParser::new();
    assert!(parser.parse_binary_dragon_bones_data(b"", 1.0).is_none());
    assert!(
        parser
            .parse_binary_dragon_bones_data(b"NOT_DBDT_DATA", 1.0)
            .is_none()
    );
    let mut truncated = b"DBDT".to_vec();
    truncated.extend_from_slice(&[200, 0, 0, 0]);
    assert!(parser.parse_binary_dragon_bones_data(&truncated, 1.0).is_none());
}
