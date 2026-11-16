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
use dragonbones_core::core::enums::AnimationFadeOutMode;
use dragonbones_core::geom::ColorTransform;
use dragonbones_core::model::AnimationConfig;

use crate::asset::AtlasAsset;
use crate::runtime::{DragonBonesInstance, DragonBonesPlaybackState, DragonBonesRuntime};

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
    /// Playback speed multiplier (`1.0` = authored speed). `None` inherits the
    /// armature's [`DragonBonesPlaybackState::time_scale`], so a speed set by
    /// `SetSpeed` is not silently reset by a later `PlayConfig`.
    pub time_scale: Option<f64>,
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
            time_scale: None,
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
        self.time_scale = Some(time_scale);
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
    /// Freeze the current animation states (`time_scale = 0`); the armature-level
    /// playback state remembers the desired speed, and clips started while paused
    /// stay frozen until [`Self::Resume`].
    Pause,
    /// Unpause and restore the remembered armature speed (`1.0` unless changed by
    /// [`Self::SetSpeed`] / [`Self::PlayConfig`]); use `SetSpeed` afterwards for a
    /// different speed.
    Resume,
    /// Set the playback speed of every active animation state.
    SetSpeed(f64),
    /// Move the current animation's playhead to `position` seconds without
    /// restarting it. A paused armature stays paused.
    Seek { position: f64 },
    /// Move the current animation's playhead to `fraction` (0..=1) of its
    /// duration.
    SeekProgress { fraction: f64 },
    /// Set the play count of every active state (`-1` loops forever).
    SetLoop { play_times: i32 },
    /// Set the animation layer of every active state.
    SetLayer { layer: i32 },
    /// Set the animation group of every active state.
    SetGroup { group: String },
    /// Fade in a named animation.
    FadeIn { animation: String, duration: f64 },
    /// Fade out every active state.
    FadeOut { duration: f64 },
    /// Set a slot's color multiplier/offset (`ColorTransform`).
    SetSlotTint { slot: String, color: ColorTransform },
    /// Show or hide one slot.
    SetSlotVisible { slot: String, visible: bool },
    /// Set the armature-wide alpha multiplier.
    SetArmatureAlpha(f64),
    /// Flip the armature horizontally and/or vertically.
    SetFlip {
        flip_x: Option<bool>,
        flip_y: Option<bool>,
    },
    /// Replace one atlas page's parsing data and image at runtime.
    ReplaceAtlas {
        page: usize,
        atlas: Handle<crate::asset::AtlasAsset>,
    },
    /// Replace the armature's skin with another skin from the same skeleton data
    /// (A1: same atlas only; a skin with armature displays is rejected).
    ReplaceSkin { skin: String },
    /// Replace one slot's display with a named display from the default skin, or
    /// clear it with `None`.
    ReplaceDisplay {
        slot: String,
        display: Option<String>,
    },
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

    /// Seeks the current animation to `position` seconds without restarting it.
    #[must_use]
    pub fn seek(armature: Entity, position: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::Seek { position },
        }
    }

    /// Seeks the current animation to `fraction` (0..=1) of its duration.
    #[must_use]
    pub fn seek_progress(armature: Entity, fraction: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SeekProgress { fraction },
        }
    }

    /// Sets the play count of the active states (`-1` loops forever).
    #[must_use]
    pub fn set_loop(armature: Entity, play_times: i32) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetLoop { play_times },
        }
    }

    /// Sets the animation layer of the active states.
    #[must_use]
    pub fn set_layer(armature: Entity, layer: i32) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetLayer { layer },
        }
    }

    /// Sets the animation group of the active states.
    #[must_use]
    pub fn set_group(armature: Entity, group: impl Into<String>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetGroup {
                group: group.into(),
            },
        }
    }

    /// Fades in `animation` over `duration` seconds.
    #[must_use]
    pub fn fade_in(armature: Entity, animation: impl Into<String>, duration: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::FadeIn {
                animation: animation.into(),
                duration,
            },
        }
    }

    /// Fades out every active state over `duration` seconds.
    #[must_use]
    pub fn fade_out(armature: Entity, duration: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::FadeOut { duration },
        }
    }

    /// Sets one slot's color multiplier/offset.
    #[must_use]
    pub fn set_slot_tint(armature: Entity, slot: impl Into<String>, color: ColorTransform) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetSlotTint {
                slot: slot.into(),
                color,
            },
        }
    }

    /// Shows or hides one slot.
    #[must_use]
    pub fn set_slot_visible(armature: Entity, slot: impl Into<String>, visible: bool) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetSlotVisible {
                slot: slot.into(),
                visible,
            },
        }
    }

    /// Sets the armature-wide alpha multiplier.
    #[must_use]
    pub fn set_armature_alpha(armature: Entity, alpha: f64) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetArmatureAlpha(alpha),
        }
    }

    /// Flips the armature horizontally and/or vertically.
    #[must_use]
    pub fn set_flip(armature: Entity, flip_x: Option<bool>, flip_y: Option<bool>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::SetFlip { flip_x, flip_y },
        }
    }

    /// Replaces one atlas page (data + image) at runtime.
    #[must_use]
    pub fn replace_atlas(armature: Entity, page: usize, atlas: Handle<AtlasAsset>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::ReplaceAtlas { page, atlas },
        }
    }

    /// Replaces the armature's skin with `skin` from the same skeleton data.
    #[must_use]
    pub fn replace_skin(armature: Entity, skin: impl Into<String>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::ReplaceSkin { skin: skin.into() },
        }
    }

    /// Replaces `slot`'s display with a display named `display`.
    #[must_use]
    pub fn replace_display(
        armature: Entity,
        slot: impl Into<String>,
        display: impl Into<String>,
    ) -> Self {
        Self {
            armature,
            action: DragonBonesAction::ReplaceDisplay {
                slot: slot.into(),
                display: Some(display.into()),
            },
        }
    }

    /// Clears `slot`'s current display.
    #[must_use]
    pub fn clear_display(armature: Entity, slot: impl Into<String>) -> Self {
        Self {
            armature,
            action: DragonBonesAction::ReplaceDisplay {
                slot: slot.into(),
                display: None,
            },
        }
    }
}

/// Reported when a [`DragonBonesCommand`] cannot be applied.
///
/// Commands are fire-and-forget messages, so failures (entity without an
/// armature, armature already disposed, non-existent animation name) are
/// reported through this message instead of being swallowed by `warn!`.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct DragonBonesCommandError {
    /// Entity the command targeted.
    pub armature: Entity,
    /// Human-readable reason.
    pub message: String,
}

fn set_state_time_scale(armature: &mut Armature, time_scale: f64) {
    let states: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
    for id in states {
        if let Some(state) = armature.animation.state_mut(id) {
            state.time_scale = time_scale;
        }
    }
}

/// Seeks the most recent active state by name/layer, preserving its speed and
/// pause state. Kept out of `apply`'s match to share the error strings.
fn seek_current_state(armature: &mut Armature, position: f64) -> Result<(), String> {
    let Some(id) = armature.animation.last_state() else {
        return Err("no active animation state to seek".to_owned());
    };
    let Some((name, layer)) = armature
        .animation
        .state(id)
        .map(|state| (state.name.clone(), state.layer))
    else {
        return Err("active animation state disappeared".to_owned());
    };
    armature
        .animation
        .seek(&name, layer, position)
        .map(|_| ())
        .ok_or_else(|| format!("animation `{name}` could not seek to {position}"))
}

/// Applies one command and returns an error description on failure.
///
/// The adapter keeps an armature-level playback state ([`DragonBonesPlaybackState`])
/// so `Play` inherits the speed set by `SetSpeed` and a paused armature keeps
/// newly started animations frozen; both are adapter-level conveniences layered
/// on top of core's per-animation-state time scale (core itself has no
/// armature-level speed/pause concept, matching the reference runtime).
fn apply(
    armature: &mut Armature,
    action: &DragonBonesAction,
    mut playback: Option<&mut DragonBonesPlaybackState>,
) -> Result<(), String> {
    let (desired_speed, paused) = playback
        .as_ref()
        .map_or((1.0, false), |state| (state.time_scale, state.paused));
    match action {
        DragonBonesAction::Play {
            animation,
            play_times,
        } => {
            if armature
                .animation
                .play(animation.as_deref(), *play_times)
                .is_none()
            {
                return Err(format!(
                    "animation `{}` does not exist",
                    animation.as_deref().unwrap_or("<default>")
                ));
            }
            set_state_time_scale(armature, if paused { 0.0 } else { desired_speed });
            Ok(())
        }
        DragonBonesAction::PlayConfig(config) => {
            let effective_speed = config.time_scale.unwrap_or(desired_speed);
            let mut animation_config = AnimationConfig {
                animation: config.animation.clone(),
                play_times: config.play_times,
                fade_in_time: config.fade_in_time,
                time_scale: effective_speed,
                position: config.position,
                ..AnimationConfig::default()
            };
            if armature
                .animation
                .play_config(&mut animation_config)
                .is_none()
            {
                return Err(format!("animation `{}` does not exist", config.animation));
            }
            // Only an explicit `with_time_scale` updates the armature-level
            // desired speed; otherwise `PlayConfig` inherits it.
            if let Some(speed) = config.time_scale
                && let Some(playback) = playback.as_mut()
            {
                playback.time_scale = speed;
            }
            // A paused armature stays frozen even when a new clip starts.
            set_state_time_scale(armature, if paused { 0.0 } else { effective_speed });
            Ok(())
        }
        DragonBonesAction::Stop => {
            armature.animation.stop(None);
            Ok(())
        }
        DragonBonesAction::Pause => {
            if let Some(state) = playback {
                state.paused = true;
            }
            set_state_time_scale(armature, 0.0);
            Ok(())
        }
        DragonBonesAction::Resume => {
            let speed = match playback {
                Some(state) => {
                    state.paused = false;
                    state.time_scale
                }
                None => 1.0,
            };
            set_state_time_scale(armature, speed);
            Ok(())
        }
        DragonBonesAction::SetSpeed(speed) => {
            if let Some(state) = playback {
                state.time_scale = *speed;
            }
            // While paused the desired speed is remembered but the states stay
            // frozen until `Resume`.
            set_state_time_scale(armature, if paused { 0.0 } else { *speed });
            Ok(())
        }
        DragonBonesAction::Seek { position } => seek_current_state(armature, *position),
        DragonBonesAction::SeekProgress { fraction } => {
            let Some(id) = armature.animation.last_state() else {
                return Err("no active animation state to seek".to_owned());
            };
            let duration = armature
                .animation
                .state(id)
                .map_or(0.0, dragonbones_core::animation::AnimationState::total_time);
            if duration <= 0.0 {
                return Err("current animation has zero duration".to_owned());
            }
            seek_current_state(armature, duration * fraction.clamp(0.0, 1.0))
        }
        DragonBonesAction::SetLoop { play_times } => {
            let ids: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(state) = armature.animation.state_mut(id) {
                    state.play_times = *play_times;
                }
            }
            Ok(())
        }
        DragonBonesAction::SetLayer { layer } => {
            let ids: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(state) = armature.animation.state_mut(id) {
                    state.layer = *layer;
                }
            }
            Ok(())
        }
        DragonBonesAction::SetGroup { group } => {
            let ids: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(state) = armature.animation.state_mut(id) {
                    state.group.clone_from(group);
                }
            }
            Ok(())
        }
        DragonBonesAction::FadeIn {
            animation,
            duration,
        } => {
            let id = armature.animation.fade_in_with_options(
                animation,
                *duration,
                -1,
                0,
                None,
                AnimationFadeOutMode::SameLayerAndGroup,
            );
            if id.is_none() {
                return Err(format!("animation `{animation}` does not exist"));
            }
            set_state_time_scale(armature, if paused { 0.0 } else { desired_speed });
            Ok(())
        }
        DragonBonesAction::FadeOut { duration } => {
            let ids: Vec<StateId> = armature.animation.states().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(state) = armature.animation.state_mut(id) {
                    state.fade_out(*duration, true);
                }
            }
            Ok(())
        }
        DragonBonesAction::SetSlotTint { slot, color } => {
            let Some(slot) = armature
                .slots
                .iter_mut()
                .find(|candidate| candidate.slot_name == *slot)
            else {
                return Err(format!("slot `{slot}` not found"));
            };
            slot.set_color(*color);
            Ok(())
        }
        DragonBonesAction::SetSlotVisible { slot, visible } => {
            let Some(slot) = armature
                .slots
                .iter_mut()
                .find(|candidate| candidate.slot_name == *slot)
            else {
                return Err(format!("slot `{slot}` not found"));
            };
            slot.set_visible(*visible);
            Ok(())
        }
        DragonBonesAction::SetArmatureAlpha(alpha) => {
            armature.set_alpha(*alpha);
            Ok(())
        }
        DragonBonesAction::SetFlip { flip_x, flip_y } => {
            if let Some(flip_x) = flip_x {
                armature.set_flip_x(*flip_x);
            }
            if let Some(flip_y) = flip_y {
                armature.set_flip_y(*flip_y);
            }
            Ok(())
        }
        // Content commands are resolved against the factory by
        // `apply_dragonbones_commands`; `apply` only sees animation actions.
        DragonBonesAction::ReplaceAtlas { .. }
        | DragonBonesAction::ReplaceSkin { .. }
        | DragonBonesAction::ReplaceDisplay { .. } => {
            Err("content commands must be applied through the factory".to_owned())
        }
    }
}

/// Applies queued [`DragonBonesCommand`] messages before the frame's advance.
pub(crate) fn apply_dragonbones_commands(
    mut commands: MessageReader<DragonBonesCommand>,
    mut errors: MessageWriter<DragonBonesCommandError>,
    mut instances: Query<(&DragonBonesInstance, Option<&mut DragonBonesPlaybackState>)>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    atlases: Res<Assets<AtlasAsset>>,
) {
    for command in commands.read() {
        let Ok((instance, mut playback)) = instances.get_mut(command.armature) else {
            errors.write(DragonBonesCommandError {
                armature: command.armature,
                message: "entity has no DragonBonesInstance".to_owned(),
            });
            continue;
        };

        // Armature-content commands resolve against the factory (skin data,
        // default-skin displays) rather than the borrowed animation state.
        let result = match &command.action {
            DragonBonesAction::ReplaceAtlas { page, atlas } => {
                apply_replace_atlas(&mut runtime, instance, *page, atlas, &atlases)
            }
            DragonBonesAction::ReplaceSkin { skin } => {
                apply_replace_skin(&mut runtime, instance, skin)
            }
            DragonBonesAction::ReplaceDisplay { slot, display } => {
                apply_replace_display(&mut runtime, instance, slot, display.as_deref())
            }
            _ => {
                let Some(armature) = runtime
                    .factory
                    .armatures
                    .get_mut(instance.armature.index())
                    .and_then(Option::as_mut)
                else {
                    errors.write(DragonBonesCommandError {
                        armature: command.armature,
                        message: format!("armature {:?} is not built", instance.armature),
                    });
                    continue;
                };
                apply(armature, &command.action, playback.as_deref_mut())
            }
        };

        if let Err(message) = result {
            errors.write(DragonBonesCommandError {
                armature: command.armature,
                message,
            });
        } else {
            // Flush state changes (seek, slot tint, alpha, content replacement)
            // into the pose before this frame's sync runs.
            runtime.factory.advance_one(instance.armature, 0.0);
        }
    }
}

fn binding_dragon_bones_key(
    runtime: &DragonBonesRuntime,
    armature: dragonbones_core::armature::ArmatureIdx,
) -> Result<String, String> {
    runtime
        .bindings
        .get(&armature)
        .map(|binding| binding.dragon_bones_key.clone())
        .ok_or_else(|| format!("armature {armature:?} has no adapter binding"))
}

/// Replaces one atlas page and refreshes every slot that samples it.
fn apply_replace_atlas(
    runtime: &mut DragonBonesRuntime,
    instance: &DragonBonesInstance,
    page: usize,
    atlas: &Handle<AtlasAsset>,
    atlases: &Assets<AtlasAsset>,
) -> Result<(), String> {
    let asset = atlases
        .get(atlas)
        .ok_or_else(|| format!("atlas asset {atlas:?} is not loaded"))?;
    let atlas_key = runtime
        .bindings
        .get(&instance.armature)
        .map(|binding| binding.atlas_key.clone())
        .ok_or_else(|| format!("armature {:?} has no adapter binding", instance.armature))?;
    let list = runtime
        .factory
        .atlas_map
        .get_mut(&atlas_key)
        .ok_or_else(|| format!("atlas registration `{atlas_key}` is missing"))?;
    let Some(target) = list.get_mut(page) else {
        return Err(format!("atlas page {page} does not exist"));
    };
    target.copy_from(&asset.data);
    let size = UVec2::new(
        asset.data.width.max(1.0) as u32,
        asset.data.height.max(1.0) as u32,
    );

    // Every armature (including children) that shares this atlas registration.
    let targets: Vec<(dragonbones_core::armature::ArmatureIdx, Vec<_>)> = runtime
        .bindings
        .iter()
        .filter(|(_, binding)| binding.atlas_key == atlas_key)
        .map(|(armature, _)| {
            let slots = runtime
                .factory
                .armature(*armature)
                .slots
                .iter()
                .filter(|slot| {
                    slot.texture_atlas_ref().is_some_and(|atlas_ref| {
                        atlas_ref.name == atlas_key && atlas_ref.index == page
                    })
                })
                .map(|slot| slot.idx)
                .collect();
            (*armature, slots)
        })
        .collect();

    for (armature, slots) in &targets {
        for slot in slots {
            runtime.factory.refresh_slot_texture(*armature, *slot);
        }
        runtime.factory.advance_one(*armature, 0.0);
        if let Some(binding) = runtime.bindings.get_mut(armature)
            && let Some(existing) = binding.atlas_pages.get_mut(page)
        {
            *existing = (asset.texture.clone(), size);
        }
    }
    Ok(())
}

/// Applies a skin replacement and immediately flushes the display-data dirty
/// flags so the adapter's content fingerprint changes in the same frame.
fn apply_replace_skin(
    runtime: &mut DragonBonesRuntime,
    instance: &DragonBonesInstance,
    skin_name: &str,
) -> Result<(), String> {
    let armature_name = runtime
        .factory
        .armature(instance.armature)
        .armature_name
        .clone();
    let db_key = binding_dragon_bones_key(runtime, instance.armature)?;
    let skin = runtime
        .factory
        .get_armature_data(&armature_name, &db_key)
        .and_then(|data| data.get_skin(skin_name))
        .cloned()
        .ok_or_else(|| format!("skin `{skin_name}` not found in armature `{armature_name}`"))?;

    if !runtime
        .factory
        .replace_skin(instance.armature, &db_key, &skin, false, &[])
    {
        return Err(format!(
            "skin `{skin_name}` could not be applied to armature `{armature_name}`"
        ));
    }
    runtime.factory.advance_one(instance.armature, 0.0);
    Ok(())
}

/// Replaces (or clears) one slot's display and flushes dirty display data.
fn apply_replace_display(
    runtime: &mut DragonBonesRuntime,
    instance: &DragonBonesInstance,
    slot_name: &str,
    display_name: Option<&str>,
) -> Result<(), String> {
    let armature_name = runtime
        .factory
        .armature(instance.armature)
        .armature_name
        .clone();
    let db_key = binding_dragon_bones_key(runtime, instance.armature)?;
    let slot_idx = runtime
        .factory
        .armature(instance.armature)
        .get_slot(slot_name)
        .ok_or_else(|| format!("slot `{slot_name}` not found in armature `{armature_name}`"))?;

    let applied = match display_name {
        Some(display_name) => runtime.factory.replace_slot_display(
            &armature_name,
            &db_key,
            slot_name,
            display_name,
            instance.armature,
            slot_idx,
            -1,
        ),
        None => {
            runtime
                .factory
                .replace_display(instance.armature, slot_idx, None, -1);
            true
        }
    };
    if !applied {
        return Err(format!(
            "display `{}` not found in slot `{slot_name}` of armature `{armature_name}`",
            display_name.unwrap_or("<none>")
        ));
    }
    runtime.factory.advance_one(instance.armature, 0.0);
    Ok(())
}
