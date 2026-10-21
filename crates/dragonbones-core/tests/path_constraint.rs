//! 阶段 6：PathConstraint 合成夹具与 JS 参考实现逐帧对拍（P1-2）。
//!
//! 夹具为 `testdata/synthetic/path/path_ske.json`；golden 由
//! `tools/path_dump.mjs` 生成三组 fixed 位置模式（length/tangent、
//! fixed/chain、fixed/chainScale）。

mod common;

use common::read_value;
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

# [test]
fn path_constraint_pose_matches_js_golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let base = read_value(&root.join("synthetic/path/path_ske.json"));
    let tex = read_value(&root.join("synthetic/synthetic_tex.json"));
    let cases = [
        ("path", "length", "tangent"),
        ("fixed_chain", "fixed", "chain"),
        ("fixed_chainscale", "fixed", "chainScale"),
    ];
    for (name, spacing, rotate) in cases {
        let mut ske = base.clone();
        let path = ske["armature"][0]["path"][0]
            .as_object_mut()
            .expect("path constraint");
        path.insert("positionMode".into(), "fixed".into());
        path.insert("spacingMode".into(), spacing.into());
        path.insert("rotateMode".into(), rotate.into());
        let golden = read_value(&root.join(format!("golden_path/{name}.json")));

        let mut factory = BaseFactory::new();
        assert!(factory.parse_dragon_bones_data(&ske, "path").is_some());
        assert!(factory.parse_texture_atlas_data(&tex, "path"));
        let armature = factory
            .build_armature(
                "path_fixture",
                "path",
                "",
                "path",
                Box::new(NullArmatureProxy::new()),
            )
            .expect("build path fixture");
        factory
            .attach_armature(factory.root_clock(), armature)
            .expect("attach");
        factory
            .armature_mut(armature)
            .animation
            .goto_and_play_by_time("idle", 0.0, -1)
            .expect("play idle");
        assert_eq!(
            factory
                .armature(armature)
                .constraints
                .iter()
                .filter(|constraint| constraint.path.is_some())
                .count(),
            1,
            "{name}: PathConstraint should be built"
        );

        let frames = golden["frames"].as_array().expect("frames");
        for (frame_index, expected) in frames.iter().enumerate() {
            let arm = factory.armature(armature);
            for (bone_name, want) in expected["bones"].as_object().expect("bones") {
                let idx = arm
                    .get_bone(bone_name)
                    .unwrap_or_else(|| panic!("{name}: missing bone {bone_name}"));
                let bone = arm
                    .get_bones()
                    .iter()
                    .find(|bone| bone.idx == idx)
                    .expect("bone");
                let g = &bone.global;
                let m = &bone.global_transform_matrix;
                let got = [
                    g.x, g.y, g.rotation, g.scale_x, g.scale_y, g.skew, m.a, m.b, m.c, m.d, m.tx,
                    m.ty, bone.global_alpha,
                ];
                for (index, value) in want.as_array().expect("vector").iter().enumerate() {
                    let Some(want) = value.as_f64() else {
                        continue;
                    };
                    let got = got[index];
                    assert!(
                        (want - got).abs() <= 1e-9 * (1.0 + want.abs()),
                        "{name} frame {frame_index} bone {bone_name}[{index}]: want={want} got={got}"
                    );
                }
            }
            if frame_index + 1 < frames.len() {
                factory.advance_time(1.0 / 24.0);
            }
        }
    }
}
