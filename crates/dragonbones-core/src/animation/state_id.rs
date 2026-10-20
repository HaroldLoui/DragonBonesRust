//! Stable identity and slot storage for AnimationState values.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StateId {
    slot: u32,
    generation: u32,
}

impl StateId {
    #[must_use]
    pub const fn slot(self) -> u32 {
        self.slot
    }

    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

struct StateSlot<T> {
    generation: u32,
    value: Option<T>,
    retired: bool,
}

/// Arena storage whose IDs remain stable when another state is removed.
pub(crate) struct StateArena<T> {
    slots: Vec<StateSlot<T>>,
    free: Vec<u32>,
}

impl<T> Default for StateArena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> StateArena<T> {
    pub(crate) fn insert(&mut self, value: T) -> Result<StateId, StateArenaError> {
        if let Some(slot_id) = self.free.pop() {
            let slot = &mut self.slots[slot_id as usize];
            debug_assert!(slot.value.is_none() && !slot.retired);
            slot.value = Some(value);
            return Ok(StateId {
                slot: slot_id,
                generation: slot.generation,
            });
        }

        let slot_id =
            u32::try_from(self.slots.len()).map_err(|_| StateArenaError::SlotExhausted)?;
        self.slots.push(StateSlot {
            generation: 0,
            value: Some(value),
            retired: false,
        });
        Ok(StateId {
            slot: slot_id,
            generation: 0,
        })
    }

    #[must_use]
    pub(crate) fn get(&self, id: StateId) -> Option<&T> {
        let slot = self.slots.get(id.slot as usize)?;
        if slot.generation == id.generation {
            slot.value.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn get_mut(&mut self, id: StateId) -> Option<&mut T> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.value.as_mut()
    }

    pub(crate) fn remove(&mut self, id: StateId) -> Option<T> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }

        let value = slot.value.take()?;
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            self.free.push(id.slot);
        } else {
            // Never wrap a generation: stale IDs must not become valid again.
            slot.retired = true;
        }
        Some(value)
    }

    pub(crate) fn clear(&mut self) -> Vec<T> {
        let mut removed = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let Some(value) = slot.value.take() else {
                continue;
            };
            removed.push(value);
            if let Some(next_generation) = slot.generation.checked_add(1) {
                slot.generation = next_generation;
                self.free.push(index as u32);
            } else {
                slot.retired = true;
            }
        }
        removed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateArenaError {
    SlotExhausted,
}

#[cfg(test)]
mod tests {
    use super::StateArena;

    #[test]
    fn removing_a_state_does_not_move_other_ids_and_reuse_invalidates_old_id() {
        let mut states = StateArena::default();
        let first = states.insert("first").expect("insert first");
        let second = states.insert("second").expect("insert second");

        assert_eq!(states.remove(first), Some("first"));
        assert_eq!(states.get(second), Some(&"second"));

        let replacement = states.insert("replacement").expect("reuse a slot");
        assert_eq!(replacement.slot(), first.slot());
        assert_ne!(replacement.generation(), first.generation());
        assert_eq!(states.get(first), None);
        assert_eq!(states.get(replacement), Some(&"replacement"));
        assert_eq!(states.get(second), Some(&"second"));
    }

    #[test]
    fn clear_invalidates_all_issued_ids() {
        let mut states = StateArena::default();
        let a = states.insert(1).expect("insert A");
        let b = states.insert(2).expect("insert B");

        assert_eq!(states.clear(), vec![1, 2]);
        assert_eq!(states.get(a), None);
        assert_eq!(states.get(b), None);
        let reused = states.insert(3).expect("reuse cleared slot");
        assert_ne!(reused.generation(), a.generation());
        assert_ne!(reused.generation(), b.generation());
    }
}
