//! AnimationProgress/Weight/Parameters timeline value axis.

use crate::animation::timeline_states::TimelineInstance;

pub(crate) fn update(timeline: &mut TimelineInstance) {
    // The owner applies `results` to the child StateId immediately after this
    // parent state finishes its group loop, while the StateArena is borrowed.
    timeline.base.dirty = false;
}

#[cfg(test)]
mod tests {
    use super::update;
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{action_time, numeric_timeline};
    use crate::armature::ArmatureIdx;
    use crate::core::enums::TimelineType;

    fn value_at(timeline_type: TimelineType, value_count: usize, values: &[f64]) -> Vec<f64> {
        // TimelineState.ts:1014-1015 / :1041-1042 / :1069-1070 all use
        // frameIntArray with valueScale 0.0001.
        let mut timeline = numeric_timeline(
            timeline_type,
            TimelineTarget::Armature(ArmatureIdx(0)),
            value_count,
            1.0,
            24.0,
            1.0,
            &[0],
            &[0],
            values,
            true,
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        update(&mut timeline);
        assert!(!timeline.base.dirty);
        timeline.results
    }

    #[test]
    fn animation_progress_and_weight_use_single_value_scale_0_0001() {
        assert_eq!(
            value_at(TimelineType::AnimationProgress, 1, &[2500.0]),
            vec![0.25]
        );
        assert_eq!(
            value_at(TimelineType::AnimationWeight, 1, &[7500.0]),
            vec![0.75]
        );
    }

    #[test]
    fn animation_parameter_uses_two_value_scale_0_0001() {
        assert_eq!(
            value_at(TimelineType::AnimationParameter, 2, &[2500.0, 5000.0]),
            vec![0.25, 0.5]
        );
    }
}
