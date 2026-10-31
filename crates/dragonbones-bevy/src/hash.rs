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

use dragonbones_core::armature::{Armature, DisplayHandle, Slot};
use dragonbones_core::geom::{ColorTransform, Matrix};
use dragonbones_core::model::{DisplayData, DisplayKind, TextureData};

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

/// Hashes a resolved texture region identity (region + rotated + trim frame).
///
/// Texture *names* are intentionally not hashed: the adapter draws every slot of
/// an armature from one atlas page, so a rename that keeps the region does not
/// change what has to be drawn. This keeps the per-frame fingerprint numeric.
fn hash_texture_data(hasher: &mut PoseHasher, texture: &TextureData) {
    hasher.mix(1);
    hasher.mix_bool(texture.rotated);
    hasher.mix_f64(texture.region.x);
    hasher.mix_f64(texture.region.y);
    hasher.mix_f64(texture.region.width);
    hasher.mix_f64(texture.region.height);
    match &texture.frame {
        Some(frame) => {
            hasher.mix(1);
            hasher.mix_f64(frame.x);
            hasher.mix_f64(frame.y);
            hasher.mix_f64(frame.width);
            hasher.mix_f64(frame.height);
        }
        None => hasher.mix(0),
    }
}

/// Hashes the display kind backing a display frame.
///
/// Display names/paths are not hashed for the same reason as texture names: they
/// do not influence the adapter's draw data.
fn hash_display_data(hasher: &mut PoseHasher, display: &DisplayData) {
    hasher.mix(1);
    match &display.kind {
        DisplayKind::Image { .. } => hasher.mix(1),
        DisplayKind::Mesh { .. } => hasher.mix(2),
        DisplayKind::Armature { .. } => hasher.mix(3),
        DisplayKind::BoundingBox { .. } => hasher.mix(4),
        DisplayKind::Path { .. } => hasher.mix(5),
    }
}

/// Fingerprint of everything that determines the slot's *geometry/mesh content*.
///
/// Deform values are deliberately excluded: they are applied by the per-frame
/// deform update paths, not by rebuilding the mesh. Display-frame indices are
/// included, while the pose hash below also includes the interpolated deform.
pub(crate) fn slot_visual_hash(slot: &Slot) -> u64 {
    let mut hasher = PoseHasher::new();
    hasher.mix_usize(slot.display_frames.len());
    hasher.mix_option_usize(slot.display_frame);
    hasher.mix_f64(slot.pivot_x);
    hasher.mix_f64(slot.pivot_y);
    match slot.child_armature {
        Some(child) => {
            hasher.mix(1);
            hasher.mix_usize(child.index());
        }
        None => hasher.mix(0),
    }
    match slot.geometry_data.as_ref() {
        Some(geometry) => {
            hasher.mix(1);
            hasher.mix(u64::from(geometry.offset as u32));
            hasher.mix_bool(geometry.inherit_deform);
            hasher.mix_bool(geometry.weight.is_some());
        }
        None => hasher.mix(0),
    }
    if let Some(frame) = slot
        .display_frame
        .and_then(|index| slot.display_frames.get(index))
    {
        match &frame.texture_data {
            Some(texture) => hash_texture_data(&mut hasher, texture),
            None => hasher.mix(0),
        }
        match &frame.raw_display_data {
            Some(display) => hash_display_data(&mut hasher, display),
            None => hasher.mix(0),
        }
        match &frame.display_data {
            Some(display) => hash_display_data(&mut hasher, display),
            None => hasher.mix(0),
        }
        match &frame.display {
            Some(DisplayHandle::Armature(child)) => {
                hasher.mix(3);
                hasher.mix_usize(child.index());
            }
            Some(DisplayHandle::Mesh) => hasher.mix(2),
            Some(DisplayHandle::Raw) => hasher.mix(1),
            Some(DisplayHandle::Null) | None => hasher.mix(0),
        }
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
