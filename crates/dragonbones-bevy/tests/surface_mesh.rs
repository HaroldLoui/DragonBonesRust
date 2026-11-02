//! End-to-end test for Surface mesh displays.
//!
//! No shipped asset attaches a *mesh* display to a Surface bone (the
//! Surface_P1_4 fixture only uses image displays), so this test drives a
//! synthetic skeleton/atlas pair through the adapter. It asserts the reference
//! adapter semantics (`EgretSlot.ts:504-522`):
//!
//! - surface-mesh vertices live in armature space and are warped per vertex by
//!   `Surface._getGlobalTransformMatrix(x, y)`;
//! - the slot itself must use an identity local transform (our
//!   `SlotRenderKind::ArmatureSpace`), not its slot matrix.

use std::sync::Arc;

use bevy::asset::{AssetApp, AssetPlugin};
use bevy::prelude::*;
use bevy::time::Virtual;
use dragonbones_bevy::{
    AtlasAsset, DragonBonesArmature, DragonBonesInstance, DragonBonesPlugin, SkeletonAsset,
};
use dragonbones_core::model::TextureAtlasData;
use dragonbones_core::parser::ObjectDataParser;
use serde_json::{Value, json};

/// The Surface lattice follows its *parent* bone (`host`), as in the reference
/// implementation (`Surface.ts:_updateVertices`): with the lattice spanning the
/// canonical -200..200 space, a rigid `host` transform makes the per-point
/// Surface matrix equal to `host`'s global matrix.
fn skeleton_json(animated: bool) -> Value {
    let host_transform = if animated {
        json!({})
    } else {
        json!({ "x": 10, "y": 20 })
    };
    let animation = if animated {
        json!([{
            "duration": 10,
            "playTimes": 0,
            "name": "move",
            "bone": [{
                "name": "host",
                "translateFrame": [
                    { "duration": 10, "tweenEasing": 0 },
                    { "duration": 0, "tweenEasing": 0, "x": 100 }
                ]
            }]
        }])
    } else {
        json!([{ "duration": 1, "playTimes": 0, "name": "idle", "bone": [{ "name": "host" }] }])
    };
    json!({
        "frameRate": 24,
        "name": "SyntheticSurface",
        "version": "5.5",
        "compatibleVersion": "5.5",
        "armature": [{
            "type": "Armature",
            "frameRate": 24,
            "name": "Armature",
            "aabb": { "x": 0, "y": 0, "width": 100, "height": 100 },
            "bone": [
                { "name": "root" },
                { "name": "host", "parent": "root", "transform": host_transform },
                {
                    "name": "surf",
                    "parent": "host",
                    "type": "surface",
                    "segmentX": 1,
                    "segmentY": 1,
                    "vertices": [-200, -200, 200, -200, -200, 200, 200, 200]
                }
            ],
            "slot": [{ "name": "slot", "parent": "surf" }],
            "skin": [{
                "name": "",
                "slot": [{
                    "name": "slot",
                    "display": [{
                        "type": "mesh",
                        "name": "tex",
                        "width": 100,
                        "height": 100,
                        "vertices": [-20, -20, 20, -20, 20, 20, -20, 20],
                        "uvs": [0, 0, 1, 0, 1, 1, 0, 1],
                        "triangles": [0, 1, 2, 0, 2, 3]
                    }]
                }]
            }],
            "animation": animation
        }]
    })
}

fn atlas_json() -> Value {
    json!({
        "name": "SyntheticSurface",
        "imagePath": "synthetic.png",
        "width": 32,
        "height": 32,
        "SubTexture": [{ "name": "tex", "x": 0, "y": 0, "width": 32, "height": 32 }]
    })
}

fn app_with_surface_rig(animated: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Mesh>()
        .add_plugins(DragonBonesPlugin);

    let skeleton_data = ObjectDataParser::default()
        .parse_dragon_bones_data(&skeleton_json(animated), 1.0)
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
                armature_name: "Armature".to_owned(),
                animation: Some(if animated { "move" } else { "idle" }.to_owned()),
            },
            Transform::default(),
        ))
        .id();
    // Pause Bevy's virtual clock so the test controls animation time exactly
    // through explicit `advance_time` calls (the first updates would otherwise
    // advance by real elapsed time).
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    app.update();
    (app, root)
}

fn slot_mesh(app: &App, root: Entity) -> Mesh {
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance");
    let slot = instance.slots.first().copied().expect("one slot entity");
    let mesh_handle = app
        .world()
        .get::<Mesh2d>(slot)
        .expect("slot mesh")
        .0
        .clone();
    app.world()
        .resource::<Assets<Mesh>>()
        .get(&mesh_handle)
        .expect("mesh asset")
        .clone()
}

fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("unexpected position attribute: {other:?}"),
    }
}

fn assert_positions(mesh: &Mesh, expected: &[[f32; 3]], what: &str) {
    let got = positions(mesh);
    println!("{what}: got {got:?} expected {expected:?}");
    assert_eq!(got.len(), expected.len(), "{what}: vertex count");
    for (index, (got, want)) in got.iter().zip(expected.iter()).enumerate() {
        for axis in 0..2 {
            assert!(
                (got[axis] - want[axis]).abs() <= 1e-3,
                "{what}: vertex {index} axis {axis}: got {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn surface_mesh_vertices_are_warped_by_the_surface_matrix() {
    let (app, root) = app_with_surface_rig(false);
    let mesh = slot_mesh(&app, root);

    // Base mesh vertices (-20,-20)..(20,20) through a rigid surface matrix
    // translated by the host bone's (10, 20).
    assert_positions(
        &mesh,
        &[
            [-10.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [30.0, 40.0, 0.0],
            [-10.0, 40.0, 0.0],
        ],
        "static surface mesh",
    );

    // The slot itself must be armature-space (identity local transform), so its
    // world matrix is the root affine (identity here) plus the draw-order z.
    let instance = app
        .world()
        .get::<DragonBonesInstance>(root)
        .expect("adapter instance");
    let slot = instance.slots.first().copied().expect("one slot entity");
    let global = app
        .world()
        .get::<GlobalTransform>(slot)
        .expect("slot GT")
        .affine();
    assert!(global.translation.x.abs() < 1e-6);
    assert!(global.translation.y.abs() < 1e-6);
}

#[test]
fn surface_mesh_follows_an_animated_surface_bone() {
    let (mut app, root) = app_with_surface_rig(true);
    // Let the "move" animation run to its middle (5 of 10 frames): the host bone
    // translates 0..100, so the surface mesh should shift by ~50 on X.
    {
        let mut runtime = app
            .world_mut()
            .get_non_send_mut::<dragonbones_bevy::DragonBonesRuntime>()
            .expect("runtime resource");
        runtime.factory.advance_time(5.0 / 24.0);
    }
    app.update();

    let mesh = slot_mesh(&app, root);
    let got = positions(&mesh);
    let base = [
        [-20.0f32, -20.0],
        [20.0, -20.0],
        [20.0, 20.0],
        [-20.0, 20.0],
    ];
    // Interpolated X offset is ~50; Y stays at 0 because the timeline only moves
    // X.
    for (index, vertex) in got.iter().enumerate() {
        let dx = vertex[0] - base[index][0];
        let dy = vertex[1] - base[index][1];
        assert!(
            (dx - 50.0).abs() < 2.0,
            "vertex {index}: expected x offset ~50, got {dx} ({vertex:?})"
        );
        assert!(
            dy.abs() < 2.0,
            "vertex {index}: expected y offset ~0, got {dy}"
        );
    }
}
