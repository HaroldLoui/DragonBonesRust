// tools/bbox_dump.mjs — 合成 BoundingBox 夹具的命中查询 golden（P1-3）。
import fs from "node:fs";
import { createFactory, buildArmature, fin } from "./dbharness.mjs";

const ske = "testdata/synthetic/bbox/bbox_ske.json";
const tex = "testdata/synthetic/synthetic_tex.json";
const out = "testdata/golden_bbox/bbox.json";

const { factory } = createFactory(ske, tex, "bbox");
const armature = buildArmature(factory, "bbox_fixture", "bbox");
factory._dragonBones.advanceTime(0.0);

const points = [];
for (let y = -80; y <= 80; y += 20) {
  for (let x = -80; x <= 80; x += 20) {
    points.push([x, y]);
  }
}

const segments = [
  [-100, 0, 100, 0],
  [-100, -100, 100, 100],
  [0, 0, 200, 0],
  [-200, 0, 0, 0],
  [10, 10, 20, 20],
  [0, -200, 0, 200],
  [-60, -60, -40, -40],
  [40, 40, 60, 60],
];

const slots = {};
for (const slot of armature.getSlots()) {
  const pointResults = points.map(([x, y]) => ({
    point: [x, y],
    hit: slot.containsPoint(x, y),
  }));
  const segmentResults = segments.map(([xa, ya, xb, yb]) => {
    const a = { x: 0, y: 0 };
    const b = { x: 0, y: 0 };
    const n = { x: 0, y: 0 };
    const count = slot.intersectsSegment(xa, ya, xb, yb, a, b, n);
    return {
      segment: [xa, ya, xb, yb],
      count,
      a: [fin(a.x), fin(a.y)],
      b: [fin(b.x), fin(b.y)],
      normal: [fin(n.x), fin(n.y)],
    };
  });
  slots[slot.name] = { pointResults, segmentResults };
}

const armaturePoints = points.map(([x, y]) => {
  const hit = armature.containsPoint(x, y);
  return { point: [x, y], hit: hit ? hit.name : null };
});

fs.mkdirSync("testdata/golden_bbox", { recursive: true });
fs.writeFileSync(
  out,
  JSON.stringify({ armature: "bbox_fixture", points: armaturePoints, slots }, null, 2),
);
console.log(`${out}: ${points.length} points, ${segments.length} segments, ${Object.keys(slots).length} slots`);
