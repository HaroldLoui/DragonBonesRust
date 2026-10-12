//! `ArmatureData::sort_bones` 拓扑排序测试。

use dragonbones_core::model::armature_data::{ArmatureData, BoneData};
use dragonbones_core::model::constraint_data::ConstraintData;

fn bone(name: &str, parent: Option<&str>) -> BoneData {
    BoneData {
        name: name.to_string(),
        parent: parent.map(str::to_string),
        ..Default::default()
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn sort_bones_places_parents_before_children() {
    let mut armature = ArmatureData::default();
    // 故意乱序插入：子在前、父在后。
    armature.add_bone(bone("child", Some("parent")));
    armature.add_bone(bone("parent", None));

    armature.sort_bones();

    assert_eq!(armature.sorted_bones, names(&["parent", "child"]));
}

#[test]
fn sort_bones_waits_for_constraint_target() {
    let mut armature = ArmatureData::default();
    armature.add_bone(bone("c", Some("b")));
    armature.add_bone(bone("b", Some("a")));
    armature.add_bone(bone("a", Some("root")));
    armature.add_bone(bone("root", None));
    armature.add_bone(bone("target", None));

    armature.add_constraint(ConstraintData {
        name: "ik".to_string(),
        root: "c".to_string(),
        target: "target".to_string(),
        ..Default::default()
    });

    armature.sort_bones();

    assert_eq!(
        armature.sorted_bones,
        names(&["root", "target", "a", "b", "c"])
    );

    let index = |name: &str| {
        armature
            .sorted_bones
            .iter()
            .position(|n| n == name)
            .unwrap()
    };
    assert!(index("target") < index("c"), "约束 target 应先于 root");
    assert!(index("root") < index("a"));
    assert!(index("a") < index("b"));
    assert!(index("b") < index("c"));
}
