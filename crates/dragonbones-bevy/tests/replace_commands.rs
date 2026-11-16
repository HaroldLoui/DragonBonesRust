//! Integration tests for runtime content replacement (`ReplaceSkin` /
//! `ReplaceDisplay`) through the Bevy command message.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesCommand, DragonBonesCommandError,
    DragonBonesInstance, DragonBonesPlugin, DragonBonesRuntime, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::Value;

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin {
            file_path: assets_dir().to_string_lossy().into_owned(),
            ..default()
        })
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);
    app
}

/// Installs the Wallnut skeleton/atlas directly (headless; no image asset).
fn install_wallnut_assets(app: &mut App) -> (Handle<SkeletonAsset>, Handle<AtlasAsset>) {
    let dir = assets_dir().join("Wallnut");
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Wallnut_ske.json")).expect("read skeleton"),
    )
    .expect("parse skeleton JSON");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Wallnut_tex.json")).expect("read atlas"),
    )
    .expect("parse atlas JSON");

    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("parse Wallnut skeleton");
    let mut atlas_data = TextureAtlasData::default();
    assert!(parser.parse_texture_atlas_data(&atlas_json, &mut atlas_data, 1.0));

    let skeleton = app
        .world_mut()
        .resource_mut::<Assets<SkeletonAsset>>()
        .add(SkeletonAsset {
            data: Arc::new(data),
        });
    let atlas = app
        .world_mut()
        .resource_mut::<Assets<AtlasAsset>>()
        .add(AtlasAsset {
            data: atlas_data,
            texture: Handle::default(),
        });
    (skeleton, atlas)
}

fn spawn_wallnut(app: &mut App) -> Entity {
    let (skeleton, atlas) = install_wallnut_assets(app);
    app.world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Wall-nut".to_owned(),
                animation: None,
            },
            Transform::from_scale(Vec3::new(1.0, -1.0, 1.0)),
        ))
        .id()
}

fn armature_index(app: &App, root: Entity) -> dragonbones_core::armature::ArmatureIdx {
    app.world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance")
        .armature
}

/// `(armature slot index, spawned slot entity)` for `body_front`.
fn body_front_slot(app: &App, root: Entity) -> (usize, Entity) {
    let armature = armature_index(app, root);
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    let position = runtime.factory.armatures[armature.index()]
        .as_ref()
        .expect("armature alive")
        .slots
        .iter()
        .position(|slot| slot.slot_name == "body_front")
        .expect("body_front slot");
    let entity = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance")
        .slots[position];
    (position, entity)
}

fn slot_texture_name(app: &App, root: Entity) -> Option<String> {
    let armature = armature_index(app, root);
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime resource");
    runtime.factory.armatures[armature.index()]
        .as_ref()
        .expect("armature alive")
        .slots
        .iter()
        .find(|slot| slot.slot_name == "body_front")
        .and_then(|slot| slot.texture_data.as_ref())
        .map(|texture| texture.name.clone())
}

fn mesh_uvs(app: &App, slot: Entity) -> Vec<[f32; 2]> {
    let handle = app
        .world()
        .get::<Mesh2d>(slot)
        .expect("slot mesh")
        .0
        .clone();
    let mesh = app
        .world()
        .resource::<Assets<Mesh>>()
        .get(&handle)
        .expect("mesh asset");
    match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(bevy::mesh::VertexAttributeValues::Float32x2(values)) => values.clone(),
        Some(_) => panic!("expected float32x2 UVs"),
        None => Vec::new(),
    }
}

fn command_errors(app: &App) -> Vec<DragonBonesCommandError> {
    app.world()
        .resource::<Messages<DragonBonesCommandError>>()
        .iter_current_update_messages()
        .cloned()
        .collect()
}

#[test]
fn replace_display_command_updates_texture_and_mesh_in_the_same_frame() {
    let mut app = test_app();
    let root = spawn_wallnut(&mut app);
    app.update();

    let (_, slot_entity) = body_front_slot(&app, root);
    assert_eq!(slot_texture_name(&app, root).as_deref(), Some("front0"));
    let before = mesh_uvs(&app, slot_entity);

    app.world_mut()
        .write_message(DragonBonesCommand::replace_display(
            root,
            "body_front",
            "body_front3",
        ));
    app.update();

    assert_eq!(
        slot_texture_name(&app, root).as_deref(),
        Some("front3"),
        "the command must resolve the replacement display's texture"
    );
    assert_ne!(
        mesh_uvs(&app, slot_entity),
        before,
        "the slot mesh must be rebuilt with the new UVs in the same frame"
    );
    assert!(command_errors(&app).is_empty());
}

#[test]
fn clear_display_command_empties_the_slot() {
    let mut app = test_app();
    let root = spawn_wallnut(&mut app);
    app.update();

    let (_, slot_entity) = body_front_slot(&app, root);
    app.world_mut()
        .write_message(DragonBonesCommand::clear_display(root, "body_front"));
    app.update();

    assert_eq!(slot_texture_name(&app, root), None);
    // The mesh is rebuilt to the empty placeholder (three zeroed vertices).
    assert!(
        mesh_uvs(&app, slot_entity)
            .iter()
            .all(|uv| uv == &[0.0, 0.0])
    );
    assert!(command_errors(&app).is_empty());
}

#[test]
fn unknown_skin_is_reported_without_changing_the_armature() {
    let mut app = test_app();
    let root = spawn_wallnut(&mut app);
    app.update();

    app.world_mut()
        .write_message(DragonBonesCommand::replace_skin(root, "does-not-exist"));
    app.update();

    let errors = command_errors(&app);
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].message.contains("does-not-exist"),
        "error should name the missing skin: {errors:?}"
    );
    assert_eq!(slot_texture_name(&app, root).as_deref(), Some("front0"));
}
