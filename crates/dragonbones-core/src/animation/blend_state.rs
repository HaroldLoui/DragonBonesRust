//! Deterministic target weight allocation (`AnimationState.ts:1528-1601`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendStateKind {
    BoneTransform,
    BoneAlpha,
    Surface,
    SlotDeform,
    SlotAlpha,
    SlotZIndex,
}

/// Lookup identity only. It is never used to determine update order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlendStateKey {
    pub kind: BlendStateKind,
    pub target_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlendState {
    pub dirty: u32,
    pub layer: i32,
    pub left_weight: f64,
    pub layer_weight: f64,
    pub blend_weight: f64,
}

impl Default for BlendState {
    fn default() -> Self {
        Self {
            dirty: 0,
            layer: 0,
            left_weight: 0.0,
            layer_weight: 0.0,
            blend_weight: 0.0,
        }
    }
}

impl BlendState {
    /// Allocates a state weight in the same target/layer traversal order as TS.
    pub fn update(&mut self, animation_layer: i32, mut animation_weight: f64) -> bool {
        if self.dirty > 0 {
            if self.left_weight > 0.0 {
                if self.layer != animation_layer {
                    if self.layer_weight >= self.left_weight {
                        self.dirty += 1;
                        self.layer = animation_layer;
                        self.left_weight = 0.0;
                        self.blend_weight = 0.0;
                        return false;
                    }

                    self.layer = animation_layer;
                    self.left_weight -= self.layer_weight;
                    self.layer_weight = 0.0;
                }

                animation_weight *= self.left_weight;
                self.dirty += 1;
                self.blend_weight = animation_weight;
                self.layer_weight += self.blend_weight;
                return true;
            }
            return false;
        }

        self.dirty += 1;
        self.layer = animation_layer;
        self.left_weight = 1.0;
        self.blend_weight = animation_weight;
        self.layer_weight = animation_weight;
        true
    }

    pub fn reset(&mut self) {
        self.dirty = 0;
        self.layer = 0;
        self.left_weight = 0.0;
        self.layer_weight = 0.0;
        self.blend_weight = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::{BlendState, BlendStateKey, BlendStateKind};
    use std::collections::HashMap;

    #[test]
    fn first_writer_gets_full_budget_and_same_layer_accumulates() {
        let mut blend = BlendState::default();

        assert!(blend.update(0, 0.4));
        assert_eq!(blend.dirty, 1);
        assert_eq!(blend.left_weight, 1.0);
        assert_eq!(blend.blend_weight, 0.4);
        assert_eq!(blend.layer_weight, 0.4);

        assert!(blend.update(0, 0.3));
        assert_eq!(blend.blend_weight, 0.3);
        assert_eq!(blend.layer_weight, 0.7);
    }

    #[test]
    fn higher_layer_consumes_remaining_weight_then_blocks_lower_layers() {
        let mut blend = BlendState::default();
        assert!(blend.update(0, 0.6));
        assert!(blend.update(1, 0.8));
        assert_eq!(blend.left_weight, 0.4);
        assert!((blend.layer_weight - 0.32).abs() < 1e-12);
        assert!((blend.blend_weight - 0.32).abs() < 1e-12);

        assert!(blend.update(2, 0.5));
        assert_eq!(blend.layer, 2);
        assert!((blend.left_weight - 0.08).abs() < 1e-12);
        assert!((blend.blend_weight - 0.04).abs() < 1e-12);
        assert!((blend.layer_weight - 0.04).abs() < 1e-12);
    }

    #[test]
    fn exhausted_lower_layer_budget_is_blocked() {
        let mut blend = BlendState::default();
        assert!(blend.update(0, 1.0));

        assert!(!blend.update(1, 0.8));
        assert_eq!(blend.layer, 1);
        assert_eq!(blend.left_weight, 0.0);
        assert_eq!(blend.blend_weight, 0.0);
        assert!(!blend.update(2, 0.5));
    }

    #[test]
    fn reset_restores_first_writer_state() {
        let mut blend = BlendState::default();
        assert!(blend.update(4, 0.75));
        blend.reset();
        assert_eq!(blend, BlendState::default());
    }

    #[test]
    fn same_target_name_with_different_kind_uses_distinct_budget_entries() {
        let transform = BlendStateKey {
            kind: BlendStateKind::BoneTransform,
            target_name: "root".to_owned(),
        };
        let alpha = BlendStateKey {
            kind: BlendStateKind::BoneAlpha,
            target_name: "root".to_owned(),
        };
        let z_index = BlendStateKey {
            kind: BlendStateKind::SlotZIndex,
            target_name: "root".to_owned(),
        };
        assert_ne!(transform, alpha);
        assert_ne!(alpha, z_index);

        let mut states: HashMap<BlendStateKey, BlendState> = HashMap::new();
        states.entry(transform.clone()).or_default().update(0, 0.4);
        states.entry(alpha.clone()).or_default().update(1, 0.8);
        states.entry(z_index.clone()).or_default().update(2, 0.2);

        assert_eq!(states.len(), 3);
        assert!((states[&transform].layer_weight - 0.4).abs() < 1e-12);
        assert!((states[&alpha].layer_weight - 0.8).abs() < 1e-12);
        assert!((states[&z_index].layer_weight - 0.2).abs() < 1e-12);
    }
}
