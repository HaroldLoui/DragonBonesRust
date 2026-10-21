//! ActionTimelineState frame crossing and event/action emission.

use crate::core::enums::ActionType;
use crate::event::EventObject;

use super::super::animation_state::AnimationState;
use super::super::timeline_state::{BaseTimelineState, TimelinePlayState};
use super::TimelineInstance;
use crate::animation::AnimationUpdateCtx;
use crate::armature::RuntimeCtx;

pub(crate) fn update_action_timeline(
    timeline: &mut TimelineInstance,
    state: &mut AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
    runtime: &mut RuntimeCtx<'_>,
) {
    if timeline.source.animation_data().is_none() {
        return;
    }
    let Some(actions) = timeline
        .source
        .armature_data()
        .map(|armature| armature.actions.clone())
    else {
        return;
    };

    let previous_state = timeline.base.play_state;
    let previous_play_times = timeline.base.current_play_times;
    let previous_time = timeline.base.current_time;
    let mut previous_frame_index = timeline.base.frame_index;
    let play_state_changed = timeline.update_cursor(state, None);
    if play_state_changed.is_none() {
        return;
    }
    if !play_state_changed.unwrap_or(false) {
        return;
    }

    // `_setCurrentTime` clears its frame cursor at loop boundaries before
    // ActionTimelineState captures the previous cursor. Preserve that crossing
    // sentinel even though `update_cursor` has already installed the new frame.
    if timeline.base.current_play_times != previous_play_times
        && timeline.base.play_state <= TimelinePlayState::Play
    {
        previous_frame_index = -1;
    }

    state.current_play_times = timeline.base.current_play_times;
    state.action_play_state = timeline.base.play_state;
    let event_active = state.parent.is_none() && state.action_enabled;

    if previous_state == TimelinePlayState::Start {
        if timeline.base.play_state != TimelinePlayState::Start {
            if state.display_control && state.reset_to_pose {
                context.sort_z_order(None, 0);
            }
            if event_active && context.proxy.has_db_event_listener(EventObject::START) {
                context.buffer_lifecycle_event(runtime, EventObject::START);
            }
        } else {
            return;
        }
    }

    let is_reverse = state.time_scale < 0.0;
    let mut loop_complete_event = if event_active
        && timeline.base.current_play_times != previous_play_times
        && context
            .proxy
            .has_db_event_listener(EventObject::LOOP_COMPLETE)
    {
        Some(EventObject::LOOP_COMPLETE)
    } else {
        None
    };
    let mut complete_event = if event_active
        && timeline.base.current_play_times != previous_play_times
        && timeline.base.play_state == TimelinePlayState::Complete
        && context.proxy.has_db_event_listener(EventObject::COMPLETE)
    {
        Some(EventObject::COMPLETE)
    } else {
        None
    };

    if timeline.base.frame_count > 1 {
        let Some(timeline_data) = timeline.timeline_data.as_ref() else {
            return;
        };
        let Some(frame_index) = BaseTimelineState::lookup_frame_index(
            timeline.base.current_time,
            timeline.base.frame_rate,
            usize::try_from(timeline_data.frame_indices_offset).unwrap_or(usize::MAX),
            timeline
                .source
                .root_info()
                .map_or(&[][..], |root| &root.ir.frame_indices),
        ) else {
            return;
        };

        if previous_frame_index != frame_index {
            let mut crossed_frame_index = previous_frame_index;
            if is_reverse {
                if crossed_frame_index < 0 {
                    crossed_frame_index =
                        lookup_frame_at_time(timeline, previous_time).unwrap_or(-1);
                    if timeline.base.current_play_times == previous_play_times
                        && crossed_frame_index == frame_index
                    {
                        crossed_frame_index = -1;
                    }
                }

                while crossed_frame_index >= 0 {
                    if frame_in_play_interval(timeline, crossed_frame_index) {
                        cross_frame(
                            timeline,
                            &actions,
                            state,
                            context,
                            runtime,
                            crossed_frame_index,
                        );
                    }
                    if loop_complete_event.is_some() && crossed_frame_index == 0 {
                        context
                            .buffer_lifecycle_event(runtime, loop_complete_event.take().unwrap());
                    }
                    crossed_frame_index = if crossed_frame_index > 0 {
                        crossed_frame_index - 1
                    } else {
                        timeline.base.frame_count - 1
                    };
                    if crossed_frame_index == frame_index {
                        break;
                    }
                }
            } else {
                if crossed_frame_index < 0 {
                    crossed_frame_index =
                        lookup_frame_at_time(timeline, previous_time).unwrap_or(-1);
                    if crossed_frame_index >= 0 {
                        let frame_position =
                            frame_position(timeline, crossed_frame_index).unwrap_or(f64::NAN);
                        if timeline.base.current_play_times == previous_play_times {
                            if previous_time <= frame_position {
                                crossed_frame_index = if crossed_frame_index > 0 {
                                    crossed_frame_index - 1
                                } else {
                                    timeline.base.frame_count - 1
                                };
                            } else if crossed_frame_index == frame_index {
                                crossed_frame_index = -1;
                            }
                        }
                    }
                }

                while crossed_frame_index >= 0 {
                    crossed_frame_index = if crossed_frame_index < timeline.base.frame_count - 1 {
                        crossed_frame_index + 1
                    } else {
                        0
                    };
                    if frame_in_play_interval(timeline, crossed_frame_index) {
                        cross_frame(
                            timeline,
                            &actions,
                            state,
                            context,
                            runtime,
                            crossed_frame_index,
                        );
                    }
                    if loop_complete_event.is_some() && crossed_frame_index == 0 {
                        context
                            .buffer_lifecycle_event(runtime, loop_complete_event.take().unwrap());
                    }
                    if crossed_frame_index == frame_index {
                        break;
                    }
                }
            }
        }
    } else if previous_frame_index < 0 {
        timeline.base.frame_index = 0;
        if timeline.timeline_data.is_some() {
            let Some(position) = frame_position(timeline, 0) else {
                return;
            };
            if timeline.base.current_play_times == previous_play_times {
                if previous_time <= position {
                    cross_frame(timeline, &actions, state, context, runtime, 0);
                }
            } else if state.position <= position {
                if !is_reverse && loop_complete_event.is_some() {
                    context.buffer_lifecycle_event(runtime, loop_complete_event.take().unwrap());
                }
                cross_frame(timeline, &actions, state, context, runtime, 0);
            }
        }
    }

    if let Some(event_type) = loop_complete_event {
        context.buffer_lifecycle_event(runtime, event_type);
    }
    if let Some(event_type) = complete_event.take() {
        context.buffer_lifecycle_event(runtime, event_type);
    }
}

fn lookup_frame_at_time(timeline: &TimelineInstance, time: f64) -> Option<i32> {
    let timeline_data = timeline.timeline_data.as_ref()?;
    let root = timeline.source.root_info()?;
    BaseTimelineState::lookup_frame_index(
        time,
        timeline.base.frame_rate,
        usize::try_from(timeline_data.frame_indices_offset).ok()?,
        &root.ir.frame_indices,
    )
}

fn frame_position(timeline: &TimelineInstance, frame_index: i32) -> Option<f64> {
    let timeline_data = timeline.timeline_data.as_ref()?;
    let root = timeline.source.root_info()?;
    let timeline_offset = usize::try_from(timeline_data.offset).ok()?;
    let frame_offset = timeline.animation_frame_offset
        + *root.ir.timeline_array.get(
            timeline_offset
                + crate::core::enums::binary_offset::timeline::FRAME_OFFSET
                + usize::try_from(frame_index).ok()?,
        )?;
    let position = *root
        .ir
        .frame_array
        .get(usize::try_from(frame_offset).ok()?)?;
    // ActionTimelineState deliberately divides at the read site (F4.16).
    Some(f64::from(position) / timeline.base.frame_rate)
}

fn frame_in_play_interval(timeline: &TimelineInstance, frame_index: i32) -> bool {
    frame_position(timeline, frame_index).is_some_and(|position| {
        timeline.base.position <= position
            && position <= timeline.base.position + timeline.base.duration
    })
}

fn cross_frame(
    timeline: &TimelineInstance,
    actions: &[crate::model::ActionData],
    state: &AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
    runtime: &mut RuntimeCtx<'_>,
    frame_index: i32,
) {
    if !state.action_enabled {
        return;
    }
    let Some(timeline_data) = timeline.timeline_data.as_ref() else {
        return;
    };
    let Some(root) = timeline.source.root_info() else {
        return;
    };
    let Ok(timeline_offset) = usize::try_from(timeline_data.offset) else {
        return;
    };
    let Ok(frame_index) = usize::try_from(frame_index) else {
        return;
    };
    let Some(relative_frame_offset) = root.ir.timeline_array.get(
        timeline_offset + crate::core::enums::binary_offset::timeline::FRAME_OFFSET + frame_index,
    ) else {
        return;
    };
    let frame_offset = timeline.animation_frame_offset + *relative_frame_offset;
    let Ok(frame_offset) = usize::try_from(frame_offset) else {
        return;
    };
    let Some(action_count) = root.ir.frame_array.get(frame_offset + 1).copied() else {
        return;
    };
    let Some(frame_position) = root.ir.frame_array.get(frame_offset).copied() else {
        return;
    };
    let event_time = f64::from(frame_position) / timeline.base.frame_rate;

    for action_offset in 0..action_count.max(0) as usize {
        let Some(action_index) = root.ir.frame_array.get(frame_offset + 2 + action_offset) else {
            continue;
        };
        let Ok(action_index) = usize::try_from(*action_index) else {
            continue;
        };
        let Some(action) = actions.get(action_index) else {
            continue;
        };

        if action.action_type == ActionType::Play {
            let mut event = EventObject {
                time: event_time,
                ..EventObject::default()
            };
            EventObject::action_data_to_instance(
                action,
                &mut event,
                context.armature_idx,
                context.bones,
                context.slots,
            );
            context.actions.push(event);
        } else {
            let event_type = if action.action_type == ActionType::Frame {
                EventObject::FRAME_EVENT
            } else {
                EventObject::SOUND_EVENT
            };
            if action.action_type == ActionType::Sound
                || context.proxy.has_db_event_listener(event_type)
            {
                let mut event = EventObject {
                    time: event_time,
                    ..EventObject::default()
                };
                EventObject::action_data_to_instance(
                    action,
                    &mut event,
                    context.armature_idx,
                    context.bones,
                    context.slots,
                );
                runtime.buffer_event(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::update_action_timeline;
    use crate::animation::AnimationUpdateCtx;
    use crate::animation::animation_state::AnimationState;
    use crate::animation::timeline_states::{
        AnimationTimelineSource, TimelineInstance, TimelineTarget,
    };
    use crate::animation::world_clock::ClockArena;
    use crate::armature::{ArmatureIdx, NullArmatureProxy, RuntimeCtx};
    use crate::core::enums::{ActionType, TimelineType};
    use crate::event::EventObject;
    use crate::model::{
        ActionData, AnimationConfig, AnimationData, ArmatureData, DragonBonesData, IrArrays,
        TimelineData,
    };
    use std::sync::Arc;

    fn update_without_listeners(action_type: ActionType) -> (Vec<EventObject>, Vec<EventObject>) {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            frame_rate: 24.0,
            ..ArmatureData::default()
        };
        armature_data.add_action(
            ActionData {
                action_type,
                name: "event".to_owned(),
                ..ActionData::default()
            },
            false,
        );
        let action_timeline = TimelineData {
            timeline_type: TimelineType::Action,
            offset: 0,
            frame_indices_offset: 0,
            x: 0.0,
            y: 0.0,
        };
        let mut animation_data = AnimationData {
            name: "test".to_owned(),
            duration: 1.0,
            frame_count: 24,
            action_timeline: Some(Box::new(action_timeline.clone())),
            ..AnimationData::default()
        };
        animation_data.parent = "test".to_owned();
        armature_data.add_animation(animation_data.clone());
        data.add_armature(armature_data);
        data.finalize_root(IrArrays {
            frame_array: vec![0, 1, 0],
            timeline_array: vec![100, 0, 1, 0, 0, 0],
            frame_indices: vec![0],
            ..IrArrays::default()
        });
        let data = Arc::new(data);
        let host = data.armatures.get("test").expect("host armature data");
        let animation_data = host.animations.get("test").expect("animation data");
        let source = AnimationTimelineSource {
            data: Arc::clone(&data),
            owner_armature: "test".to_owned(),
            animation_name: "test".to_owned(),
        };
        let config = AnimationConfig {
            pause_fade_in: false,
            fade_in_time: 0.0,
            play_times: 0,
            time_scale: 1.0,
            ..AnimationConfig::default()
        };
        let mut state = AnimationState::default();
        state.init(ArmatureIdx(0), animation_data, &config);
        let mut timeline = TimelineInstance::new(
            TimelineType::Action,
            TimelineTarget::Armature(ArmatureIdx(0)),
            None,
            Some(action_timeline),
            source,
            &state,
            host,
            None,
        )
        .expect("action timeline");
        state.time = 0.1;

        let mut bones = Vec::new();
        let mut slots = Vec::new();
        let mut constraints = Vec::new();
        let mut actions = Vec::new();
        let mut cache_frame_index = -1;
        let mut alpha_dirty = false;
        let mut slots_dirty = false;
        let mut z_order_dirty = false;
        let mut z_index_dirty = false;
        let mut proxy = NullArmatureProxy::new();
        let mut update_context = AnimationUpdateCtx {
            armature_idx: ArmatureIdx(0),
            armature_data: host,
            bones: &mut bones,
            slots: &mut slots,
            constraints: &mut constraints,
            actions: &mut actions,
            cache_frame_index: &mut cache_frame_index,
            frame_cache: None,
            alpha_dirty: &mut alpha_dirty,
            slots_dirty: &mut slots_dirty,
            z_order_dirty: &mut z_order_dirty,
            z_index_dirty: &mut z_index_dirty,
            proxy: &mut proxy,
        };
        let mut armatures = Vec::new();
        let mut clocks = ClockArena::default();
        let mut pending_events = std::collections::VecDeque::new();
        let mut runtime = RuntimeCtx::new(
            &mut armatures,
            &mut clocks,
            &mut pending_events,
            Some(ArmatureIdx(0)),
            None,
        );

        assert_eq!(timeline.base.frame_count, 1);
        assert_eq!(
            timeline
                .source
                .armature_data()
                .expect("source armature")
                .actions
                .len(),
            1
        );

        update_action_timeline(&mut timeline, &mut state, &mut update_context, &mut runtime);
        (pending_events.into_iter().collect(), actions)
    }

    #[test]
    fn sound_bypasses_listener_gate_while_frame_and_play_do_not_enter_event_queue() {
        let (frame_events, frame_actions) = update_without_listeners(ActionType::Frame);
        assert!(frame_events.is_empty());
        assert!(frame_actions.is_empty());

        let (sound_events, sound_actions) = update_without_listeners(ActionType::Sound);
        assert_eq!(sound_events.len(), 1);
        assert_eq!(sound_events[0].event_type, EventObject::SOUND_EVENT);
        assert_eq!(sound_events[0].name, "event");
        assert!(sound_actions.is_empty());

        let (play_events, play_actions) = update_without_listeners(ActionType::Play);
        assert!(play_events.is_empty());
        assert_eq!(play_actions.len(), 1);
        assert_eq!(play_actions[0].name, "event");
    }
}
