// tools/path_dump.mjs — Path 约束合成夹具的 pose golden（P1-2）。
import fs from "node:fs";
import { createFactory, buildArmature, fin } from "./dbharness.mjs";

const ske = "testdata/synthetic/path/path_ske.json";
const tex = "testdata/synthetic/synthetic_tex.json";
const out = "testdata/golden_path/path.json";
const { factory } = createFactory(ske, tex, "path");
const armature = buildArmature(factory, "path_fixture", "path");
armature.animation.gotoAndPlayByTime("idle", 0, -1);

const frames = [];
for (let frame = 0; frame <= 120; frame++) {
  const bones = {};
  for (const bone of armature.getBones()) {
    const g = bone.global;
    const m = bone.globalTransformMatrix;
    bones[bone.name] = [
      fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
      fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty), fin(bone.globalAlpha),
    ];
  }
  frames.push({ frame, bones });
  factory._dragonBones.advanceTime(1 / 24);
}
fs.mkdirSync("testdata/golden_path", { recursive: true });
fs.writeFileSync(out, JSON.stringify({ armature: "path_fixture", animation: "idle", frames }, null, 2));
console.log(`${out}: ${frames.length} frames`);
