//! Runtime clock arena (`WorldClock.ts`).
//!
//! - [`WorldClock::update_time`] follows `WorldClock.ts:97-155`: NaN -> 0,
//!   negative sentinel -> wall-clock delta, time scale, zero early-out, and the
//!   signed member dt.
//! - `WorldClock.ts:168-254` (`contains` / `add` / `remove` / `clear` / clock
//!   setter) does **not** guard against clock-parent cycles. Rust
//!   [`ClockArena::attach_clock`] rejects them before mutating state; this is a
//!   deliberate limitation vs. the official implementation (D-4.33, distinct
//!   from the AnimationState graph guard D-4.32).
//! - [`ClockArena`] has no delete operation, so a valid [`ClockId`] never
//!   becomes stale.

use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::armature::ArmatureIdx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatableHandle {
    Armature(ArmatureIdx),
    Clock(ClockId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MemberToken(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MemberEntry {
    handle: AnimatableHandle,
    token: MemberToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachResult {
    AlreadyPresent(MemberToken),
    Added(MemberToken),
}

#[derive(Error, Debug, PartialEq, Eq, Clone, Copy)]
pub enum ClockError {
    #[error("clock id is invalid")]
    InvalidClockId,
    #[error("found cycle")]
    Cycle,
    #[error("clock relationship is inconsistent")]
    InconsistentRelationship,
    #[error("token is exhausted")]
    MemberTokenExhausted,
}

pub struct WorldClock {
    pub time: f64,
    pub time_scale: f64,
    system_time: f64,
    parent: Option<ClockId>,
    members: Vec<Option<MemberEntry>>,
    next_member_token: u64,
}

impl WorldClock {
    pub fn new(time: f64) -> Self {
        let system_time = wall_clock_seconds();
        Self {
            time,
            time_scale: 1.0,
            system_time,
            parent: None,
            members: Vec::new(),
            next_member_token: 1,
        }
    }

    #[doc(hidden)]
    pub fn new_with_system_time(time: f64, system_time: f64) -> Self {
        Self {
            time,
            time_scale: 1.0,
            system_time,
            parent: None,
            members: Vec::new(),
            next_member_token: 1,
        }
    }

    /// 对齐 `WorldClock.add`（WorldClock.ts:192-197）：直接成员判重、追加。
    pub(crate) fn attach(&mut self, handle: AnimatableHandle) -> Result<AttachResult, ClockError> {
        if let Some(Some(entry)) = self
            .members
            .iter()
            .find(|m| m.is_some_and(|entry| entry.handle == handle))
        {
            return Ok(AttachResult::AlreadyPresent(entry.token));
        };

        let token = MemberToken(self.next_member_token);
        self.next_member_token = self
            .next_member_token
            .checked_add(1)
            .ok_or(ClockError::MemberTokenExhausted)?;
        self.members.push(Some(MemberEntry { handle, token }));

        Ok(AttachResult::Added(token))
    }

    /// 对齐 `WorldClock.remove`（WorldClock.ts:210-216）：tombstone，不压缩。
    pub(crate) fn detach(&mut self, handle: AnimatableHandle) -> bool {
        if let Some(entry) = self
            .members
            .iter_mut()
            .find(|m| m.is_some_and(|entry| entry.handle == handle))
        {
            *entry = None;
            return true;
        };
        false
    }

    /// 对齐 `WorldClock.clear`（WorldClock.ts:227-233）：清空直接成员。
    pub(crate) fn clear_members(&mut self) -> Vec<AnimatableHandle> {
        let handles = self
            .members
            .iter()
            .filter(|m| m.is_some())
            .map(|m| m.unwrap().handle)
            .collect();

        self.members.clear();
        handles
    }

    /// 对齐 `WorldClock.advanceTime` 的时间段（WorldClock.ts:97-123）。
    pub(crate) fn update_time(&mut self, passed_time: f64, now: f64) -> Option<f64> {
        let mut passed_time = passed_time;
        if passed_time.is_nan() {
            passed_time = 0.0;
        }

        let current_time = now;
        if passed_time < 0.0 {
            passed_time = current_time - self.system_time;
        }

        self.system_time = current_time;
        if self.time_scale != 1.0 {
            passed_time *= self.time_scale;
        }

        if passed_time == 0.0 {
            return None;
        }

        if passed_time < 0.0 {
            self.time -= passed_time;
        } else {
            self.time += passed_time;
        }
        Some(passed_time)
    }

    pub(crate) fn member_snapshot(&self) -> Vec<(usize, MemberToken)> {
        self.members
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.map(|entry| (index, entry.token)))
            .collect()
    }

    pub(crate) fn active_members(&self) -> Vec<AnimatableHandle> {
        self.members
            .iter()
            .filter_map(|entry| entry.map(|entry| entry.handle))
            .collect()
    }

    pub(crate) fn active_member_at(
        &self,
        index: usize,
        token: MemberToken,
    ) -> Option<AnimatableHandle> {
        self.members
            .get(index)
            .and_then(Option::as_ref)
            .filter(|entry| entry.token == token)
            .map(|entry| entry.handle)
    }

    /// Post-visit tombstone compaction (WorldClock.ts:125-154).
    pub(crate) fn compact(&mut self) {
        self.members.retain(Option::is_some);
    }

    /// Direct-member query (TS `contains` is an ancestor walk, WorldClock.ts:168-179;
    /// this helper intentionally checks only the local member list).
    pub(crate) fn contains_member(&self, handle: AnimatableHandle) -> bool {
        self.members
            .iter()
            .any(|entry| entry.is_some_and(|entry| entry.handle == handle))
    }

    /// Whether `count` consecutive `attach` calls can allocate tokens without
    /// overflowing `next_member_token`. Used as an atomicity precheck.
    pub(crate) fn can_attach_count(&self, count: u64) -> bool {
        self.next_member_token.checked_add(count).is_some()
    }
}

pub(crate) fn wall_clock_seconds() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    }
}

#[derive(Default)]
pub struct ClockArena {
    slots: Vec<Option<WorldClock>>,
}

impl ClockArena {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, clock: WorldClock) -> ClockId {
        let index = u32::try_from(self.slots.len())
            .expect("ClockArena cannot contain more than u32::MAX clocks");
        self.slots.push(Some(clock));
        ClockId(index)
    }

    pub fn get(&self, id: ClockId) -> Option<&WorldClock> {
        let index = id.0 as usize;
        match self.slots.get(index) {
            Some(opt) => opt.as_ref(),
            None => None,
        }
    }

    pub fn get_mut(&mut self, id: ClockId) -> Option<&mut WorldClock> {
        let index = id.0 as usize;
        match self.slots.get_mut(index) {
            Some(opt) => opt.as_mut(),
            None => None,
        }
    }

    pub(crate) fn parent(&self, id: ClockId) -> Option<ClockId> {
        self.get(id).and_then(|clock| clock.parent)
    }

    /// Updates both sides of a nested-clock relationship, mirroring the
    /// `WorldClock.clock` setter (WorldClock.ts:240-254). Cycles are rejected
    /// before the existing relation is changed (D-4.33).
    pub(crate) fn attach_clock(
        &mut self,
        parent: ClockId,
        child: ClockId,
    ) -> Result<(), ClockError> {
        if parent == child {
            return Err(ClockError::Cycle);
        }

        let old_parent = self.get(child).ok_or(ClockError::InvalidClockId)?.parent;
        if self.get(parent).is_none() {
            return Err(ClockError::InvalidClockId);
        }

        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            if id == child {
                return Err(ClockError::Cycle);
            }
            ancestor = self.get(id).ok_or(ClockError::InvalidClockId)?.parent;
        }

        if old_parent == Some(parent) {
            return if self
                .get(parent)
                .is_some_and(|clock| clock.contains_member(AnimatableHandle::Clock(child)))
            {
                Ok(())
            } else {
                Err(ClockError::InconsistentRelationship)
            };
        }

        if let Some(old_parent) = old_parent {
            let old_clock = self
                .get(old_parent)
                .ok_or(ClockError::InconsistentRelationship)?;
            if !old_clock.contains_member(AnimatableHandle::Clock(child)) {
                return Err(ClockError::InconsistentRelationship);
            }
        }

        // Add to the new parent first; token exhaustion leaves the old
        // relation intact.
        match self
            .get_mut(parent)
            .ok_or(ClockError::InvalidClockId)?
            .attach(AnimatableHandle::Clock(child))?
        {
            AttachResult::Added(_) => {}
            AttachResult::AlreadyPresent(_) => {
                return Err(ClockError::InconsistentRelationship);
            }
        }

        if let Some(old_parent) = old_parent {
            let detached = self
                .get_mut(old_parent)
                .expect("old parent was validated")
                .detach(AnimatableHandle::Clock(child));
            debug_assert!(detached, "old parent membership was validated");
        }

        self.get_mut(child)
            .expect("child clock was validated")
            .parent = Some(parent);
        Ok(())
    }

    pub(crate) fn detach_clock(&mut self, child: ClockId) -> Result<bool, ClockError> {
        let parent = self.get(child).ok_or(ClockError::InvalidClockId)?.parent;
        let Some(parent) = parent else {
            return Ok(false);
        };

        let parent_clock = self
            .get(parent)
            .ok_or(ClockError::InconsistentRelationship)?;
        if !parent_clock.contains_member(AnimatableHandle::Clock(child)) {
            return Err(ClockError::InconsistentRelationship);
        }

        self.get_mut(parent)
            .expect("parent clock was validated")
            .detach(AnimatableHandle::Clock(child));
        self.get_mut(child)
            .expect("child clock was validated")
            .parent = None;
        Ok(true)
    }

    /// Removes `handle` from every clock's member list.
    ///
    /// Recovery-only helper for `dispose_armature`: when a back-reference was
    /// already inconsistent, the owning clock cannot be derived reliably, so
    /// scan all clocks and drop any residual membership. Clock parent
    /// relationships are not touched (Armature handles only in practice).
    pub(crate) fn detach_member_everywhere(&mut self, handle: AnimatableHandle) -> bool {
        let mut detached = false;
        for slot in &mut self.slots {
            if let Some(clock) = slot
                && clock.detach(handle)
            {
                detached = true;
            }
        }
        detached
    }

    /// Clears all direct members of `id` and returns their handles.
    ///
    /// ClockArena owns only Clock-side membership, so callers (BaseFactory) use
    /// the returned list to clear Armature back-references. Clock members have
    /// their `parent` reset to `None` here as well. The arena has no delete
    /// operation; `id` stays valid and can be populated again.
    pub(crate) fn clear_clock(&mut self, id: ClockId) -> Result<Vec<AnimatableHandle>, ClockError> {
        let handles = self
            .get_mut(id)
            .ok_or(ClockError::InvalidClockId)?
            .clear_members();

        for handle in &handles {
            if let AnimatableHandle::Clock(child) = handle
                && let Some(child_clock) = self.get_mut(*child)
                && child_clock.parent == Some(id)
            {
                child_clock.parent = None;
            }
        }

        Ok(handles)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnimatableHandle, AttachResult, ClockArena, ClockError, ClockId, MemberToken, WorldClock,
    };
    use crate::armature::ArmatureIdx;

    #[test]
    fn initial_time_is_separate_from_system_time() {
        let clock = WorldClock::new_with_system_time(12.5, 100.0);

        assert_eq!(clock.time, 12.5);
        assert_eq!(clock.system_time, 100.0);
        assert_eq!(clock.time_scale, 1.0);
        assert_eq!(clock.parent, None);
    }

    #[test]
    fn attach_is_idempotent_and_reattach_gets_a_new_token() {
        let mut clock = WorldClock::new_with_system_time(0.0, 100.0);
        let handle = AnimatableHandle::Armature(ArmatureIdx(7));

        let first_token = match clock.attach(handle).expect("first attach") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => panic!("first attach must insert"),
        };
        assert_eq!(
            clock.attach(handle).expect("duplicate attach"),
            AttachResult::AlreadyPresent(first_token)
        );
        assert_eq!(clock.member_snapshot(), vec![(0, first_token)]);

        assert!(clock.detach(handle));
        let second_token = match clock.attach(handle).expect("reattach") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => panic!("detached handle must get a new entry"),
        };

        assert_ne!(first_token, second_token);
        assert_eq!(clock.active_member_at(0, first_token), None);
        assert_eq!(clock.active_member_at(1, second_token), Some(handle));
    }

    #[test]
    fn clear_and_reattach_does_not_revive_a_snapshot_entry() {
        let mut clock = WorldClock::new_with_system_time(0.0, 100.0);
        let handle = AnimatableHandle::Armature(ArmatureIdx(3));
        let first_token = match clock.attach(handle).expect("attach") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => panic!("first attach must insert"),
        };
        let snapshot = clock.member_snapshot();

        assert_eq!(clock.clear_members(), vec![handle]);
        let second_token = match clock.attach(handle).expect("reattach") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => panic!("clear must remove the old entry"),
        };

        assert_ne!(first_token, second_token);
        assert_eq!(clock.active_member_at(snapshot[0].0, snapshot[0].1), None);
    }

    #[test]
    fn update_time_preserves_official_order_and_sign() {
        let mut clock = WorldClock::new_with_system_time(4.0, 100.0);

        assert_eq!(clock.update_time(-1.0, 100.25), Some(0.25));
        assert_eq!(clock.time, 4.25);
        assert_eq!(clock.system_time, 100.25);

        clock.time_scale = 2.0;
        assert_eq!(clock.update_time(-0.5, 100.5), Some(0.5));
        assert_eq!(clock.time, 4.75);
        assert_eq!(clock.system_time, 100.5);

        assert_eq!(clock.update_time(f64::NAN, 101.0), None);
        assert_eq!(clock.time, 4.75);
        assert_eq!(clock.system_time, 101.0);
    }

    #[test]
    fn update_time_applies_fractional_and_negative_time_scale() {
        let mut clock = WorldClock::new_with_system_time(10.0, 100.0);

        clock.time_scale = 0.5;
        // 返回值就是派发给成员的带符号 dt（BaseFactory 直接透传）。
        assert_eq!(clock.update_time(1.5, 100.1), Some(0.75));
        assert_eq!(clock.time, 10.75);

        // 负 timeScale 让成员收到负 dt；官方 `time -= passedTime` 使时钟时间增加。
        clock.time_scale = -0.5;
        assert_eq!(clock.update_time(1.0, 100.2), Some(-0.5));
        assert_eq!(clock.time, 11.25);
    }

    #[test]
    fn arena_ids_are_slot_handles_and_nested_cycles_are_rejected() {
        let mut arena = ClockArena::new();
        let parent = arena.insert(WorldClock::new_with_system_time(0.0, 100.0));
        let child = arena.insert(WorldClock::new_with_system_time(0.0, 100.0));

        assert_eq!(parent, ClockId(0));
        assert_eq!(child, ClockId(1));
        assert_eq!(arena.parent(child), None);

        arena
            .attach_clock(parent, child)
            .expect("attach child clock");
        assert_eq!(arena.parent(child), Some(parent));
        assert_eq!(arena.attach_clock(child, parent), Err(ClockError::Cycle));
        assert_eq!(arena.parent(child), Some(parent));

        let handles = arena.clear_clock(parent).expect("clear parent clock");
        assert_eq!(handles, vec![AnimatableHandle::Clock(child)]);
        assert_eq!(arena.parent(child), None);
    }

    #[test]
    fn active_member_lookup_checks_index_and_token() {
        let mut clock = WorldClock::new_with_system_time(0.0, 100.0);
        let first = AnimatableHandle::Armature(ArmatureIdx(1));
        let second = AnimatableHandle::Armature(ArmatureIdx(2));
        let first_token = match clock.attach(first).expect("attach first") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => unreachable!(),
        };
        let second_token = match clock.attach(second).expect("attach second") {
            AttachResult::Added(token) => token,
            AttachResult::AlreadyPresent(_) => unreachable!(),
        };
        assert_eq!(first_token, MemberToken(1));
        assert_eq!(second_token, MemberToken(2));

        assert!(clock.detach(first));
        assert_eq!(clock.active_member_at(0, first_token), None);
        assert_eq!(clock.active_member_at(1, second_token), Some(second));
    }
}
