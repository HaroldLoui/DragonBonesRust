// tools/gen_matrix.mjs
// 生成阶段 5 交叉动画矩阵黄金数据：testdata/matrix/<scenario>/...
// 用法: node tools/gen_matrix.mjs
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  ASSETS, PAIR_SCENARIOS, ALL_ANIMATION_SCENARIOS, REPRESENTATIVE_SCENARIOS,
} from "./matrix_cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const ASSET_DIR = path.join(ROOT, "assets");
const OUT = path.join(ROOT, "testdata", "matrix");

function texPathFor(skeFile) {
  return path.join(ASSET_DIR, skeFile.replace(/_ske(_compressed)?[.]json$/, "_tex.json"));
}

function readAsset(asset) {
  const ske = path.join(ASSET_DIR, asset.file);
  const data = JSON.parse(fs.readFileSync(ske, "utf8"));
  const armature = data.armature[0].name;
  const animations = data.armature[0].animation.map(a => a.name);
  return { ske, tex: texPathFor(asset.file), armature, animations };
}

function run(caseDef) {
  const args = [
    path.join(HERE, "matrix.mjs"),
    caseDef.ske, caseDef.tex, caseDef.armature,
    caseDef.scenario, caseDef.animA, caseDef.animB, caseDef.out,
  ];
  if (caseDef.frames) args.push(String(caseDef.frames));
  const result = spawnSync(process.execPath, args, { encoding: "utf8" });
  return { ok: result.status === 0, stderr: result.stderr, stdout: result.stdout };
}

fs.rmSync(OUT, { recursive: true, force: true });

const cases = [];
for (const asset of ASSETS) {
  const info = readAsset(asset);
  const base = { ske: info.ske, tex: info.tex, armature: info.armature };

  if (asset.pair) {
    for (const scenario of PAIR_SCENARIOS) {
      cases.push({
        ...base,
        scenario,
        animA: asset.pair[0],
        animB: asset.pair[1],
        out: path.join(OUT, scenario, `${asset.name}.json`),
      });
    }
  }

  for (const scenario of ALL_ANIMATION_SCENARIOS) {
    for (const anim of info.animations) {
      cases.push({
        ...base,
        scenario,
        animA: anim,
        animB: anim,
        out: path.join(OUT, scenario, asset.name, `${anim}.json`),
      });
    }
  }

  const rep = asset.rep && info.animations.includes(asset.rep) ? asset.rep : info.animations[0];
  const second = info.animations.find(name => name !== rep) || rep;
  for (const scenario of REPRESENTATIVE_SCENARIOS) {
    cases.push({
      ...base,
      scenario,
      animA: rep,
      animB: second,
      out: path.join(OUT, scenario, `${asset.name}.json`),
    });
  }
}

let ok = 0;
let fail = 0;
for (const caseDef of cases) {
  const result = run(caseDef);
  if (result.ok) {
    ok++;
  } else {
    fail++;
    console.error(`FAIL ${caseDef.scenario}/${caseDef.animA}\n${result.stderr}`);
  }
}
console.log(`matrix done: ${ok} ok, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
