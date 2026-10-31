//! End-to-end tests for child-armature slot displays.
//!
//! No shipped asset uses an armature display, so this test drives a synthetic
//! skeleton with two armatures ("Parent" whose slot displays "Child") through the
//! adapter:
//!
//! - core builds the child armature (`BaseFactory::_buildChildArmature`) and
//!   links it from the slot display;
//! - the adapter spawns a nested root entity under the owning slot entity, and
//!   the child armature's world matrix is the owning slot's world matrix;
//! - despawning the parent releases the whole subtree's meshes/materials.

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy::time::Virtual;
use bevy::transform::TransformPlugin;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesChildArmature, DragonBonesInstance,
    DragonBonesPlugin, DragonBonesRuntime, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::{Value, json};

fn skeleton_json() -> Value {
    json!({
        "frameRate": 24,
        "name": "SyntheticChild",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [
            {
                "type": "Armature",
                "frameRate": 24,
                "name": "Parent",
                "aabb": { "x": 0, "y": 0, "width": 40, "height": 40 },
                "bone": [{ "name": "root" }],
                "slot": [{ "name": "ps", "parent": "root" }],
                "skin": [{
                    "name": "",
                    "slot": [{
                        "name": "ps",
                        "display": [{
                            "type": "armature",
                            "name": "Child",
                            "inheritAnimation": true
                        }]
                    }]
                }],
                "animation": [{
                    "duration": 1,
                    "playTimes": 0,
                    "name": "idle",
                    "bone": [{ "name": "root" }]
                }]
            },
            {
                "type": "Armature",
                "frameRate": 24,
                "name": "Child",
                "aabb": { "x": 0, "y": 0, "width": 20, "height": 20 },
                "bone": [{ "name": "root" }],
                "slot": [{ "name": "cs", "parent": "root" }],
                "skin": [{
                    "name": "",
                    "slot": [{
                        "name": "cs",
                        "display": [{
                            "type": "image",
                            "name": "tex",
                            "transform": { "x": 0, "y": 0 }
                        }]
                    }]
                }],
                "animation": [{
                    "duration": 1,
                    "playTimes": 0,
                    "name": "idle",
                    "bone": [{ "name": "root" }]
                }]
            }
        ]
    })
}

/// Parent slot with two armature displays: a slot shows exactly one display at
/// a time, so only the selected child armature may be visible.
fn dual_display_skeleton_json() -> Value {
    json!({
        "frameRate": 24,
        "name": "SyntheticDual",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [
            {
                "type": "Armature",
                "frameRate": 24,
                "name": "Parent",
                "aabb": { "x": 0, "y": 0, "width": 40, "height": 40 },
                "bone": [{ "name": "root" }],
                "slot": [{ "name": "ps", "parent": "root" }],
                "skin": [{
                    "name": "",
                    "slot": [{
                        "name": "ps",
                        "display": [
                            { "type": "armature", "name": "ChildA", "inheritAnimation": true },
                            { "type": "armature", "name": "ChildB", "inheritAnimation": true }
                        ]
                    }]
                }],
                "animation": [{
                    "duration": 1,
                    "playTimes": 0,
                    "name": "idle",
                    "bone": [{ "name": "root" }]
                }]
            },
            {
                "type": "Armature",
                "frameRate": 24,
                "name": "ChildA",
                "aabb": { "x": 0, "y": 0, "width": 20, "height": 20 },
                "bone": [{ "name": "root" }],
                "slot": [{ "name": "cs", "parent": "root" }],
                "skin": [{
                    "name": "",
                    "slot": [{
                        "name": "cs",
                        "display": [{ "type": "image", "name": "tex", "transform": { "x": 0, "y": 0 } }]
                    }]
                }],
                "animation": [{
                    "duration": 1,
                    "playTimes": 0,
                    "name": "idle",
                    "bone": [{ "name": "root" }]
                }]
            },
            {
                "type": "Armature",
                "frameRate": 24,
                "name": "ChildB",
                "aabb": { "x": 0, "y": 0, "width": 20, "height": 20 },
                "bone": [{ "name": "root" }],
                "slot": [{ "name": "cs", "parent": "root" }],
                "skin": [{
                    "name": "",
                    "slot": [{
                        "name": "cs",
                        "display": [{ "type": "image", "name": "tex", "transform": { "x": 0, "y": 0 } }]
                    }]
                }],
                "animation": [{
                    "duration": 1,
                    "playTimes": 0,
                    "name": "idle",
                    "bone": [{ "name": "root" }]
                }]
            }
        ]
    })
}

fn atlas_json() -> Value {
    json!({
        "name": "SyntheticChild",
        "imagePath": "synthetic.png",
        "width": 32,
        "height": 32,
        "SubTexture": [{ "name": "tex", "x": 0, "y": 0, "width": 16, "height": 16 }]
    })
}

fn app_with_child_rig() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        // Real transform propagation so the parent root's Transform reaches its
        // GlobalTransform (the adapter reads the root GT).
        .add_plugins(TransformPlugin)
        .add_plugins(DragonBonesPlugin);

    let skeleton_data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton_json(), 1.0)
        .expect("synthetic skeleton parses");
    let mut atlas_data = TextureAtlasData::default();
    assert!(
        ObjectDataParser::default().parse_texture_atlas_data(&atlas_json(), &mut atlas_data, 1.0),
        "synthetic atlas parses"
    );

    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(skeleton_data),
        });
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas_data,
            texture: Handle::default(),
        });
    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Parent".to_owned(),
                animation: Some("idle".to_owned()),
            },
            Transform::from_xyz(10.0, 20.0, 0.0),
        ))
        .id();
    app.update();
    (app, root)
}

fn child_armature_root(app: &mut App) -> (Entity, Entity, dragonbones_core::armature::ArmatureIdx) {
    let mut query = app
        .world_mut()
        .query::<(Entity, &DragonBonesChildArmature, &DragonBonesInstance)>();
    let (entity, child, instance) = query
        .iter(app.world())
        .next()
        .expect("adapter spawns the child armature root");
    (entity, child.parent_slot, instance.armature)
}

#[test]
fn child_armature_is_spawned_under_the_parent_slot_and_posed() {
    let (mut app, parent_root) = app_with_child_rig();
    let parent_instance = app
        .world()
        .get::<DragonBonesInstance>(parent_root)
        .expect("parent instance");
    let parent_slot = parent_instance.slots[0];
    let (child_root, child_parent_slot, _child_armature) = child_armature_root(&mut app);
    assert_eq!(
        child_parent_slot, parent_slot,
        "child armature root is parented to the slot entity that displays it"
    );

    // The child root itself sits at the owning slot's world matrix.
    let child_global = app
        .world()
        .get::<GlobalTransform>(child_root)
        .expect("child root GT")
        .affine();
    assert!((child_global.translation.x - 10.0).abs() < 1e-5);
    assert!((child_global.translation.y - 20.0).abs() < 1e-5);

    // Its slot is rendered at the same place (identity slot matrices in this
    // synthetic rig).
    let child_instance = app
        .world()
        .get::<DragonBonesInstance>(child_root)
        .expect("child instance");
    assert_eq!(child_instance.slots.len(), 1);
    let slot_global = app
        .world()
        .get::<GlobalTransform>(child_instance.slots[0])
        .expect("child slot GT")
        .affine();
    assert!((slot_global.translation.x - 10.0).abs() < 1e-5);
    assert!((slot_global.translation.y - 20.0).abs() < 1e-5);

    // Parent slot displays an armature, so the child armature inherits
    // visibility from the root (and is inherited-visible).
    assert!(matches!(
        *app.world()
            .get::<Visibility>(child_root)
            .expect("child root visibility"),
        Visibility::Inherited
    ));
}

#[test]
fn despawning_the_parent_releases_the_child_armature_assets() {
    let (mut app, parent_root) = app_with_child_rig();
    // Parent slot + child slot meshes/materials are alive.
    assert!(app.world().resource::<Assets<Mesh>>().len() >= 2);

    app.world_mut().entity_mut(parent_root).despawn();
    app.update();

    assert_eq!(
        app.world().resource::<Assets<Mesh>>().len(),
        0,
        "child armature meshes are released with the parent"
    );
    assert_eq!(
        app.world()
            .resource::<Assets<dragonbones_bevy::DragonBonesMaterial>>()
            .len(),
        0,
        "child armature materials are released with the parent"
    );
    let mut query = app.world_mut().query::<&DragonBonesChildArmature>();
    assert_eq!(
        query.iter(app.world()).count(),
        0,
        "child root entity is despawned with the parent"
    );
}

fn app_with_dual_display_rig() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .add_plugins(TransformPlugin)
        .add_plugins(DragonBonesPlugin);

    let skeleton_data = ObjectDataParser::default()
        .parse_dragon_bones_data(&dual_display_skeleton_json(), 1.0)
        .expect("synthetic dual-display skeleton parses");
    let mut atlas_data = TextureAtlasData::default();
    assert!(
        ObjectDataParser::default().parse_texture_atlas_data(&atlas_json(), &mut atlas_data, 1.0),
        "synthetic atlas parses"
    );

    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(skeleton_data),
        });
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas_data,
            texture: Handle::default(),
        });
    let root = app
        .world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Parent".to_owned(),
                animation: Some("idle".to_owned()),
            },
            Transform::default(),
        ))
        .id();
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    app.update();
    (app, root)
}

fn child_roots_by_name(app: &mut App) -> std::collections::HashMap<String, Entity> {
    let roots: Vec<(Entity, dragonbones_core::armature::ArmatureIdx)> = {
        let mut query = app
            .world_mut()
            .query::<(Entity, &DragonBonesChildArmature, &DragonBonesInstance)>();
        query
            .iter(app.world())
            .map(|(entity, _child, instance)| (entity, instance.armature))
            .collect()
    };
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    roots
        .into_iter()
        .map(|(entity, armature)| {
            let name = runtime.factory.armatures[armature.index()]
                .as_ref()
                .expect("live child armature")
                .armature_data()
                .name
                .clone();
            (name, entity)
        })
        .collect()
}

fn slot_visibility(app: &App, entity: Entity) -> Visibility {
    *app.world()
        .get::<Visibility>(entity)
        .expect("slot visibility")
}

#[test]
fn only_the_currently_selected_armature_display_is_visible() {
    let (mut app, parent_root) = app_with_dual_display_rig();
    let parent_instance = app
        .world()
        .get::<DragonBonesInstance>(parent_root)
        .expect("parent instance");
    let parent_armature = parent_instance.armature;
    let slot_entity = parent_instance.slots[0];
    let slot_idx = app
        .world()
        .get::<dragonbones_bevy::DragonBonesSlot>(slot_entity)
        .expect("parent slot")
        .slot;

    let children = child_roots_by_name(&mut app);
    assert_eq!(children.len(), 2, "both child armatures are spawned");
    let child_a = children["ChildA"];
    let child_b = children["ChildB"];
    assert!(matches!(
        slot_visibility(&app, child_a),
        Visibility::Inherited
    ));
    assert!(matches!(slot_visibility(&app, child_b), Visibility::Hidden));

    // Switch the slot to its second armature display (the official setter only
    // records the index; the next armature advance resolves the new display).
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<DragonBonesRuntime>()
            .expect("runtime resource");
        let armature = runtime.factory.armatures[parent_armature.index()]
            .as_mut()
            .expect("live parent armature");
        let slot = armature
            .slots
            .iter_mut()
            .find(|slot| slot.idx == slot_idx)
            .expect("parent slot");
        slot.set_display_index(1, false);
        // Slot updates only run when the cache frame index changes, so advance a
        // full frame (the virtual clock is paused, hence the explicit step).
        runtime.factory.advance_one(parent_armature, 1.0 / 24.0);
    }
    app.update();

    assert!(
        matches!(slot_visibility(&app, child_b), Visibility::Inherited),
        "switching display shows the newly selected child armature"
    );
    assert!(
        matches!(slot_visibility(&app, child_a), Visibility::Hidden),
        "the previously selected child armature is hidden, not frozen in place"
    );
}
