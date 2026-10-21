// tools/gen_ir_binary.mjs — 生成 JSON 路径的 data.binary（P1-10 字节对拍）。
import fs from "node:fs";
import path from "node:path";
import { createFactory } from "./dbharness.mjs";

const assetsDir = "assets";
const outDir = "testdata/ir_binary";
fs.mkdirSync(outDir, { recursive: true });

const skeletons = fs
  .readdirSync(assetsDir)
  .filter((name) => name.endsWith("_ske.json"))
  .sort();

for (const file of skeletons) {
  const stem = file.replace(/\.json$/, "");
  const skePath = path.join(assetsDir, file);
  const texPath = path.join(assetsDir, stem.replace(/_ske$/, "_tex") + ".json");
  const { factory } = createFactory(skePath, texPath, stem);
  const data = factory._dragonBonesDataMap[stem];
  if (!data || !data.binary) {
    throw new Error(`missing data.binary for ${stem}`);
  }
  const bytes = Buffer.from(new Uint8Array(data.binary));
  fs.writeFileSync(path.join(outDir, `${stem}.bin`), bytes);
  console.log(`${stem}: ${bytes.length} bytes`);
}
