#![allow(dead_code)] // 测试辅助模块：并非每个测试二进制都用到全部函数

pub mod ir_snapshot;

use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden")
}

pub(crate) fn all_golden_files() -> Vec<PathBuf> {
    let root = golden_root();
    let mut files = golden_files_by_root(&root);
    files.sort();
    files
}

pub(crate) fn golden_path(dir: &str, anim: &str) -> PathBuf {
    golden_root().join(dir).join(format!("{anim}.json"))
}

pub(crate) fn read_value(path: &Path) -> Value {
    let s = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("parse {} failed: {e}", path.display()))
}

/// 目录下所有 *.json 的文件名（不含扩展名），已排序
pub(crate) fn golden_dir_stems(dir: &str) -> Vec<String> {
    let path = golden_root().join(dir);
    let mut stems: Vec<String> = std::fs::read_dir(&path)
        .unwrap_or_else(|e| panic!("read_dir {} failed: {e}", path.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    stems.sort();
    stems
}

fn golden_files_by_root(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)
        .unwrap_or_else(|e| panic!("read_dir {} failed: {e}", root.display()))
    {
        let path = entry.expect("read_dir entry failed").path();
        if path.is_dir() {
            files.extend(golden_files_by_root(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            files.push(path);
        }
    }
    files
}
