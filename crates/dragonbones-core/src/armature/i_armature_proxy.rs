//! `armature/IArmatureProxy.ts`（93 行）+ `event/IEventDispatcher.ts`（109 行）。
//!
//! D-3.4：两个接口合并成一个 trait [`ArmatureProxy`]（Rust 侧 `thisObject`
//! 用闭包捕获表达，`EventStringType` 用 `&str`），阶段 3 交 [`NullArmatureProxy`]
//! （= 阶段 0 `jsdump.mjs` 的 `EvProxy` 语义），阶段 7 换 Bevy 实现。
//!
//! D-4.30（监听器删除语义）：官方 `removeDBEventListener(type, listener, thisObject)`
//! 按三元组删**单个**监听器（IEventDispatcher.ts:107），jsdump 的 EvProxy 是 no-op
//! （tools/jsdump.mjs:54），而 Rust `Box<dyn Fn>` **无身份可比**（闭包不可 `==`）——
//! 故句柄化：`add` 返回 [`ListenerId`]、`remove` 按句柄删单个。与上述两者均不同
//!（既非三元组匹配、也非 no-op），阶段 5 事件系统按此接口接。

use crate::armature::ArmatureIdx;
use crate::event::EventObject;
use std::collections::HashMap;

/// 事件监听器（IEventDispatcher.ts 的 `Function + thisObject` → Rust 闭包捕获，D-3.4）。
pub type DbEventListener = Box<dyn Fn(&EventObject)>;

/// 监听器句柄（D-4.30）：`Box<dyn Fn>` 无身份可比 → add 时发号、remove 按号删单个。
///
/// 字段私有（外部不可构造）；内部自 1 递增发放，`ListenerId(0)` 恒为"从未发放"哨兵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

/// `IArmatureProxy extends IEventDispatcher`（IArmatureProxy.ts:36-92 / IEventDispatcher.ts:43-108）。
///
/// 调用点（TS 实测）：`Armature.ts:288` `dbInit`（init 内）、`Armature.ts:148`
/// `dbClear`（_onClear 内）、`Armature.ts:386` `dbUpdate`（advanceTime 收尾解锁后）。
pub trait ArmatureProxy {
    /// 对齐 `dbInit(armature)`（IArmatureProxy.ts:40）。
    fn db_init(&mut self, armature: ArmatureIdx);
    /// 对齐 `dbClear()`（IArmatureProxy.ts:44）。
    fn db_clear(&mut self);
    /// 对齐 `dbUpdate()`（IArmatureProxy.ts:48）。
    fn db_update(&mut self);
    /// 对齐 `dispose(disposeProxy)`（IArmatureProxy.ts:69）。
    fn dispose(&mut self, dispose_proxy: bool);
    /// 对齐 `addDBEventListener(type, listener, thisObject)`（IEventDispatcher.ts:90）。
    ///
    /// TS 无返回值；Rust 返回句柄供 [`ArmatureProxy::remove_db_event_listener`] 用
    /// （D-4.30 句柄化，见模块头）。
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId;
    /// 对齐 `removeDBEventListener(type, listener, thisObject)`（IEventDispatcher.ts:107）。
    ///
    /// D-4.30：官方按 `(type, listener, thisObject)` 三元组删单个、jsdump 为 no-op
    ///（tools/jsdump.mjs:54），Rust 按句柄删单个（已删/未发放 → 静默忽略）。
    fn remove_db_event_listener(&mut self, id: ListenerId);
    /// 对齐 `hasDBEventListener(type)`（IEventDispatcher.ts:56）。
    fn has_db_event_listener(&mut self, ty: &str) -> bool;
    /// 对齐 `dispatchDBEvent(type, eventObject)`（IEventDispatcher.ts:73）。
    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject);
}

/// Null 代理 = `jsdump.mjs` 的 `EvProxy`（tools/jsdump.mjs:44-56）：
/// 记住骨架索引、按事件名存监听器、派发时逐个回调；`dispose` no-op。
///（唯一有意偏离 EvProxy 处：`remove` 按句柄删单个而非 no-op——D-4.30。）
#[derive(Default)]
pub struct NullArmatureProxy {
    /// `dbInit` 写入的骨架索引（对齐 EvProxy `_armature`）。
    pub armature: Option<ArmatureIdx>,
    listeners: HashMap<String, Vec<(ListenerId, DbEventListener)>>,
    /// D-4.30 句柄发放计数（自 1 起；0 = 未发放哨兵）。
    next_listener_id: u64,
}

impl NullArmatureProxy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 发放下一个句柄（自 1 递增，0 恒为哨兵）。
    fn take_listener_id(&mut self) -> ListenerId {
        self.next_listener_id += 1;
        ListenerId(self.next_listener_id)
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

    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
        let id = self.take_listener_id();
        self.listeners
            .entry(ty.to_string())
            .or_default()
            .push((id, listener));
        id
    }

    fn remove_db_event_listener(&mut self, id: ListenerId) {
        if id.0 == 0 {
            return; // 未发放哨兵
        }
        for listeners in self.listeners.values_mut() {
            if let Some(pos) = listeners.iter().position(|(listener_id, _)| *listener_id == id) {
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

#[cfg(test)]
mod tests {
    use super::{ArmatureProxy, ListenerId, NullArmatureProxy};
    use crate::event::EventObject;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// D-4.30 句柄语义：add 发唯一句柄、remove 按句柄删**单个**（同类型另一监听器
    /// 不受影响）、未知/未发放句柄静默忽略；`has`/`dispatch` 语义不变。
    #[test]
    fn listener_handle_removes_single() {
        let mut proxy = NullArmatureProxy::new();
        let hits_a = Arc::new(AtomicUsize::new(0));
        let hits_b = Arc::new(AtomicUsize::new(0));
        let (cb_a, cb_b) = (Arc::clone(&hits_a), Arc::clone(&hits_b));

        let id_a = proxy.add_db_event_listener("start", Box::new(move |_| {
            cb_a.fetch_add(1, Ordering::SeqCst);
        }));
        let id_b = proxy.add_db_event_listener("start", Box::new(move |_| {
            cb_b.fetch_add(1, Ordering::SeqCst);
        }));
        assert_ne!(id_a, id_b, "每次 add 发放唯一句柄");
        assert!(proxy.has_db_event_listener("start"));
        assert!(!proxy.has_db_event_listener("complete"));

        let event = EventObject::default();
        proxy.dispatch_db_event("start", &event);
        assert_eq!((hits_a.load(Ordering::SeqCst), hits_b.load(Ordering::SeqCst)), (1, 1));

        proxy.remove_db_event_listener(id_a);
        proxy.dispatch_db_event("start", &event);
        assert_eq!(
            (hits_a.load(Ordering::SeqCst), hits_b.load(Ordering::SeqCst)),
            (1, 2),
            "只删 id_a，id_b 仍在"
        );

        proxy.remove_db_event_listener(id_b);
        assert!(!proxy.has_db_event_listener("start"));

        // 未知句柄（含未发放哨兵 0）→ 静默忽略、不 panic
        proxy.remove_db_event_listener(ListenerId(0));
        proxy.remove_db_event_listener(ListenerId(u64::MAX));
    }
}
