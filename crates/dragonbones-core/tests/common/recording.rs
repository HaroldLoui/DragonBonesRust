//! 阶段 5 测试接缝：记录真实派发的事件（替代阶段 4 的 `take_flushed_events` 快照）。
//!
//! - [`RecordingProxy`] 实现 `ArmatureProxy` + `EventDispatcher`：按事件类型注册
//!   listener（打开官方入队闸门），在 `dispatch_db_event` 时调用 listener 并记录 clone；
//! - [`RecordingEventManager`] 只记录全局 `eventManager` 收到的 SOUND 双发；
//! - [`EventLog`] 跨线程共享（`Arc<Mutex<...>>`），并记录 `db_update` 次数供
//!   “派发生在 clock/dbUpdate 之后”的断言使用。

use dragonbones_core::armature::{ArmatureIdx, ArmatureProxy};
use dragonbones_core::event::{DbEventListener, EventDispatcher, EventObject, ListenerId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 9 类事件（与 `jsdump.mjs` 的 `EVENT_TYPES` 一致）。
pub const EVENT_TYPES: [&str; 9] = [
    EventObject::START,
    EventObject::LOOP_COMPLETE,
    EventObject::COMPLETE,
    EventObject::FADE_IN,
    EventObject::FADE_IN_COMPLETE,
    EventObject::FADE_OUT,
    EventObject::FADE_OUT_COMPLETE,
    EventObject::FRAME_EVENT,
    EventObject::SOUND_EVENT,
];

#[derive(Clone, Default)]
pub struct EventLog {
    events: Arc<Mutex<Vec<EventObject>>>,
    db_updates: Arc<AtomicUsize>,
    db_clears: Arc<AtomicUsize>,
}

impl EventLog {
    /// 取走当前记录的事件（每帧采样后调用）。
    pub fn drain(&self) -> Vec<EventObject> {
        std::mem::take(&mut *self.events.lock().expect("event log poisoned"))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.lock().expect("event log poisoned").len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `db_update` 累计次数（正/负向断言“派发在 dbUpdate 之后”）。
    #[must_use]
    pub fn db_updates(&self) -> usize {
        self.db_updates.load(Ordering::SeqCst)
    }

    /// `db_clear` 累计次数（延迟回收后应恰好在 flush 时发生一次）。
    #[must_use]
    pub fn db_clears(&self) -> usize {
        self.db_clears.load(Ordering::SeqCst)
    }

    fn push(&self, event: &EventObject) {
        self.events
            .lock()
            .expect("event log poisoned")
            .push(event.clone());
    }

    fn note_db_update(&self) {
        self.db_updates.fetch_add(1, Ordering::SeqCst);
    }

    fn note_db_clear(&self) {
        self.db_clears.fetch_add(1, Ordering::SeqCst);
    }
}

/// 记录派发的骨架代理：注册 listener 才会记录（与“无 listener 不观察”一致）。
pub struct RecordingProxy {
    log: EventLog,
    listeners: HashMap<String, Vec<(ListenerId, DbEventListener)>>,
}

impl RecordingProxy {
    #[must_use]
    pub fn new(log: EventLog) -> Self {
        Self {
            log,
            listeners: HashMap::new(),
        }
    }

    /// 注册 9 类空 listener：打开 Frame/lifecycle 闸门，并让派发被记录。
    pub fn record_all(&mut self) {
        for event_type in EVENT_TYPES {
            self.add_db_event_listener(event_type, Box::new(|_| {}));
        }
    }

    #[must_use]
    pub fn log(&self) -> &EventLog {
        &self.log
    }
}

impl EventDispatcher for RecordingProxy {
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
        let id = ListenerId::next();
        self.listeners
            .entry(ty.to_owned())
            .or_default()
            .push((id, listener));
        id
    }

    fn remove_db_event_listener(&mut self, id: ListenerId) {
        for listeners in self.listeners.values_mut() {
            listeners.retain(|(candidate, _)| *candidate != id);
        }
    }

    fn has_db_event_listener(&mut self, ty: &str) -> bool {
        self.listeners
            .get(ty)
            .is_some_and(|listeners| !listeners.is_empty())
    }

    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
        if let Some(listeners) = self.listeners.get(ty) {
            for (_, listener) in listeners {
                listener(event);
            }
            self.log.push(event);
        }
    }
}

impl ArmatureProxy for RecordingProxy {
    fn db_init(&mut self, _armature: ArmatureIdx) {}

    fn db_clear(&mut self) {
        self.log.note_db_clear();
        self.listeners.clear();
    }

    fn db_update(&mut self) {
        self.log.note_db_update();
    }

    fn dispose(&mut self, _dispose_proxy: bool) {}
}

/// 记录全局 `eventManager`（SOUND 双发）的 dispatcher。
#[derive(Default)]
pub struct RecordingEventManager {
    log: EventLog,
    listeners: HashMap<String, Vec<ListenerId>>,
}

impl RecordingEventManager {
    #[must_use]
    pub fn new(log: EventLog) -> Self {
        Self {
            log,
            listeners: HashMap::new(),
        }
    }

    #[must_use]
    pub fn log(&self) -> &EventLog {
        &self.log
    }
}

impl EventDispatcher for RecordingEventManager {
    fn add_db_event_listener(&mut self, ty: &str, _listener: DbEventListener) -> ListenerId {
        let id = ListenerId::next();
        self.listeners.entry(ty.to_owned()).or_default().push(id);
        id
    }

    fn remove_db_event_listener(&mut self, id: ListenerId) {
        for listeners in self.listeners.values_mut() {
            listeners.retain(|candidate| *candidate != id);
        }
    }

    fn has_db_event_listener(&mut self, ty: &str) -> bool {
        self.listeners
            .get(ty)
            .is_some_and(|listeners| !listeners.is_empty())
    }

    fn dispatch_db_event(&mut self, _ty: &str, event: &EventObject) {
        // 官方 `_eventManager.dispatchDBEvent` 无条件被调用；记录所有到达 manager 的事件。
        self.log.push(event);
    }
}
