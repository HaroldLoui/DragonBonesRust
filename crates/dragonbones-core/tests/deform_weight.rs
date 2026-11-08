//! Deform (FFD) timelines on weighted meshes.
//!
//! The reference runtime's `_parseDeformFrame` only implements the unweighted
//! branch; the `weight !== null` branch is a bare `// TODO`
//! (`DragonBonesJS/DragonBones/src/dragonBones/parser/ObjectDataParser.ts:1993`).
//! Rust deliberately keeps the same no-op behaviour instead of inventing a
//! semantic the official runtime does not have.
//!
//! This test injects an FFD timeline targeting a **weighted** mesh slot of the
//! Rooster rig and asserts the parser accepts it while leaving the frame float
//! array untouched (i.e. the authoritative runtime would also ignore those
//! values).

use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;
use std::path::PathBuf;

fn rooster_skeleton() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/Rooster_Ani/Rooster_Ani_ske.json");
    let text = std::fs::read_to_string(path).expect("read Rooster_Ani_ske.json");
    serde_json::from_str(&text).expect("parse Rooster_Ani_ske.json")
}

/// Finds `(armature_index, slot_name, display_name)` of a weighted mesh display.
fn weighted_mesh_slot(skeleton: &Value) -> (usize, String, String) {
    for (armature_index, armature) in skeleton["armature"]
        .as_array()
        .expect("armatures")
        .iter()
        .enumerate()
    {
        for skin in armature["skin"].as_array().into_iter().flatten() {
            for slot in skin["slot"].as_array().into_iter().flatten() {
                let slot_name = slot["name"].as_str().expect("slot name").to_owned();
                for display in slot["display"].as_array().into_iter().flatten() {
                    if display["type"] == "mesh" && display.get("weights").is_some() {
                        let display_name = display["name"].as_str().unwrap_or("").to_owned();
                        return (armature_index, slot_name, display_name);
                    }
                }
            }
        }
    }
    panic!("Rooster rig has weighted mesh displays");
}

fn parse(skeleton: &Value) -> dragonbones_core::model::DragonBonesData {
    ObjectDataParser::default()
        .parse_dragon_bones_data(skeleton, 1.0)
        .expect("skeleton parses")
}

#[test]
fn ffd_timeline_on_a_weighted_mesh_is_accepted_and_ignored_like_the_runtime() {
    let mut skeleton = rooster_skeleton();
    let (armature_index, slot_name, display_name) = weighted_mesh_slot(&skeleton);

    let baseline = parse(&skeleton);

    // Inject an FFD timeline for that weighted mesh into the rig's first
    // animation. Official data has no such combination (weighted meshes have no
    // FFD timeline), so this is synthetic.
    let animation = &mut skeleton["armature"][armature_index]["animation"][0];
    animation["ffd"] = serde_json::json!([{
        "name": display_name,
        "slot": slot_name,
        "frame": [{ "duration": 0, "tweenEasing": 0, "vertices": [] }]
    }]);
    let injected = parse(&skeleton);

    let baseline_ir = baseline.root.as_ref().expect("root").ir.clone();
    let injected_ir = injected.root.as_ref().expect("root").ir.clone();
    assert_eq!(
        injected_ir.frame_float_array, baseline_ir.frame_float_array,
        "the weighted deform branch is a no-op in the reference runtime, so no FFD values may be written"
    );
    assert_eq!(
        injected_ir.float_array, baseline_ir.float_array,
        "skeleton geometry/texture data is unchanged by the timeline"
    );
}
