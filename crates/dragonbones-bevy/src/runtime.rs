use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::AssetApp;
use bevy::camera::visibility::VisibilitySystems;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::sprite_render::Material2dPlugin;
use bevy::transform::TransformSystems;
use dragonbones_core::armature::{Armature, ArmatureIdx, DisplayHandle, SlotIdx};
use dragonbones_core::factory::BaseFactory;

use crate::asset::{AtlasAsset, AtlasAssetLoader, SkeletonAsset, SkeletonAssetLoader};
use crate::blend::DragonBonesBlend;
use crate::events::{BevyArmatureProxy, DragonBonesEvent, DragonBonesEventQueue};
use crate::material::{self, DragonBonesMaterial, DragonBonesMaterials, MaterialKey};
use crate::player::{DragonBonesCommand, apply_dragonbones_commands};
use crate::render;
use crate::sync;

/// Non-send runtime state. `BaseFactory` contains trait objects
/// (`Box<dyn ArmatureProxy>`, `Box<dyn EventDispatcher>`) without `Send` bounds,
/// so it is stored as a Bevy non-send resource and only touched from the main
/// thread.
#[derive(Default)]
pub struct DragonBonesRuntime {
    pub factory: BaseFactory,
    /// Adapter bookkeeping for every built armature, used to release core and
    /// GPU state when the root entity is despawned.
    pub(crate) bindings: HashMap<ArmatureIdx, ArmatureBinding>,
    /// `factory.data_map` / `factory.atlas_map` reference counts. The adapter
    /// registers one entry per unique asset handle; the entry is removed with
    /// the last armature that used it.
    data_refs: HashMap<String, usize>,
    atlas_refs: HashMap<String, usize>,
}

/// What the adapter owns on behalf of one armature instance.
pub(crate) struct ArmatureBinding {
    root: Entity,
    slots: Vec<Entity>,
    meshes: Vec<Handle<Mesh>>,
    /// Every `(atlas image, blend mode)` material key this armature acquired,
    /// deduplicated. Cleanup releases exactly these keys, so the shared material
    /// cache's reference counts stay balanced even when slots change blend mode
    /// at runtime (the old key is kept until the armature is disposed, which
    /// bounds the set to at most four keys per atlas page).
    pub(crate) materials: HashSet<MaterialKey>,
    dragon_bones_key: String,
    atlas_key: String,
    /// Root affine written by the last sync, used for the armature-level dirty
    /// check.
    pub(crate) last_root: Option<Affine3A>,
    /// Pose hash of the last sync, used for the armature-level dirty check.
    pub(crate) last_pose: Option<u64>,
    /// Parent armature when this binding belongs to a child-armature display.
    pub(crate) parent: Option<ArmatureIdx>,
    /// Effective visibility written by the last sync: toggling the root's
    /// `Visibility` has to resync the slots so concrete inherited/hidden values
    /// stay in sync with the pose.
    pub(crate) last_visible: Option<bool>,
}

/// Adapter-level playback state for one armature root.
///
/// Core animation states each carry their own `time_scale` (matching the
/// reference runtime); this component remembers the *desired* armature speed and
/// whether the armature is paused so a subsequent `Play` inherits them.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DragonBonesPlaybackState {
    /// Desired playback speed (`1.0` = authored speed).
    pub time_scale: f64,
    /// Whether commands should keep newly started animations frozen.
    pub paused: bool,
}

impl Default for DragonBonesPlaybackState {
    fn default() -> Self {
        Self {
            time_scale: 1.0,
            paused: false,
        }
    }
}

/// Marks the root entity spawned for a child-armature display.
///
/// Child armatures are built by core when a slot's display is an armature; the
/// adapter spawns a nested root entity under the owning slot entity and drives
/// its slots through the same sync path. The child armature's world transform is
/// the owning slot's world transform.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragonBonesChildArmature {
    /// Slot entity of the parent armature whose display is this armature.
    pub parent_slot: Entity,
    /// Core armature instance of the parent.
    pub parent: ArmatureIdx,
}

/// Tuning knobs for the adapter's per-frame slot sync.
///
/// The resource must be inserted *before* [`DragonBonesPlugin`] to take effect
/// (the plugin only calls `init_resource`, which keeps an existing value).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragonBonesSyncConfig {
    /// Skip the whole per-slot sync pass for an armature whose pose hash and
    /// root affine are unchanged since the previous frame.
    ///
    /// This is what makes idle armatures (paused animation, static props) cheap:
    /// instead of rewriting every slot, the adapter only hashes the pose. Set to
    /// `false` to force the pre-optimization behaviour (every slot, every frame).
    pub skip_clean_armatures: bool,
}

impl Default for DragonBonesSyncConfig {
    fn default() -> Self {
        Self {
            skip_clean_armatures: true,
        }
    }
}

impl DragonBonesRuntime {
    /// Splits the runtime into the core factory and the adapter bindings so the
    /// sync system can hold an immutable factory borrow and a mutable bindings
    /// borrow at the same time.
    pub(crate) fn split_mut(
        &mut self,
    ) -> (&BaseFactory, &mut HashMap<ArmatureIdx, ArmatureBinding>) {
        (&self.factory, &mut self.bindings)
    }

    fn retain_data(&mut self, dragon_bones_key: &str, atlas_key: &str) {
        *self
            .data_refs
            .entry(dragon_bones_key.to_owned())
            .or_insert(0) += 1;
        *self.atlas_refs.entry(atlas_key.to_owned()).or_insert(0) += 1;
    }

    /// Removes registrations made for a build that failed before any armature
    /// bound them, without touching refcounts owned by live armatures.
    fn drop_unretained_registration(&mut self, dragon_bones_key: &str, atlas_key: &str) {
        if self.data_refs.get(dragon_bones_key).copied().unwrap_or(0) == 0 {
            let _ = self.factory.data_map.shift_remove(dragon_bones_key);
        }
        if self.atlas_refs.get(atlas_key).copied().unwrap_or(0) == 0 {
            let _ = self.factory.atlas_map.shift_remove(atlas_key);
        }
    }

    fn release_data(&mut self, dragon_bones_key: &str, atlas_key: &str) {
        if let Some(count) = self.data_refs.get_mut(dragon_bones_key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.data_refs.remove(dragon_bones_key);
                let _ = self.factory.data_map.shift_remove(dragon_bones_key);
            }
        }
        if let Some(count) = self.atlas_refs.get_mut(atlas_key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.atlas_refs.remove(atlas_key);
                let _ = self.factory.atlas_map.shift_remove(atlas_key);
            }
        }
    }
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

/// Build state of a [`DragonBonesArmature`] request.
///
/// The request is a one-shot build descriptor: the adapter builds the armature
/// when both assets are loaded and then treats the component as immutable.
/// Changing fields afterwards is ignored (warned once per distinct request); use
/// [`crate::DragonBonesCommand`] to switch animations at runtime.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct DragonBonesArmatureState {
    pub phase: DragonBonesArmaturePhase,
    /// Fingerprint of the request fields the state was last synced with.
    pub request_hash: u64,
}

/// Phase of a [`DragonBonesArmature`] request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DragonBonesArmaturePhase {
    /// The skeleton/atlas assets are still loading.
    WaitingForAssets,
    /// The armature was built and is driven by the adapter.
    Ready,
    /// Building failed (e.g. the armature name does not exist in the skeleton).
    Failed {
        /// Human-readable reason.
        message: String,
    },
}

fn request_hash(request: &DragonBonesArmature) -> u64 {
    const SEED: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = SEED;
    let mut mix = |value: u64| {
        hash = (hash ^ value).wrapping_mul(PRIME);
    };
    for byte in request.armature_name.as_bytes() {
        mix(u64::from(*byte));
    }
    match &request.animation {
        Some(animation) => {
            mix(1);
            for byte in animation.as_bytes() {
                mix(u64::from(*byte));
            }
        }
        None => mix(0),
    }
    mix_asset_id(&mut mix, request.skeleton.id());
    mix_asset_id(&mut mix, request.atlas.id());
    hash
}

fn mix_asset_id<A: bevy::asset::Asset>(mix: &mut impl FnMut(u64), id: bevy::asset::AssetId<A>) {
    match id.untyped() {
        bevy::asset::UntypedAssetId::Index { index, .. } => {
            mix(0);
            mix(index.to_bits());
        }
        bevy::asset::UntypedAssetId::Uuid { uuid, .. } => {
            mix(1);
            let bits = uuid.as_u128();
            mix(bits as u64);
            mix((bits >> 64) as u64);
        }
    }
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
    /// Release armatures whose root entity disappeared before building new ones.
    Cleanup,
    Setup,
    Advance,
    SyncTransforms,
}

/// Bevy plugin for the DragonBones adapter.
pub struct DragonBonesPlugin;

impl Plugin for DragonBonesPlugin {
    fn build(&self, app: &mut App) {
        // The custom material's fragment shader is embedded in this crate;
        // register it before adding the material plugin so the shader asset is
        // available when the first pipeline is specialized.
        material::register(app);

        // Slot visibility is written as inherited/hidden, so the adapter relies
        // on Bevy's hierarchy propagation; the mesh asset collection is also
        // registered here so headless/adapter-only apps can spawn slots.
        app.init_asset::<Mesh>()
            .init_asset::<SkeletonAsset>()
            .init_asset::<AtlasAsset>()
            .add_plugins(Material2dPlugin::<DragonBonesMaterial>::default())
            .register_asset_loader(SkeletonAssetLoader)
            .register_asset_loader(AtlasAssetLoader)
            .init_resource::<DragonBonesMaterials>()
            .init_resource::<DragonBonesSyncConfig>()
            .init_resource::<DragonBonesEventQueue>()
            .add_message::<DragonBonesEvent>()
            .add_message::<DragonBonesCommand>()
            .add_message::<crate::player::DragonBonesCommandError>()
            .insert_non_send(DragonBonesRuntime::default())
            .configure_sets(
                Update,
                (
                    DragonBonesSystems::Cleanup,
                    DragonBonesSystems::Setup,
                    DragonBonesSystems::Advance,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                DragonBonesSystems::SyncTransforms
                    .after(TransformSystems::Propagate)
                    .before(VisibilitySystems::VisibilityPropagate)
                    .before(VisibilitySystems::CalculateBounds)
                    .before(VisibilitySystems::UpdateFrusta)
                    .before(VisibilitySystems::CheckVisibility),
            )
            .add_systems(
                Update,
                (
                    cleanup_armatures.in_set(DragonBonesSystems::Cleanup),
                    setup_armatures.in_set(DragonBonesSystems::Setup),
                    (
                        // Commands take effect before this frame's advance, and the
                        // events buffered by the advance must be readable in the
                        // same frame.
                        apply_dragonbones_commands,
                        advance_dragonbones,
                        crate::events::forward_dragonbones_events,
                    )
                        .chain()
                        .in_set(DragonBonesSystems::Advance),
                ),
            )
            .add_systems(
                PostUpdate,
                sync::sync_dragonbones.in_set(DragonBonesSystems::SyncTransforms),
            );
    }
}

/// Releases the core armature tree, its meshes and its shared atlas materials
/// when a root entity (or at least its [`DragonBonesInstance`]) disappears.
///
/// Core `dispose_armature` only queues the disposal (`Armature.dispose()` in the
/// reference implementation buffers the object); the real `on_clear` runs in the
/// next `advance_time`, which happens later in this same `Update` because
/// [`DragonBonesSystems::Cleanup`] is ordered before
/// [`DragonBonesSystems::Advance`]. Child armatures are disposed by core's own
/// recursive BFS, so the adapter only releases their GPU assets and entities.
#[allow(clippy::type_complexity)]
fn cleanup_armatures(
    mut commands: Commands,
    mut removed: RemovedComponents<DragonBonesInstance>,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DragonBonesMaterial>>,
    mut material_cache: ResMut<DragonBonesMaterials>,
) {
    for entity in removed.read() {
        let Some(root_armature) = runtime
            .bindings
            .iter()
            .find(|(_, binding)| binding.root == entity)
            .map(|(armature_idx, _)| *armature_idx)
        else {
            continue;
        };
        // Child-armature roots are adapter-internal (children of a parent slot):
        // when they are torn down, the top-level cleanup below releases the whole
        // subtree. Despawning a child root on its own is not supported.
        if runtime
            .bindings
            .get(&root_armature)
            .is_some_and(|binding| binding.parent.is_some())
        {
            continue;
        }

        // Collect the armature and every child-armature binding below it.
        let mut subtree = Vec::new();
        let mut stack = vec![root_armature];
        while let Some(current) = stack.pop() {
            subtree.push(current);
            for (armature_idx, binding) in &runtime.bindings {
                if binding.parent == Some(current) {
                    stack.push(*armature_idx);
                }
            }
        }

        for armature_idx in subtree {
            let Some(binding) = runtime.bindings.remove(&armature_idx) else {
                continue;
            };
            for mesh in &binding.meshes {
                meshes.remove(mesh.id());
            }
            for key in &binding.materials {
                material_cache.release(*key, &mut materials);
            }
            runtime.release_data(&binding.dragon_bones_key, &binding.atlas_key);

            if binding.parent.is_none() {
                // Slots are Bevy children of the root, so they are already gone
                // when the root was despawned; this only matters if a user
                // removed the instance component while keeping the root alive.
                for slot in binding.slots {
                    if let Ok(mut slot_commands) = commands.get_entity(slot) {
                        slot_commands.despawn();
                    }
                }
            }
            // Child roots are descendants of the parent root; despawn explicitly
            // only for the component-removed-while-alive case (slot children are
            // despawned recursively by Bevy).
            if binding.parent.is_some()
                && let Ok(mut child_commands) = commands.get_entity(binding.root)
            {
                child_commands.despawn();
            }
        }

        runtime.factory.dispose_armature(root_armature);
    }
}

/// Result of spawning one armature's slot entities.
struct SpawnedSlots {
    slot_entities: Vec<Entity>,
    mesh_handles: Vec<Handle<Mesh>>,
    material_keys: HashSet<MaterialKey>,
}

/// Spawns one entity per slot of `armature_idx` under `parent`.
#[allow(clippy::too_many_arguments)]
fn spawn_armature_slots(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<DragonBonesMaterial>,
    material_cache: &mut DragonBonesMaterials,
    runtime: &DragonBonesRuntime,
    parent: Entity,
    armature_idx: ArmatureIdx,
    atlas_texture: &Handle<Image>,
    atlas_size: UVec2,
) -> Option<SpawnedSlots> {
    let armature = runtime
        .factory
        .armatures
        .get(armature_idx.index())
        .and_then(Option::as_ref)?;
    let armature_scale = armature.armature_data().scale;
    let mut slot_entities = Vec::with_capacity(armature.slots.len());
    let mut mesh_handles = Vec::with_capacity(armature.slots.len());
    // One shared material per (atlas page, blend mode) used by this armature;
    // the key set is what cleanup releases.
    let mut armature_materials: HashMap<DragonBonesBlend, Handle<DragonBonesMaterial>> =
        HashMap::new();
    for slot in &armature.slots {
        let blend = DragonBonesBlend::from_core(slot.blend_mode);
        let material = if let Some(material) = armature_materials.get(&blend) {
            material.clone()
        } else {
            let material = material_cache.acquire(atlas_texture, blend, materials);
            armature_materials.insert(blend, material.clone());
            material
        };
        let (entity, mesh) = render::spawn_slot(
            commands,
            meshes,
            parent,
            armature_idx,
            armature,
            slot,
            &material,
            blend,
            atlas_texture,
            atlas_size,
            armature_scale,
        );
        slot_entities.push(entity);
        mesh_handles.push(mesh);
    }
    let material_keys = armature_materials
        .into_keys()
        .map(|blend| MaterialKey::new(atlas_texture, blend))
        .collect();
    Some(SpawnedSlots {
        slot_entities,
        mesh_handles,
        material_keys,
    })
}

/// Child armature instances referenced by `armature`'s slot displays, paired
/// with the slot entity that displays them (deduplicated per child instance).
fn child_armature_refs(
    armature: &Armature,
    slot_entities: &[Entity],
) -> Vec<(ArmatureIdx, Entity)> {
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for (order, slot) in armature.slots.iter().enumerate() {
        let Some(&slot_entity) = slot_entities.get(order) else {
            continue;
        };
        let mut candidates: Vec<ArmatureIdx> = slot
            .display_frames
            .iter()
            .filter_map(|frame| match frame.display {
                Some(DisplayHandle::Armature(idx)) => Some(idx),
                _ => None,
            })
            .collect();
        if let Some(child) = slot.child_armature {
            candidates.push(child);
        }
        for child in candidates {
            if seen.insert(child) {
                refs.push((child, slot_entity));
            }
        }
    }
    refs
}

/// Spawns nested root entities (and their slots) for every child armature below
/// `parent_idx`, recursively.
#[allow(clippy::too_many_arguments)]
fn spawn_child_armatures(
    commands: &mut Commands,
    runtime: &mut DragonBonesRuntime,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<DragonBonesMaterial>,
    material_cache: &mut DragonBonesMaterials,
    parent_idx: ArmatureIdx,
    parent_slot_entities: &[Entity],
    atlas_texture: &Handle<Image>,
    atlas_size: UVec2,
) {
    let mut pending: Vec<(ArmatureIdx, Entity, ArmatureIdx)> = {
        let Some(parent) = runtime
            .factory
            .armatures
            .get(parent_idx.index())
            .and_then(Option::as_ref)
        else {
            return;
        };
        child_armature_refs(parent, parent_slot_entities)
            .into_iter()
            .map(|(child, slot)| (parent_idx, slot, child))
            .collect()
    };
    while let Some((parent_armature, parent_slot, child_idx)) = pending.pop() {
        if runtime.bindings.contains_key(&child_idx) {
            continue;
        }
        let child_root = commands
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                ChildOf(parent_slot),
                DragonBonesChildArmature {
                    parent_slot,
                    parent: parent_armature,
                },
            ))
            .id();
        let Some(spawned) = spawn_armature_slots(
            commands,
            meshes,
            materials,
            material_cache,
            runtime,
            child_root,
            child_idx,
            atlas_texture,
            atlas_size,
        ) else {
            commands.entity(child_root).despawn();
            continue;
        };
        let Some((db_key, atlas_key)) = runtime
            .bindings
            .get(&parent_armature)
            .map(|binding| (binding.dragon_bones_key.clone(), binding.atlas_key.clone()))
        else {
            commands.entity(child_root).despawn();
            continue;
        };
        let grandchildren = runtime
            .factory
            .armatures
            .get(child_idx.index())
            .and_then(Option::as_ref)
            .map(|child| child_armature_refs(child, &spawned.slot_entities))
            .unwrap_or_default();
        runtime.retain_data(&db_key, &atlas_key);
        runtime.bindings.insert(
            child_idx,
            ArmatureBinding {
                root: child_root,
                slots: spawned.slot_entities.clone(),
                meshes: spawned.mesh_handles,
                materials: spawned.material_keys,
                dragon_bones_key: db_key,
                atlas_key,
                last_root: None,
                last_pose: None,
                parent: Some(parent_armature),
                last_visible: None,
            },
        );
        commands.entity(child_root).insert((
            DragonBonesInstance {
                armature: child_idx,
                slots: spawned.slot_entities,
            },
            DragonBonesPlaybackState::default(),
        ));
        pending.extend(
            grandchildren
                .into_iter()
                .map(|(child, slot)| (child_idx, slot, child)),
        );
    }
}

fn set_state(
    commands: &mut Commands,
    root: Entity,
    state: Option<Mut<DragonBonesArmatureState>>,
    phase: DragonBonesArmaturePhase,
    request_hash: u64,
) {
    match state {
        Some(mut state) => {
            if state.phase != phase {
                state.phase = phase;
            }
            state.request_hash = request_hash;
        }
        None => {
            commands.entity(root).insert(DragonBonesArmatureState {
                phase,
                request_hash,
            });
        }
    }
}

fn warn_waiting(
    commands: &mut Commands,
    root: Entity,
    state: Option<Mut<DragonBonesArmatureState>>,
    armature_name: &str,
    missing: &str,
    request_hash: u64,
) {
    let already_waiting = state
        .as_ref()
        .is_some_and(|state| state.phase == DragonBonesArmaturePhase::WaitingForAssets);
    if !already_waiting {
        warn!("DragonBones: waiting for the {missing} asset of armature `{armature_name}`");
    }
    set_state(
        commands,
        root,
        state,
        DragonBonesArmaturePhase::WaitingForAssets,
        request_hash,
    );
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn setup_armatures(
    mut commands: Commands,
    mut runtime: NonSendMut<DragonBonesRuntime>,
    skeletons: Res<Assets<SkeletonAsset>>,
    atlases: Res<Assets<AtlasAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DragonBonesMaterial>>,
    mut material_cache: ResMut<DragonBonesMaterials>,
    event_queue: Res<DragonBonesEventQueue>,
    mut query: Query<(
        Entity,
        Ref<DragonBonesArmature>,
        Option<&DragonBonesInstance>,
        Option<&mut DragonBonesArmatureState>,
    )>,
) {
    for (root, request_ref, instance, mut state) in &mut query {
        let request: &DragonBonesArmature = &request_ref;
        let hash = request_hash(request);
        if instance.is_some() {
            // The request is a one-shot build descriptor; ignore later field
            // changes but warn once per distinct request.
            match state.as_mut() {
                Some(state) => {
                    if state.request_hash != hash {
                        warn!(
                            "DragonBones: changes to DragonBonesArmature `{}` after it was built are ignored (use DragonBonesCommand)",
                            request.armature_name
                        );
                        state.request_hash = hash;
                    }
                    if state.phase != DragonBonesArmaturePhase::Ready {
                        state.phase = DragonBonesArmaturePhase::Ready;
                    }
                }
                None => {
                    commands.entity(root).insert(DragonBonesArmatureState {
                        phase: DragonBonesArmaturePhase::Ready,
                        request_hash: hash,
                    });
                }
            }
            continue;
        }
        let Some(skeleton) = skeletons.get(&request.skeleton) else {
            warn_waiting(
                &mut commands,
                root,
                state,
                &request.armature_name,
                "skeleton",
                hash,
            );
            continue;
        };
        let Some(atlas) = atlases.get(&request.atlas) else {
            warn_waiting(
                &mut commands,
                root,
                state,
                &request.armature_name,
                "atlas",
                hash,
            );
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
            Box::new(BevyArmatureProxy::new(event_queue.clone())),
        ) else {
            let message = format!(
                "armature `{}` not found in skeleton {:?}",
                request.armature_name, request.skeleton
            );
            warn!("DragonBones: {message}");
            runtime.drop_unretained_registration(&db_key, &atlas_key);
            set_state(
                &mut commands,
                root,
                state,
                DragonBonesArmaturePhase::Failed { message },
                hash,
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

        let atlas_size = UVec2::new(
            atlas.data.width.max(1.0) as u32,
            atlas.data.height.max(1.0) as u32,
        );
        let Some(spawned) = spawn_armature_slots(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut material_cache,
            &runtime,
            root,
            armature_idx,
            &atlas.texture,
            atlas_size,
        ) else {
            // `advance_one` above cannot drop the armature.
            warn!("DragonBones: armature {armature_idx:?} disappeared after build");
            continue;
        };

        runtime.retain_data(&db_key, &atlas_key);
        runtime.bindings.insert(
            armature_idx,
            ArmatureBinding {
                root,
                slots: spawned.slot_entities.clone(),
                meshes: spawned.mesh_handles,
                materials: spawned.material_keys,
                dragon_bones_key: db_key,
                atlas_key,
                last_root: None,
                last_pose: None,
                parent: None,
                last_visible: None,
            },
        );
        commands.entity(root).insert((
            DragonBonesInstance {
                armature: armature_idx,
                slots: spawned.slot_entities.clone(),
            },
            DragonBonesPlaybackState::default(),
            DragonBonesArmatureState {
                phase: DragonBonesArmaturePhase::Ready,
                request_hash: hash,
            },
        ));
        spawn_child_armatures(
            &mut commands,
            &mut runtime,
            &mut meshes,
            &mut materials,
            &mut material_cache,
            armature_idx,
            &spawned.slot_entities,
            &atlas.texture,
            atlas_size,
        );
    }
}

fn advance_dragonbones(time: Option<Res<Time>>, mut runtime: NonSendMut<DragonBonesRuntime>) {
    let delta = time.map_or(0.0, |time| time.delta_secs() as f64);
    runtime.factory.advance_time(delta);
}
