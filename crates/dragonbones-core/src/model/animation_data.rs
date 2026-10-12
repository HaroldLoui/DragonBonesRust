//! 对齐 `model/AnimationData.ts`（`TimelineData` + `AnimationTimelineData` + `AnimationData`）。

use indexmap::IndexMap;

use crate::core::enums::{AnimationBlendType, TimelineType};

/// - 时间轴数据（`TimelineData.ts` + `AnimationTimelineData`，按 TS 同容器混存，字段平铺）。
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineData {
    /// TS 字段名 `type`。
    pub timeline_type: TimelineType,
    pub offset: i32,
    pub frame_indices_offset: i32,
    /// 仅 `AnimationTimelineData` 有效（`AnimationTimelineData._onClear` 默认 0）。
    pub x: f64,
    /// 仅 `AnimationTimelineData` 有效。
    pub y: f64,
}

impl Default for TimelineData {
    fn default() -> Self {
        Self {
            timeline_type: TimelineType::BoneAll,
            offset: 0,
            frame_indices_offset: -1,
            x: 0.0,
            y: 0.0,
        }
    }
}

/// - 动画数据（`AnimationData.ts`）。
///
/// `parent` 为骨架名引用（D-1.1）。
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationData {
    pub frame_int_offset: i32,
    pub frame_float_offset: i32,
    pub frame_offset: i32,
    pub blend_type: AnimationBlendType,
    pub frame_count: i32,
    pub play_times: i32,
    pub duration: f64,
    pub scale: f64,
    pub fade_in_time: f64,
    pub cache_frame_rate: f64,
    pub name: String,
    pub cached_frames: Vec<bool>,
    pub bone_timelines: IndexMap<String, Vec<TimelineData>>,
    pub slot_timelines: IndexMap<String, Vec<TimelineData>>,
    pub constraint_timelines: IndexMap<String, Vec<TimelineData>>,
    pub animation_timelines: IndexMap<String, Vec<TimelineData>>,
    pub bone_cached_frame_indices: IndexMap<String, Vec<i32>>,
    pub slot_cached_frame_indices: IndexMap<String, Vec<i32>>,
    pub action_timeline: Option<Box<TimelineData>>,
    pub z_order_timeline: Option<Box<TimelineData>>,
    pub parent: String,
}

impl Default for AnimationData {
    fn default() -> Self {
        Self {
            frame_int_offset: 0,
            frame_float_offset: 0,
            frame_offset: 0,
            blend_type: AnimationBlendType::None,
            frame_count: 0,
            play_times: 0,
            duration: 0.0,
            scale: 1.0,
            fade_in_time: 0.0,
            cache_frame_rate: 0.0,
            name: String::new(),
            cached_frames: Vec::new(),
            bone_timelines: IndexMap::new(),
            slot_timelines: IndexMap::new(),
            constraint_timelines: IndexMap::new(),
            animation_timelines: IndexMap::new(),
            bone_cached_frame_indices: IndexMap::new(),
            slot_cached_frame_indices: IndexMap::new(),
            action_timeline: None,
            z_order_timeline: None,
            parent: String::new(),
        }
    }
}

impl AnimationData {
    /// 对齐 `AnimationData.addBoneTimeline`。
    pub fn add_bone_timeline(&mut self, timeline_name: &str, timeline: TimelineData) {
        push_timeline(&mut self.bone_timelines, timeline_name, timeline);
    }

    /// 对齐 `AnimationData.addSlotTimeline`。
    pub fn add_slot_timeline(&mut self, timeline_name: &str, timeline: TimelineData) {
        push_timeline(&mut self.slot_timelines, timeline_name, timeline);
    }

    /// 对齐 `AnimationData.addConstraintTimeline`。
    pub fn add_constraint_timeline(&mut self, timeline_name: &str, timeline: TimelineData) {
        push_timeline(&mut self.constraint_timelines, timeline_name, timeline);
    }

    /// 对齐 `AnimationData.addAnimationTimeline`。
    pub fn add_animation_timeline(&mut self, timeline_name: &str, timeline: TimelineData) {
        push_timeline(&mut self.animation_timelines, timeline_name, timeline);
    }

    /// 对齐 `AnimationData.getBoneTimelines`。
    #[must_use]
    pub fn get_bone_timelines(&self, timeline_name: &str) -> Option<&[TimelineData]> {
        self.bone_timelines.get(timeline_name).map(Vec::as_slice)
    }

    /// 对齐 `AnimationData.getSlotTimelines`。
    #[must_use]
    pub fn get_slot_timelines(&self, timeline_name: &str) -> Option<&[TimelineData]> {
        self.slot_timelines.get(timeline_name).map(Vec::as_slice)
    }

    /// 对齐 `AnimationData.getConstraintTimelines`。
    #[must_use]
    pub fn get_constraint_timelines(&self, timeline_name: &str) -> Option<&[TimelineData]> {
        self.constraint_timelines
            .get(timeline_name)
            .map(Vec::as_slice)
    }

    /// 对齐 `AnimationData.getAnimationTimelines`。
    #[must_use]
    pub fn get_animation_timelines(&self, timeline_name: &str) -> Option<&[TimelineData]> {
        self.animation_timelines
            .get(timeline_name)
            .map(Vec::as_slice)
    }

    /// 对齐 `AnimationData.getBoneCachedFrameIndices`。
    #[must_use]
    pub fn get_bone_cached_frame_indices(&self, bone_name: &str) -> Option<&[i32]> {
        self.bone_cached_frame_indices
            .get(bone_name)
            .map(Vec::as_slice)
    }

    /// 对齐 `AnimationData.getSlotCachedFrameIndices`。
    #[must_use]
    pub fn get_slot_cached_frame_indices(&self, slot_name: &str) -> Option<&[i32]> {
        self.slot_cached_frame_indices
            .get(slot_name)
            .map(Vec::as_slice)
    }
}

fn push_timeline(
    map: &mut IndexMap<String, Vec<TimelineData>>,
    timeline_name: &str,
    timeline: TimelineData,
) {
    map.entry(timeline_name.to_string())
        .or_default()
        .push(timeline);
}
