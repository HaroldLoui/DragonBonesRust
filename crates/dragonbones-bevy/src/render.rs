use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::{AlphaMode2d, ColorMaterial};

use dragonbones_core::armature::{Armature, ArmatureIdx, BoneIdx, Slot};
use dragonbones_core::core::enums::binary_offset;
use dragonbones_core::model::{GeometryData, TextureData};

use crate::math::slot_color;
use crate::runtime::DragonBonesSlot;

/// What kind of geometry the adapter created for a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotRenderKind {
    /// Image or non-skinned mesh: the slot's own affine matrix applies.
    Local,
    /// Skinned mesh: vertices are rebuilt in armature space every frame.
    ArmatureSpace,
    /// No renderable geometry (empty slot / unsupported display).
    Empty,
}

/// One per-vertex bone influence of a skinned DragonBones mesh.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BoneWeight {
    pub bone: Option<BoneIdx>,
    pub weight: f32,
    pub x: f32,
    pub y: f32,
}

/// CPU-side data needed to update a skinned mesh every frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct WeightedMeshData {
    pub vertices: Vec<Vec<BoneWeight>>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// CPU-side data for an unskinned mesh whose vertices may be moved by a
/// DragonBones FFD (deform) timeline.
#[derive(Clone, Debug, Default)]
pub(crate) struct UnskinnedMeshData {
    pub base_positions: Vec<[f32; 3]>,
}

/// Adapter-side slot data needed to rebuild the mesh when the display frame
/// changes and to update skinned vertices every frame.
#[derive(Component)]
pub struct DragonBonesSlotVisual {
    pub kind: SlotRenderKind,
    pub atlas_texture: Handle<Image>,
    pub atlas_size: UVec2,
    pub last_display_frame: Option<usize>,
    pub(crate) weighted: Option<WeightedMeshData>,
    pub(crate) unskinned: Option<UnskinnedMeshData>,
}

/// Spawn one slot entity under the armature root.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_slot(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    root: Entity,
    armature_idx: ArmatureIdx,
    armature: &Armature,
    slot: &Slot,
    atlas_texture: &Handle<Image>,
    atlas_size: UVec2,
    armature_scale: f64,
) -> Entity {
    let (mesh, kind, weighted, unskinned) =
        build_slot_visual(slot, armature, atlas_size, armature_scale);
    let material = ColorMaterial {
        color: slot_color(slot),
        texture: Some(atlas_texture.clone()),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    };
    let material_handle = materials.add(material);
    commands
        .spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(material_handle),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(root),
            DragonBonesSlot {
                armature: armature_idx,
                slot: slot.idx,
            },
            DragonBonesSlotVisual {
                kind,
                atlas_texture: atlas_texture.clone(),
                atlas_size,
                last_display_frame: slot.display_frame,
                weighted,
                unskinned,
            },
        ))
        .id()
}

/// Build the mesh and any per-frame skinning data for a slot.
pub(crate) fn build_slot_visual(
    slot: &Slot,
    armature: &Armature,
    atlas_size: UVec2,
    armature_scale: f64,
) -> (
    Mesh,
    SlotRenderKind,
    Option<WeightedMeshData>,
    Option<UnskinnedMeshData>,
) {
    if slot.display.is_none() {
        return (empty_mesh(), SlotRenderKind::Empty, None, None);
    }

    if let Some(geometry) = &slot.geometry_data {
        let Some(texture) = slot.texture_data.as_ref() else {
            return (empty_mesh(), SlotRenderKind::Empty, None, None);
        };
        if geometry.weight.is_some() {
            let data = decode_weighted_mesh(slot, geometry, texture, atlas_size, armature_scale);
            let mesh = build_weighted_mesh(&data, armature);
            return (mesh, SlotRenderKind::ArmatureSpace, Some(data), None);
        }
        let (mesh, data) = build_unskinned_mesh(geometry, texture, atlas_size, armature_scale);
        return (mesh, SlotRenderKind::Local, None, Some(data));
    }

    if let Some(texture) = slot.texture_data.as_ref() {
        return (
            build_image_mesh(slot, texture, atlas_size),
            SlotRenderKind::Local,
            None,
            None,
        );
    }

    (empty_mesh(), SlotRenderKind::Empty, None, None)
}

/// Apply the current FFD/deform offsets to an unskinned mesh.
pub(crate) fn update_unskinned_positions(
    data: &UnskinnedMeshData,
    deform: &[f64],
    mesh: &mut Mesh,
) {
    if deform.len() < data.base_positions.len() * 2 {
        return;
    }
    let mut positions = Vec::with_capacity(data.base_positions.len());
    for (index, base) in data.base_positions.iter().enumerate() {
        positions.push([
            base[0] + deform[index * 2] as f32,
            base[1] + deform[index * 2 + 1] as f32,
            base[2],
        ]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
}

/// Recompute skinned vertex positions from the current bone matrices.
pub(crate) fn update_weighted_positions(
    data: &WeightedMeshData,
    armature: &Armature,
    mesh: &mut Mesh,
) {
    let mut positions = Vec::with_capacity(data.vertices.len());
    for influences in &data.vertices {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for influence in influences {
            let Some(bone_idx) = influence.bone else {
                continue;
            };
            let Some(bone) = armature.bones.get(bone_idx.index()) else {
                continue;
            };
            let matrix = bone.base.global_transform_matrix;
            let weight = influence.weight as f64;
            x += (matrix.a * influence.x as f64 + matrix.c * influence.y as f64 + matrix.tx)
                * weight;
            y += (matrix.b * influence.x as f64 + matrix.d * influence.y as f64 + matrix.ty)
                * weight;
        }
        positions.push([x as f32, y as f32, 0.0]);
    }
    // Replacing the attribute is safe here: this runs in PostUpdate before
    // render extraction, and the mesh is created with MAIN_WORLD usage.
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
}

fn build_weighted_mesh(data: &WeightedMeshData, armature: &Armature) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    let mut positions = Vec::with_capacity(data.vertices.len());
    for influences in &data.vertices {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for influence in influences {
            let Some(bone_idx) = influence.bone else {
                continue;
            };
            let Some(bone) = armature.bones.get(bone_idx.index()) else {
                continue;
            };
            let matrix = bone.base.global_transform_matrix;
            let weight = influence.weight as f64;
            x += (matrix.a * influence.x as f64 + matrix.c * influence.y as f64 + matrix.tx)
                * weight;
            y += (matrix.b * influence.x as f64 + matrix.d * influence.y as f64 + matrix.ty)
                * weight;
        }
        positions.push([x as f32, y as f32, 0.0]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
    mesh.insert_indices(Indices::U32(data.indices.clone()));
    mesh
}

fn empty_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0f32, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 0.0], [0.0, 0.0], [0.0, 0.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
    mesh
}

fn build_image_mesh(slot: &Slot, texture: &TextureData, atlas_size: UVec2) -> Mesh {
    let region = texture.region;
    let frame = texture.frame;
    // Core's pivot already includes the trimmed frame offset. Local vertices
    // are the content rectangle (region), shifted so the pivot is the origin.
    let content_x = frame.map_or(0.0, |frame| -frame.x);
    let content_y = frame.map_or(0.0, |frame| -frame.y);
    let x0 = (content_x - slot.pivot_x) as f32;
    let y0 = (content_y - slot.pivot_y) as f32;
    let x1 = x0 + region.width as f32;
    let y1 = y0 + region.height as f32;

    let (u0, v0, u1, v1) = uv_rect(texture, atlas_size);
    let positions = vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]];
    let uvs = if texture.rotated {
        vec![[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
    } else {
        vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
    };
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

fn build_unskinned_mesh(
    geometry: &GeometryData,
    texture: &TextureData,
    atlas_size: UVec2,
    armature_scale: f64,
) -> (Mesh, UnskinnedMeshData) {
    let Some(root) = geometry.root.as_ref() else {
        return (empty_mesh(), UnskinnedMeshData::default());
    };
    let ints = &root.ir.int_array;
    let floats = &root.ir.float_array;
    let geo = geometry.offset as usize;
    if geo + binary_offset::geometry::VERTEX_INDICES > ints.len() {
        return (empty_mesh(), UnskinnedMeshData::default());
    }
    let vertex_count = ints[geo + binary_offset::geometry::VERTEX_COUNT].max(0) as usize;
    let triangle_count = ints[geo + binary_offset::geometry::TRIANGLE_COUNT].max(0) as usize;
    if vertex_count == 0 || triangle_count == 0 {
        return (empty_mesh(), UnskinnedMeshData::default());
    }
    let float_offset = fix_i16_offset(ints[geo + binary_offset::geometry::FLOAT_OFFSET]);
    if float_offset + vertex_count * 2 > floats.len() {
        return (empty_mesh(), UnskinnedMeshData::default());
    }

    let mut positions = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let x = floats[float_offset + i * 2] * armature_scale;
        let y = floats[float_offset + i * 2 + 1] * armature_scale;
        positions.push([x as f32, y as f32, 0.0]);
    }

    let uv_offset = float_offset + vertex_count * 2;
    let (u_base, v_base, u_scale, v_scale) = uv_transform(texture, atlas_size);
    let mut uvs = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let u = floats.get(uv_offset + i * 2).copied().unwrap_or(0.0);
        let v = floats.get(uv_offset + i * 2 + 1).copied().unwrap_or(0.0);
        uvs.push([(u_base + u * u_scale) as f32, (v_base + v * v_scale) as f32]);
    }

    let index_start = geo + binary_offset::geometry::VERTEX_INDICES;
    let index_count = triangle_count * 3;
    if index_start + index_count > ints.len() {
        return (empty_mesh(), UnskinnedMeshData::default());
    }
    let mut indices = Vec::with_capacity(index_count);
    for i in 0..index_count {
        let idx = ints[index_start + i];
        if idx < 0 || idx as usize >= vertex_count {
            return (empty_mesh(), UnskinnedMeshData::default());
        }
        indices.push(idx as u32);
    }

    let data = UnskinnedMeshData {
        base_positions: positions.clone(),
    };
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, data)
}

fn decode_weighted_mesh(
    slot: &Slot,
    geometry: &GeometryData,
    texture: &TextureData,
    atlas_size: UVec2,
    armature_scale: f64,
) -> WeightedMeshData {
    let Some(root) = geometry.root.as_ref() else {
        return WeightedMeshData::default();
    };
    let Some(weight) = geometry.weight.as_ref() else {
        return WeightedMeshData::default();
    };
    let ints = &root.ir.int_array;
    let floats = &root.ir.float_array;
    let geo = geometry.offset as usize;
    let vertex_count = ints[geo + binary_offset::geometry::VERTEX_COUNT].max(0) as usize;
    let triangle_count = ints[geo + binary_offset::geometry::TRIANGLE_COUNT].max(0) as usize;
    let float_offset = fix_i16_offset(ints[geo + binary_offset::geometry::FLOAT_OFFSET]);
    let weight_offset = weight.offset as usize;
    let weight_float = fix_i16_offset(ints[weight_offset + binary_offset::weight::FLOAT_OFFSET]);
    let bone_count = weight.bones.len();
    let mut ib = weight_offset + binary_offset::weight::BONE_INDICES + bone_count;
    let mut iv = weight_float;

    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        let Some(&influence_count) = ints.get(ib) else {
            return WeightedMeshData::default();
        };
        ib += 1;
        let count = influence_count.max(0) as usize;
        let mut influences = Vec::with_capacity(count);
        for _ in 0..count {
            let Some(&bone_slot) = ints.get(ib) else {
                return WeightedMeshData::default();
            };
            ib += 1;
            let Some(&w) = floats.get(iv) else {
                return WeightedMeshData::default();
            };
            let Some(&x) = floats.get(iv + 1) else {
                return WeightedMeshData::default();
            };
            let Some(&y) = floats.get(iv + 2) else {
                return WeightedMeshData::default();
            };
            iv += 3;
            let bone = if bone_slot < 0 {
                None
            } else {
                slot.geometry_bones
                    .get(bone_slot as usize)
                    .copied()
                    .flatten()
            };
            influences.push(BoneWeight {
                bone,
                weight: w as f32,
                x: (x * armature_scale) as f32,
                y: (y * armature_scale) as f32,
            });
        }
        vertices.push(influences);
    }

    // UVs follow the bind vertices, not the weight float block.
    let uv_offset = float_offset + vertex_count * 2;
    let (u_base, v_base, u_scale, v_scale) = uv_transform(texture, atlas_size);
    let mut uvs = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let u = floats.get(uv_offset + i * 2).copied().unwrap_or(0.0);
        let v = floats.get(uv_offset + i * 2 + 1).copied().unwrap_or(0.0);
        uvs.push([(u_base + u * u_scale) as f32, (v_base + v * v_scale) as f32]);
    }

    let index_start = geo + binary_offset::geometry::VERTEX_INDICES;
    let mut indices = Vec::with_capacity(triangle_count * 3);
    for i in 0..triangle_count * 3 {
        let Some(&idx) = ints.get(index_start + i) else {
            return WeightedMeshData::default();
        };
        if idx < 0 || idx as usize >= vertex_count {
            return WeightedMeshData::default();
        }
        indices.push(idx as u32);
    }

    WeightedMeshData {
        vertices,
        uvs,
        indices,
    }
}

fn uv_rect(texture: &TextureData, atlas_size: UVec2) -> (f32, f32, f32, f32) {
    let width = atlas_size.x.max(1) as f32;
    let height = atlas_size.y.max(1) as f32;
    let x0 = texture.region.x as f32 / width;
    let y0 = texture.region.y as f32 / height;
    let x1 = (texture.region.x + texture.region.width) as f32 / width;
    let y1 = (texture.region.y + texture.region.height) as f32 / height;
    (x0, y0, x1, y1)
}

fn uv_transform(texture: &TextureData, atlas_size: UVec2) -> (f64, f64, f64, f64) {
    let width = atlas_size.x.max(1) as f64;
    let height = atlas_size.y.max(1) as f64;
    (
        texture.region.x / width,
        texture.region.y / height,
        texture.region.width / width,
        texture.region.height / height,
    )
}

fn fix_i16_offset(value: i32) -> usize {
    if value < 0 {
        (value + 65536) as usize
    } else {
        value as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dragonbones_core::armature::NullArmatureProxy;
    use dragonbones_core::factory::BaseFactory;
    use serde_json::Value;
    use std::path::PathBuf;

    fn assets_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    fn build_factory(
        ske_name: &str,
        tex_name: &str,
        armature_name: &str,
    ) -> (BaseFactory, ArmatureIdx) {
        let dir = assets_dir();
        let ske = std::fs::read_to_string(dir.join(ske_name)).expect("read skeleton");
        let tex = std::fs::read_to_string(dir.join(tex_name)).expect("read atlas");
        let ske: Value = serde_json::from_str(&ske).expect("parse skeleton");
        let tex: Value = serde_json::from_str(&tex).expect("parse atlas");

        let mut factory = BaseFactory::new();
        factory
            .parse_dragon_bones_data(&ske, "db")
            .expect("skeleton");
        assert!(factory.parse_texture_atlas_data(&tex, "db"));
        let idx = factory
            .build_armature(
                armature_name,
                "db",
                "",
                "db",
                Box::new(NullArmatureProxy::new()),
            )
            .expect("armature");
        factory.advance_one(idx, 0.0);
        (factory, idx)
    }

    #[test]
    fn dragon_image_slot_builds_quad_mesh() {
        let (factory, idx) = build_factory("Dragon_ske.json", "Dragon_tex.json", "Dragon");
        let armature = factory.armatures[idx.index()].as_ref().unwrap();
        let slot = armature
            .slots
            .iter()
            .find(|slot| slot.geometry_data.is_none() && slot.texture_data.is_some())
            .expect("image slot");
        let (mesh, kind, weighted, unskinned) =
            build_slot_visual(slot, armature, UVec2::new(1024, 1024), 1.0);
        assert_eq!(kind, SlotRenderKind::Local);
        assert!(weighted.is_none());
        assert!(unskinned.is_none());
        assert_eq!(mesh.count_vertices(), 4);
    }

    #[test]
    fn rooster_weighted_slot_decodes_and_builds() {
        let (factory, idx) = build_factory(
            "Rooster_Ani_ske.json",
            "Rooster_Ani_tex.json",
            "armatureName",
        );
        let armature = factory.armatures[idx.index()].as_ref().unwrap();
        let slot = armature
            .slots
            .iter()
            .find(|slot| {
                slot.geometry_data
                    .as_ref()
                    .is_some_and(|g| g.weight.is_some())
            })
            .expect("weighted mesh slot");
        let scale = armature.armature_data().scale;
        let (mesh, kind, weighted, unskinned) =
            build_slot_visual(slot, armature, UVec2::new(512, 1024), scale);
        assert_eq!(kind, SlotRenderKind::ArmatureSpace);
        assert!(unskinned.is_none());
        let weighted = weighted.expect("weighted data");
        assert!(!weighted.vertices.is_empty());
        assert!(!weighted.indices.is_empty());
        assert!(mesh.count_vertices() > 0);
    }
}
