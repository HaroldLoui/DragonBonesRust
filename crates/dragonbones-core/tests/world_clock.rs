//! 阶段 4 WorldClock 调度验收（任务 1 测试矩阵 + 工作簿 §1.7 测试卡）。
//!
//! 覆盖 timeScale=0.5/负缩放的实际成员 dt、事件快照替换语义、Slot 旧/新 child
//! 的 clock 继承、clear 后重新 attach、`advance_clock` 公开入口等价性与非法
//! ClockId 错误。失败信息全部中文带上下文。

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::ir_snapshot::{asset_root, tex_stem};
use dragonbones_core::armature::{
    Armature, ArmatureIdx, ArmatureProxy, DbEventListener, DisplayHandle, ListenerId,
    NullArmatureProxy,
};
use dragonbones_core::animation::world_clock::{AnimatableHandle, ClockError, ClockId};
use dragonbones_core::event::EventObject;
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::transform::Transform;
use dragonbones_core::model::{
    ActionData, ArmatureData, DisplayData, DisplayKind, DragonBonesData, IrArrays,
};

struct CountingProxy {
    inner: NullArmatureProxy,
    advances: Arc<AtomicUsize>,
}

impl CountingProxy {
    fn new(advances: Arc<AtomicUsize>) -> Self {
        Self {
            inner: NullArmatureProxy::new(),
            advances,
        }
    }
}

impl ArmatureProxy for CountingProxy {
    fn db_init(&mut self, armature: ArmatureIdx) {
        self.inner.db_init(armature);
    }

    fn db_clear(&mut self) {
        self.inner.db_clear();
    }

    fn db_update(&mut self) {
        self.advances.fetch_add(1, Ordering::SeqCst);
        self.inner.db_update();
    }

    fn dispose(&mut self, dispose_proxy: bool) {
        self.inner.dispose(dispose_proxy);
    }

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
        self.inner.dispatch_db_event(ty, event);
    }
}

fn add_empty_armature(
    factory: &mut BaseFactory,
    advances: Arc<AtomicUsize>,
) -> ArmatureIdx {
    let idx = ArmatureIdx(factory.armatures.len() as u32);
    let name = format!("clock_test_{}", idx.0);
    let mut data = DragonBonesData::default();
    let mut armature_data = ArmatureData::default();
    armature_data.name.clone_from(&name);
    data.add_armature(armature_data);
    data.finalize_root(IrArrays::default());

    let armature = Armature::new(
        idx,
        Arc::new(data),
        name,
        Box::new(CountingProxy::new(advances)),
        DisplayHandle::Null,
    );
    factory.armatures.push(Some(armature));
    idx
}

fn advances(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

#[test]
fn global_advance_only_updates_explicit_clock_members() {
    let mut factory = BaseFactory::new();
    let a_count = Arc::new(AtomicUsize::new(0));
    let b_count = Arc::new(AtomicUsize::new(0));
    let a = add_empty_armature(&mut factory, Arc::clone(&a_count));
    let b = add_empty_armature(&mut factory, Arc::clone(&b_count));
    let root = factory.root_clock();

    factory.advance_time_with(0.1, 100.0);
    assert_eq!(advances(&a_count), 0);
    assert_eq!(advances(&b_count), 0);
    assert_eq!(factory.armature(a).clock(), None);

    factory.attach_armature(root, a).expect("attach A to root");
    factory.advance_time_with(0.1, 100.1);

    assert_eq!(advances(&a_count), 1);
    assert_eq!(advances(&b_count), 0);
    assert_eq!(factory.armature(a).clock(), Some(root));
    assert!(factory.clock_contains(root, AnimatableHandle::Armature(a)));
    assert!(!factory.clock_contains(root, AnimatableHandle::Armature(b)));
}

#[test]
fn nested_clock_scales_member_time_and_contains_follows_parent_chain() {
    let mut factory = BaseFactory::new();
    let count = Arc::new(AtomicUsize::new(0));
    let armature = add_empty_armature(&mut factory, Arc::clone(&count));
    let root = factory.root_clock();
    let child = factory.create_clock(5.0);

    factory.attach_clock(root, child).expect("attach child clock");
    factory
        .clock_mut(child)
        .expect("child clock")
        .time_scale = 2.0;
    factory
        .attach_armature(child, armature)
        .expect("attach Armature to child clock");

    factory.advance_time_with(0.25, 100.0);

    assert_eq!(factory.clock(root).expect("root").time, 0.25);
    assert_eq!(factory.clock(child).expect("child").time, 5.5);
    assert_eq!(advances(&count), 1);
    assert!(factory.clock_contains(child, AnimatableHandle::Armature(armature)));
    assert!(factory.clock_contains(root, AnimatableHandle::Armature(armature)));
    assert!(factory.clock_contains(root, AnimatableHandle::Clock(child)));
    assert!(!factory.clock_contains(child, AnimatableHandle::Clock(child)));
}

#[test]
fn zero_time_scale_updates_wall_clock_baseline_but_does_not_advance_members() {
    let mut factory = BaseFactory::new();
    let count = Arc::new(AtomicUsize::new(0));
    let armature = add_empty_armature(&mut factory, Arc::clone(&count));
    let root = factory.root_clock();
    factory
        .attach_armature(root, armature)
        .expect("attach member");
    factory.clock_mut(root).expect("root clock").time_scale = 0.0;

    factory.advance_time_with(0.25, 100.0);

    assert_eq!(factory.clock(root).expect("root clock").time, 0.0);
    assert_eq!(advances(&count), 0);

    factory.clock_mut(root).expect("root clock").time_scale = 1.0;
    factory.advance_time_with(-1.0, 100.2);

    assert!((factory.clock(root).expect("root clock").time - 0.2).abs() < 1e-12);
    assert_eq!(advances(&count), 1);
}

#[test]
fn moving_armature_between_clocks_detaches_old_membership() {
    let mut factory = BaseFactory::new();
    let armature = add_empty_armature(&mut factory, Arc::new(AtomicUsize::new(0)));
    let old_clock = factory.root_clock();
    let new_clock = factory.create_clock(0.0);

    factory
        .attach_armature(old_clock, armature)
        .expect("attach to old clock");
    factory
        .attach_armature(new_clock, armature)
        .expect("move to new clock");

    assert_eq!(factory.armature(armature).clock(), Some(new_clock));
    assert!(!factory.clock_contains(old_clock, AnimatableHandle::Armature(armature)));
    assert!(factory.clock_contains(new_clock, AnimatableHandle::Armature(armature)));
}

#[test]
fn clear_parent_clock_detaches_direct_members_but_keeps_child_clock_members() {
    let mut factory = BaseFactory::new();
    let armature = add_empty_armature(&mut factory, Arc::new(AtomicUsize::new(0)));
    let parent = factory.root_clock();
    let child = factory.create_clock(3.0);

    factory.attach_clock(parent, child).expect("nest clock");
    factory
        .attach_armature(child, armature)
        .expect("attach armature to child");
    factory.clear_clock(parent).expect("clear parent clock");

    assert_eq!(factory.clock(child).expect("child survives").time, 3.0);
    assert!(!factory.clock_contains(parent, AnimatableHandle::Clock(child)));
    assert!(factory.clock_contains(child, AnimatableHandle::Armature(armature)));
    assert_eq!(factory.armature(armature).clock(), Some(child));
}

#[test]
fn clear_clock_removes_all_direct_armature_back_references() {
    let mut factory = BaseFactory::new();
    let first = add_empty_armature(&mut factory, Arc::new(AtomicUsize::new(0)));
    let second = add_empty_armature(&mut factory, Arc::new(AtomicUsize::new(0)));
    let root = factory.root_clock();

    factory.attach_armature(root, first).expect("attach first");
    factory.attach_armature(root, second).expect("attach second");
    factory.clear_clock(root).expect("clear root members");

    assert_eq!(factory.armature(first).clock(), None);
    assert_eq!(factory.armature(second).clock(), None);
    assert!(!factory.clock_contains(root, AnimatableHandle::Armature(first)));
    assert!(!factory.clock_contains(root, AnimatableHandle::Armature(second)));
}

#[test]
fn dispose_detaches_armature_before_releasing_its_slot() {
    let mut factory = BaseFactory::new();
    let armature = add_empty_armature(&mut factory, Arc::new(AtomicUsize::new(0)));
    let root = factory.root_clock();
    factory
        .attach_armature(root, armature)
        .expect("attach before disposal");

    factory.dispose_armature(armature);

    assert!(factory.armatures[armature.index()].is_none());
    assert!(!factory.clock_contains(root, AnimatableHandle::Armature(armature)));
}

#[test]
fn global_advance_zero_flushes_events_without_advancing_members() {
    let mut factory = BaseFactory::new();
    let count = Arc::new(AtomicUsize::new(0));
    let armature = add_empty_armature(&mut factory, Arc::clone(&count));
    factory
        .attach_armature(factory.root_clock(), armature)
        .expect("attach member");
    factory.buffer_event(EventObject::default());
    factory.buffer_event(EventObject::default());

    factory.advance_time_with(0.0, 100.0);

    assert_eq!(advances(&count), 0);
    assert_eq!(factory.take_flushed_events().len(), 2);
    assert!(factory.take_flushed_events().is_empty());

    factory.advance_time_with(0.0, 100.1);
    assert!(factory.take_flushed_events().is_empty());
}

#[test]
fn attach_rejects_clock_cycles_without_changing_existing_relation() {
    let mut factory = BaseFactory::new();
    let root = factory.root_clock();
    let child = factory.create_clock(0.0);
    factory.attach_clock(root, child).expect("attach child");

    assert_eq!(factory.attach_clock(child, root), Err(ClockError::Cycle));
    assert!(factory.clock_contains(root, AnimatableHandle::Clock(child)));
    assert!(!factory.clock_contains(child, AnimatableHandle::Clock(root)));
}


const DRAGON_KEY: &str = "Dragon_ske";

fn build_animated_dragon(advances: Arc<AtomicUsize>) -> (BaseFactory, ArmatureIdx) {
    let ske = common::read_value(&asset_root().join(format!("{DRAGON_KEY}.json")));
    let tex = common::read_value(&asset_root().join(format!("{}.json", tex_stem(DRAGON_KEY))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, DRAGON_KEY)
        .unwrap_or_else(|| panic!("读 Dragon 素材失败：parse_dragon_bones_data 返回 None"));
    assert!(
        factory.parse_texture_atlas_data(&tex, DRAGON_KEY),
        "读 Dragon 图集失败：parse_texture_atlas_data 返回 false"
    );
    let idx = factory
        .build_armature(
            "Dragon",
            DRAGON_KEY,
            "",
            DRAGON_KEY,
            Box::new(CountingProxy::new(advances)),
        )
        .unwrap_or_else(|| panic!("build_armature(`Dragon`) 失败"));
    (factory, idx)
}

fn state_time(factory: &BaseFactory, armature: ArmatureIdx, state: u32) -> f64 {
    let state_id = factory
        .armature(armature)
        .animation
        .states()
        .find(|(_, candidate)| candidate.name == "walk")
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("walk 状态不存在（state slot {state}）"));
    factory
        .armature(armature)
        .animation
        .state(state_id)
        .expect("walk state")
        .current_time()
}

/// 任务 1 矩阵「timeScale 0/0.5/2：时间累计与传入成员 dt 符合 TS」+ F4.7。
///
/// `ArmatureProxy::db_update()` 没有 dt 形参，CountingProxy 无法直接记录成员 dt；
/// 这里用成员内 `AnimationState.current_time` 的真实增量作为成员收到的 dt 观测值，
/// 同时用 CountingProxy 记录推进次数。负 timeScale 下成员 dt 为负，Animation 层
/// 按 F4.7 取绝对值，因此 state 时间仍正向推进（这正是官方分工的可观察结果）。
#[test]
fn member_receives_scaled_signed_dt_for_fractional_and_negative_time_scale() {
    let advances_count = Arc::new(AtomicUsize::new(0));
    let (mut factory, armature) = build_animated_dragon(Arc::clone(&advances_count));
    let root = factory.root_clock();
    factory
        .attach_armature(root, armature)
        .expect("挂接 Dragon 到 root clock");
    assert!(
        factory
            .armature_mut(armature)
            .animation
            .goto_and_play_by_time("walk", 0.0, -1)
            .is_some(),
        "Dragon.walk 应存在"
    );
    factory
        .clock_mut(root)
        .expect("root clock")
        .time_scale = 0.5;

    let mut now = 100.0;
    // build 收尾 `advance_one(0.0)` 也会经过 proxy.db_update，先记录基线。
    let base_advances = advances(&advances_count);
    let mut previous_clock = factory.clock(root).expect("root clock").time;
    let mut previous_state = state_time(&factory, armature, 0);
    for frame in 0..4 {
        factory.advance_time_with(0.1, now);
        now += 0.1;
        let clock = factory.clock(root).expect("root clock").time;
        let state = state_time(&factory, armature, 0);
        let clock_delta = clock - previous_clock;
        let state_delta = state - previous_state;
        assert!(
            (clock_delta - 0.05).abs() <= 1e-12,
            "timeScale=0.5 第 {frame} 帧：clock.time 增量应为 0.05，实际 {clock_delta}"
        );
        assert!(
            (state_delta - 0.05).abs() <= 1e-12 * (1.0 + state.abs()),
            "timeScale=0.5 第 {frame} 帧：成员实际收到的 dt 应为 0.05，\
             以 AnimationState.current_time 增量观测为 {state_delta}"
        );
        assert_eq!(
            advances_count.load(Ordering::SeqCst),
            base_advances + frame + 1,
            "timeScale=0.5 第 {frame} 帧：成员应恰好被推进一次"
        );
        previous_clock = clock;
        previous_state = state;
    }

    // 负缩放：WorldClock 仍按 `time -= passedTime` 增加累计时间，成员收到 -0.05；
    // Animation 层取正（F4.7），故 state 时间继续正向推进 0.05。
    factory
        .clock_mut(root)
        .expect("root clock")
        .time_scale = -0.5;
    for frame in 0..2 {
        factory.advance_time_with(0.1, now);
        now += 0.1;
        let clock = factory.clock(root).expect("root clock").time;
        let state = state_time(&factory, armature, 0);
        let clock_delta = clock - previous_clock;
        let state_delta = state - previous_state;
        assert!(
            (clock_delta - 0.05).abs() <= 1e-12,
            "timeScale=-0.5 第 {frame} 帧：官方 `time -= passedTime` 应使 clock.time 增加 0.05，\
             实际 {clock_delta}"
        );
        assert!(
            (state_delta - 0.05).abs() <= 1e-12 * (1.0 + state.abs()),
            "timeScale=-0.5 第 {frame} 帧：成员 dt 为 -0.05，Animation 取正后应以 0.05 推进，\
             实际 state 增量 {state_delta}"
        );
        assert_eq!(
            advances_count.load(Ordering::SeqCst),
            base_advances + 4 + frame + 1,
            "timeScale=-0.5 第 {frame} 帧：成员应恰好被推进一次"
        );
        previous_clock = clock;
        previous_state = state;
    }
}

/// S4-Event：flush 每次用本帧 pending 整体替换上一帧快照；未 take 的旧快照
/// 也会在下一次 flush 时被无事件帧替换，且同帧重复 take 第二次为空。
#[test]
fn event_snapshot_is_replaced_even_when_previous_batch_was_not_taken() {
    let mut factory = BaseFactory::new();

    factory.buffer_event(EventObject {
        event_type: EventObject::FRAME_EVENT.to_owned(),
        name: "first".to_owned(),
        ..EventObject::default()
    });
    factory.buffer_event(EventObject {
        event_type: EventObject::FRAME_EVENT.to_owned(),
        name: "second".to_owned(),
        ..EventObject::default()
    });
    factory.advance_time_with(0.1, 100.0);

    let first = factory.take_flushed_events();
    assert_eq!(
        first.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["first", "second"],
        "同值/不同值事件都应保留入队顺序（S4-Event 不做内容去重）"
    );
    assert!(
        factory.take_flushed_events().is_empty(),
        "同一帧第 2 次 take_flushed_events() 必须为空"
    );

    // 第二帧产生事件但**不 take**，第三帧无事件 → 旧快照必须被空快照替换。
    factory.buffer_event(EventObject {
        event_type: EventObject::FRAME_EVENT.to_owned(),
        name: "stale".to_owned(),
        ..EventObject::default()
    });
    factory.advance_time_with(0.1, 100.1);
    factory.advance_time_with(0.1, 100.2);
    assert!(
        factory.take_flushed_events().is_empty(),
        "无事件帧必须用空快照替换上一帧（即使上一帧未被 take）"
    );
}

const CHILD_A: &str = "ClockChildA";
const CHILD_B: &str = "ClockChildB";

/// 注入两个最小子骨架显示帧，返回 `(槽身份, 初始 displayIndex, 切换目标 displayIndex)`。
fn child_display(name: &str, parent: &str) -> DisplayData {
    DisplayData {
        name: name.to_owned(),
        path: name.to_owned(),
        transform: Transform::default(),
        parent: parent.to_owned(),
        kind: DisplayKind::Armature {
            inherit_animation: true,
            actions: Vec::<ActionData>::new(),
            armature: Some(name.to_owned()),
        },
    }
}

/// 任务 1 矩阵「根挂钟与 child 切换」+ F4.25：Slot 换显示导致旧 child detach、
/// 新 child 继承父骨架当前 clock。
#[test]
fn slot_display_switch_detaches_old_child_and_new_child_inherits_parent_clock() {
    let ske = common::read_value(&asset_root().join(format!("{DRAGON_KEY}.json")));
    let tex = common::read_value(&asset_root().join(format!("{}.json", tex_stem(DRAGON_KEY))));
    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, DRAGON_KEY)
        .expect("parse Dragon");
    assert!(factory.parse_texture_atlas_data(&tex, DRAGON_KEY));

    // 注入前后都通过运行期 display_frames 定位目标槽，避免依赖 sorted_slots 位置。
    let (initial_index, other_index) = {
        let arc = factory.data_map.get_mut(DRAGON_KEY).expect("Dragon data");
        let data = Arc::get_mut(arc).expect("DragonBonesData 独占");
        for name in [CHILD_A, CHILD_B] {
            data.add_armature(ArmatureData {
                name: name.to_owned(),
                frame_rate: 24.0,
                ..ArmatureData::default()
            });
        }
        let armature_data = data.armatures.get_mut("Dragon").expect("Dragon armature");
        let skin_name = armature_data.default_skin.clone().expect("default skin");
        let ArmatureData {
            slots,
            skins,
            sorted_slots,
            ..
        } = armature_data;
        let skin = skins.get_mut(&skin_name).expect("skin");
        let chosen_name = sorted_slots
            .iter()
            .find(|name| {
                slots.get(*name).is_some()
                    && skin
                        .displays
                        .get(*name)
                        .is_some_and(|displays| !displays.is_empty())
            })
            .cloned()
            .expect("找不到含显示列表的槽注入双子骨架");
        // 固定初始显示帧为 0，再扩到两帧：0 = ChildA，1 = ChildB。
        slots
            .get_mut(&chosen_name)
            .expect("chosen slot data")
            .display_index = 0;
        let displays = skin.displays.get_mut(&chosen_name).expect("slot displays");
        if displays.len() < 2 {
            displays.resize(2, None);
        }
        displays[0] = Some(child_display(CHILD_A, &skin_name));
        displays[1] = Some(child_display(CHILD_B, &skin_name));
        (0, 1)
    };

    let parent = factory
        .build_armature(
            "Dragon",
            DRAGON_KEY,
            "",
            DRAGON_KEY,
            Box::new(NullArmatureProxy::new()),
        )
        .expect("build Dragon");

    // 用运行期 display_frames 找到两个 child 以及当前显示者。
    let (child_a, child_b, current_child, slot_identity) = {
        let armature = factory.armature(parent);
        let slot = armature
            .slots
            .iter()
            .find(|slot| {
                slot.display_frames
                    .get(usize::try_from(initial_index).unwrap_or(usize::MAX))
                    .is_some_and(|frame| {
                        matches!(frame.display, Some(DisplayHandle::Armature(_)))
                    })
            })
            .unwrap_or_else(|| panic!("找不到注入双 child 的槽"));
        let index = |name: &str| {
            slot.display_frames
                .iter()
                .find_map(|frame| match frame.display {
                    Some(DisplayHandle::Armature(idx))
                        if factory.armature(idx).armature_name == name =>
                    {
                        Some(idx)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("display_frames 中找不到子骨架 `{name}`"))
        };
        let child_a = index(CHILD_A);
        let child_b = index(CHILD_B);
        let current = slot
            .child_armature
            .unwrap_or_else(|| panic!("初始显示的子骨架未接线到 child_armature"));
        (child_a, child_b, current, slot.idx)
    };

    let root = factory.root_clock();
    factory
        .attach_armature(root, parent)
        .expect("父骨架挂 root clock");
    assert_eq!(
        factory.armature(current_child).clock(),
        Some(root),
        "初始 child 应随父骨架继承 root clock"
    );

    {
        let armature = factory.armature_mut(parent);
        let slot = armature
            .slots
            .iter_mut()
            .find(|slot| slot.idx == slot_identity)
            .expect("注入槽");
        slot.set_display_index(other_index, false);
    }
    factory.advance_time_with(1.0 / 24.0, 100.0);

    let (old_child, new_child) = if current_child == child_a {
        (child_a, child_b)
    } else {
        (child_b, child_a)
    };
    assert_eq!(
        factory.armature(old_child).clock(),
        None,
        "旧 child `{}` 应在显示切换后从父 clock 摘除",
        factory.armature(old_child).armature_name
    );
    assert!(
        !factory.clock_contains(root, AnimatableHandle::Armature(old_child)),
        "旧 child 不应再是 root clock 成员"
    );
    assert_eq!(
        factory.armature(new_child).clock(),
        Some(root),
        "新 child `{}` 应继承父骨架当前 clock",
        factory.armature(new_child).armature_name
    );
    assert!(
        factory.clock_contains(root, AnimatableHandle::Armature(new_child)),
        "新 child 应成为 root clock 成员"
    );
    assert_eq!(
        factory.armature(old_child).parent,
        None,
        "旧 child 的 parent 回指应清空"
    );
    assert_eq!(
        factory.armature(new_child).parent.map(|value| value.0),
        Some(parent),
        "新 child 的 parent 应回指父骨架"
    );
}

/// 任务 1 矩阵「clear 多成员 → 之后可重新 attach」。
#[test]
fn clear_then_reattach_member_advances_again() {
    let mut factory = BaseFactory::new();
    let count = Arc::new(AtomicUsize::new(0));
    let armature = add_empty_armature(&mut factory, Arc::clone(&count));
    let root = factory.root_clock();

    factory
        .attach_armature(root, armature)
        .expect("首次挂接");
    factory.clear_clock(root).expect("clear root");
    assert_eq!(factory.armature(armature).clock(), None);
    factory.advance_time_with(0.1, 100.0);
    assert_eq!(
        advances(&count),
        0,
        "clear 后成员不应再被推进"
    );

    factory
        .attach_armature(root, armature)
        .expect("clear 后重新挂接");
    factory.advance_time_with(0.1, 100.1);
    assert_eq!(
        advances(&count),
        1,
        "重新挂接后成员应再次参与推进"
    );
}

/// 任务 1 矩阵「嵌套 clock/contains」+ 公开入口 `advance_clock(id, dt)`：
/// 直接推进独立 clock 与把它挂在 root 下再由 `advance_time` 推进等价；
/// 非法 ClockId 返回 `ClockError::InvalidClockId`。
#[test]
fn advance_clock_matches_nested_root_member_and_rejects_invalid_id() {
    let mut factory = BaseFactory::new();
    let nested_count = Arc::new(AtomicUsize::new(0));
    let direct_count = Arc::new(AtomicUsize::new(0));
    let nested_armature = add_empty_armature(&mut factory, Arc::clone(&nested_count));
    let direct_armature = add_empty_armature(&mut factory, Arc::clone(&direct_count));
    let root = factory.root_clock();
    let nested = factory.create_clock(5.0);
    let direct = factory.create_clock(0.0);
    factory.attach_clock(root, nested).expect("嵌套 clock");
    factory
        .attach_armature(nested, nested_armature)
        .expect("挂 nested 成员");
    factory
        .attach_armature(direct, direct_armature)
        .expect("挂 direct 成员");

    factory.advance_time_with(0.25, 100.0);
    factory
        .advance_clock(direct, 0.25)
        .expect("advance_clock(direct) 应成功");

    let nested_time = factory.clock(nested).expect("nested clock").time;
    let direct_time = factory.clock(direct).expect("direct clock").time;
    assert!(
        (nested_time - 5.25).abs() <= 1e-12,
        "作为 root 嵌套成员的 clock 应从 5.0 推进到 5.25，实际 {nested_time}"
    );
    assert!(
        (direct_time - 0.25).abs() <= 1e-12,
        "advance_clock 直接推进的 clock 应从 0.0 推进到 0.25，实际 {direct_time}"
    );
    assert_eq!(
        advances(&nested_count),
        1,
        "嵌套成员应被推进一次"
    );
    assert_eq!(
        advances(&direct_count),
        1,
        "advance_clock 成员应被推进一次"
    );

    let error = factory
        .advance_clock(ClockId(u32::MAX), 0.1)
        .expect_err("越界 ClockId 必须返回错误");
    assert_eq!(
        error,
        ClockError::InvalidClockId,
        "越界 ClockId(u32::MAX) 应返回 ClockError::InvalidClockId，实际 {error:?}"
    );
}
