//! SlotDisplay/Color/ZIndex/Alpha/Deform timeline axes.
//!
//! D-4.35 (pending registration): official `SlotZIndexTimelineState` keeps
//! `slot._zIndex` as a JS `number` and can accumulate fractional tween values
//! (`TimelineState.ts:843/846`), while the stage-3 runtime type is `i32`.
//! The truncation in [`blend`] is an intentional, documented divergence until
//! the runtime type is revisited (stage 6 review; no asset currently ships
//! SlotZIndex data).

use crate::animation::AnimationUpdateCtx;
use crate::animation::animation_state::{FadeState, SubFadeState};
use crate::animation::timeline_state::TimelinePlayState;
use crate::animation::timeline_states::{TimelineInstance, TimelineTarget};
use crate::core::enums::TimelineType;
use crate::geom::color::ColorTransform;

pub(crate) fn update_direct(
    timeline: &mut TimelineInstance,
    context: &mut AnimationUpdateCtx<'_>,
    active: bool,
    fade_state: FadeState,
    sub_fade_state: SubFadeState,
    fade_progress: f64,
) {
    let TimelineTarget::Slot(target) = timeline.target else {
        return;
    };
    let Some(slot) = context.slots.iter_mut().find(|slot| slot.idx == target) else {
        return;
    };

    match timeline.timeline_type {
        // `SlotDisplayTimelineState._onArriveAtFrame` requires the play state
        // left by `_setCurrentTime` to be `>= 0` (`TimelineState.ts:642`).
        // `TimelinePlayState::Start` is -1, so pause-fade-in must not switch.
        TimelineType::SlotDisplay
            if active && timeline.base.play_state >= TimelinePlayState::Play =>
        {
            let display_index = if timeline.is_pose {
                context
                    .armature_data
                    .slots
                    .get(&slot.slot_name)
                    .map_or(-1, |data| data.display_index)
            } else {
                let Ok(frame_offset) = usize::try_from(timeline.base.frame_offset) else {
                    return;
                };
                let Some(display_index) = timeline
                    .source
                    .root_info()
                    .and_then(|root| root.ir.frame_array.get(frame_offset + 1))
                else {
                    return;
                };
                *display_index
            };
            let before = slot.display_index;
            slot.set_display_index(display_index, true);
            if before != slot.display_index {
                *context.slots_dirty = true;
            }
            timeline.base.dirty = false;
        }
        TimelineType::SlotColor => {
            if !active && !timeline.base.dirty {
                return;
            }
            if !timeline.is_pose
                && !timeline.base.dirty
                && timeline.tween_frame.is_none_or(|t| !t.is_tween)
            {
                return;
            }
            let target_color = if timeline.is_pose {
                context
                    .armature_data
                    .slots
                    .get(&slot.slot_name)
                    .map_or(ColorTransform::default(), |data| data.color)
            } else {
                color_from_results(&timeline.results)
            };
            let mut changed = false;
            if fade_state != FadeState::Complete || sub_fade_state != SubFadeState::Fading {
                let progress = fade_progress.powf(4.0);
                let mut color = slot.color_transform;
                color.alpha_multiplier +=
                    (target_color.alpha_multiplier - color.alpha_multiplier) * progress;
                color.red_multiplier +=
                    (target_color.red_multiplier - color.red_multiplier) * progress;
                color.green_multiplier +=
                    (target_color.green_multiplier - color.green_multiplier) * progress;
                color.blue_multiplier +=
                    (target_color.blue_multiplier - color.blue_multiplier) * progress;
                color.alpha_offset += (target_color.alpha_offset - color.alpha_offset) * progress;
                color.red_offset += (target_color.red_offset - color.red_offset) * progress;
                color.green_offset += (target_color.green_offset - color.green_offset) * progress;
                color.blue_offset += (target_color.blue_offset - color.blue_offset) * progress;
                if color != slot.color_transform {
                    slot.color_transform = color;
                    changed = true;
                }
            } else if timeline.base.dirty {
                if target_color != slot.color_transform {
                    slot.color_transform = target_color;
                    changed = true;
                }
                timeline.base.dirty = false;
            }
            if changed {
                slot.color_dirty = true;
            }
        }
        _ => {}
    }
}

pub(crate) fn blend(
    timeline: &mut TimelineInstance,
    context: &mut AnimationUpdateCtx<'_>,
    blend_weight: f64,
    blend_dirty: u32,
    is_blend_dirty: bool,
) {
    let TimelineTarget::Slot(target) = timeline.target else {
        return;
    };
    let Some(slot) = context.slots.iter_mut().find(|slot| slot.idx == target) else {
        return;
    };
    let mut deform_dirty = false;
    match timeline.timeline_type {
        TimelineType::SlotAlpha => {
            let value = if timeline.is_pose {
                1.0
            } else {
                timeline.results.first().copied().unwrap_or(1.0)
            };
            if blend_dirty > 1 {
                slot.alpha += value * blend_weight;
                if slot.alpha > 1.0 {
                    slot.alpha = 1.0;
                }
            } else {
                slot.alpha = value * blend_weight;
            }
            if is_blend_dirty || timeline.base.dirty {
                *context.alpha_dirty = true;
            }
            timeline.base.dirty = false;
        }
        TimelineType::SlotZIndex => {
            let value = if timeline.is_pose {
                context
                    .armature_data
                    .slots
                    .get(&slot.slot_name)
                    .map_or(0.0, |data| f64::from(data.z_index))
            } else {
                timeline.results.first().copied().unwrap_or(0.0)
            };
            // D-4.35: TS `slot._zIndex +=/= _result * blendWeight`
            // (`TimelineState.ts:843/846`) keeps a fractional number; the
            // runtime field is i32, so this `as i32` intentionally truncates.
            if blend_dirty > 1 {
                slot.z_index += (value * blend_weight) as i32;
            } else {
                slot.z_index = (value * blend_weight) as i32;
            }
            if is_blend_dirty || timeline.base.dirty {
                *context.z_index_dirty = true;
            }
            timeline.base.dirty = false;
        }
        TimelineType::SlotDeform => {
            deform_dirty =
                super::deform::blend(timeline, slot, blend_weight, blend_dirty, is_blend_dirty);
        }
        _ => {}
    }
    if deform_dirty {
        slot.vertices_dirty = true;
    }
}

fn color_from_results(values: &[f64]) -> ColorTransform {
    ColorTransform {
        alpha_multiplier: values.first().copied().unwrap_or(1.0),
        red_multiplier: values.get(1).copied().unwrap_or(1.0),
        green_multiplier: values.get(2).copied().unwrap_or(1.0),
        blue_multiplier: values.get(3).copied().unwrap_or(1.0),
        alpha_offset: values.get(4).copied().unwrap_or(0.0),
        red_offset: values.get(5).copied().unwrap_or(0.0),
        green_offset: values.get(6).copied().unwrap_or(0.0),
        blue_offset: values.get(7).copied().unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{blend, update_direct};
    use crate::animation::animation_state::{FadeState, SubFadeState};
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        CtxFixture, action_time, build_data, make_state, make_timeline, numeric_timeline,
        pose_timeline, slot_color_timeline, timeline_data, timeline_ir,
    };
    use crate::armature::{Slot, SlotIdx};
    use crate::core::enums::TimelineType;
    use crate::geom::color::ColorTransform;
    use crate::model::{AnimationData, ArmatureData};

    fn approx(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-12, "{left} != {right}");
    }

    fn slot_fixture() -> (CtxFixture, Slot) {
        let slot = Slot {
            idx: SlotIdx(0),
            slot_name: "slot".to_owned(),
            ..Slot::default()
        };
        (CtxFixture::new(ArmatureData::default()), slot)
    }

    fn display_timeline(display_index: i32) -> crate::animation::timeline_states::TimelineInstance {
        let ir = timeline_ir(24.0, 1.0, &[0], &[display_index], 0, &[], &[]);
        let animation = AnimationData {
            duration: 1.0,
            frame_count: 1,
            ..AnimationData::default()
        };
        let armature = ArmatureData {
            frame_rate: 24.0,
            ..ArmatureData::default()
        };
        let data = build_data(animation, armature, ir);
        let state = make_state(&data, false);
        make_timeline(
            &data,
            &state,
            TimelineType::SlotDisplay,
            TimelineTarget::Slot(SlotIdx(0)),
            Some(timeline_data(TimelineType::SlotDisplay)),
        )
    }

    #[test]
    fn slot_display_requires_play_state_at_or_after_play() {
        // TimelineState.ts:642 gate: playState >= 0.
        let mut paused = display_timeline(2);
        assert_eq!(
            paused.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Start))),
            Some((true, true))
        );
        let (mut fixture, mut slot) = slot_fixture();
        slot.display_frames = (0..3).map(|_| Default::default()).collect();
        fixture.slots.push(slot);
        fixture.with_context(|context| {
            update_direct(
                &mut paused,
                context,
                true,
                FadeState::Complete,
                SubFadeState::Complete,
                1.0,
            );
        });
        assert_eq!(fixture.slots[0].display_index, -1);

        let mut playing = display_timeline(2);
        assert_eq!(
            playing.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        fixture.with_context(|context| {
            update_direct(
                &mut playing,
                context,
                true,
                FadeState::Complete,
                SubFadeState::Complete,
                1.0,
            );
        });
        assert_eq!(fixture.slots[0].display_index, 2);
    }

    #[test]
    fn slot_color_reads_indirect_offsets_with_int16_wrap_and_multiplier_scale() {
        // Negative color offsets are Int16-wrapped by +65536
        // (TimelineState.ts:677-679).
        let mut color_array = vec![0; 32776];
        color_array[32768..32776].copy_from_slice(&[200, 50, 60, 70, 1, 2, 3, 4]);
        let timeline = slot_color_timeline(24.0, 1.0, &[0], &[0], &[-32768], color_array);
        let mut timeline = timeline;
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        for (index, want) in [2.0, 0.5, 0.6, 0.7, 1.0, 2.0, 3.0, 4.0]
            .into_iter()
            .enumerate()
        {
            approx(timeline.results[index], want);
        }
    }

    #[test]
    fn slot_color_tween_interpolates_then_scales_only_the_multipliers() {
        let color_array = vec![
            200, 50, 60, 70, 1, 2, 3, 4, 0, 100, 200, 300, 11, 22, 33, 44,
        ];
        // TimelineState.ts:740-747: `(current + difference * progress) * 0.01`
        // applies only to components 0..4.
        let mut timeline = slot_color_timeline(24.0, 2.0, &[0, 24], &[1, 0], &[0, 8], color_array);
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        for (index, want) in [1.0, 0.75, 1.3, 1.85, 6.0, 12.0, 18.0, 24.0]
            .into_iter()
            .enumerate()
        {
            approx(timeline.results[index], want);
        }
    }

    #[test]
    fn slot_color_fade_uses_fade_progress_to_the_fourth() {
        let color_array = vec![100, 200, 300, 400, 10, 20, 30, 40];
        let mut timeline = slot_color_timeline(24.0, 1.0, &[0], &[0], &[0], color_array);
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        let (mut fixture, mut slot) = slot_fixture();
        slot.color_transform = ColorTransform::default();
        fixture.slots.push(slot);
        fixture.with_context(|context| {
            update_direct(
                &mut timeline,
                context,
                true,
                FadeState::FadeIn,
                SubFadeState::Fading,
                0.5,
            );
        });
        let color = fixture.slots[0].color_transform;
        approx(color.alpha_multiplier, 1.0);
        approx(color.red_multiplier, 1.0625);
        approx(color.green_multiplier, 1.125);
        approx(color.blue_multiplier, 1.1875);
        approx(color.alpha_offset, 0.625);
        approx(color.red_offset, 1.25);
        approx(color.green_offset, 1.875);
        approx(color.blue_offset, 2.5);
        assert!(fixture.slots[0].color_dirty);
    }

    #[test]
    fn slot_alpha_accumulates_clamps_and_pose_falls_back_to_one() {
        // TimelineState.ts:596-624 via BaseTimelineState.ts:390-414.
        let mut timeline = numeric_timeline(
            TimelineType::SlotAlpha,
            TimelineTarget::Slot(SlotIdx(0)),
            1,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[50.0, 80.0],
            true,
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        approx(timeline.results[0], 0.65);

        let (mut fixture, mut slot) = slot_fixture();
        slot.alpha = 0.9;
        fixture.slots.push(slot);
        fixture.with_context(|context| blend(&mut timeline, context, 1.0, 1, true));
        approx(fixture.slots[0].alpha, 0.65);
        assert!(fixture.alpha_dirty);

        fixture.slots[0].alpha = 0.2;
        fixture.with_context(|context| blend(&mut timeline, context, 1.0, 2, true));
        approx(fixture.slots[0].alpha, 0.85);

        let mut pose = pose_timeline(TimelineType::SlotAlpha, TimelineTarget::Slot(SlotIdx(0)));
        fixture.with_context(|context| blend(&mut pose, context, 1.0, 1, true));
        approx(fixture.slots[0].alpha, 1.0);
    }

    #[test]
    fn slot_z_index_accumulates_or_overwrites_with_documented_i32_truncation() {
        // D-4.35: TS keeps a fractional number (TimelineState.ts:843/846).
        let mut timeline = numeric_timeline(
            TimelineType::SlotZIndex,
            TimelineTarget::Slot(SlotIdx(0)),
            1,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[2.0, 5.0],
            true,
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        approx(timeline.results[0], 3.5);

        let (mut fixture, mut slot) = slot_fixture();
        slot.z_index = 10;
        fixture.slots.push(slot);
        fixture.with_context(|context| blend(&mut timeline, context, 1.0, 1, true));
        assert_eq!(fixture.slots[0].z_index, 3);
        assert!(fixture.z_index_dirty);

        fixture.slots[0].z_index = 1;
        fixture.with_context(|context| blend(&mut timeline, context, 1.0, 2, true));
        assert_eq!(fixture.slots[0].z_index, 4);
    }
}
