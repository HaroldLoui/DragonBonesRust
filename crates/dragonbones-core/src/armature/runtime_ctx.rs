//! Narrow arena/clock/event access available while an Armature is advancing.
//!
//! `current_armature` is temporarily taken out of the armature arena, so the
//! context can resolve other Armatures by handle but cannot use the arena to
//! access the current one. Its current clock is copied into `current_clock`,
//! which is reserved for future child-clock inheritance and currently has no
//! reader in production code.

use thiserror::Error;

use crate::animation::world_clock::{
    AnimatableHandle, AttachResult, ClockArena, ClockError, ClockId,
};
use crate::event::EventObject;

use super::{Armature, ArmatureIdx};

/// Borrowed runtime capabilities needed by Armature, Slot, and animation code.
pub struct RuntimeCtx<'a> {
    armatures: &'a mut [Option<Armature>],
    clocks: &'a mut ClockArena,
    pending_events: &'a mut Vec<EventObject>,
    current_armature: Option<ArmatureIdx>,
    /// 预留字段：构造时由调用方复制当前骨架的 clock，供后续子 clock 继承
    /// 逻辑读取；阶段 4 生产代码无调用方（`RuntimeCtx::new` 的调用面在
    /// player/timeline 中，暂不删参数以免牵连并行改动）。
    current_clock: Option<ClockId>,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCtxError {
    #[error("armature handle is invalid or currently taken out of the arena")]
    InvalidArmature,
    #[error(transparent)]
    Clock(#[from] ClockError),
}

impl<'a> RuntimeCtx<'a> {
    pub(crate) fn new(
        armatures: &'a mut [Option<Armature>],
        clocks: &'a mut ClockArena,
        pending_events: &'a mut Vec<EventObject>,
        current_armature: Option<ArmatureIdx>,
        current_clock: Option<ClockId>,
    ) -> Self {
        Self {
            armatures,
            clocks,
            pending_events,
            current_armature,
            current_clock,
        }
    }

    /// Resolves an Armature in the arena. The Armature being advanced is absent.
    #[must_use]
    pub fn armature(&self, id: ArmatureIdx) -> Option<&Armature> {
        if self.current_armature == Some(id) {
            return None;
        }
        self.armatures.get(id.index()).and_then(Option::as_ref)
    }

    /// Resolves a mutable Armature in the arena. The Armature being advanced is absent.
    pub fn armature_mut(&mut self, id: ArmatureIdx) -> Option<&mut Armature> {
        if self.current_armature == Some(id) {
            return None;
        }
        self.armatures.get_mut(id.index()).and_then(Option::as_mut)
    }

    /// Clock inherited by children of the Armature currently being advanced.
    ///
    /// 预留：阶段 4 生产代码没有调用方，保留该方法以说明该字段的预期用途。
    #[must_use]
    pub fn current_clock(&self) -> Option<ClockId> {
        self.current_clock
    }

    /// Atomically moves an arena-resident Armature between clock memberships.
    ///
    /// The currently advancing Armature is not in the arena and must be managed
    /// by its owner before it is taken out or after it is restored.
    pub(crate) fn move_armature_clock(
        &mut self,
        id: ArmatureIdx,
        new_clock: Option<ClockId>,
    ) -> Result<(), RuntimeCtxError> {
        if self.current_armature == Some(id) {
            return Err(RuntimeCtxError::InvalidArmature);
        }

        let old_clock = self
            .armature(id)
            .ok_or(RuntimeCtxError::InvalidArmature)?
            .clock();

        if old_clock == new_clock {
            if let Some(clock_id) = old_clock
                && !self
                    .clocks
                    .get(clock_id)
                    .ok_or(ClockError::InvalidClockId)?
                    .contains_member(AnimatableHandle::Armature(id))
            {
                return Err(ClockError::InconsistentRelationship.into());
            }
            return Ok(());
        }

        if let Some(old_clock) = old_clock {
            let old = self
                .clocks
                .get(old_clock)
                .ok_or(ClockError::InvalidClockId)?;
            if !old.contains_member(AnimatableHandle::Armature(id)) {
                return Err(ClockError::InconsistentRelationship.into());
            }
        }

        if let Some(new_clock) = new_clock {
            let new = self
                .clocks
                .get_mut(new_clock)
                .ok_or(ClockError::InvalidClockId)?;
            match new.attach(AnimatableHandle::Armature(id))? {
                AttachResult::Added(_) => {}
                AttachResult::AlreadyPresent(_) => {
                    return Err(ClockError::InconsistentRelationship.into());
                }
            }
        }

        if let Some(old_clock) = old_clock {
            let detached = self
                .clocks
                .get_mut(old_clock)
                .ok_or(ClockError::InvalidClockId)?
                .detach(AnimatableHandle::Armature(id));
            if !detached {
                if let Some(new_clock) = new_clock {
                    self.clocks
                        .get_mut(new_clock)
                        .expect("new clock was validated")
                        .detach(AnimatableHandle::Armature(id));
                }
                return Err(ClockError::InconsistentRelationship.into());
            }
        }

        self.armature_mut(id)
            .ok_or(RuntimeCtxError::InvalidArmature)?
            .clock = new_clock;
        Ok(())
    }

    /// Best-effort recovery used by `dispose_armature` when the clock
    /// relationship is already inconsistent: removes any residual membership
    /// from every clock and forces the back-reference to `None`. Returns
    /// `false` only when the Armature itself is unavailable in the arena.
    pub(crate) fn force_detach_armature_clock(&mut self, id: ArmatureIdx) -> bool {
        if self.current_armature == Some(id) {
            return false;
        }

        self.clocks
            .detach_member_everywhere(AnimatableHandle::Armature(id));
        let Some(armature) = self.armature_mut(id) else {
            return false;
        };
        armature.clock = None;
        true
    }

    /// Queues a newly-created event. Stage 4 does not dispatch it here.
    pub(crate) fn buffer_event(&mut self, event: EventObject) {
        self.pending_events.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeCtx, RuntimeCtxError};
    use crate::animation::world_clock::{AnimatableHandle, ClockArena, ClockError, WorldClock};
    use crate::armature::{Armature, ArmatureIdx, DisplayHandle, NullArmatureProxy};
    use crate::event::EventObject;
    use crate::model::DragonBonesData;
    use std::sync::Arc;

    fn armature() -> Armature {
        Armature::new(
            ArmatureIdx(0),
            Arc::new(DragonBonesData::default()),
            "runtime-context-test",
            Box::new(NullArmatureProxy::new()),
            DisplayHandle::Null,
        )
    }

    #[test]
    fn current_armature_is_not_resolved_through_its_context() {
        let mut armatures = vec![Some(armature())];
        let mut clocks = ClockArena::new();
        let clock = clocks.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let mut pending_events = Vec::new();

        let context = RuntimeCtx::new(
            &mut armatures,
            &mut clocks,
            &mut pending_events,
            Some(ArmatureIdx(0)),
            Some(clock),
        );

        assert!(context.armature(ArmatureIdx(0)).is_none());
    }

    #[test]
    fn moving_armature_clock_updates_both_memberships_and_back_reference() {
        let mut armatures = vec![Some(armature())];
        let mut clocks = ClockArena::new();
        let old_clock = clocks.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let new_clock = clocks.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let mut pending_events = Vec::new();

        {
            let mut context =
                RuntimeCtx::new(&mut armatures, &mut clocks, &mut pending_events, None, None);
            context
                .move_armature_clock(ArmatureIdx(0), Some(old_clock))
                .expect("initial attach");
            context
                .move_armature_clock(ArmatureIdx(0), Some(new_clock))
                .expect("move to new clock");
        }

        assert_eq!(
            armatures[0].as_ref().map(Armature::clock),
            Some(Some(new_clock))
        );
        assert!(
            !clocks
                .get(old_clock)
                .expect("old clock")
                .contains_member(AnimatableHandle::Armature(ArmatureIdx(0)))
        );
        assert!(
            clocks
                .get(new_clock)
                .expect("new clock")
                .contains_member(AnimatableHandle::Armature(ArmatureIdx(0)))
        );
    }

    #[test]
    fn invalid_new_clock_leaves_the_old_relationship_intact() {
        let mut armatures = vec![Some(armature())];
        let mut clocks = ClockArena::new();
        let old_clock = clocks.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let mut pending_events = Vec::new();

        {
            let mut context =
                RuntimeCtx::new(&mut armatures, &mut clocks, &mut pending_events, None, None);
            context
                .move_armature_clock(ArmatureIdx(0), Some(old_clock))
                .expect("initial attach");
            assert_eq!(
                context.move_armature_clock(
                    ArmatureIdx(0),
                    Some(crate::animation::world_clock::ClockId(99))
                ),
                Err(RuntimeCtxError::Clock(ClockError::InvalidClockId))
            );
        }

        assert_eq!(
            armatures[0].as_ref().map(Armature::clock),
            Some(Some(old_clock))
        );
        assert!(
            clocks
                .get(old_clock)
                .expect("old clock")
                .contains_member(AnimatableHandle::Armature(ArmatureIdx(0)))
        );
    }

    #[test]
    fn buffer_event_appends_without_value_deduplication() {
        let mut armatures = Vec::new();
        let mut clocks = ClockArena::new();
        let mut pending_events = Vec::new();
        {
            let mut context =
                RuntimeCtx::new(&mut armatures, &mut clocks, &mut pending_events, None, None);
            context.buffer_event(EventObject::default());
            context.buffer_event(EventObject::default());
        }

        assert_eq!(pending_events.len(), 2);
    }
}
