//! 阶段 5 事件派发验收（指南任务 1 测试卡）。
//!
//! 覆盖：派发发生在 clock/dbUpdate 之后、SOUND 双发顺序、无骨架 listener 时
//! SOUND 仍到达全局 manager、已回收骨架整条跳过、同值事件保留、FIFO、dt=0 派发。

mod common;

use common::{EventLog, RecordingEventManager, RecordingProxy};
use dragonbones_core::armature::{ArmatureIdx, ArmatureProxy};
use dragonbones_core::event::{DbEventListener, EventDispatcher, EventObject, ListenerId};
use dragonbones_core::factory::BaseFactory;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn fixture(record_all: bool) -> (BaseFactory, ArmatureIdx, EventLog) {
    let ske = serde_json::json!({
        "version": "5.6",
        "name": "EventFixture",
        "frameRate": 24.0,
        "armature": [{
            "name": "EventFixture",
            "frameRate": 24.0,
            "animation": [{
                "name": "sfx",
                "duration": 24,
                "frame": [{
                    "duration": 24,
                    "sound": [{ "name": "boom" }],
                    "events": [{ "name": "tick" }]
                }]
            }]
        }]
    });
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, "evt")
        .expect("parse minimal event fixture");
    let log = EventLog::default();
    let mut proxy = RecordingProxy::new(log.clone());
    if record_all {
        proxy.record_all();
    }
    let idx = factory
        .build_armature("EventFixture", "evt", "", "", Box::new(proxy))
        .expect("build event fixture");
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("attach event fixture");
    (factory, idx, log)
}

fn injected(armature: ArmatureIdx, name: &str) -> EventObject {
    EventObject {
        event_type: EventObject::FRAME_EVENT.to_owned(),
        name: name.to_owned(),
        armature: Some(armature),
        ..EventObject::default()
    }
}

/// F5.20：事件在 clock 推进（含 `proxy.db_update`）之后才派发。
#[test]
fn events_dispatch_after_clock_and_db_update() {
    let (mut factory, idx, log) = fixture(true);
    let flag = Arc::new(AtomicBool::new(false));
    let update_log = log.clone();
    let seen = Arc::clone(&flag);
    factory.armature_mut(idx).proxy.add_db_event_listener(
        EventObject::START,
        Box::new(move |_| {
            seen.store(update_log.db_updates() > 0, Ordering::SeqCst);
        }),
    );
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time("sfx", 0.0, -1)
            .is_some()
    );
    factory.advance_time(1.0 / 24.0);

    let events = log.drain();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == EventObject::START),
        "Shoot 类动画首帧应派发 start，实际 {:?}",
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        flag.load(Ordering::SeqCst),
        "listener 必须在 clock 推进/dbUpdate 之后运行（F5.20）"
    );
}

/// F5.3：SOUND 先派发到骨架 proxy，再派发到全局 eventManager。
#[test]
fn sound_is_dispatched_to_armature_then_global_manager() {
    #[derive(Clone, Default)]
    struct Order(Arc<Mutex<Vec<&'static str>>>);

    struct OrderProxy {
        inner: RecordingProxy,
        order: Order,
    }

    impl EventDispatcher for OrderProxy {
        fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
            self.inner.add_db_event_listener(ty, listener)
        }
        fn remove_db_event_listener(&mut self, id: ListenerId) {
            self.inner.remove_db_event_listener(id);
        }
        fn has_db_event_listener(&mut self, ty: &str) -> bool {
            self.inner.has_db_event_listener(ty)
        }
        fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
            if ty == EventObject::SOUND_EVENT {
                self.order.0.lock().unwrap().push("armature");
            }
            self.inner.dispatch_db_event(ty, event);
        }
    }

    impl ArmatureProxy for OrderProxy {
        fn db_init(&mut self, armature: ArmatureIdx) {
            self.inner.db_init(armature);
        }
        fn db_clear(&mut self) {
            self.inner.db_clear();
        }
        fn db_update(&mut self) {
            self.inner.db_update();
        }
        fn dispose(&mut self, dispose_proxy: bool) {
            self.inner.dispose(dispose_proxy);
        }
    }

    struct OrderManager {
        inner: RecordingEventManager,
        order: Order,
    }

    impl EventDispatcher for OrderManager {
        fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
            self.inner.add_db_event_listener(ty, listener)
        }
        fn remove_db_event_listener(&mut self, id: ListenerId) {
            self.inner.remove_db_event_listener(id);
        }
        fn has_db_event_listener(&mut self, ty: &str) -> bool {
            self.inner.has_db_event_listener(ty)
        }
        fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
            self.order.0.lock().unwrap().push("manager");
            self.inner.dispatch_db_event(ty, event);
        }
    }

    let ske = serde_json::json!({
        "version": "5.6",
        "name": "SoundFixture",
        "frameRate": 24.0,
        "armature": [{
            "name": "SoundFixture",
            "frameRate": 24.0,
            "animation": [{
                "name": "sfx",
                "duration": 24,
                "frame": [{ "duration": 24, "sound": [{ "name": "boom" }] }]
            }]
        }]
    });
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, "sfx")
        .expect("parse sound fixture");

    let order = Order::default();
    let armature_log = EventLog::default();
    let manager_log = EventLog::default();
    let mut proxy = RecordingProxy::new(armature_log.clone());
    proxy.record_all();
    let idx = factory
        .build_armature(
            "SoundFixture",
            "sfx",
            "",
            "",
            Box::new(OrderProxy {
                inner: proxy,
                order: order.clone(),
            }),
        )
        .expect("build sound fixture");
    factory.set_event_manager(Box::new(OrderManager {
        inner: RecordingEventManager::new(manager_log.clone()),
        order: order.clone(),
    }));
    factory
        .attach_armature(factory.root_clock(), idx)
        .expect("attach sound fixture");
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time("sfx", 0.0, -1)
            .is_some()
    );

    let mut sound_events = 0;
    for _ in 0..4 {
        factory.advance_time(1.0 / 24.0);
        let armed = armature_log
            .drain()
            .into_iter()
            .filter(|event| event.event_type == EventObject::SOUND_EVENT)
            .count();
        let managed = manager_log
            .drain()
            .into_iter()
            .filter(|event| event.event_type == EventObject::SOUND_EVENT)
            .count();
        assert_eq!(armed, managed, "同一条 SOUND 必须骨架/manager 各一次");
        sound_events += armed;
    }
    assert_eq!(sound_events, 1, "Sound action 应恰好触发一次");
    assert_eq!(
        *order.0.lock().unwrap(),
        ["armature", "manager"],
        "SOUND 必须先骨架、后全局 manager（F5.3）"
    );
}

/// F5.2：骨架已回收时整条事件跳过，连 SOUND 也不进全局 manager。
#[test]
fn events_targeting_recycled_armature_are_skipped() {
    let (mut factory, idx, log) = fixture(true);
    let manager_log = EventLog::default();
    factory.set_event_manager(Box::new(RecordingEventManager::new(manager_log.clone())));

    factory.dispose_armature(idx);
    factory.advance_time_with(0.0, 100.0); // flush → armature 槽位 None
    assert!(factory.armatures[idx.index()].is_none());

    let mut sound = injected(idx, "late");
    sound.event_type = EventObject::SOUND_EVENT.to_owned();
    factory.buffer_event(sound);
    factory.buffer_event(injected(idx, "late-frame"));
    factory.advance_time_with(0.0, 100.1);

    assert!(log.is_empty(), "已回收骨架的事件不得派发到骨架 proxy");
    assert!(
        manager_log.is_empty(),
        "已回收骨架的 SOUND 也不得进全局 manager（F5.2）"
    );
}

/// F5.6 / D-5.5：同值事件各派发一次；FIFO 顺序保持。
#[test]
fn same_value_events_are_preserved_and_dispatched_in_fifo_order() {
    let (mut factory, idx, log) = fixture(true);
    for name in ["first", "second", "first"] {
        factory.buffer_event(injected(idx, name));
    }
    factory.advance_time_with(0.0, 100.0);
    let names: Vec<_> = log.drain().iter().map(|event| event.name.clone()).collect();
    assert_eq!(
        names,
        ["first", "second", "first"],
        "不做内容去重，且保持入队顺序"
    );

    factory.advance_time_with(0.0, 100.1);
    assert!(log.is_empty(), "无事件帧不得重放上一帧事件");
}

/// F5.4：派发中追加到队尾的事件在同一次 advance 内被继续派发。
///
/// 当前 listener 契约（D-5.4）不允许 listener 改运行时，因此用测试 seam
/// `buffer_event` 在派发前预置两条事件，验证 while-let 不会只取首条。
#[test]
fn dispatch_drains_the_whole_queue_in_one_advance() {
    let (mut factory, idx, log) = fixture(true);
    for name in ["a", "b", "c"] {
        factory.buffer_event(injected(idx, name));
    }
    factory.advance_time_with(0.0, 100.0);
    let names: Vec<_> = log.drain().iter().map(|event| event.name.clone()).collect();
    assert_eq!(names, ["a", "b", "c"]);
}

/// 无骨架 listener 时 Sound 仍到达全局 manager（入队闸门例外的端到端验证）。
#[test]
fn sound_reaches_manager_without_armature_listener() {
    let (mut factory, idx, _log) = fixture(false);
    let manager_log = EventLog::default();
    factory.set_event_manager(Box::new(RecordingEventManager::new(manager_log.clone())));
    assert!(
        factory
            .armature_mut(idx)
            .animation
            .goto_and_play_by_time("sfx", 0.0, -1)
            .is_some()
    );

    let mut sounds = 0;
    for _ in 0..4 {
        factory.advance_time(1.0 / 24.0);
        sounds += manager_log
            .drain()
            .into_iter()
            .filter(|event| event.event_type == EventObject::SOUND_EVENT)
            .count();
    }
    assert_eq!(sounds, 1, "Sound action 应恰好到达 manager 一次");
}
