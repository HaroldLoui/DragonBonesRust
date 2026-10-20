//! Surface deform blend axis.

use crate::animation::AnimationUpdateCtx;
use crate::animation::timeline_states::{TimelineInstance, TimelineTarget};
use crate::armature::bone::BoneRuntime;

pub(crate) fn blend(
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
    let BoneRuntime::Surface(surface) = &mut bone.runtime else {
        return;
    };

    // Official iterates the full `_deformCount` without clamping
    // (`TimelineState.ts:555`). Grow first to mirror a JS array write past the
    // end; out-of-range source reads then become NaN like `undefined * number`.
    if surface.deform_vertices.len() < timeline.deform_count {
        surface.deform_vertices.resize(timeline.deform_count, 0.0);
    }
    for index in 0..timeline.deform_count {
        let value = if timeline.timeline_data.is_none() {
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
            let source_index = timeline.same_value_offset + index - timeline.value_count;
            timeline
                .source
                .root_info()
                .and_then(|root| root.ir.frame_float_array.get(source_index))
                .copied()
                .unwrap_or(f64::NAN)
        };
        if blend_dirty > 1 {
            surface.deform_vertices[index] += value * blend_weight;
        } else {
            surface.deform_vertices[index] = value * blend_weight;
        }
    }

    if is_blend_dirty || timeline.base.dirty {
        bone.transform_dirty = true;
        timeline.base.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::blend;
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::TimelineTarget;
    use crate::animation::timeline_states::test_support::{
        CtxFixture, action_time, build_data, make_state, make_timeline, timeline_data, timeline_ir,
    };
    use crate::armature::bone::BoneRuntime;
    use crate::armature::surface::SurfaceState;
    use crate::armature::{Bone, BoneIdx};
    use crate::core::enums::TimelineType;
    use crate::model::{AnimationData, ArmatureData};

    fn approx(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-12, "{left} != {right}");
    }

    #[test]
    fn surface_deform_header_wrap_and_dynamic_count_match_ts_layout() {
        // Deform header is located through TimelineFrameValueCount (index 3)
        // and has five entries (TimelineState.ts:521-540).
        // [VERTEX_OFFSET, COUNT, VALUE_COUNT, VALUE_OFFSET, FLOAT_OFFSET]
        let mut float_values = vec![0.0; 14];
        float_values[0] = 10.0;
        float_values[1] = 20.0;
        float_values[10..14].copy_from_slice(&[7.0, 8.0, 9.0, 10.0]);
        // Header lives at frameIntArray[2], proving the index-3
        // `TimelineFrameValueCount` lookup is used.
        let ir = timeline_ir(
            24.0,
            1.0,
            &[0],
            &[0],
            2,
            &float_values,
            &[99, 99, 0, 4, 2, 1, 10],
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
            TimelineType::Surface,
            TimelineTarget::Bone(BoneIdx(0)),
            Some(timeline_data(TimelineType::Surface)),
        );
        assert_eq!(timeline.deform_count, 4);
        assert_eq!(timeline.deform_offset, 1);
        assert_eq!(timeline.value_count, 2);

        assert_eq!(
            timeline.update_values(0.0, 1, 3, Some(action_time(0.0, TimelinePlayState::Play))),
            Some((true, true))
        );
        assert_eq!(timeline.results, vec![10.0, 20.0]);

        let mut fixture = CtxFixture::new(ArmatureData::default());
        fixture.bones.push(Bone {
            idx: BoneIdx(0),
            runtime: BoneRuntime::Surface(SurfaceState {
                deform_vertices: vec![0.0; 2],
                ..SurfaceState::default()
            }),
            ..Bone::default()
        });
        fixture.with_context(|context| blend(&mut timeline, context, 1.0, 1, true));

        let BoneRuntime::Surface(surface) = &fixture.bones[0].runtime else {
            panic!("surface bone");
        };
        // i=0 reads sameValueOffset+0; i=1,2 read results; i=3 reads
        // sameValueOffset+1 (TimelineState.ts:555-574).
        assert_eq!(surface.deform_vertices.len(), 4);
        approx(surface.deform_vertices[0], 7.0);
        approx(surface.deform_vertices[1], 10.0);
        approx(surface.deform_vertices[2], 20.0);
        approx(surface.deform_vertices[3], 8.0);
        assert!(fixture.bones[0].transform_dirty);
    }
}
