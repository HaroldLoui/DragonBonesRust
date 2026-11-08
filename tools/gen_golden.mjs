// tools/gen_golden.mjs — 遍历 assets/ 下全部 ske × 全部动画，写 testdata/golden/<asset>/<anim>.json
// 用法: node tools/gen_golden.mjs [frameCount]   默认 120
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { listSkeletons } from "./assets_layout.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSETS = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "golden");
const FRAME_COUNT = process.argv[2] || "120";

const skes = listSkeletons(ASSETS);

let ok = 0, fail = 0;
for (const { rigDir, file: ske } of skes) {
  const tex = ske.replace(/_ske[.]json$/, "_tex.json");   // F7
  const data = JSON.parse(fs.readFileSync(path.join(rigDir, ske), "utf8"));
  const armName = data.armature[0].name;                               // F7: 从数据读
  const anims = data.armature[0].animation.map(a => a.name);
  // 目录名: Dragon_ske → Dragon（PlantAnimate 的 compressed 变体已在 1142e46
  // 合并进 PlantAnimate_ske.json，不再有独立素材）
  const stem = ske.replace(/[.]json$/, "").replace(/_ske$/, "");
  for (const anim of anims) {
    const out = path.join(OUT, stem, `${anim}.json`);
    const r = spawnSync(process.execPath, [
      path.join(HERE, "jsdump.mjs"),
      path.join(rigDir, ske), path.join(rigDir, tex),
      armName, anim, out, FRAME_COUNT,
    ], { encoding: "utf8" });
    if (r.status === 0) ok++;
    else { fail++; console.error(`FAIL ${ske}/${anim}\n${r.stderr}`); }
  }
}
console.log(`golden done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);