//! 方案 B 基线补充：批量加载 29 套 assets、dbbin 二进制入口。
//!
//! 与 `parse.rs`/`assemble.rs` 的分工：
//! - `load/batch29/*`：把 `assets/<rig>/*_ske.json` 全部解析一遍（启动加载路径），
//!   分别记录「serde_json 解析 Value」「parser 消费 Value」「两者合计」；
//! - `dbbin/*`：官方 dbbin 样本走 `parse_binary_dragon_bones_data` 的耗时。
//!
//! 解析耗时用 `Throughput::Bytes` 给出 MiB/s，方便和 §3.1 的历史数字对齐。

mod common;

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

const DT: f64 = 1.0 / 60.0;

fn load_batch(c: &mut Criterion) {
    let assets = common::load_assets();
    let total_bytes: u64 = assets.iter().map(|asset| asset.text.len() as u64).sum();
    let values: Vec<Value> = assets
        .iter()
        .map(|asset| serde_json::from_str(&asset.text).expect("assets ske JSON 合法"))
        .collect();

    let mut group = c.benchmark_group("load/batch29");
    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_function("serde_json_only", |b| {
        b.iter(|| {
            for asset in &assets {
                let value: Value =
                    serde_json::from_str(black_box(&asset.text)).expect("assets ske JSON 合法");
                black_box(value);
            }
        });
    });

    group.bench_function("parser_only", |b| {
        let mut parser = ObjectDataParser::new();
        b.iter(|| {
            for value in &values {
                let data = parser.parse_dragon_bones_data(black_box(value), 1.0);
                black_box(data);
            }
        });
    });

    group.bench_function("serde_json+parser", |b| {
        let mut parser = ObjectDataParser::new();
        b.iter(|| {
            for asset in &assets {
                let value: Value =
                    serde_json::from_str(black_box(&asset.text)).expect("assets ske JSON 合法");
                let data = parser.parse_dragon_bones_data(black_box(&value), 1.0);
                black_box(data);
            }
        });
    });
    group.finish();

    // 单文件吞吐拆解：对最大的三个素材分别计时，作为对照（其余素材见 parse.rs）。
    let mut group = c.benchmark_group("load/batch29/single");
    for rig in ["Dragon", "PlantAnimate", "Rooster_Ani"] {
        let asset = assets
            .iter()
            .find(|asset| asset.stem == format!("{rig}_ske"))
            .unwrap_or_else(|| panic!("assets 缺少 {rig}"));
        group.throughput(Throughput::Bytes(asset.text.len() as u64));
        group.bench_function(rig, |b| {
            let mut parser = ObjectDataParser::new();
            b.iter(|| {
                let value: Value =
                    serde_json::from_str(black_box(&asset.text)).expect("assets ske JSON 合法");
                let data = parser.parse_dragon_bones_data(black_box(&value), 1.0);
                black_box(data);
            });
        });
    }
    group.finish();
}

fn dbbin_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/dbbin")
}

fn load_dbbin(c: &mut Criterion) {
    let root = dbbin_root();
    let mut group = c.benchmark_group("dbbin");
    for dir in std::fs::read_dir(&root).expect("testdata/dbbin 存在") {
        let dir = dir.expect("entry").path();
        if !dir.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&dir).expect("dbbin 子目录") {
            let path = file.expect("entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("dbbin") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("dbbin")
                .to_owned();
            let bytes = std::fs::read(&path).expect("read dbbin");
            group.throughput(Throughput::Bytes(bytes.len() as u64));
            group.bench_function(name, |b| {
                let mut parser = ObjectDataParser::new();
                b.iter(|| {
                    let data = parser.parse_binary_dragon_bones_data(black_box(&bytes), 1.0);
                    black_box(data);
                });
            });
        }
    }
    group.finish();
    // 让 clippy 知道常量被用到（dbbin 目前不含动画推进，仅解析）。
    black_box(DT);
}

criterion_group!(benches, load_batch, load_dbbin);
criterion_main!(benches);
