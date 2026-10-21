//! 阶段 6：P1-9 脏标记回归（lock/update、slots/zIndex、alpha 传播）。

mod common;

use common::read_value;
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

fn build() -> (BaseFactory, dragonbones_core::armature::ArmatureIdx) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    let ske = read_value(&root.join("assets/Dragon_ske.json"));
    let tex = read_value(&root.join("assets/Dragon_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "Dragon").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "Dragon"));
    let idx = factory
        .build_armature("Dragon", "Dragon", "", "Dragon", Box::new(NullArmatureProxy::new()))
        .expect("build");
    factory.attach_armature(factory.root_clock(), idx).expect("attach");
    (factory, idx)
}

#[test]
fn lock_update_blocks_advance_and_z_index_sorts_slots() {
    let (mut factory, idx) = build();
    factory.armature_mut(idx).lock_update = true;
    let before = factory.armature(idx).bones[0].global.x;
    factory.advance_time(0.5);
    assert_eq!(factory.armature(idx).bones[0].global.x, before);
    factory.armature_mut(idx).lock_update = false;

    let armature = factory.armature_mut(idx);
    let first = armature.slots[0].idx;
    let second = armature.slots[1].idx;
    armature
        .slots
        .iter_mut()
        .find(|slot| slot.idx == first)
        .unwrap()
        .z_index = 10.0;
    armature
        .slots
        .iter_mut()
        .find(|slot| slot.idx == second)
        .unwrap()
        .z_index = -10.0;
    armature.z_index_dirty = true;
    factory.advance_time(1.0 / 24.0);
    let armature = factory.armature(idx);
    let first_pos = armature.slots.iter().position(|slot| slot.idx == first).unwrap();
    let second_pos = armature.slots.iter().position(|slot| slot.idx == second).unwrap();
    assert!(second_pos < first_pos, "zIndex 小的槽应排在前面");
}

#[test]
fn alpha_dirty_propagates_to_slots() {
    let (mut factory, idx) = build();
    let armature = factory.armature_mut(idx);
    armature.alpha = 0.5;
    armature.alpha_dirty = true;
    let slot_idx = armature.slots[0].idx;
    factory.advance_time(1.0 / 24.0);
    let slot = factory
        .armature(idx)
        .slots
        .iter()
        .find(|slot| slot.idx == slot_idx)
        .unwrap();
    assert!((slot.global_alpha - 0.5 * slot.alpha).abs() < 1e-12);
}
