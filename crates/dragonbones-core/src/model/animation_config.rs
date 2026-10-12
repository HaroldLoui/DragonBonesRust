//! 对齐 `model/AnimationConfig.ts`。

use crate::core::enums::{AnimationFadeOutMode, TweenType};

/// - 动画配置（`AnimationConfig.ts`）。
///
/// [`Default`] 严格照 `AnimationConfig._onClear` 逐行翻译（含 `time_scale = -100.0`
/// 等反直觉默认值，见 `docs/guide/01-stage1.md` F1.4）。
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationConfig {
    pub pause_fade_out: bool,
    pub fade_out_mode: AnimationFadeOutMode,
    pub fade_out_tween_type: TweenType,
    pub fade_out_time: f64,

    pub pause_fade_in: bool,
    pub action_enabled: bool,
    pub additive: bool,
    pub display_control: bool,
    pub reset_to_pose: bool,
    pub fade_in_tween_type: TweenType,
    pub play_times: i32,
    pub layer: i32,
    pub position: f64,
    pub duration: f64,
    pub time_scale: f64,
    pub weight: f64,
    pub fade_in_time: f64,
    pub auto_fade_out_time: f64,
    pub name: String,
    pub animation: String,
    pub group: String,
    pub bone_mask: Vec<String>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            pause_fade_out: true,
            fade_out_mode: AnimationFadeOutMode::All,
            fade_out_tween_type: TweenType::Line,
            fade_out_time: -1.0,

            pause_fade_in: true,
            action_enabled: true,
            additive: false,
            display_control: true,
            reset_to_pose: true,
            fade_in_tween_type: TweenType::Line,
            play_times: -1,
            layer: 0,
            position: 0.0,
            duration: -1.0,
            time_scale: -100.0,
            weight: 1.0,
            fade_in_time: -1.0,
            auto_fade_out_time: -1.0,
            name: String::new(),
            animation: String::new(),
            group: String::new(),
            bone_mask: Vec::new(),
        }
    }
}

impl AnimationConfig {
    /// 对齐 `AnimationConfig.clear`（即 `_onClear`）。
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// 对齐 `AnimationConfig.copyFrom`。
    pub fn copy_from(&mut self, value: &AnimationConfig) {
        self.pause_fade_out = value.pause_fade_out;
        self.fade_out_mode = value.fade_out_mode;
        self.auto_fade_out_time = value.auto_fade_out_time;
        self.fade_out_tween_type = value.fade_out_tween_type;

        self.action_enabled = value.action_enabled;
        self.additive = value.additive;
        self.display_control = value.display_control;
        self.pause_fade_in = value.pause_fade_in;
        self.reset_to_pose = value.reset_to_pose;
        self.play_times = value.play_times;
        self.layer = value.layer;
        self.position = value.position;
        self.duration = value.duration;
        self.time_scale = value.time_scale;
        self.fade_in_time = value.fade_in_time;
        self.fade_out_time = value.fade_out_time;
        self.fade_in_tween_type = value.fade_in_tween_type;
        self.weight = value.weight;
        self.name.clone_from(&value.name);
        self.animation.clone_from(&value.animation);
        self.group.clone_from(&value.group);

        self.bone_mask.clone_from(&value.bone_mask);
    }
}
