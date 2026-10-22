// tools/gen_pose.mjs — 镜像 gen_golden.mjs 的 9 素材扫描，逐个 spawn posedump.mjs
//              → testdata/pose/<去 _ske(_compressed) 后缀主名>.json（每个素材 1 个静姿态文件）
// 用法: node tools/gen_pose.mjs
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSETS = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "pose");

// ==== 结构照 gen_golden.mjs:14-24 ====
const skes = fs.readdirSync(ASSETS)
  .filter(f => f.endsWith("_ske.json") || f.endsWith("_ske_compressed.json"));

let ok = 0, fail = 0;
for (const ske of skes) {
  const tex = ske.replace(/_ske(_compressed)?[.]json$/, "_tex.json");   // F7
  const data = JSON.parse(fs.readFileSync(path.join(ASSETS, ske), "utf8"));
  const armName = data.armature[0].name;                               // F7: 从数据读（9 素材单骨架）
  // 文件名: Dragon_ske → Dragon.json, PlantAnimate_ske → PlantAnimate_compressed.json
  const stem = ske.replace(/[.]json$/, "").replace(/_ske$/, "").replace(/_ske_compressed$/, "_compressed");
  const out = path.join(OUT, `${stem}.json`);
  const r = spawnSync(process.execPath, [
    path.join(HERE, "posedump.mjs"),
    path.join(ASSETS, ske), path.join(ASSETS, tex),
    armName, out,
  ], { encoding: "utf8" });
  if (r.status === 0) ok++;
  else { fail++; console.error(`FAIL ${ske}\n${r.stderr}`); }
}
console.log(`pose done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
