// tools/gen_ir.mjs — 遍历 assets/ 全部 ske，生成阶段 2 IR 黄金数据 → testdata/ir/<asset>.json
// 用法: node tools/gen_ir.mjs
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { listSkeletons } from "./assets_layout.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSETS = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "ir");

const skes = listSkeletons(ASSETS);

let ok = 0, fail = 0;
for (const { rigDir, file: ske } of skes) {
  const tex = ske.replace(/_ske[.]json$/, "_tex.json");
  const data = JSON.parse(fs.readFileSync(path.join(rigDir, ske), "utf8"));
  const armName = data.armature[0].name;
  const stem = ske.replace(/[.]json$/, "").replace(/_ske$/, "");
  const out = path.join(OUT, `${stem}.json`);
  const r = spawnSync(process.execPath, [
    path.join(HERE, "irdump.mjs"),
    path.join(rigDir, ske), path.join(rigDir, tex), armName, out,
  ], { encoding: "utf8" });
  if (r.status === 0) { ok++; process.stdout.write(r.stdout.trim() + "\n"); }
  else { fail++; console.error(`FAIL ${ske}\n${r.stderr}`); }
}
console.log(`ir golden done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
