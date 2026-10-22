use std::collections::HashMap;

use bevy::prelude::*;
use bevy::sprite_render::ColorMaterial;

use crate::math::{matrix_to_affine3a, slot_color, slot_z, write_global_transform};
use crate::render::{
    DragonBonesSlotVisual, SlotRenderKind, build_slot_visual, update_unskinned_positions,
    update_weighted_positions,
};
use crate::runtime::{DragonBonesInstance, DragonBonesRuntime, DragonBonesSlot};

/// B2 transform/color/visibility sync.
///
/// Runs in `PostUpdate` after Bevy's `TransformSystems::Propagate` and before
/// visibility/rendering. Every slot is written every frame: moving the armature
/// root makes Bevy rewrite all child `GlobalTransform`s from placeholder
/// `Transform`s, so a dirty-only optimization here would leave stale slots.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_dragonbones(
    runtime: NonSend<DragonBonesRuntime>,
    roots: Query<(&DragonBonesInstance, &GlobalTransform), Without<DragonBonesSlot>>,
    mut slots: Query<(
        &DragonBonesSlot,
        &mut GlobalTransform,
        &mut Visibility,
        &Mesh2d,
        &MeshMaterial2d<ColorMaterial>,
        &mut DragonBonesSlotVisual,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mut root_affines: HashMap<_, _> = HashMap::new();
    for (instance, global) in &roots {
        root_affines.insert(instance.armature, global.affine());
    }

    for (slot_ref, mut global, mut visibility, mesh_handle, material_handle, mut visual) in
        &mut slots
    {
        let Some(root_affine) = root_affines.get(&slot_ref.armature).copied() else {
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
        let Some((order, slot)) = armature
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.idx == slot_ref.slot)
        else {
            continue;
        };

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

        if let Some(mut material) = materials.get_mut(&material_handle.0) {
            let color = slot_color(slot);
            if material.color != color {
                material.color = color;
            }
            let texture = Some(visual.atlas_texture.clone());
            if material.texture != texture {
                material.texture = texture;
            }
        }

        if visual.last_display_frame != slot.display_frame {
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
    }
}
