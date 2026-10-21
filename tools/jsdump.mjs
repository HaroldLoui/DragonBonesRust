// tools/jsdump.mjs
// 用法: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]
// 输出必须与阶段 0-4 的 baseline golden 逐字节一致；共享环境见 dbharness.mjs。
import fs from "node:fs";
import path from "node:path";
import {
  createFactory, buildArmature, installRecorder, drainEvents,
  snapshotBones, snapshotSlots,
} from "./dbharness.mjs";

// ---- CLI ----
const [skePath, texPath, armName, animName, outPath, frameCountArg] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !animName || !outPath) {
  console.error("usage: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]");
  process.exit(2);
}
const frameCount = frameCountArg ? parseInt(frameCountArg, 10) : 120;

const key = path.basename(skePath).replace(/[.]json$/, ""); // 缓存键（骨架与图集同键）
const { factory } = createFactory(skePath, texPath, key);
const armature = buildArmature(factory, armName, key);

const events = [];
installRecorder(armature, events);

const frameRate = armature.armatureData.frameRate || 24;
const dt = 1 / frameRate;                                      // F6
const bones = armature.getBones();
const slots = armature.getSlots();

armature.animation.gotoAndPlayByTime(animName, 0, -1);

const frames = [];
for (let i = 0; i < frameCount; i++) {
  events.length = 0;
  factory._dragonBones.advanceTime(dt);   // F2: 必须走全局实例，事件在其中派发

  frames.push({
    i,
    bones: snapshotBones(bones),
    slots: snapshotSlots(slots),
    events: drainEvents(events),
  });
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({
  meta: {
    skeFile: path.basename(skePath), armature: armName, animation: animName,
    frameRate, dt, frameCount,
    boneNames: bones.map(b => b.name),
    slotNames: slots.map(s => s.name),
  },
  frames,
}));
console.log(`OK ${path.basename(skePath)} / ${armName} / ${animName}: ${bones.length} bones, ${slots.length} slots, ${frames.length} frames`);
