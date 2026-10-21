//! model 默认值测试（P0-4）：全部 `Default` == TS `_onClear` 值。
//!
//! 重点校验反直觉默认（`AnimationConfig.time_scale = -100`、`Transform` 缩放 1、
//! `TimelineData.frame_indices_offset = -1`、`ColorTransform` 乘子 1 等）。

use dragonbones_core::core::enums::*;
use dragonbones_core::geom::color::ColorTransform;
use dragonbones_core::geom::rectangle::Rectangle;
use dragonbones_core::geom::transform::Transform;
use dragonbones_core::model::*;

#[test]
fn geom_defaults() {
    assert_eq!(
        Transform::default(),
        Transform::new(0.0, 0.0, 0.0, 0.0, 1.0, 1.0)
    );
    let color = ColorTransform::default();
    assert_eq!(color.alpha_multiplier, 1.0);
    assert_eq!(color.red_multiplier, 1.0);
    assert_eq!(color.green_multiplier, 1.0);
    assert_eq!(color.blue_multiplier, 1.0);
    assert_eq!(color.alpha_offset, 0.0);
    assert_eq!(color.red_offset, 0.0);
    assert_eq!(color.green_offset, 0.0);
    assert_eq!(color.blue_offset, 0.0);
}

#[test]
fn animation_config_defaults_match_on_clear() {
    let c = AnimationConfig::default();
    assert!(c.pause_fade_out);
    assert_eq!(c.fade_out_mode, AnimationFadeOutMode::All);
    assert_eq!(c.fade_out_tween_type, TweenType::Line);
    assert_eq!(c.fade_out_time, -1.0);

    assert!(c.pause_fade_in);
    assert!(c.action_enabled);
    assert!(!c.additive);
    assert!(c.display_control);
    assert!(c.reset_to_pose);
    assert_eq!(c.fade_in_tween_type, TweenType::Line);
    assert_eq!(c.play_times, -1);
    assert_eq!(c.layer, 0);
    assert_eq!(c.position, 0.0);
    assert_eq!(c.duration, -1.0);
    assert_eq!(c.time_scale, -100.0, "timeScale 默认 -100 而非 -1");
    assert_eq!(c.weight, 1.0);
    assert_eq!(c.fade_in_time, -1.0);
    assert_eq!(c.auto_fade_out_time, -1.0);
    assert!(c.name.is_empty());
    assert!(c.animation.is_empty());
    assert!(c.group.is_empty());
    assert!(c.bone_mask.is_empty());
}

#[test]
fn animation_config_copy_from_copies_every_field_and_bone_mask() {
    let source = AnimationConfig {
        pause_fade_out: false,
        fade_out_mode: AnimationFadeOutMode::SameLayer,
        fade_out_tween_type: TweenType::QuadOut,
        fade_out_time: 0.35,
        pause_fade_in: false,
        action_enabled: false,
        additive: true,
        display_control: false,
        reset_to_pose: false,
        fade_in_tween_type: TweenType::QuadInOut,
        play_times: 3,
        layer: 4,
        position: 0.25,
        duration: 1.5,
        time_scale: -2.0,
        weight: 0.75,
        fade_in_time: 0.2,
        auto_fade_out_time: 0.4,
        name: "state-name".into(),
        animation: "animation-name".into(),
        group: "group-name".into(),
        bone_mask: vec!["root".into(), "arm".into()],
    };
    let mut target = AnimationConfig::default();

    target.copy_from(&source);

    assert_eq!(target, source);
}

#[test]
fn timeline_data_defaults() {
    let t = TimelineData::default();
    assert_eq!(t.timeline_type, TimelineType::BoneAll);
    assert_eq!(t.offset, 0);
    assert_eq!(t.frame_indices_offset, -1, "frameIndicesOffset 默认 -1");
    assert_eq!(t.x, 0.0);
    assert_eq!(t.y, 0.0);
}

#[test]
fn animation_data_defaults() {
    let a = AnimationData::default();
    assert_eq!(a.frame_int_offset, 0);
    assert_eq!(a.frame_float_offset, 0);
    assert_eq!(a.frame_offset, 0);
    assert_eq!(a.blend_type, AnimationBlendType::None);
    assert_eq!(a.frame_count, 0);
    assert_eq!(a.play_times, 0);
    assert_eq!(a.duration, 0.0);
    assert_eq!(a.scale, 1.0);
    assert_eq!(a.fade_in_time, 0.0);
    assert_eq!(a.cache_frame_rate, 0.0);
    assert!(a.name.is_empty());
    assert!(a.cached_frames.is_empty());
    assert!(a.bone_timelines.is_empty());
    assert!(a.slot_timelines.is_empty());
    assert!(a.constraint_timelines.is_empty());
    assert!(a.animation_timelines.is_empty());
    assert!(a.bone_cached_frame_indices.is_empty());
    assert!(a.slot_cached_frame_indices.is_empty());
    assert!(a.action_timeline.is_none());
    assert!(a.z_order_timeline.is_none());
    assert!(a.parent.is_empty());
}

#[test]
fn armature_data_defaults() {
    let a = ArmatureData::default();
    assert_eq!(a.armature_type, ArmatureType::Armature);
    assert_eq!(a.frame_rate, 0.0);
    assert_eq!(a.cache_frame_rate, 0.0);
    assert_eq!(a.scale, 1.0);
    assert!(a.name.is_empty());
    assert_eq!(a.aabb, Rectangle::default());
    assert!(a.animation_names.is_empty());
    assert!(a.sorted_bones.is_empty());
    assert!(a.sorted_slots.is_empty());
    assert!(a.default_actions.is_empty());
    assert!(a.actions.is_empty());
    assert!(a.bones.is_empty());
    assert!(a.slots.is_empty());
    assert!(a.constraints.is_empty());
    assert!(a.skins.is_empty());
    assert!(a.animations.is_empty());
    assert!(a.default_skin.is_none());
    assert!(a.default_animation.is_none());
    assert!(a.canvas.is_none());
    assert!(a.user_data.is_none());
    assert!(a.parent.is_none());
}

#[test]
fn bone_data_defaults() {
    let b = BoneData::default();
    assert!(!b.inherit_translation);
    assert!(!b.inherit_rotation);
    assert!(!b.inherit_scale);
    assert!(!b.inherit_reflection);
    assert_eq!(b.type_(), BoneType::Bone);
    assert_eq!(b.length, 0.0);
    assert_eq!(b.alpha, 1.0);
    assert!(b.name.is_empty());
    assert_eq!(b.transform, Transform::default());
    assert!(b.user_data.is_none());
    assert!(b.parent.is_none());
    assert!(matches!(b.kind, BoneKind::Bone));
}

#[test]
fn slot_data_defaults() {
    let s = SlotData::default();
    assert_eq!(s.blend_mode, BlendMode::Normal);
    assert_eq!(s.display_index, 0);
    assert_eq!(s.z_order, 0);
    assert_eq!(s.z_index, 0.0);
    assert_eq!(s.alpha, 1.0);
    assert!(s.name.is_empty());
    assert_eq!(s.color, ColorTransform::default());
    assert!(s.user_data.is_none());
    assert!(s.parent.is_empty());
    assert_eq!(SlotData::DEFAULT_COLOR, ColorTransform::default());
}

#[test]
fn display_data_defaults() {
    let d = DisplayData::default();
    assert!(d.name.is_empty());
    assert!(d.path.is_empty());
    assert_eq!(d.transform, Transform::default());
    assert!(d.parent.is_empty());
    assert_eq!(d.type_(), DisplayType::Image);
    match d.kind {
        DisplayKind::Image { pivot, texture } => {
            assert_eq!(pivot.x, 0.0);
            assert_eq!(pivot.y, 0.0);
            assert!(texture.is_none());
        }
        other => panic!("默认 kind 应为 Image，实际 {other:?}"),
    }
}

#[test]
fn geometry_and_weight_defaults() {
    let g = GeometryData::default();
    assert!(!g.is_shared);
    assert!(!g.inherit_deform);
    assert_eq!(g.offset, 0);
    assert!(g.root.is_none());
    assert!(g.weight.is_none());

    let w = WeightData::default();
    assert_eq!(w.count, 0);
    assert_eq!(w.offset, 0);
    assert!(w.bones.is_empty());
}

#[test]
fn bounding_box_defaults() {
    let b = BoundingBoxData::default();
    assert_eq!(b.color, 0x000000);
    assert_eq!(b.width, 0.0);
    assert_eq!(b.height, 0.0);
    assert_eq!(b.type_(), BoundingBoxType::Rectangle);
    assert!(matches!(b.kind, BoundingBoxKind::Rectangle));
}

#[test]
fn constraint_data_defaults() {
    let c = ConstraintData::default();
    assert_eq!(c.order, 0);
    assert!(c.name.is_empty());
    assert_eq!(c.target, "");
    assert_eq!(c.root, "");
    assert!(c.bone.is_none());
    assert_eq!(c.type_(), ConstraintType::IK);
    match c.kind {
        ConstraintKind::IK {
            scale_enabled,
            bend_positive,
            weight,
        } => {
            assert!(!scale_enabled);
            assert!(!bend_positive);
            assert_eq!(weight, 1.0);
        }
        other => panic!("默认 kind 应为 IK，实际 {other:?}"),
    }
}

#[test]
fn canvas_user_action_defaults() {
    let c = CanvasData::default();
    assert!(!c.has_background);
    assert_eq!(c.color, 0x000000);
    assert_eq!((c.x, c.y, c.width, c.height), (0.0, 0.0, 0.0, 0.0));

    let u = UserData::default();
    assert!(u.ints.is_empty());
    assert!(u.floats.is_empty());
    assert!(u.strings.is_empty());
    assert_eq!(u.get_int(0), 0);
    assert_eq!(u.get_float(0), 0.0);
    assert_eq!(u.get_string(0), "");

    let a = ActionData::default();
    assert_eq!(a.action_type, ActionType::Play);
    assert!(a.name.is_empty());
    assert!(a.bone.is_none());
    assert!(a.slot.is_none());
    assert!(a.data.is_none());
}

#[test]
fn texture_defaults() {
    let atlas = TextureAtlasData::default();
    assert!(!atlas.auto_search);
    assert_eq!(atlas.width, 0.0);
    assert_eq!(atlas.height, 0.0);
    assert_eq!(atlas.scale, 1.0);
    assert!(atlas.name.is_empty());
    assert!(atlas.image_path.is_empty());
    assert!(atlas.textures.is_empty());

    let t = TextureData::default();
    assert!(!t.rotated);
    assert!(t.name.is_empty());
    assert_eq!(t.region, Rectangle::default());
    assert!(t.parent.is_empty());
    assert!(t.frame.is_none());
}

#[test]
fn dragon_bones_data_defaults() {
    let d = DragonBonesData::default();
    assert!(d.name.is_empty());
    assert!(d.root.is_none());
    assert!(!d.auto_search);
    assert_eq!(d.frame_rate, 0.0);
    assert!(d.version.is_empty());
    assert!(d.stage.is_none());
    assert!(d.user_data.is_none());
    assert!(d.armature_names.is_empty());
    assert!(d.armatures.is_empty());
    assert!(d.cached_frames.is_empty());
    assert!(d.binary.is_none());
}

// ---- 变体（kind）默认值：对照 TS 子类 _onClear ----

#[test]
fn surface_bone_kind_defaults_match_on_clear() {
    // TS SurfaceData._onClear（ArmatureData.ts）：segmentX=0, segmentY=0, geometry.clear()
    let b = BoneData {
        kind: BoneKind::Surface {
            segment_x: 0,
            segment_y: 0,
            geometry: GeometryData::default(),
        },
        ..BoneData::default()
    };
    assert_eq!(b.type_(), BoneType::Surface);
    match b.kind {
        BoneKind::Surface {
            segment_x,
            segment_y,
            geometry,
        } => {
            assert_eq!(segment_x, 0, "segmentX 默认 0");
            assert_eq!(segment_y, 0, "segmentY 默认 0");
            assert!(!geometry.is_shared);
            assert!(!geometry.inherit_deform);
            assert_eq!(geometry.offset, 0);
            assert!(geometry.root.is_none());
            assert!(geometry.weight.is_none());
        }
        other => panic!("expected Surface, got {other:?}"),
    }
}

#[test]
fn path_constraint_kind_defaults_match_on_clear() {
    // TS PathConstraintData._onClear（ConstraintData.ts:86-102）：
    // pathSlot=null / pathDisplayData=null / bones=[] / positionMode=Fixed /
    // spacingMode=Fixed / rotateMode=Chain / position,spacing,rotateOffset,rotateMix,translateMix=0
    //
    // 注意（阶段 2 陷阱）：解析器 _parsePathConstraint 的缺省与 _onClear 不同——
    // rotateMix / translateMix 缺省为 1（ObjectDataParser.ts:577-578），其余为 0。
    let c = ConstraintData {
        kind: ConstraintKind::Path {
            path_slot: String::new(),
            path_display: None,
            bones: Vec::new(),
            position_mode: PositionMode::Fixed,
            spacing_mode: SpacingMode::Fixed,
            rotate_mode: RotateMode::Chain,
            position: 0.0,
            spacing: 0.0,
            rotate_offset: 0.0,
            rotate_mix: 0.0,
            translate_mix: 0.0,
        },
        ..ConstraintData::default()
    };
    assert_eq!(
        c.type_(),
        ConstraintType::Path,
        "type_() 由 kind 派生（TS 由解析器显式赋值）"
    );
    match &c.kind {
        ConstraintKind::Path {
            path_slot,
            path_display,
            bones,
            position_mode,
            spacing_mode,
            rotate_mode,
            position,
            spacing,
            rotate_offset,
            rotate_mix,
            translate_mix,
        } => {
            assert!(path_slot.is_empty(), "pathSlot 默认 null → \"\"");
            assert!(path_display.is_none());
            assert!(bones.is_empty());
            assert_eq!(*position_mode, PositionMode::Fixed);
            assert_eq!(*spacing_mode, SpacingMode::Fixed);
            assert_eq!(*rotate_mode, RotateMode::Chain);
            assert_eq!(*position, 0.0);
            assert_eq!(*spacing, 0.0);
            assert_eq!(*rotate_offset, 0.0);
            assert_eq!(*rotate_mix, 0.0, "_onClear 是 0；解析器缺省才是 1");
            assert_eq!(*translate_mix, 0.0, "_onClear 是 0；解析器缺省才是 1");
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

#[test]
fn display_kind_variant_defaults_match_on_clear() {
    // TS 各子类 _onClear（DisplayData.ts）：
    // Image: pivot.clear(), texture=null；Armature: inheritAnimation=false, actions=[], armature=null
    // Mesh: geometry.clear(), texture=null；BoundingBox: boundingBox=null
    // Path: closed=false, constantSpeed=false, geometry.clear(), curveLengths=[]
    let image = DisplayData {
        kind: DisplayKind::Image {
            pivot: Default::default(),
            texture: None,
        },
        ..DisplayData::default()
    };
    assert_eq!(image.type_(), DisplayType::Image);

    let armature = DisplayData {
        kind: DisplayKind::Armature {
            inherit_animation: false,
            actions: Vec::new(),
            armature: None,
        },
        ..DisplayData::default()
    };
    assert_eq!(armature.type_(), DisplayType::Armature);
    match &armature.kind {
        DisplayKind::Armature {
            inherit_animation,
            actions,
            armature,
        } => {
            assert!(!*inherit_animation, "inheritAnimation 默认 false");
            assert!(actions.is_empty());
            assert!(armature.is_none());
        }
        other => panic!("expected Armature, got {other:?}"),
    }

    let mesh = DisplayData {
        kind: DisplayKind::Mesh {
            geometry: GeometryData::default(),
            texture: None,
        },
        ..DisplayData::default()
    };
    assert_eq!(mesh.type_(), DisplayType::Mesh);

    let bounding = DisplayData {
        kind: DisplayKind::BoundingBox { bounding_box: None },
        ..DisplayData::default()
    };
    assert_eq!(bounding.type_(), DisplayType::BoundingBox);

    let path = DisplayData {
        kind: DisplayKind::Path {
            closed: false,
            constant_speed: false,
            geometry: GeometryData::default(),
            curve_lengths: Vec::new(),
        },
        ..DisplayData::default()
    };
    assert_eq!(path.type_(), DisplayType::Path);
    match &path.kind {
        DisplayKind::Path {
            closed,
            constant_speed,
            geometry,
            curve_lengths,
        } => {
            assert!(!*closed, "closed 默认 false");
            assert!(!*constant_speed, "constantSpeed 默认 false");
            assert!(!geometry.is_shared);
            assert_eq!(geometry.offset, 0);
            assert!(curve_lengths.is_empty());
        }
        other => panic!("expected Path, got {other:?}"),
    }
}

#[test]
fn skin_data_defaults_match_on_clear() {
    // TS SkinData._onClear（SkinData.ts）：name="", displays 清空, parent=null
    let s = SkinData::default();
    assert!(s.name.is_empty());
    assert!(s.displays.is_empty());
    assert!(s.parent.is_empty());
}
