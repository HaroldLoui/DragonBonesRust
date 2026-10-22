use std::sync::Arc;

use bevy::asset::AssetApp;
use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use dragonbones_core::armature::{ArmatureIdx, NullArmatureProxy, SlotIdx};
use dragonbones_core::factory::BaseFactory;

use crate::asset::{AtlasAsset, AtlasAssetLoader, SkeletonAsset, SkeletonAssetLoader};
use crate::render;
use crate::sync;

/// Non-send runtime state. `BaseFactory` contains trait objects
/// (`Box<dyn ArmatureProxy>`, `Box<dyn EventDispatcher>`) without `Send` bounds,
/// so it is stored as a Bevy non-send resource and only touched from the main
/// thread.
#[derive(Default)]
pub struct DragonBonesRuntime {
    pub factory: BaseFactory,
}

/// Request an armature on an entity. The entity is the armature root; its
/// `Transform` is the armature's world transform (apply the y-down flip here
/// once, e.g. `scale = (1, -1, 1)`).
#[derive(Component, Clone)]
#[require(Transform, Visibility)]
pub struct DragonBonesArmature {
    pub skeleton: Handle<SkeletonAsset>,
    pub atlas: Handle<AtlasAsset>,
    pub armature_name: String,
    /// Animation to play when the armature is built. `None` plays the data's
    /// default animation, if any.
    pub animation: Option<String>,
}

/// Runtime binding between an armature root entity and the core arena.
#[derive(Component)]
pub struct DragonBonesInstance {
    pub armature: ArmatureIdx,
    pub slots: Vec<Entity>,
}

/// Marks a spawned slot entity and identifies its core slot identity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragonBonesSlot {
    pub armature: ArmatureIdx,
    pub slot: SlotIdx,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DragonBonesSystems {
    Setup,
    Advance,
    SyncTransforms,
}

/// Bevy plugin for the DragonBones adapter.
pub struct DragonBonesPlugin;

impl Plugin for DragonBonesPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SkeletonAsset>()
            .init_asset::<AtlasAsset>()
            .register_asset_loader(SkeletonAssetLoader)
            .register_asset_loader(AtlasAssetLoader)
            .insert_non_send(DragonBonesRuntime::default())
            .configure_sets(
                Update,
                (DragonBonesSystems::Setup, DragonBonesSystems::Advance).chain(),
            )
            .configure_sets(
                PostUpdate,
                DragonBonesSystems::SyncTransforms
                    .after(TransformSystems::Propagate)
                    .before(VisibilitySystems::CalculateBounds)
                    .before(VisibilitySystems::UpdateFrusta)
                    .before(VisibilitySystems::CheckVisibility),
            )
            .add_systems(
                Update,
                (
                    setup_armatures.in_set(DragonBonesSystems::Setup),
                    advance_dragonbones.in_set(DragonBonesSystems::Advance),
                ),
            )
            .add_systems(
                PostUpdate,
                sync::sync_dragonbones.in_set(DragonBonesSystems::SyncTransforms),
            );
    }
}

fn setup_armatures(
    mut commands: Commands,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    skeletons: Res<Assets<SkeletonAsset>>,
    atlases: Res<Assets<AtlasAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    query: Query<(Entity, &DragonBonesArmature, Option<&DragonBonesInstance>)>,
) {
    for (root, request, instance) in &query {
        if instance.is_some() {
            continue;
        }
        let Some(skeleton) = skeletons.get(&request.skeleton) else {
            continue;
        };
        let Some(atlas) = atlases.get(&request.atlas) else {
            continue;
        };

        // Asset names are not unique across split files ("Dragon" is used by
        // Dragon, Dragon_Blend and Surface). Key the factory caches by the
        // asset handle id so each loaded file is independent.
        let db_key = format!("{}::{:?}", skeleton.data.name, request.skeleton.id());
        let atlas_key = format!("{}::{:?}", atlas.data.name, request.atlas.id());
        runtime
            .factory
            .data_map
            .entry(db_key.clone())
            .or_insert_with(|| Arc::clone(&skeleton.data));
        let atlas_list = runtime
            .factory
            .atlas_map
            .entry(atlas_key.clone())
            .or_default();
        if !atlas_list.contains(&atlas.data) {
            atlas_list.push(atlas.data.clone());
        }

        let Some(armature_idx) = runtime.factory.build_armature(
            &request.armature_name,
            &db_key,
            "",
            &atlas_key,
            Box::new(NullArmatureProxy::new()),
        ) else {
            warn!(
                "DragonBones: failed to build armature `{}` from {:?}",
                request.armature_name, request.skeleton
            );
            continue;
        };

        let root_clock = runtime.factory.root_clock();
        if let Err(error) = runtime.factory.attach_armature(root_clock, armature_idx) {
            warn!("DragonBones: failed to attach armature clock: {error}");
        }

        {
            let Some(armature) = runtime
                .factory
                .armatures
                .get_mut(armature_idx.index())
                .and_then(Option::as_mut)
            else {
                continue;
            };
            armature.animation.play(request.animation.as_deref(), -1);
        }
        // Build already ran advance(0), but the animation was just started;
        // force one update so the first rendered frame is posed.
        runtime.factory.advance_one(armature_idx, 0.0);

        let Some(armature) = runtime
            .factory
            .armatures
            .get(armature_idx.index())
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let armature_scale = armature.armature_data().scale;
        let atlas_size = UVec2::new(
            atlas.data.width.max(1.0) as u32,
            atlas.data.height.max(1.0) as u32,
        );
        let mut slot_entities = Vec::with_capacity(armature.slots.len());
        for slot in &armature.slots {
            let entity = render::spawn_slot(
                &mut commands,
                &mut meshes,
                &mut materials,
                root,
                armature_idx,
                armature,
                slot,
                &atlas.texture,
                atlas_size,
                armature_scale,
            );
            slot_entities.push(entity);
        }

        commands.entity(root).insert(DragonBonesInstance {
            armature: armature_idx,
            slots: slot_entities,
        });
    }
}

fn advance_dragonbones(time: Option<Res<Time>>, mut runtime: NonSendMut<DragonBonesRuntime>) {
    let delta = time.map_or(0.0, |time| time.delta_secs() as f64);
    runtime.factory.advance_time(delta);
}
