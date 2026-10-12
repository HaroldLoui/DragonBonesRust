mod common;

use common::{golden_dir_stems, golden_path, read_value};

/// 素材清单（实测基准）：目录 → (armature, bones, slots, frameRate, 动画集合)
struct AssetSpec {
    dir: &'static str,
    armature: &'static str,
    bones: usize,
    slots: usize,
    frame_rate: u64,
    anims: &'static [&'static str],
}

const SPECS: &[AssetSpec] = &[
    AssetSpec {
        dir: "Dragon",
        armature: "Dragon",
        bones: 19,
        slots: 18,
        frame_rate: 24,
        anims: &["stand", "walk", "jump", "fall"],
    },
    AssetSpec {
        dir: "Dragon_Blend",
        armature: "Dragon",
        bones: 19,
        slots: 18,
        frame_rate: 24,
        anims: &["stand", "walk", "jump", "fall"],
    },
    AssetSpec {
        dir: "PlantAnimate",
        armature: "MegaGatlingPea",
        bones: 29,
        slots: 31,
        frame_rate: 30,
        anims: &["Idle", "Shoot", "Death", "Food", "Water", "Random"],
    },
    AssetSpec {
        dir: "PlantAnimate_compressed",
        armature: "MegaGatlingPea",
        bones: 29,
        slots: 31,
        frame_rate: 30,
        anims: &["Idle", "Shoot", "Death", "Food", "Water", "Random"],
    },
    AssetSpec {
        dir: "Rooster_Ani",
        armature: "armatureName",
        bones: 63,
        slots: 15,
        frame_rate: 24,
        anims: &[
            "rooster_idle_anim",
            "rooster_walk_anim",
            "rooster_idle_sleep_anim",
            "rooster_eat_anim",
            "rooster_run_anim",
        ],
    },
    AssetSpec {
        dir: "Rooster_Blend",
        armature: "armatureName",
        bones: 63,
        slots: 15,
        frame_rate: 24,
        anims: &[
            "rooster_idle_anim",
            "rooster_walk_anim",
            "rooster_idle_sleep_anim",
            "rooster_eat_anim",
            "rooster_run_anim",
        ],
    },
    AssetSpec {
        dir: "Rooster_P1",
        armature: "armatureName",
        bones: 63,
        slots: 15,
        frame_rate: 24,
        anims: &[
            "rooster_idle_anim",
            "rooster_walk_anim",
            "rooster_idle_sleep_anim",
            "rooster_eat_anim",
            "rooster_run_anim",
        ],
    },
    AssetSpec {
        dir: "Surface_P1_4",
        armature: "fixture",
        bones: 4,
        slots: 2,
        frame_rate: 24,
        anims: &["idle"],
    },
    AssetSpec {
        dir: "龙",
        armature: "armatureName",
        bones: 60,
        slots: 29,
        frame_rate: 30,
        anims: &["stand"],
    },
];

/// 目录 / 动画 / 骨架结构 与素材清单完全一致
#[test]
fn golden_inventory_matches_asset_specs() {
    let mut total_anims = 0usize;

    for spec in SPECS {
        // 目录下的动画文件集合 == 清单
        let mut expected: Vec<String> = spec.anims.iter().map(|s| s.to_string()).collect();
        expected.sort();
        let actual = golden_dir_stems(spec.dir);
        assert_eq!(actual, expected, "目录 `{}` 的动画文件集合不符", spec.dir);
        total_anims += spec.anims.len();

        // 每个 golden 的 meta 结构一致
        for anim in spec.anims {
            let path = golden_path(spec.dir, anim);
            let v = read_value(&path);
            let meta = &v["meta"];
            let ctx = path.display().to_string();

            assert_eq!(
                meta["armature"].as_str(),
                Some(spec.armature),
                "{ctx}: armature"
            );
            assert_eq!(meta["animation"].as_str(), Some(*anim), "{ctx}: animation");
            assert_eq!(
                meta["frameRate"].as_u64(),
                Some(spec.frame_rate),
                "{ctx}: frameRate"
            );
            assert_eq!(
                meta["boneNames"].as_array().map(|a| a.len()),
                Some(spec.bones),
                "{ctx}: 骨骼数"
            );
            assert_eq!(
                meta["slotNames"].as_array().map(|a| a.len()),
                Some(spec.slots),
                "{ctx}: 插槽数"
            );
            assert_eq!(meta["frameCount"].as_u64(), Some(120), "{ctx}: 帧数");
        }
    }

    assert_eq!(total_anims, 37, "合计动画数应为 37");
}

/// PlantAnimate 压缩/非压缩是同一数据：除 meta.skeFile 外逐字节语义相等
#[test]
fn plantanimate_compressed_matches_plain() {
    let anims = ["Idle", "Shoot", "Death", "Food", "Water", "Random"];

    for anim in anims {
        let mut plain = read_value(&golden_path("PlantAnimate", anim));
        let mut compressed = read_value(&golden_path("PlantAnimate_compressed", anim));

        // skeFile 字段必然不同（文件名不同），剔除后其余应完全相等
        plain["meta"]
            .as_object_mut()
            .unwrap()
            .remove("skeFile")
            .expect("plain meta 应含 skeFile");
        compressed["meta"]
            .as_object_mut()
            .unwrap()
            .remove("skeFile")
            .expect("compressed meta 应含 skeFile");

        assert_eq!(
            plain, compressed,
            "PlantAnimate/{anim} 与 compressed 版本应一致（仅 skeFile 可不同）"
        );
    }
}
