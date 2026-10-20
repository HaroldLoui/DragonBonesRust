//! Synthetic-IR helpers for module-local timeline-state tests.

use std::sync::Arc;

use crate::animation::AnimationUpdateCtx;
use crate::animation::animation_state::AnimationState;
use crate::armature::{ArmatureIdx, Bone, Constraint, NullArmatureProxy, Slot};
use crate::core::enums::TimelineType;
use crate::event::EventObject;
use crate::model::{
    AnimationConfig, AnimationData, ArmatureData, DragonBonesData, IrArrays, TimelineData,
};

use super::{AnimationTimelineSource, TimelineInstance, TimelineTarget};

pub(crate) const ARMATURE: &str = "arm";
pub(crate) const ANIMATION: &str = "anim";

pub(crate) fn test_config(reset_to_pose: bool) -> AnimationConfig {
    AnimationConfig {
        reset_to_pose,
        play_times: 1,
        time_scale: 1.0,
        pause_fade_in: false,
        fade_in_time: 0.0,
        ..AnimationConfig::default()
    }
}

pub(crate) fn build_data(
    animation: AnimationData,
    armature: ArmatureData,
    ir: IrArrays,
) -> Arc<DragonBonesData> {
    let mut animation = animation;
    animation.name = ANIMATION.to_owned();
    animation.parent = ARMATURE.to_owned();
    let mut armature = armature;
    armature.name = ARMATURE.to_owned();
    armature.animations.insert(ANIMATION.to_owned(), animation);

    let mut data = DragonBonesData {
        name: "data".to_owned(),
        frame_rate: armature.frame_rate,
        ..DragonBonesData::default()
    };
    data.add_armature(armature);
    data.finalize_root(ir);
    Arc::new(data)
}

pub(crate) fn make_state(data: &Arc<DragonBonesData>, reset_to_pose: bool) -> AnimationState {
    let armature = data.armatures.get(ARMATURE).expect("armature");
    let animation = armature.animations.get(ANIMATION).expect("animation");
    let mut state = AnimationState::default();
    state.init(ArmatureIdx(0), animation, &test_config(reset_to_pose));
    state
}

pub(crate) fn source(data: &Arc<DragonBonesData>) -> AnimationTimelineSource {
    AnimationTimelineSource {
        data: Arc::clone(data),
        owner_armature: ARMATURE.to_owned(),
        animation_name: ANIMATION.to_owned(),
    }
}

pub(crate) fn make_timeline(
    data: &Arc<DragonBonesData>,
    state: &AnimationState,
    timeline_type: TimelineType,
    target: TimelineTarget,
    timeline_data: Option<TimelineData>,
) -> TimelineInstance {
    let host = data.armatures.get(ARMATURE).expect("host armature");
    TimelineInstance::new(
        timeline_type,
        target,
        None,
        timeline_data,
        source(data),
        state,
        host,
        None,
    )
    .expect("synthetic timeline")
}

/// Frame-index table for `duration * frame_rate + 1` slots, mapping each time
/// slot to the last key frame whose position is `<= slot`.
fn frame_indices(positions: &[i32], frame_rate: f64, duration: f64) -> Vec<i32> {
    let slots = (duration * frame_rate).floor() as i32;
    (0..=slots)
        .map(|time| {
            positions
                .iter()
                .rposition(|position| *position <= time)
                .unwrap_or(0) as i32
        })
        .collect()
}

/// Builds one timeline at offset 0. `payloads[i]` is the frame record's second
/// integer (tween type for numeric axes, display index for SlotDisplay).
#[allow(clippy::too_many_arguments)]
pub(crate) fn timeline_ir(
    frame_rate: f64,
    duration: f64,
    positions: &[i32],
    payloads: &[i32],
    frame_value_count: i32,
    float_values: &[f64],
    int_values: &[i32],
) -> IrArrays {
    let mut timeline_array = vec![100, 0, positions.len() as i32, frame_value_count, 0];
    let mut frame_array = Vec::new();
    for (position, payload) in positions.iter().zip(payloads) {
        timeline_array.push(frame_array.len() as i32);
        frame_array.push(*position);
        frame_array.push(*payload);
    }
    let frame_float_array = float_values
        .iter()
        .map(|value| *value as f32 as f64)
        .collect();
    IrArrays {
        frame_array,
        timeline_array,
        frame_indices: frame_indices(positions, frame_rate, duration),
        frame_float_array,
        frame_int_array: int_values.to_vec(),
        ..IrArrays::default()
    }
}

pub(crate) fn timeline_data(timeline_type: TimelineType) -> TimelineData {
    TimelineData {
        timeline_type,
        offset: 0,
        frame_indices_offset: 0,
        ..TimelineData::default()
    }
}

pub(crate) fn empty_data() -> Arc<DragonBonesData> {
    build_data(
        AnimationData {
            duration: 1.0,
            ..AnimationData::default()
        },
        ArmatureData {
            frame_rate: 24.0,
            ..ArmatureData::default()
        },
        IrArrays::default(),
    )
}

pub(crate) fn pose_timeline(
    timeline_type: TimelineType,
    target: TimelineTarget,
) -> TimelineInstance {
    let data = empty_data();
    let state = make_state(&data, true);
    make_timeline(&data, &state, timeline_type, target, None)
}

/// SlotColor stores one `colorArray` index per frame in `frameIntArray`
/// (`TimelineState.ts:674-700`).
pub(crate) fn slot_color_timeline(
    frame_rate: f64,
    duration: f64,
    positions: &[i32],
    tween_types: &[i32],
    color_offsets: &[i32],
    color_array: Vec<i32>,
) -> TimelineInstance {
    let mut ir = timeline_ir(
        frame_rate,
        duration,
        positions,
        tween_types,
        0,
        &[],
        color_offsets,
    );
    ir.color_array = color_array;
    let animation = AnimationData {
        duration,
        frame_count: positions.len() as i32,
        ..AnimationData::default()
    };
    let armature = ArmatureData {
        frame_rate,
        ..ArmatureData::default()
    };
    let data = build_data(animation, armature, ir);
    let state = make_state(&data, false);
    make_timeline(
        &data,
        &state,
        TimelineType::SlotColor,
        TimelineTarget::Slot(crate::armature::SlotIdx(0)),
        Some(timeline_data(TimelineType::SlotColor)),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn numeric_timeline(
    timeline_type: TimelineType,
    target: TimelineTarget,
    value_count: usize,
    armature_scale: f64,
    frame_rate: f64,
    duration: f64,
    positions: &[i32],
    tween_types: &[i32],
    values: &[f64],
    int_backed: bool,
) -> TimelineInstance {
    debug_assert!(value_count == 0 || values.len().is_multiple_of(value_count));
    let (float_values, int_values) = if int_backed {
        let ints: Vec<i32> = values.iter().map(|value| *value as i32).collect();
        (&[][..], ints)
    } else {
        (values, Vec::new())
    };
    let ir = timeline_ir(
        frame_rate,
        duration,
        positions,
        tween_types,
        0,
        float_values,
        &int_values,
    );
    let animation = AnimationData {
        duration,
        frame_count: positions.len() as i32,
        ..AnimationData::default()
    };
    let armature = ArmatureData {
        frame_rate,
        scale: armature_scale,
        ..ArmatureData::default()
    };
    let data = build_data(animation, armature, ir);
    let state = make_state(&data, false);
    make_timeline(
        &data,
        &state,
        timeline_type,
        target,
        Some(timeline_data(timeline_type)),
    )
}

/// Convenience action-time snapshot for `TimelineInstance::update_values`.
pub(crate) fn action_time(
    current_time: f64,
    play_state: super::super::timeline_state::TimelinePlayState,
) -> super::super::timeline_state::ActionTimelineTime {
    super::super::timeline_state::ActionTimelineTime {
        play_state,
        current_play_times: 0,
        current_time,
    }
}

pub(crate) struct CtxFixture {
    pub(crate) armature_data: ArmatureData,
    pub(crate) bones: Vec<Bone>,
    pub(crate) slots: Vec<Slot>,
    pub(crate) constraints: Vec<Constraint>,
    pub(crate) actions: Vec<EventObject>,
    pub(crate) cache_frame_index: i32,
    pub(crate) alpha_dirty: bool,
    pub(crate) slots_dirty: bool,
    pub(crate) z_order_dirty: bool,
    pub(crate) z_index_dirty: bool,
    pub(crate) proxy: NullArmatureProxy,
}

impl CtxFixture {
    pub(crate) fn new(armature_data: ArmatureData) -> Self {
        Self {
            armature_data,
            bones: Vec::new(),
            slots: Vec::new(),
            constraints: Vec::new(),
            actions: Vec::new(),
            cache_frame_index: -1,
            alpha_dirty: false,
            slots_dirty: false,
            z_order_dirty: false,
            z_index_dirty: false,
            proxy: NullArmatureProxy::new(),
        }
    }

    pub(crate) fn with_context<R>(
        &mut self,
        f: impl FnOnce(&mut AnimationUpdateCtx<'_>) -> R,
    ) -> R {
        let Self {
            armature_data,
            bones,
            slots,
            constraints,
            actions,
            cache_frame_index,
            alpha_dirty,
            slots_dirty,
            z_order_dirty,
            z_index_dirty,
            proxy,
        } = self;
        let mut context = AnimationUpdateCtx {
            armature_idx: ArmatureIdx(0),
            armature_data,
            bones,
            slots,
            constraints,
            actions,
            cache_frame_index,
            alpha_dirty,
            slots_dirty,
            z_order_dirty,
            z_index_dirty,
            proxy,
        };
        f(&mut context)
    }
}
