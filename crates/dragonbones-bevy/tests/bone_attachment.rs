//! C2 bone attachment: an entity follows a bone without spawning bone ECS entities.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::transform::TransformPlugin;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesBoneAttachment, DragonBonesInstance,
    DragonBonesPlugin, DragonBonesRuntime, SkeletonAsset,
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
        .add_plugins(TransformPlugin)
        .add_plugins(DragonBonesPlugin);
    app
}

fn install_dragon(app: &mut App) -> Entity {
    let dir = assets_dir();
    let skeleton_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon/Dragon_ske.json")).expect("read skeleton"),
    )
    .expect("parse skeleton");
    let atlas_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("Dragon/Dragon_tex.json")).expect("read atlas"),
    )
    .expect("parse atlas");
    let mut parser = ObjectDataParser::default();
    let data = parser
        .parse_dragon_bones_data(&skeleton_json, 1.0)
        .expect("parse Dragon");
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
    app.world_mut()
        .spawn((
            DragonBonesArmature {
                skeleton,
                atlas,
                armature_name: "Dragon".to_owned(),
                animation: Some("walk".to_owned()),
            },
            Transform::from_xyz(10.0, 20.0, 0.0),
        ))
        .id()
}

#[test]
fn bone_attachment_follows_the_root_and_bone() {
    let mut app = test_app();
    let root = install_dragon(&mut app);
    app.update();

    let attachment = app
        .world_mut()
        .spawn((
            DragonBonesBoneAttachment {
                root,
                bone: "root".to_owned(),
                offset: Transform::from_xyz(5.0, 0.0, 0.0),
            },
            GlobalTransform::default(),
            Visibility::default(),
        ))
        .id();

    app.update();
    let armature = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("instance")
        .armature;
    let runtime = app
        .world()
        .get_non_send::<DragonBonesRuntime>()
        .expect("runtime");
    let root_affine = app
        .world()
        .get::<GlobalTransform>(root)
        .expect("root GT")
        .affine();
    let bone = runtime
        .bone_transform(armature, "root")
        .expect("root bone transform");
    let expected = root_affine * bone * Affine3A::from_translation(Vec3::new(5.0, 0.0, 0.0));
    let got = app
        .world()
        .get::<GlobalTransform>(attachment)
        .expect("attachment GT")
        .affine();
    let want = expected.translation;
    let got = got.translation;
    assert!(
        (got - want).length() < 1e-4,
        "attachment should be at the bone + offset: want {want:?}, got {got:?}"
    );

    // A second update must not let Bevy's propagation move the attachment away.
    app.update();
    let after = app
        .world()
        .get::<GlobalTransform>(attachment)
        .expect("attachment GT")
        .affine()
        .translation;
    assert!(
        (after - want).length() < 1e-4,
        "attachment must stay on the bone across updates: want {want:?}, got {after:?}"
    );
}
