//! `event/IEventDispatcher.ts`（109 行）。
//!
//! 官方 `IArmatureProxy extends IEventDispatcher`；阶段 5 起 Rust 也拆成基础
//! [`EventDispatcher`] + `ArmatureProxy: EventDispatcher`，全局 `eventManager`
//! 只需要实现前者。
//!
//! D-4.30（监听器删除语义）：官方 `removeDBEventListener(type, listener, thisObject)`
//! 按三元组删**单个**监听器（IEventDispatcher.ts:107），而 Rust `Box<dyn Fn>`
//! 无身份可比（闭包不可 `==`）——故句柄化：`add` 返回 [`ListenerId`]、`remove`
//! 按句柄删单个（已删/未发放静默忽略）。
//!
//! [`ListenerId`] 的字段保持私有，发号统一走 [`ListenerId::next`]（进程内唯一）。
//! 这样 core 之外的适配层（RecordingProxy、阶段 7 的 Bevy 实现）也能实现
//! `EventDispatcher` 而不必接触内部字段。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::EventObject;

/// 事件监听器（IEventDispatcher.ts 的 `Function + thisObject` → Rust 闭包捕获，D-3.4）。
pub type DbEventListener = Box<dyn Fn(&EventObject)>;

/// 监听器句柄（D-4.30）：`Box<dyn Fn>` 无身份可比 → add 时发号、remove 按号删单个。
///
/// `ListenerId(0)` 恒为“从未发放”哨兵（`next` 从 1 开始）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

impl ListenerId {
    /// 发放一个进程内唯一的句柄。
    ///
    /// 官方由对象池/实例内部计数；Rust 交给所有 `EventDispatcher` 实现共用，
    /// 避免“字段私有导致外部实现无法发号”的问题（Bevy 适配层同样需要）。
    #[must_use]
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// 是否为“从未发放”的哨兵句柄。
    #[must_use]
    pub(crate) fn is_sentinel(self) -> bool {
        self.0 == 0
    }
}

/// `IEventDispatcher`（IEventDispatcher.ts:43-108）。
pub trait EventDispatcher {
    /// 对齐 `addDBEventListener(type, listener, thisObject)`（IEventDispatcher.ts:90）。
    ///
    /// TS 无返回值；Rust 返回句柄供 [`EventDispatcher::remove_db_event_listener`] 用。
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId;
    /// 对齐 `removeDBEventListener(type, listener, thisObject)`（IEventDispatcher.ts:107）。
    ///
    /// D-4.30：官方按 `(type, listener, thisObject)` 三元组删单个；Rust 按句柄删单个
    /// （已删/未发放 → 静默忽略）。
    fn remove_db_event_listener(&mut self, id: ListenerId);
    /// 对齐 `hasDBEventListener(type)`（IEventDispatcher.ts:56）。
    fn has_db_event_listener(&mut self, ty: &str) -> bool;
    /// 对齐 `dispatchDBEvent(type, eventObject)`（IEventDispatcher.ts:73）。
    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject);
}
