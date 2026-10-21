//! `armature/IArmatureProxy.ts`（93 行）。
//!
//! D-3.4：Rust 侧 `thisObject` 用闭包捕获表达，`EventStringType` 用 `&str`；
//! 阶段 3 交 [`NullArmatureProxy`]（= 阶段 0 `jsdump.mjs` 的 `EvProxy` 语义），
//! 阶段 7 换 Bevy 实现。
//!
//! 阶段 5 起监听器接口拆到 [`EventDispatcher`]（对应官方
//! `IArmatureProxy extends IEventDispatcher`），本 trait 只保留骨架代理相关方法。
//! [`DbEventListener`] / [`ListenerId`] 从本模块 re-export 以保持旧调用点不变。

use crate::armature::ArmatureIdx;
use crate::event::{EventDispatcher, EventObject};
use std::collections::HashMap;

// 旧路径兼容：`dragonbones_core::armature::{DbEventListener, ListenerId}`。
pub use crate::event::{DbEventListener, ListenerId};

/// `IArmatureProxy extends IEventDispatcher`（IArmatureProxy.ts:36-92）。
///
/// 调用点（TS 实测）：`Armature.ts:288` `dbInit`（init 内）、`Armature.ts:148`
/// `dbClear`（_onClear 内）、`Armature.ts:386` `dbUpdate`（advanceTime 收尾解锁后）。
pub trait ArmatureProxy: EventDispatcher {
    /// 对齐 `dbInit(armature)`（IArmatureProxy.ts:40）。
    fn db_init(&mut self, armature: ArmatureIdx);
    /// 对齐 `dbClear()`（IArmatureProxy.ts:44）。
    fn db_clear(&mut self);
    /// 对齐 `dbUpdate()`（IArmatureProxy.ts:48）。
    fn db_update(&mut self);
    /// 对齐 `dispose(disposeProxy)`（IArmatureProxy.ts:69）。
    fn dispose(&mut self, dispose_proxy: bool);
}

/// Null 代理 = `jsdump.mjs` 的 `EvProxy`（tools/jsdump.mjs:44-56）：
/// 记住骨架索引、按事件名存监听器、派发时逐个回调；`dispose` no-op。
///（唯一有意偏离 EvProxy 处：`remove` 按句柄删单个而非 no-op——D-4.30。）
#[derive(Default)]
pub struct NullArmatureProxy {
    /// `dbInit` 写入的骨架索引（对齐 EvProxy `_armature`）。
    pub armature: Option<ArmatureIdx>,
    listeners: HashMap<String, Vec<(ListenerId, DbEventListener)>>,
}

impl NullArmatureProxy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventDispatcher for NullArmatureProxy {
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
        let id = ListenerId::next();
        self.listeners
            .entry(ty.to_string())
            .or_default()
            .push((id, listener));
        id
    }

    fn remove_db_event_listener(&mut self, id: ListenerId) {
        if id.is_sentinel() {
            return; // 未发放哨兵
        }
        for listeners in self.listeners.values_mut() {
            if let Some(pos) = listeners
                .iter()
                .position(|(listener_id, _)| *listener_id == id)
            {
                drop(listeners.remove(pos)); // 只删这一个、随 drop 回收（D-4.30）
                return;
            }
        }
    }

    fn has_db_event_listener(&mut self, ty: &str) -> bool {
        self.listeners.get(ty).is_some_and(|l| !l.is_empty())
    }

    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
        if let Some(listeners) = self.listeners.get(ty) {
            for (_, listener) in listeners {
                listener(event);
            }
        }
    }
}

impl ArmatureProxy for NullArmatureProxy {
    fn db_init(&mut self, armature: ArmatureIdx) {
        self.armature = Some(armature);
    }

    fn db_clear(&mut self) {
        self.armature = None;
        self.listeners.clear();
    }

    fn db_update(&mut self) {}

    fn dispose(&mut self, _dispose_proxy: bool) {}
}

#[cfg(test)]
mod tests {
    use super::{EventDispatcher, ListenerId, NullArmatureProxy};
    use crate::event::EventObject;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// D-4.30 句柄语义：add 发唯一句柄、remove 按句柄删**单个**（同类型另一监听器
    /// 不受影响）、未知句柄静默忽略；`has`/`dispatch` 语义不变。
    #[test]
    fn listener_handle_removes_single() {
        let mut proxy = NullArmatureProxy::new();
        let hits_a = Arc::new(AtomicUsize::new(0));
        let hits_b = Arc::new(AtomicUsize::new(0));
        let (cb_a, cb_b) = (Arc::clone(&hits_a), Arc::clone(&hits_b));

        let id_a = proxy.add_db_event_listener(
            "start",
            Box::new(move |_| {
                cb_a.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let id_b = proxy.add_db_event_listener(
            "start",
            Box::new(move |_| {
                cb_b.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert_ne!(id_a, id_b, "每次 add 发放唯一句柄");
        assert!(proxy.has_db_event_listener("start"));
        assert!(!proxy.has_db_event_listener("complete"));

        let event = EventObject::default();
        proxy.dispatch_db_event("start", &event);
        assert_eq!(
            (hits_a.load(Ordering::SeqCst), hits_b.load(Ordering::SeqCst)),
            (1, 1)
        );

        proxy.remove_db_event_listener(id_a);
        proxy.dispatch_db_event("start", &event);
        assert_eq!(
            (hits_a.load(Ordering::SeqCst), hits_b.load(Ordering::SeqCst)),
            (1, 2),
            "只删 id_a，id_b 仍在"
        );

        proxy.remove_db_event_listener(id_b);
        assert!(!proxy.has_db_event_listener("start"));

        // 未知句柄 → 静默忽略、不 panic（含从未 add 的句柄）
        proxy.remove_db_event_listener(ListenerId::next());
    }
}
