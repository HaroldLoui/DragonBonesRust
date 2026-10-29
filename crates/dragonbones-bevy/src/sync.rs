use std::collections::{HashMap, HashSet};

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use dragonbones_core::armature::{Armature, ArmatureIdx, SlotIdx};
use dragonbones_core::geom::{ColorTransform, Matrix};

use crate::blend::DragonBonesBlend;
use crate::material::{DragonBonesMaterial, DragonBonesMaterials, MaterialKey};
use crate::math::{matrix_to_affine3a, slot_color, slot_z, write_global_transform};
use crate::render::{
    DragonBonesSlotVisual, SlotRenderKind, apply_vertex_color, build_slot_visual,
    update_unskinned_positions, update_weighted_positions,
};
use crate::runtime::{
    DragonBonesInstance, DragonBonesRuntime, DragonBonesSlot, DragonBonesSyncConfig,
};

/// B2 transform/color/visibility sync.
///
/// Runs in `PostUpdate` after Bevy's `TransformSystems::Propagate` and before
/// visibility/rendering. A slot's `GlobalTransform` is written by this system
/// only, but Bevy's propagation would overwrite it with the placeholder
/// `Transform` whenever the armature root moves, so a clean armature still has
/// to be re-synced when its root transform changes.
///
/// Two levels of work avoidance:
///
/// 1. **armature level** (see [`DragonBonesSyncConfig`]): each frame the adapter
///    hashes the armature's pose and compares it with the root affine against the
///    values stored in the armature binding. Idle armatures (paused animation,
///    static props) skip the whole per-slot pass.
/// 2. **slot level**: `armature.slots` is indexed through a [`SlotIdx`] → order
///    map built once per armature per frame instead of a linear `find` per slot
///    (the previous MVP was O(slots²)).
///
/// The pose hash deliberately covers every input the slot pass reads (slot
/// order, slot matrices, display frame, visibility, colors, blend mode, bone
/// matrices for skinned meshes, interpolated deform vertices) instead of
/// relying on a dirty flag that every mutation site would have to remember to
/// set.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_dragonbones(
    config: Res<DragonBonesSyncConfig>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    roots: Query<(&DragonBonesInstance, &GlobalTransform), Without<DragonBonesSlot>>,
    mut slots: Query<(
        &DragonBonesSlot,
        &mut GlobalTransform,
        &mut Visibility,
        &Mesh2d,
        &mut MeshMaterial2d<DragonBonesMaterial>,
        &mut DragonBonesSlotVisual,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DragonBonesMaterial>>,
    mut material_cache: ResMut<DragonBonesMaterials>,
) {
    let mut root_affines: HashMap<ArmatureIdx, Affine3A> = HashMap::new();
    let mut slot_orders: HashMap<ArmatureIdx, HashMap<SlotIdx, usize>> = HashMap::new();
    let mut dirty: HashSet<ArmatureIdx> = HashSet::new();
    // Snapshot of the material keys each dirty armature already holds, so a
    // runtime blend-mode change only takes a new cache reference when the
    // armature did not acquire that `(image, blend)` pair yet.
    let mut held_materials: HashMap<ArmatureIdx, HashSet<MaterialKey>> = HashMap::new();
    // Newly acquired keys, committed to the armature binding after the slot pass.
    let mut new_materials: HashMap<ArmatureIdx, HashSet<MaterialKey>> = HashMap::new();

    for (instance, global) in &roots {
        let armature_idx = instance.armature;
        let root_affine = global.affine();
        root_affines.insert(armature_idx, root_affine);

        let clean = if config.skip_clean_armatures {
            let pose = {
                let Some(armature) = runtime
                    .factory
                    .armatures
                    .get(armature_idx.index())
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                pose_hash(armature)
            };
            let Some(binding) = runtime.bindings.get_mut(&armature_idx) else {
                continue;
            };
            if binding.last_root == Some(root_affine) && binding.last_pose == Some(pose) {
                true
            } else {
                binding.last_root = Some(root_affine);
                binding.last_pose = Some(pose);
                false
            }
        } else {
            // Invalidate the cached pose so flipping the switch back on cannot
            // skip a frame that the disabled path left out of date.
            if let Some(binding) = runtime.bindings.get_mut(&armature_idx) {
                binding.last_root = None;
                binding.last_pose = None;
            }
            false
        };
        if clean {
            continue;
        }

        let Some(armature) = runtime
            .factory
            .armatures
            .get(armature_idx.index())
            .and_then(Option::as_ref)
        else {
            continue;
        };
        // Core re-sorts `armature.slots` while playing zOrder/zIndex timelines,
        // so the order map is rebuilt for every dirty armature every frame.
        slot_orders.insert(
            armature_idx,
            armature
                .slots
                .iter()
                .enumerate()
                .map(|(order, slot)| (slot.idx, order))
                .collect(),
        );
        if let Some(binding) = runtime.bindings.get(&armature_idx) {
            held_materials.insert(armature_idx, binding.materials.clone());
        }
        dirty.insert(armature_idx);
    }

    for (slot_ref, mut global, mut visibility, mesh_handle, mut material, mut visual) in &mut slots
    {
        if !dirty.contains(&slot_ref.armature) {
            continue;
        }
        let Some(root_affine) = root_affines.get(&slot_ref.armature).copied() else {
            continue;
        };
        let Some(order) = slot_orders
            .get(&slot_ref.armature)
            .and_then(|orders| orders.get(&slot_ref.slot))
            .copied()
        else {
            continue;
        };
        let Some(armature) = runtime
            .factory
            .armatures
            .get(slot_ref.armature.index())
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let Some(slot) = armature.slots.get(order) else {
            continue;
        };
        debug_assert_eq!(slot.idx, slot_ref.slot);

        let world = match visual.kind {
            SlotRenderKind::Local => {
                root_affine * matrix_to_affine3a(&slot.base.global_transform_matrix, 0.0)
            }
            SlotRenderKind::ArmatureSpace | SlotRenderKind::Empty => root_affine,
        };
        let mut world = world;
        world.translation.z = root_affine.translation.z + slot_z(order);
        write_global_transform(&mut global, world);

        let parent_visible = armature
            .bones
            .get(slot.parent.index())
            .is_none_or(|bone| bone.visible);
        let visible = slot.visible && parent_visible && slot.display.is_some();
        let next_visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != next_visibility {
            *visibility = next_visibility;
        }

        // The slot's blend mode is part of the pose hash, so a runtime change
        // marks the armature dirty and lands here: swap to the shared material
        // of the new `(atlas page, blend)` pair, taking a cache reference for
        // this armature the first time it uses that pair.
        let blend = DragonBonesBlend::from_core(slot.blend_mode);
        if visual.blend != blend
            && let Some(held_keys) = held_materials.get(&slot_ref.armature)
        {
            let key = MaterialKey::new(&visual.atlas_image, blend);
            let next = if held_keys.contains(&key) {
                material_cache.handle(key)
            } else {
                let handle = material_cache.acquire(&visual.atlas_image, blend, &mut materials);
                new_materials
                    .entry(slot_ref.armature)
                    .or_default()
                    .insert(key);
                Some(handle)
            };
            if let Some(handle) = next {
                *material = MeshMaterial2d(handle);
                visual.blend = blend;
            }
        }

        // Per-slot tint lives in the mesh vertex color attribute, so every slot
        // of an atlas page shares one material.
        let color = slot_color(slot);
        if visual.last_display_frame != slot.display_frame {
            // `build_slot_visual` already bakes `color` into vertex colors.
            let (mesh, kind, weighted, unskinned) = build_slot_visual(
                slot,
                armature,
                visual.atlas_size,
                armature.armature_data().scale,
            );
            if let Some(mut target) = meshes.get_mut(&mesh_handle.0) {
                *target = mesh;
            }
            visual.kind = kind;
            visual.last_display_frame = slot.display_frame;
            visual.color = color;
            visual.weighted = weighted;
            visual.unskinned = unskinned;
        } else if visual.kind == SlotRenderKind::ArmatureSpace
            && let Some(weighted) = visual.weighted.as_ref()
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            update_weighted_positions(weighted, armature, &mut mesh);
        } else if visual.kind == SlotRenderKind::Local
            && let Some(unskinned) = visual.unskinned.as_ref()
            && let Some(frame) = slot
                .display_frame
                .and_then(|index| slot.display_frames.get(index))
            && !frame.deform_vertices.is_empty()
            && slot
                .geometry_data
                .as_ref()
                .is_some_and(|geometry| geometry.inherit_deform)
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            update_unskinned_positions(unskinned, &frame.deform_vertices, &mut mesh);
        }

        if visual.color != color
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            apply_vertex_color(&mut mesh, color);
            visual.color = color;
        }
    }

    // Commit the keys the slot pass acquired. Old keys stay acquired until the
    // armature is disposed, which keeps the cache refcounts balanced without a
    // second per-slot bookkeeping pass (an armature can hold at most one key
    // per blend mode).
    for (armature_idx, keys) in new_materials {
        if let Some(binding) = runtime.bindings.get_mut(&armature_idx) {
            binding.materials.extend(keys);
        }
    }
}

/// FNV-1a based pose hash used by the armature-level dirty check.
struct PoseHasher(u64);

/// FNV-1a 64-bit offset basis.
const POSE_HASH_SEED: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const POSE_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

impl PoseHasher {
    fn new() -> Self {
        Self(POSE_HASH_SEED)
    }

    fn mix(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(POSE_HASH_PRIME);
    }

    fn mix_f64(&mut self, value: f64) {
        self.mix(value.to_bits());
    }

    fn mix_bool(&mut self, value: bool) {
        self.mix(u64::from(value));
    }

    fn mix_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }

    fn mix_option_usize(&mut self, value: Option<usize>) {
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

fn hash_matrix(hasher: &mut PoseHasher, matrix: &Matrix) {
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

/// Hash of everything [`sync_dragonbones`] reads from the core armature.
///
/// Slot iteration order is part of the hash (the draw order feeds the z write),
/// so re-sorting `armature.slots` marks the armature dirty as well.
fn pose_hash(armature: &Armature) -> u64 {
    let mut hasher = PoseHasher::new();
    for slot in &armature.slots {
        hasher.mix(u64::from(slot.idx.0));
        hash_matrix(&mut hasher, &slot.base.global_transform_matrix);
        hasher.mix_option_usize(slot.display_frame);
        hasher.mix_bool(slot.visible);
        hasher.mix_bool(slot.display.is_some());
        // The blend mode selects the slot's shared material, so a change has to
        // mark the armature dirty.
        hasher.mix(u64::from(slot.blend_mode as u32));
        hash_color(&mut hasher, &slot.color_transform);
        hasher.mix_f64(slot.base.global_alpha);

        // FFD/deform values are interpolated *into* the current display frame,
        // so they can change without the frame index changing.
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
