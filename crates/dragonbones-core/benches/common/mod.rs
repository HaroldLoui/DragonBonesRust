//! bench 共用工具：素材路径、读取、装配 fixture。
//!
//! 每个 bench target 各自编译一份本模块（`benches/common/` 不是 cargo target），
//! 因此允许存在某个 target 未用到的辅助函数。

#![allow(dead_code)]

use std::path::PathBuf;

use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy};
use dragonbones_core::factory::BaseFactory;
use serde_json::Value;

/// 仓库 `assets/` 目录：`<crate>/../../assets`。
#[must_use]
pub fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// 一个 bench 素材的静态描述（bench 注册期使用）。
pub struct RigSpec {
    /// 报告里显示的素材名（= `testdata/golden/<name>` 目录名）。
    pub name: &'static str,
    /// ske 文件名去掉 `.json`，同时作为 factory 的缓存名（ske/tex 共用）。
    pub ske_stem: &'static str,
    /// 骨架名——**不是文件名**，DragonBones 导出时另行命名。
    pub armature_name: &'static str,
    /// advance bench 播放的动画名（取素材里循环播放的代表动画）。
    pub animation: &'static str,
}

/// 解析 / 装配 / 推进三类 bench 共用的三个代表性素材。
///
/// 选型：`Dragon`（中等骨架、4 动画）、`PlantAnimate`（29 骨 31 槽、6 动画）、
/// `Rooster_Ani`（63 骨 / 8 个 mesh 槽，最大素材）。
pub const RIGS: &[RigSpec] = &[
    RigSpec {
        name: "Dragon",
        ske_stem: "Dragon_ske",
        armature_name: "Dragon",
        animation: "walk",
    },
    RigSpec {
        name: "PlantAnimate",
        ske_stem: "PlantAnimate_ske",
        armature_name: "MegaGatlingPea",
        animation: "Shoot",
    },
    RigSpec {
        name: "Rooster_Ani",
        ske_stem: "Rooster_Ani_ske",
        armature_name: "armatureName",
        animation: "rooster_eat_anim",
    },
];

/// 已读入内存的单个素材（文件 IO 与 `serde_json` 解析都在计时之外完成）。
pub struct Rig {
    pub spec: &'static RigSpec,
    /// ske 原文，供「serde_json::from_str + parse」完整路径 bench 使用。
    pub ske_text: String,
    /// 预先解析好的 ske `Value`，供只测 parser 的 bench 使用。
    pub ske: Value,
    pub tex: Value,
}

/// 读取并解析一个素材的 ske / tex。
#[must_use]
pub fn load_rig(spec: &'static RigSpec) -> Rig {
    let root = asset_root();
    let ske_path = root.join(format!("{}.json", spec.ske_stem));
    let tex_path = root.join(format!("{}.json", spec.ske_stem.replace("_ske", "_tex")));

    let ske_text = std::fs::read_to_string(&ske_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", ske_path.display()));
    let ske: Value = serde_json::from_str(&ske_text)
        .unwrap_or_else(|error| panic!("解析 {} 失败：{error}", ske_path.display()));
    let tex_text = std::fs::read_to_string(&tex_path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", tex_path.display()));
    let tex: Value = serde_json::from_str(&tex_text)
        .unwrap_or_else(|error| panic!("解析 {} 失败：{error}", tex_path.display()));

    Rig {
        spec,
        ske_text,
        ske,
        tex,
    }
}

/// 单个 ske 素材的原文（解析吞吐 group 用，素材按 `assets/*_ske.json` 自动发现）。
pub struct Asset {
    /// 文件名去扩展名，如 `Dragon_ske`。
    pub stem: String,
    /// ske 原文（字节数同时用作 throughput 单位）。
    pub text: String,
}

/// 扫描 `assets/*_ske.json`，按文件名排序读入全部 ske 原文。
///
/// 注：`testdata/golden/PlantAnimate_compressed` 复用同一份
/// `PlantAnimate_ske.json`，仓库内没有独立的 compressed 素材文件，故这里自然为 8 个。
#[must_use]
pub fn load_assets() -> Vec<Asset> {
    let root = asset_root();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("读取目录 {} 失败：{error}", root.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_ske.json"))
        })
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "{} 下没有找到任何 *_ske.json 素材",
        root.display()
    );

    paths
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| panic!("{}: 无文件名", path.display()));
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
            Asset { stem, text }
        })
        .collect()
}

/// 装配一个完整 fixture：`BaseFactory::new` + 解析 ske + 解析图集 + build_armature。
///
/// 返回的 factory 持有全部运行时对象，调用方负责保持其存活。
pub fn build_fixture(rig: &Rig) -> (BaseFactory, ArmatureIdx) {
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&rig.ske, rig.spec.ske_stem)
        .unwrap_or_else(|| panic!("{}: parse_dragon_bones_data 失败", rig.spec.name));
    assert!(
        factory.parse_texture_atlas_data(&rig.tex, rig.spec.ske_stem),
        "{}: parse_texture_atlas_data 失败",
        rig.spec.name
    );
    let idx = factory
        .build_armature(
            rig.spec.armature_name,
            rig.spec.ske_stem,
            "",
            rig.spec.ske_stem,
            Box::new(NullArmatureProxy::new()),
        )
        .unwrap_or_else(|| panic!("{}: build_armature 失败", rig.spec.name));
    (factory, idx)
}

/// 把骨架挂到根 clock 并循环播放指定动画。
///
/// 只有成为 clock 成员，`BaseFactory::advance_time` 才会推进该骨架。
pub fn attach_and_play(factory: &mut BaseFactory, idx: ArmatureIdx, animation: &str) {
    factory
        .attach_armature(factory.root_clock(), idx)
        .unwrap_or_else(|error| panic!("attach_armature 失败：{error}"));
    factory
        .armature_mut(idx)
        .animation
        .goto_and_play_by_time(animation, 0.0, -1)
        .unwrap_or_else(|| panic!("goto_and_play_by_time({animation}) 失败"));
}
