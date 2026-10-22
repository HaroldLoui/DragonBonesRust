//! 阶段 2 IR 差分测试：Rust 解析产物对齐 `testdata/ir/*.json`（JS 参考运行时）。
//!
//! - 8 组 IR 数组：精确相等（int 部 wrap 后整数、float 部 f32 量化后 f64 bit 相等）。
//! - structure / atlas / meta 快照：`norm()` 后逐值相等。

mod common;

use common::ir_snapshot::{
    ir_root, norm, parse_asset, read_json, snapshot_atlas, snapshot_meta, snapshot_structure,
};
use serde_json::Value;

/// (golden 目录名, assets ske 文件名去扩展)。
const ASSETS: &[(&str, &str)] = &[
    ("Dragon", "Dragon_ske"),
    ("Dragon_Blend", "Dragon_Blend_ske"),
    ("PlantAnimate", "PlantAnimate_ske"),
    ("PlantAnimate_compressed", "PlantAnimate_ske"),
    ("Rooster_Ani", "Rooster_Ani_ske"),
    ("Rooster_Blend", "Rooster_Blend_ske"),
    ("Rooster_P1", "Rooster_P1_ske"),
    ("Surface_P1_4", "Surface_P1_4_ske"),
    ("龙", "龙_ske"),
];

fn to_i32_vec(v: &Value) -> Vec<i32> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_i64().expect("i64") as i32)
        .collect()
}

fn to_f64_vec(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("f64"))
        .collect()
}

fn assert_f64_eq(got: &[f64], want: &[f64], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx}: 长度不同");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            g.to_bits() == w.to_bits(),
            "{ctx}[{i}]: got {g} (f32={}, bits {:#x}), want {w} (f32={}, bits {:#x})",
            *g as f32,
            g.to_bits(),
            *w as f32,
            w.to_bits()
        );
    }
}

#[test]
fn ir_arrays_match_js_elementwise() {
    for (dir, stem) in ASSETS {
        let golden = read_json(&ir_root().join(format!("{dir}.json")));
        let (data, _) = parse_asset(stem);
        let ir = &data.root.as_ref().expect("finalize_root").ir;
        let g = &golden["arrays"];

        assert_eq!(ir.int_array, to_i32_vec(&g["intArray"]), "{dir} intArray");
        assert_f64_eq(
            &ir.float_array,
            &to_f64_vec(&g["floatArray"]),
            &format!("{dir} floatArray"),
        );
        assert_eq!(
            ir.frame_int_array,
            to_i32_vec(&g["frameIntArray"]),
            "{dir} frameIntArray"
        );
        assert_f64_eq(
            &ir.frame_float_array,
            &to_f64_vec(&g["frameFloatArray"]),
            &format!("{dir} frameFloatArray"),
        );
        assert_eq!(
            ir.frame_array,
            to_i32_vec(&g["frameArray"]),
            "{dir} frameArray"
        );
        assert_eq!(
            ir.timeline_array,
            to_i32_vec(&g["timelineArray"]),
            "{dir} timelineArray"
        );
        assert_eq!(
            ir.color_array,
            to_i32_vec(&g["colorArray"]),
            "{dir} colorArray"
        );
        assert_eq!(
            ir.frame_indices,
            to_i32_vec(&g["frameIndices"]),
            "{dir} frameIndices"
        );
    }
}

#[test]
fn structure_snapshot_matches_js() {
    for (dir, stem) in ASSETS {
        let golden = read_json(&ir_root().join(format!("{dir}.json")));
        let (data, atlas) = parse_asset(stem);

        assert_eq!(
            norm(snapshot_meta(&data, &format!("{stem}.json"))),
            norm(golden["meta"].clone()),
            "{dir} meta"
        );
        assert_eq!(
            norm(snapshot_structure(&data)),
            norm(golden["structure"].clone()),
            "{dir} structure"
        );
        assert_eq!(
            norm(snapshot_atlas(&atlas)),
            norm(golden["atlas"].clone()),
            "{dir} atlas"
        );
    }
}
