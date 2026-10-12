//! 对象池语义，对齐 `core/BaseObject.ts`。
//!
//! JS 用 `String(constructor)` 作为池的键，Rust 用 [`std::any::TypeId`]。
//! 池是 `thread_local`（core 无并发），池内元素以 `Box<dyn Any>` 存储，
//! 借出时按类型 downcast 回具体类型。
//!
//! 与 TS 的语义差异（由 Rust 所有权天然保证，见 `docs/model_mapping.md`）：
//! - `_isInPool` 重复入池检测：move 语义下同一个对象不可能被两次归还，编译期消除。
//! - `bufferObject` 的重复 push 检测：同理，`push` 消费所有权，无法重复 push。

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// TS `BaseObject._defaultMaxCount`。
pub const DEFAULT_MAX_COUNT: usize = 3000;

/// 全局自增的 `hashCode` 计数器（对齐 `BaseObject._hashCode`）。
static HASH_CODE: AtomicU64 = AtomicU64::new(0);

/// 所有可池化对象的“基对象”：唯一标识号。
///
/// TS 里 `hashCode` 在构造时分配、池复用时不重发；Rust 通过 [`Default`]
/// 分配新号，并保证 [`Poolable::on_clear`] 不修改它。
///
/// TS 的 `_isInPool` 字段在 Rust 的 move 语义下无需存在（同一对象不可能被两次归还），
/// 属编译期消除。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BaseObject {
    hash_code: u64,
}

impl BaseObject {
    /// 分配一个新的唯一标识号。
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash_code: HASH_CODE.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// 对齐 TS `BaseObject.hashCode`（readonly）。
    #[must_use]
    pub fn hash_code(&self) -> u64 {
        self.hash_code
    }
}

impl Default for BaseObject {
    fn default() -> Self {
        Self::new()
    }
}

/// 可池化对象：实现 [`Poolable::on_clear`] 即对齐 TS 子类的 `_onClear`。
///
/// `on_clear` 必须把除 [`BaseObject`] 标识以外的全部字段重置为 TS 默认值。
pub trait Poolable {
    /// 对应 TS 的 `_onClear`。
    fn on_clear(&mut self);
}

/// 扩展 trait：让对象能以 `obj.return_to_pool()` 的形态消费自身，杜绝重复归还。
pub trait PoolExt: Poolable + Sized + 'static {
    /// 清除该实例并返还对象池（对应 `BaseObject.returnToPool`）。
    fn return_to_pool(self);
}

impl<T: Poolable + 'static> PoolExt for T {
    fn return_to_pool(self) {
        pool_return(self);
    }
}

struct TypePool {
    objects: Vec<Box<dyn Any>>,
    max_count: usize,
}

impl Default for TypePool {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            max_count: DEFAULT_MAX_COUNT,
        }
    }
}

thread_local! {
    static POOLS: RefCell<HashMap<TypeId, TypePool>> = RefCell::new(HashMap::new());
}

fn with_pool<R>(key: TypeId, f: impl FnOnce(&mut TypePool) -> R) -> R {
    POOLS.with(|pools| {
        let mut pools = pools.borrow_mut();
        let pool = pools.entry(key).or_default();
        f(pool)
    })
}

/// 从对象池借出实例，对齐 TS `BaseObject.borrowObject`。
///
/// 池中有则复用（归还时已清空），否则 `T::default()` 并 `on_clear()` 一次。
pub fn borrow_object<T: Poolable + Default + 'static>() -> T {
    let key = TypeId::of::<T>();
    let recycled = with_pool(key, |pool| pool.objects.pop());

    if let Some(boxed) = recycled {
        return *boxed.downcast::<T>().expect("object pool type mismatch");
    }

    let mut object = T::default();
    object.on_clear();
    object
}

/// 清除实例并返还对象池，对齐 TS `BaseObject.returnToPool`。
pub fn pool_return<T: Poolable + 'static>(mut object: T) {
    object.on_clear();

    let key = TypeId::of::<T>();
    let boxed: Box<dyn Any> = Box::new(object);
    with_pool(key, |pool| {
        if pool.objects.len() < pool.max_count {
            pool.objects.push(boxed);
        }
    });
}

/// 清除某个类型的全部缓存实例，对齐 TS `BaseObject.clearPool`。
pub fn pool_clear<T: 'static>() {
    let key = TypeId::of::<T>();
    with_pool(key, |pool| pool.objects.clear());
}

/// 设置某个类型对象池的最大缓存数量，对齐 TS `BaseObject.setMaxCount`。
pub fn pool_set_max_count<T: 'static>(max_count: usize) {
    let key = TypeId::of::<T>();
    with_pool(key, |pool| {
        pool.max_count = max_count;
        if pool.objects.len() > max_count {
            pool.objects.truncate(max_count);
        }
    });
}

/// 当前某个类型对象池内的实例数量（测试辅助）。
#[must_use]
pub fn pool_len<T: 'static>() -> usize {
    let key = TypeId::of::<T>();
    with_pool(key, |pool| pool.objects.len())
}

/// 缓冲回收队列，对齐 `DragonBones.bufferObject` / `_objects`。
///
/// 对象先暂存、在下一帧 `drain()` 时才统一返还对象池（D4：行为语义必须保留）。
#[derive(Default)]
pub struct ObjectBuffer {
    entries: Vec<BufferedEntry>,
}

struct BufferedEntry {
    object: Box<dyn Any>,
    /// 类型擦除的“返还器”，保存了具体的 `T`，从而能在 drain 时 `on_clear` + 回池。
    returner: fn(Box<dyn Any>),
}

impl ObjectBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 暂存一个对象（对应 `DragonBones.bufferObject`）。
    pub fn push<T: Poolable + 'static>(&mut self, object: T) {
        self.entries.push(BufferedEntry {
            object: Box::new(object),
            returner: buffer_return::<T>,
        });
    }

    /// 统一返还全部暂存对象（对应 `advanceTime` 开头对 `_objects` 的处理）。
    pub fn drain(&mut self) {
        for entry in self.entries.drain(..) {
            (entry.returner)(entry.object);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn buffer_return<T: Poolable + 'static>(boxed: Box<dyn Any>) {
    let object = *boxed.downcast::<T>().expect("object buffer type mismatch");
    pool_return(object);
}
