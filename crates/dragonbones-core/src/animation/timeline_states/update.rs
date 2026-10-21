//! Fixed-order timeline group dispatch and per-frame BlendState allocation.

use std::collections::HashMap;

use crate::animation::AnimationUpdateCtx;
use crate::animation::animation_state::{
    AnimationState, FadeState, SubFadeState, TIMELINE_GROUP_UPDATE_ORDER, TimelineGroup,
};
use crate::animation::blend_state::{BlendState, BlendStateKey};
use crate::animation::timeline_state::ActionTimelineTime;

use super::{TimelineInstance, TimelineTarget};

pub(crate) fn update_groups(
    state: &mut AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
    blend_states: &mut HashMap<BlendStateKey, BlendState>,
    action_time: Option<ActionTimelineTime>,
    update_bone_timeline: bool,
) {
    let state_time = state.time;
    let play_times = state.play_times;
    let playhead_state = state.playhead_state.bits();
    let weight_result = state.weight_result;
    let layer = state.layer;
    let display_control = state.display_control;
    let state_name = state.name.clone();
    let state_group = state.group.clone();
    let fade_state = state.fade_state;
    let sub_fade_state = state.sub_fade_state;
    let fade_progress = state.fade_progress;
    let blend_dirty =
        state.fade_state != FadeState::Complete || state.sub_fade_state == SubFadeState::Fading;

    for group in TIMELINE_GROUP_UPDATE_ORDER {
        let timelines = state.timeline_group_mut(group);
        let mut previous_bone_key: Option<BlendStateKey> = None;
        let mut previous_bone_blend = false;

        for timeline in timelines {
            if !update_bone_timeline && matches!(timeline.target, TimelineTarget::Bone(_)) {
                continue;
            }
            let active =
                timeline.base.play_state <= super::super::timeline_state::TimelinePlayState::Play;
            if active {
                let Some((time_changed, frame_changed)) =
                    timeline.update_values(state_time, play_times, playhead_state, action_time)
                else {
                    continue;
                };
                let _ = (time_changed, frame_changed);
            }

            match group {
                TimelineGroup::Bone => {
                    let Some(key) = timeline.blend_key.clone() else {
                        continue;
                    };
                    let (blend, dirty, blend_weight) = if previous_bone_key.as_ref() == Some(&key) {
                        blend_states.get(&key).map_or((false, 0, 0.0), |value| {
                            (previous_bone_blend, value.dirty, value.blend_weight)
                        })
                    } else {
                        let blend_state = blend_states.entry(key.clone()).or_default();
                        let blend = blend_state.update(layer, weight_result);
                        let dirty = blend_state.dirty;
                        let blend_weight = blend_state.blend_weight;
                        if dirty == 1 {
                            reset_bone_target(timeline, context);
                        }
                        previous_bone_key = Some(key);
                        previous_bone_blend = blend;
                        (blend, dirty, blend_weight)
                    };
                    if blend {
                        super::bone::blend_transform(
                            timeline,
                            context,
                            blend_weight,
                            dirty,
                            blend_dirty,
                        );
                    }
                }
                TimelineGroup::BoneBlend => {
                    let Some(key) = timeline.blend_key.clone() else {
                        continue;
                    };
                    let blend_state = blend_states.entry(key).or_default();
                    if blend_state.update(layer, weight_result) {
                        let dirty = blend_state.dirty;
                        super::bone::blend_auxiliary(
                            timeline,
                            context,
                            blend_state.blend_weight,
                            dirty,
                            blend_dirty,
                        );
                    }
                }
                TimelineGroup::Slot if display_control => {
                    let allowed = match timeline.target {
                        TimelineTarget::Slot(target) => context
                            .slots
                            .iter()
                            .find(|slot| slot.idx == target)
                            .is_some_and(|slot| {
                                slot.display_controller.as_ref().is_none_or(|controller| {
                                    controller == &state_name || controller == &state_group
                                })
                            }),
                        _ => false,
                    };
                    if allowed {
                        super::slot::update_direct(
                            timeline,
                            context,
                            active,
                            fade_state,
                            sub_fade_state,
                            fade_progress,
                        );
                    }
                }
                TimelineGroup::Slot => {}
                TimelineGroup::SlotBlend if active => {
                    let Some(key) = timeline.blend_key.clone() else {
                        continue;
                    };
                    let blend_state = blend_states.entry(key).or_default();
                    if blend_state.update(layer, weight_result) {
                        super::slot::blend(
                            timeline,
                            context,
                            blend_state.blend_weight,
                            blend_state.dirty,
                            blend_dirty,
                        );
                    }
                }
                TimelineGroup::SlotBlend => {}
                TimelineGroup::Constraint if active => {
                    super::constraint::update(timeline, context);
                }
                TimelineGroup::Constraint => {}
                TimelineGroup::Animation if active => {
                    super::animation::update(timeline);
                }
                TimelineGroup::Animation => {}
            }
        }
    }
}

fn reset_bone_target(timeline: &TimelineInstance, context: &mut AnimationUpdateCtx<'_>) {
    let TimelineTarget::Bone(bone_idx) = timeline.target else {
        return;
    };
    let Some(bone) = context.bones.iter_mut().find(|bone| bone.idx == bone_idx) else {
        return;
    };
    bone.animation_pose = crate::geom::Transform::default();
}
