//! 对象池语义测试（P0-2）：复用 / hash_code 保留 / 缓冲延迟回收 / 上限。

use dragonbones_core::core::base_object::{
    BaseObject, ObjectBuffer, PoolExt, Poolable, borrow_object, pool_clear, pool_len,
    pool_set_max_count,
};

#[derive(Debug, Default)]
struct Dummy {
    value: i32,
    base: BaseObject,
}

impl Poolable for Dummy {
    fn on_clear(&mut self) {
        self.value = 0;
    }
}

#[test]
fn borrow_reuses_and_preserves_hash_code() {
    pool_clear::<Dummy>();

    let mut object = borrow_object::<Dummy>();
    object.value = 42;
    let code = object.base.hash_code();
    object.return_to_pool();

    let reused = borrow_object::<Dummy>();
    assert_eq!(reused.base.hash_code(), code, "复用的对象应保留 hash_code");
    assert_eq!(reused.value, 0, "复用前应已 on_clear");
}

#[test]
fn fresh_objects_get_distinct_hash_codes() {
    let a = Dummy::default();
    let b = Dummy::default();
    assert_ne!(a.base.hash_code(), b.base.hash_code());
}

#[test]
fn fresh_and_pooled_objects_differ() {
    pool_clear::<Dummy>();

    let pooled = borrow_object::<Dummy>();
    let pooled_code = pooled.base.hash_code();
    pooled.return_to_pool();

    let reused = borrow_object::<Dummy>();
    let fresh = Dummy::default();

    assert_eq!(reused.base.hash_code(), pooled_code);
    assert_ne!(fresh.base.hash_code(), reused.base.hash_code());
}

#[test]
fn set_max_count_caps_pool() {
    pool_clear::<Dummy>();
    pool_set_max_count::<Dummy>(2);

    for _ in 0..5 {
        let object = borrow_object::<Dummy>();
        object.return_to_pool();
    }

    assert!(pool_len::<Dummy>() <= 2, "池上限应为 2");

    pool_set_max_count::<Dummy>(3000);
    pool_clear::<Dummy>();
}

#[test]
fn object_buffer_defers_return_until_drain() {
    pool_clear::<Dummy>();

    let mut buffer = ObjectBuffer::new();
    buffer.push(Dummy {
        value: 7,
        base: BaseObject::new(),
    });

    assert_eq!(buffer.len(), 1);
    assert_eq!(pool_len::<Dummy>(), 0, "drain 前不应回池");

    buffer.drain();

    assert!(buffer.is_empty());
    assert_eq!(pool_len::<Dummy>(), 1, "drain 后应回池");

    let reused = borrow_object::<Dummy>();
    assert_eq!(reused.value, 0, "回池时应已 on_clear");

    pool_clear::<Dummy>();
}
