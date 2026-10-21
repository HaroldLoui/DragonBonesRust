//! 阶段 6：P1-10 JSON 路径 `data.binary` 与 JS `_modifyArray` 字节对拍。

mod common;

use common::ir_snapshot::{asset_root, read_json};
use dragonbones_core::factory::BaseFactory;
use std::path::Path;

#[test]
fn json_data_binary_matches_js_modify_array_bytes() {
    let assets = asset_root();
    let golden_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ir_binary");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&assets).expect("assets dir") {
        let path = entry.expect("entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with("_ske.json") {
            continue;
        }
        let stem = name.trim_end_matches(".json");
        let raw = read_json(&path);
        let mut factory = BaseFactory::new();
        let data = factory
            .parse_dragon_bones_data(&raw, stem)
            .unwrap_or_else(|| panic!("parse {stem}"));
        let got = data.binary.as_ref().unwrap_or_else(|| panic!("{stem}: binary"));
        let want = std::fs::read(golden_root.join(format!("{stem}.bin")))
            .unwrap_or_else(|error| panic!("read {stem}.bin: {error}"));
        assert_eq!(
            got.len(),
            want.len(),
            "{stem}: binary length (rust={} js={})",
            got.len(),
            want.len()
        );
        for (index, (got, want)) in got.iter().zip(&want).enumerate() {
            assert_eq!(
                got, want,
                "{stem}: binary byte {index} differs (rust={got:#04x} js={want:#04x})"
            );
        }
        checked += 1;
    }
    assert!(checked >= 8, "expected at least 8 skeletons, checked {checked}");
}
