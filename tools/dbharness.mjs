// tools/dbharness.mjs
// 阶段 5 共享的 Node 参考环境：PIXI 桩、Null 适配器、事件记录与姿态采样。
// jsdump.mjs（baseline golden）与 matrix.mjs（交叉场景矩阵）共用本文件。
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";

const require = createRequire(import.meta.url);

// PIXI 打桩：仅满足 bundle 顶层引用（PixiArmatureDisplay extends PIXI.Container），core 不会真正用到
globalThis.PIXI = {
  Container: class {}, Texture: class {}, Rectangle: class {},
  Sprite: class {}, Mesh: class {}, BLEND_MODES: {},
};

export const db = require("../DragonBonesJS/Pixi/Demos/8.x/libs/dragonBones/dragonBones.js");

// 对象池按 String(constructor) 分键，自定义子类必须覆盖静态 toString()
export class NullAtlas extends db.TextureAtlasData {
  static toString() { return "NullAtlas"; }
  _onClear() { super._onClear(); }
  createTexture() { return new db.TextureData(); }
}

// Slot 14 个抽象方法全部 no-op —— 只要核心逻辑，不要渲染
export class NullSlot extends db.Slot {
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
export class EvProxy {
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
export class NullFactory extends db.BaseFactory {
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

export const EVENT_TYPES = [
  "start", "loopComplete", "complete",
  "fadeIn", "fadeInComplete", "fadeOut", "fadeOutComplete",
  "frameEvent", "soundEvent",
];

export const fin = v => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** 建 factory + DragonBones 全局实例（事件在 factory._dragonBones.advanceTime 中派发）。 */
export function createFactory(skePath, texPath, key) {
  const em = new EvProxy();
  const factory = new NullFactory();
  factory._dragonBones = new db.DragonBones(em);
  factory.parseDragonBonesData(JSON.parse(fs.readFileSync(skePath, "utf8")), key);
  factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);
  return { factory, em, key };
}

/** 建 factory + DragonBones 全局实例（dbbin 输入；Node Buffer → ArrayBuffer）。 */
export function createFactoryFromBinary(dbbinPath, texPath, key) {
  const em = new EvProxy();
  const factory = new NullFactory();
  factory._dragonBones = new db.DragonBones(em);
  const buffer = fs.readFileSync(dbbinPath);
  const arrayBuffer = buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  factory.parseDragonBonesData(arrayBuffer, key);
  factory.parseTextureAtlasData(JSON.parse(fs.readFileSync(texPath, "utf8")), null, key);
  return { factory, em, key };
}

/** buildArmature + 显式挂 factory.clock（F2/F4：全局推进才派发事件）。 */
export function buildArmature(factory, armName, key) {
  const armature = factory.buildArmature(armName, key, "", key);
  if (!armature) throw new Error(`buildArmature failed: ${armName}`);
  armature.clock = factory.clock;
  return armature;
}

/** 为 armature 注册 9 类 listener，事件写入调用方给的数组。 */
export function installRecorder(armature, sink, armatureIndex = 0) {
  for (const t of EVENT_TYPES) {
    armature.eventDispatcher.addDBEventListener(t, e => sink.push({
      armature: armatureIndex,
      type: e.type,
      name: e.name ?? "",
      bone: e.bone ? e.bone.name : null,
      slot: e.slot ? e.slot.name : null,
    }));
  }
}

/** jsdump 的逐帧事件形状（无 armature 索引，保持 baseline golden 不变）。 */
export function drainEvents(events) {
  return events.map(({ type, name, bone, slot }) => ({ type, name, bone, slot }));
}

export function snapshotBones(bones) {
  const out = {};
  for (const b of bones) {
    const g = b.global, m = b.globalTransformMatrix;
    out[b.name] = [
      fin(g.x), fin(g.y), fin(g.rotation), fin(g.scaleX), fin(g.scaleY), fin(g.skew),
      fin(m.a), fin(m.b), fin(m.c), fin(m.d), fin(m.tx), fin(m.ty),
      fin(b._globalAlpha),
    ];
  }
  return out;
}

export function snapshotSlots(slots) {
  const out = {};
  for (const s of slots) {
    const g = s.global, c = s._colorTransform;
    out[s.name] = [
      s.displayIndex, s.visible ? 1 : 0, s._zOrder, s._zIndex,
      fin(c.redMultiplier), fin(c.greenMultiplier), fin(c.blueMultiplier), fin(c.alphaMultiplier),
      fin(c.redOffset), fin(c.greenOffset), fin(c.blueOffset), fin(c.alphaOffset),
      fin(g.x), fin(g.y),
      fin(s._globalAlpha),
    ];
  }
  return out;
}

/** 阶段 5 矩阵的 state 摘要（`_animationStates` 是 TS private，仅测试工具读取）。 */
export function snapshotStates(armatures) {
  const out = [];
  armatures.forEach((armature, armatureIndex) => {
    const animation = armature.animation; // dispose 后 _onClear 置 null
    if (!animation) return;
    for (const s of animation._animationStates) {
      out.push({
        armature: armatureIndex,
        name: s.name,
        layer: s.layer,
        weight: s.weight,
        weightResult: s._weightResult,
        currentTime: s.currentTime,
        fadeProgress: s._fadeProgress,
        playing: s.isPlaying,
        completed: s.isCompleted,
      });
    }
  });
  return out;
}

export { path };
