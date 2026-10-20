//! IK constraint timeline axis.

use crate::animation::AnimationUpdateCtx;
use crate::animation::timeline_states::{TimelineInstance, TimelineTarget};
use crate::model::ConstraintKind;

pub(crate) fn update(timeline: &mut TimelineInstance, context: &mut AnimationUpdateCtx<'_>) {
    let TimelineTarget::Constraint(target) = timeline.target else {
        return;
    };
    // `TimelineState.update` calls `_onUpdateFrame` only when
    // `_isTween || dirty` (`BaseTimelineState.ts:220-222`). Tween frames set
    // dirty from the shared updater; a non-tween/pose arrival sets it once in
    // `_onArriveAtFrame` (`:320-321`, `:341-342`) and this consumes it.
    if !timeline.base.dirty {
        return;
    }
    let Some(constraint) = context
        .constraints
        .iter_mut()
        .find(|constraint| constraint.idx == target)
    else {
        return;
    };

    if timeline.is_pose {
        if let Some(data) = context.armature_data.constraints.get(&constraint.name)
            && let ConstraintKind::IK {
                bend_positive,
                weight,
                ..
            } = &data.kind
        {
            constraint.bend_positive = *bend_positive;
            constraint.weight = *weight;
        }
    } else {
        // Official reads `_currentA/_currentB`, which are only refreshed on
        // tween arrivals (`TimelineState.ts:971-972` + `BaseTimelineState.ts:447-468`).
        // On non-tween frames this deliberately keeps the stale tween value.
        constraint.bend_positive = timeline.current_values.first().copied().unwrap_or(0.0) > 0.0;
        constraint.weight = timeline.current_values.get(1).copied().unwrap_or(0.0);
    }
    constraint.invalid_update(context.bones);
    timeline.base.dirty = false;
}

#[cfg(test)]
mod tests {
    use super::update;
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        CtxFixture, action_time, numeric_timeline, pose_timeline,
    };
    use crate::armature::{Bone, BoneIdx, Constraint, ConstraintIdx};
    use crate::core::enums::TimelineType;
    use crate::model::{ArmatureData, ConstraintData, ConstraintKind};

    fn fixture(constraint: Constraint) -> CtxFixture {
        let mut fixture = CtxFixture::new(ArmatureData::default());
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            ..Bone::default()
        });
        fixture.constraints.push(constraint);
        fixture
    }

    fn constraint() -> Constraint {
        Constraint {
            idx: ConstraintIdx(0),
            name: "ik".to_owned(),
            root: BoneIdx(0),
            ..Constraint::default()
        }
    }

    #[test]
    fn ik_reads_current_not_result_and_keeps_stale_values_on_step_frames() {
        // TimelineState.ts:971-972 reads `_currentA/_currentB`; those are only
        // assigned on tween arrivals (BaseTimelineState.ts:447-468).
        let mut timeline = numeric_timeline(
            TimelineType::IKConstraint,
            TimelineTarget::Constraint(ConstraintIdx(0)),
            2,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[1.0, 50.0, 0.0, 80.0],
            true,
        );
        let mut fixture = fixture(constraint());

        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.5, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![0.005, 0.65]);
        fixture.with_context(|context| update(&mut timeline, context));
        assert!(fixture.constraints[0].bend_positive);
        assert_eq!(fixture.constraints[0].weight, 0.5);

        // Non-tween last frame: `results` becomes [0, 0.8], but the official
        // `_current*` buffer is untouched and still holds [0.01, 0.5].
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(1.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![0.0, 0.8]);
        fixture.with_context(|context| update(&mut timeline, context));
        assert!(fixture.constraints[0].bend_positive);
        assert_eq!(fixture.constraints[0].weight, 0.5);
    }

    #[test]
    fn ik_pose_branch_uses_constraint_data_and_runs_only_while_dirty() {
        // TimelineState.ts:974-977.
        let mut timeline = pose_timeline(
            TimelineType::IKConstraint,
            TimelineTarget::Constraint(ConstraintIdx(0)),
        );
        let mut fixture = fixture(constraint());
        fixture.armature_data.constraints.insert(
            "ik".to_owned(),
            ConstraintData {
                name: "ik".to_owned(),
                kind: ConstraintKind::IK {
                    scale_enabled: false,
                    bend_positive: false,
                    weight: 0.7,
                },
                ..ConstraintData::default()
            },
        );

        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        fixture.with_context(|context| update(&mut timeline, context));
        assert!(!fixture.constraints[0].bend_positive);
        assert_eq!(fixture.constraints[0].weight, 0.7);
        assert!(!timeline.base.dirty);

        // A second tick with unchanged time does not re-run the pose update.
        fixture.constraints[0].weight = 0.1;
        fixture.with_context(|context| update(&mut timeline, context));
        assert_eq!(fixture.constraints[0].weight, 0.1);
    }
}
