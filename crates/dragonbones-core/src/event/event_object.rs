//! `event/EventObject.ts`（282 行）。
//!
//! 字段 + 事件名常量 + `actionDataToInstance`（Slot 子骨架接线、工厂 defaultActions、
//! Armature action 队列三处调用）。阶段 5 起事件由 `BaseFactory` 的 `VecDeque`
//! 派发后立即 drop（D-5.3）；`Default` 即 TS `_onClear` 复位态。
//! `animationState` 字段继续按 D-4.20 省略（core/golden 不读）。
//!
//! D-4.20 扩充一句：`action_data` 存 `ActionData` **克隆**（TS 存引用，Rust data 层
//! 不可变 → 值语义克隆）之外，另设 `bone_name` / `slot_name` **冗余名字字段**——
//! 理由：`bone`/`slot` 索引给核心逻辑（快速定位），名字给事件消费者（jsdump golden
//! 事件断言读 `e.bone.name` / `e.slot.name`，tools/jsdump.mjs:107-108；arena 索引
//! 无从反解名字，Slot/Bone 不在事件对象可达范围内）。两字段随索引一并解析填充、
//! 解析失败为 None，`_onClear` 复位 None（`Default` 同值）。

use crate::armature::{ArmatureIdx, Bone, BoneIdx, Slot, SlotIdx};
use crate::core::enums::ActionType;
use crate::model::{ActionData, UserData};

/// 对齐 `EventObject`（EventObject.ts:35，`extends BaseObject`）。
///
/// TS 的 `bone: Bone | null` / `slot: Slot | null` 是对象引用；Rust 用
/// **本骨架内索引**（事件消费方经 `armature` + 索引解析，与 D-3.2 同法）。
/// `animationState` 字段阶段 4/5 再补（类型尚未存在）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventObject {
    /// 对齐 `time`（EventObject.ts:174），`_onClear` = 0.0。
    pub time: f64,
    /// 对齐 `type: EventStringType`（EventObject.ts:185），`_onClear` = ""。
    /// 字段名带 `event_` 前缀避免与 `action_type` 等语义混淆（§1 `type` 单值枚举规则）。
    pub event_type: String,
    /// 对齐 `name`（EventObject.ts:196），`_onClear` = ""。
    pub name: String,
    /// 对齐 `armature`（EventObject.ts:209），`_onClear` = null。
    pub armature: Option<ArmatureIdx>,
    /// 对齐 `bone`（EventObject.ts:222），`_onClear` = null。
    pub bone: Option<BoneIdx>,
    /// 对齐 `slot`（EventObject.ts:235），`_onClear` = null。
    pub slot: Option<SlotIdx>,
    /// 解析出的**骨名**（TS 无此字段——对象引用自带 `name`；Rust 索引无 arena
    /// 反解不了 → 冗余存名，见模块头 D-4.20 扩充）。
    /// `_onClear`（EventObject.ts:275 `bone = null`）→ None（`Default` 同值）。
    /// 索引给核心逻辑、名字给事件消费者（jsdump golden 事件断言读 `e.bone.name`）。
    pub bone_name: Option<String>,
    /// 解析出的**槽名**（同上；`_onClear` EventObject.ts:276 `slot = null` → None；
    /// jsdump golden 读 `e.slot.name`，tools/jsdump.mjs:108）。
    pub slot_name: Option<String>,
    /// 对齐 `actionData`（EventObject.ts:252），`_onClear` = null。
    /// TS 存引用；Rust 存克隆（ActionData 小且 data 层不可变，D-1.1 同法）。
    pub action_data: Option<ActionData>,
    /// 对齐 `data: UserData | null`（EventObject.ts:268），`_onClear` = null。
    pub data: Option<Box<UserData>>,
    // Deliberately omitted per F4.27: TS writes this reference but core/golden never reads it.
}

impl EventObject {
    /// 事件名常量（EventObject.ts:46-134，值即 TS 字符串字面量）。
    pub const START: &'static str = "start";
    pub const LOOP_COMPLETE: &'static str = "loopComplete";
    pub const COMPLETE: &'static str = "complete";
    pub const FADE_IN: &'static str = "fadeIn";
    pub const FADE_IN_COMPLETE: &'static str = "fadeInComplete";
    pub const FADE_OUT: &'static str = "fadeOut";
    pub const FADE_OUT_COMPLETE: &'static str = "fadeOutComplete";
    pub const FRAME_EVENT: &'static str = "frameEvent";
    pub const SOUND_EVENT: &'static str = "soundEvent";

    /// 对齐 `EventObject.actionDataToInstance`（EventObject.ts:139-159）。
    ///
    /// - `bones`/`slots` 用于按名解析 `data.bone`/`data.slot`（TS 152-158
    ///   `armature.getBone/getSlot`）。槽循环内调用时 `slots` 传 `&[]`——
    ///   该调用点随后恒有 `event.slot = 发射槽` 覆写（Slot.ts:570），解析结果不影响行为；
    ///   工厂装配期调用则传真实 `&[Slot]`。
    /// - TS 不清除 `instance` 中本次未赋值的字段（池借出时已 `_onClear`），
    ///   Rust 调用方传 `EventObject::default()` 或复位过的实例，行为一致。
    /// - 索引 + 名字一并填（见模块头 D-4.20 扩充）：按名解析成功 →
    ///   `bone/slot = Some(创建序身份 idx)`、`bone_name/slot_name = Some(名字)`；
    ///   解析失败（名字无匹配）→ 两者皆 None（TS `getBone/getSlot → null` 同步）。
    ///   身份取 `b.idx`/`s.idx` 而非数组位置（`Slot.idx` = 创建序身份，见
    ///   `armature.rs` 排序处语义注；调用点传入的 `bones`/`slots` 恒创建序，
    ///   两者当前等值）。
    pub fn action_data_to_instance(
        data: &ActionData,
        instance: &mut EventObject,
        armature: ArmatureIdx,
        bones: &[Bone],
        slots: &[Slot],
    ) {
        // EventObject.ts:140-145（Play 与 Frame 同归 FRAME_EVENT，原样照抄语义；
        // 写成 match 规避 `if_same_then_else`）
        instance.event_type = match data.action_type {
            ActionType::Play | ActionType::Frame => Self::FRAME_EVENT,
            ActionType::Sound => Self::SOUND_EVENT,
        }
        .to_string();

        instance.name = data.name.clone(); // :147
        instance.armature = Some(armature); // :148
        instance.action_data = Some(data.clone()); // :149
        instance.data = data.data.clone(); // :150

        if let Some(bone_name) = &data.bone {
            // :152-154
            instance.bone = bones
                .iter()
                .find(|b| b.bone_name == *bone_name)
                .map(|b| b.idx);
            instance.bone_name = instance.bone.map(|_| bone_name.clone());
        }

        if let Some(slot_name) = &data.slot {
            // :156-158
            instance.slot = slots
                .iter()
                .find(|s| s.slot_name == *slot_name)
                .map(|s| s.idx);
            instance.slot_name = instance.slot.map(|_| slot_name.clone());
        }
    }
}
