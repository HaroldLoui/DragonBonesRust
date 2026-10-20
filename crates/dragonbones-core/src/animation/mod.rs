//! Animation runtime modules.

pub mod animation_state;
pub mod blend_state;
mod player;
pub mod state_id;
pub mod timeline_state;
pub mod timeline_states;
pub mod world_clock;

pub use animation_state::{
    AnimationState, FadeEvent, FadeState, PlayheadState, SubFadeState, TIMELINE_GROUP_UPDATE_ORDER,
    TimelineDirty, TimelineGroup,
};
pub use blend_state::{BlendState, BlendStateKey, BlendStateKind};
pub use player::Animation;
pub(crate) use player::AnimationUpdateCtx;
pub use state_id::StateId;
pub use timeline_state::{
    ActionTimelineTime, BaseTimelineState, TimelinePlayState, TimelineTimeInput,
    TweenFrameParameters,
};
