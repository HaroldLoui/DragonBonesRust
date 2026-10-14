use serde_json::{Number, Value};

/// 半数向 +∞，不是 Rust round()
pub fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// JS `+value || 0`：取数字，失败给 0
pub fn js_to_number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => s.parse::<f64>().unwrap_or(f64::NAN),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => f64::NAN,
    }
}

pub fn or_zero(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v }
}

pub fn get_number(raw: &Value, key: &str, default_value: f64) -> f64 {
    match raw.get(key) {
        Some(Value::Null) => default_value,
        Some(Value::String(s)) if matches!(s.as_str(), "NaN") => default_value,
        Some(v) => or_zero(js_to_number(v)),
        _ => default_value,
    }
}

pub fn get_boolean(raw: &Value, key: &str, default_value: bool) -> bool {
    match raw.get(key) {
        None => default_value,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !matches!(
            s.as_str(),
            "0" | "NaN" | "" | "false" | "null" | "undefined"
        ),
        Some(Value::Null) => false,
        Some(Value::Number(n)) => n.as_f64().is_none_or(|x| x != 0.0 && !x.is_nan()),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

pub fn get_string(raw: &Value, key: &str, default_value: &str) -> String {
    match raw.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Number(n)) => number_to_js_string(n),
        Some(Value::Bool(b)) => b.to_string(),
        Some(v) => v.to_string(),
        None => default_value.to_string(),
    }
}

/// 等价于 JS 的 `key in rawData`。
/// 注意：`serde_json::Value::get` 只在 Object 上返回值，
/// 且“键存在但值为 Null”时也返回 `Some(&Value::Null)`，因此用 `is_some()` 判断即可。
pub fn has_key(raw: &Value, key: &str) -> bool {
    raw.get(key).is_some()
    // 等价写法：
    // raw.as_object().map_or(false, |m| m.contains_key(key))
}

/// 等价于 JS 的 `String(number)`。整数不带 ".0"。
///
/// 但在float下未和js语义保持一致
pub fn number_to_js_string(n: &Number) -> String {
    n.to_string()
}

/// 等价于 JS 的 ToInt16（向 Int16Array 存值）：
///   NaN/±∞/±0  → 0
///   截断（向零）→ 对 2^16 取数学模 → 若 >= 2^15 则减 2^16（符号扩展）
/// 最后 widen 到 i32。
pub fn js_store_i16(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let t = v.trunc(); // 向零截断
    let m = t.rem_euclid(65536.0) as i32; // 数学模，范围 [0, 65536)
    if m >= 32768 { m - 65536 } else { m } // 符号扩展，落到 [-32768, 32767]
}

/// 等价于 JS 的 ToUint16（向 Uint16Array 存值）：
///   NaN/±∞/±0  → 0
///   截断（向零）→ 对 2^16 取数学模
/// 最后 widen 到 i32（值域 0..=65535）。
pub fn js_store_u16(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    let t = v.trunc();
    t.rem_euclid(65536.0) as i32
}

/// 等价于向 Float32Array 存值再读出：f64 → f32 → f64。
/// Rust 的 `as` 转换默认就是 IEEE 754 的 round-to-nearest-even，
/// 与 JS Float32Array 的 SetValueInBuffer 一致。
pub fn js_store_f32(v: f64) -> f64 {
    v as f32 as f64
}
