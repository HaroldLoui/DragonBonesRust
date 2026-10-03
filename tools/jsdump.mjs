// tools/jsdump.mjs
// 用法: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]
import { createRequire } from "node:module";           // F1: .mjs 无 require
import fs from "node:fs";
import path from "node:path";

const require = createRequire(import.meta.url);

// PIXI 打桩：仅满足 bundle 顶层引用（PixiArmatureDisplay extends PIXI.Container），core 不会真正用到
globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
};

const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

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

const EVENT_TYPES = [
  "start", "loopComplete", "complete",
  "fadeIn", "fadeInComplete", "fadeOut", "fadeOutComplete",
  "frameEvent", "soundEvent",
];

const fin = v => (typeof v === "number" && Number.isFinite(v) ? v : null);

// ---- CLI ----
const [skePath, texPath, armName, animName, outPath, frameCountArg] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !animName || !outPath) {
  console.error("usage: node tools/jsdump.mjs <skeJson> <texJson> <armatureName> <animName> <outJson> [frameCount]");
  process.exit(2);
}
const frameCount = frameCountArg ? parseInt(frameCountArg, 10) : 120;

const key = path.basename(skePath).replace(/[.]json$/, ""); // 缓存键（骨架与图集同键）
const em = new EvProxy();
const factory = new NullFactory();
factory._dragonBones = new db.DragonBones(em);              // F3

factory.parseDragonBonesData(JSON.parse(fs.readFileSync(skePath, "utf8")), key);
factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);

const armature = factory.buildArmature(armName, key, "", key); // F4: 第 4 参显式传
if (!armature) { console.error(`buildArmature failed: ${armName}`); process.exit(1); }
armature.clock = factory.clock;                                // F2: 挂进全局时钟

const events = [];
for (const t of EVENT_TYPES) {
  armature.eventDispatcher.addDBEventListener(t, e => events.push({
    type: e.type, name: e.name ?? "",
    bone: e.bone ? e.bone.name : null,
    slot: e.slot ? e.slot.name : null,
  }));
}

const frameRate = armature.armatureData.frameRate || 24;
const dt = 1 / frameRate;                                      // F6
const bones = armature.getBones();
const slots = armature.getSlots();

armature.animation.gotoAndPlayByTime(animName, 0, -1);

const frames = [];
for (let i = 0; i < frameCount; i++) {
  events.length = 0;
  factory._dragonBones.advanceTime(dt);   // F2: 必须走全局实例，事件在其中派发

  const boneMap = {};
  for (const b of bones) {
    const g = b.global, m = b.globalTransformMatrix;
    boneMap[b.name] = [
      fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
      fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty),
      fin(b._globalAlpha),
    ];
  }
  const slotMap = {};
  for (const s of slots) {
    const g = s.global, c = s._colorTransform;
    slotMap[s.name] = [
      s.displayIndex, s.visible ? 1 : 0, s._zOrder, s._zIndex,
      fin(c.redMultiplier), fin(c.greenMultiplier), fin(c.blueMultiplier), fin(c.alphaMultiplier),
      fin(c.redOffset), fin(c.greenOffset), fin(c.blueOffset), fin(c.alphaOffset),
      fin(g.x), fin(g.y),
      fin(s._globalAlpha),
    ];
  }
  frames.push({ i, bones: boneMap, slots: slotMap, events: events.slice() });
}

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify({
  meta: {
    skeFile: path.basename(skePath), armature: armName, animation: animName,
    frameRate, dt, frameCount,
    boneNames: bones.map(b => b.name),
    slotNames: slots.map(s => s.name),
  },
  frames,
}));
console.log(`OK ${path.basename(skePath)} / ${armName} / ${animName}: ${bones.length} bones, ${slots.length} slots, ${frames.length} frames`);