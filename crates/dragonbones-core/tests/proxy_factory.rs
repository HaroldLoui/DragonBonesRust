//! `BaseFactory::set_proxy_factory` must be used for child armatures.

mod common;

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use common::read_value;
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;

fn build_features_factory() -> BaseFactory {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/synthetic");
    let ske = read_value(&root.join("features/features_ske.json"));
    let tex = read_value(&root.join("synthetic_tex.json"));
    let mut factory = BaseFactory::new();
    assert!(factory.parse_dragon_bones_data(&ske, "features").is_some());
    assert!(factory.parse_texture_atlas_data(&tex, "features"));
    factory
}

#[test]
fn proxy_factory_is_used_for_child_armatures() {
    let mut factory = build_features_factory();
    let calls = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&calls);
    factory.set_proxy_factory(move || {
        counter.set(counter.get() + 1);
        Box::new(NullArmatureProxy::new())
    });

    let idx = factory
        .build_armature(
            "features_fixture",
            "features",
            "",
            "features",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build features fixture");
    assert!(
        calls.get() >= 1,
        "child armature build must call the installed proxy factory"
    );

    // The child is reachable from the parent slot and still has a live proxy.
    let parent = factory.armature(idx);
    let child = parent
        .slots
        .iter()
        .find_map(|slot| slot.child_armature)
        .expect("features fixture has a child armature");
    assert!(factory.armature(child).display != dragonbones_core::armature::DisplayHandle::Null);
}

#[test]
fn without_a_factory_child_armatures_still_build() {
    let mut factory = build_features_factory();
    let idx = factory
        .build_armature(
            "features_fixture",
            "features",
            "",
            "features",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build features fixture");
    assert!(
        factory
            .armature(idx)
            .slots
            .iter()
            .any(|slot| slot.child_armature.is_some()),
        "default NullArmatureProxy child construction must remain available"
    );
}
