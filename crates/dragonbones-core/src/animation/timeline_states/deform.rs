//! Slot deform timeline blend axis.

use crate::animation::timeline_states::TimelineInstance;
use crate::armature::Slot;

/// Blends the selected display frame's deform buffer and reports whether the
/// frame is the slot's currently displayed geometry.
pub(crate) fn blend(
    timeline: &mut TimelineInstance,
    slot: &mut Slot,
    blend_weight: f64,
    blend_dirty: u32,
    is_blend_dirty: bool,
) -> bool {
    let Some(display_index) = timeline.display_frame else {
        return false;
    };
    let Some(frame) = slot.display_frames.get_mut(display_index) else {
        return false;
    };
    // Official iterates the full `_deformCount` without clamping
    // (`TimelineState.ts:921`). A JS array write past the end extends the
    // array; reproduce that by growing first, then let out-of-range source
    // reads become NaN (JS `undefined * number`).
    if frame.deform_vertices.len() < timeline.deform_count {
        frame.deform_vertices.resize(timeline.deform_count, 0.0);
    }
    for index in 0..timeline.deform_count {
        let value = if timeline.is_pose {
            0.0
        } else if index < timeline.deform_offset {
            timeline
                .source
                .root_info()
                .and_then(|root| {
                    root.ir
                        .frame_float_array
                        .get(timeline.same_value_offset + index)
                })
                .copied()
                .unwrap_or(f64::NAN)
        } else if index < timeline.deform_offset + timeline.value_count {
            timeline
                .results
                .get(index - timeline.deform_offset)
                .copied()
                .unwrap_or(f64::NAN)
        } else {
            timeline
                .source
                .root_info()
                .and_then(|root| {
                    root.ir
                        .frame_float_array
                        .get(timeline.same_value_offset + index - timeline.value_count)
                })
                .copied()
                .unwrap_or(f64::NAN)
        };
        if blend_dirty > 1 {
            frame.deform_vertices[index] += value * blend_weight;
        } else {
            frame.deform_vertices[index] = value * blend_weight;
        }
    }
    let geometry_matches = slot
        .geometry_data
        .as_ref()
        .zip(frame.get_geometry_data())
        .is_some_and(|(slot_geometry, frame_geometry)| {
            slot_geometry.offset == frame_geometry.offset
        });
    let dirty = geometry_matches && (is_blend_dirty || timeline.base.dirty);
    timeline.base.dirty = false;
    dirty
}

#[cfg(test)]
mod tests {
    use super::blend;
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        action_time, build_data, make_state, make_timeline, pose_timeline, timeline_data,
        timeline_ir,
    };
    use crate::armature::{DisplayFrame, Slot, SlotIdx};
    use crate::core::enums::TimelineType;
    use crate::model::{AnimationData, ArmatureData, DisplayData, DisplayKind, GeometryData};

    fn slot_with_geometry(offset: i32) -> Slot {
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
        let frame = DisplayFrame {
            raw_display_data: Some(display),
            deform_vertices: vec![1.0; 4],
            ..DisplayFrame::default()
        };
        Slot {
            idx: SlotIdx(0),
            slot_name: "slot".to_owned(),
            display_frames: vec![frame],
            geometry_data: Some(GeometryData {
                offset,
                ..GeometryData::default()
            }),
            ..Slot::default()
        }
    }

    #[test]
    fn pose_deform_only_marks_vertices_dirty_for_the_displayed_geometry() {
        // TimelineState.ts:948-953 gates `_verticesDirty` on
        // `slot._geometryData === displayFrame.getGeometryData()`.
        let mut timeline =
            pose_timeline(TimelineType::SlotDeform, TimelineTarget::Slot(SlotIdx(0)));
        timeline.display_frame = Some(0);
        timeline.deform_count = 4;

        let mut matching = slot_with_geometry(5);
        assert!(blend(&mut timeline, &mut matching, 1.0, 1, true));
        assert_eq!(matching.display_frames[0].deform_vertices, vec![0.0; 4]);

        let mut different = slot_with_geometry(6);
        timeline.base.dirty = false;
        assert!(!blend(&mut timeline, &mut different, 1.0, 1, true));
        assert_eq!(different.display_frames[0].deform_vertices, vec![0.0; 4]);
    }

    #[test]
    fn deform_same_value_offset_wraps_int16_and_reads_shared_vertices() {
        // TimelineState.ts:891-897 wraps a negative DeformFloatOffset by +65536.
        // deformOffset=1/valueCount=1 makes i=0 read the shared block and i=1
        // read the tween results (TimelineState.ts:921-933).
        let mut float_values = vec![0.0; 32769];
        float_values[0] = 10.0;
        float_values[32768] = 7.0;
        let ir = timeline_ir(
            24.0,
            1.0,
            &[0],
            &[0],
            0,
            &float_values,
            &[0, 2, 1, 1, -32768],
        );
        let data = build_data(
            AnimationData {
                duration: 1.0,
                frame_count: 1,
                ..AnimationData::default()
            },
            ArmatureData {
                frame_rate: 24.0,
                ..ArmatureData::default()
            },
            ir,
        );
        let state = make_state(&data, false);
        let mut timeline = make_timeline(
            &data,
            &state,
            TimelineType::SlotDeform,
            TimelineTarget::Slot(SlotIdx(0)),
            Some(timeline_data(TimelineType::SlotDeform)),
        );
        assert_eq!(timeline.same_value_offset, 32768);
        timeline.display_frame = Some(0);
        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![10.0]);

        let mut slot = Slot {
            display_frames: vec![DisplayFrame {
                deform_vertices: vec![0.0; 2],
                ..DisplayFrame::default()
            }],
            ..Slot::default()
        };
        // No slot geometry means the identity guard suppresses verticesDirty.
        assert!(!blend(&mut timeline, &mut slot, 1.0, 1, true));
        assert_eq!(slot.display_frames[0].deform_vertices, vec![7.0, 10.0]);
    }
}
