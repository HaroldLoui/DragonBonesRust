//! Runtime animation commands for built armatures.
//!
//! The adapter builds armatures from ECS data, but animation playback is a
//! runtime concern: switching clips, stopping, pausing, changing speed. Instead
//! of exposing the non-send [`crate::DragonBonesRuntime`] factory to game code,
//! the adapter accepts [`DragonBonesCommand`] messages that target an armature
//! root entity:
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use dragonbones_bevy::prelude::*;
//!
//! fn switch(mut commands: MessageWriter<DragonBonesCommand>, root: Entity) {
//!     commands.write(DragonBonesCommand::play(root, "walk"));
//! }
//! ```
//!
//! Commands are applied at the start of [`crate::DragonBonesSystems::Advance`],
//! before the frame's `advance_time`, so a `Play` starts animating in the same
//! frame it was requested.

use bevy::prelude::*;
use dragonbones_core::animation::StateId;
use dragonbones_core::armature::Armature;
use dragonbones_core::model::AnimationConfig;

use crate::runtime::{DragonBonesInstance, DragonBonesRuntime};

/// Configuration for starting an animation with fade/seek/speed options.
///
/// Mirrors the subset of core [`AnimationConfig`] that the adapter exposes;
/// unset behaviour (fade curves, layers, bone masks, …) keeps core defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct DragonBonesPlayConfig {
    /// Animation name in the skeleton data.
    pub animation: String,
    /// Play count: `-1` loops forever (core also falls back to the animation's
    /// authored play count when negative).
    pub play_times: i32,
    /// Fade-in duration in seconds; `0.0` starts immediately.
    pub fade_in_time: f64,
    /// Playback speed multiplier (`1.0` = authored speed).
    pub time_scale: f64,
    /// Start position inside the animation, in seconds; `0.0` starts at the
    /// beginning.
    pub position: f64,
}

impl DragonBonesPlayConfig {
    /// A looping playback starting at position 0 with no fade.
    #[must_use]
    pub fn new(animation: impl Into<String>) -> Self {
        Self {
            animation: animation.into(),
            play_times: -1,
            fade_in_time: 0.0,
            time_scale: 1.0,
            position: 0.0,
        }
    }

    #[must_use]
    pub fn with_play_times(mut self, play_times: i32) -> Self {
        self.play_times = play_times;
        self
    }

    #[must_use]
    pub fn with_fade_in_time(mut self, seconds: f64) -> Self {
        self.fade_in_time = seconds;
        self
    }

    #[must_use]
    pub fn with_time_scale(mut self, time_scale: f64) -> Self {
        self.time_scale = time_scale;
        self
    }

    #[must_use]
    pub fn with_position(mut self, seconds: f64) -> Self {
        self.position = seconds;
        self
    }
}

/// What to do to the target armature's animation state.
#[derive(Clone, Debug, PartialEq)]
pub enum DragonBonesAction {
    /// Play a named animation (`None` = the data's default animation), with
    /// `play_times` (`-1` loops forever).
    Play {
        animation: Option<String>,
        play_times: i32,
    },
    /// Play with fade/seek/speed options.
    PlayConfig(DragonBonesPlayConfig),
    /// Stop every animation on this armature.
    Stop,
    /// Freeze the current animation states (`time_scale = 0`). Plays started
    /// while paused are not affected.
    Pause,
    /// Resume at normal speed (`time_scale = 1.0`); use `SetSpeed` afterwards
    /// for a different speed.
    Resume,
    /// Set the playback speed of every active animation state.
    SetSpeed(f64),
}

/// One animation command addressed to an armature root entity.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DragonBonesCommand {
    /// The entity carrying [`crate::DragonBonesArmature`] / [`DragonBonesInstance`].
    pub armature: Entity,
    pub action: DragonBonesAction,
}

impl DragonBonesCommand {
    /// Plays `animation` (looping) on `armature`.
    #[must_use]
    pub fn play(armature: Entity, animation: impl Into<String>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Play {
                animation: Some(animation.into()),
                play_times: -1,
            },
        }
    }

    /// Plays with explicit play count (`-1` loops forever).
    #[must_use]
    pub fn play_times(armature: Entity, animation: impl Into<String>, play_times: i32) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Play {
                animation: Some(animation.into()),
                play_times,
            },
        }
    }

    /// Plays with fade/seek/speed options.
    #[must_use]
    pub fn play_config(armature: Entity, config: DragonBonesPlayConfig) -> Self {
        Self {
            armature,
            action: DragonBonesAction::PlayConfig(config),
        }
    }

    #[must_use]
    pub fn stop(armature: Entity) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Stop,
        }
    }

    #[must_use]
    pub fn pause(armature: Entity) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Pause,
        }
    }

    #[must_use]
    pub fn resume(armature: Entity) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Resume,
        }
    }

    /// Sets the playback speed (`1.0` = normal).
    #[must_use]
    pub fn set_speed(armature: Entity, speed: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetSpeed(speed),
        }
    }
}

fn set_state_time_scale(armature: &mut Armature, time_scale: f64) {
    let states: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
    for id in states {
        if let Some(state) = armature.animation.state_mut(id) {
            state.time_scale = time_scale;
        }
    }
}

fn apply(armature: &mut Armature, action: &DragonBonesAction) {
    match action {
        DragonBonesAction::Play {
            animation,
            play_times,
        } => {
            armature.animation.play(animation.as_deref(), *play_times);
        }
        DragonBonesAction::PlayConfig(config) => {
            let mut animation_config = AnimationConfig {
                animation: config.animation.clone(),
                play_times: config.play_times,
                fade_in_time: config.fade_in_time,
                time_scale: config.time_scale,
                position: config.position,
                ..AnimationConfig::default()
            };
            armature.animation.play_config(&mut animation_config);
        }
        DragonBonesAction::Stop => armature.animation.stop(None),
        DragonBonesAction::Pause => set_state_time_scale(armature, 0.0),
        DragonBonesAction::Resume => set_state_time_scale(armature, 1.0),
        DragonBonesAction::SetSpeed(speed) => set_state_time_scale(armature, *speed),
    }
}

/// Applies queued [`DragonBonesCommand`] messages before the frame's advance.
pub(crate) fn apply_dragonbones_commands(
    mut commands: MessageReader<DragonBonesCommand>,
    instances: Query<&DragonBonesInstance>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
) {
    for command in commands.read() {
        let Ok(instance) = instances.get(command.armature) else {
            warn!(
                "DragonBones command ignored: entity {:?} has no DragonBonesInstance",
                command.armature
            );
            continue;
        };
        let Some(armature) = runtime
            .factory
            .armatures
            .get_mut(instance.armature.index())
            .and_then(Option::as_mut)
        else {
            warn!(
                "DragonBones command ignored: armature {:?} is not built",
                instance.armature
            );
            continue;
        };
        apply(armature, &command.action);
    }
}
