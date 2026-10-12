//! geom 差分测试：Rust 实现对齐 JS 参考运行时（`testdata/geom/vectors.json`，704 用例）。

use dragonbones_core::geom::matrix::Matrix;
use dragonbones_core::geom::rectangle::Rectangle;
use dragonbones_core::geom::transform::{Transform, normalize_radian};
use serde_json::Value;
use std::path::PathBuf;

fn vectors() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/geom/vectors.json");
    let s = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("parse {} failed: {e}", path.display()))
}

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().unwrap_or(f64::NAN))
        .collect()
}

fn matrix_from(v: &Value) -> Matrix {
    let a = f64s(v);
    assert_eq!(a.len(), 6, "matrix 应为 6 个数");
    Matrix::new(a[0], a[1], a[2], a[3], a[4], a[5])
}

fn matrix_to_arr(m: &Matrix) -> [f64; 6] {
    [m.a, m.b, m.c, m.d, m.tx, m.ty]
}

fn transform_from(v: &Value) -> Transform {
    let a = f64s(v);
    assert_eq!(a.len(), 6, "transform 应为 6 个数");
    Transform::new(a[0], a[1], a[2], a[3], a[4], a[5])
}

fn transform_to_arr(t: &Transform) -> [f64; 6] {
    [t.x, t.y, t.skew, t.rotation, t.scale_x, t.scale_y]
}

fn assert_close(got: &[f64], want: &[f64], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx}: 长度不同");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if g.is_nan() && w.is_nan() {
            continue; // JSON null 表示 NaN 哨兵
        }
        let tol = 1e-9 * (1.0 + w.abs());
        assert!(
            (g - w).abs() <= tol,
            "{ctx}[{i}]: got {g}, want {w} (diff {})",
            (g - w).abs()
        );
    }
}

#[test]
fn geom_concat_matches_js() {
    let v = vectors();
    for (i, case) in v["concat"].as_array().unwrap().iter().enumerate() {
        let mut m = matrix_from(&case["a"]);
        m.concat(&matrix_from(&case["b"]));
        let got = matrix_to_arr(&m);
        let want = f64s(&case["expect"]);
        assert_close(
            &got,
            &want,
            &format!("concat[{i}] a={:?}", f64s(&case["a"])),
        );
    }
}

#[test]
fn geom_invert_matches_js() {
    let v = vectors();
    for (i, case) in v["invert"].as_array().unwrap().iter().enumerate() {
        let mut m = matrix_from(&case["a"]);
        m.invert();
        let got = matrix_to_arr(&m);
        let want = f64s(&case["expect"]);
        assert_close(
            &got,
            &want,
            &format!("invert[{i}] a={:?}", f64s(&case["a"])),
        );
    }
}

#[test]
fn geom_transform_point_matches_js() {
    let v = vectors();
    for (i, case) in v["transformPoint"].as_array().unwrap().iter().enumerate() {
        let m = matrix_from(&case["m"]);
        let x = case["x"].as_f64().unwrap();
        let y = case["y"].as_f64().unwrap();
        let delta = case["delta"].as_bool().unwrap();
        let (gx, gy) = m.transform_point(x, y, delta);
        let want = f64s(&case["expect"]);
        assert_close(&[gx, gy], &want, &format!("transformPoint[{i}]"));
    }
}

#[test]
fn geom_transform_rectangle_matches_js() {
    let v = vectors();
    for (i, case) in v["transformRectangle"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let m = matrix_from(&case["m"]);
        let r = f64s(&case["r"]);
        let mut rect = Rectangle::new(r[0], r[1], r[2], r[3]);
        let delta = case["delta"].as_bool().unwrap();
        m.transform_rectangle(&mut rect, delta);
        let want = f64s(&case["expect"]);
        assert_close(
            &[rect.x, rect.y, rect.width, rect.height],
            &want,
            &format!("transformRectangle[{i}]"),
        );
    }
}

#[test]
fn geom_to_matrix_matches_js() {
    let v = vectors();
    for (i, case) in v["toMatrix"].as_array().unwrap().iter().enumerate() {
        let t = transform_from(&case["t"]);
        let mut m = Matrix::default();
        t.to_matrix(&mut m);
        let got = matrix_to_arr(&m);
        let want = f64s(&case["expect"]);
        assert_close(&got, &want, &format!("toMatrix[{i}]"));
    }
}

#[test]
fn geom_from_matrix_matches_js() {
    let v = vectors();
    for (i, case) in v["fromMatrix"].as_array().unwrap().iter().enumerate() {
        let mut t = Transform::default();
        t.from_matrix(&matrix_from(&case["m"]));
        let got = transform_to_arr(&t);
        let want = f64s(&case["expect"]);
        assert_close(&got, &want, &format!("fromMatrix[{i}]"));
    }
}

#[test]
fn geom_add_minus_matches_js() {
    let v = vectors();
    for (i, case) in v["addMinus"].as_array().unwrap().iter().enumerate() {
        let a = transform_from(&case["a"]);
        let b = transform_from(&case["b"]);

        let mut added = a;
        added += b;
        assert_close(
            &transform_to_arr(&added),
            &f64s(&case["expectAdd"]),
            &format!("addMinus[{i}].add"),
        );

        let mut minused = a;
        minused -= b;
        assert_close(
            &transform_to_arr(&minused),
            &f64s(&case["expectMinus"]),
            &format!("addMinus[{i}].minus"),
        );
    }
}

#[test]
fn geom_normalize_radian_matches_js() {
    let v = vectors();
    for (i, case) in v["normalizeRadian"].as_array().unwrap().iter().enumerate() {
        let x = case["x"].as_f64().unwrap();
        let want = case["expect"].as_f64().unwrap();
        let got = normalize_radian(x);
        assert!(
            (got - want).abs() <= 1e-9,
            "normalizeRadian[{i}] x={x}: got {got}, want {want}"
        );
    }
}
