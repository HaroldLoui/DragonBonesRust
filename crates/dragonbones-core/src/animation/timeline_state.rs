//! Shared timeline time/frame/tween algorithms from `BaseTimelineState.ts`.

use crate::core::enums::TweenType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i8)]
pub enum TimelinePlayState {
    Start = -1,
    Play = 0,
    Complete = 1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActionTimelineTime {
    pub play_state: TimelinePlayState,
    pub current_play_times: i32,
    pub current_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineTimeInput {
    pub passed_time: f64,
    pub action_time: Option<ActionTimelineTime>,
    pub animation_play_times: i32,
    pub animation_playhead_state: u8,
    pub animation_duration: f64,
}

/// Time/frame state shared by action, tween, and concrete timeline axes.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseTimelineState {
    pub dirty: bool,
    pub play_state: TimelinePlayState,
    pub current_play_times: i32,
    pub current_time: f64,
    pub frame_index: i32,
    pub frame_count: i32,
    pub position: f64,
    pub duration: f64,
    pub frame_value_offset: i32,
    pub time_scale: f64,
    pub time_offset: f64,
    pub frame_rate: f64,
    pub frame_rate_inverse: f64,
    pub frame_offset: i32,
    pub is_tween: bool,
}

impl Default for BaseTimelineState {
    fn default() -> Self {
        Self {
            dirty: false,
            play_state: TimelinePlayState::Start,
            current_play_times: 0,
            current_time: -1.0,
            frame_index: -1,
            frame_count: 0,
            position: 0.0,
            duration: 0.0,
            frame_value_offset: 0,
            time_scale: 1.0,
            time_offset: 0.0,
            frame_rate: 0.0,
            frame_rate_inverse: 0.0,
            frame_offset: 0,
            is_tween: false,
        }
    }
}

impl BaseTimelineState {
    /// Updates this timeline's local time, matching `_setCurrentTime` branch order.
    pub fn set_current_time(&mut self, input: TimelineTimeInput) -> bool {
        let previous_state = self.play_state;
        let previous_play_times = self.current_play_times;
        let previous_time = self.current_time;

        if let Some(action_time) = input.action_time
            && self.frame_count <= 1
        {
            self.play_state = if action_time.play_state >= TimelinePlayState::Play {
                TimelinePlayState::Complete
            } else {
                TimelinePlayState::Start
            };
            self.current_play_times = 1;
            self.current_time = action_time.current_time;
        } else if input.action_time.is_none() || self.time_scale != 1.0 || self.time_offset != 0.0 {
            let mut passed_time = input.passed_time;
            let total_time = f64::from(input.animation_play_times) * self.duration;

            passed_time *= self.time_scale;
            if self.time_offset != 0.0 {
                passed_time += self.time_offset * input.animation_duration;
            }

            if input.animation_play_times > 0
                && (passed_time >= total_time || passed_time <= -total_time)
            {
                if self.play_state <= TimelinePlayState::Play && input.animation_playhead_state == 3
                {
                    self.play_state = TimelinePlayState::Complete;
                }

                self.current_play_times = input.animation_play_times;
                if passed_time < 0.0 {
                    self.current_time = 0.0;
                } else if self.play_state == TimelinePlayState::Complete {
                    self.current_time = self.duration + 0.000001;
                } else {
                    self.current_time = self.duration;
                }
            } else {
                if self.play_state != TimelinePlayState::Play && input.animation_playhead_state == 3
                {
                    self.play_state = TimelinePlayState::Play;
                }

                if passed_time < 0.0 {
                    passed_time = -passed_time;
                    self.current_play_times = (passed_time / self.duration).floor() as i32;
                    self.current_time = self.duration - (passed_time % self.duration);
                } else {
                    self.current_play_times = (passed_time / self.duration).floor() as i32;
                    self.current_time = passed_time % self.duration;
                }
            }

            self.current_time += self.position;
        } else if let Some(action_time) = input.action_time {
            self.play_state = action_time.play_state;
            self.current_play_times = action_time.current_play_times;
            self.current_time = action_time.current_time;
        }

        if self.current_play_times == previous_play_times && self.current_time == previous_time {
            return false;
        }

        // JS `prevState < 0`（0 = Play）：从 Start 进入 Play 时清空帧游标，
        // 使下一次 update 重新 `_onArriveAtFrame`。阶段 4 误写成 `< Start`
        // （恒 false），淡入暂停后的首帧会漏掉 ZOrder 等“到达帧”副作用。
        if (previous_state < TimelinePlayState::Play && self.play_state != previous_state)
            || (self.play_state <= TimelinePlayState::Play
                && self.current_play_times != previous_play_times)
        {
            self.frame_index = -1;
        }
        true
    }

    /// Resolves the flat frame-index table using the official O(1) expression.
    #[must_use]
    pub fn lookup_frame_index(
        current_time: f64,
        frame_rate: f64,
        frame_indices_offset: usize,
        frame_indices: &[i32],
    ) -> Option<i32> {
        let frame_position = (current_time * frame_rate).floor();
        if !frame_position.is_finite() || frame_position < 0.0 {
            return None;
        }
        let frame_position = usize::try_from(frame_position as u64).ok()?;
        frame_indices
            .get(frame_indices_offset.checked_add(frame_position)?)
            .copied()
    }
}

/// Tween header decoded when a timeline arrives at a key frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TweenFrameParameters {
    pub tween_type: TweenType,
    pub curve_count: i32,
    pub frame_position: f64,
    pub frame_duration_inverse: f64,
    pub tween_easing: f64,
    pub is_tween: bool,
    pub dirty: bool,
}

impl TweenFrameParameters {
    /// Reads the frame header and determines whether the frame may tween.
    /// `next_frame_offset` is None for the last key frame.
    #[allow(clippy::too_many_arguments)]
    pub fn arrive_at_frame(
        frame_array: &[i32],
        frame_offset: usize,
        next_frame_offset: Option<usize>,
        frame_index: i32,
        frame_count: i32,
        animation_play_times: i32,
        current_play_times: i32,
        animation_duration: f64,
        frame_rate_inverse: f64,
    ) -> Option<Self> {
        let raw_tween_type = *frame_array.get(frame_offset + 1)?;
        let tween_type = match raw_tween_type {
            0 => TweenType::None,
            1 => TweenType::Line,
            2 => TweenType::Curve,
            3 => TweenType::QuadIn,
            4 => TweenType::QuadOut,
            5 => TweenType::QuadInOut,
            _ => TweenType::None,
        };
        let is_last_frame = frame_index == frame_count - 1;
        let can_tween = frame_count > 1
            && (!is_last_frame
                || animation_play_times == 0
                || current_play_times < animation_play_times - 1);
        let frame_position = f64::from(*frame_array.get(frame_offset)?) * frame_rate_inverse;

        if !can_tween {
            return Some(Self {
                tween_type: TweenType::None,
                curve_count: 0,
                frame_position,
                frame_duration_inverse: 0.0,
                tween_easing: 0.0,
                is_tween: false,
                dirty: true,
            });
        }

        let is_tween = tween_type != TweenType::None;
        let header_value = if tween_type == TweenType::Curve
            || (tween_type != TweenType::None && tween_type != TweenType::Line)
        {
            *frame_array.get(frame_offset + 2)?
        } else {
            0
        };
        let curve_count = if tween_type == TweenType::Curve {
            header_value
        } else {
            0
        };
        let tween_easing = if tween_type != TweenType::None
            && tween_type != TweenType::Line
            && tween_type != TweenType::Curve
        {
            f64::from(header_value) * 0.01
        } else {
            0.0
        };

        let frame_duration_inverse = if is_last_frame {
            1.0 / (animation_duration - frame_position)
        } else {
            let next_frame_offset = next_frame_offset?;
            let next_position =
                f64::from(*frame_array.get(next_frame_offset)?) * frame_rate_inverse;
            let frame_duration = next_position - frame_position;
            if frame_duration > 0.0 {
                1.0 / frame_duration
            } else {
                0.0
            }
        };

        Some(Self {
            tween_type,
            curve_count,
            frame_position,
            frame_duration_inverse,
            tween_easing,
            is_tween,
            dirty: !is_tween,
        })
    }

    /// Computes the interpolated tween progress using the pre-sampled curve table.
    #[must_use]
    pub fn progress(
        self,
        current_time: f64,
        frame_array: &[i32],
        curve_samples_offset: usize,
    ) -> Option<f64> {
        if !self.is_tween {
            return None;
        }

        let progress = (current_time - self.frame_position) * self.frame_duration_inverse;
        match self.tween_type {
            TweenType::Curve => Some(BaseTimelineState::easing_curve_value(
                progress,
                frame_array,
                self.curve_count,
                curve_samples_offset,
            )),
            TweenType::Line | TweenType::None => Some(progress),
            _ => Some(BaseTimelineState::easing_value(
                self.tween_type,
                progress,
                self.tween_easing,
            )),
        }
    }
}

impl BaseTimelineState {
    /// Matches `TweenTimelineState._getEasingValue` arithmetic and branch order.
    #[must_use]
    pub fn easing_value(tween_type: TweenType, progress: f64, easing: f64) -> f64 {
        let value = match tween_type {
            TweenType::QuadIn => progress.powf(2.0),
            TweenType::QuadOut => 1.0 - (1.0 - progress).powf(2.0),
            TweenType::QuadInOut => 0.5 * (1.0 - (progress * std::f64::consts::PI).cos()),
            _ => progress,
        };
        (value - progress) * easing + progress
    }

    /// Interpolates parser-sampled Bezier values; it does not resample a curve.
    #[must_use]
    pub fn easing_curve_value(progress: f64, samples: &[i32], count: i32, offset: usize) -> f64 {
        if progress <= 0.0 {
            return 0.0;
        }
        if progress >= 1.0 {
            return 1.0;
        }
        if !progress.is_finite() {
            return f64::NAN;
        }

        let is_omitted = count > 0;
        let segment_count = count + 1;
        let value_index = (progress * f64::from(segment_count)).floor() as i64;
        let read_sample = |relative_index: i64| {
            i128::try_from(offset)
                .ok()
                .and_then(|base| base.checked_add(i128::from(relative_index)))
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| samples.get(index))
                .copied()
                .map_or(f64::NAN, f64::from)
        };
        let (from_value, to_value) = if is_omitted {
            let from = if value_index == 0 {
                0.0
            } else {
                read_sample(value_index - 1)
            };
            let to = if value_index == i64::from(segment_count - 1) {
                10000.0
            } else {
                read_sample(value_index)
            };
            (from, to)
        } else {
            // For count <= 0 TS intentionally reads offset + valueIndex - 1,
            // including a possible negative relative index. Keep that branch.
            let from = read_sample(value_index - 1);
            let to = read_sample(value_index);
            (from, to)
        };

        (from_value
            + (to_value - from_value) * (progress * f64::from(segment_count) - value_index as f64))
            * 0.0001
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionTimelineTime, BaseTimelineState, TimelinePlayState, TimelineTimeInput,
        TweenFrameParameters,
    };
    use crate::core::enums::TweenType;

    // Value-family contract (TS `SingleValueTimelineState` /
    // `DoubleValueTimelineState` / `MutilpleValueTimelineState`):
    // `_current*` is only refreshed on tween arrivals (`BaseTimelineState.ts:390-397`,
    // `:447-468`, `:516-524`), while non-tween arrivals write `_result*` only.
    // The Rust concrete reader is `timeline_states::TimelineInstance`
    // (`current_values` / `differences` / `results`); IK consumes the Double
    // semantics at `TimelineState.ts:971-972`.

    fn input(passed_time: f64) -> TimelineTimeInput {
        TimelineTimeInput {
            passed_time,
            action_time: None,
            animation_play_times: 0,
            animation_playhead_state: 3,
            animation_duration: 2.0,
        }
    }

    #[test]
    fn action_time_is_copied_for_ordinary_multiframe_timelines() {
        let mut state = BaseTimelineState {
            frame_count: 4,
            duration: 2.0,
            ..Default::default()
        };
        let mut frame = input(0.5);
        frame.action_time = Some(ActionTimelineTime {
            play_state: TimelinePlayState::Play,
            current_play_times: 2,
            current_time: 1.25,
        });

        assert!(state.set_current_time(frame));
        assert_eq!(state.play_state, TimelinePlayState::Play);
        assert_eq!(state.current_play_times, 2);
        assert_eq!(state.current_time, 1.25);
    }

    #[test]
    fn one_frame_timeline_copies_action_time_but_normalizes_its_play_state() {
        let mut state = BaseTimelineState {
            frame_count: 1,
            ..Default::default()
        };
        let mut frame = input(0.0);
        frame.action_time = Some(ActionTimelineTime {
            play_state: TimelinePlayState::Play,
            current_play_times: 7,
            current_time: 0.75,
        });

        assert!(state.set_current_time(frame));
        assert_eq!(state.play_state, TimelinePlayState::Complete);
        assert_eq!(state.current_play_times, 1);
        assert_eq!(state.current_time, 0.75);
    }

    #[test]
    fn local_scale_offset_and_negative_integer_loop_boundaries_match_ts() {
        let mut state = BaseTimelineState {
            duration: 2.0,
            time_scale: 2.0,
            time_offset: 0.25,
            position: 0.5,
            ..Default::default()
        };
        let frame = input(-1.0);
        assert!(state.set_current_time(frame));
        // (-1 * 2) + (0.25 * 2) = -1.5; floor(1.5 / 2) = 0,
        // current = 2 - (1.5 % 2) = 0.5, then add position.
        assert_eq!(state.current_time, 1.0);
        assert_eq!(state.current_play_times, 0);

        state.time_offset = 0.0;
        state.position = 0.0;
        assert!(state.set_current_time(input(-1.0)));
        assert_eq!(state.current_time, 2.0); // reverse exact loop boundary
        assert_eq!(state.current_play_times, 1);
    }

    #[test]
    fn finite_completion_preserves_precision_epsilon_and_playhead_gate() {
        let mut state = BaseTimelineState {
            duration: 1.0,
            ..Default::default()
        };
        let mut frame = input(1.0);
        frame.animation_play_times = 1;
        frame.animation_duration = 1.0;

        assert!(state.set_current_time(frame));
        assert_eq!(state.play_state, TimelinePlayState::Complete);
        assert_eq!(state.current_time, 1.000001);

        let mut paused = BaseTimelineState {
            duration: 1.0,
            ..Default::default()
        };
        frame.animation_playhead_state = 2;
        assert!(paused.set_current_time(frame));
        assert_eq!(paused.play_state, TimelinePlayState::Start);
        assert_eq!(paused.current_time, 1.0);
    }

    #[test]
    fn frame_lookup_is_constant_offset_indexing() {
        let indices = [9, 8, 7, 6, 5];
        assert_eq!(
            BaseTimelineState::lookup_frame_index(0.5, 4.0, 1, &indices),
            Some(6)
        );
        assert_eq!(
            BaseTimelineState::lookup_frame_index(-0.1, 4.0, 0, &indices),
            None
        );
        assert_eq!(
            BaseTimelineState::lookup_frame_index(5.0, 4.0, 0, &indices),
            None
        );
    }

    #[test]
    fn easing_and_sampled_curve_interpolation_match_formulas() {
        assert_eq!(
            BaseTimelineState::easing_value(TweenType::QuadIn, 0.5, 1.0),
            0.25
        );
        assert_eq!(
            BaseTimelineState::easing_value(TweenType::QuadOut, 0.5, 1.0),
            0.75
        );
        assert!(
            (BaseTimelineState::easing_value(TweenType::QuadInOut, 0.5, 1.0) - 0.5).abs() < 1e-15
        );
        assert_eq!(BaseTimelineState::easing_curve_value(0.0, &[], 0, 0), 0.0);
        assert_eq!(BaseTimelineState::easing_curve_value(1.0, &[], 0, 0), 1.0);
        assert_eq!(
            BaseTimelineState::easing_curve_value(0.5, &[5000], 1, 0),
            0.5
        );
        // TS count<=0 path uses offset + valueIndex - 1 (can read before offset).
        assert_eq!(
            BaseTimelineState::easing_curve_value(0.5, &[6000, 8000], -1, 1),
            0.6
        );
    }

    #[test]
    fn finite_last_frame_does_not_tween_before_final_play() {
        let frame_array = [0, 1, 0, 24, 1, 0];
        let last = TweenFrameParameters::arrive_at_frame(
            &frame_array,
            3,
            None,
            1,
            2,
            1,
            0,
            1.0,
            1.0 / 24.0,
        )
        .expect("frame header");
        assert!(!last.is_tween);
        assert!(last.dirty);

        let looping_last = TweenFrameParameters::arrive_at_frame(
            &frame_array,
            3,
            None,
            1,
            2,
            0,
            0,
            1.0,
            1.0 / 24.0,
        )
        .expect("looping frame header");
        assert!(looping_last.is_tween);
    }

    #[test]
    fn looping_last_line_frame_does_not_require_an_easing_header() {
        let frame =
            TweenFrameParameters::arrive_at_frame(&[12, 1], 0, None, 1, 2, 0, 0, 1.0, 1.0 / 24.0)
                .expect("line tween header only stores position and type");

        assert_eq!(frame.tween_type, TweenType::Line);
        assert!(frame.is_tween);
        assert_eq!(frame.frame_position, 0.5);
        assert_eq!(frame.frame_duration_inverse, 2.0);
    }
}
