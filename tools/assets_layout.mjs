// tools/assets_layout.mjs — `assets/` 按 rig 名分目录：`assets/<rig>/<stem>_ske.json`。
//
// 阶段 2/5/6 的黄金数据生成脚本共用这里，避免每个脚本各写一份递归扫描。
import fs from "node:fs";
import path from "node:path";

/// 列出 `assets/` 下所有骨架，按骨架名排序。
/// 返回 `{ rigName, rigDir, file, stem }`，`stem` 形如 `Dragon_ske`。
export function listSkeletons(assetsDir) {
  const out = [];
  for (const entry of fs.readdirSync(assetsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const rigDir = path.join(assetsDir, entry.name);
    for (const name of fs.readdirSync(rigDir)) {
      if (!name.endsWith("_ske.json")) continue;
      out.push({
        rigName: entry.name,
        rigDir,
        file: name,
        stem: name.replace(/\.json$/, ""),
      });
    }
  }
  out.sort((a, b) => a.stem.localeCompare(b.stem));
  return out;
}

/// 某套素材的三件套路径；`stem` 是 `Dragon_ske` 形式。
export function rigPaths(assetsDir, stem) {
  const rigName = stem.replace(/_ske$/, "");
  const texStem = stem.replace(/_ske$/, "_tex");
  const dir = path.join(assetsDir, rigName);
  return {
    dir,
    ske: path.join(dir, `${stem}.json`),
    tex: path.join(dir, `${texStem}.json`),
    png: path.join(dir, `${texStem}.png`),
  };
}
