use std::collections::{HashMap, HashSet};

use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use dragonbones_core::armature::{ArmatureIdx, SlotIdx};
use dragonbones_core::factory::BaseFactory;

use crate::blend::DragonBonesBlend;
use crate::hash::{armature_pose_hash, slot_visual_hash};
use crate::material::{DragonBonesMaterial, DragonBonesMaterials, MaterialKey};
use crate::math::{matrix_to_affine3a, slot_color, slot_z, write_global_transform};
use crate::render::{
    DragonBonesSlotVisual, SlotRenderKind, apply_vertex_color, build_slot_visual, slot_deform,
    update_surface_positions, update_unskinned_positions, update_weighted_positions,
};
use crate::runtime::{
    DragonBonesChildArmature, DragonBonesInstance, DragonBonesRuntime, DragonBonesSlot,
    DragonBonesSyncConfig,
};

/// B2 transform/color/visibility sync.
///
/// Runs in `PostUpdate` after Bevy's `TransformSystems::Propagate` and before
/// `VisibilitySystems::VisibilityPropagate`/visibility checks/rendering. A slot's
/// `GlobalTransform` is written by this system only, but Bevy's propagation would
/// overwrite it with the placeholder `Transform` whenever the armature root
/// moves, so a clean armature still has to be re-synced when its root transform
/// changes.
///
/// Visibility follows Bevy's hierarchy instead of writing absolute
/// `Visibility::Visible` values: a visible slot gets `Visibility::Inherited` and
/// a hidden slot gets `Visibility::Hidden`, so hiding the armature root (or any
/// ancestor) hides the whole subtree like any other Bevy entity. The root's own
/// `Visibility` is part of the armature-level dirty check, so a `Visibility`
/// toggle resyncs the slots with the concrete inherited/hidden values.
///
/// Two levels of work avoidance:
///
/// 1. **armature level** (see [`DragonBonesSyncConfig`]): each frame the adapter
///    hashes the armature's pose and compares it with the root affine against the
///    values stored in the armature binding. Idle armatures (paused animation,
///    static props) skip the whole per-slot pass.
/// 2. **slot level**: `armature.slots` is indexed through a [`SlotIdx`] → order
///    map built once per *dirty* armature per frame instead of a linear `find`
///    per slot (the previous MVP was O(slots²)).
///
/// Child-armature displays are resolved first: a child armature's world matrix
/// is the owning slot's world matrix, and it is only visible when it is the
/// slot's *current* display (`slot.child_armature`), matching the reference
/// implementation where a slot shows exactly one display at a time.
///
/// The pose hash deliberately covers every input the slot pass reads (slot
/// order, slot matrices, display frame + display data/texture content,
/// visibility, colors, blend mode, bone matrices for skinned meshes,
/// interpolated deform vertices) instead of relying on a dirty flag that every
/// mutation site would have to remember to set.
/// Reused per-frame scratch containers for [`sync_dragonbones`].
///
/// The system used to allocate a dozen `HashMap`/`HashSet`/`Vec` values every
/// frame (even for clean armatures); they are now kept in a `Local` and cleared
/// in place so the capacity survives across frames.
#[derive(Default)]
pub(crate) struct SyncScratch {
    slot_orders: HashMap<ArmatureIdx, HashMap<SlotIdx, usize>>,
    top_affines: HashMap<ArmatureIdx, Affine3A>,
    top_visible: HashMap<ArmatureIdx, bool>,
    child_links: Vec<(ArmatureIdx, Entity, ArmatureIdx, Entity)>,
    parent_slot_info: HashMap<Entity, (ArmatureIdx, SlotIdx)>,
    resolved: HashMap<ArmatureIdx, (Affine3A, bool)>,
    dirty: HashSet<ArmatureIdx>,
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn sync_dragonbones(
    mut scratch: Local<SyncScratch>,
    config: Res<DragonBonesSyncConfig>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    mut roots: Query<
        (
            Entity,
            &DragonBonesInstance,
            &mut GlobalTransform,
            &mut Visibility,
            Option<&DragonBonesChildArmature>,
        ),
        Without<DragonBonesSlot>,
    >,
    mut slots: Query<(
        Entity,
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
    let (factory, bindings) = runtime.split_mut();
    let SyncScratch {
        slot_orders,
        top_affines,
        top_visible,
        child_links,
        parent_slot_info,
        resolved,
        dirty,
    } = &mut *scratch;
    slot_orders.clear();
    top_affines.clear();
    top_visible.clear();
    child_links.clear();
    parent_slot_info.clear();
    resolved.clear();
    dirty.clear();

    // 1) Top-level armature affines + effective root visibility, and the parent
    //    links of every child-armature root.
    for (entity, instance, global, visibility, child) in &mut roots {
        let armature_idx = instance.armature;
        match child {
            Some(child) => {
                child_links.push((armature_idx, entity, child.parent, child.parent_slot))
            }
            None => {
                top_affines.insert(armature_idx, global.affine());
                // Hiding the root has to hide the armature; ancestors are handled
                // by Bevy's visibility propagation over the slot `Inherited`
                // values. `Visible`/`Inherited` both mean "not hidden here".
                let visible = !matches!(*visibility, Visibility::Hidden);
                top_visible.insert(armature_idx, visible);
            }
        }
    }

    // 2) Child-armature resolution (skipped entirely for the common no-child
    //    case, which keeps idle armatures cheap).
    if !child_links.is_empty() {
        let parents: HashSet<ArmatureIdx> = child_links.iter().map(|link| link.2).collect();
        for parent in parents {
            if let Some(orders) = slot_order_map(factory, parent) {
                slot_orders.insert(parent, orders);
            }
        }
        // Slot identity of the parent slots used by child armatures.
        let wanted: HashSet<Entity> = child_links.iter().map(|link| link.3).collect();
        for (entity, slot_ref, _global, _visibility, _mesh, _material, _visual) in &mut slots {
            if wanted.contains(&entity) {
                parent_slot_info.insert(entity, (slot_ref.armature, slot_ref.slot));
            }
        }
    }
    resolved.extend(top_affines.iter().map(|(armature, affine)| {
        (
            *armature,
            (*affine, top_visible.get(armature).copied().unwrap_or(true)),
        )
    }));
    let mut remaining = std::mem::take(child_links);
    while !remaining.is_empty() {
        let mut progressed = false;
        let mut next = Vec::with_capacity(remaining.len());
        for (child_idx, child_root, parent_idx, parent_slot) in remaining {
            let Some(&(parent_affine, parent_visible)) = resolved.get(&parent_idx) else {
                next.push((child_idx, child_root, parent_idx, parent_slot));
                continue;
            };
            let Some(&(slot_armature, slot_idx)) = parent_slot_info.get(&parent_slot) else {
                continue;
            };
            let Some(order) = slot_orders
                .get(&slot_armature)
                .and_then(|orders| orders.get(&slot_idx))
                .copied()
            else {
                continue;
            };
            let Some(armature) = factory
                .armatures
                .get(slot_armature.index())
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(slot) = armature.slots.get(order) else {
                continue;
            };
            let mut slot_world =
                parent_affine * matrix_to_affine3a(&slot.base.global_transform_matrix, 0.0);
            slot_world.translation.z = parent_affine.translation.z + slot_z(order);
            let parent_bone_visible = armature
                .bones
                .get(slot.parent.index())
                .is_none_or(|bone| bone.visible);
            // Official semantics: a slot displays exactly one display at a time,
            // so only the currently selected child armature may be visible.
            let active = slot.child_armature == Some(child_idx);
            let slot_visible =
                slot.visible && parent_bone_visible && slot.display.is_some() && active;
            resolved.insert(child_idx, (slot_world, parent_visible && slot_visible));
            progressed = true;
        }
        if !progressed {
            break;
        }
        remaining = next;
    }

    // 3) Drive child-armature roots: world matrix = owning slot's world matrix,
    //    visibility inherited through the parent chain.
    for (_entity, instance, mut global, mut visibility, child) in &mut roots {
        if child.is_none() {
            continue;
        }
        let Some(&(affine, visible)) = resolved.get(&instance.armature) else {
            continue;
        };
        write_global_transform(&mut global, affine);
        let next_visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next_visibility {
            *visibility = next_visibility;
        }
    }

    // 4) Armature-level dirty check, then slot order maps for dirty armatures.
    for (_entity, instance, global, visibility, child) in &mut roots {
        let armature_idx = instance.armature;
        let (affine, visible) = if child.is_some() {
            match resolved.get(&armature_idx) {
                Some(&(affine, visible)) => (affine, visible),
                None => continue,
            }
        } else {
            (global.affine(), !matches!(*visibility, Visibility::Hidden))
        };
        let Some(armature) = factory
            .armatures
            .get(armature_idx.index())
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let pose = armature_pose_hash(armature);
        let Some(binding) = bindings.get_mut(&armature_idx) else {
            continue;
        };
        let clean = if config.skip_clean_armatures {
            if binding.last_root == Some(affine)
                && binding.last_pose == Some(pose)
                && binding.last_visible == Some(visible)
            {
                true
            } else {
                binding.last_root = Some(affine);
                binding.last_pose = Some(pose);
                binding.last_visible = Some(visible);
                false
            }
        } else {
            // Invalidate the cached pose so flipping the switch back on cannot
            // skip a frame that the disabled path left out of date.
            binding.last_root = None;
            binding.last_pose = None;
            binding.last_visible = None;
            false
        };
        if !clean {
            dirty.insert(armature_idx);
        }
    }
    for armature_idx in dirty.iter().copied().collect::<Vec<_>>() {
        if let Some(orders) = slot_order_map(factory, armature_idx) {
            slot_orders.insert(armature_idx, orders);
        }
    }

    // 5) Per-slot pass for dirty armatures.
    for (_entity, slot_ref, mut global, mut visibility, mesh_handle, mut material, mut visual) in
        &mut slots
    {
        if !dirty.contains(&slot_ref.armature) {
            continue;
        }
        let Some(&(root_affine, armature_visible)) = resolved.get(&slot_ref.armature) else {
            continue;
        };
        let Some(order) = slot_orders
            .get(&slot_ref.armature)
            .and_then(|orders| orders.get(&slot_ref.slot))
            .copied()
        else {
            continue;
        };
        let Some(armature) = factory
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
        let visible = armature_visible && slot.visible && parent_visible && slot.display.is_some();
        // Visible slots inherit (so hiding an ancestor hides them); hidden slots
        // are explicit so they stay hidden even when the root is visible.
        let next_visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != next_visibility {
            *visibility = next_visibility;
        }

        // The slot's blend mode is part of the pose hash, so a runtime change
        // marks the armature dirty and lands here: swap to the shared material
        // of the new `(atlas page, blend)` pair. `binding.materials` is updated
        // immediately, so several slots switching to the same new blend in one
        // frame only acquire the shared material once.
        let blend = DragonBonesBlend::from_core(slot.blend_mode);
        if visual.blend != blend {
            let key = MaterialKey::new(&visual.atlas_image, blend);
            let held = bindings
                .get(&slot_ref.armature)
                .is_some_and(|binding| binding.materials.contains(&key));
            let next = if held {
                material_cache.handle(key)
            } else {
                let handle = material_cache.acquire(&visual.atlas_image, blend, &mut materials);
                if let Some(binding) = bindings.get_mut(&slot_ref.armature) {
                    binding.materials.insert(key);
                }
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
        let visual_hash = slot_visual_hash(slot);
        // Rebuild when the display frame changes *or* the frame content changed
        // without an index change (e.g. `replace_texture_data` / `replace_skin`).
        if visual.last_display_frame != slot.display_frame || visual.content_hash != visual_hash {
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
            visual.content_hash = visual_hash;
            visual.color = color;
            visual.weighted = weighted;
            visual.unskinned = unskinned;
        } else if visual.kind == SlotRenderKind::ArmatureSpace
            && let Some(weighted) = visual.weighted.as_mut()
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            update_weighted_positions(weighted, armature, slot_deform(slot), &mut mesh);
        } else if visual.kind == SlotRenderKind::ArmatureSpace
            && let Some(unskinned) = visual.unskinned.as_mut()
            && unskinned.surface.is_some()
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            update_surface_positions(unskinned, armature, slot_deform(slot), &mut mesh);
        } else if visual.kind == SlotRenderKind::Local
            && let Some(unskinned) = visual.unskinned.as_mut()
            && !slot_deform(slot).is_empty()
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            update_unskinned_positions(unskinned, slot_deform(slot), &mut mesh);
        }

        if visual.color != color
            && let Some(mut mesh) = meshes.get_mut(&mesh_handle.0)
        {
            apply_vertex_color(&mut mesh, color);
            visual.color = color;
        }
    }
}

/// `SlotIdx` → position in `armature.slots` for one armature.
///
/// Core re-sorts `armature.slots` while playing zOrder/zIndex timelines, so the
/// map is rebuilt when the armature is dirty (or, for a parent slot of a child
/// armature, every frame it is needed).
fn slot_order_map(
    factory: &BaseFactory,
    armature_idx: ArmatureIdx,
) -> Option<HashMap<SlotIdx, usize>> {
    let armature = factory
        .armatures
        .get(armature_idx.index())
        .and_then(Option::as_ref)?;
    Some(
        armature
            .slots
            .iter()
            .enumerate()
            .map(|(order, slot)| (slot.idx, order))
            .collect(),
    )
}
