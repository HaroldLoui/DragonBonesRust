//! BoneAll/Translate/Rotate/Scale plus bone-target blend axes.

use crate::animation::AnimationUpdateCtx;
use crate::animation::blend_state::BlendStateKind;
use crate::animation::timeline_states::{TimelineInstance, TimelineTarget};
use crate::core::enums::TimelineType;

pub(crate) fn blend_transform(
    timeline: &mut TimelineInstance,
    context: &mut AnimationUpdateCtx<'_>,
    blend_weight: f64,
    blend_dirty: u32,
    is_blend_dirty: bool,
) {
    let TimelineTarget::Bone(target) = timeline.target else {
        return;
    };
    let Some(bone) = context.bones.iter_mut().find(|bone| bone.idx == target) else {
        return;
    };
    let pose = &mut bone.animation_pose;
    match timeline.timeline_type {
        TimelineType::BoneAll => {
            let values = if timeline.is_pose {
                [0.0, 0.0, 0.0, 0.0, 1.0, 1.0]
            } else {
                let mut values = [0.0; 6];
                values.copy_from_slice(&timeline.results[..6]);
                values
            };
            let scale = context.armature_data.scale;
            if blend_dirty > 1 {
                pose.x += values[0] * blend_weight * scale;
                pose.y += values[1] * blend_weight * scale;
                pose.rotation += values[2] * blend_weight;
                pose.skew += values[3] * blend_weight;
                pose.scale_x += (values[4] - 1.0) * blend_weight;
                pose.scale_y += (values[5] - 1.0) * blend_weight;
            } else {
                pose.x = values[0] * blend_weight * scale;
                pose.y = values[1] * blend_weight * scale;
                pose.rotation = values[2] * blend_weight;
                pose.skew = values[3] * blend_weight;
                pose.scale_x = (values[4] - 1.0) * blend_weight + 1.0;
                pose.scale_y = (values[5] - 1.0) * blend_weight + 1.0;
            }
        }
        TimelineType::BoneTranslate => {
            let x = pose_or_result(timeline, 0.0, 0);
            let y = pose_or_result(timeline, 0.0, 1);
            if blend_dirty > 1 {
                pose.x += x * blend_weight;
                pose.y += y * blend_weight;
            } else {
                pose.x = x * blend_weight;
                pose.y = y * blend_weight;
            }
        }
        TimelineType::BoneRotate => {
            let rotation = pose_or_result(timeline, 0.0, 0);
            let skew = pose_or_result(timeline, 0.0, 1);
            if blend_dirty > 1 {
                pose.rotation += rotation * blend_weight;
                pose.skew += skew * blend_weight;
            } else {
                pose.rotation = rotation * blend_weight;
                pose.skew = skew * blend_weight;
            }
        }
        TimelineType::BoneScale => {
            let scale_x = pose_or_result(timeline, 1.0, 0);
            let scale_y = pose_or_result(timeline, 1.0, 1);
            if blend_dirty > 1 {
                pose.scale_x += (scale_x - 1.0) * blend_weight;
                pose.scale_y += (scale_y - 1.0) * blend_weight;
            } else if blend_weight != 1.0 {
                pose.scale_x = (scale_x - 1.0) * blend_weight + 1.0;
                pose.scale_y = (scale_y - 1.0) * blend_weight + 1.0;
            } else {
                pose.scale_x = scale_x;
                pose.scale_y = scale_y;
            }
        }
        _ => return,
    }
    if is_blend_dirty || timeline.base.dirty {
        bone.transform_dirty = true;
    }
    timeline.base.dirty = false;
}

pub(crate) fn blend_auxiliary(
    timeline: &mut TimelineInstance,
    context: &mut AnimationUpdateCtx<'_>,
    blend_weight: f64,
    blend_dirty: u32,
    is_blend_dirty: bool,
) {
    match timeline.blend_key.as_ref().map(|key| key.kind) {
        Some(BlendStateKind::BoneAlpha) => {
            let TimelineTarget::Bone(target) = timeline.target else {
                return;
            };
            let Some(bone) = context.bones.iter_mut().find(|bone| bone.idx == target) else {
                return;
            };
            let value = pose_or_result(timeline, 1.0, 0);
            if blend_dirty > 1 {
                bone.base.alpha += value * blend_weight;
                if bone.base.alpha > 1.0 {
                    bone.base.alpha = 1.0;
                }
            } else {
                bone.base.alpha = value * blend_weight;
            }
            if is_blend_dirty || timeline.base.dirty {
                *context.alpha_dirty = true;
            }
            timeline.base.dirty = false;
        }
        Some(BlendStateKind::Surface) => {
            super::surface::blend(timeline, context, blend_weight, blend_dirty, is_blend_dirty);
        }
        _ => {}
    }
}

fn pose_or_result(timeline: &TimelineInstance, pose_value: f64, index: usize) -> f64 {
    if timeline.is_pose {
        pose_value
    } else {
        timeline.results.get(index).copied().unwrap_or(pose_value)
    }
}

#[cfg(test)]
mod tests {
    use super::{blend_auxiliary, blend_transform};
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        CtxFixture, action_time, numeric_timeline, pose_timeline,
    };
    use crate::armature::{Bone, BoneIdx};
    use crate::core::enums::TimelineType;
    use crate::model::ArmatureData;

    fn approx(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-12, "{left} != {right}");
    }

    #[test]
    fn bone_all_tween_midpoint_applies_armature_scale_at_blend() {
        // Frame 0 -> 1 Line tween; duration 2s puts the next key at t=1s.
        let mut timeline = numeric_timeline(
            TimelineType::BoneAll,
            TimelineTarget::Bone(BoneIdx(0)),
            6,
            2.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[
                10.0, 20.0, 30.0, 40.0, 2.0, 3.0, 20.0, 40.0, 50.0, 60.0, 4.0, 5.0,
            ],
            false,
        );
        let action = action_time(0.5, TimelinePlayState::Play);
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action)),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![15.0, 30.0, 40.0, 50.0, 3.0, 4.0]);

        let mut fixture = CtxFixture::new(ArmatureData {
            scale: 2.0,
            ..ArmatureData::default()
        });
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            ..Bone::default()
        });
        fixture.with_context(|context| {
            blend_transform(&mut timeline, context, 1.0, 1, true);
        });

        let pose = fixture.bones[0].animation_pose;
        // TimelineState.ts:334-347: translation only is scaled by armature.scale.
        approx(pose.x, 30.0);
        approx(pose.y, 60.0);
        approx(pose.rotation, 40.0);
        approx(pose.skew, 50.0);
        approx(pose.scale_x, 3.0);
        approx(pose.scale_y, 4.0);
        assert!(!timeline.base.dirty);
    }

    #[test]
    fn bone_translate_reads_with_value_scale_and_pose_scale_falls_back_to_one() {
        // TimelineState.ts:367-368: translate itself carries armature.scale.
        let mut timeline = numeric_timeline(
            TimelineType::BoneTranslate,
            TimelineTarget::Bone(BoneIdx(0)),
            2,
            2.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[0.0, 0.0, 5.0, 6.0],
            false,
        );
        let action = action_time(0.5, TimelinePlayState::Play);
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action)),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![5.0, 6.0]);

        let mut fixture = CtxFixture::new(ArmatureData::default());
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            ..Bone::default()
        });
        fixture.with_context(|context| {
            blend_transform(&mut timeline, context, 1.0, 1, true);
        });
        approx(fixture.bones[0].animation_pose.x, 5.0);
        approx(fixture.bones[0].animation_pose.y, 6.0);

        // TimelineState.ts:463-466: pose scale is 1.0, not 0.0.
        let mut pose = pose_timeline(TimelineType::BoneScale, TimelineTarget::Bone(BoneIdx(0)));
        fixture.with_context(|context| {
            blend_transform(&mut pose, context, 1.0, 1, true);
        });
        approx(fixture.bones[0].animation_pose.scale_x, 1.0);
        approx(fixture.bones[0].animation_pose.scale_y, 1.0);
    }

    #[test]
    fn bone_alpha_shares_the_slot_alpha_axis_semantics() {
        // AlphaTimelineState is reused for BoneAlpha/SlotAlpha
        // (TimelineState.ts:591-631); valueScale is 0.01.
        let mut timeline = numeric_timeline(
            TimelineType::BoneAlpha,
            TimelineTarget::Bone(BoneIdx(0)),
            1,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 0],
            &[50.0, 150.0],
            true,
        );
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(1.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        approx(timeline.results[0], 1.5);
        timeline.blend_key = Some(crate::animation::blend_state::BlendStateKey {
            kind: crate::animation::blend_state::BlendStateKind::BoneAlpha,
            target_name: "bone".to_owned(),
        });

        let mut fixture = CtxFixture::new(ArmatureData::default());
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            ..Bone::default()
        });
        fixture.with_context(|context| {
            blend_auxiliary(&mut timeline, context, 1.0, 1, true);
        });
        approx(fixture.bones[0].base.alpha, 1.5);
        assert!(fixture.alpha_dirty);

        fixture.bones[0].base.alpha = 0.9;
        fixture.with_context(|context| {
            blend_auxiliary(&mut timeline, context, 1.0, 2, true);
        });
        approx(fixture.bones[0].base.alpha, 1.0);
    }

    #[test]
    fn bone_rotate_last_tween_frame_normalizes_the_difference() {
        let mut timeline = numeric_timeline(
            TimelineType::BoneRotate,
            TimelineTarget::Bone(BoneIdx(0)),
            2,
            1.0,
            24.0,
            2.0,
            &[0, 24],
            &[1, 1],
            &[0.0, 0.0, 5.5, 2.0],
            false,
        );
        // Looping (playTimes == 0) keeps the final frame tweenable
        // (BaseTimelineState.ts:300-307).
        let action = action_time(1.0, TimelinePlayState::Play);
        assert_eq!(
            timeline.update_values(0.0, 0, 3, Some(action)),
            Some((true, true))
        );
        assert_eq!(timeline.base.frame_index, 1);
        approx(
            timeline.differences[0],
            crate::geom::transform::normalize_radian(-5.5),
        );
        approx(
            timeline.differences[1],
            crate::geom::transform::normalize_radian(-2.0),
        );
    }
}
