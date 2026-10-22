use bevy::math::{Affine3A, Mat4, Vec3A};
use bevy::prelude::*;
use dragonbones_core::armature::Slot;
use dragonbones_core::geom::{ColorTransform, Matrix};

/// Convert core's row-vector 2D matrix layout into Bevy's column-vector
/// `Affine3A`.
///
/// core stores `Matrix { a, b, c, d, tx, ty }` such that
/// `x' = a*x + c*y + tx`, `y' = b*x + d*y + ty`. Bevy's affine columns are
/// therefore `x_axis = (a, b)`, `y_axis = (c, d)` and `translation = (tx, ty)`.
#[must_use]
pub fn matrix_to_affine3a(matrix: &Matrix, z: f32) -> Affine3A {
    Affine3A::from_cols(
        Vec3A::new(matrix.a as f32, matrix.b as f32, 0.0),
        Vec3A::new(matrix.c as f32, matrix.d as f32, 0.0),
        Vec3A::new(0.0, 0.0, 1.0),
        Vec3A::new(matrix.tx as f32, matrix.ty as f32, z),
    )
}

/// Write an affine matrix into `GlobalTransform`, preserving change-detection
/// semantics (`set_if_neq`) so static slots do not wake Bevy's render
/// extraction every frame.
pub fn write_global_transform(global: &mut GlobalTransform, affine: Affine3A) {
    let next = GlobalTransform::from(Mat4::from(affine));
    if *global != next {
        *global = next;
    }
}

/// DragonBones color multiplier + alpha. Color offsets are intentionally not
/// represented by `ColorMaterial`; they are a post-MVP concern.
#[must_use]
pub fn color_from_transform(transform: &ColorTransform, global_alpha: f64) -> Color {
    let alpha = (transform.alpha_multiplier * global_alpha).clamp(0.0, 1.0) as f32;
    Color::linear_rgba(
        transform.red_multiplier.max(0.0) as f32,
        transform.green_multiplier.max(0.0) as f32,
        transform.blue_multiplier.max(0.0) as f32,
        alpha,
    )
}

/// Effective color for a slot, including its armature-wide alpha.
#[must_use]
pub fn slot_color(slot: &Slot) -> Color {
    color_from_transform(&slot.color_transform, slot.base.global_alpha)
}

/// Slot draw order is the position in `Armature::slots` after the core's
/// z-sort. Use a small negative z so Bevy's transparent phase draws earlier
/// slots first.
#[must_use]
pub fn slot_z(order: usize) -> f32 {
    // Bevy's 2D transparent phase sorts ascending by world z; higher z draws
    // later (in front). Core's slot Vec order is back-to-front.
    order as f32 * 0.001
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    #[test]
    fn matrix_conversion_preserves_point_transform() {
        let matrix = Matrix::new(2.0, 0.0, 1.0, 3.0, 10.0, 20.0);
        let affine = matrix_to_affine3a(&matrix, 7.0);
        let point = affine.transform_point3(Vec3::new(1.0, 2.0, 0.0));
        assert!((point.x - 14.0).abs() < 1e-5);
        assert!((point.y - 26.0).abs() < 1e-5);
        assert!((point.z - 7.0).abs() < 1e-5);
    }

    #[test]
    fn slot_z_is_back_to_front() {
        assert!(slot_z(0) < slot_z(1));
        assert!(slot_z(1) < slot_z(2));
    }

    #[test]
    fn color_multiplies_alpha() {
        let color = ColorTransform {
            red_multiplier: 0.5,
            green_multiplier: 0.25,
            blue_multiplier: 1.0,
            alpha_multiplier: 0.5,
            ..Default::default()
        };
        let out = color_from_transform(&color, 0.5).to_linear();
        assert!((out.red - 0.5).abs() < 1e-5);
        assert!((out.green - 0.25).abs() < 1e-5);
        assert!((out.blue - 1.0).abs() < 1e-5);
        assert!((out.alpha - 0.25).abs() < 1e-5);
    }
}
