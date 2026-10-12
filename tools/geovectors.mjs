// tools/geovectors.mjs — 用 JS 参考运行时生成 geom 差分向量 → testdata/geom/vectors.json
//
// 用法: node tools/geovectors.mjs
// 说明: 随机种子固定为 20261003，输出确定。vectors.json 入库后不要随意重生成（diff 噪音大）。
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");

globalThis.PIXI = {
  Container: class {},
  Texture: class {},
  Rectangle: class {},
  Sprite: class {},
  Mesh: class {},
  BLEND_MODES: {},
};
const db = require(path.join(ROOT, "DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js"));

let seed = 20261003; // 可复现 LCG
const rnd = () => {
  seed = (seed * 1103515245 + 12345) & 0x7fffffff;
  return (seed / 0x7fffffff) * 4 - 2; // [-2,2)
};
const rndT = () => {
  seed = (seed * 1103515245 + 12345) & 0x7fffffff;
  return (seed / 0x7fffffff) * 200 - 100; // [-100,100)
};

const M = (a, b, c, d, tx, ty) => [a, b, c, d, tx, ty];
const T = (x, y, skew, rot, sx, sy) => [x, y, skew, rot, sx, sy];
const mkM = (v) => new db.Matrix(...v);
const outM = (m) => [m.a, m.b, m.c, m.d, m.tx, m.ty];
const mkT = (v) => new db.Transform(...v);
const outT = (t) => [t.x, t.y, t.skew, t.rotation, t.scaleX, t.scaleY];

const out = {
  concat: [],
  invert: [],
  transformPoint: [],
  transformRectangle: [],
  toMatrix: [],
  fromMatrix: [],
  addMinus: [],
  normalizeRadian: [],
};

// 边界矩阵（identity/纯平移/90°/纯缩放/含 b,c/全零/仅 b/负缩放/极小缩放）+ 60 个随机矩阵
const mats = [
  M(1, 0, 0, 1, 0, 0),
  M(1, 0, 0, 1, 10, -20),
  M(0, 1, -1, 0, 5, 5),
  M(2, 0, 0, 3, 0, 0),
  M(1, 0.5, -0.5, 1, 7, 9),
  M(0, 0, 0, 0, 1, 1),
  M(0, 1, 0, 0, 0, 0),
  M(-1, 0, 0, -1, 3, 4),
  M(0.001, 0, 0, 0.001, 0, 0),
];
for (let i = 0; i < 60; i++) mats.push(M(rnd(), rnd(), rnd(), rnd(), rndT(), rndT()));

for (let i = 0; i < mats.length; i++) {
  // concat: 有序对
  const b = mats[(i * 7 + 3) % mats.length];
  const a = mkM(mats[i]);
  a.concat(mkM(b));
  out.concat.push({ a: mats[i], b, expect: outM(a) });
}
for (const v of [...mats, M(1e-15, 2, 3, 4, 5, 6), M(0, 0, 0, 1, 0, 0)]) {
  // invert: 含奇异/半奇异
  const m = mkM(v);
  m.invert();
  out.invert.push({ a: v, expect: outM(m) });
}
for (let i = 0; i < 30; i++) {
  // transformPoint: delta 双分支
  const v = mats[i % mats.length];
  for (const delta of [false, true]) {
    const x = rndT() / 10;
    const y = rndT() / 10;
    const r = { x: 0, y: 0 };
    mkM(v).transformPoint(x, y, r, delta);
    out.transformPoint.push({ m: v, x, y, delta, expect: [r.x, r.y] });
  }
}
const rects = [
  [0, 0, 100, 50],
  [-10, -10, 20, 5],
  [3.7, -2.3, 15.5, 8.1],
  [0, 0, 1, 1],
];
for (let i = 0; i < 30; i++) {
  for (const r0 of rects) {
    for (const delta of [false, true]) {
      const rect = { x: r0[0], y: r0[1], width: r0[2], height: r0[3] };
      mkM(mats[i % mats.length]).transformRectangle(rect, delta);
      out.transformRectangle.push({
        m: mats[i % mats.length],
        r: r0,
        delta,
        expect: [rect.x, rect.y, rect.width, rect.height],
      });
    }
  }
}
// toMatrix: 覆盖 rotation==0 / skew==0 / 负缩放 / 极小 skew 分支
const tans = [
  T(0, 0, 0, 0, 1, 1),
  T(1, 2, 0, 0, 2, 3),
  T(1, 2, 0.3, 0, 1, 1),
  T(1, 2, 0, 0.5, 1, 1),
  T(1, 2, 0.3, 0.5, 2, 3),
  T(1, 2, -1.2, -0.7, -2, -3),
  T(0, 0, Math.PI, 0, 1, 1),
  T(5, -5, 0.001, -0.001, 0.5, 1.5),
];
for (let i = 0; i < 60; i++) tans.push(T(rndT() / 10, rndT() / 10, rnd() * Math.PI, rnd() * Math.PI, rnd(), rnd()));
for (const v of tans) {
  const m = new db.Matrix();
  mkT(v).toMatrix(m);
  out.toMatrix.push({ t: v, expect: outM(m) });
}
// fromMatrix: 覆盖 backupScale 符号修正分支（输入含随机矩阵与 toMatrix 产物）
for (const v of [...mats.map(outM), ...out.toMatrix.map((x) => x.expect)]) {
  const t = new db.Transform();
  t.fromMatrix(mkM(v));
  out.fromMatrix.push({ m: v, expect: outT(t) });
}
for (let i = 0; i < 40; i++) {
  // add / minus
  const a = T(rndT() / 10, rndT() / 10, rnd(), rnd(), rnd(), rnd());
  const b = T(rndT() / 10, rndT() / 10, rnd(), rnd(), rnd(), rnd());
  const ta = mkT(a);
  ta.add(mkT(b));
  const tb = mkT(a);
  tb.minus(mkT(b));
  out.addMinus.push({ a, b, expectAdd: outT(ta), expectMinus: outT(tb) });
}
// normalizeRadian: 正负大数 / ±π / 越界 / 0
for (const x of [
  -100, -20, -2 * Math.PI, -Math.PI - 0.1, -Math.PI, -3, -1, -0.5, -0.0001, 0,
  0.0001, 0.5, 1, 3, Math.PI - 0.1, Math.PI, 2 * Math.PI, 20, 100,
]) {
  out.normalizeRadian.push({ x, expect: db.Transform.normalizeRadian(x) });
}

const p = path.join(ROOT, "testdata", "geom", "vectors.json");
fs.mkdirSync(path.dirname(p), { recursive: true });
fs.writeFileSync(p, JSON.stringify(out, null, 1));
console.log("cases:", Object.entries(out).map(([k, v]) => k + "=" + v.length).join(" "));
