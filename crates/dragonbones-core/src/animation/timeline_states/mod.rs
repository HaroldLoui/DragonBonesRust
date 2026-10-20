//! Concrete timeline runtime state and target handles.
//!
//! The per-axis formulas are implemented by the child modules; this shared
//! state stores the decoded IR offsets and interpolation buffers.

pub mod action;
pub mod animation;
pub mod bone;
pub mod constraint;
pub mod deform;
pub(crate) mod rebuild;
pub mod slot;
pub mod surface;
pub(crate) mod update;
pub mod z_order;

#[cfg(test)]
pub(crate) mod test_support;

use std::sync::Arc;

use crate::armature::{ArmatureIdx, BoneIdx, ConstraintIdx, SlotIdx};
use crate::core::enums::{TimelineType, binary_offset};
use crate::model::{AnimationData, ArmatureData, DragonBonesData, RootInfo, TimelineData};

use super::animation_state::AnimationState;
use super::blend_state::BlendStateKey;
use super::state_id::StateId;
use super::timeline_state::{
    ActionTimelineTime, BaseTimelineState, TimelineTimeInput, TweenFrameParameters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimelineTarget {
    Armature(ArmatureIdx),
    Bone(BoneIdx),
    Slot(SlotIdx),
    Constraint(ConstraintIdx),
    AnimationState(StateId),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnimationTimelineSource {
    pub(crate) data: Arc<DragonBonesData>,
    pub(crate) owner_armature: String,
    pub(crate) animation_name: String,
}

impl AnimationTimelineSource {
    #[must_use]
    pub(crate) fn animation_data(&self) -> Option<&AnimationData> {
        self.data
            .armatures
            .get(&self.owner_armature)?
            .animations
            .get(&self.animation_name)
    }

    #[must_use]
    pub(crate) fn armature_data(&self) -> Option<&ArmatureData> {
        self.data.armatures.get(&self.owner_armature)
    }

    #[must_use]
    pub(crate) fn root_info(&self) -> Option<&Arc<RootInfo>> {
        self.data.root.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueArrayKind {
    None,
    Int,
    Float,
    SlotColor,
}

/// One owned runtime axis. Its `target`/`blend_key` refer to stable armature IDs;
/// immutable animation IR is resolved through `source`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimelineInstance {
    pub(crate) id: u32,
    pub(crate) base: BaseTimelineState,
    pub(crate) timeline_type: TimelineType,
    pub(crate) target: TimelineTarget,
    pub(crate) blend_key: Option<BlendStateKey>,
    pub(crate) timeline_data: Option<TimelineData>,
    pub(crate) source: AnimationTimelineSource,
    pub(crate) animation_frame_offset: i32,
    pub(crate) animation_duration: f64,
    pub(crate) value_array_kind: ValueArrayKind,
    pub(crate) value_offset: i32,
    pub(crate) value_scale: f64,
    pub(crate) value_count: usize,
    /// `_current` / `_currentA`+`_currentB` / `rd[0..count]` equivalent.
    /// Double/Single/Multiple only assign this buffer while `_isTween`
    /// (`BaseTimelineState.ts:390-397`, `:452-463`, `:516-524`); a non-tween
    /// frame leaves the previous tween's values untouched. IK reads them
    /// (`TimelineState.ts:971-972`), so stale data is official behavior.
    pub(crate) current_values: Vec<f64>,
    pub(crate) differences: Vec<f64>,
    pub(crate) results: Vec<f64>,
    pub(crate) tween_frame: Option<TweenFrameParameters>,
    pub(crate) color_offset: i32,
    pub(crate) deform_count: usize,
    pub(crate) deform_offset: usize,
    pub(crate) same_value_offset: usize,
    pub(crate) display_frame: Option<usize>,
    pub(crate) is_pose: bool,
}

impl TimelineInstance {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        timeline_type: TimelineType,
        target: TimelineTarget,
        blend_key: Option<BlendStateKey>,
        timeline_data: Option<TimelineData>,
        source: AnimationTimelineSource,
        state: &AnimationState,
        host_armature_data: &ArmatureData,
        display_frame: Option<usize>,
    ) -> Option<Self> {
        let animation_data = source.animation_data()?;
        let animation_frame_offset = animation_data.frame_offset;
        let animation_duration = animation_data.duration;
        let root = source.root_info()?;
        let mut base = BaseTimelineState {
            position: state.position,
            duration: state.duration,
            frame_rate: source.armature_data()?.frame_rate,
            frame_rate_inverse: 1.0 / source.armature_data()?.frame_rate,
            ..BaseTimelineState::default()
        };
        if timeline_type == TimelineType::Action {
            base.current_time = state.time;
            if base.current_time < 0.0 {
                base.current_time = state.duration - base.current_time;
            }
        }

        let mut value_array_kind = ValueArrayKind::None;
        let mut value_offset = 0;
        let mut value_scale = 1.0;
        let mut value_count = 0usize;
        let mut deform_count = 0usize;
        let mut deform_offset = 0usize;
        let mut same_value_offset = 0usize;

        if let Some(timeline) = &timeline_data {
            let timeline_offset = usize::try_from(timeline.offset).ok()?;
            let ir = &root.ir;
            base.frame_count = *ir
                .timeline_array
                .get(timeline_offset + binary_offset::timeline::KEY_FRAME_COUNT)?;
            base.frame_value_offset = *ir
                .timeline_array
                .get(timeline_offset + binary_offset::timeline::FRAME_VALUE_OFFSET)?;
            base.time_scale = 100.0
                / f64::from(
                    *ir.timeline_array
                        .get(timeline_offset + binary_offset::timeline::SCALE)?,
                );
            base.time_offset = f64::from(
                *ir.timeline_array
                    .get(timeline_offset + binary_offset::timeline::OFFSET)?,
            ) * 0.01;

            match timeline_type {
                TimelineType::BoneAll => {
                    value_array_kind = ValueArrayKind::Float;
                    value_offset = animation_data.frame_float_offset;
                    value_count = 6;
                }
                TimelineType::BoneTranslate => {
                    value_array_kind = ValueArrayKind::Float;
                    value_offset = animation_data.frame_float_offset;
                    value_count = 2;
                    value_scale = host_armature_data.scale;
                }
                TimelineType::BoneRotate | TimelineType::BoneScale => {
                    value_array_kind = ValueArrayKind::Float;
                    value_offset = animation_data.frame_float_offset;
                    value_count = 2;
                }
                TimelineType::SlotColor => {
                    value_array_kind = ValueArrayKind::SlotColor;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 8;
                }
                TimelineType::SlotZIndex => {
                    value_array_kind = ValueArrayKind::Int;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 1;
                }
                TimelineType::SlotAlpha | TimelineType::BoneAlpha => {
                    value_array_kind = ValueArrayKind::Int;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 1;
                    value_scale = 0.01;
                }
                TimelineType::IKConstraint => {
                    value_array_kind = ValueArrayKind::Int;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 2;
                    value_scale = 0.01;
                }
                TimelineType::AnimationProgress | TimelineType::AnimationWeight => {
                    value_array_kind = ValueArrayKind::Int;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 1;
                    value_scale = 0.0001;
                }
                TimelineType::AnimationParameter => {
                    value_array_kind = ValueArrayKind::Int;
                    value_offset = animation_data.frame_int_offset;
                    value_count = 2;
                    value_scale = 0.0001;
                }
                TimelineType::Surface | TimelineType::SlotDeform => {
                    let header_index =
                        usize::try_from(animation_data.frame_int_offset).ok()?
                            + usize::try_from(*ir.timeline_array.get(
                                timeline_offset + binary_offset::timeline::FRAME_VALUE_COUNT,
                            )?)
                            .ok()?;
                    let int_array = &ir.frame_int_array;
                    let value_count_offset = usize::try_from(
                        *int_array.get(header_index + binary_offset::deform::VALUE_COUNT)?,
                    )
                    .ok()?;
                    deform_count = usize::try_from(
                        *int_array.get(header_index + binary_offset::deform::COUNT)?,
                    )
                    .ok()?;
                    deform_offset = usize::try_from(
                        *int_array.get(header_index + binary_offset::deform::VALUE_OFFSET)?,
                    )
                    .ok()?;
                    let mut same_offset =
                        *int_array.get(header_index + binary_offset::deform::FLOAT_OFFSET)?;
                    if timeline_type == TimelineType::SlotDeform && same_offset < 0 {
                        same_offset += 65536;
                    }
                    same_value_offset = usize::try_from(same_offset).ok()?
                        + usize::try_from(animation_data.frame_float_offset).ok()?;
                    value_array_kind = ValueArrayKind::Float;
                    value_offset = animation_data.frame_float_offset;
                    value_scale = host_armature_data.scale;
                    value_count = value_count_offset;
                }
                TimelineType::Action | TimelineType::ZOrder | TimelineType::SlotDisplay => {}
            }
        }

        let buffer_size = if timeline_type == TimelineType::SlotColor {
            8
        } else {
            value_count
        };
        Some(Self {
            id: 0,
            base,
            timeline_type,
            target,
            blend_key,
            is_pose: timeline_data.is_none(),
            timeline_data,
            source,
            animation_frame_offset,
            animation_duration,
            value_array_kind,
            value_offset,
            value_scale,
            value_count,
            current_values: vec![0.0; buffer_size],
            differences: vec![0.0; buffer_size],
            results: vec![0.0; buffer_size],
            tween_frame: None,
            color_offset: 0,
            deform_count,
            deform_offset,
            same_value_offset,
            display_frame,
        })
    }

    /// Updates normalized time and the O(1) frame cursor. Returns `Some(true)`
    /// when the current frame changed, `Some(false)` when it did not, or None
    /// if the immutable IR does not contain a required index.
    pub(crate) fn update_cursor(
        &mut self,
        animation_state: &AnimationState,
        action_time: Option<ActionTimelineTime>,
    ) -> Option<bool> {
        self.update_cursor_at(
            animation_state.time,
            animation_state.play_times,
            animation_state.playhead_state.bits(),
            action_time,
        )
    }

    pub(crate) fn update_cursor_at(
        &mut self,
        passed_time: f64,
        animation_play_times: i32,
        animation_playhead_state: u8,
        action_time: Option<ActionTimelineTime>,
    ) -> Option<bool> {
        let changed = self.base.set_current_time(TimelineTimeInput {
            passed_time,
            action_time,
            animation_play_times,
            animation_playhead_state,
            animation_duration: self.animation_duration,
        });
        if !changed {
            return Some(false);
        }

        if self.base.frame_count > 1 {
            let timeline = self.timeline_data.as_ref()?;
            let frame_index = BaseTimelineState::lookup_frame_index(
                self.base.current_time,
                self.base.frame_rate,
                usize::try_from(timeline.frame_indices_offset).ok()?,
                &self.source.root_info()?.ir.frame_indices,
            )?;
            if self.base.frame_index != frame_index {
                self.base.frame_index = frame_index;
                let timeline_offset = usize::try_from(timeline.offset).ok()?;
                let frame_entry = timeline_offset
                    .checked_add(binary_offset::timeline::FRAME_OFFSET)?
                    .checked_add(usize::try_from(frame_index).ok()?)?;
                let relative = *self
                    .source
                    .root_info()?
                    .ir
                    .timeline_array
                    .get(frame_entry)?;
                self.base.frame_offset = self.animation_frame_offset + relative;
                return Some(true);
            }
            return Some(false);
        }

        if self.base.frame_index < 0 {
            self.base.frame_index = 0;
            if let Some(timeline) = &self.timeline_data {
                let timeline_offset = usize::try_from(timeline.offset).ok()?;
                let relative = *self
                    .source
                    .root_info()?
                    .ir
                    .timeline_array
                    .get(timeline_offset + binary_offset::timeline::FRAME_OFFSET)?;
                self.base.frame_offset = self.animation_frame_offset + relative;
            }
            return Some(true);
        }
        Some(false)
    }

    /// Advances the shared cursor and refreshes sampled/tweened values.
    /// Returns `(time_changed, frame_changed)` for the concrete axis updater.
    pub(crate) fn update_values(
        &mut self,
        passed_time: f64,
        animation_play_times: i32,
        animation_playhead_state: u8,
        action_time: Option<ActionTimelineTime>,
    ) -> Option<(bool, bool)> {
        let previous_time = self.base.current_time;
        let previous_play_times = self.base.current_play_times;
        let frame_changed = self.update_cursor_at(
            passed_time,
            animation_play_times,
            animation_playhead_state,
            action_time,
        )?;
        let time_changed = previous_time != self.base.current_time
            || previous_play_times != self.base.current_play_times;
        if !time_changed {
            return Some((false, false));
        }

        if frame_changed {
            // The action axis is the authority for the last-frame tween gate
            // (`BaseTimelineState.ts:304-305`), not this axis' own normalized
            // play times. `action_time` carries the action timeline snapshot.
            let action_play_times =
                action_time.map_or(self.base.current_play_times, |time| time.current_play_times);
            self.arrive_at_frame(animation_play_times, action_play_times)?;
        }
        if let Some(tween) = self.tween_frame
            && tween.is_tween
        {
            let root = self.source.root_info()?;
            let progress = tween.progress(
                self.base.current_time,
                &root.ir.frame_array,
                usize::try_from(self.base.frame_offset).ok()? + binary_offset::frame::CURVE_SAMPLES,
            )?;
            for index in 0..self.value_count {
                let current = *self.current_values.get(index)?;
                let difference = *self.differences.get(index)?;
                let mut result = current + difference * progress;
                if self.value_array_kind == ValueArrayKind::SlotColor && index < 4 {
                    result *= 0.01;
                }
                *self.results.get_mut(index)? = result;
            }
            self.base.dirty = true;
        }
        Some((true, frame_changed))
    }

    pub(crate) fn fade_out(&mut self) {
        match self.timeline_type {
            TimelineType::SlotColor => {
                if let Some(tween) = self.tween_frame.as_mut() {
                    tween.is_tween = false;
                }
            }
            TimelineType::BoneAll => {
                self.base.dirty = false;
                for index in [2, 3] {
                    if let Some(value) = self.results.get_mut(index) {
                        *value = crate::geom::transform::normalize_radian(*value);
                    }
                }
            }
            TimelineType::BoneRotate => {
                self.base.dirty = false;
                for index in [0, 1] {
                    if let Some(value) = self.results.get_mut(index) {
                        *value = crate::geom::transform::normalize_radian(*value);
                    }
                }
            }
            _ => self.base.dirty = false,
        }
    }

    fn arrive_at_frame(&mut self, animation_play_times: i32, action_play_times: i32) -> Option<()> {
        let mut tween = if self.timeline_data.is_some() {
            let root = self.source.root_info()?;
            let next_frame_offset = if self.base.frame_index < self.base.frame_count - 1 {
                let timeline = self.timeline_data.as_ref()?;
                let timeline_offset = usize::try_from(timeline.offset).ok()?;
                let next_relative = *root.ir.timeline_array.get(
                    timeline_offset
                        + binary_offset::timeline::FRAME_OFFSET
                        + usize::try_from(self.base.frame_index + 1).ok()?,
                )?;
                Some(usize::try_from(self.animation_frame_offset + next_relative).ok()?)
            } else {
                None
            };
            Some(TweenFrameParameters::arrive_at_frame(
                &root.ir.frame_array,
                usize::try_from(self.base.frame_offset).ok()?,
                next_frame_offset,
                self.base.frame_index,
                self.base.frame_count,
                animation_play_times,
                // `_animationState.currentPlayTimes` mirrors the action axis
                // (`BaseTimelineState.ts:304-305`).
                action_play_times,
                self.animation_duration,
                self.base.frame_rate_inverse,
            )?)
        } else {
            None
        };

        if let Some(parameters) = tween.as_mut()
            && !parameters.is_tween
        {
            parameters.dirty = true;
        }
        self.tween_frame = tween;
        self.base.dirty = true;

        if self.timeline_data.is_none() || self.value_array_kind == ValueArrayKind::None {
            return Some(());
        }
        match self.value_array_kind {
            ValueArrayKind::SlotColor => self.read_slot_color_values(),
            ValueArrayKind::Int | ValueArrayKind::Float => self.read_numeric_values(),
            ValueArrayKind::None => Some(()),
        }?;
        if self.timeline_type == TimelineType::BoneRotate
            && self.tween_frame.is_some_and(|frame| frame.is_tween)
            && self.base.frame_index == self.base.frame_count - 1
        {
            for difference in self.differences.iter_mut().take(2) {
                *difference = crate::geom::transform::normalize_radian(*difference);
            }
        }
        Some(())
    }

    fn read_numeric_values(&mut self) -> Option<()> {
        let root = self.source.root_info()?;
        self.timeline_data.as_ref()?;
        let frame_value_index =
            usize::try_from(self.value_offset + self.base.frame_value_offset).ok()?;
        let frame_index = usize::try_from(self.base.frame_index).ok()?;
        let current_offset =
            frame_value_index.checked_add(frame_index.checked_mul(self.value_count)?)?;
        let next_index = if self.base.frame_index == self.base.frame_count - 1 {
            0
        } else {
            frame_index.checked_add(1)?
        };
        let next_offset =
            frame_value_index.checked_add(next_index.checked_mul(self.value_count)?)?;
        let is_tween = self.tween_frame.is_some_and(|tween| tween.is_tween);

        for index in 0..self.value_count {
            let raw_current = match self.value_array_kind {
                ValueArrayKind::Int => {
                    f64::from(*root.ir.frame_int_array.get(current_offset + index)?)
                }
                ValueArrayKind::Float => *root.ir.frame_float_array.get(current_offset + index)?,
                _ => return None,
            };
            let current = raw_current * self.value_scale;
            if is_tween {
                // Single/Double only refresh `_current*` on tween arrivals
                // (`BaseTimelineState.ts:390-397`, `:452-463`).
                self.current_values[index] = current;
                let raw_next = match self.value_array_kind {
                    ValueArrayKind::Int => {
                        f64::from(*root.ir.frame_int_array.get(next_offset + index)?)
                    }
                    ValueArrayKind::Float => *root.ir.frame_float_array.get(next_offset + index)?,
                    _ => return None,
                };
                self.differences[index] = if self.is_multiple_value() {
                    // MutilpleValue order: subtract first, then scale
                    // (`BaseTimelineState.ts:522-524`).
                    (raw_next - raw_current) * self.value_scale
                } else {
                    // Single/Double order: `next * scale - current`
                    // (`BaseTimelineState.ts:394-397`, `:458-463`).
                    raw_next * self.value_scale - current
                };
            } else {
                self.results[index] = current;
            }
        }
        Some(())
    }

    fn is_multiple_value(&self) -> bool {
        matches!(
            self.timeline_type,
            TimelineType::BoneAll | TimelineType::Surface | TimelineType::SlotDeform
        )
    }

    fn read_slot_color_values(&mut self) -> Option<()> {
        let root = self.source.root_info()?;
        let frame_value_offset = usize::try_from(
            self.value_offset + self.base.frame_value_offset + self.base.frame_index,
        )
        .ok()?;
        let raw_color_offset = *root.ir.frame_int_array.get(frame_value_offset)?;
        let color_offset = usize::try_from(if raw_color_offset < 0 {
            raw_color_offset + 65536
        } else {
            raw_color_offset
        })
        .ok()?;
        let is_tween = self.tween_frame.is_some_and(|tween| tween.is_tween);
        let next_frame_index = if self.base.frame_index == self.base.frame_count - 1 {
            0
        } else {
            self.base.frame_index + 1
        };
        let next_color_offset = if is_tween {
            let next_value_index = usize::try_from(
                self.value_offset + self.base.frame_value_offset + next_frame_index,
            )
            .ok()?;
            let raw = *root.ir.frame_int_array.get(next_value_index)?;
            usize::try_from(if raw < 0 { raw + 65536 } else { raw }).ok()?
        } else {
            color_offset
        };

        for index in 0..8 {
            let current = f64::from(*root.ir.color_array.get(color_offset + index)?);
            if is_tween {
                // SlotColor only fills `_current` for tween frames
                // (`TimelineState.ts:681-710`).
                self.current_values[index] = current;
                let next = f64::from(*root.ir.color_array.get(next_color_offset + index)?);
                self.differences[index] = next - current;
            } else {
                // `TimelineState.ts:711-720`: multipliers *0.01, offsets raw.
                self.results[index] = if index < 4 { current * 0.01 } else { current };
            }
        }
        self.color_offset = i32::try_from(color_offset).ok()?;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::TimelineTarget;
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::test_support::{
        action_time, build_data, make_state, make_timeline, numeric_timeline, timeline_data,
        timeline_ir,
    };
    use crate::armature::BoneIdx;
    use crate::core::enums::TimelineType;
    use crate::model::{AnimationData, ArmatureData};

    #[test]
    fn multiple_value_difference_subtracts_before_scaling() {
        // BaseTimelineState.ts:522-524: `(next - current) * valueScale`.
        // Values/scale are chosen so the endpoint-scaled order differs in f64.
        let scale = 1.793_943_415_325_198_3;
        let current = -841.647_216_796_875_f64;
        let next = -800.984_863_281_25_f64;
        let mut float_values = vec![0.0; 14];
        float_values[0] = current;
        float_values[1] = next;
        let ir = timeline_ir(
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            0,
            &float_values,
            &[0, 1, 1, 0, 10],
        );
        let data = build_data(
            AnimationData {
                duration: 2.0,
                frame_count: 2,
                ..AnimationData::default()
            },
            ArmatureData {
                frame_rate: 24.0,
                scale,
                ..ArmatureData::default()
            },
            ir,
        );
        let state = make_state(&data, false);
        let mut timeline = make_timeline(
            &data,
            &state,
            TimelineType::Surface,
            TimelineTarget::Bone(BoneIdx(0)),
            Some(timeline_data(TimelineType::Surface)),
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.differences[0], (next - current) * scale);
    }

    #[test]
    fn double_value_difference_scales_each_endpoint_before_subtracting() {
        // BaseTimelineState.ts:458-463: `next * scale - current`, which differs
        // from `(next - current) * scale` for these Int16 endpoints.
        let mut timeline = numeric_timeline(
            TimelineType::IKConstraint,
            TimelineTarget::Constraint(crate::armature::ConstraintIdx(0)),
            2,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[-32768.0, 0.0, -32767.0, 0.0],
            true,
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(
            timeline.differences[0],
            -32767.0_f64 * 0.01 - (-32768.0_f64 * 0.01)
        );
    }

    #[test]
    fn last_frame_tween_gate_reads_action_play_times_not_axis_play_times() {
        // BaseTimelineState.ts:304-305 uses `_animationState.currentPlayTimes`;
        // a time-scaled axis computes its own (here 2) but the action still
        // reports 0, which keeps the final frame tweenable.
        let mut timeline = numeric_timeline(
            TimelineType::BoneRotate,
            TimelineTarget::Bone(BoneIdx(0)),
            2,
            1.0,
            24.0,
            1.0,
            &[0, 24],
            &[1, 1],
            &[0.0, 0.0, 5.0, 5.0],
            false,
        );
        timeline.base.time_scale = 2.0;
        assert_eq!(
            timeline.update_values(1.0, 2, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.base.current_play_times, 2);
        assert_eq!(timeline.base.frame_index, 1);
        assert!(timeline.tween_frame.is_some_and(|tween| tween.is_tween));
    }
}
