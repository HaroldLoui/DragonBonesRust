//! `parser/js_compat.rs` 的 JS 语义真值表测试。

use dragonbones_core::parser::js_compat::*;
use serde_json::json;

#[test]
fn js_round_matches_js_half_up() {
    assert_eq!(js_round(0.5), 1.0);
    assert_eq!(js_round(-0.5), 0.0); // Math.round(-0.5) === -0
    assert_eq!(js_round(-1.5), -1.0);
    assert_eq!(js_round(1.5), 2.0);
    assert_eq!(js_round(2.5), 3.0);
    assert_eq!(js_round(-2.5), -2.0);
}

#[test]
fn get_number_truth_table() {
    assert_eq!(get_number(&json!({}), "k", 7.0), 7.0); // 缺键
    assert_eq!(get_number(&json!({"k": null}), "k", 7.0), 7.0); // null
    assert_eq!(get_number(&json!({"k": "NaN"}), "k", 7.0), 7.0);
    assert_eq!(get_number(&json!({"k": ""}), "k", 7.0), 0.0);
    assert_eq!(get_number(&json!({"k": "abc"}), "k", 7.0), 0.0);
    assert_eq!(get_number(&json!({"k": "12.5"}), "k", 7.0), 12.5);
    assert_eq!(get_number(&json!({"k": true}), "k", 7.0), 1.0);
    assert_eq!(get_number(&json!({"k": false}), "k", 7.0), 0.0);
    assert_eq!(get_number(&json!({"k": 0}), "k", 7.0), 0.0);
    assert_eq!(get_number(&json!({"k": []}), "k", 7.0), 0.0);
    assert_eq!(get_number(&json!({"k": {}}), "k", 7.0), 0.0);
}

#[test]
fn get_boolean_truth_table() {
    assert!(get_boolean(&json!({}), "k", true));
    assert!(!get_boolean(&json!({}), "k", false));

    for s in ["0", "NaN", "", "false", "null", "undefined"] {
        assert!(!get_boolean(&json!({"k": s}), "k", true), "{s}");
    }
    assert!(get_boolean(&json!({"k": "abc"}), "k", false));
    assert!(!get_boolean(&json!({"k": null}), "k", true));
    assert!(!get_boolean(&json!({"k": 0}), "k", true));
    assert!(get_boolean(&json!({"k": 1}), "k", false));
    assert!(get_boolean(&json!({"k": true}), "k", false));
}

#[test]
fn get_string_truth_table() {
    assert_eq!(get_string(&json!({}), "k", "def"), "def");
    assert_eq!(get_string(&json!({"k": null}), "k", "def"), "null");
    assert_eq!(get_string(&json!({"k": 24}), "k", "def"), "24");
    assert_eq!(get_string(&json!({"k": "hi"}), "k", "def"), "hi");
    assert_eq!(get_string(&json!({"k": true}), "k", "def"), "true");
}

#[test]
fn has_key_counts_null_value() {
    assert!(has_key(&json!({"k": null}), "k"));
    assert!(!has_key(&json!({}), "k"));
}

#[test]
fn typed_array_store_semantics() {
    // i16 / u16 是 wrap，不是 clamp。
    assert_eq!(js_store_i16(65535.0), -1);
    assert_eq!(js_store_u16(65535.0), 65535);
    assert_eq!(js_store_i16(-1.0), -1);
    assert_eq!(js_store_u16(-1.0), 65535);
    assert_eq!(js_store_i16(40000.0), -25536);
    assert_eq!(js_store_u16(40000.0), 40000);
    assert_eq!(js_store_i16(3.7), 3);
    assert_eq!(js_store_i16(-3.7), -3);
    assert_eq!(js_store_i16(f64::NAN), 0);
}

#[test]
fn f32_store_matches_float32array() {
    assert_eq!(js_store_f32(0.1), 0.1f32 as f64);
    assert_eq!(js_store_f32(1.0), 1.0);
}
