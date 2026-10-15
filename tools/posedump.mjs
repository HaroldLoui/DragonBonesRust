// tools/posedump.mjs — 阶段 3 静姿态黄金数据 dump（jsdump.mjs 变体）
// 用法: node tools/posedump.mjs <skeJson> <texJson> <armatureName> <outJson>
// 与 jsdump.mjs 的关系: 只 buildArmature、不播放动画、不逐帧推进 ——
//   buildArmature（内部已 advanceTime(0.0)，F3.11）→ 挂钟 → 再调一遍全局 advanceTime(0)
//   → dump 静姿态。注: 该第二遍全局调用实测为 no-op（见步骤 4 注释），保留只为镜像官方管线。
//   tools 下脚本互相不可 import（无 export 且顶层即执行 argv 校验 + process.exit），
//   所以四桩 + PIXI 桩整段复制自 jsdump.mjs；bones 13 元 / slots 15 元顺序与 jsdump 逐元素一致。
import { createRequire } from "node:module";           // F1: .mjs 无 require
import fs from "node:fs";
import path from "node:path";

const require = createRequire(import.meta.url);

// ==== 复制自 jsdump.mjs:9-13 ====
// PIXI 打桩：仅满足 bundle 顶层引用（PixiArmatureDisplay extends PIXI.Container），core 不会真正用到
globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
};

// ==== 复制自 jsdump.mjs:15 ====
const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

// ==== 复制自 jsdump.mjs:17-73（NullAtlas / NullSlot / EvProxy / NullFactory 四桩）====
// 对象池按 String(constructor) 分键，自定义子类必须覆盖静态 toString()
class NullAtlas extends db.TextureAtlasData {
  static toString() { return "NullAtlas"; }
  _onClear() { super._onClear(); }
  createTexture() { return new db.TextureData(); }
}

// Slot 14 个抽象方法全部 no-op —— 只要核心逻辑，不要渲染
class NullSlot extends db.Slot {
  static toString() { return "NullSlot"; }
  _initDisplay() {}
  _disposeDisplay() {}
  _onUpdateDisplay() { this._rd = this._display || this._rawDisplay; }
  _addDisplay() {}
  _replaceDisplay() {}
  _removeDisplay() {}
  _updateZOrder() {}
  _updateVisible() {}
  _updateBlendMode() {}
  _updateColor() {}
  _updateFrame() {}
  _updateMesh() {}
  _updateTransform() {}
  _identityTransform() {}
}

// IArmatureProxy + IEventDispatcher
class EvProxy {
  constructor() { this._armature = null; this._l = {}; }
  dbInit(a) { this._armature = a; }
  dbClear() {}
  dbUpdate() {}
  dispose() {}
  get armature() { return this._armature; }
  get animation() { return this._armature.animation; }
  addDBEventListener(t, f) { (this._l[t] = this._l[t] || []).push(f); }
  hasDBEventListener(t) { return (this._l[t] || []).length > 0; }
  removeDBEventListener() {}
  dispatchDBEvent(t, e) { (this._l[t] || []).forEach(f => f(e)); }
}

// BaseFactory 3 个抽象方法
class NullFactory extends db.BaseFactory {
  static toString() { return "NullFactory"; }
  _buildTextureAtlasData(ta) { return ta || db.BaseObject.borrowObject(NullAtlas); }
  _buildArmature(pkg) {
    const arm = db.BaseObject.borrowObject(db.Armature);
    const p = new EvProxy();
    arm.init(pkg.armature, p, p, this._dragonBones);   // F3: _dragonBones 需外部注入
    return arm;
  }
  _buildSlot(pkg, slotData, armature) {
    const s = db.BaseObject.borrowObject(NullSlot);
    s.init(slotData, armature, {}, {});
    return s;
  }
}

// ==== 复制自 jsdump.mjs:81 ====
const fin = v => (typeof v === "number" && Number.isFinite(v) ? v : null);

// ---- CLI ----
const [skePath, texPath, armName, outPath] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !outPath) {
  console.error("usage: node tools/posedump.mjs <skeJson> <texJson> <armatureName> <outJson>");
  process.exit(2);
}

// ==== 以下流程照 jsdump.mjs:91-101，但不挂事件、不播动画 ====
const key = path.basename(skePath).replace(/[.]json$/, ""); // 缓存键（骨架与图集同键）
const em = new EvProxy();
const factory = new NullFactory();
factory._dragonBones = new db.DragonBones(em);              // F3

factory.parseDragonBonesData(JSON.parse(fs.readFileSync(skePath, "utf8")), key);
factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);

const armature = factory.buildArmature(armName, key, "", key); // F4: 第 4 参显式传；内部已 advanceTime(0.0)（F3.11）
if (!armature) { console.error(`buildArmature failed: ${armName}`); process.exit(1); }
armature.clock = factory.clock;                                // F2: 挂进全局时钟
// 任务 8 步骤 4: 实测本调用在 JS 侧是 no-op —— 官方 WorldClock.advanceTime(0.0)
// 在 `if (passedTime === 0.0) return;`（WorldClock.ts:114-116）早退，不推进任何骨架；
// 真正的收敛点是上面 buildArmature 内部那遍 `armature.advanceTime(0)`（BaseFactory.ts:713）。
// 保留调用以镜像官方管线（挂钟 + 第二遍全局 advance 的调用形态不变）。
factory._dragonBones.advanceTime(0);

// ---- dump（必须在 advance 之后取 → slotNames = 排序后槽序，F3.10 锚点）----
const bonesArr = armature.getBones();                          // 活数组引用；bones 序 = sortedBones 拓扑序
const slotsArr = armature.getSlots();                          // 活数组引用；advance 后已是 zIndex*1000+zOrder 稳定排序序
const boneNames = bonesArr.map(b => b.name);
const slotNames = slotsArr.map(s => s.name);
const frameRate = armature.armatureData.frameRate || 24;

const bones = {};
for (const b of bonesArr) {
  const g = b.global, m = b.globalTransformMatrix;
  bones[b.name] = [
    fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
    fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty),
    fin(b._globalAlpha),
  ];
}

const slots = {};
for (const s of slotsArr) {
  const g = s.global, c = s._colorTransform;
  slots[s.name] = [
    s.displayIndex, s.visible ? 1 : 0, s._zOrder, s._zIndex,
    fin(c.redMultiplier), fin(c.greenMultiplier), fin(c.blueMultiplier), fin(c.alphaMultiplier),
    fin(c.redOffset), fin(c.greenOffset), fin(c.blueOffset), fin(c.alphaOffset),
    fin(g.x), fin(g.y),
    fin(s._globalAlpha),
  ];
}

// 几何摘要: 只收 _geometryData 非空的槽（mesh 槽）; vertexCount/triangleCount 走 TS GeometryData getter
// （DisplayData.ts:52-60，读 parent.intArray）—— 没有 mesh 槽的素材输出空对象 {}
const geometry = {};
for (const s of slotsArr) {
  if (s._geometryData !== null) {
    geometry[s.name] = [fin(s._geometryData.vertexCount), fin(s._geometryData.triangleCount)];
  }
}

// ---- 写盘前自检: 非空 + 向量长度 + key 集合，失败 exit 1 ----
// （防写出"合法"的空黄金: 空骨架 dump 各节全 {}，测试侧的逐键差分会静默通过）
const die = msg => { console.error(`ASSERT ${msg}`); process.exit(1); };
const sortedKeys = ks => [...ks].sort().join("\u0000");
if (boneNames.length === 0) die(`armature "${armName}" has no bones (empty skeleton dump rejected)`);
if (slotNames.length === 0) die(`armature "${armName}" has no slots (empty skeleton dump rejected)`);
for (const n of boneNames) {
  if (!Array.isArray(bones[n]) || bones[n].length !== 13) die(`bone "${n}" vector length != 13`);
}
for (const n of slotNames) {
  if (!Array.isArray(slots[n]) || slots[n].length !== 15) die(`slot "${n}" vector length != 15`);
}
if (sortedKeys(Object.keys(bones)) !== sortedKeys(boneNames)) die("bones key set != boneNames");
if (sortedKeys(Object.keys(slots)) !== sortedKeys(slotNames)) die("slots key set != slotNames");
for (const n of Object.keys(geometry)) {
  if (geometry[n].length !== 2) die(`geometry "${n}" vector length != 2`);
  if (!slotNames.includes(n)) die(`geometry key "${n}" not in slotNames`);
}
// geometry 键集合 == 运行时有 geometry 的槽集合（posedump 内可判 `_geometryData !== null` 的全集）
const runtimeGeometrySlots = slotsArr.filter(s => s._geometryData !== null).map(s => s.name);
if (sortedKeys(Object.keys(geometry)) !== sortedKeys(runtimeGeometrySlots)) {
  die("geometry key set != runtime geometry slot set (_geometryData !== null)");
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({
  meta: {
    skeFile: path.basename(skePath),
    armature: armName,
    frameRate,
    boneNames,
    slotNames,
  },
  bones,
  slots,
  geometry,
}));
console.log(`OK ${path.basename(skePath)} / ${armName}: ${bonesArr.length} bones, ${slotsArr.length} slots, ${Object.keys(geometry).length} geometry`);
