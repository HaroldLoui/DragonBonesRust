//! Shared custom [`Material2d`] cache for the Bevy adapter.
//!
//! DragonBones slots need a per-slot tint and a per-slot *blend mode*; neither
//! justifies one material per slot. The adapter therefore keeps
//! **one material per `(atlas page image, blend mode)` pair** and bakes the
//! per-slot color into the mesh vertex color attribute
//! ([`Mesh::ATTRIBUTE_COLOR`]), exactly like the previous `ColorMaterial`-based
//! implementation did for colors.
//!
//! [`DragonBonesMaterial`] is a small custom `Material2d`:
//!
//! - its only binding is the atlas page texture (`binding(0)`) plus sampler
//!   (`binding(1)`);
//! - the blend mode is *not* a uniform: it lives in the material asset and is
//!   forwarded to the pipeline key through `#[bind_group_data(DragonBonesBlend)]`
//!   (see [`DragonBonesMaterial::specialize`]), so Bevy compiles one pipeline
//!   per blend mode while all slots sharing `(image, blend)` share one material
//!   asset, bind group and draw batch.
//!
//! The cache is reference counted per `(image, blend)`: `acquire` when an
//! armature is built, `release` when its root entity is despawned, and the
//! material asset is removed with the last user.

use std::collections::HashMap;

use bevy::asset::{Asset, AssetId, AssetPath, Handle, embedded_asset, embedded_path};
use bevy::image::Image;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline};

use crate::blend::DragonBonesBlend;

/// Custom 2D material shared by every DragonBones slot that renders the same
/// atlas page with the same blend mode.
///
/// The fragment shader is `dragonbones_material.wesl`: it samples the atlas page
/// and multiplies by the mesh vertex color (the slot tint). Blending is applied
/// by the fixed-function blend unit, configured per material in
/// [`Material2d::specialize`].
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
// The blend mode selects the pipeline; it is deliberately not a GPU binding.
#[bind_group_data(DragonBonesBlend)]
pub struct DragonBonesMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    /// Fixed-function blend mode of this material (also the pipeline key).
    pub blend: DragonBonesBlend,
}

impl From<&DragonBonesMaterial> for DragonBonesBlend {
    fn from(material: &DragonBonesMaterial) -> Self {
        material.blend
    }
}

impl Material2d for DragonBonesMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("dragonbones_material.wesl"))
                .with_source("embedded"),
        )
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        // DragonBones art is authored with straight (non-premultiplied) alpha
        // and may contain soft edges, so slots always go through Bevy's
        // transparent phase. The actual blend state is set in `specialize`.
        AlphaMode2d::Blend
    }

    fn specialize(
        _pipeline: &Material2dPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The mesh2d pipeline specializer already put the material color target
        // in place (with `BlendState::ALPHA_BLENDING` for the transparent
        // phase); replace its blend state with the one this material wants.
        if let Some(fragment) = descriptor.fragment.as_mut()
            && let Some(Some(target)) = fragment.targets.first_mut()
        {
            target.blend = Some(key.bind_group_data.blend_state());
        }
        Ok(())
    }
}

/// Cache key of one shared material: the atlas page image plus the blend mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MaterialKey {
    pub(crate) image: AssetId<Image>,
    pub(crate) blend: DragonBonesBlend,
}

impl MaterialKey {
    pub(crate) fn new(image: &Handle<Image>, blend: DragonBonesBlend) -> Self {
        Self {
            image: image.id(),
            blend,
        }
    }
}

/// One shared [`DragonBonesMaterial`] per `(atlas page image, blend mode)`, with
/// a live-armature reference count.
#[derive(Resource, Default)]
pub(crate) struct DragonBonesMaterials {
    materials: HashMap<MaterialKey, SharedMaterial>,
}

struct SharedMaterial {
    handle: Handle<DragonBonesMaterial>,
    refs: usize,
}

impl DragonBonesMaterials {
    /// Returns the shared material for `(image, blend)`, creating it on first
    /// use, and takes one reference on it.
    pub(crate) fn acquire(
        &mut self,
        image: &Handle<Image>,
        blend: DragonBonesBlend,
        materials: &mut Assets<DragonBonesMaterial>,
    ) -> Handle<DragonBonesMaterial> {
        let key = MaterialKey::new(image, blend);
        if let Some(shared) = self.materials.get_mut(&key) {
            shared.refs += 1;
            return shared.handle.clone();
        }
        let handle = materials.add(DragonBonesMaterial {
            texture: image.clone(),
            blend,
        });
        self.materials.insert(
            key,
            SharedMaterial {
                handle: handle.clone(),
                refs: 1,
            },
        );
        handle
    }

    /// Looks up an already acquired material without changing its refcount.
    pub(crate) fn handle(&self, key: MaterialKey) -> Option<Handle<DragonBonesMaterial>> {
        self.materials.get(&key).map(|shared| shared.handle.clone())
    }

    /// Drops one reference; removes the material asset together with the last
    /// armature that used it.
    pub(crate) fn release(
        &mut self,
        key: MaterialKey,
        materials: &mut Assets<DragonBonesMaterial>,
    ) {
        let Some(shared) = self.materials.get_mut(&key) else {
            return;
        };
        debug_assert!(
            shared.refs > 0,
            "material released more often than acquired"
        );
        shared.refs = shared.refs.saturating_sub(1);
        if shared.refs == 0
            && let Some(shared) = self.materials.remove(&key)
        {
            materials.remove(shared.handle.id());
        }
    }

    /// Number of distinct `(image, blend)` materials currently alive.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.materials.len()
    }
}

/// Registers the embedded fragment shader used by [`DragonBonesMaterial`].
///
/// Split out of [`crate::DragonBonesPlugin`] so the plugin's `build` stays
/// readable; must run before the material plugin so the shader asset exists when
/// the first pipeline is specialized.
pub(crate) fn register(app: &mut App) {
    embedded_asset!(app, "dragonbones_material.wesl");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Handle<Image> {
        Handle::default()
    }

    #[test]
    fn material_is_shared_per_image_and_blend_and_released_with_last_reference() {
        let mut cache = DragonBonesMaterials::default();
        let mut materials = Assets::<DragonBonesMaterial>::default();
        let image = image();

        let first = cache.acquire(&image, DragonBonesBlend::Normal, &mut materials);
        let second = cache.acquire(&image, DragonBonesBlend::Normal, &mut materials);
        assert_eq!(
            first, second,
            "same atlas page and blend share one material"
        );
        assert_eq!(materials.len(), 1);
        assert_eq!(cache.len(), 1);

        cache.release(
            MaterialKey::new(&image, DragonBonesBlend::Normal),
            &mut materials,
        );
        assert_eq!(materials.len(), 1, "one armature still uses the material");

        cache.release(
            MaterialKey::new(&image, DragonBonesBlend::Normal),
            &mut materials,
        );
        assert_eq!(materials.len(), 0, "last reference removes the material");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn different_blend_modes_get_distinct_materials() {
        let mut cache = DragonBonesMaterials::default();
        let mut materials = Assets::<DragonBonesMaterial>::default();
        let image = image();

        let mut handles = Vec::new();
        for blend in DragonBonesBlend::ALL {
            handles.push(cache.acquire(&image, blend, &mut materials));
        }
        assert_eq!(materials.len(), 4, "one material per blend mode");
        for (index, handle) in handles.iter().enumerate() {
            let material = materials.get(handle).expect("material asset");
            assert_eq!(material.blend, DragonBonesBlend::ALL[index]);
            assert_eq!(
                material.texture.id(),
                image.id(),
                "every material samples the atlas page"
            );
            for other in &handles[index + 1..] {
                assert_ne!(handle, other, "blend modes must not share a material");
            }
        }

        // Releasing one blend keeps the others alive.
        cache.release(
            MaterialKey::new(&image, DragonBonesBlend::Add),
            &mut materials,
        );
        assert_eq!(materials.len(), 3);
        assert!(
            cache
                .handle(MaterialKey::new(&image, DragonBonesBlend::Add))
                .is_none()
        );
        assert!(
            cache
                .handle(MaterialKey::new(&image, DragonBonesBlend::Screen))
                .is_some()
        );
    }

    #[test]
    fn releasing_unknown_key_is_a_no_op() {
        let mut cache = DragonBonesMaterials::default();
        let mut materials = Assets::<DragonBonesMaterial>::default();
        cache.release(
            MaterialKey::new(&Handle::<Image>::default(), DragonBonesBlend::Add),
            &mut materials,
        );
        assert_eq!(materials.len(), 0);
    }
}
