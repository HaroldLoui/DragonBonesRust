// tools/irdump.mjs — 用 JS 参考运行时转储解析后的 IR 数组 + 结构快照（阶段 2 黄金数据）
// 用法: node tools/irdump.mjs <skeJson> <texJson> <armatureName> <outJson>
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const require = createRequire(import.meta.url);

globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
};
const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

// 与 jsdump.mjs 相同的四件套桩（池键 / 14 抽象方法 / 代理 / 工厂）
class NullAtlas extends db.TextureAtlasData {
  static toString() { return "NullAtlas"; }
  _onClear() { super._onClear(); }
  createTexture() { return new db.TextureData(); }
}
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
class NullFactory extends db.BaseFactory {
  static toString() { return "NullFactory"; }
  _buildTextureAtlasData(ta) { return ta || db.BaseObject.borrowObject(NullAtlas); }
  _buildArmature(pkg) {
    const arm = db.BaseObject.borrowObject(db.Armature);
    const p = new EvProxy();
    arm.init(pkg.armature, p, p, this._dragonBones);
    return arm;
  }
  _buildSlot(pkg, slotData, armature) {
    const s = db.BaseObject.borrowObject(NullSlot);
    s.init(slotData, armature, {}, {});
    return s;
  }
}

const [skePath, texPath, armName, outPath] = process.argv.slice(2);
if (!skePath || !texPath || !armName || !outPath) {
  console.error("usage: node tools/irdump.mjs <skeJson> <texJson> <armatureName> <outJson>");
  process.exit(2);
}

const em = new EvProxy();
const factory = new NullFactory();
factory._dragonBones = new db.DragonBones(em);
const key = path.basename(skePath).replace(/[.]json$/, "");

const raw = JSON.parse(fs.readFileSync(skePath, "utf8"));
const data = factory.parseDragonBonesData(raw, key);
factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);
if (!data) { console.error("parse failed"); process.exit(1); }

// ---- 8 组 IR 数组（TypedArray → 普通数组；JSON 数字即 f32/i16/u16 精确值） ----
const arr = ta => Array.from(ta);
const arrays = {
  intArray: arr(data.intArray),
  floatArray: arr(data.floatArray),
  frameIntArray: arr(data.frameIntArray),
  frameFloatArray: arr(data.frameFloatArray),
  frameArray: arr(data.frameArray),
  timelineArray: arr(data.timelineArray),
  colorArray: arr(data.colorArray),
  frameIndices: arr(data.frameIndices),
};

// ---- 结构快照：锁定解析后的 model 树（IR 之外的行为面） ----
const tf = t => t ? [t.x, t.y, t.skew, t.rotation, t.scaleX, t.scaleY] : null;
const col = c => c ? [c.alphaMultiplier, c.redMultiplier, c.greenMultiplier, c.blueMultiplier,
                       c.alphaOffset, c.redOffset, c.greenOffset, c.blueOffset] : null;
const tl = t => ({ type: t.type, offset: t.offset, frameIndicesOffset: t.frameIndicesOffset,
                   ...(t instanceof db.AnimationTimelineData ? { x: t.x, y: t.y } : {}) });
const timelinesMap = m => {
  const out = {};
  for (const k of Object.keys(m)) out[k] = m[k].map(tl);
  return out;
};
const geometry = g => ({ offset: g.offset, isShared: g.isShared, inheritDeform: g.inheritDeform,
  weight: g.weight ? { count: g.weight.count, offset: g.weight.offset,
                       bones: g.weight.bones.map(b => b.name) } : null });

const armatures = {};
for (const armName2 of data.armatureNames) {
  const a = data.armatures[armName2];
  const bones = {};
  for (const k of Object.keys(a.bones)) {
    const b = a.bones[k];
    bones[k] = {
      parent: b.parent ? b.parent.name : null,
      type: b.type, length: b.length, alpha: b.alpha,
      inheritTranslation: b.inheritTranslation, inheritRotation: b.inheritRotation,
      inheritScale: b.inheritScale, inheritReflection: b.inheritReflection,
      transform: tf(b.transform),
      ...(b.type === 1 ? { segmentX: b.segmentX, segmentY: b.segmentY, geometry: geometry(b.geometry) } : {}),
    };
  }
  const slots = {};
  for (const k of Object.keys(a.slots)) {
    const s = a.slots[k];
    slots[k] = { parent: s.parent ? s.parent.name : null, displayIndex: s.displayIndex,
      zOrder: s.zOrder, zIndex: s.zIndex, alpha: s.alpha, blendMode: s.blendMode,
      color: col(s.color) };
  }
  const constraints = {};
  for (const k of Object.keys(a.constraints)) {
    const c = a.constraints[k];
    constraints[k] = { type: c.type, order: c.order,
      target: c.target ? c.target.name : null, root: c.root ? c.root.name : null,
      bone: c.bone ? c.bone.name : null,
      ...(c.type === 1 ? { positionMode: c.positionMode, spacingMode: c.spacingMode,
          rotateMode: c.rotateMode, position: c.position, spacing: c.spacing,
          rotateOffset: c.rotateOffset, rotateMix: c.rotateMix, translateMix: c.translateMix,
          pathSlot: c.pathSlot ? c.pathSlot.name : null,
          bones: c.bones.map(b => b.name) }
        : { scaleEnabled: c.scaleEnabled, bendPositive: c.bendPositive, weight: c.weight }) };
  }
  const skins = {};
  for (const skinName of Object.keys(a.skins)) {
    const skin = a.skins[skinName];
    const slotsDisp = {};
    for (const slotName of Object.keys(skin.displays)) {
      slotsDisp[slotName] = skin.displays[slotName].map(d => {
        if (d === null) return null;
        const base = { name: d.name, type: d.type, path: d.path, transform: tf(d.transform) };
        if (d.type === 0) base.pivot = [d.pivot.x, d.pivot.y];
        if (d.type === 1) { base.inheritAnimation = d.inheritAnimation;
          base.armature = d.armature ? d.armature.name : null;
          base.actions = d.actions.map(x => [x.type, x.name, x.bone ? x.bone.name : null, x.slot ? x.slot.name : null]); }
        if (d.type === 2) base.geometry = geometry(d.geometry);
        if (d.type === 3 && d.boundingBox) { const bb = d.boundingBox;
          base.boundingBox = { type: bb.type, color: bb.color, width: bb.width, height: bb.height,
                               vertices: Array.from(bb.vertices || []) }; }
        if (d.type === 4) { base.closed = d.closed; base.constantSpeed = d.constantSpeed;
          base.geometry = geometry(d.geometry); }
        return base;
      });
    }
    skins[skinName] = slotsDisp;
  }
  const animations = {};
  for (const animName of Object.keys(a.animations)) {
    const n = a.animations[animName];
    animations[animName] = {
      frameCount: n.frameCount, playTimes: n.playTimes, duration: n.duration,
      scale: n.scale, fadeInTime: n.fadeInTime, blendType: n.blendType,
      frameIntOffset: n.frameIntOffset, frameFloatOffset: n.frameFloatOffset,
      frameOffset: n.frameOffset,
      actionTimeline: n.actionTimeline ? tl(n.actionTimeline) : null,
      zOrderTimeline: n.zOrderTimeline ? tl(n.zOrderTimeline) : null,
      boneTimelines: timelinesMap(n.boneTimelines),
      slotTimelines: timelinesMap(n.slotTimelines),
      constraintTimelines: timelinesMap(n.constraintTimelines),
      animationTimelines: timelinesMap(n.animationTimelines),
    };
  }
  armatures[armName2] = {
    type: a.type, frameRate: a.frameRate, scale: a.scale,
    aabb: [a.aabb.x, a.aabb.y, a.aabb.width, a.aabb.height],
    animationNames: Array.from(a.animationNames),
    sortedBones: a.sortedBones.map(b => b.name),
    sortedSlots: a.sortedSlots.map(s => s.name),
    defaultSkin: a.defaultSkin ? a.defaultSkin.name : null,
    defaultAnimation: a.defaultAnimation ? a.defaultAnimation.name : null,
    defaultActions: a.defaultActions.map(x => [x.type, x.name, x.bone ? x.bone.name : null, x.slot ? x.slot.name : null]),
    actions: a.actions.map(x => [x.type, x.name, x.bone ? x.bone.name : null, x.slot ? x.slot.name : null]),
    bones, slots, constraints, skins, animations,
  };
}

// ---- 图集快照（parseTextureAtlasData 也在阶段 2 范围内） ----
const atlasList = (factory._textureAtlasDataMap[key] || []).map(ta => ({
  width: ta.width, height: ta.height, scale: ta.scale, name: ta.name,
  imagePath: ta.imagePath, autoSearch: ta.autoSearch,
  textures: Object.fromEntries(Object.keys(ta.textures).map(n => {
    const t = ta.textures[n];
    return [n, { rotated: t.rotated, name: t.name,
      region: [t.region.x, t.region.y, t.region.width, t.region.height],
      frame: t.frame ? [t.frame.x, t.frame.y, t.frame.width, t.frame.height] : null }];
  })),
}));

const out = {
  meta: {
    skeFile: path.basename(skePath),
    version: raw.version,
    name: data.name,
    frameRate: data.frameRate,
    stage: data.stage ? data.stage.name : null,
    armatureNames: Array.from(data.armatureNames),
  },
  structure: { armatures },
  atlas: atlasList,
  arrays,
};

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(out));
const kb = Math.round(fs.statSync(outPath).size / 1024);
console.log(`OK ${path.basename(skePath)}: armatures=${Object.keys(armatures).length} ` +
  `int=${arrays.intArray.length} float=${arrays.floatArray.length} ` +
  `frameInt=${arrays.frameIntArray.length} frameFloat=${arrays.frameFloatArray.length} ` +
  `frame=${arrays.frameArray.length} timeline=${arrays.timelineArray.length} ` +
  `color=${arrays.colorArray.length} indices=${arrays.frameIndices.length} (${kb}KB)`);
