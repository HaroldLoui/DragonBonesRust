//! Runtime seek semantics: change the playhead without restarting the state.

use std::path::Path;

use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn build_dragon() -> (BaseFactory, ArmatureIdx) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/Dragon");
    let skeleton = read_json(&root.join("Dragon_ske.json"));
    let atlas = read_json(&root.join("Dragon_tex.json"));
    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton, 1.0)
        .expect("parse Dragon skeleton");
    let mut atlas_data = TextureAtlasData::default();
    assert!(parser.parse_texture_atlas_data(&atlas, &mut atlas_data, 1.0));
    let mut factory = BaseFactory::new();
    factory.add_dragon_bones_data(data, Some("Dragon"));
    factory.add_texture_atlas_data(atlas_data, Some("Dragon"));

    let idx = factory
        .build_armature(
            "Dragon",
            "Dragon",
            "",
            "Dragon",
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build Dragon");
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("attach");
    (factory, idx)
}

#[test]
fn seek_changes_playhead_and_pose_without_restarting() {
    let (mut factory, idx) = build_dragon();
    factory
        .armature_mut(idx)
        .animation
        .play(Some("walk"), -1)
        .expect("walk state");
    factory.advance_one(idx, 0.0);
    factory.advance_one(idx, 0.3);

    let duration = factory
        .armature(idx)
        .animation
        .get_state("walk", -1)
        .and_then(|id| factory.armature(idx).animation.state(id))
        .expect("walk state")
        .total_time();
    assert!(
        duration > 0.1,
        "Dragon walk should have a positive duration"
    );

    let before_time = factory
        .armature(idx)
        .animation
        .get_state("walk", -1)
        .and_then(|id| factory.armature(idx).animation.state(id))
        .expect("walk state")
        .current_time();
    let target = if before_time < duration * 0.5 {
        duration * 0.9
    } else {
        duration * 0.1
    };
    let before = factory.armature(idx).bones[1].base.global_transform_matrix;
    let id = factory
        .armature_mut(idx)
        .animation
        .seek("walk", -1, target)
        .expect("seek");
    factory.advance_one(idx, 0.0);

    let state = factory.armature(idx).animation.state(id).expect("state");
    assert!(
        (state.current_time() - target).abs() < 1e-6,
        "seek target: want {target}, got {}",
        state.current_time()
    );
    assert!(
        factory.armature(idx).bones[1].base.global_transform_matrix != before,
        "seek must re-evaluate the pose at the target time"
    );
    assert!(
        factory
            .armature(idx)
            .animation
            .get_state("walk", -1)
            .is_some_and(|id| factory
                .armature(idx)
                .animation
                .state(id)
                .is_some_and(|s| s.is_playing())),
        "seek must not stop the state"
    );
}

#[test]
fn seek_backwards_and_while_paused_keeps_the_pause() {
    let (mut factory, idx) = build_dragon();
    factory
        .armature_mut(idx)
        .animation
        .play(Some("walk"), -1)
        .expect("walk state");
    factory.advance_one(idx, 0.0);
    factory.advance_one(idx, 1.0);

    let id = factory
        .armature_mut(idx)
        .animation
        .seek("walk", -1, 0.2)
        .expect("seek backwards");
    factory.advance_one(idx, 0.0);
    let state = factory.armature(idx).animation.state(id).expect("state");
    assert!((state.current_time() - 0.2).abs() < 1e-6);

    factory
        .armature_mut(idx)
        .animation
        .state_mut(id)
        .expect("state")
        .time_scale = 0.0;
    factory
        .armature_mut(idx)
        .animation
        .seek("walk", -1, 0.4)
        .expect("seek while paused");
    factory.advance_one(idx, 0.5);
    let state = factory.armature(idx).animation.state(id).expect("state");
    assert!(
        (state.current_time() - 0.4).abs() < 1e-6,
        "paused seek must not advance: got {}",
        state.current_time()
    );
    assert!(state.is_playing(), "paused via time_scale, not playhead");
}
