//! 方案 B 基线：用计数 allocator 量化「加载」与「每帧推进」的分配次数。
//!
//! 这是 harness=false 的普通程序，`cargo bench -p dragonbones-core --bench allocations`
//! 直接打印表格；不做 criterion 计时（时间数字由 `baseline.rs` / `advance.rs` 提供）。
//!
//! 指标：
//! - `parse`: 一次 `serde_json::from_str` + `ObjectDataParser` 全量解析；
//! - `build`: 一次 `build_armature` + `parse_texture_atlas_data`；
//! - `advance/frame`: 稳态下 `BaseFactory::advance_time(1/60)` 每帧的分配次数/字节；
//! - `load_all`: 29 套 assets 全量解析（demo 启动路径）。

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use dragonbones_core::factory::BaseFactory;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

struct CountingAlloc;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size.saturating_sub(layout.size()), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

fn reset() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn counts() -> (usize, usize) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

fn measure(mut f: impl FnMut()) -> (usize, usize) {
    reset();
    f();
    counts()
}

const DT: f64 = 1.0 / 60.0;

fn measure_parse(rig: &common::Rig) -> (usize, usize) {
    measure(|| {
        let value: Value = serde_json::from_str(&rig.ske_text).expect("ske JSON");
        let mut parser = ObjectDataParser::new();
        let data = parser.parse_dragon_bones_data(&value, 1.0);
        std::hint::black_box(data);
    })
}

fn measure_build(rig: &common::Rig) -> (usize, usize) {
    measure(|| {
        let mut factory = BaseFactory::new();
        let data = factory
            .parse_dragon_bones_data(&rig.ske, rig.spec.ske_stem)
            .expect("parse ske");
        assert!(factory.parse_texture_atlas_data(&rig.tex, rig.spec.ske_stem));
        let idx = factory
            .build_armature(
                rig.spec.armature_name,
                rig.spec.ske_stem,
                "",
                rig.spec.ske_stem,
                Box::new(dragonbones_core::armature::NullArmatureProxy::new()),
            )
            .expect("build armature");
        std::hint::black_box((data, idx, factory));
    })
}

fn measure_advance(rig: &common::Rig, frames: usize) -> (usize, usize, f64) {
    let (mut factory, idx) = common::build_fixture(rig);
    common::attach_and_play(&mut factory, idx, rig.spec.animation);
    for _ in 0..16 {
        factory.advance_time(DT);
    }
    let started = Instant::now();
    let (allocs, bytes) = measure(|| {
        for _ in 0..frames {
            factory.advance_time(DT);
        }
    });
    let elapsed = started.elapsed().as_secs_f64();
    std::hint::black_box(&factory);
    (allocs, bytes, elapsed)
}

fn main() {
    println!("== per-rig (allocations per operation) ==");
    println!(
        "{:<14} {:>8} {:>10} {:>8} {:>10} {:>9} {:>9} {:>8}",
        "rig", "parse_n", "parse_B", "build_n", "build_B", "adv_n/f", "adv_B/f", "adv_µs/f"
    );
    for spec in common::RIGS {
        let rig = common::load_rig(spec);
        let (parse_n, parse_b) = measure_parse(&rig);
        let (build_n, build_b) = measure_build(&rig);
        let (adv_n, adv_b, elapsed) = measure_advance(&rig, 600);
        println!(
            "{:<14} {:>8} {:>10} {:>8} {:>10} {:>9} {:>9} {:>8.2}",
            spec.name,
            parse_n,
            parse_b,
            build_n,
            build_b,
            adv_n / 600,
            adv_b / 600,
            elapsed * 1e6 / 600.0
        );
    }

    println!("\n== batch load (29 assets) ==");
    let assets = common::load_assets();
    let (allocs, bytes) = measure(|| {
        for asset in &assets {
            let value: Value = serde_json::from_str(&asset.text).expect("assets ske JSON");
            let mut parser = ObjectDataParser::new();
            std::hint::black_box(parser.parse_dragon_bones_data(&value, 1.0));
        }
    });
    println!(
        "load_all: {} rigs, {allocs} allocs, {bytes} bytes ({} B/rig)",
        assets.len(),
        bytes / assets.len()
    );
}
