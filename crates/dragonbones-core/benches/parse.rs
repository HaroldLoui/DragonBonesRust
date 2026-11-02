//! 解析层基线：`ObjectDataParser`（ske / tex）与「serde_json + parse」完整路径。
//!
//! 覆盖：
//! - `parse/<rig>/object_data_parser`：`Value` 已预解析，只测 parser 本体；
//! - `parse/<rig>/serde_json+object_data_parser`：从 JSON 原文开始的完整解析；
//! - `parse/<rig>/texture_atlas`：`_tex.json` 图集解析；
//! - `parse/assets/<stem>`：8 个素材的完整解析吞吐（字节吞吐，MiB/s）。

mod common;

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;

fn parse_rigs(c: &mut Criterion) {
    for spec in common::RIGS {
        let rig = common::load_rig(spec);
        let mut group = c.benchmark_group(format!("parse/{}", rig.spec.name));

        // 素材 ske 不含内嵌 `textureAtlas`，parser 跨 iteration 复用无残留状态。
        let mut parser = ObjectDataParser::new();
        group.bench_function("object_data_parser", |b| {
            b.iter(|| {
                let data = parser.parse_dragon_bones_data(black_box(&rig.ske), 1.0);
                black_box(data)
            });
        });

        group.bench_function("serde_json+object_data_parser", |b| {
            b.iter(|| {
                let mut parser = ObjectDataParser::new();
                let raw: serde_json::Value = serde_json::from_str(black_box(&rig.ske_text))
                    .expect("bench 输入的 ske JSON 必然合法");
                let data = parser.parse_dragon_bones_data(black_box(&raw), 1.0);
                black_box(data)
            });
        });

        group.bench_function("texture_atlas", |b| {
            let mut parser = ObjectDataParser::new();
            b.iter(|| {
                let mut atlas = TextureAtlasData::default();
                let parsed = parser.parse_texture_atlas_data(black_box(&rig.tex), &mut atlas, 1.0);
                black_box((parsed, atlas))
            });
        });

        group.finish();
    }
}

fn parse_assets(c: &mut Criterion) {
    let assets = common::load_assets();
    let mut group = c.benchmark_group("parse/assets");
    for asset in &assets {
        group.throughput(Throughput::Bytes(asset.text.len() as u64));
        group.bench_function(asset.stem.as_str(), |b| {
            b.iter(|| {
                let mut parser = ObjectDataParser::new();
                let raw: serde_json::Value = serde_json::from_str(black_box(&asset.text))
                    .expect("bench 输入的 ske JSON 必然合法");
                let data = parser.parse_dragon_bones_data(black_box(&raw), 1.0);
                black_box(data)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, parse_rigs, parse_assets);
criterion_main!(benches);
