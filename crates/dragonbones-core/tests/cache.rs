//! 阶段 6：cacheFrameRate（P1-4）与 JS 参考对拍。

mod common;

use common::read_value;
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

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
        .build_armature("Dragon", "Dragon", "", "Dragon", Box::new(NullArmatureProxy::new()))
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

    for (frame_index, expected) in golden["frames"].as_array().expect("frames").iter().enumerate() {
        let arm = factory.armature(armature);
        for (name, want) in expected["bones"].as_object().expect("bones") {
            let idx = arm.get_bone(name).unwrap_or_else(|| panic!("missing bone {name}"));
            let bone = arm.get_bones().iter().find(|bone| bone.idx == idx).expect("bone");
            let g = &bone.global;
            let m = &bone.global_transform_matrix;
            let got = [g.x, g.y, g.rotation, g.scale_x, g.scale_y, g.skew, m.a, m.b, m.c, m.d, m.tx, m.ty];
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
        .build_armature("Dragon", "Dragon", "", "Dragon", Box::new(NullArmatureProxy::new()))
        .expect("first build");
    factory.set_armature_cache_frame_rate(first, 24.0);
    let second = factory
        .build_armature("Dragon", "Dragon", "", "Dragon", Box::new(NullArmatureProxy::new()))
        .expect("second build");
    assert_eq!(factory.armature(second).cache_frame_rate(), 24.0);
    assert!(!factory.armature(second).frame_cache.animations.is_empty());
}
