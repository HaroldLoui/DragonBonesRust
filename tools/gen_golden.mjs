// tools/gen_golden.mjs — 遍历 assets/ 下全部 ske × 全部动画，写 testdata/golden/<asset>/<anim>.json
// 用法: node tools/gen_golden.mjs [frameCount]   默认 120
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSETS = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "golden");
const FRAME_COUNT = process.argv[2] || "120";

const skes = fs.readdirSync(ASSETS)
  .filter(f => f.endsWith("_ske.json") || f.endsWith("_ske_compressed.json"));

let ok = 0, fail = 0;
for (const ske of skes) {
  const tex = ske.replace(/_ske(_compressed)?[.]json$/, "_tex.json");   // F7
  const data = JSON.parse(fs.readFileSync(path.join(ASSETS, ske), "utf8"));
  const armName = data.armature[0].name;                               // F7: 从数据读
  const anims = data.armature[0].animation.map(a => a.name);
  // 目录名: Dragon_ske → Dragon, PlantAnimate_ske → PlantAnimate_compressed
  const stem = ske.replace(/[.]json$/, "").replace(/_ske$/, "").replace(/_ske_compressed$/, "_compressed");
  for (const anim of anims) {
    const out = path.join(OUT, stem, `${anim}.json`);
    const r = spawnSync(process.execPath, [
      path.join(HERE, "jsdump.mjs"),
      path.join(ASSETS, ske), path.join(ASSETS, tex),
      armName, anim, out, FRAME_COUNT,
    ], { encoding: "utf8" });
    if (r.status === 0) ok++;
    else { fail++; console.error(`FAIL ${ske}/${anim}\n${r.stderr}`); }
  }
}
console.log(`golden done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);