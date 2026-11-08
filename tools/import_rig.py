#!/usr/bin/env python3
"""把一个 DragonBones 导出目录导入为 `assets/` 下的三件套，并打印 demo 注册分支。

用法：
    python tools/import_rig.py --rig-dir <dir> --target <PascalCaseName> [--write]

`<dir>` 里需要（名字可自动识别）：
    skeleton.json / *_ske.json      DragonBones 骨架
    atlas.json / *_tex.json         图集描述
    *.png                           图集贴图

导入后（每套素材一个同名子目录）：
    assets/<Target>/<Target>_ske.json
    assets/<Target>/<Target>_tex.json   （imagePath 会被改写为 <Target>_tex.png）
    assets/<Target>/<Target>_tex.png

默认只打印计划与 demo match 分支；加 `--write` 才真正写文件。
之后记得跑 `node tools/gen_ir_binary.mjs` 生成黄金数据。
"""

import argparse
import json
import os
import shutil
import struct
import sys

IDLE_HINTS = ("idle", "stand", "wait", "static", "loop")


def read_json(path):
    with open(path, "r", encoding="utf-8-sig") as f:
        return json.load(f)


def write_json(path, obj):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, separators=(",", ":"))


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"not a PNG: {path}（ATSC/AVIF/WebP 请先转 PNG）")
    return struct.unpack(">II", head[16:24])


def find_files(rig_dir):
    ske = tex = img = None
    for name in sorted(os.listdir(rig_dir)):
        path = os.path.join(rig_dir, name)
        if not os.path.isfile(path):
            continue
        low = name.lower()
        if low.endswith("_ske.json") or low == "skeleton.json":
            ske = path
        elif low.endswith("_tex.json") or low == "atlas.json":
            tex = path
        elif low.endswith(".png"):
            img = path
    return ske, tex, img


def pick_armature(ske, wanted):
    arms = ske.get("armature") or []
    if not arms:
        raise SystemExit("skeleton 里没有 armature")
    if wanted:
        for arm in arms:
            if arm.get("name") == wanted:
                return arm
        raise SystemExit(f"没有 armature {wanted!r}，可选：{[a.get('name') for a in arms]}")
    return max(arms, key=lambda a: (len(a.get("animation") or []), len(a.get("bone") or [])))


def pick_animation(arm, wanted):
    anims = [a.get("name") for a in (arm.get("animation") or []) if a.get("name")]
    if not anims:
        return None
    if wanted:
        return wanted
    for name in anims:
        low = name.lower()
        if any(low == hint or low.startswith(hint) for hint in IDLE_HINTS):
            return name
    return anims[0]


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--rig-dir", required=True)
    parser.add_argument("--target", required=True, help="PascalCase 素材名，如 Chomper")
    parser.add_argument("--armature", help="多 armature 文件里指定用哪个")
    parser.add_argument("--animation", help="默认动画名")
    parser.add_argument("--scale", type=float, help="覆盖自动计算的缩放")
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args(argv)

    assets = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
    ske_path, tex_path, img_path = find_files(args.rig_dir)
    if not (ske_path and tex_path and img_path):
        raise SystemExit(
            f"{args.rig_dir} 需要 ske+tex+png，实际拿到 {ske_path}, {tex_path}, {img_path}"
        )

    ske = read_json(ske_path)
    tex = read_json(tex_path)
    armature = pick_armature(ske, args.armature)
    animation = pick_animation(armature, args.animation)
    aabb = armature.get("aabb") or {}
    center_x = aabb.get("x", 0.0) + aabb.get("width", 0.0) / 2.0
    center_y = aabb.get("y", 0.0) + aabb.get("height", 0.0) / 2.0
    png_w, png_h = png_size(img_path)
    atlas_w, atlas_h = tex.get("width"), tex.get("height")
    if (atlas_w, atlas_h) != (png_w, png_h):
        print(f"WARN: atlas {atlas_w}x{atlas_h} != png {png_w}x{png_h}", file=sys.stderr)
    max_dim = max(aabb.get("width", 1.0), aabb.get("height", 1.0)) or 1.0
    scale = args.scale if args.scale else round(min(3.0, max(0.3, 320.0 / max_dim)), 2)

    target_dir = os.path.join(assets, args.target)
    target_ske = os.path.join(target_dir, f"{args.target}_ske.json")
    target_tex = os.path.join(target_dir, f"{args.target}_tex.json")
    target_png = os.path.join(target_dir, f"{args.target}_tex.png")

    print(json.dumps(
        {
            "target": args.target,
            "armature": armature.get("name"),
            "animations": [a.get("name") for a in (armature.get("animation") or [])],
            "default_animation": animation,
            "center": [round(center_x, 2), round(center_y, 2)],
            "scale": scale,
            "atlas": f"{atlas_w}x{atlas_h}",
            "bones": len(armature.get("bone") or []),
            "slots": len(armature.get("slot") or []),
            "files": [target_ske, target_tex, target_png],
        },
        ensure_ascii=False,
        indent=2,
    ))

    if args.write:
        os.makedirs(target_dir, exist_ok=True)
        shutil.copyfile(ske_path, target_ske)
        tex["imagePath"] = os.path.basename(target_png)
        write_json(target_tex, tex)
        shutil.copyfile(img_path, target_png)
        print(f"\nwrote {target_ske}")
        print(f"wrote {target_tex}")
        print(f"wrote {target_png}")
        print("next: node tools/gen_ir_binary.mjs && cargo test --workspace")

    key = args.target[:1].lower() + args.target[1:]
    print(f"""
        "{key}" => DemoChoice {{
            name: "{args.target}",
            skeleton: "{args.target}/{args.target}_ske.json",
            atlas: "{args.target}/{args.target}_tex.json",
            armature: "{armature.get('name')}",
            default_animation: "{animation}",
            animations: Vec::new(),
            scale: {scale},
            center: Vec2::new({center_x:.2f}, {center_y:.2f}),
        }},""")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
