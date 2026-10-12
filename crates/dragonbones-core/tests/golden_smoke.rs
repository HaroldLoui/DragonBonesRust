mod common;

use common::{all_golden_files, read_value};
use serde_json::Value;
use std::collections::HashSet;

/// 转储协议登记的全部事件类型（tools/jsdump.mjs EVENT_TYPES）
const EVENT_TYPES: [&str; 9] = [
    "start",
    "loopComplete",
    "complete",
    "fadeIn",
    "fadeInComplete",
    "fadeOut",
    "fadeOutComplete",
    "frameEvent",
    "soundEvent",
];

const BONE_VEC_LEN: usize = 13;
const SLOT_VEC_LEN: usize = 15;

/// 结构完整性：37 个文件 × 120 帧 × 键集合 / 向量长度 / 数值类型 / 事件形状
#[test]
fn golden_data_present_and_well_formed() {
    let files = all_golden_files();
    assert_eq!(files.len(), 37, "9 素材应产出 37 个 golden 文件");

    for path in &files {
        let ctx = path.display().to_string();
        let v = read_value(path);

        let obj = v
            .as_object()
            .unwrap_or_else(|| panic!("{ctx}: 顶层应为 object"));
        let top_keys: HashSet<&str> = obj.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            top_keys,
            HashSet::from(["meta", "frames"]),
            "{ctx}: 顶层键应恰为 meta/frames"
        );

        // ---- meta ----
        let meta = &v["meta"];
        assert_eq!(meta["frameCount"].as_u64(), Some(120), "{ctx}: frameCount");
        let frame_rate = meta["frameRate"].as_f64().expect("frameRate number");
        assert!(
            frame_rate == 24.0 || frame_rate == 30.0,
            "{ctx}: frameRate 应为 24 或 30，实际 {frame_rate}"
        );
        let dt = meta["dt"].as_f64().expect("dt number");
        assert!(
            (dt * frame_rate - 1.0).abs() < 1e-12,
            "{ctx}: dt 应为 1/frameRate"
        );
        assert!(
            !meta["skeFile"].as_str().expect("skeFile string").is_empty(),
            "{ctx}"
        );
        assert!(
            !meta["armature"]
                .as_str()
                .expect("armature string")
                .is_empty(),
            "{ctx}"
        );
        assert!(
            !meta["animation"]
                .as_str()
                .expect("animation string")
                .is_empty(),
            "{ctx}"
        );

        let bone_names: Vec<&str> = str_array(&meta["boneNames"], &ctx, "boneNames");
        let slot_names: Vec<&str> = str_array(&meta["slotNames"], &ctx, "slotNames");
        assert!(!bone_names.is_empty(), "{ctx}: boneNames 非空");
        assert!(!slot_names.is_empty(), "{ctx}: slotNames 非空");
        assert_unique(&bone_names, &ctx, "boneNames");
        assert_unique(&slot_names, &ctx, "slotNames");
        let bone_set: HashSet<&str> = bone_names.iter().copied().collect();
        let slot_set: HashSet<&str> = slot_names.iter().copied().collect();

        // ---- frames ----
        let frames = v["frames"]
            .as_array()
            .unwrap_or_else(|| panic!("{ctx}: frames 应为 array"));
        assert_eq!(frames.len(), 120, "{ctx}: 恰 120 帧");

        for (i, frame) in frames.iter().enumerate() {
            let fctx = format!("{ctx} frame {i}");
            assert_eq!(frame["i"].as_u64(), Some(i as u64), "{fctx}: 帧号连续");

            // bones：键集合 == meta.boneNames，每项 13 个 number
            let bones = frame["bones"]
                .as_object()
                .unwrap_or_else(|| panic!("{fctx}: bones object"));
            assert_eq!(bones.len(), bone_names.len(), "{fctx}: bones 键数");
            for name in &bone_names {
                let vec = bones
                    .get(*name)
                    .unwrap_or_else(|| panic!("{fctx}: 缺少 bone `{name}`"));
                assert_number_vec(vec, BONE_VEC_LEN, &format!("{fctx} bone `{name}`"));
            }

            // slots：键集合 == meta.slotNames，每项 15 个 number
            let slots = frame["slots"]
                .as_object()
                .unwrap_or_else(|| panic!("{fctx}: slots object"));
            assert_eq!(slots.len(), slot_names.len(), "{fctx}: slots 键数");
            for name in &slot_names {
                let vec = slots
                    .get(*name)
                    .unwrap_or_else(|| panic!("{fctx}: 缺少 slot `{name}`"));
                assert_number_vec(vec, SLOT_VEC_LEN, &format!("{fctx} slot `{name}`"));
            }

            // events：数组，元素形状合法
            let events = frame["events"]
                .as_array()
                .unwrap_or_else(|| panic!("{fctx}: events array"));
            for (k, e) in events.iter().enumerate() {
                let ectx = format!("{fctx} event {k}");
                let ty = e["type"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{ectx}: type 应为 string"));
                assert!(
                    EVENT_TYPES.contains(&ty),
                    "{ectx}: 未知事件类型 `{ty}`（转储协议只允许 9 种）"
                );
                assert!(e["name"].is_string(), "{ectx}: name 应为 string");
                assert!(
                    e["bone"].is_null() || e["bone"].is_string(),
                    "{ectx}: bone 应为 null|string"
                );
                assert!(
                    e["slot"].is_null() || e["slot"].is_string(),
                    "{ectx}: slot 应为 null|string"
                );
                // 引用的 bone/slot 名必须真实存在
                if let Some(b) = e["bone"].as_str() {
                    assert!(bone_set.contains(b), "{ectx}: 引用不存在的 bone `{b}`");
                }
                if let Some(s) = e["slot"].as_str() {
                    assert!(slot_set.contains(s), "{ectx}: 引用不存在的 slot `{s}`");
                }
            }
        }
    }
}

/// 数值健全性：全量 37×120 帧不允许出现 null（NaN 哨兵；实测应为 0）
#[test]
fn golden_no_nan_sentinel() {
    for path in &all_golden_files() {
        let ctx = path.display().to_string();
        let v = read_value(path);
        for (i, frame) in v["frames"].as_array().unwrap().iter().enumerate() {
            for (name, vec) in frame["bones"].as_object().unwrap() {
                for (j, x) in vec.as_array().unwrap().iter().enumerate() {
                    assert!(
                        x.is_number(),
                        "{ctx} frame {i} bone `{name}`[{j}] = {x}，预期 number（null 仅保留给 NaN）"
                    );
                }
            }
            for (name, vec) in frame["slots"].as_object().unwrap() {
                for (j, x) in vec.as_array().unwrap().iter().enumerate() {
                    assert!(
                        x.is_number(),
                        "{ctx} frame {i} slot `{name}`[{j}] = {x}，预期 number"
                    );
                }
            }
        }
    }
}

fn str_array<'a>(v: &'a Value, ctx: &str, field: &str) -> Vec<&'a str> {
    v.as_array()
        .unwrap_or_else(|| panic!("{ctx}: {field} 应为 array"))
        .iter()
        .map(|x| {
            x.as_str()
                .unwrap_or_else(|| panic!("{ctx}: {field} 元素应为 string"))
        })
        .collect()
}

fn assert_unique(names: &[&str], ctx: &str, field: &str) {
    let set: HashSet<&str> = names.iter().copied().collect();
    assert_eq!(set.len(), names.len(), "{ctx}: {field} 应无重复");
}

fn assert_number_vec(v: &Value, len: usize, ctx: &str) {
    let arr = v.as_array().unwrap_or_else(|| panic!("{ctx}: 应为 array"));
    assert_eq!(arr.len(), len, "{ctx}: 长度应为 {len}");
    for (j, x) in arr.iter().enumerate() {
        assert!(x.is_number(), "{ctx}[{j}] = {x} 应为 number");
    }
}
