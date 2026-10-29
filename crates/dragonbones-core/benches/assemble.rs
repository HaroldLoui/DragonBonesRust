//! 装配层基线：`BaseFactory` 的完整装配路径。
//!
//! 每个 iteration 新建 factory 并走完整条链路：
//! `parse_dragon_bones_data` → `parse_texture_atlas_data` → `build_armature`。
//! 这条路径包含解析 + 骨/槽/约束构建 + 初始 `invalid_update`/`advance_one(0)`，
//! 是「从素材到可推进骨架」的真实成本。

mod common;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use dragonbones_core::armature::NullArmatureProxy;
use dragonbones_core::factory::BaseFactory;

fn assemble_rigs(c: &mut Criterion) {
    for spec in common::RIGS {
        let rig = common::load_rig(spec);

        // 先跑一遍（不参与计时）确认素材可装配，失败信息比 bench 内部 panic 更明确。
        let (factory, idx) = common::build_fixture(&rig);
        black_box((factory, idx));

        let mut group = c.benchmark_group(format!("assemble/{}", rig.spec.name));
        group.bench_function("factory_parse_build_armature", |b| {
            b.iter(|| {
                let mut factory = BaseFactory::new();
                factory
                    .parse_dragon_bones_data(black_box(&rig.ske), rig.spec.ske_stem)
                    .expect("bench 素材必然解析成功");
                factory.parse_texture_atlas_data(black_box(&rig.tex), rig.spec.ske_stem);
                let idx = factory
                    .build_armature(
                        rig.spec.armature_name,
                        rig.spec.ske_stem,
                        "",
                        rig.spec.ske_stem,
                        Box::new(NullArmatureProxy::new()),
                    )
                    .expect("bench 素材必然装配成功");
                black_box(idx)
            });
        });
        group.finish();
    }
}

criterion_group!(benches, assemble_rigs);
criterion_main!(benches);
