// tools/cache_dump.mjs — cacheFrameRate=24 的 pose golden（P1-4）。
import fs from "node:fs";
import { createFactory, buildArmature, fin } from "./dbharness.mjs";

const asset = "Dragon";
const ske = `assets/${asset}_ske.json`;
const tex = `assets/${asset}_tex.json`;
const out = "testdata/golden_cache/dragon_walk.json";
const { factory } = createFactory(ske, tex, asset);
const armature = buildArmature(factory, "Dragon", asset);
armature.cacheFrameRate = 24;
armature.animation.gotoAndPlayByTime("walk", 0, -1);

const frames = [];
for (let frame = 0; frame <= 120; frame++) {
  const bones = {};
  for (const bone of armature.getBones()) {
    const g = bone.global;
    const m = bone.globalTransformMatrix;
    bones[bone.name] = [
      fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
      fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty),
    ];
  }
  frames.push({ frame, bones });
  factory._dragonBones.advanceTime(1 / 24);
}
fs.mkdirSync("testdata/golden_cache", { recursive: true });
fs.writeFileSync(out, JSON.stringify({ armature: "Dragon", animation: "walk", cacheFrameRate: 24, frames }, null, 2));
console.log(`${out}: ${frames.length} frames`);
