//! 动画推进基线：装配一次后测「每帧推进」的成本。
//!
//! - `advance/<rig>/factory_advance_time`：`BaseFactory::advance_time(1/60)`
//!   （根 clock 整棵子树 + 事件 flush），每个素材循环播放自带动画；
//! - `advance/Dragon|Rooster_Ani/factory_advance_one`：单骨架直接推进
//!   `advance_one(idx, 1/60)`，绕开 clock 树。

mod common;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

/// bench 固定用 60 FPS 的帧间隔（与素材自身 frameRate 无关，便于横向比较）。
const DT: f64 = 1.0 / 60.0;

fn advance_rigs(c: &mut Criterion) {
    for spec in common::RIGS {
        let rig = common::load_rig(spec);
        let (mut factory, idx) = common::build_fixture(&rig);
        common::attach_and_play(&mut factory, idx, rig.spec.animation);
        // 推进一帧：建立 timeline 状态 / 首帧事件，避免把首帧冷路径算进每次迭代。
        factory.advance_time(DT);

        let mut group = c.benchmark_group(format!("advance/{}", rig.spec.name));
        group.bench_function("factory_advance_time", |b| {
            b.iter(|| factory.advance_time(black_box(DT)));
        });

        // 任务要求：Dragon / Rooster_Ani 各加一个单骨架推进。
        if matches!(rig.spec.name, "Dragon" | "Rooster_Ani") {
            group.bench_function("factory_advance_one", |b| {
                b.iter(|| factory.advance_one(black_box(idx), black_box(DT)));
            });
        }
        group.finish();
    }
}

criterion_group!(benches, advance_rigs);
criterion_main!(benches);
