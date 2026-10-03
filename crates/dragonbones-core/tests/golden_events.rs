mod common;

use common::{all_golden_files, golden_path, read_value};
use serde_json::Value;

/// frameEvent 的实测精确序列（帧号, 事件名），仅出现在 PlantAnimate 两个变体的 Death/Shoot
const DEATH_FRAME_EVENTS: &[(u64, &str)] = &[(29, "die"), (57, "die"), (87, "die"), (116, "die")];

const SHOOT_FRAME_EVENTS: &[(u64, &str)] = &[
    (15, "Shoot"),
    (15, "ShootSound"),
    (15, "Shoot"),
    (16, "Shoot"),
    (17, "Shoot"),
    (18, "Shoot5"),
    (40, "Shoot"),
    (40, "ShootSound"),
    (41, "Shoot"),
    (42, "Shoot"),
    (43, "Shoot"),
    (44, "Shoot5"),
    (66, "Shoot"),
    (66, "ShootSound"),
    (67, "Shoot"),
    (68, "Shoot"),
    (69, "Shoot"),
    (70, "Shoot5"),
    (93, "Shoot"),
    (93, "ShootSound"),
    (94, "Shoot"),
    (95, "Shoot"),
    (96, "Shoot"),
    (97, "Shoot5"),
    (119, "Shoot"),
    (119, "ShootSound"),
];

/// 所有 37 个文件的 frame 0 事件序列恒为 fadeIn → fadeInComplete → start
#[test]
fn frame0_event_sequence_is_uniform() {
    for path in &all_golden_files() {
        let v = read_value(path);
        let types: Vec<&str> = v["frames"][0]["events"]
            .as_array()
            .expect("frame0 events array")
            .iter()
            .map(|e| e["type"].as_str().expect("type string"))
            .collect();
        assert_eq!(
            types,
            ["fadeIn", "fadeInComplete", "start"],
            "{}: frame0 事件序列",
            path.display()
        );
    }
}

/// fadeIn/fadeOut/complete 等生命周期事件只允许出现在 frame 0（后续帧只能是 loopComplete/frameEvent）
#[test]
fn lifecycle_events_only_on_frame_zero() {
    const LIFECYCLE: [&str; 6] = [
        "start",
        "complete",
        "fadeIn",
        "fadeInComplete",
        "fadeOut",
        "fadeOutComplete",
    ];

    for path in &all_golden_files() {
        let v = read_value(path);
        for frame in v["frames"].as_array().unwrap().iter().skip(1) {
            let i = frame["i"].as_u64().unwrap();
            for e in frame["events"].as_array().unwrap() {
                let ty = e["type"].as_str().unwrap();
                assert!(
                    ty == "loopComplete" || ty == "frameEvent",
                    "{} frame {i}: 后续帧只允许 loopComplete/frameEvent，出现 {ty}",
                    path.display()
                );
                assert!(
                    !LIFECYCLE.contains(&ty),
                    "{} frame {i}: 生命周期事件不应出现在 frame 0 之后",
                    path.display()
                );
            }
        }
    }
}

/// frameEvent 精确时序：仅 PlantAnimate / PlantAnimate_compressed 的 Death 与 Shoot，序列逐条相等
#[test]
fn frame_event_sequences_are_exact() {
    /// (目录, 动画, 期望的 (帧号, 事件名) 序列)
    type FrameEventCase = (&'static str, &'static str, &'static [(u64, &'static str)]);
    let expected: &[FrameEventCase] = &[
        ("PlantAnimate", "Death", DEATH_FRAME_EVENTS),
        ("PlantAnimate", "Shoot", SHOOT_FRAME_EVENTS),
        ("PlantAnimate_compressed", "Death", DEATH_FRAME_EVENTS),
        ("PlantAnimate_compressed", "Shoot", SHOOT_FRAME_EVENTS),
    ];

    // 先收集所有实际 frameEvent，确认"只"出现在这 4 个文件
    let mut files_with_frame_events: Vec<String> = Vec::new();
    for path in &all_golden_files() {
        let v = read_value(path);
        if collect_frame_events(&v).is_empty() {
            continue;
        }
        let rel = path
            .strip_prefix(common::golden_root())
            .expect("path under golden root")
            .to_string_lossy()
            .replace('\\', "/");
        files_with_frame_events.push(rel.trim_end_matches(".json").to_string());
    }
    files_with_frame_events.sort();
    let mut expected_files: Vec<String> = expected
        .iter()
        .map(|(d, a, _)| format!("{d}/{a}"))
        .collect();
    expected_files.sort();
    assert_eq!(
        files_with_frame_events, expected_files,
        "出现 frameEvent 的文件集合不符"
    );

    // 逐文件精确比对 (帧号, 名称) 序列
    for (dir, anim, want) in expected {
        let v = read_value(&golden_path(dir, anim));
        let got = collect_frame_events(&v);
        let want_vec: Vec<(u64, String)> = want
            .iter()
            .map(|(f, n)| (*f, n.to_string()))
            .collect();
        assert_eq!(got, want_vec, "{dir}/{anim}: frameEvent 序列不符");
    }
}

/// 每个动画 120 帧内至少触发一次 loopComplete（素材均为循环动画）
#[test]
fn every_animation_reports_loop_complete() {
    for path in &all_golden_files() {
        let v = read_value(path);
        let count = v["frames"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|f| f["events"].as_array().unwrap())
            .filter(|e| e["type"].as_str() == Some("loopComplete"))
            .count();
        assert!(
            count >= 1,
            "{}: 120 帧内应至少 1 次 loopComplete，实际 {count}",
            path.display()
        );
    }
}

/// Dragon/walk 首帧基准值（结构与数值双重锚点，防止转储协议字段顺序回归）
#[test]
fn dragon_walk_frame0_baseline() {
    let v = read_value(&golden_path("Dragon", "walk"));

    let root = &v["frames"][0]["bones"]["root"];
    assert_number_array(root, "Dragon/walk frame0 root bone", 13);
    // [x, y, rotation, scaleX, scaleY, skew, a,b,c,d, tx,ty, alpha]
    assert_close(root, &[0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);

    let tail = &v["frames"][0]["slots"]["tailTip"];
    assert_number_array(tail, "Dragon/walk frame0 tailTip slot", 15);
    // [displayIndex, visible, zOrder, zIndex, rM,gM,bM,aM, rO,gO,bO,aO, gx, gy, alpha]
    assert_close(
        tail,
        &[
            0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 81.65, -0.82, 1.0,
        ],
    );
}

fn collect_frame_events(v: &Value) -> Vec<(u64, String)> {
    v["frames"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| {
            let i = f["i"].as_u64().unwrap();
            f["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["type"].as_str() == Some("frameEvent"))
                .map(move |e| (i, e["name"].as_str().unwrap().to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_number_array(v: &Value, ctx: &str, len: usize) {
    let arr = v.as_array().unwrap_or_else(|| panic!("{ctx}: 应为 array"));
    assert_eq!(arr.len(), len, "{ctx}: 长度应为 {len}");
    for (j, x) in arr.iter().enumerate() {
        assert!(x.is_number(), "{ctx}[{j}] = {x} 应为 number");
    }
}

fn assert_close(v: &Value, expected: &[f64]) {
    let arr = v.as_array().unwrap();
    for (j, (got, want)) in arr.iter().zip(expected).enumerate() {
        let got = got.as_f64().unwrap();
        assert!(
            (got - want).abs() < 1e-9,
            "index {j}: got {got}, want {want}"
        );
    }
}
