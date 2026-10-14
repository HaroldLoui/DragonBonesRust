//! 阶段 2 黄金数据快照辅助：与 `tools/irdump.mjs` 的 `structure` / `atlas` / `meta`
//! 段一一对应，并提供 `norm()` 消除 JS 整数与 Rust 浮点的序列化变体差异。

use std::path::{Path, PathBuf};

use dragonbones_core::core::enums::{AnimationBlendType, TimelineType};
use dragonbones_core::model::animation_data::TimelineData;
use dragonbones_core::model::armature_data::BoneKind;
use dragonbones_core::model::display_data::{DisplayData, DisplayKind, GeometryData};
use dragonbones_core::model::dragon_bones_data::DragonBonesData;
use dragonbones_core::model::texture_atlas_data::{TextureAtlasData, TextureData};
use dragonbones_core::parser::ObjectDataParser;
use serde_json::{Number, Value, json};

pub fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

pub fn ir_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ir")
}

pub fn read_json(path: &Path) -> Value {
    let s = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("parse {} failed: {e}", path.display()))
}

/// `X_ske.json` / `X_ske_compressed.json` → `X_tex.json`（先匹配 compressed）。
pub fn tex_stem(ske_stem: &str) -> String {
    ske_stem
        .replace("_ske_compressed", "_tex")
        .replace("_ske", "_tex")
}

/// 解析单个素材：返回 (骨架数据, 图集)。
pub fn parse_asset(ske_stem: &str) -> (DragonBonesData, TextureAtlasData) {
    let ske = read_json(&asset_root().join(format!("{ske_stem}.json")));
    let tex = read_json(&asset_root().join(format!("{}.json", tex_stem(ske_stem))));

    let mut parser = ObjectDataParser::new();
    let data = parser
        .parse_dragon_bones_data(&ske, 1.0)
        .unwrap_or_else(|| panic!("parse {ske_stem} failed"));
    let mut atlas = TextureAtlasData::default();
    parser.parse_texture_atlas_data(&tex, &mut atlas, 1.0);

    (data, atlas)
}

/// 递归把所有 `Number` 重写为 `from_f64(as_f64)`，消除 u64/i64/f64 序列化变体差异。
#[must_use]
pub fn norm(v: Value) -> Value {
    match v {
        Value::Number(n) => Value::Number(
            Number::from_f64(n.as_f64().unwrap_or(f64::NAN)).unwrap_or_else(|| Number::from(0)),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, norm(v))).collect()),
        other => other,
    }
}

fn opt_str(v: &Option<String>) -> Value {
    v.clone().map_or(Value::Null, Value::String)
}

fn transform_value(t: &dragonbones_core::geom::transform::Transform) -> Value {
    json!([t.x, t.y, t.skew, t.rotation, t.scale_x, t.scale_y])
}

fn color_value(c: &dragonbones_core::geom::color::ColorTransform) -> Value {
    json!([
        c.alpha_multiplier,
        c.red_multiplier,
        c.green_multiplier,
        c.blue_multiplier,
        c.alpha_offset,
        c.red_offset,
        c.green_offset,
        c.blue_offset
    ])
}

fn geometry_value(g: &GeometryData) -> Value {
    let weight = g
        .weight
        .as_ref()
        .map(|w| json!({"count": w.count, "offset": w.offset, "bones": w.bones}))
        .unwrap_or(Value::Null);
    json!({
        "offset": g.offset,
        "isShared": g.is_shared,
        "inheritDeform": g.inherit_deform,
        "weight": weight,
    })
}

fn action_value(a: &dragonbones_core::model::user_data::ActionData) -> Value {
    json!([
        a.action_type as u32,
        a.name,
        opt_str(&a.bone),
        opt_str(&a.slot)
    ])
}

fn display_value(d: &DisplayData) -> Value {
    let mut base = json!({
        "name": d.name,
        "type": d.type_() as u32,
        "path": d.path,
        "transform": transform_value(&d.transform),
    });

    match &d.kind {
        DisplayKind::Image { pivot, .. } => {
            base["pivot"] = json!([pivot.x, pivot.y]);
        }
        DisplayKind::Armature {
            inherit_animation,
            actions,
            armature,
        } => {
            base["inheritAnimation"] = json!(inherit_animation);
            base["armature"] = opt_str(armature);
            base["actions"] = Value::Array(actions.iter().map(action_value).collect());
        }
        DisplayKind::Mesh { geometry, .. } => {
            base["geometry"] = geometry_value(geometry);
        }
        DisplayKind::BoundingBox { bounding_box } => {
            if let Some(bb) = bounding_box {
                let vertices = match &bb.kind {
                    dragonbones_core::model::bounding_box::BoundingBoxKind::Polygon {
                        vertices,
                        ..
                    } => vertices.clone(),
                    _ => Vec::new(),
                };
                base["boundingBox"] = json!({
                    "type": bb.type_() as u32,
                    "color": bb.color,
                    "width": bb.width,
                    "height": bb.height,
                    "vertices": vertices,
                });
            }
        }
        DisplayKind::Path {
            closed,
            constant_speed,
            geometry,
            ..
        } => {
            base["closed"] = json!(closed);
            base["constantSpeed"] = json!(constant_speed);
            base["geometry"] = geometry_value(geometry);
        }
    }

    base
}

fn timeline_value(t: &TimelineData, blend_type: AnimationBlendType) -> Value {
    let mut m = json!({
        "type": t.timeline_type as u32,
        "offset": t.offset,
        "frameIndicesOffset": t.frame_indices_offset,
    });
    if t.timeline_type == TimelineType::AnimationProgress && blend_type != AnimationBlendType::None
    {
        m["x"] = json!(t.x);
        m["y"] = json!(t.y);
    }
    m
}

fn timelines_map(
    map: &indexmap::IndexMap<String, Vec<TimelineData>>,
    blend_type: AnimationBlendType,
) -> Value {
    let mut out = serde_json::Map::new();
    for (k, timelines) in map {
        out.insert(
            k.clone(),
            Value::Array(
                timelines
                    .iter()
                    .map(|t| timeline_value(t, blend_type))
                    .collect(),
            ),
        );
    }
    Value::Object(out)
}

pub fn snapshot_meta(data: &DragonBonesData, ske_file: &str) -> Value {
    json!({
        "skeFile": ske_file,
        "version": data.version,
        "name": data.name,
        "frameRate": data.frame_rate,
        "stage": opt_str(&data.stage),
        "armatureNames": data.armature_names,
    })
}

pub fn snapshot_structure(data: &DragonBonesData) -> Value {
    let mut armatures = serde_json::Map::new();

    for (armature_name, armature) in &data.armatures {
        let mut bones = serde_json::Map::new();
        for (name, b) in &armature.bones {
            let mut entry = json!({
                "parent": opt_str(&b.parent),
                "type": b.type_() as u32,
                "length": b.length,
                "alpha": b.alpha,
                "inheritTranslation": b.inherit_translation,
                "inheritRotation": b.inherit_rotation,
                "inheritScale": b.inherit_scale,
                "inheritReflection": b.inherit_reflection,
                "transform": transform_value(&b.transform),
            });
            if let BoneKind::Surface {
                segment_x,
                segment_y,
                geometry,
            } = &b.kind
            {
                entry["segmentX"] = json!(segment_x);
                entry["segmentY"] = json!(segment_y);
                entry["geometry"] = geometry_value(geometry);
            }
            bones.insert(name.clone(), entry);
        }

        let mut slots = serde_json::Map::new();
        for (name, s) in &armature.slots {
            slots.insert(
                name.clone(),
                json!({
                    "parent": s.parent,
                    "displayIndex": s.display_index,
                    "zOrder": s.z_order,
                    "zIndex": s.z_index,
                    "alpha": s.alpha,
                    "blendMode": s.blend_mode as u32,
                    "color": color_value(&s.color),
                }),
            );
        }

        let mut constraints = serde_json::Map::new();
        for (name, c) in &armature.constraints {
            let mut entry = json!({
                "type": c.type_() as u32,
                "order": c.order,
                "target": c.target,
                "root": c.root,
                "bone": opt_str(&c.bone),
            });
            match &c.kind {
                dragonbones_core::model::constraint_data::ConstraintKind::IK {
                    scale_enabled,
                    bend_positive,
                    weight,
                } => {
                    entry["scaleEnabled"] = json!(scale_enabled);
                    entry["bendPositive"] = json!(bend_positive);
                    entry["weight"] = json!(weight);
                }
                dragonbones_core::model::constraint_data::ConstraintKind::Path {
                    path_slot,
                    position_mode,
                    spacing_mode,
                    rotate_mode,
                    position,
                    spacing,
                    rotate_offset,
                    rotate_mix,
                    translate_mix,
                    bones,
                    ..
                } => {
                    entry["positionMode"] = json!(*position_mode as u32);
                    entry["spacingMode"] = json!(*spacing_mode as u32);
                    entry["rotateMode"] = json!(*rotate_mode as u32);
                    entry["position"] = json!(position);
                    entry["spacing"] = json!(spacing);
                    entry["rotateOffset"] = json!(rotate_offset);
                    entry["rotateMix"] = json!(rotate_mix);
                    entry["translateMix"] = json!(translate_mix);
                    entry["pathSlot"] = json!(path_slot);
                    entry["bones"] = json!(bones);
                }
            }
            constraints.insert(name.clone(), entry);
        }

        let mut skins = serde_json::Map::new();
        for (skin_name, skin) in &armature.skins {
            let mut slot_displays = serde_json::Map::new();
            for (slot_name, displays) in &skin.displays {
                let arr: Vec<Value> = displays
                    .iter()
                    .map(|d| d.as_ref().map_or(Value::Null, display_value))
                    .collect();
                slot_displays.insert(slot_name.clone(), Value::Array(arr));
            }
            skins.insert(skin_name.clone(), Value::Object(slot_displays));
        }

        let mut animations = serde_json::Map::new();
        for (anim_name, n) in &armature.animations {
            let blend = n.blend_type;
            animations.insert(
                anim_name.clone(),
                json!({
                    "frameCount": n.frame_count,
                    "playTimes": n.play_times,
                    "duration": n.duration,
                    "scale": n.scale,
                    "fadeInTime": n.fade_in_time,
                    "blendType": n.blend_type as u32,
                    "frameIntOffset": n.frame_int_offset,
                    "frameFloatOffset": n.frame_float_offset,
                    "frameOffset": n.frame_offset,
                    "actionTimeline": n.action_timeline.as_ref().map_or(Value::Null, |t| timeline_value(t, blend)),
                    "zOrderTimeline": n.z_order_timeline.as_ref().map_or(Value::Null, |t| timeline_value(t, blend)),
                    "boneTimelines": timelines_map(&n.bone_timelines, blend),
                    "slotTimelines": timelines_map(&n.slot_timelines, blend),
                    "constraintTimelines": timelines_map(&n.constraint_timelines, blend),
                    "animationTimelines": timelines_map(&n.animation_timelines, blend),
                }),
            );
        }

        armatures.insert(
            armature_name.clone(),
            json!({
                "type": armature.armature_type as u32,
                "frameRate": armature.frame_rate,
                "scale": armature.scale,
                "aabb": [armature.aabb.x, armature.aabb.y, armature.aabb.width, armature.aabb.height],
                "animationNames": armature.animation_names,
                "sortedBones": armature.sorted_bones,
                "sortedSlots": armature.sorted_slots,
                "defaultSkin": opt_str(&armature.default_skin),
                "defaultAnimation": opt_str(&armature.default_animation),
                "defaultActions": armature.default_actions.iter().map(action_value).collect::<Vec<_>>(),
                "actions": armature.actions.iter().map(action_value).collect::<Vec<_>>(),
                "bones": Value::Object(bones),
                "slots": Value::Object(slots),
                "constraints": Value::Object(constraints),
                "skins": Value::Object(skins),
                "animations": Value::Object(animations),
            }),
        );
    }

    json!({ "armatures": Value::Object(armatures) })
}

fn texture_value(t: &TextureData) -> Value {
    json!({
        "rotated": t.rotated,
        "name": t.name,
        "region": [t.region.x, t.region.y, t.region.width, t.region.height],
        "frame": t.frame.map(|f| json!([f.x, f.y, f.width, f.height])).unwrap_or(Value::Null),
    })
}

pub fn snapshot_atlas(atlas: &TextureAtlasData) -> Value {
    let mut textures = serde_json::Map::new();
    for (name, t) in &atlas.textures {
        textures.insert(name.clone(), texture_value(t));
    }

    Value::Array(vec![json!({
        "width": atlas.width,
        "height": atlas.height,
        "scale": atlas.scale,
        "name": atlas.name,
        "imagePath": atlas.image_path,
        "autoSearch": atlas.auto_search,
        "textures": Value::Object(textures),
    })])
}
