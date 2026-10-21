// tools/path_dump.mjs — Path 约束合成夹具的 pose golden（P1-2）。
// 生成 fixed/length/tangent、fixed/fixed/chain、fixed/fixed/chainScale 三组。
import fs from "node:fs";
import { createFactory, buildArmature, fin } from "./dbharness.mjs";

const basePath = "testdata/synthetic/path/path_ske.json";
const tex = "testdata/synthetic/synthetic_tex.json";
const base = JSON.parse(fs.readFileSync(basePath, "utf8"));
const cases = [
  { name: "path", spacingMode: "length", rotateMode: "tangent" },
  { name: "fixed_chain", spacingMode: "fixed", rotateMode: "chain" },
  { name: "fixed_chainscale", spacingMode: "fixed", rotateMode: "chainScale" },
];

fs.mkdirSync("testdata/golden_path", { recursive: true });
for (const testCase of cases) {
  const ske = structuredClone(base);
  Object.assign(ske.armature[0].path[0], {
    positionMode: "fixed",
    spacingMode: testCase.spacingMode,
    rotateMode: testCase.rotateMode,
  });
  const temp = `testdata/synthetic/path/_${testCase.name}.json`;
  fs.writeFileSync(temp, JSON.stringify(ske));
  const { factory } = createFactory(temp, tex, "path");
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
  const out = `testdata/golden_path/${testCase.name}.json`;
  fs.writeFileSync(
    out,
    JSON.stringify({ armature: "path_fixture", animation: "idle", frames }, null, 2),
  );
  fs.unlinkSync(temp);
  console.log(`${out}: ${frames.length} frames`);
}
