//! 阶段 3 armature 层单元/集成测试（对应 `docs/guide/03-stage3.md` 验收：
//! **子骨架接线 F3.9 / D-3.2、槽排序稳定性 R5 / F3.10、`invalid_update` 传播
//! （指南 3.3 转述勘误）、`displayIndex == -1` 提前返回（坑 10）**）。
//!
//! 以 `Dragon` 素材（19 骨 18 槽）为主。子骨架在 9 个素材里均无覆盖
//! （F3.9 golden 无此数据），故用「parse 后改数据 + 注入子骨架显示」构造。
//!
//! 断言信息全中文带上下文；`#[test]` 各对应一条验收点（另含修复回归：
//! S1 内嵌 textureAtlas 无条件捕获、S4.5 `advance_time(0.0)` no-op）。

mod common;

use common::ir_snapshot::{asset_root, read_json, tex_stem};
use dragonbones_core::animation::world_clock::AnimatableHandle;
use dragonbones_core::armature::{
    Armature, ArmatureIdx, ArmatureProxy, Bone, BoneIdx, Constraint, DbEventListener,
    DisplayHandle, ListenerId, NullArmatureProxy, Slot, SlotIdx,
};
use dragonbones_core::core::enums::ActionType;
use dragonbones_core::event::EventObject;
use dragonbones_core::factory::BaseFactory;
use dragonbones_core::geom::transform::Transform;
use dragonbones_core::model::{ActionData, ArmatureData, DisplayData, DisplayKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const DRAGON_KEY: &str = "Dragon_ske";
const PARENT_ARMATURE: &str = "Dragon";
const CHILD_ARMATURE: &str = "ChildA";

struct AdvanceCounterProxy {
    inner: NullArmatureProxy,
    advances: Arc<AtomicUsize>,
}

impl AdvanceCounterProxy {
    fn new(advances: Arc<AtomicUsize>) -> Self {
        Self {
            inner: NullArmatureProxy::new(),
            advances,
        }
    }
}

impl ArmatureProxy for AdvanceCounterProxy {
    fn db_init(&mut self, armature: ArmatureIdx) {
        self.inner.db_init(armature);
    }

    fn db_clear(&mut self) {
        self.inner.db_clear();
    }

    fn db_update(&mut self) {
        self.advances.fetch_add(1, Ordering::SeqCst);
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

/// 按**创建序身份**取槽（`SlotIdx` 是身份而非排序后位置——槽排序重排后不可作
/// 下标，见 `armature.rs` 排序处 D-4.29 语义注）。
fn slot_of(armature: &Armature, idx: SlotIdx) -> Option<&Slot> {
    armature.slots.iter().find(|s| s.idx == idx)
}

/// 同 [`slot_of`]，可变版本。
fn slot_of_mut(armature: &mut Armature, idx: SlotIdx) -> Option<&mut Slot> {
    armature.slots.iter_mut().find(|s| s.idx == idx)
}

/// 只 parse 不 build 的 Dragon 工厂（`Arc<DragonBonesData>` 独占于 `data_map`，
/// 供子骨架注入改数据）。
fn dragon_factory() -> (BaseFactory, String) {
    let key = DRAGON_KEY.to_string();
    let ske = read_json(&asset_root().join(format!("{key}.json")));
    let tex = read_json(&asset_root().join(format!("{}.json", tex_stem(DRAGON_KEY))));

    let mut factory = BaseFactory::new();
    factory
        .parse_dragon_bones_data(&ske, &key)
        .unwrap_or_else(|| panic!("读 Dragon 素材失败：parse_dragon_bones_data 返回 None"));
    let atlas_ok = factory.parse_texture_atlas_data(&tex, &key);
    assert!(
        atlas_ok,
        "读 Dragon 图集失败：parse_texture_atlas_data 返回 false"
    );
    (factory, key)
}

/// build Dragon + 再驱动该骨架一遍 `advance_one(idx, 0.0)`（= TS 单骨架
/// `advanceTime(0)`；posedump 步骤 4 的**全局** `advanceTime(0)` 经 WorldClock.ts:114
/// 早退是 no-op，驱动语义须用 advance_one，见 `BaseFactory::advance_time` 文档）。
fn build_dragon(factory: &mut BaseFactory, key: &str) -> ArmatureIdx {
    let idx = factory
        .build_armature(
            PARENT_ARMATURE,
            key,
            "",
            key,
            Box::new(NullArmatureProxy::new()),
        )
        .unwrap_or_else(|| panic!("build_armature(`{PARENT_ARMATURE}`) 失败"));
    factory.advance_one(idx, 0.0);
    idx
}

fn slot_names(factory: &BaseFactory, idx: ArmatureIdx) -> Vec<String> {
    factory
        .armature(idx)
        .get_slots()
        .iter()
        .map(|s| s.slot_name.clone())
        .collect()
}

/// `transform_dirty == flag` 的骨名列表（诊断/断言用）。
fn bones_with_dirty(factory: &BaseFactory, idx: ArmatureIdx, flag: bool) -> Vec<String> {
    factory
        .armature(idx)
        .get_bones()
        .iter()
        .filter(|b| b.transform_dirty == flag)
        .map(|b| b.bone_name.clone())
        .collect()
}

/// `transform_dirty == flag` 的槽名列表。
fn slots_with_dirty(factory: &BaseFactory, idx: ArmatureIdx, flag: bool) -> Vec<String> {
    factory
        .armature(idx)
        .get_slots()
        .iter()
        .filter(|s| s.transform_dirty == flag)
        .map(|s| s.slot_name.clone())
        .collect()
}

/// 找到第一个 display 已接线为子骨架的槽 → `(子骨架 idx, 槽 idx)`。
fn find_child_slot(factory: &BaseFactory, parent: ArmatureIdx) -> (ArmatureIdx, SlotIdx) {
    for slot in factory.armature(parent).get_slots() {
        if let Some(DisplayHandle::Armature(child)) = slot.display {
            return (child, slot.idx);
        }
    }
    panic!("父骨架没有任何槽的 display 为 DisplayHandle::Armature(child)（子骨架接线失败）");
}

/// 往 default skin 注入「子骨架显示」+ 往 `DragonBonesData.armatures` 插最小子骨架。
///
/// - 子骨架 `ChildA`：0 骨 0 槽、`default_skin = None` → build 走 fill → skin None →
///   `_build_slots` 早退（BaseFactory.ts:180-182），只建空骨架 + 收尾 invalid/advance。
/// - 注入槽选择：`display_index` 指向**存在**的显示帧（否则 `set_display_index`
///   clamp 后落不到注入帧），且该槽在 default skin 有显示列表。
/// - 数据改写要求 `Arc<DragonBonesData>` 独占：parse 后工厂 `data_map` 持唯一引用，
///   未 build 时 `Arc::get_mut` 可用；dispose 全部骨架后可再次注入（变体 2）。
fn inject_child_display(factory: &mut BaseFactory, key: &str, inherit_animation: bool) {
    let arc = factory
        .data_map
        .get_mut(key)
        .unwrap_or_else(|| panic!("data_map 中无 `{key}`（parse_dragon_bones_data 未入表）"));
    let data = Arc::get_mut(arc).unwrap_or_else(|| {
        panic!(
            "DragonBonesData 的 Arc 非独占，无法注入子骨架数据（接口缺口：需要可变数据入口或 parse 返回值被外部持有）"
        )
    });

    // ① 最小子骨架（重名时 add_armature 静默忽略，幂等）
    data.add_armature(ArmatureData {
        name: CHILD_ARMATURE.into(),
        frame_rate: 24.0,
        ..Default::default()
    });

    // ② 选槽（display_index 合法 + default skin 有该槽显示列表）
    let armature_data = data
        .armatures
        .get_mut(PARENT_ARMATURE)
        .unwrap_or_else(|| panic!("DragonBonesData 中无 `{PARENT_ARMATURE}` 骨架"));
    let skin_name = armature_data
        .default_skin
        .clone()
        .unwrap_or_else(|| panic!("`{PARENT_ARMATURE}` 无 default_skin"));
    let candidates: Vec<(String, i32)> = armature_data
        .sorted_slots
        .iter()
        .filter_map(|name| {
            armature_data
                .slots
                .get(name)
                .map(|slot_data| (name.clone(), slot_data.display_index))
        })
        .filter(|(_, display_index)| *display_index >= 0)
        .collect();

    let skin = armature_data
        .skins
        .get_mut(&skin_name)
        .unwrap_or_else(|| panic!("`{PARENT_ARMATURE}` 无皮肤 `{skin_name}`"));
    let (slot_name, display_index) = candidates
        .into_iter()
        .find(|(name, display_index)| {
            skin.displays
                .get(name)
                .is_some_and(|list| usize::try_from(*display_index).is_ok_and(|i| i < list.len()))
        })
        .unwrap_or_else(|| panic!("找不到「显示帧存在且 display_index 合法」的槽可注入子骨架显示"));
    let frame_index = usize::try_from(display_index).expect("display_index >= 0（候选已过滤）");

    let displays = skin
        .displays
        .get_mut(&slot_name)
        .unwrap_or_else(|| panic!("皮肤 `{skin_name}` 无槽 `{slot_name}` 的显示列表"));
    displays[frame_index] = Some(DisplayData {
        name: CHILD_ARMATURE.into(),
        path: CHILD_ARMATURE.into(), // `_getSlotDisplay` TS:255：armatureName = displayData.path
        transform: Transform::default(),
        parent: skin_name.clone(),
        kind: DisplayKind::Armature {
            inherit_animation,
            actions: vec![ActionData {
                action_type: ActionType::Play,
                name: "walk".into(),
                bone: None,
                slot: None,
                data: None,
            }],
            armature: Some(CHILD_ARMATURE.into()),
        },
    });
}

/// F3.9 / D-3.2：子骨架接线（golden 无覆盖）——`inherit_animation` 两变体 +
/// action 消费 + dispose 递归释放。
#[test]
fn sub_armature_wiring_inherit_variants_and_dispose() {
    let (mut factory, key) = dragon_factory();

    // ---- 变体 1：inherit_animation = false（display 自带 Play action）----
    inject_child_display(&mut factory, &key, false);
    let parent_idx = build_dragon(&mut factory, &key);
    let (child_idx, slot_idx) = find_child_slot(&factory, parent_idx);

    // a) D-3.2：arena 序里子 > 父
    assert!(
        child_idx.0 > parent_idx.0,
        "子骨架 idx（{}）应大于父骨架 idx（{}）（D-3.2 不变量）",
        child_idx.0,
        parent_idx.0
    );
    // b) parent 回指（build 收尾 advance 后已接线，Slot.ts:548）
    assert_eq!(
        factory.armature(child_idx).parent,
        Some((parent_idx, slot_idx)),
        "子骨架 parent 应回指 (父 idx = {}, 槽 idx = {})",
        parent_idx.0,
        slot_idx.index()
    );
    assert_eq!(
        slot_of(factory.armature(parent_idx), slot_idx)
            .expect("注入槽应按身份存在")
            .child_armature,
        Some(child_idx),
        "父槽 child_armature 应回指子骨架 idx"
    );
    assert!(
        !factory.armature(child_idx).inherit_animation,
        "inherit_animation = false 应写入子骨架运行时（BaseFactory.ts:291）"
    );
    // c) action 被消费：build 收尾 advance 后父 actions 队列清空
    //（阶段 3 `Animation::fade_in` 为 no-op 占位，无法断言 fadeIn 调用——只断言队列收敛）
    assert!(
        factory.armature(parent_idx).actions.is_empty(),
        "inherit_animation = false：build 收尾 advance 后父骨架 actions 队列应已清空（action 被 advance ⑦ 消费）"
    );

    // 根 Armature clock setter 会把 clock 传播给当前已显示的 child armature。
    let root_clock = factory.root_clock();
    factory
        .attach_armature(root_clock, parent_idx)
        .expect("挂接父骨架到 root clock");
    assert_eq!(factory.armature(parent_idx).clock(), Some(root_clock));
    assert_eq!(factory.armature(child_idx).clock(), Some(root_clock));
    assert!(factory.clock_contains(root_clock, AnimatableHandle::Armature(child_idx)));

    factory
        .detach_armature(parent_idx)
        .expect("摘除父骨架应传播到 child");
    assert_eq!(factory.armature(parent_idx).clock(), None);
    assert_eq!(factory.armature(child_idx).clock(), None);
    factory
        .attach_armature(root_clock, parent_idx)
        .expect("重新挂接以验证 dispose 会先 detach");

    slot_of_mut(factory.armature_mut(parent_idx), slot_idx)
        .expect("child display 所在槽")
        .set_display_index(-1, false);
    factory.advance_one(parent_idx, 0.0);
    assert_eq!(factory.armature(parent_idx).clock(), Some(root_clock));
    assert_eq!(factory.armature(child_idx).clock(), None);
    assert_eq!(factory.armature(child_idx).parent, None);
    assert!(!factory.clock_contains(root_clock, AnimatableHandle::Armature(child_idx)));

    // e) dispose 父骨架 → 子骨架 arena 槽位 None（递归释放，D-3.1）
    factory.dispose_armature(parent_idx);
    assert!(
        factory.armatures[parent_idx.index()].is_none(),
        "dispose 后父骨架 arena 槽位应为 None"
    );
    assert!(
        factory.armatures[child_idx.index()].is_none(),
        "dispose 父骨架后子骨架 arena 槽位应为 None（经 Slot.child_armature / display_frames 递归释放）"
    );

    // ---- 变体 2：inherit_animation = true（动作由 Slot._UpdateDisplay 分支缓存）----
    // dispose 后 Arc 恢复独占 → 可再注入
    inject_child_display(&mut factory, &key, true);
    let parent_idx = build_dragon(&mut factory, &key);
    let (child_idx, slot_idx) = find_child_slot(&factory, parent_idx);

    assert!(
        child_idx.0 > parent_idx.0,
        "（inherit_animation = true 变体）子骨架 idx（{}）应大于父骨架 idx（{}）",
        child_idx.0,
        parent_idx.0
    );
    assert_eq!(
        factory.armature(child_idx).parent,
        Some((parent_idx, slot_idx)),
        "（inherit_animation = true 变体）子骨架 parent 应回指 (父 idx, 槽 idx)"
    );
    assert!(
        factory.armature(child_idx).inherit_animation,
        "inherit_animation = true 应写入子骨架运行时"
    );
    assert!(
        factory.armature(parent_idx).actions.is_empty(),
        "inherit_animation = true：父骨架 actions 恒空（Slot._UpdateDisplay 缓存的 action 已被 advance ⑦ 消费）"
    );
}

/// Clock membership changes made by a Slot during its parent's advance obey
/// the fixed-start-boundary rule: an added child waits until next frame, while
/// a child removed before its turn is skipped in the current frame.
#[test]
fn slot_clock_switch_respects_current_clock_iteration_boundary() {
    let (mut factory, key) = dragon_factory();
    inject_child_display(&mut factory, &key, true);
    let parent_idx = build_dragon(&mut factory, &key);
    let (child_idx, slot_idx) = find_child_slot(&factory, parent_idx);
    let display_index = slot_of(factory.armature(parent_idx), slot_idx)
        .expect("child display slot")
        .display_index;
    let child_advances = Arc::new(AtomicUsize::new(0));

    let mut proxy = AdvanceCounterProxy::new(Arc::clone(&child_advances));
    proxy.db_init(child_idx);
    factory.armature_mut(child_idx).proxy = Box::new(proxy);

    // First make the slot empty while the parent is not clocked.
    slot_of_mut(factory.armature_mut(parent_idx), slot_idx)
        .expect("child display slot")
        .set_display_index(-1, false);
    factory.advance_one(parent_idx, 0.0);
    assert_eq!(factory.armature(child_idx).clock(), None);

    let root_clock = factory.root_clock();
    factory
        .attach_armature(root_clock, parent_idx)
        .expect("attach parent; empty slot means child is not a member");
    slot_of_mut(factory.armature_mut(parent_idx), slot_idx)
        .expect("child display slot")
        .set_display_index(display_index, false);

    // Parent update attaches the child after the root clock captured its member snapshot.
    factory.advance_time_with(1.0 / 24.0, 100.0);
    assert_eq!(factory.armature(child_idx).clock(), Some(root_clock));
    assert_eq!(child_advances.load(Ordering::SeqCst), 0);

    // The child is now in the snapshot and advances on the next tick.
    factory.advance_time_with(1.0 / 24.0, 100.1);
    assert_eq!(child_advances.load(Ordering::SeqCst), 1);

    // Parent update removes the child before the child's captured turn; it must be skipped.
    slot_of_mut(factory.armature_mut(parent_idx), slot_idx)
        .expect("child display slot")
        .set_display_index(-1, false);
    factory.advance_time_with(1.0 / 24.0, 100.2);
    assert_eq!(factory.armature(child_idx).clock(), None);
    assert_eq!(child_advances.load(Ordering::SeqCst), 1);
}

/// R5 / F3.10：槽排序稳定性——复合键 `zIndex*1000+zOrder` 平局保持原序（稳定排序）、
/// `zIndex` 变化时 `set_z_order` 回写 0..n-1、最终按下标序期望。
///
/// 断言钉的是 **Rust 稳定排序**行为（偏差 D-4.29：官方比较器永不返 0 →
/// V8 对等键组实际**逆序**，但官方公开路径 `z_order` 恒唯一、等键不可达）。
#[test]
fn slot_sort_stability_and_z_index_rewrite() {
    let (mut factory, key) = dragon_factory();
    let idx = build_dragon(&mut factory, &key);
    let initial = slot_names(&factory, idx);
    let total = initial.len();
    assert!(total >= 2, "Dragon 应至少 2 个槽，实际 {total}");
    let total_i32 = i32::try_from(total).expect("槽数量在 i32 范围内");

    // ① 复合键全平局（zIndex=3 / zOrder=5）+ 只置 slots_dirty（不动 z_index_dirty）
    for slot in factory.armature_mut(idx).slots.iter_mut() {
        slot.z_index = 3;
        slot.z_order = 5;
    }
    factory.armature_mut(idx).slots_dirty = true;
    factory.advance_one(idx, 0.0); // 驱动该骨架跑一遍排序（advance_time(0.0) 是 no-op）

    assert_eq!(
        slot_names(&factory, idx),
        initial,
        "① 复合键 zIndex*1000+zOrder 全平局时应保持原槽序（F3.10：Rust 稳定排序保持原序。\
         偏差 D-4.29：官方比较器永不返 0 → V8 实际逆序等键组，\
         但官方公开路径 z_order 恒唯一、等键不可达）"
    );
    let orders: Vec<i32> = factory
        .armature(idx)
        .get_slots()
        .iter()
        .map(|s| s.z_order)
        .collect();
    assert!(
        orders.iter().all(|&o| o == 5),
        "① 只置 slots_dirty（z_index_dirty = false）时不应触发 zOrder 回写，实际 {orders:?}"
    );

    // ② 保留平局键 + z_index_dirty → 排序 + 回写 zOrder = 0..n-1，顺序仍不变
    factory.armature_mut(idx).z_index_dirty = true;
    factory.advance_one(idx, 0.0); // 驱动（advance_time(0.0) 全局早退是 no-op）

    assert_eq!(
        slot_names(&factory, idx),
        initial,
        "② 平局键下 zIndex 变化触发的排序/回写不应改变槽序"
    );
    let orders: Vec<i32> = factory
        .armature(idx)
        .get_slots()
        .iter()
        .map(|s| s.z_order)
        .collect();
    assert_eq!(
        orders,
        (0..total_i32).collect::<Vec<i32>>(),
        "② set_z_order 应按排序后位置回写 0..n-1"
    );

    // ③ 不同 zIndex（按当前位置倒序赋 0..n-1）+ z_index_dirty → 按键升序
    //    注：此时 zOrder = 0..n-1（② 回写），键 = z_index*1000 + z_order 两两不同
    for (i, slot) in factory.armature_mut(idx).slots.iter_mut().enumerate() {
        let z_index = i32::try_from(total - 1 - i).expect("槽数量在 i32 范围内");
        slot.z_index = z_index;
    }
    // 自算期望：稳定排序按 z_index*1000+z_order 升序（平局保持当前序，本例无平局）
    let mut expected: Vec<(String, i64)> = factory
        .armature(idx)
        .get_slots()
        .iter()
        .map(|s| {
            (
                s.slot_name.clone(),
                i64::from(s.z_index) * 1000 + i64::from(s.z_order),
            )
        })
        .collect();
    expected.sort_by_key(|(_, key)| *key); // Vec::sort_by_key 稳定
    let expected_names: Vec<String> = expected.into_iter().map(|(name, _)| name).collect();

    factory.armature_mut(idx).z_index_dirty = true;
    factory.advance_one(idx, 0.0); // 驱动（advance_time(0.0) 全局早退是 no-op）

    assert_eq!(
        slot_names(&factory, idx),
        expected_names,
        "③ 槽序应等于按 zIndex*1000+zOrder 升序（稳定排序、平局保持原序）自算的期望序"
    );
    let orders: Vec<i32> = factory
        .armature(idx)
        .get_slots()
        .iter()
        .map(|s| s.z_order)
        .collect();
    assert_eq!(
        orders,
        (0..total_i32).collect::<Vec<i32>>(),
        "③ 排序后 set_z_order 应回写 0..n-1"
    );
}

#[test]
fn z_order_timeline_reorders_by_model_slot_identity_and_restores_original_order() {
    let (mut factory, key) = dragon_factory();
    let armature_idx = build_dragon(&mut factory, &key);
    let sorted_names = factory
        .armature(armature_idx)
        .armature_data()
        .sorted_slots
        .clone();
    let frame_indices: Vec<i32> = (0..sorted_names.len())
        .rev()
        .map(|index| i32::try_from(index).expect("slot index fits i32"))
        .collect();

    factory
        .armature_mut(armature_idx)
        .sort_z_order(Some(&frame_indices), 0);
    factory.advance_one(armature_idx, 0.0);
    let ordered: Vec<_> = factory
        .armature(armature_idx)
        .slots
        .iter()
        .map(|slot| (slot.slot_name.as_str(), slot.z_order))
        .collect();
    for (position, (name, z_order)) in ordered.iter().enumerate() {
        assert_eq!(*name, sorted_names[sorted_names.len() - 1 - position]);
        assert_eq!(
            *z_order,
            i32::try_from(position).expect("position fits i32")
        );
    }

    factory.armature_mut(armature_idx).sort_z_order(None, 0);
    factory.advance_one(armature_idx, 0.0);
    assert_eq!(
        factory
            .armature(armature_idx)
            .slots
            .iter()
            .map(|slot| slot.slot_name.as_str())
            .collect::<Vec<_>>(),
        sorted_names.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn constraint_invalid_update_marks_root_and_optional_chain_only() {
    let mut bones = Vec::new();
    for index in 0..3 {
        bones.push(Bone {
            idx: BoneIdx(index),
            transform_dirty: false,
            ..Bone::default()
        });
    }
    let mut constraint = Constraint {
        root: BoneIdx(1),
        bone: Some(BoneIdx(2)),
        ..Constraint::default()
    };

    constraint.invalid_update(&mut bones);
    assert_eq!(
        bones
            .iter()
            .map(|bone| bone.transform_dirty)
            .collect::<Vec<_>>(),
        [false, true, true]
    );

    for bone in &mut bones {
        bone.transform_dirty = false;
    }
    constraint.bone = None;
    constraint.invalid_update(&mut bones);
    assert_eq!(
        bones
            .iter()
            .map(|bone| bone.transform_dirty)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
}

/// `invalid_update` 传播（指南 3.3 转述勘误）：只置目标骨自身 `transform_dirty`，
/// 不沿父/子传播；`None` + `update_slot = true` 才全骨全槽。
#[test]
fn invalid_update_sets_only_target_bone_then_all() {
    let (mut factory, key) = dragon_factory();
    let idx = build_dragon(&mut factory, &key);

    // ---- 前置：build 收尾（invalid_update(null, true) + advance）后脏标记应全部消费 ----
    assert!(
        bones_with_dirty(&factory, idx, true).is_empty(),
        "前置失败：build 后所有骨 transform_dirty 应为 false，仍脏：{:?}",
        bones_with_dirty(&factory, idx, true)
    );
    assert!(
        slots_with_dirty(&factory, idx, true).is_empty(),
        "前置失败：build 后所有槽 transform_dirty 应为 false，仍脏：{:?}",
        slots_with_dirty(&factory, idx, true)
    );

    // 选「有父且被至少一骨当父」的中间骨（比叶子骨更有信息量）
    let (target_name, target_idx, parent_idx, child_idx) = {
        let armature = factory.armature(idx);
        let bones = armature.get_bones();
        let target = bones
            .iter()
            .find(|b| b.parent.is_some() && bones.iter().any(|c| c.parent == Some(b.idx)))
            .unwrap_or_else(|| panic!("Dragon 中找不到同时有父有子的中间骨"));
        let parent = target.parent.expect("前置保证目标骨有父");
        let child = bones
            .iter()
            .find(|c| c.parent == Some(target.idx))
            .expect("前置保证目标骨有子")
            .idx;
        (target.bone_name.clone(), target.idx, parent, child)
    };

    // ---- ① 指名 + update_slot = false：只脏目标骨 ----
    factory
        .armature_mut(idx)
        .invalid_update(Some(&target_name), false);

    let dirtied_bones = bones_with_dirty(&factory, idx, true);
    assert_eq!(
        dirtied_bones,
        vec![target_name.clone()],
        "invalid_update(Some(`{target_name}`), false) 后应恰好只有目标骨 transform_dirty = true"
    );
    {
        let armature = factory.armature(idx);
        assert!(
            armature.get_bones()[target_idx.index()].transform_dirty,
            "目标骨 `{target_name}` transform_dirty 应为 true"
        );
        assert!(
            !armature.get_bones()[parent_idx.index()].transform_dirty,
            "父骨 transform_dirty 应仍为 false（TS Bone.ts:481-483 `invalidUpdate` 只置自身——指南 3.3 转述的「沿父传播」有误）"
        );
        assert!(
            !armature.get_bones()[child_idx.index()].transform_dirty,
            "子骨 transform_dirty 应仍为 false（不沿子传播；子骨靠 update 读父 children_transform_dirty 在下一帧间接变脏）"
        );
    }
    assert!(
        slots_with_dirty(&factory, idx, true).is_empty(),
        "update_slot = false 时不应碰任何槽，实际被置脏：{:?}",
        slots_with_dirty(&factory, idx, true)
    );

    // ---- ② 不指名 + update_slot = true：全骨 + 全槽 ----
    factory.armature_mut(idx).invalid_update(None, true);

    let clean_bones = bones_with_dirty(&factory, idx, false);
    assert!(
        clean_bones.is_empty(),
        "invalid_update(None, true) 后全骨 transform_dirty 应为 true，仍干净：{clean_bones:?}"
    );
    let clean_slots = slots_with_dirty(&factory, idx, false);
    assert!(
        clean_slots.is_empty(),
        "invalid_update(None, true) 且 update_slot = true 后全槽 transform_dirty 应为 true（Slot::invalid_update 置 display_data_dirty / display_dirty / transform_dirty），仍干净：{clean_slots:?}"
    );
}

/// 坑 10：`displayIndex == -1` 时 `Slot.update` 在 `display is None` 处提前返回——
/// visible 段与 transform 段均未推进（用对照槽证明消费发生在 display 正常时）。
#[test]
fn display_index_negative_returns_early_in_slot_update() {
    let (mut factory, key) = dragon_factory();
    let idx = build_dragon(&mut factory, &key);

    // 目标槽 = 第一个 display 正常的槽；对照槽 = 第二个（同样 display 正常）
    let (target_name, sibling_name) = {
        let armature = factory.armature(idx);
        let mut valid = armature
            .get_slots()
            .iter()
            .filter(|s| s.display.is_some() && s.display_index >= 0);
        let target = valid
            .next()
            .unwrap_or_else(|| {
                panic!("骨架无 display 正常（display 非 None 且 displayIndex >= 0）的槽")
            })
            .slot_name
            .clone();
        let sibling = valid
            .next()
            .unwrap_or_else(|| panic!("骨架不足两个 display 正常的槽，无法做对照"))
            .slot_name
            .clone();
        (target, sibling)
    };

    let target_idx = factory
        .armature(idx)
        .get_slot(&target_name)
        .unwrap_or_else(|| panic!("目标槽 `{target_name}` 不存在"));
    let sibling_idx = factory
        .armature(idx)
        .get_slot(&sibling_name)
        .unwrap_or_else(|| panic!("对照槽 `{sibling_name}` 不存在"));
    assert!(
        slot_of(factory.armature(idx), target_idx)
            .expect("目标槽应按身份存在")
            .display
            .is_some(),
        "前置：`{target_name}` build 后 display 应非 None"
    );

    // 目标槽：displayIndex = -1 + 手动置 visible_dirty（transform_dirty 基线 = false）
    {
        let armature = factory.armature_mut(idx);
        let slot = slot_of_mut(armature, target_idx).expect("目标槽应按身份存在");
        slot.set_display_index(-1, false);
        slot.visible_dirty = true;
    }
    // 对照槽：同样手动置 visible_dirty（display 正常 → advance 应正常消费）
    slot_of_mut(factory.armature_mut(idx), sibling_idx)
        .expect("对照槽应按身份存在")
        .visible_dirty = true;

    factory.advance_one(idx, 0.0); // 驱动该骨架跑一遍 update（advance_time(0.0) 是 no-op）

    let armature = factory.armature(idx);
    let target = slot_of(armature, target_idx).expect("目标槽应按身份存在");
    assert_eq!(
        target.display_index, -1,
        "`{target_name}` set_display_index(-1) 后 display_index 应为 -1"
    );
    assert!(
        target.display.is_none(),
        "`{target_name}` displayIndex = -1 → update ② 后 display 应为 None（displayFrame = null）"
    );
    assert!(
        target.visible_dirty,
        "`{target_name}` visible_dirty 应仍为 true：④ `display.is_none()` 提前返回，未走到 ⑤ visible 段（坑 10 的行为证明）"
    );
    assert!(
        target.transform_dirty,
        "`{target_name}` transform_dirty 应仍为 true：① 置脏后 ④ 提前返回，⑪ 变换分支（末尾会消费为 false）未推进"
    );

    let sibling = slot_of(armature, sibling_idx).expect("对照槽应按身份存在");
    assert!(
        sibling.display.is_some(),
        "对照槽 `{sibling_name}` display 应仍非 None"
    );
    assert!(
        !sibling.visible_dirty,
        "对照槽 `{sibling_name}`（display 正常）的 visible_dirty 应被 ⑤ 正常消费 → 反证目标槽未消费正是提前返回所致"
    );
}

/// S4.5 回归：`BaseFactory::advance_time(0.0)` 对齐 `WorldClock.advanceTime(0.0)`
/// **clock 内层**早退（WorldClock.ts:114-116）：挂钟成员不被推进；但外层
/// objects/events 两段仍执行（04-stage4.md §1.6：0 早退已下移到 clock 层）。
/// `advance_one(idx, 0.0)` 才是"跑一遍更新/排序"（= TS 单骨架 `armature.advanceTime(0)`，
/// build 收尾 BaseFactory.ts:713 同款）。
#[test]
fn advance_time_zero_is_noop_while_advance_one_drives() {
    let (mut factory, key) = dragon_factory();
    let idx = build_dragon(&mut factory, &key);

    // build 收尾已消费干净（前置）：见 invalid_update 测试的同款基线断言
    assert!(
        !factory.armature(idx).slots_dirty,
        "前置失败：build 收尾后 slots_dirty 应已被消费"
    );

    // 挂到 root clock：dt=0 时 clock 内层早退，成员不得推进（脏标记保留）。
    let root = factory.root_clock();
    factory
        .attach_armature(root, idx)
        .expect("挂接 Dragon 到 root clock 以验证 dt=0 不推进成员");
    assert_eq!(factory.armature(idx).clock(), Some(root));

    // ① 手动置脏 + 全局 advance_time(0.0) → 应原样保留（成员未推进）
    factory.armature_mut(idx).slots_dirty = true;
    factory.advance_time(0.0);
    assert!(
        factory.armature(idx).slots_dirty,
        "advance_time(0.0) 的 clock 内层早退（WorldClock.ts:114-116）不应推进挂钟成员，\
         slots_dirty 必须保留"
    );

    // ② advance_one(idx, 0.0) → 主循环照跑（Armature::advanceTime 无 0 值早退），脏标记被消费
    factory.advance_one(idx, 0.0);
    assert!(
        !factory.armature(idx).slots_dirty,
        "advance_one(idx, 0.0) 应跑完整主循环并消费 slots_dirty（= TS armature.advanceTime(0)）"
    );
}

/// S1 回归：内嵌 `textureAtlas` 的**无条件**捕获（ObjectDataParser.ts:2336-2338 在
/// `if (ARMATURE in rawData)` 块外、`return data` 前捕获，与 armature 键在否无关）。
/// 修复前 Rust 在 armature 分支内提前 return 漏捕 → `BaseFactory::parse_dragon_bones_data`
/// 的内嵌图集循环（BaseFactory.ts:357-366）首圈即 break，图集静默丢失。
/// （9 素材均无内嵌图集 → ir_golden 覆盖不到，故用最小合成 ske 钉住。）
#[test]
fn embedded_texture_atlas_captured_when_armature_key_present() {
    let mut factory = BaseFactory::new();
    let ske = serde_json::json!({
        "version": "5.6",
        "name": "Mini",
        "frameRate": 24.0,
        "armature": [],
        "textureAtlas": [{
            "name": "Mini_tex",
            "imagePath": "Mini",
            "width": 64.0,
            "height": 64.0,
            "SubTexture": [{ "name": "part", "x": 0.0, "y": 0.0, "width": 8.0, "height": 8.0 }]
        }]
    });
    factory
        .parse_dragon_bones_data(&ske, "mini")
        .unwrap_or_else(|| panic!("含内嵌 textureAtlas 的最小 ske 应解析成功"));

    let atlases = factory.atlas_map.get("mini").unwrap_or_else(|| {
        panic!(
            "内嵌 textureAtlas 应被捕获并经 BaseFactory 内嵌图集循环（BaseFactory.ts:357-366）\
             以 parse key `mini` 入 atlas_map——S1 修复前此处静默丢失"
        )
    });
    assert_eq!(atlases.len(), 1, "应恰好 1 个内嵌图集入表");
    assert_eq!(atlases[0].name, "Mini_tex", "内嵌图集 name 应为 `Mini_tex`");
    assert!(
        atlases[0].get_texture("part").is_some(),
        "内嵌图集应解析出 SubTexture `part`"
    );
}
