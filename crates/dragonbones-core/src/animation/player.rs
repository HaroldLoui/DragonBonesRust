//! Animation container and stable state ordering (`Animation.ts`).

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::armature::{ArmatureIdx, ArmatureProxy, Bone, Constraint, FrameCache, RuntimeCtx, Slot};
use crate::core::enums::AnimationFadeOutMode;
use crate::event::EventObject;
use crate::model::{AnimationConfig, AnimationData, ArmatureData, DragonBonesData};

use super::animation_state::{AnimationState, FadeEvent, FadeState, SubFadeState};
use super::blend_state::{BlendState, BlendStateKey};
use super::state_id::{StateArena, StateArenaError, StateId};
use super::timeline_states::{AnimationTimelineSource, TimelineInstance, TimelineTarget};
use crate::core::enums::TimelineType;

/// Armature-owned mutable targets and read-only data visible to the timeline
/// update. `Armature` itself is taken out of its arena while this context lives.
pub(crate) struct AnimationUpdateCtx<'a> {
    pub armature_idx: ArmatureIdx,
    pub armature_data: &'a ArmatureData,
    pub bones: &'a mut [Bone],
    pub slots: &'a mut [Slot],
    pub constraints: &'a mut [Constraint],
    pub actions: &'a mut Vec<EventObject>,
    pub cache_frame_index: &'a mut i32,
    pub frame_cache: Option<&'a mut FrameCache>,
    pub alpha_dirty: &'a mut bool,
    pub slots_dirty: &'a mut bool,
    pub z_order_dirty: &'a mut bool,
    pub z_index_dirty: &'a mut bool,
    pub proxy: &'a mut dyn ArmatureProxy,
}

impl AnimationUpdateCtx<'_> {
    pub(crate) fn buffer_lifecycle_event(
        &mut self,
        runtime: &mut RuntimeCtx<'_>,
        event_type: &'static str,
    ) {
        let event = EventObject {
            event_type: event_type.to_owned(),
            armature: Some(self.armature_idx),
            ..EventObject::default()
        };
        runtime.buffer_event(event);
    }
}

impl AnimationUpdateCtx<'_> {
    /// Timeline-side equivalent of `Armature._sortZOrder`.
    pub(crate) fn sort_z_order(&mut self, slot_indices: Option<&[i32]>, offset: usize) {
        let is_original = slot_indices.is_none();
        if !*self.z_order_dirty && is_original {
            return;
        }
        let sorted_slot_names = &self.armature_data.sorted_slots;
        for index in 0..sorted_slot_names.len() {
            let slot_index = if let Some(indices) = slot_indices {
                let Some(slot_index) = offset.checked_add(index).and_then(|i| indices.get(i))
                else {
                    continue;
                };
                *slot_index
            } else {
                index as i32
            };
            let Ok(slot_index) = usize::try_from(slot_index) else {
                continue;
            };
            let Some(slot_name) = sorted_slot_names.get(slot_index) else {
                continue;
            };
            if let Some(slot) = self
                .slots
                .iter_mut()
                .find(|slot| slot.slot_name == *slot_name)
            {
                slot.set_z_order(index as i32);
            }
        }
        *self.slots_dirty = true;
        *self.z_order_dirty = !is_original;
    }
}

/// Animation player and owner of its state arena.
pub struct Animation {
    pub armature: Option<ArmatureIdx>,
    data: Option<Arc<DragonBonesData>>,
    armature_name: String,
    states: StateArena<AnimationState>,
    play_order: Vec<StateId>,
    last_state: Option<StateId>,
    animation_config: AnimationConfig,
    blend_states: HashMap<BlendStateKey, BlendState>,
    /// 父骨架 `playConfig` 新状态创建后请求子骨架同名继承（`Animation.ts:443-452`）。
    /// TODO D-4.36：按 arena 序（父 < 子）在同一帧内排空；仅 `play()` 返回时子状态
    /// 尚未创建。若某子骨架 idx < 父（工厂正常装配不会出现），其本帧已提前推进，
    /// 子状态要晚一帧才首次 advance —— 见 `take_child_animation_requests` 说明。
    pending_child_animations: Vec<String>,
    pub time_scale: f64,
    inherit_time_scale: f64,
    /// 多状态回收置位（`Animation.ts:239`）；消费点为单状态路径的
    /// `cachedFrameIndices` 挂载（`Animation.ts:204-228`）。
    /// TODO P1-4：阶段 6 接入 `bone/slot._cachedFrameIndices` 时消费并清除；
    /// 当前仅写，`cache_frame_rate == 0` 时官方也永不消费（`:207`）。
    animations_override: Option<IndexMap<String, AnimationData>>,
    animation_dirty: bool,
    cache_frame_invalidated: bool,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            armature: None,
            data: None,
            armature_name: String::new(),
            states: StateArena::default(),
            play_order: Vec::new(),
            last_state: None,
            animation_config: AnimationConfig::default(),
            blend_states: HashMap::new(),
            pending_child_animations: Vec::new(),
            time_scale: 1.0,
            inherit_time_scale: 1.0,
            animations_override: None,
            animation_dirty: false,
            cache_frame_invalidated: false,
        }
    }
}

impl Animation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds this Animation to its armature and immutable data source.
    pub fn init(
        &mut self,
        armature: ArmatureIdx,
        data: Arc<DragonBonesData>,
        armature_name: impl Into<String>,
    ) {
        if self.armature.is_some() {
            return;
        }
        self.armature = Some(armature);
        self.data = Some(data);
        self.armature_name = armature_name.into();
    }

    /// Returns animation names in the source `ArmatureData` insertion order.
    #[must_use]
    pub fn animation_names(&self) -> &[String] {
        self.armature_data().map_or(&[], |armature_data| {
            armature_data.animation_names.as_slice()
        })
    }

    /// Resolves immutable animation data by name.
    #[must_use]
    pub fn animation_data(&self, name: &str) -> Option<&AnimationData> {
        if let Some(animations) = self.animations_override.as_ref() {
            return animations.get(name);
        }
        self.armature_data()?.get_animation(name)
    }

    #[must_use]
    pub fn has_animation(&self, name: &str) -> bool {
        self.animation_data(name).is_some()
    }

    #[must_use]
    pub fn animation_config(&mut self) -> &mut AnimationConfig {
        self.animation_config.clear();
        &mut self.animation_config
    }

    /// 阶段 6：对齐 `Animation.animations` 的替换（`replaceAnimation`）。
    ///
    /// data 不可变 → 每 Animation 保存 override map（D-6.9）。
    pub fn replace_animations(
        &mut self,
        animations: IndexMap<String, AnimationData>,
        is_override: bool,
    ) {
        if is_override {
            self.animations_override = Some(animations);
        } else {
            let map = self
                .animations_override
                .get_or_insert_with(IndexMap::new);
            for (name, animation) in animations {
                map.insert(name, animation);
            }
        }
        self.animation_dirty = true;
    }

    fn armature_data(&self) -> Option<&crate::model::ArmatureData> {
        let data = self.data.as_ref()?;
        data.armatures.get(&self.armature_name)
    }

    /// Gets the newest matching state by scanning the ordered play list backwards.
    #[must_use]
    pub fn get_state(&self, name: &str, layer: i32) -> Option<StateId> {
        self.play_order.iter().rev().copied().find(|id| {
            self.states
                .get(*id)
                .is_some_and(|state| state.name == name && (layer < 0 || state.layer == layer))
        })
    }

    #[must_use]
    pub fn state(&self, id: StateId) -> Option<&AnimationState> {
        self.states.get(id)
    }

    pub fn state_mut(&mut self, id: StateId) -> Option<&mut AnimationState> {
        self.states.get_mut(id)
    }

    #[must_use]
    pub fn states(&self) -> impl DoubleEndedIterator<Item = (StateId, &AnimationState)> {
        self.play_order
            .iter()
            .copied()
            .filter_map(|id| self.states.get(id).map(|state| (id, state)))
    }

    pub(crate) fn insert_state(
        &mut self,
        state: AnimationState,
    ) -> Result<StateId, StateArenaError> {
        let id = self.states.insert(state)?;
        // Store the stable identity in the state after the arena assigned it.
        self.states
            .get_mut(id)
            .expect("newly inserted StateId must resolve")
            .id = Some(id);
        // The first inserted state at a layer stays first; higher layers are
        // inserted before lower layers.
        let layer = self
            .states
            .get(id)
            .expect("newly inserted StateId must resolve")
            .layer;
        let index = self
            .play_order
            .iter()
            .position(|existing| {
                self.states
                    .get(*existing)
                    .is_some_and(|existing| layer > existing.layer)
            })
            .unwrap_or(self.play_order.len());
        self.play_order.insert(index, id);
        self.last_state = Some(id);
        Ok(id)
    }

    pub(crate) fn remove_state(&mut self, id: StateId) -> Option<AnimationState> {
        let state = self.states.remove(id)?;
        self.play_order.retain(|existing| *existing != id);
        if self.last_state == Some(id) {
            self.last_state = self.play_order.last().copied();
        }

        if let Some(parent_id) = state.parent
            && let Some(parent) = self.states.get_mut(parent_id)
        {
            parent
                .animation_timelines
                .retain(|timeline| timeline.target != TimelineTarget::AnimationState(id));
        }
        for timeline in &state.animation_timelines {
            let TimelineTarget::AnimationState(child_id) = timeline.target else {
                continue;
            };
            if let Some(child) = self.states.get_mut(child_id)
                && child.parent == Some(id)
            {
                child.fade_state = FadeState::FadeOut;
                child.sub_fade_state = SubFadeState::Complete;
                child.parent = None;
            }
        }
        Some(state)
    }

    pub(crate) fn reset_blend_states(&mut self) {
        for state in self.blend_states.values_mut() {
            state.reset();
        }
    }

    /// Clears active states and reusable config while retaining armature/data bindings.
    /// This matches `Animation.reset()` and is not a full `_onClear`.
    pub fn reset(&mut self) {
        let _ = self.states.clear();
        self.play_order.clear();
        self.last_state = None;
        self.pending_child_animations.clear();
        self.animation_dirty = false;
        self.cache_frame_invalidated = false;
        self.animation_config.clear();
    }

    /// Fully clears this Animation before its owning Armature is disposed.
    pub fn on_clear(&mut self) {
        self.reset();
        self.blend_states.clear();
        self.data = None;
        self.armature_name.clear();
        self.armature = None;
        self.time_scale = 1.0;
        self.inherit_time_scale = 1.0;
        self.pending_child_animations.clear();
        self.animations_override = None;
        self.animation_dirty = false;
    }

    /// Advances Animation-level time scaling and each active AnimationState.
    /// Timeline/action groups are attached in the following timeline task.
    pub fn advance_time(
        &mut self,
        mut passed_time: f64,
        parent_inherit_time_scale: Option<f64>,
    ) -> Vec<(FadeEvent, bool)> {
        if passed_time < 0.0 {
            passed_time = -passed_time;
        }

        self.inherit_time_scale = if let Some(parent_scale) = parent_inherit_time_scale {
            parent_scale * self.time_scale
        } else {
            self.time_scale
        };
        if self.inherit_time_scale != 1.0 {
            passed_time *= self.inherit_time_scale;
        }

        self.reset_blend_states();

        if self.play_order.is_empty() {
            self.cache_frame_invalidated = true;
        }

        let state_ids = self.play_order.clone();
        let state_count = state_ids.len();
        let mut remove = Vec::new();
        let mut fade_events = Vec::new();
        for id in state_ids {
            let Some(state) = self.states.get(id) else {
                continue;
            };
            if state.fade_state == FadeState::FadeOut
                && state.sub_fade_state == SubFadeState::Complete
            {
                remove.push(id);
                continue;
            }

            let parent_weight = state
                .parent
                .and_then(|parent| self.states.get(parent))
                .map_or(1.0, |parent| parent.weight_result);
            let event_active = state.parent.is_none() && state.action_enabled;
            let started_fade_out = if let Some(state) = self.states.get_mut(id) {
                let was_fading_out = state.fade_state == FadeState::FadeOut;
                for event in state.advance_time(passed_time, parent_weight) {
                    fade_events.push((event, event_active));
                }
                // AnimationState.ts:846-848：weight==0 在 action 更新后直接 return，
                // 跳过 :1018-1069 的淡入善后与自动淡出。此公开 seam 与带 context 的
                // 路径保持同一早退门（带 context 版本在状态内部提前 return）。
                if state.weight != 0.0 {
                    state.finish_advance();
                }
                !was_fading_out && state.fade_state == FadeState::FadeOut
            } else {
                false
            };
            self.apply_animation_timeline_results(id);
            if started_fade_out {
                self.fade_out_children(id);
            }
        }

        if !remove.is_empty() {
            for id in remove {
                self.remove_state(id);
            }
            // Animation.ts:196-202：单状态回收只清 states/last，不置
            // `_animationDirty`，也不改 `_cacheFrameIndex`。
            // Animation.ts:233-262：多状态回收置 dirty，并把缓存帧号归 -1。
            if state_count > 1 {
                self.animation_dirty = true;
                self.cache_frame_invalidated = true;
            }
        }
        fade_events
    }

    /// Armature call path, extended with the current target slices for timeline
    /// construction/update. Timeline groups are populated in the timeline task.
    pub(crate) fn advance_time_with_context(
        &mut self,
        mut passed_time: f64,
        parent_inherit_time_scale: Option<f64>,
        context: &mut AnimationUpdateCtx<'_>,
        runtime: &mut RuntimeCtx<'_>,
    ) -> Vec<(FadeEvent, bool)> {
        if passed_time < 0.0 {
            passed_time = -passed_time;
        }
        self.inherit_time_scale = parent_inherit_time_scale
            .map_or(self.time_scale, |parent_scale| {
                parent_scale * self.time_scale
            });
        if self.inherit_time_scale != 1.0 {
            passed_time *= self.inherit_time_scale;
        }

        self.reset_blend_states();
        if self.play_order.is_empty() {
            // Animation.ts:264-265：零状态 → `_cacheFrameIndex = -1`。
            *context.cache_frame_index = -1;
        }

        let ids = self.play_order.clone();
        let state_count = ids.len();
        let animation_name = ids
            .first()
            .and_then(|id| self.states.get(*id))
            .map(|state| state.animation_name.clone())
            .unwrap_or_default();
        let cache_frame_rate = if state_count == 1 {
            context
                .frame_cache
                .as_ref()
                .and_then(|cache| cache.animation(&animation_name))
                .map_or(0.0, |cache| cache.rate)
        } else {
            // Animation.ts:250/262：多状态恒传 cacheFrameRate=0，且帧号归 -1。
            *context.cache_frame_index = -1;
            0.0
        };
        if self.animation_dirty && cache_frame_rate > 0.0 {
            if let Some(cache) = context
                .frame_cache
                .as_deref()
                .and_then(|cache| cache.animation(&animation_name))
            {
                for bone in context.bones.iter_mut() {
                    if let Some(indices) = cache.bone_indices.get(&bone.bone_name) {
                        bone.cached_frame_indices = Some(indices.clone());
                    }
                }
                for slot in context.slots.iter_mut() {
                    if let Some(indices) = cache.slot_indices.get(&slot.slot_name) {
                        slot.cached_frame_indices = Some(indices.clone());
                    }
                }
            }
            self.animation_dirty = false;
        }
        let mut remove = Vec::new();
        let mut events = Vec::new();
        for id in ids {
            let Some(state) = self.states.get(id) else {
                continue;
            };
            if state.fade_state == FadeState::FadeOut
                && state.sub_fade_state == SubFadeState::Complete
            {
                remove.push(id);
                continue;
            }
            let parent_weight = state
                .parent
                .and_then(|parent| self.states.get(parent))
                .map_or(1.0, |parent| parent.weight_result);
            let event_active = state.parent.is_none() && state.action_enabled;
            let started_fade_out = if let Some(state) = self.states.get_mut(id) {
                let was_fading_out = state.fade_state == FadeState::FadeOut;
                for event in state.advance_time_with_context(
                    passed_time,
                    parent_weight,
                    context,
                    runtime,
                    cache_frame_rate,
                    &mut self.blend_states,
                ) {
                    events.push((event, event_active));
                }
                !was_fading_out && state.fade_state == FadeState::FadeOut
            } else {
                false
            };
            self.apply_animation_timeline_results(id);
            if started_fade_out {
                self.fade_out_children(id);
            }
        }

        if !remove.is_empty() {
            for id in remove {
                self.remove_state(id);
            }
            // Animation.ts:196-202：单状态回收只清 states/last，不置 dirty，
            // 也不改缓存帧号。Animation.ts:233-262：多状态回收才置 dirty 并
            // 最终把 `_cacheFrameIndex` 归 -1。
            if state_count > 1 {
                self.animation_dirty = true;
                *context.cache_frame_index = -1;
            }
        }
        events
    }

    /// Plays an animation using TS defaults and resume/replay behavior.
    pub fn play(&mut self, animation_name: Option<&str>, play_times: i32) -> Option<StateId> {
        let mut config = std::mem::take(&mut self.animation_config);
        config.clear();
        config.reset_to_pose = true;
        config.play_times = play_times;
        config.fade_in_time = 0.0;
        config.animation = animation_name.unwrap_or("").to_owned();

        let result = if !config.animation.is_empty() {
            self.play_config(&mut config)
        } else if self.last_state.is_none() {
            if let Some(default_animation) = self
                .armature_data()
                .and_then(|armature| armature.default_animation.as_deref())
            {
                config.animation = default_animation.to_owned();
                self.play_config(&mut config)
            } else {
                None
            }
        } else if let Some(last_state) = self.last_state
            && let Some(state) = self.states.get_mut(last_state)
            && !state.is_playing()
            && !state.is_completed()
        {
            state.play();
            Some(last_state)
        } else if let Some(last_state) = self.last_state
            && let Some(state) = self.states.get(last_state)
        {
            config.animation.clone_from(&state.name);
            self.play_config(&mut config)
        } else {
            None
        };

        self.animation_config = config;
        result.or(self.last_state)
    }

    /// Fades in an animation using the official `fadeIn` defaults.
    pub fn fade_in(&mut self, animation_name: &str) -> Option<StateId> {
        self.fade_in_with_options(
            animation_name,
            -1.0,
            -1,
            0,
            None,
            AnimationFadeOutMode::SameLayerAndGroup,
        )
    }

    /// Full `fadeIn` configuration for callers that need non-default values.
    #[allow(clippy::too_many_arguments)]
    pub fn fade_in_with_options(
        &mut self,
        animation_name: &str,
        fade_in_time: f64,
        play_times: i32,
        layer: i32,
        group: Option<&str>,
        fade_out_mode: AnimationFadeOutMode,
    ) -> Option<StateId> {
        let mut config = std::mem::take(&mut self.animation_config);
        config.clear();
        config.fade_out_mode = fade_out_mode;
        config.play_times = play_times;
        config.layer = layer;
        config.fade_in_time = fade_in_time;
        config.animation = animation_name.to_owned();
        config.group = group.unwrap_or("").to_owned();
        let result = self.play_config(&mut config);
        self.animation_config = config;
        result
    }

    pub fn goto_and_play_by_time(
        &mut self,
        animation_name: &str,
        time: f64,
        play_times: i32,
    ) -> Option<StateId> {
        let mut config = std::mem::take(&mut self.animation_config);
        config.clear();
        config.reset_to_pose = true;
        config.play_times = play_times;
        config.position = time;
        config.fade_in_time = 0.0;
        config.animation = animation_name.to_owned();
        let state = self.play_config(&mut config);
        self.animation_config = config;
        state
    }

    pub fn goto_and_play_by_frame(
        &mut self,
        animation_name: &str,
        frame: i32,
        play_times: i32,
    ) -> Option<StateId> {
        let mut config = std::mem::take(&mut self.animation_config);
        config.clear();
        config.reset_to_pose = true;
        config.play_times = play_times;
        config.fade_in_time = 0.0;
        config.animation = animation_name.to_owned();
        if let Some(animation) = self.animation_data(animation_name) {
            config.position = if animation.frame_count > 0 {
                animation.duration * f64::from(frame) / f64::from(animation.frame_count)
            } else {
                0.0
            };
        }
        let state = self.play_config(&mut config);
        self.animation_config = config;
        state
    }

    pub fn goto_and_play_by_progress(
        &mut self,
        animation_name: &str,
        progress: f64,
        play_times: i32,
    ) -> Option<StateId> {
        let mut config = std::mem::take(&mut self.animation_config);
        config.clear();
        config.reset_to_pose = true;
        config.play_times = play_times;
        config.fade_in_time = 0.0;
        config.animation = animation_name.to_owned();
        if let Some(animation) = self.animation_data(animation_name) {
            config.position = animation.duration * if progress > 0.0 { progress } else { 0.0 };
        }
        let state = self.play_config(&mut config);
        self.animation_config = config;
        state
    }

    pub fn goto_and_stop_by_time(&mut self, animation_name: &str, time: f64) -> Option<StateId> {
        let id = self.goto_and_play_by_time(animation_name, time, 1)?;
        self.states.get_mut(id)?.stop();
        Some(id)
    }

    pub fn goto_and_stop_by_frame(&mut self, animation_name: &str, frame: i32) -> Option<StateId> {
        let id = self.goto_and_play_by_frame(animation_name, frame, 1)?;
        self.states.get_mut(id)?.stop();
        Some(id)
    }

    pub fn goto_and_stop_by_progress(
        &mut self,
        animation_name: &str,
        progress: f64,
    ) -> Option<StateId> {
        let id = self.goto_and_play_by_progress(animation_name, progress, 1)?;
        self.states.get_mut(id)?.stop();
        Some(id)
    }

    /// Plays using a normalized caller-owned config. Normalized values are written back.
    pub fn play_config(&mut self, config: &mut AnimationConfig) -> Option<StateId> {
        let data = self.data.as_ref()?.clone();
        let armature_data = data.armatures.get(&self.armature_name)?;
        let animation_source = match self.animations_override.as_ref() {
            Some(animations) => animations.get(&config.animation),
            None => armature_data.animations.get(&config.animation),
        };
        let Some(animation_data) = animation_source.cloned() else {
            log::warn!("non-existent animation `{}`", config.animation);
            return None;
        };

        if config.fade_out_mode == AnimationFadeOutMode::Single {
            for id in &self.play_order {
                if let Some(state) = self.states.get(*id)
                    && state.fade_state != FadeState::FadeOut
                    && state.layer == config.layer
                    && state.animation_name == animation_data.name
                {
                    return Some(*id);
                }
            }
        }

        if self.play_order.is_empty() {
            config.fade_in_time = 0.0;
        } else if config.fade_in_time < 0.0 {
            config.fade_in_time = animation_data.fade_in_time;
        }
        if config.fade_out_time < 0.0 {
            config.fade_out_time = config.fade_in_time;
        }
        if config.time_scale <= -100.0 {
            config.time_scale = 1.0 / animation_data.scale;
        }

        if animation_data.frame_count > 0 {
            if config.position < 0.0 {
                config.position %= animation_data.duration;
                config.position = animation_data.duration - config.position;
            } else if config.position == animation_data.duration {
                config.position -= 0.000001;
            } else if config.position > animation_data.duration {
                config.position %= animation_data.duration;
            }

            if config.duration > 0.0 && config.position + config.duration > animation_data.duration
            {
                config.duration = animation_data.duration - config.position;
            }
            if config.play_times < 0 {
                config.play_times = animation_data.play_times;
            }
        } else {
            config.play_times = 1;
            config.position = 0.0;
            if config.duration > 0.0 {
                config.duration = 0.0;
            }
        }
        if config.duration == 0.0 {
            config.duration = -1.0;
        }

        self.fade_out_states(config);
        // TS 从对象池取 AnimationState（_onClear 默认值）；Rust 每次构造 Default。
        // 即使 StateArena 复用刚释放的槽位，新值也不携带旧 fade/playhead 残留。
        let mut state = AnimationState::default();
        let armature_idx = self.armature?;
        state.init(armature_idx, &animation_data, config);
        let source = AnimationTimelineSource {
            data: Arc::clone(&data),
            owner_armature: animation_data.parent.clone(),
            animation_name: animation_data.name.clone(),
        };
        let host_armature_data = data.armatures.get(&self.armature_name)?;
        state.action_timeline = Some(TimelineInstance::new(
            TimelineType::Action,
            TimelineTarget::Armature(armature_idx),
            None,
            animation_data.action_timeline.as_deref().cloned(),
            source.clone(),
            &state,
            host_armature_data,
            None,
        )?);
        state.z_order_timeline = animation_data
            .z_order_timeline
            .as_deref()
            .and_then(|timeline| {
                TimelineInstance::new(
                    TimelineType::ZOrder,
                    TimelineTarget::Armature(armature_idx),
                    None,
                    Some(timeline.clone()),
                    source.clone(),
                    &state,
                    host_armature_data,
                    None,
                )
            });
        let id = match self.insert_state(state) {
            Ok(id) => id,
            Err(error) => {
                log::warn!("could not allocate AnimationState: {error:?}");
                return None;
            }
        };
        // Animation.ts:443-452 的子骨架继承在 Rust 借 arena 的约束下改为排队：
        // 请求在本骨架同帧 advance 尾部由 `take_child_animation_requests` 排空。
        // D-4.36：play() 返回时子状态尚未创建；正常 arena 序（父 < 子）下同帧生效。
        self.pending_child_animations
            .push(animation_data.name.clone());
        self.animation_dirty = true;
        self.cache_frame_invalidated = true;

        for (child_name, timeline_data) in &animation_data.animation_timelines {
            let child_id = self.fade_in_with_options(
                child_name,
                0.0,
                1,
                config.layer,
                Some(""),
                AnimationFadeOutMode::Single,
            );
            let Some(child_id) = child_id else {
                continue;
            };
            if child_id == id || self.would_create_state_cycle(id, child_id) {
                log::warn!(
                    "animation timeline `{child_name}` would create a parent-state cycle; ignored"
                );
                continue;
            }

            if let Some(child) = self.states.get_mut(child_id) {
                child.action_enabled = false;
                child.reset_to_pose = false;
                child.stop();
            }

            let parent_state = self.states.get(id)?;
            let mut child_timelines = Vec::new();
            for timeline_data in timeline_data {
                if !matches!(
                    timeline_data.timeline_type,
                    TimelineType::AnimationProgress
                        | TimelineType::AnimationWeight
                        | TimelineType::AnimationParameter
                ) {
                    continue;
                }
                let Some(timeline) = TimelineInstance::new(
                    timeline_data.timeline_type,
                    TimelineTarget::AnimationState(child_id),
                    None,
                    Some(timeline_data.clone()),
                    source.clone(),
                    parent_state,
                    host_armature_data,
                    None,
                ) else {
                    continue;
                };
                child_timelines.push((timeline_data.clone(), timeline));
            }

            let is_blended =
                parent_state.blend_type != crate::core::enums::AnimationBlendType::None;
            for (timeline_data, timeline) in child_timelines {
                if timeline_data.timeline_type == TimelineType::AnimationProgress {
                    if let Some(parent) = self.states.get_mut(id) {
                        parent.reset_to_pose = false;
                    }
                    if is_blended && let Some(child) = self.states.get_mut(child_id) {
                        child.position_x = timeline_data.x;
                        child.position_y = timeline_data.y;
                        child.set_weight(0.0);
                    }
                }
                if let Some(parent) = self.states.get_mut(id) {
                    parent.push_timeline(
                        crate::animation::animation_state::TimelineGroup::Animation,
                        timeline,
                        false,
                    );
                }
            }
            if let Some(child) = self.states.get_mut(child_id)
                && child.parent.is_none()
            {
                child.parent = Some(id);
            }

            self.reorder_parent_before_child(id, child_id);
        }
        self.last_state = Some(id);
        Some(id)
    }

    fn would_create_state_cycle(&self, parent: StateId, child: StateId) -> bool {
        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child {
                return true;
            }
            ancestor = self.states.get(id).and_then(|state| state.parent);
        }
        false
    }

    fn reorder_parent_before_child(&mut self, parent: StateId, child: StateId) {
        let Some(parent_index) = self.play_order.iter().position(|id| *id == parent) else {
            return;
        };
        let Some(child_index) = self.play_order.iter().position(|id| *id == child) else {
            return;
        };
        if child_index < parent_index {
            let parent_id = self.play_order.remove(parent_index);
            self.play_order.insert(child_index, parent_id);
        }
    }

    fn fade_out_states(&mut self, config: &AnimationConfig) {
        let ids = self.play_order.clone();
        for id in ids {
            let Some(state) = self.states.get(id) else {
                continue;
            };
            if state.parent.is_some() {
                continue;
            }
            let matches = match config.fade_out_mode {
                AnimationFadeOutMode::SameLayer => state.layer == config.layer,
                AnimationFadeOutMode::SameGroup => state.group == config.group,
                AnimationFadeOutMode::SameLayerAndGroup => {
                    state.layer == config.layer && state.group == config.group
                }
                AnimationFadeOutMode::All => true,
                AnimationFadeOutMode::Single => false,
            };
            if matches {
                self.fade_out_state_and_children(id, config.fade_out_time, config.pause_fade_out);
            }
        }
    }

    fn child_state_ids(&self, id: StateId) -> Vec<StateId> {
        self.states
            .get(id)
            .into_iter()
            .flat_map(|state| &state.animation_timelines)
            .filter_map(|timeline| match timeline.target {
                TimelineTarget::AnimationState(child) => Some(child),
                _ => None,
            })
            .collect()
    }

    fn apply_animation_timeline_results(&mut self, parent_id: StateId) {
        let updates: Vec<_> = self
            .states
            .get(parent_id)
            .into_iter()
            .flat_map(|parent| &parent.animation_timelines)
            .filter_map(|timeline| match timeline.target {
                TimelineTarget::AnimationState(child_id) => {
                    Some((child_id, timeline.timeline_type, timeline.results.clone()))
                }
                _ => None,
            })
            .collect();

        for (child_id, timeline_type, values) in updates {
            let Some(child) = self.states.get_mut(child_id) else {
                continue;
            };
            if child.parent != Some(parent_id) {
                continue;
            }
            match timeline_type {
                TimelineType::AnimationProgress => {
                    child.set_current_time(values.first().copied().unwrap_or(0.0) * child.duration);
                }
                TimelineType::AnimationWeight => {
                    child.set_weight(values.first().copied().unwrap_or(0.0));
                }
                TimelineType::AnimationParameter => {
                    child.parameter_x = values.first().copied().unwrap_or(0.0);
                    child.parameter_y = values.get(1).copied().unwrap_or(0.0);
                }
                _ => {}
            }
        }
    }

    fn fade_out_state_and_children(
        &mut self,
        root: StateId,
        fade_out_time: f64,
        pause_playhead: bool,
    ) {
        let mut pending = vec![(root, fade_out_time, pause_playhead)];
        let mut visited = std::collections::HashSet::new();
        while let Some((id, time, pause)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let children = self.child_state_ids(id);
            let began_fade_out = self
                .states
                .get_mut(id)
                .is_some_and(|state| state.fade_out(time, pause));
            // AnimationState.ts:1153-1158：子状态 999999 淡出只在父状态本次
            // 真正进入淡出 else 分支时执行；root 被 :1120-1123 guard 忽略则不得级联。
            if began_fade_out {
                pending.extend(children.into_iter().map(|child| (child, 999_999.0, true)));
            }
        }
    }

    fn fade_out_children(&mut self, parent: StateId) {
        let mut pending: Vec<_> = self
            .child_state_ids(parent)
            .into_iter()
            .map(|child| (child, 999_999.0, true))
            .collect();
        let mut visited = std::collections::HashSet::from([parent]);
        while let Some((id, time, pause)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let children = self.child_state_ids(id);
            let began_fade_out = self
                .states
                .get_mut(id)
                .is_some_and(|state| state.fade_out(time, pause));
            if began_fade_out {
                pending.extend(children.into_iter().map(|child| (child, 999_999.0, true)));
            }
        }
    }

    /// Stops one named state, or every state when `animation_name` is `None`.
    pub fn stop(&mut self, animation_name: Option<&str>) {
        if let Some(name) = animation_name {
            if let Some(id) = self.get_state(name, -1)
                && let Some(state) = self.states.get_mut(id)
            {
                state.stop();
            }
        } else {
            for id in self.play_order.clone() {
                if let Some(state) = self.states.get_mut(id) {
                    state.stop();
                }
            }
        }
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.states().any(|(_, state)| state.is_playing())
    }

    #[must_use]
    pub fn is_completed(&self) -> bool {
        !self.play_order.is_empty() && self.states().all(|(_, state)| state.is_completed())
    }

    #[must_use]
    pub fn last_animation_name(&self) -> &str {
        self.last_state
            .and_then(|id| self.states.get(id))
            .map_or("", |state| state.name.as_str())
    }

    /// 由 `Armature::advance_time` 在本骨架 animation 推进后、同帧 actions 处理后再排空
    /// （armature.rs `process_child_animation_requests` 两处调用）。D-4.36：
    /// clock 成员按 arena/插入序推进，工厂装配保证父臂架 idx < 子臂架 idx，因此在父臂架
    /// advance 内排空的子 `fade_in` 会在**同一帧**稍后轮到子骨架时生效；只有 `play()`
    /// 返回那一刻子状态尚未创建（请求已入队）。若外部替换子显示得到 idx 更小的子骨架，
    /// 子骨架在本帧已提前推进，则子状态会晚一帧首次 advance——此为本借用架构的已知边界。
    pub fn take_child_animation_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_child_animations)
    }

    pub fn take_cache_frame_invalidation(&mut self) -> bool {
        std::mem::take(&mut self.cache_frame_invalidated)
    }

    #[must_use]
    pub fn last_state(&self) -> Option<StateId> {
        self.last_state
    }

    #[must_use]
    pub fn inherit_time_scale(&self) -> f64 {
        self.inherit_time_scale
    }

    #[must_use]
    pub fn animation_dirty(&self) -> bool {
        self.animation_dirty
    }

    /// 阶段 6：cacheFrameRate 设置后强制下一帧重新分配 cachedFrameIndices。
    pub(crate) fn mark_animation_dirty(&mut self) {
        self.animation_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{Animation, AnimationUpdateCtx, FadeEvent, FadeState};
    use crate::animation::animation_state::{AnimationState, PlayheadState, SubFadeState};
    use crate::animation::blend_state::{BlendStateKey, BlendStateKind};
    use crate::animation::timeline_state::TimelinePlayState;
    use crate::animation::timeline_states::{TimelineInstance, TimelineTarget};
    use crate::animation::world_clock::ClockArena;
    use crate::armature::{ArmatureIdx, FrameCache, NullArmatureProxy, RuntimeCtx};
    use crate::core::enums::{AnimationFadeOutMode, TimelineType};
    use crate::event::{EventDispatcher, EventObject};
    use crate::model::{AnimationData, ArmatureData, DragonBonesData, IrArrays};
    use std::sync::Arc;

    fn walk_animation() -> Animation {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            frame_rate: 24.0,
            ..ArmatureData::default()
        };
        armature_data.add_animation(AnimationData {
            name: "walk".to_owned(),
            duration: 1.0,
            frame_count: 24,
            play_times: 3,
            cache_frame_rate: 10.0,
            ..AnimationData::default()
        });
        data.add_armature(armature_data);
        data.finalize_root(IrArrays::default());
        let mut animation = Animation::new();
        animation.init(ArmatureIdx(0), Arc::new(data), "test");
        animation
    }

    /// Parent + animation-timeline child fixture (same shape as the existing
    /// `animation_progress_timeline_creates_a_stopped_parented_child_state`).
    fn blend_animation() -> (Animation, super::StateId, super::StateId) {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            ..ArmatureData::default()
        };
        let mut parent_data = AnimationData {
            name: "blend".to_owned(),
            duration: 1.0,
            frame_count: 24,
            blend_type: crate::core::enums::AnimationBlendType::E1D,
            ..AnimationData::default()
        };
        parent_data.animation_timelines.insert(
            "child".to_owned(),
            vec![crate::model::TimelineData {
                timeline_type: TimelineType::AnimationProgress,
                offset: 0,
                frame_indices_offset: 0,
                x: 0.25,
                y: 0.75,
            }],
        );
        armature_data.add_animation(parent_data);
        armature_data.add_animation(AnimationData {
            name: "child".to_owned(),
            duration: 1.0,
            frame_count: 24,
            ..AnimationData::default()
        });
        data.add_armature(armature_data);
        data.finalize_root(IrArrays {
            timeline_array: vec![100, 0, 1, 0, 0, 0],
            frame_indices: vec![0],
            ..IrArrays::default()
        });

        let mut animation = Animation::new();
        animation.init(ArmatureIdx(0), Arc::new(data), "test");
        let parent_id = animation.play(Some("blend"), 1).expect("play parent");
        let child_id = animation
            .get_state("child", -1)
            .expect("animation timeline child state");
        (animation, parent_id, child_id)
    }

    fn animation() -> Animation {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            ..ArmatureData::default()
        };
        let animation_data = AnimationData {
            name: "idle".to_owned(),
            ..AnimationData::default()
        };
        armature_data.add_animation(animation_data);
        data.add_armature(armature_data);
        data.finalize_root(IrArrays::default());
        let mut animation = Animation::new();
        animation.init(ArmatureIdx(0), Arc::new(data), "test");
        animation
    }

    #[test]
    fn state_ids_are_independent_of_play_order_and_reuse_generation() {
        let mut animation = animation();
        let low = animation
            .insert_state(AnimationState {
                name: "low".to_owned(),
                layer: 0,
                ..Default::default()
            })
            .expect("insert low");
        let high = animation
            .insert_state(AnimationState {
                name: "high".to_owned(),
                layer: 2,
                ..Default::default()
            })
            .expect("insert high");

        assert_eq!(
            animation
                .states()
                .map(|(_, state)| state.name.as_str())
                .collect::<Vec<_>>(),
            ["high", "low"]
        );
        assert_eq!(animation.get_state("low", -1), Some(low));
        assert!(animation.remove_state(low).is_some());
        assert!(animation.state(high).is_some());

        let replacement = animation
            .insert_state(AnimationState {
                name: "replacement".to_owned(),
                ..Default::default()
            })
            .expect("reuse state slot");
        assert_eq!(replacement.slot(), low.slot());
        assert_ne!(replacement.generation(), low.generation());
        assert!(animation.state(low).is_none());
        assert!(animation.state(high).is_some());
    }

    #[test]
    fn play_config_normalizes_defaults_and_creates_stable_state() {
        let mut animation = animation();
        let state_id = animation.play(Some("idle"), -1).expect("play idle");
        let state = animation.state(state_id).expect("new state");

        assert_eq!(state.animation_name, "idle");
        assert_eq!(state.name, "idle");
        assert_eq!(
            state.play_times, 1,
            "zero-frame animation is forced to one play"
        );
        assert_eq!(
            state.time_scale, 1.0,
            "timeScale -100 normalizes to inverse data scale"
        );
        assert!(state.reset_to_pose);
        assert_eq!(animation.get_state("idle", -1), Some(state_id));
        assert!(animation.is_playing());
        assert!(!animation.is_completed());
    }

    #[test]
    fn caller_owned_play_config_receives_normalized_values() {
        let mut animation = animation();
        let mut config = crate::model::AnimationConfig {
            animation: "idle".to_owned(),
            fade_in_time: -1.0,
            fade_out_time: -1.0,
            play_times: -1,
            time_scale: -100.0,
            ..crate::model::AnimationConfig::default()
        };

        assert!(animation.play_config(&mut config).is_some());

        assert_eq!(
            config.fade_in_time, 0.0,
            "first state starts without a fade"
        );
        assert_eq!(
            config.fade_out_time, 0.0,
            "fade-out follows normalized fade-in"
        );
        assert_eq!(config.play_times, 1, "zero-frame data has one play");
        assert_eq!(
            config.time_scale, 1.0,
            "sentinel timeScale resolves from animation data"
        );
        assert_eq!(config.duration, -1.0);
    }

    #[test]
    fn first_fade_events_are_generated_in_state_order_and_child_request_is_deferred() {
        let mut animation = animation();
        let id = animation.play(Some("idle"), -1).expect("play");
        assert_eq!(animation.take_child_animation_requests(), ["idle"]);

        let events = animation.advance_time(0.0, None);
        assert_eq!(
            events,
            [(FadeEvent::FadeIn, true), (FadeEvent::FadeInComplete, true)]
        );
        assert_eq!(animation.state(id).unwrap().fade_state, FadeState::Complete);
        assert!(animation.take_child_animation_requests().is_empty());
    }

    #[test]
    fn single_reuse_returns_existing_state_without_normalizing_config() {
        let mut animation = animation();
        let existing = animation.play(Some("idle"), -1).expect("initial play");
        assert_eq!(animation.take_child_animation_requests(), ["idle"]);
        let mut config = crate::model::AnimationConfig {
            fade_out_mode: crate::core::enums::AnimationFadeOutMode::Single,
            animation: "idle".to_owned(),
            layer: 0,
            fade_in_time: -1.0,
            position: 0.75,
            ..crate::model::AnimationConfig::default()
        };

        assert_eq!(animation.play_config(&mut config), Some(existing));
        assert_eq!(config.fade_in_time, -1.0);
        assert_eq!(config.position, 0.75);
        assert!(animation.take_child_animation_requests().is_empty());
    }

    #[test]
    fn goto_and_stop_freezes_state_and_empty_animation_set_is_not_completed() {
        let mut animation = animation();
        assert!(!animation.is_completed());

        let state_id = animation
            .goto_and_stop_by_time("idle", 0.0)
            .expect("goto and stop");
        let before = animation.state(state_id).expect("state").time;
        animation.advance_time(0.5, None);
        assert_eq!(animation.state(state_id).expect("state").time, before);
    }

    #[test]
    fn play_without_name_resumes_then_replays_the_last_state() {
        let mut animation = animation();
        let first = animation.play(Some("idle"), -1).expect("play idle");
        animation.stop(Some("idle"));
        assert_eq!(
            animation.state(first).expect("first").playhead_state.bits(),
            0
        );

        assert_eq!(animation.play(None, -1), Some(first));
        assert_eq!(
            animation
                .state(first)
                .expect("resumed")
                .playhead_state
                .bits(),
            3
        );

        animation.state_mut(first).expect("first").action_play_state =
            crate::animation::timeline_state::TimelinePlayState::Complete;
        let replay = animation.play(None, -1).expect("replay last");
        assert_ne!(replay, first);
        assert_eq!(
            animation.state(replay).expect("replay").animation_name,
            "idle"
        );
    }

    #[test]
    fn get_state_returns_newest_matching_state_and_honors_layer_filter() {
        let mut animation = animation();
        let lower = animation
            .insert_state(AnimationState {
                name: "walk".to_owned(),
                layer: 0,
                ..AnimationState::default()
            })
            .expect("insert lower");
        let upper = animation
            .insert_state(AnimationState {
                name: "walk".to_owned(),
                layer: 2,
                ..AnimationState::default()
            })
            .expect("insert upper");
        let newest_upper = animation
            .insert_state(AnimationState {
                name: "walk".to_owned(),
                layer: 2,
                ..AnimationState::default()
            })
            .expect("insert newest upper");

        assert_eq!(animation.get_state("walk", -1), Some(lower));
        assert_eq!(animation.get_state("walk", 2), Some(newest_upper));
        assert_eq!(animation.get_state("walk", 0), Some(lower));
        assert_ne!(animation.get_state("walk", 2), Some(upper));
    }

    #[test]
    fn fade_in_uses_its_requested_layer_group_and_fade_range() {
        let mut animation = animation();
        let first = animation
            .fade_in_with_options(
                "idle",
                0.2,
                -1,
                1,
                Some("upper"),
                crate::core::enums::AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("fade in");
        assert_eq!(animation.state(first).expect("first").layer, 1);
        assert_eq!(animation.state(first).expect("first").group, "upper");

        let second = animation
            .fade_in_with_options(
                "idle",
                0.2,
                -1,
                2,
                Some("lower"),
                crate::core::enums::AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("second fade in");
        assert_eq!(
            animation.state(first).expect("first remains").fade_state,
            FadeState::FadeIn
        );
        assert_eq!(animation.state(second).expect("second").layer, 2);
    }

    #[test]
    fn reset_retains_owner_and_animation_data_binding_but_on_clear_drops_them() {
        let mut animation = animation();
        animation.time_scale = 2.0;
        animation.reset();
        assert_eq!(animation.armature, Some(ArmatureIdx(0)));
        assert!(animation.has_animation("idle"));
        assert_eq!(animation.time_scale, 2.0);

        animation.on_clear();
        assert_eq!(animation.armature, None);
        assert_eq!(animation.time_scale, 1.0);
        assert!(!animation.has_animation("idle"));
    }

    #[test]
    fn blend_state_lookup_is_stable_and_advance_resets_its_frame_budget() {
        let mut animation = animation();
        let key = BlendStateKey {
            kind: BlendStateKind::BoneTransform,
            target_name: "root".to_owned(),
        };

        assert!(
            animation
                .blend_states
                .entry(key.clone())
                .or_default()
                .update(2, 0.6)
        );
        assert_eq!(
            animation
                .blend_states
                .get(&key)
                .expect("blend state")
                .blend_weight,
            0.6
        );
        animation.advance_time(1.0 / 24.0, None);
        assert_eq!(
            animation.blend_states.get(&key).expect("blend state").dirty,
            0
        );
    }

    #[test]
    fn zero_weight_state_still_advances_its_action_timeline() {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            ..ArmatureData::default()
        };
        let animation_data = AnimationData {
            name: "walk".to_owned(),
            duration: 1.0,
            frame_count: 24,
            cache_frame_rate: 10.0,
            ..AnimationData::default()
        };
        armature_data.add_animation(animation_data);
        data.add_armature(armature_data);
        data.finalize_root(IrArrays::default());

        let data = Arc::new(data);
        let mut animation = Animation::new();
        animation.init(ArmatureIdx(0), Arc::clone(&data), "test");
        let state_id = animation.play(Some("walk"), 1).expect("play walk");
        animation
            .state_mut(state_id)
            .expect("state")
            .set_weight(0.0);

        let armature_data = data.armatures.get("test").expect("armature data");
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
        let mut frame_cache = FrameCache::default();
        frame_cache.initialize(armature_data, 10.0);
        let mut animation_context = AnimationUpdateCtx {
            armature_idx: ArmatureIdx(0),
            armature_data,
            bones: &mut bones,
            slots: &mut slots,
            constraints: &mut constraints,
            actions: &mut actions,
            cache_frame_index: &mut cache_frame_index,
            frame_cache: Some(&mut frame_cache),
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
        animation.advance_time_with_context(0.1, None, &mut animation_context, &mut runtime);

        let state = animation.state(state_id).expect("state remains active");
        assert_eq!(state.weight, 0.0);
        assert_eq!(
            state.action_play_state,
            crate::animation::timeline_state::TimelinePlayState::Play
        );
        assert_eq!(state.time, 0.1);

        let state = animation.state_mut(state_id).expect("state");
        state.set_weight(1.0);
        state.auto_fade_out_time = 0.0;
        animation.advance_time_with_context(0.9, None, &mut animation_context, &mut runtime);

        let state = animation.state(state_id).expect("state remains active");
        assert_eq!(
            state.action_play_state,
            crate::animation::timeline_state::TimelinePlayState::Complete
        );
        assert_eq!(state.fade_state, FadeState::FadeOut);
        assert_eq!(cache_frame_index, 10);
    }

    #[test]
    fn animation_progress_timeline_creates_a_stopped_parented_child_state() {
        let mut data = DragonBonesData::default();
        let mut armature_data = ArmatureData {
            name: "test".to_owned(),
            ..ArmatureData::default()
        };
        let mut parent_data = AnimationData {
            name: "blend".to_owned(),
            duration: 1.0,
            frame_count: 24,
            blend_type: crate::core::enums::AnimationBlendType::E1D,
            ..AnimationData::default()
        };
        parent_data.animation_timelines.insert(
            "child".to_owned(),
            vec![crate::model::TimelineData {
                timeline_type: TimelineType::AnimationProgress,
                offset: 0,
                frame_indices_offset: 0,
                x: 0.25,
                y: 0.75,
            }],
        );
        armature_data.add_animation(parent_data);
        armature_data.add_animation(AnimationData {
            name: "child".to_owned(),
            duration: 1.0,
            frame_count: 24,
            ..AnimationData::default()
        });
        data.add_armature(armature_data);
        data.finalize_root(IrArrays {
            timeline_array: vec![100, 0, 1, 0, 0, 0],
            frame_indices: vec![0],
            ..IrArrays::default()
        });

        let mut animation = Animation::new();
        animation.init(ArmatureIdx(0), Arc::new(data), "test");
        let parent_id = animation.play(Some("blend"), 1).expect("play parent");
        assert_eq!(animation.last_state(), Some(parent_id));
        let child_id = animation
            .get_state("child", -1)
            .expect("animation timeline child state");

        let parent = animation.state(parent_id).expect("parent state");
        assert!(!parent.reset_to_pose);
        assert_eq!(parent.animation_timelines.len(), 1);
        assert_eq!(
            parent.animation_timelines[0].target,
            TimelineTarget::AnimationState(child_id)
        );
        let child = animation.state(child_id).expect("child state");
        assert_eq!(child.parent, Some(parent_id));
        assert!(!child.action_enabled);
        assert!(!child.reset_to_pose);
        assert_eq!(child.playhead_state.bits(), 0);
        assert_eq!(child.position_x, 0.25);
        assert_eq!(child.position_y, 0.75);
        assert_eq!(child.weight, 0.0);

        animation.fade_out_states(&crate::model::AnimationConfig {
            fade_out_mode: AnimationFadeOutMode::All,
            fade_out_time: 0.5,
            ..crate::model::AnimationConfig::default()
        });
        assert!(animation.state(parent_id).unwrap().is_fade_out());
        assert!(animation.state(child_id).unwrap().is_fade_out());

        animation.remove_state(parent_id).expect("remove parent");
        let child = animation
            .state(child_id)
            .expect("child remains until advance");
        assert_eq!(child.parent, None);
        assert_eq!(
            child.sub_fade_state,
            crate::animation::animation_state::SubFadeState::Complete
        );
    }

    #[test]
    fn play_uses_all_fade_mode_while_fade_in_uses_layer_and_group_range() {
        // play() 的 clear() 默认 fadeOutMode=All：同名重播淡出所有顶层状态。
        let mut all = walk_animation();
        let a = all
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                0,
                Some("g"),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("state A");
        let b = all
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                1,
                Some("g"),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("state B");
        let replayed = all.play(Some("walk"), 1).expect("play All");
        assert!(all.state(a).expect("A").is_fade_out());
        assert!(all.state(b).expect("B").is_fade_out());
        assert_ne!(replayed, a);
        assert_ne!(replayed, b);

        // fadeIn() 的 SameLayerAndGroup 只淡出同 layer+group。
        let mut ranged = walk_animation();
        let lower = ranged
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                0,
                Some("g"),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("lower");
        let upper = ranged
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                1,
                Some("g"),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("upper");
        ranged
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                1,
                Some("g"),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("same layer/group replacement");
        assert!(!ranged.state(lower).expect("lower").is_fade_out());
        assert!(ranged.state(upper).expect("upper").is_fade_out());
    }

    #[test]
    fn fade_out_single_mode_is_an_empty_filter_branch() {
        let mut animation = walk_animation();
        let id = animation
            .fade_in_with_options(
                "walk",
                0.0,
                1,
                0,
                Some(""),
                AnimationFadeOutMode::SameLayerAndGroup,
            )
            .expect("state");
        animation.fade_out_states(&crate::model::AnimationConfig {
            fade_out_mode: AnimationFadeOutMode::Single,
            fade_out_time: 0.5,
            ..crate::model::AnimationConfig::default()
        });
        assert!(
            !animation.state(id).expect("state").is_fade_out(),
            "Animation.ts:153-155 Single is a no-op in _fadeOut"
        );
    }

    #[test]
    fn play_config_normalizes_negative_position_data_play_times_and_duration_truncation() {
        let mut animation = walk_animation();

        let mut reverse_position = crate::model::AnimationConfig {
            animation: "walk".to_owned(),
            position: -0.3,
            play_times: -1,
            time_scale: 1.0,
            ..crate::model::AnimationConfig::default()
        };
        assert!(animation.play_config(&mut reverse_position).is_some());
        // Animation.ts:380-382：duration - (position % duration) = 1 - (-0.3) = 1.3。
        assert!((reverse_position.position - 1.3).abs() < 1e-12);
        assert_eq!(reverse_position.play_times, 3, "Animation.ts:395-396");
        assert_eq!(reverse_position.duration, -1.0);

        let mut truncated = crate::model::AnimationConfig {
            animation: "walk".to_owned(),
            position: 0.6,
            duration: 0.8,
            play_times: -1,
            time_scale: 1.0,
            ..crate::model::AnimationConfig::default()
        };
        assert!(animation.play_config(&mut truncated).is_some());
        // Animation.ts:391-392：position + duration > data.duration → 截断。
        assert!((truncated.duration - 0.4).abs() < 1e-12);
    }

    #[test]
    fn reset_retains_blend_states_names_and_inherit_scale_but_on_clear_drops_them() {
        let mut animation = walk_animation();
        let key = BlendStateKey {
            kind: BlendStateKind::BoneTransform,
            target_name: "root".to_owned(),
        };
        animation.blend_states.entry(key.clone()).or_default();
        animation.inherit_time_scale = 0.25;

        animation.reset();
        assert!(animation.blend_states.contains_key(&key));
        assert_eq!(animation.animation_names(), ["walk"]);
        assert_eq!(animation.inherit_time_scale(), 0.25);

        animation.on_clear();
        assert!(animation.blend_states.is_empty());
        assert!(animation.animation_names().is_empty());
        assert_eq!(animation.inherit_time_scale(), 1.0);
    }

    #[test]
    fn single_state_removal_leaves_animation_dirty_and_cache_invalidation_untouched() {
        let mut animation = walk_animation();
        let id = animation.play(Some("walk"), 1).expect("play");
        {
            let state = animation.state_mut(id).expect("state");
            state.fade_state = FadeState::FadeOut;
            state.sub_fade_state = SubFadeState::Complete;
        }
        animation.animation_dirty = false;
        animation.cache_frame_invalidated = false;

        animation.advance_time(0.0, None);

        assert!(animation.state(id).is_none());
        assert!(
            !animation.animation_dirty,
            "Animation.ts:196-202 single-state recycle does not set _animationDirty"
        );
        assert!(!animation.cache_frame_invalidated);
    }

    #[test]
    fn multi_state_removal_marks_dirty_and_invalidates_cache() {
        let mut animation = walk_animation();
        let a = animation
            .insert_state(AnimationState {
                fade_state: FadeState::FadeOut,
                sub_fade_state: SubFadeState::Complete,
                ..AnimationState::default()
            })
            .expect("A");
        let b = animation
            .insert_state(AnimationState {
                fade_state: FadeState::FadeOut,
                sub_fade_state: SubFadeState::Complete,
                ..AnimationState::default()
            })
            .expect("B");
        animation.animation_dirty = false;
        animation.cache_frame_invalidated = false;

        animation.advance_time(0.0, None);

        assert!(animation.state(a).is_none());
        assert!(animation.state(b).is_none());
        assert!(
            animation.animation_dirty,
            "Animation.ts:233-239 multi-state recycle sets _animationDirty"
        );
        assert!(
            animation.cache_frame_invalidated,
            "Animation.ts:262 multi-state path forces _cacheFrameIndex = -1"
        );
    }

    fn advance_one_with_context(animation: &mut Animation) -> (i32, bool) {
        let data = animation.data.clone().expect("data");
        let host = data.armatures.get("test").expect("armature data");
        let mut bones = Vec::new();
        let mut slots = Vec::new();
        let mut constraints = Vec::new();
        let mut actions = Vec::new();
        let mut cache_frame_index = 7;
        let mut alpha_dirty = false;
        let mut slots_dirty = false;
        let mut z_order_dirty = false;
        let mut z_index_dirty = false;
        let mut proxy = NullArmatureProxy::new();
        let mut pending_events = std::collections::VecDeque::new();
        {
            let mut armatures = Vec::new();
            let mut clocks = ClockArena::default();
            let mut runtime = RuntimeCtx::new(
                &mut armatures,
                &mut clocks,
                &mut pending_events,
                Some(ArmatureIdx(0)),
                None,
            );
            let mut context = AnimationUpdateCtx {
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
            animation.advance_time_with_context(0.0, None, &mut context, &mut runtime);
        }
        (cache_frame_index, animation.animation_dirty)
    }

    #[test]
    fn context_removal_branches_keep_single_cache_but_reset_multi_cache() {
        let mut single = walk_animation();
        let id = single.play(Some("walk"), 1).expect("play");
        {
            let state = single.state_mut(id).expect("state");
            state.fade_state = FadeState::FadeOut;
            state.sub_fade_state = SubFadeState::Complete;
        }
        single.animation_dirty = false;
        let (cache_index, dirty) = advance_one_with_context(&mut single);
        assert!(single.state(id).is_none());
        assert_eq!(cache_index, 7, "single-state recycle leaves cache index");
        assert!(!dirty);

        let mut multi = walk_animation();
        multi
            .insert_state(AnimationState {
                fade_state: FadeState::FadeOut,
                sub_fade_state: SubFadeState::Complete,
                ..AnimationState::default()
            })
            .expect("A");
        multi
            .insert_state(AnimationState {
                fade_state: FadeState::FadeOut,
                sub_fade_state: SubFadeState::Complete,
                ..AnimationState::default()
            })
            .expect("B");
        multi.animation_dirty = false;
        let (cache_index, dirty) = advance_one_with_context(&mut multi);
        assert_eq!(cache_index, -1, "multi-state path forces cache index -1");
        assert!(dirty);
    }

    #[test]
    fn public_advance_time_skips_tail_cleanup_at_zero_weight() {
        let mut animation = walk_animation();
        let id = animation.play(Some("walk"), 1).expect("play");
        {
            let state = animation.state_mut(id).expect("state");
            state.set_weight(0.0);
            state.fade_state = FadeState::FadeIn;
            state.sub_fade_state = SubFadeState::Complete;
            state.action_play_state = TimelinePlayState::Complete;
            state.auto_fade_out_time = 0.0;
        }

        animation.advance_time(0.0, None);
        let state = animation.state(id).expect("state");
        assert_eq!(
            state.sub_fade_state,
            SubFadeState::Complete,
            "weight==0 returns before AnimationState.ts:1018-1069 tail"
        );
        assert_eq!(state.fade_state, FadeState::Complete);

        // 对照：weight>0 时同一帧会执行 tail（pose 清理 + 自动淡出）。
        animation.state_mut(id).expect("state").set_weight(1.0);
        animation.advance_time(0.0, None);
        let state = animation.state(id).expect("state");
        // tail 先复位 subFadeState，再由自动 fadeOut(0) 重开为 Start。
        assert_eq!(state.sub_fade_state, SubFadeState::Start);
        assert_eq!(state.fade_state, FadeState::FadeOut);
    }

    #[test]
    fn ignored_parent_fade_out_does_not_cascade_to_children() {
        let (mut animation, parent_id, child_id) = blend_animation();
        {
            let parent = animation.state_mut(parent_id).expect("parent");
            parent.fade_state = FadeState::FadeOut;
            parent.fade_total_time = 1.0;
            parent.fade_time = 0.9;
        }
        {
            let child = animation.state_mut(child_id).expect("child");
            child.fade_state = FadeState::Complete;
            child.sub_fade_state = SubFadeState::Fading;
        }

        // 0.5 > remaining 0.1 → AnimationState.ts:1120-1123 guard 忽略本次根淡出；
        // :1153-1158 的子状态 999999 级联不得发生。
        animation.fade_out_state_and_children(parent_id, 0.5, true);

        assert!(animation.state(parent_id).expect("parent").is_fade_out());
        assert!(
            animation.state(child_id).expect("child").is_fade_complete(),
            "child must stay untouched when the root guard ignores fade_out"
        );
    }

    fn run_complete_event(action_enabled: bool, parented: bool, has_listener: bool) -> Vec<String> {
        let mut animation = walk_animation();
        let id = animation.play(Some("walk"), 1).expect("play");
        {
            let state = animation.state_mut(id).expect("state");
            state.playhead_state = PlayheadState::PLAYING;
            state.fade_state = FadeState::Complete;
            state.sub_fade_state = SubFadeState::Fading;
            state.fade_progress = 1.0;
            state.action_enabled = action_enabled;
            if parented {
                state.parent = Some(id);
            }
        }
        let data = animation.data.clone().expect("data");
        let host = data.armatures.get("test").expect("armature data");
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
        if has_listener {
            proxy.add_db_event_listener(EventObject::COMPLETE, Box::new(|_| {}));
        }
        let mut pending_events = std::collections::VecDeque::new();
        {
            let mut armatures = Vec::new();
            let mut clocks = ClockArena::default();
            let mut runtime = RuntimeCtx::new(
                &mut armatures,
                &mut clocks,
                &mut pending_events,
                Some(ArmatureIdx(0)),
                None,
            );
            let mut context = AnimationUpdateCtx {
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
            animation.advance_time_with_context(1.0, None, &mut context, &mut runtime);
        }
        pending_events
            .into_iter()
            .map(|event| event.event_type)
            .collect()
    }

    #[test]
    fn complete_event_requires_root_action_enabled_and_listener() {
        assert_eq!(
            run_complete_event(true, false, true),
            [EventObject::COMPLETE]
        );
        assert!(run_complete_event(false, false, true).is_empty());
        assert!(run_complete_event(true, true, true).is_empty());
        assert!(run_complete_event(true, false, false).is_empty());
    }

    fn run_fade_out_events(action_enabled: bool, has_listener: bool) -> Vec<String> {
        let mut state = AnimationState {
            action_enabled,
            fade_progress: 0.5,
            fade_total_time: 1.0,
            playhead_state: PlayheadState::PLAYING,
            ..AnimationState::default()
        };
        assert!(state.fade_out(0.5, true));
        let armature_data = ArmatureData::default();
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
        if has_listener {
            proxy.add_db_event_listener(EventObject::FADE_OUT, Box::new(|_| {}));
            proxy.add_db_event_listener(EventObject::FADE_OUT_COMPLETE, Box::new(|_| {}));
        }
        let mut pending_events = std::collections::VecDeque::new();
        {
            let mut armatures = Vec::new();
            let mut clocks = ClockArena::default();
            let mut runtime = RuntimeCtx::new(
                &mut armatures,
                &mut clocks,
                &mut pending_events,
                Some(ArmatureIdx(0)),
                None,
            );
            let mut context = AnimationUpdateCtx {
                armature_idx: ArmatureIdx(0),
                armature_data: &armature_data,
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
            let mut blend_states = std::collections::HashMap::new();
            state.advance_time_with_context(
                1.0,
                1.0,
                &mut context,
                &mut runtime,
                0.0,
                &mut blend_states,
            );
        }
        pending_events
            .into_iter()
            .map(|event| event.event_type)
            .collect()
    }

    #[test]
    fn fade_out_events_are_buffered_only_with_root_action_enabled_and_listener() {
        assert_eq!(
            run_fade_out_events(true, true),
            [EventObject::FADE_OUT, EventObject::FADE_OUT_COMPLETE]
        );
        assert!(run_fade_out_events(false, true).is_empty());
        assert!(run_fade_out_events(true, false).is_empty());
    }

    #[test]
    fn reused_state_slot_starts_from_on_clear_defaults() {
        let mut animation = walk_animation();
        let first = animation.play(Some("walk"), 1).expect("first play");
        {
            let state = animation.state_mut(first).expect("first state");
            state.fade_state = FadeState::FadeOut;
            state.sub_fade_state = SubFadeState::Complete;
            state.fade_time = 9.0;
            state.fade_progress = 0.2;
            state.time = 0.7;
            state.playhead_state = PlayheadState::STOPPED;
        }
        animation.remove_state(first).expect("remove first");

        let second = animation.play(Some("walk"), 1).expect("second play");
        assert_eq!(second.slot(), first.slot(), "arena reuses the freed slot");
        assert_ne!(second.generation(), first.generation());
        let state = animation.state(second).expect("second state");
        // _onClear 默认值：fade 从 FadeIn/Start 开始，playhead 由 config 决定。
        assert_eq!(state.fade_state, FadeState::FadeIn);
        assert_eq!(state.sub_fade_state, SubFadeState::Start);
        assert_eq!(state.fade_time, 0.0);
        assert_eq!(state.playhead_state, PlayheadState::FADE_PAUSED);
        assert_eq!(state.time, 0.0);
        assert!((state.fade_progress - 0.999999).abs() < 1e-12);
    }

    #[test]
    fn cache_quantization_precedes_z_order_and_cache_index_uses_quantized_time() {
        let mut animation = walk_animation();
        let id = animation.play(Some("walk"), 1).expect("play");
        {
            let state = animation.state_mut(id).expect("state");
            state.fade_state = FadeState::Complete;
            state.sub_fade_state = SubFadeState::Fading;
            state.playhead_state = PlayheadState::PLAYING;
        }
        let data = animation.data.clone().expect("data");
        let host = data.armatures.get("test").expect("armature data");
        let source = animation
            .state(id)
            .expect("state")
            .action_timeline
            .as_ref()
            .expect("action timeline")
            .source
            .clone();
        let state = animation.state(id).expect("state");
        let z_order = TimelineInstance::new(
            TimelineType::ZOrder,
            TimelineTarget::Armature(ArmatureIdx(0)),
            None,
            None,
            source,
            state,
            host,
            None,
        )
        .expect("z-order timeline");
        animation.state_mut(id).expect("state").z_order_timeline = Some(z_order);

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
        let mut pending_events = std::collections::VecDeque::new();
        {
            let mut armatures = Vec::new();
            let mut clocks = ClockArena::default();
            let mut runtime = RuntimeCtx::new(
                &mut armatures,
                &mut clocks,
                &mut pending_events,
                Some(ArmatureIdx(0)),
                None,
            );
            let mut frame_cache = FrameCache::default();
            frame_cache.initialize(host, 10.0);
            let mut context = AnimationUpdateCtx {
                armature_idx: ArmatureIdx(0),
                armature_data: host,
                bones: &mut bones,
                slots: &mut slots,
                constraints: &mut constraints,
                actions: &mut actions,
                cache_frame_index: &mut cache_frame_index,
                frame_cache: Some(&mut frame_cache),
                alpha_dirty: &mut alpha_dirty,
                slots_dirty: &mut slots_dirty,
                z_order_dirty: &mut z_order_dirty,
                z_index_dirty: &mut z_index_dirty,
                proxy: &mut proxy,
            };
            // 0.13 在 cacheFrameRate=10 下量化为 floor(0.13*20)/20 = 0.1。
            animation.advance_time_with_context(0.13, None, &mut context, &mut runtime);
        }

        let state = animation.state(id).expect("state");
        let action_time = state
            .action_timeline
            .as_ref()
            .expect("action")
            .base
            .current_time;
        let z_order_time = state
            .z_order_timeline
            .as_ref()
            .expect("z-order")
            .base
            .current_time;
        assert!(
            (action_time - 0.1).abs() < 1e-12,
            "action time is quantized"
        );
        assert!(
            (z_order_time - action_time).abs() < 1e-12,
            "zOrder must read the already-quantized action time (AnimationState.ts:850-857)"
        );
        assert_eq!(cache_frame_index, 1);
    }
}
