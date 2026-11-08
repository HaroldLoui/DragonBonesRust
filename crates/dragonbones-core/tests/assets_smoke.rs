//! 素材冒烟测试：`assets/<rig>/<stem>_ske.json` 都必须能解析、装配并推进动画。
//!
//! 这是一个“素材级”回归护栏：新增 DragonBones 导出物时，只要 skeleton/atlas 与运行时
//! 语义不兼容（缺 tex、图集名对不上、armature 建不出来、时间轴推进 panic），这里会直接
//! 失败，而不是等 demo 里目视发现。
//!
//! 与 `benches/parse.rs`（只测解析吞吐）和 `golden_*`（固定素材数值对拍）互补。

use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;
use serde_json::Value;
use std::path::PathBuf;

fn assets_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn read_json(path: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("解析 {} 失败：{error}", path.display()))
}

#[test]
fn every_asset_parses_assembles_and_advances() {
    let root = assets_root();
    let mut skeletons: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("读取目录 {} 失败：{error}", root.display()))
        .filter_map(|entry| entry.ok())
    {
        let rig_dir = entry.path();
        if !rig_dir.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&rig_dir)
            .unwrap_or_else(|error| panic!("读取目录 {} 失败：{error}", rig_dir.display()))
            .filter_map(|file| file.ok())
        {
            let path = file.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_ske.json"))
            {
                skeletons.push(path);
            }
        }
    }
    skeletons.sort();
    assert!(
        !skeletons.is_empty(),
        "{} 下没有找到任何 <rig>/*_ske.json 素材",
        root.display()
    );

    for ske_path in skeletons {
        let file_name = ske_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("asset file name");
        let stem = file_name
            .strip_suffix("_ske.json")
            .expect("filtered to *_ske.json");
        let tex_path = ske_path
            .parent()
            .expect("skeleton lives in its rig directory")
            .join(format!("{stem}_tex.json"));
        assert!(
            tex_path.is_file(),
            "素材 `{stem}` 缺少配套图集 {}",
            tex_path.display()
        );

        let ske = read_json(&ske_path);
        let tex = read_json(&tex_path);

        let armatures = ske
            .get("armature")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("素材 `{stem}` 没有 armature 数组"));

        let mut factory = BaseFactory::new();
        assert!(
            factory.parse_dragon_bones_data(&ske, stem).is_some(),
            "素材 `{stem}` 骨架解析失败"
        );
        assert!(
            factory.parse_texture_atlas_data(&tex, stem),
            "素材 `{stem}` 图集解析失败"
        );

        for armature in armatures {
            let armature_name = armature
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("素材 `{stem}` 有匿名 armature"));
            let animation_name = armature
                .get("animation")
                .and_then(Value::as_array)
                .and_then(|animations| animations.first())
                .and_then(|animation| animation.get("name"))
                .and_then(Value::as_str);

            let built = factory
                .build_armature(
                    armature_name,
                    stem,
                    "",
                    stem,
                    Box::new(NullArmatureProxy::new()),
                )
                .unwrap_or_else(|| panic!("素材 `{stem}` 的 armature `{armature_name}` 装配失败"));
            factory
                .attach_armature(factory.root_clock(), built)
                .unwrap_or_else(|error| {
                    panic!("素材 `{stem}`/`{armature_name}` attach 失败：{error:?}")
                });

            if let Some(animation_name) = animation_name {
                factory
                    .armature_mut(built)
                    .animation
                    .goto_and_play_by_time(animation_name, 0.0, -1)
                    .unwrap_or_else(|| {
                        panic!("素材 `{stem}`/`{armature_name}` 播放动画 `{animation_name}` 失败")
                    });
            }

            // 推进 10 帧（30fps），覆盖时间轴求值与 pose 更新，触发潜在 panic。
            for _ in 0..10 {
                factory.advance_time(1.0 / 30.0);
            }
        }
    }
}
