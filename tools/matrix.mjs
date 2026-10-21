// tools/matrix.mjs
// 单场景交叉动画参考 dump（阶段 5）。由 gen_matrix.mjs 逐 case 调用。
// 用法:
//   node tools/matrix.mjs <skeJson> <texJson> <armatureName> <scenario> <animA> <animB> <outJson> [frameCount]
//
// 脚本时序：场景动作在该帧 advanceTime **之前**执行；帧 i 的采样在第 i 次 advance 之后。
// 输出 compact schema：states/events 每帧；pose 仅在 meta.checkpoints 帧。
import fs from "node:fs";
import path from "node:path";
import {
  createFactory, buildArmature, installRecorder, snapshotBones, snapshotSlots,
  snapshotStates,
} from "./dbharness.mjs";

// `AnimationFadeOutMode` 是 TS const enum（运行时被擦除），官方值为 All = 4。
const FADE_OUT_ALL = 4;

const [
  skePath, texPath, armName, scenario, animA, animB, outPath, frameCountArg,
] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !scenario || !animA || !outPath) {
  console.error("usage: node tools/matrix.mjs <ske> <tex> <armature> <scenario> <animA> <animB> <out> [frames]");
  process.exit(2);
}

const SCENARIO_FRAMES = {
  fade_cross: 120,
  layer_overlay: 120,
  reset_to_pose: 120,
  replay: 120,
  event_dense: 120,
  goto_roundtrip: 10,
  timescale: 60,
  dual_armature: 120,
  dispose: 120,
};

const CHECKPOINTS = {
  fade_cross: [0, 15, 29, 30, 35, 45, 60, 90, 119],
  layer_overlay: [0, 14, 15, 20, 30, 60, 90, 119],
  reset_to_pose: [0, 29, 30, 35, 60, 119],
  replay: [],
  event_dense: [],
  goto_roundtrip: [0, 5, 9],
  timescale: [0, 20, 40, 59],
  dual_armature: [0, 30, 60, 90, 119],
  dispose: [],
};

if (!(scenario in SCENARIO_FRAMES)) {
  console.error(`unknown scenario: ${scenario}`);
  process.exit(2);
}
const frameCount = frameCountArg ? parseInt(frameCountArg, 10) : SCENARIO_FRAMES[scenario];
const checkpoints = CHECKPOINTS[scenario];
const comparePose = checkpoints.length > 0;

const key = path.basename(skePath).replace(/[.]json$/, "");
const { factory } = createFactory(skePath, texPath, key);

const sink = [];
const armatures = [];
const armature = buildArmature(factory, armName, key);
armatures.push(armature);
installRecorder(armature, sink, 0);

if (scenario === "dual_armature") {
  const second = buildArmature(factory, armName, key);
  armatures.push(second);
  installRecorder(second, sink, 1);
}

const frameRate = armature.armatureData.frameRate || 24;
const dt = 1 / frameRate;
const bones = armature.getBones();
const slots = armature.getSlots();

function play(target, name) {
  if (!target.animation.gotoAndPlayByTime(name, 0, -1)) {
    throw new Error(`animation not found: ${name}`);
  }
}

function fade(target, name, fadeInTime, layer, group, mode) {
  const state = target.animation.fadeIn(name, fadeInTime, -1, layer, group, mode);
  if (!state) throw new Error(`fadeIn failed: ${name}`);
  return state;
}

function applyScript(step) {
  switch (scenario) {
    case "fade_cross":
      if (step === 0) play(armature, animA);
      if (step === 30) {
        fade(armature, animB, 0.2, 0, "", FADE_OUT_ALL);
      }
      break;
    case "layer_overlay":
      if (step === 0) {
        fade(armature, animA, 0, 0, "g1").resetToPose = false;
      }
      if (step === 15) {
        fade(armature, animB, 0, 0, "g2").resetToPose = false;
      }
      break;
    case "reset_to_pose":
      if (step === 0) play(armature, animA);
      if (step === 30) {
        fade(armature, animB, 0.2, 0, "", FADE_OUT_ALL);
      }
      break;
    case "replay":
      if (step === 0) play(armature, animA);
      if (step === 30) armature.animation.play(animA);
      break;
    case "event_dense":
      if (step === 0) play(armature, animA);
      break;
    case "goto_roundtrip":
      if (step === 0) armature.animation.gotoAndStopByFrame(animA, 6);
      if (step === 6) armature.animation.gotoAndPlayByFrame(animA, 6, -1);
      break;
    case "timescale":
      if (step === 0) {
        fade(armature, animA, 0, 0, "").timeScale = 2;
      }
      break;
    case "dual_armature":
      if (step === 0) {
        play(armatures[0], animA);
        play(armatures[1], animB || animA);
      }
      break;
    case "dispose":
      if (step === 0) play(armature, animA);
      if (step === 30) armature.dispose();
      break;
    default:
      throw new Error(`unhandled scenario ${scenario}`);
  }
}

const frames = [];
for (let i = 0; i < frameCount; i++) {
  applyScript(i);
  sink.length = 0;
  factory._dragonBones.advanceTime(dt);

  const frame = {
    i,
    states: snapshotStates(armatures),
    events: sink.slice(),
  };
  if (comparePose && checkpoints.includes(i)) {
    frame.pose = armatures.map((target, armatureIndex) => ({
      armature: armatureIndex,
      bones: snapshotBones(target.getBones()),
      slots: snapshotSlots(target.getSlots()),
    }));
  }
  frames.push(frame);
}

const asset = path
  .basename(skePath)
  .replace(/[.]json$/, "")
  .replace(/_ske_compressed$/, "_compressed")
  .replace(/_ske$/, "");

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({
  meta: {
    scenario,
    asset,
    armature: armName,
    skeFile: path.basename(skePath),
    animA,
    animB: animB || animA,
    frameRate,
    dt,
    frameCount,
    checkpoints,
    comparePose,
    boneNames: bones.map(b => b.name),
    slotNames: slots.map(s => s.name),
  },
  frames,
}));
console.log(`MATRIX ${scenario} / ${path.basename(outPath)}: ${frameCount} frames, ${armatures.length} armature(s)`);
