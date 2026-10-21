//! 阶段 5 对象生命周期验收（指南任务 2 测试卡）。
//!
//! 覆盖：dispose 的“上锁 + 入缓冲”窗口、帧首 flush 清场、clock detach、
//! `db_clear` 恰好一次、幂等、arena 空位复用、未挂 clock 的骨架。

mod common;

use common::{EventLog, RecordingProxy};
use dragonbones_core::animation::world_clock::AnimatableHandle;
use dragonbones_core::armature::ArmatureIdx;
use dragonbones_core::factory::BaseFactory;

fn fixture() -> (BaseFactory, ArmatureIdx, EventLog) {
    let ske = serde_json::json!({
        "version": "5.6",
        "name": "LifecycleFixture",
        "frameRate": 24.0,
        "armature": [{
            "name": "LifecycleFixture",
            "frameRate": 24.0,
            "animation": [{ "name": "idle", "duration": 24 }]
        }]
    });
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, "life")
        .expect("parse lifecycle fixture");
    let log = EventLog::default();
    let idx = factory
        .build_armature(
            "LifecycleFixture",
            "life",
            "",
            "",
            Box::new(RecordingProxy::new(log.clone())),
        )
        .expect("build lifecycle fixture");
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("attach lifecycle fixture");
    (factory, idx, log)
}

/// F5.7/F5.9：dispose 只上锁 + 入队；窗口内仍可查询、`advance_one` 不推进。
#[test]
fn dispose_defers_clearing_until_next_global_advance() {
    let (mut factory, idx, log) = fixture();
    let updates_before = log.db_updates();

    factory.dispose_armature(idx);
    assert!(
        factory.armatures[idx.index()].is_some(),
        "flush 前槽位必须仍存在（官方 _objects 延迟回收）"
    );
    assert_eq!(factory.armature(idx).name(), "LifecycleFixture");
    assert!(
        factory.clock_contains(factory.root_clock(), AnimatableHandle::Armature(idx)),
        "flush 前仍应保留 clock 成员关系"
    );

    factory.advance_one(idx, 0.1);
    assert_eq!(
        log.db_updates(),
        updates_before,
        "已 dispose 的骨架因 lock_update 不得推进/调用 db_update"
    );
}

/// F5.8/F5.1：下一次 `advance_time` 先 flush：clock detach、`on_clear`（含
/// `db_clear`）、槽位释放、free 入栈。
#[test]
fn frame_start_flush_detaches_clock_clears_and_frees_slot() {
    let (mut factory, idx, log) = fixture();
    let root = factory.root_clock();
    factory.dispose_armature(idx);
    factory.advance_time_with(0.0, 100.0);

    assert!(
        factory.armatures[idx.index()].is_none(),
        "flush 后槽位应为 None"
    );
    assert!(
        factory.free.contains(&idx.0),
        "flush 后槽位应进入 free 复用栈"
    );
    assert!(
        !factory.clock_contains(root, AnimatableHandle::Armature(idx)),
        "flush 后必须从 clock 成员表移除"
    );
    assert_eq!(log.db_clears(), 1, "proxy.db_clear 应恰好调用一次");
}

/// 幂等：同帧重复 dispose 只入队一次；flush 后再次 dispose 只 warn、不 panic。
#[test]
fn dispose_is_idempotent_within_and_across_frames() {
    let (mut factory, idx, log) = fixture();
    factory.dispose_armature(idx);
    factory.dispose_armature(idx);
    factory.advance_time_with(0.0, 100.0);
    assert_eq!(log.db_clears(), 1, "重复 dispose 只能清场一次");
    assert!(factory.free.contains(&idx.0));

    // 已回收 idx 再次 dispose：warn 后静默返回。
    factory.dispose_armature(idx);
    assert_eq!(log.db_clears(), 1);
}

/// flush 后 free 槽可被下一次 build 复用（arena 池语义，D-3.1）。
#[test]
fn freed_slot_is_reused_by_next_build() {
    let (mut factory, idx, _log) = fixture();
    factory.dispose_armature(idx);
    factory.advance_time_with(0.0, 100.0);
    assert!(factory.free.contains(&idx.0));

    let rebuilt = factory
        .build_armature(
            "LifecycleFixture",
            "life",
            "",
            "",
            Box::new(RecordingProxy::new(EventLog::default())),
        )
        .expect("rebuild after dispose");
    assert_eq!(rebuilt, idx, "空位栈应复用刚释放的 ArmatureIdx");
}

/// 未挂 clock 的骨架 dispose + flush 不 panic，`clock=None` 保持不变。
#[test]
fn disposing_unattached_armature_is_safe() {
    let (mut factory, idx, log) = fixture();
    factory.detach_armature(idx).expect("detach fixture");
    assert_eq!(factory.armature(idx).clock(), None);

    factory.dispose_armature(idx);
    factory.advance_time_with(0.0, 100.0);
    assert!(factory.armatures[idx.index()].is_none());
    assert_eq!(log.db_clears(), 1);
}
