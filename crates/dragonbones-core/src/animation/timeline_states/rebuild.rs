//! AnimationState timeline construction and target refresh.

use std::collections::{HashMap, HashSet};

use crate::animation::AnimationUpdateCtx;
use crate::animation::animation_state::{AnimationState, TimelineDirty, TimelineGroup};
use crate::animation::blend_state::{BlendState, BlendStateKey, BlendStateKind};
use crate::animation::timeline_states::{
    AnimationTimelineSource, TimelineInstance, TimelineTarget,
};
use crate::armature::bone::BoneRuntime;
use crate::armature::{Bone, BoneIdx, Slot, SlotIdx};
use crate::core::enums::{TimelineType, binary_offset};
use crate::model::{AnimationData, ArmatureData, TimelineData};

pub(crate) fn rebuild(
    state: &mut AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
    blend_states: &mut HashMap<BlendStateKey, BlendState>,
) {
    let Some(source) = state
        .action_timeline
        .as_ref()
        .map(|timeline| timeline.source.clone())
    else {
        return;
    };
    let Some(animation_data) = source.animation_data() else {
        return;
    };
    let host_data = context.armature_data;

    // `AnimationState.ts:822-828`: only `dirty === 2` (`RebuildAll`) rebuilds
    // constraint timelines via `_updateTimelines`; `dirty === 1`
    // (`RefreshTargets`) just re-syncs bone/slot timeline ownership through
    // `_updateBoneAndSlotTimelines` and must not reset the constraint axes.
    if state.timeline_dirty == TimelineDirty::RefreshTargets {
        refresh_targets(
            state,
            context,
            blend_states,
            &source,
            animation_data,
            host_data,
        );
        state.timeline_dirty = TimelineDirty::Clean;
        return;
    }

    state.bone_timelines.clear();
    state.bone_blend_timelines.clear();
    state.slot_timelines.clear();
    state.slot_blend_timelines.clear();
    state.constraint_timelines.clear();
    state.pose_timeline_ids.clear();

    for constraint in context.constraints.iter() {
        match animation_data.get_constraint_timelines(&constraint.name) {
            Some(timelines) => {
                for timeline in timelines {
                    if timeline.timeline_type == TimelineType::IKConstraint {
                        push_axis(
                            state,
                            &source,
                            host_data,
                            TimelineGroup::Constraint,
                            TimelineType::IKConstraint,
                            TimelineTarget::Constraint(constraint.idx),
                            None,
                            Some(timeline),
                            None,
                            None,
                            false,
                        );
                    }
                }
            }
            None if state.reset_to_pose => {
                push_axis(
                    state,
                    &source,
                    host_data,
                    TimelineGroup::Constraint,
                    TimelineType::IKConstraint,
                    TimelineTarget::Constraint(constraint.idx),
                    None,
                    None,
                    None,
                    None,
                    true,
                );
            }
            None => {}
        }
    }

    for bone in context.bones.iter() {
        if !state.contains_bone_mask(&bone.bone_name) {
            continue;
        }
        create_bone_axes(
            state,
            &source,
            host_data,
            blend_states,
            animation_data,
            bone,
        );
    }

    for slot in context.slots.iter_mut() {
        let Some(parent_bone) = context.bones.get(slot.parent.index()) else {
            continue;
        };
        if !state.contains_bone_mask(&parent_bone.bone_name) {
            continue;
        }
        create_slot_axes(
            state,
            &source,
            host_data,
            blend_states,
            animation_data,
            slot,
        );
    }

    state.timeline_dirty = TimelineDirty::Clean;
}

/// One target's timeline construction, shared by the full rebuild and the
/// `RefreshTargets` path (`AnimationState.ts:369-494`).
#[allow(clippy::too_many_arguments)]
fn create_bone_axes(
    state: &mut AnimationState,
    source: &AnimationTimelineSource,
    host_data: &ArmatureData,
    blend_states: &mut HashMap<BlendStateKey, BlendState>,
    animation_data: &AnimationData,
    bone: &Bone,
) {
    match animation_data.get_bone_timelines(&bone.bone_name) {
        Some(timelines) => {
            for timeline in timelines {
                let (group, kind) = match timeline.timeline_type {
                    TimelineType::BoneAll
                    | TimelineType::BoneTranslate
                    | TimelineType::BoneRotate
                    | TimelineType::BoneScale => {
                        (TimelineGroup::Bone, BlendStateKind::BoneTransform)
                    }
                    TimelineType::BoneAlpha => {
                        (TimelineGroup::BoneBlend, BlendStateKind::BoneAlpha)
                    }
                    TimelineType::Surface => (TimelineGroup::BoneBlend, BlendStateKind::Surface),
                    _ => continue,
                };
                let key = blend_key(kind, &bone.bone_name);
                blend_states.entry(key.clone()).or_default();
                push_axis(
                    state,
                    source,
                    host_data,
                    group,
                    timeline.timeline_type,
                    TimelineTarget::Bone(bone.idx),
                    Some(key),
                    Some(timeline),
                    None,
                    None,
                    false,
                );
            }
        }
        None if state.reset_to_pose => {
            let (group, kind, timeline_type, deform_count) = match &bone.runtime {
                BoneRuntime::Bone => (
                    TimelineGroup::Bone,
                    BlendStateKind::BoneTransform,
                    TimelineType::BoneAll,
                    None,
                ),
                BoneRuntime::Surface(surface) => (
                    TimelineGroup::BoneBlend,
                    BlendStateKind::Surface,
                    TimelineType::Surface,
                    Some(surface.deform_vertices.len()),
                ),
            };
            let key = blend_key(kind, &bone.bone_name);
            blend_states.entry(key.clone()).or_default();
            push_axis(
                state,
                source,
                host_data,
                group,
                timeline_type,
                TimelineTarget::Bone(bone.idx),
                Some(key),
                None,
                None,
                deform_count,
                true,
            );
        }
        None => {}
    }
}

/// One slot's timeline construction (`AnimationState.ts:496-645`).
#[allow(clippy::too_many_arguments)]
fn create_slot_axes(
    state: &mut AnimationState,
    source: &AnimationTimelineSource,
    host_data: &ArmatureData,
    blend_states: &mut HashMap<BlendStateKey, BlendState>,
    animation_data: &AnimationData,
    slot: &mut Slot,
) {
    let mut has_display_timeline = false;
    let mut has_color_timeline = false;
    let mut deform_geometries = Vec::new();
    if let Some(timelines) = animation_data.get_slot_timelines(&slot.slot_name) {
        for timeline in timelines {
            match timeline.timeline_type {
                TimelineType::SlotDisplay => {
                    has_display_timeline = true;
                    push_axis(
                        state,
                        source,
                        host_data,
                        TimelineGroup::Slot,
                        TimelineType::SlotDisplay,
                        TimelineTarget::Slot(slot.idx),
                        None,
                        Some(timeline),
                        None,
                        None,
                        false,
                    );
                }
                TimelineType::SlotColor => {
                    has_color_timeline = true;
                    push_axis(
                        state,
                        source,
                        host_data,
                        TimelineGroup::Slot,
                        TimelineType::SlotColor,
                        TimelineTarget::Slot(slot.idx),
                        None,
                        Some(timeline),
                        None,
                        None,
                        false,
                    );
                }
                TimelineType::SlotAlpha | TimelineType::SlotZIndex => {
                    let kind = if timeline.timeline_type == TimelineType::SlotZIndex {
                        BlendStateKind::SlotZIndex
                    } else {
                        BlendStateKind::SlotAlpha
                    };
                    let key = blend_key(kind, &slot.slot_name);
                    blend_states.entry(key.clone()).or_default();
                    push_axis(
                        state,
                        source,
                        host_data,
                        TimelineGroup::SlotBlend,
                        timeline.timeline_type,
                        TimelineTarget::Slot(slot.idx),
                        Some(key),
                        Some(timeline),
                        None,
                        None,
                        false,
                    );
                }
                TimelineType::SlotDeform => {
                    let Some(geometry_offset) =
                        deform_geometry_offset(source, animation_data.frame_int_offset, timeline)
                    else {
                        continue;
                    };
                    let Some((display_index, frame)) = slot
                        .display_frames
                        .iter_mut()
                        .enumerate()
                        .find(|(_, frame)| {
                            frame
                                .get_geometry_data()
                                .is_some_and(|geometry| geometry.offset == geometry_offset)
                        })
                    else {
                        continue;
                    };
                    frame.update_deform_vertices();
                    deform_geometries.push(geometry_offset);
                    let display_name = frame
                        .raw_display_data
                        .as_ref()
                        .map_or(slot.slot_name.as_str(), |display| display.name.as_str());
                    let key = blend_key(BlendStateKind::SlotDeform, display_name);
                    blend_states.entry(key.clone()).or_default();
                    push_axis(
                        state,
                        source,
                        host_data,
                        TimelineGroup::SlotBlend,
                        TimelineType::SlotDeform,
                        TimelineTarget::Slot(slot.idx),
                        Some(key),
                        Some(timeline),
                        Some(display_index),
                        None,
                        false,
                    );
                }
                _ => {}
            }
        }
    }

    if !state.reset_to_pose {
        return;
    }
    if !has_display_timeline {
        push_axis(
            state,
            source,
            host_data,
            TimelineGroup::Slot,
            TimelineType::SlotDisplay,
            TimelineTarget::Slot(slot.idx),
            None,
            None,
            None,
            None,
            true,
        );
    }
    if !has_color_timeline {
        push_axis(
            state,
            source,
            host_data,
            TimelineGroup::Slot,
            TimelineType::SlotColor,
            TimelineTarget::Slot(slot.idx),
            None,
            None,
            None,
            None,
            true,
        );
    }
    for (display_index, frame) in slot.display_frames.iter_mut().enumerate() {
        if frame.deform_vertices.is_empty() {
            continue;
        }
        let Some(geometry) = frame.get_geometry_data() else {
            continue;
        };
        if deform_geometries.contains(&geometry.offset) {
            continue;
        }
        // Pose branch keys by `slot.name` (AnimationState.ts:639); only
        // the animated branch above uses `rawDisplayData.name` (:585).
        let key = blend_key(BlendStateKind::SlotDeform, &slot.slot_name);
        blend_states.entry(key.clone()).or_default();
        push_axis(
            state,
            source,
            host_data,
            TimelineGroup::SlotBlend,
            TimelineType::SlotDeform,
            TimelineTarget::Slot(slot.idx),
            Some(key),
            None,
            Some(display_index),
            Some(frame.deform_vertices.len()),
            true,
        );
    }
}

/// `_updateBoneAndSlotTimelines` equivalent for `dirty === 1`
/// (`AnimationState.ts:822-828`, body `:369-645`): keep existing axes for
/// still-active targets, drop axes for targets that left the bone mask, and
/// create axes only for targets that have none yet. Constraint timelines are
/// untouched because `_updateTimelines` is not called for this dirty level.
fn refresh_targets(
    state: &mut AnimationState,
    context: &mut AnimationUpdateCtx<'_>,
    blend_states: &mut HashMap<BlendStateKey, BlendState>,
    source: &AnimationTimelineSource,
    animation_data: &AnimationData,
    host_data: &ArmatureData,
) {
    let active_bones: HashSet<BoneIdx> = context
        .bones
        .iter()
        .filter(|bone| state.contains_bone_mask(&bone.bone_name))
        .map(|bone| bone.idx)
        .collect();
    let active_slots: HashSet<SlotIdx> = context
        .slots
        .iter()
        .filter(|slot| {
            context
                .bones
                .get(slot.parent.index())
                .is_some_and(|bone| state.contains_bone_mask(&bone.bone_name))
        })
        .map(|slot| slot.idx)
        .collect();

    state
        .bone_timelines
        .retain(|timeline| match timeline.target {
            TimelineTarget::Bone(idx) => active_bones.contains(&idx),
            _ => true,
        });
    state
        .bone_blend_timelines
        .retain(|timeline| match timeline.target {
            TimelineTarget::Bone(idx) => active_bones.contains(&idx),
            _ => true,
        });
    state
        .slot_timelines
        .retain(|timeline| match timeline.target {
            TimelineTarget::Slot(idx) => active_slots.contains(&idx),
            _ => true,
        });
    state
        .slot_blend_timelines
        .retain(|timeline| match timeline.target {
            TimelineTarget::Slot(idx) => active_slots.contains(&idx),
            _ => true,
        });

    let live_ids: HashSet<u32> = state
        .bone_timelines
        .iter()
        .chain(state.bone_blend_timelines.iter())
        .chain(state.slot_timelines.iter())
        .chain(state.slot_blend_timelines.iter())
        .chain(state.constraint_timelines.iter())
        .chain(state.animation_timelines.iter())
        .map(|timeline| timeline.id)
        .collect();
    state
        .pose_timeline_ids
        .retain(|timeline_id| live_ids.contains(timeline_id));

    for bone in context.bones.iter() {
        if active_bones.contains(&bone.idx) && !bone_has_timeline(state, bone.idx) {
            create_bone_axes(state, source, host_data, blend_states, animation_data, bone);
        }
    }
    for slot in context.slots.iter_mut() {
        if active_slots.contains(&slot.idx) && !slot_has_timeline(state, slot.idx) {
            create_slot_axes(state, source, host_data, blend_states, animation_data, slot);
        }
    }
}

fn bone_has_timeline(state: &AnimationState, idx: BoneIdx) -> bool {
    state
        .bone_timelines
        .iter()
        .chain(state.bone_blend_timelines.iter())
        .any(|timeline| timeline.target == TimelineTarget::Bone(idx))
}

fn slot_has_timeline(state: &AnimationState, idx: SlotIdx) -> bool {
    state
        .slot_timelines
        .iter()
        .chain(state.slot_blend_timelines.iter())
        .any(|timeline| timeline.target == TimelineTarget::Slot(idx))
}

#[allow(clippy::too_many_arguments)]
fn push_axis(
    state: &mut AnimationState,
    source: &AnimationTimelineSource,
    host_data: &ArmatureData,
    group: TimelineGroup,
    timeline_type: TimelineType,
    target: TimelineTarget,
    blend_key: Option<BlendStateKey>,
    timeline_data: Option<&TimelineData>,
    display_frame: Option<usize>,
    deform_count: Option<usize>,
    is_pose: bool,
) {
    let Some(mut timeline) = TimelineInstance::new(
        timeline_type,
        target,
        blend_key,
        timeline_data.cloned(),
        source.clone(),
        state,
        host_data,
        display_frame,
    ) else {
        return;
    };
    if let Some(count) = deform_count {
        timeline.deform_count = count;
    }
    state.push_timeline(group, timeline, is_pose);
}

fn blend_key(kind: BlendStateKind, target_name: &str) -> BlendStateKey {
    BlendStateKey {
        kind,
        target_name: target_name.to_owned(),
    }
}

fn deform_geometry_offset(
    source: &AnimationTimelineSource,
    frame_int_offset: i32,
    timeline: &TimelineData,
) -> Option<i32> {
    let timeline_offset = usize::try_from(timeline.offset).ok()?;
    let root = source.root_info()?;
    let header = usize::try_from(frame_int_offset).ok()?
        + usize::try_from(
            *root
                .ir
                .timeline_array
                .get(timeline_offset + binary_offset::timeline::FRAME_VALUE_COUNT)?,
        )
        .ok()?;
    let mut geometry_offset = *root
        .ir
        .frame_int_array
        .get(header + binary_offset::deform::VERTEX_OFFSET)?;
    if geometry_offset < 0 {
        geometry_offset += 65536;
    }
    Some(geometry_offset)
}

#[cfg(test)]
mod tests {
    use super::rebuild;
    use crate::animation::animation_state::{AnimationState, TimelineDirty};
    use crate::animation::blend_state::{BlendState, BlendStateKey};
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        ARMATURE, CtxFixture, empty_data, make_state, make_timeline,
    };
    use crate::armature::{ArmatureIdx, Bone, BoneIdx, ConstraintIdx, DisplayFrame, Slot, SlotIdx};
    use crate::core::enums::TimelineType;
    use crate::model::{DisplayData, DisplayKind, GeometryData};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn state_with_action(
        data: &Arc<crate::model::DragonBonesData>,
        reset_to_pose: bool,
    ) -> AnimationState {
        let mut state = make_state(data, reset_to_pose);
        let action = make_timeline(
            data,
            &state,
            TimelineType::Action,
            TimelineTarget::Armature(ArmatureIdx(0)),
            None,
        );
        state.action_timeline = Some(action);
        state
    }

    fn host(data: &Arc<crate::model::DragonBonesData>) -> crate::model::ArmatureData {
        data.armatures.get(ARMATURE).expect("armature").clone()
    }

    #[test]
    fn refresh_targets_keeps_constraint_axes_while_rebuild_all_resets_them() {
        // AnimationState.ts:822-828: only dirty==2 runs `_updateTimelines`.
        let data = empty_data();
        let mut state = state_with_action(&data, false);
        let constraint_timeline = make_timeline(
            &data,
            &state,
            TimelineType::IKConstraint,
            TimelineTarget::Constraint(ConstraintIdx(0)),
            None,
        );
        let timeline_id = constraint_timeline.id;
        state.constraint_timelines.push(constraint_timeline);
        state.timeline_dirty = TimelineDirty::RefreshTargets;

        let mut fixture = CtxFixture::new(host(&data));
        let mut blend_states: HashMap<BlendStateKey, BlendState> = HashMap::new();
        fixture.with_context(|context| rebuild(&mut state, context, &mut blend_states));
        assert_eq!(state.constraint_timelines.len(), 1);
        assert_eq!(state.constraint_timelines[0].id, timeline_id);
        assert_eq!(state.timeline_dirty, TimelineDirty::Clean);

        state.timeline_dirty = TimelineDirty::RebuildAll;
        fixture.with_context(|context| rebuild(&mut state, context, &mut blend_states));
        // No constraint data and no resetToPose pose axis.
        assert!(state.constraint_timelines.is_empty());
    }

    #[test]
    fn pose_deform_blend_key_uses_slot_name_not_display_name() {
        // AnimationState.ts:639 (pose) vs :585 (animated).
        let data = empty_data();
        let mut state = state_with_action(&data, true);
        let mut fixture = CtxFixture::new(host(&data));
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            bone_name: "bone".to_owned(),
            ..Bone::default()
        });
        let display = DisplayData {
            name: "display".to_owned(),
            kind: DisplayKind::Mesh {
                geometry: GeometryData {
                    offset: 5,
                    ..GeometryData::default()
                },
                texture: None,
            },
            ..DisplayData::default()
        };
        fixture.slots.push(Slot {
            idx: SlotIdx(0),
            slot_name: "slot".to_owned(),
            parent: BoneIdx(0),
            display_frames: vec![DisplayFrame {
                raw_display_data: Some(display),
                deform_vertices: vec![0.0; 4],
                ..DisplayFrame::default()
            }],
            ..Slot::default()
        });

        let mut blend_states: HashMap<BlendStateKey, BlendState> = HashMap::new();
        fixture.with_context(|context| rebuild(&mut state, context, &mut blend_states));

        let deform = state
            .slot_blend_timelines
            .iter()
            .find(|timeline| timeline.timeline_type == TimelineType::SlotDeform)
            .expect("pose deform axis");
        assert!(deform.is_pose);
        assert_eq!(
            deform
                .blend_key
                .as_ref()
                .map(|key| key.target_name.as_str()),
            Some("slot")
        );
        // The animation name also anchors the pose display/color axes.
        assert_eq!(state.animation_timelines.len(), 0);
    }
}
