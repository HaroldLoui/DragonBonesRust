//! ZOrder timeline axis.

use super::super::animation_state::AnimationState;
use super::super::timeline_state::TimelinePlayState;
use super::TimelineInstance;
use crate::animation::AnimationUpdateCtx;

pub(crate) fn update_z_order_timeline(
    timeline: &mut TimelineInstance,
    state: &AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
) {
    if timeline.source.animation_data().is_none() {
        return;
    }
    let action_time = state.action_timeline.as_ref().map(|action| {
        super::super::timeline_state::ActionTimelineTime {
            play_state: action.base.play_state,
            current_play_times: action.base.current_play_times,
            current_time: action.base.current_time,
        }
    });
    let Some(changed) = timeline.update_cursor(state, action_time) else {
        return;
    };
    if !changed || timeline.base.play_state < TimelinePlayState::Play {
        return;
    }

    let frame_indices_offset = if timeline.timeline_data.is_some() {
        let Ok(frame_offset) = usize::try_from(timeline.base.frame_offset) else {
            return;
        };
        let Some(count) = timeline
            .source
            .root_info()
            .and_then(|root| root.ir.frame_array.get(frame_offset + 1))
        else {
            return;
        };
        if *count > 0 {
            let Some(indices_offset) = frame_offset.checked_add(2) else {
                return;
            };
            Some(indices_offset)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(offset) = frame_indices_offset {
        if let Some(root) = timeline.source.root_info() {
            context.sort_z_order(Some(&root.ir.frame_array), offset);
        }
    } else {
        context.sort_z_order(None, 0);
    }
}
