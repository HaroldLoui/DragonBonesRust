//! DragonBones animation events forwarded into Bevy messages.
//!
//! Core dispatches every [`EventObject`] (start / complete / loopComplete /
//! frameEvent / soundEvent / fadeIn* / …) through the armature proxy
//! (`ArmatureProxy::dispatch_db_event` in `BaseFactory::dispatch_events`). The
//! adapter installs [`BevyArmatureProxy`], which keeps the reference listener
//! behaviour and additionally records the event in a shared queue; a Bevy system
//! drains that queue into [`DragonBonesEvent`] messages in `Update`.
//!
//! Consumers read them like any other Bevy message:
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use dragonbones_bevy::prelude::*;
//!
//! fn on_events(mut events: MessageReader<DragonBonesEvent>) {
//!     for event in events.read() {
//!         info!("{} fired for armature {:?}", event.event_type(), event.armature);
//!     }
//! }
//! ```
//!
//! The queue uses `Arc<Mutex<…>>` because the core factory (and therefore the
//! proxies) lives in a non-send Bevy resource, while the messages are written by
//! a regular send system.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use dragonbones_core::armature::{ArmatureIdx, ArmatureProxy, NullArmatureProxy};
use dragonbones_core::event::{DbEventListener, EventDispatcher, EventObject, ListenerId};

/// One DragonBones animation event, mirrored into Bevy as a [`Message`].
#[derive(Message, Debug, Clone, PartialEq)]
pub struct DragonBonesEvent {
    /// Armature instance that emitted the event (core arena index; matches
    /// [`crate::DragonBonesInstance::armature`]).
    pub armature: ArmatureIdx,
    /// Full core event: `event_type`, `time`, `name`, bone/slot names, action
    /// data and user data.
    pub event: EventObject,
}

impl DragonBonesEvent {
    /// Shorthand for the core event type string (`"start"`, `"complete"`,
    /// `"frameEvent"`, …). Compare against [`EventObject`] constants.
    #[must_use]
    pub fn event_type(&self) -> &str {
        &self.event.event_type
    }
}

/// Queue shared between the non-send armature proxies and the draining Bevy
/// system.
#[derive(Resource, Clone, Default)]
pub(crate) struct DragonBonesEventQueue {
    events: Arc<Mutex<Vec<DragonBonesEvent>>>,
}

impl DragonBonesEventQueue {
    fn push(&self, event: DragonBonesEvent) {
        match self.events.lock() {
            Ok(mut events) => events.push(event),
            // A poisoned lock only happens if a previous drain panicked; dropping
            // events is better than panicking inside the animation update.
            Err(poisoned) => poisoned.into_inner().push(event),
        }
    }

    /// Takes every queued event, leaving the queue empty.
    pub(crate) fn drain(&self) -> Vec<DragonBonesEvent> {
        match self.events.lock() {
            Ok(mut events) => std::mem::take(&mut *events),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

/// Armature proxy that records events in [`DragonBonesEventQueue`].
///
/// Listener registration/removal is delegated to [`NullArmatureProxy`], so the
/// reference `addDBEventListener` semantics stay available even when the Bevy
/// message path is not used.
pub(crate) struct BevyArmatureProxy {
    inner: NullArmatureProxy,
    queue: DragonBonesEventQueue,
}

impl BevyArmatureProxy {
    pub(crate) fn new(queue: DragonBonesEventQueue) -> Self {
        Self {
            inner: NullArmatureProxy::new(),
            queue,
        }
    }
}

impl EventDispatcher for BevyArmatureProxy {
    fn add_db_event_listener(&mut self, ty: &str, listener: DbEventListener) -> ListenerId {
        self.inner.add_db_event_listener(ty, listener)
    }

    fn remove_db_event_listener(&mut self, id: ListenerId) {
        self.inner.remove_db_event_listener(id);
    }

    /// The Bevy bridge is a permanent listener for every event type.
    ///
    /// Core gates event *creation/buffering* on this method (`action.rs`,
    /// `animation_state.rs`, `armature.rs` all call
    /// `proxy.has_db_event_listener(type)` before buffering), so returning the
    /// inner listener state would suppress events that no plain listener asked
    /// for. The adapter forwards every event into Bevy messages, hence the
    /// unconditional `true`.
    fn has_db_event_listener(&mut self, _ty: &str) -> bool {
        true
    }

    fn dispatch_db_event(&mut self, ty: &str, event: &EventObject) {
        self.inner.dispatch_db_event(ty, event);
        // `EventObject.armature` is set by core before dispatch; fall back to the
        // armature recorded by `db_init` for defensive completeness.
        if let Some(armature) = event.armature.or(self.inner.armature) {
            self.queue.push(DragonBonesEvent {
                armature,
                event: event.clone(),
            });
        }
    }
}

impl ArmatureProxy for BevyArmatureProxy {
    fn db_init(&mut self, armature: ArmatureIdx) {
        self.inner.db_init(armature);
    }

    fn db_clear(&mut self) {
        self.inner.db_clear();
    }

    fn db_update(&mut self) {
        self.inner.db_update();
    }

    fn dispose(&mut self, dispose_proxy: bool) {
        self.inner.dispose(dispose_proxy);
    }
}

/// Drains queued core events into [`DragonBonesEvent`] messages.
///
/// Runs after [`crate::DragonBonesSystems::Advance`]'s advance step, so events
/// produced by this frame's animation update are readable in the same frame.
pub(crate) fn forward_dragonbones_events(
    queue: Res<DragonBonesEventQueue>,
    mut writer: MessageWriter<DragonBonesEvent>,
) {
    for event in queue.drain() {
        writer.write(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_records_dispatched_events() {
        let queue = DragonBonesEventQueue::default();
        let mut proxy = BevyArmatureProxy::new(queue.clone());
        proxy.db_init(ArmatureIdx(0));
        let event = EventObject {
            event_type: "start".to_owned(),
            armature: Some(ArmatureIdx(0)),
            ..EventObject::default()
        };
        proxy.dispatch_db_event("start", &event);
        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].event_type(), "start");
        assert!(queue.drain().is_empty());
    }
}
