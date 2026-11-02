//! Deterministic fingerprints used by the adapter's dirty checks.
//!
//! The adapter must notice *any* core mutation that changes what a slot looks
//! like — including changes that do not move the display index, such as
//! `replace_texture_data` / `replace_skin` (the reference runtimes can swap a
//! slot's display list or atlas region at runtime). Instead of relying on dirty
//! flags that every mutation site would have to remember to set, the adapter
//! hashes the inputs it reads:
//!
//! - [`armature_pose_hash`] decides whether the whole armature is skipped for the
//!   frame (armature-level dirty check);
//! - [`slot_visual_hash`] decides whether one slot's mesh has to be rebuilt
//!   (display/texture/geometry content changed without a display-frame change).

use dragonbones_core::armature::{Armature, Slot};
use dragonbones_core::geom::{ColorTransform, Matrix};

/// FNV-1a 64-bit offset basis.
const POSE_HASH_SEED: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const POSE_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Small deterministic FNV-1a hasher.
pub(crate) struct PoseHasher(u64);

impl PoseHasher {
    fn new() -> Self {
        Self(POSE_HASH_SEED)
    }

    pub(crate) fn mix(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(POSE_HASH_PRIME);
    }

    pub(crate) fn mix_f64(&mut self, value: f64) {
        self.mix(value.to_bits());
    }

    pub(crate) fn mix_bool(&mut self, value: bool) {
        self.mix(u64::from(value));
    }

    pub(crate) fn mix_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }

    pub(crate) fn mix_option_usize(&mut self, value: Option<usize>) {
        match value {
            Some(value) => {
                self.mix(1);
                self.mix_usize(value);
            }
            None => self.mix(0),
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

pub(crate) fn hash_matrix(hasher: &mut PoseHasher, matrix: &Matrix) {
    hasher.mix_f64(matrix.a);
    hasher.mix_f64(matrix.b);
    hasher.mix_f64(matrix.c);
    hasher.mix_f64(matrix.d);
    hasher.mix_f64(matrix.tx);
    hasher.mix_f64(matrix.ty);
}

fn hash_color(hasher: &mut PoseHasher, color: &ColorTransform) {
    hasher.mix_f64(color.alpha_multiplier);
    hasher.mix_f64(color.red_multiplier);
    hasher.mix_f64(color.green_multiplier);
    hasher.mix_f64(color.blue_multiplier);
    hasher.mix_f64(color.alpha_offset);
    hasher.mix_f64(color.red_offset);
    hasher.mix_f64(color.green_offset);
    hasher.mix_f64(color.blue_offset);
}

/// Fingerprint of the slot's geometry/mesh content.
///
/// Cheap by design: core bumps [`Slot::content_revision`] whenever
/// `_updateDisplayData` observes a display frame / geometry / texture content
/// change (which is what runtime `replace_texture_data` / `replace_skin` do), so
/// the adapter only hashes a counter plus the structural fields it also needs
/// for the rebuild decision instead of every region/name every frame.
pub(crate) fn slot_visual_hash(slot: &Slot) -> u64 {
    let mut hasher = PoseHasher::new();
    hasher.mix(u64::from(slot.content_revision));
    hasher.mix_usize(slot.display_frames.len());
    hasher.mix_option_usize(slot.display_frame);
    match slot.child_armature {
        Some(child) => {
            hasher.mix(1);
            hasher.mix_usize(child.index());
        }
        None => hasher.mix(0),
    }
    hasher.finish()
}

/// Hash of everything [`crate::sync::sync_dragonbones`] reads from the core
/// armature for one frame.
///
/// Slot iteration order is part of the hash (the draw order feeds the z write),
/// so re-sorting `armature.slots` marks the armature dirty as well.
pub(crate) fn armature_pose_hash(armature: &Armature) -> u64 {
    let mut hasher = PoseHasher::new();
    for slot in &armature.slots {
        hasher.mix(u64::from(slot.idx.0));
        hash_matrix(&mut hasher, &slot.base.global_transform_matrix);
        hasher.mix_bool(slot.visible);
        hasher.mix_bool(slot.display.is_some());
        // The blend mode selects the slot's shared material, so a change has to
        // mark the armature dirty.
        hasher.mix(u64::from(slot.blend_mode as u32));
        hash_color(&mut hasher, &slot.color_transform);
        hasher.mix_f64(slot.base.global_alpha);
        hasher.mix(slot_visual_hash(slot));
        // FFD/deform values are interpolated *into* the current display frame,
        // so they can change without the frame/texture content changing.
        if slot.geometry_data.is_some()
            && let Some(frame) = slot
                .display_frame
                .and_then(|index| slot.display_frames.get(index))
        {
            for value in &frame.deform_vertices {
                hasher.mix_f64(*value);
            }
        }
    }
    // Skinned meshes are rebuilt from bone matrices, and slot visibility depends
    // on the parent bone, so bone state is part of the pose too.
    for bone in &armature.bones {
        hash_matrix(&mut hasher, &bone.base.global_transform_matrix);
        hasher.mix_bool(bone.visible);
    }
    hasher.finish()
}
