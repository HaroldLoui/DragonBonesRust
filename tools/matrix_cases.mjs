// tools/matrix_cases.mjs
// 阶段 5 矩阵的资产 → 动画配对声明（唯一事实来源）。
// - pair: 需要两个动画的场景（fade_cross / layer_overlay / reset_to_pose）
// - rep: 单动画代表用例（goto_roundtrip / timescale / dual_armature / dispose）
export const ASSETS = [
  { name: "Dragon", file: "Dragon_ske.json", pair: ["walk", "stand"], rep: "walk" },
  { name: "Dragon_Blend", file: "Dragon_Blend_ske.json", pair: ["walk", "jump"], rep: "walk" },
  { name: "PlantAnimate", file: "PlantAnimate_ske.json", pair: ["Shoot", "Idle"], rep: "Shoot" },
  {
    name: "PlantAnimate_compressed",
    file: "PlantAnimate_ske_compressed.json",
    pair: ["Shoot", "Idle"],
    rep: "Shoot",
  },
  {
    name: "Rooster_Ani",
    file: "Rooster_Ani_ske.json",
    pair: ["rooster_run_anim", "rooster_idle_anim"],
    rep: "rooster_run_anim",
  },
  {
    name: "Rooster_Blend",
    file: "Rooster_Blend_ske.json",
    pair: ["rooster_run_anim", "rooster_idle_anim"],
    rep: "rooster_run_anim",
  },
  {
    name: "Rooster_P1",
    file: "Rooster_P1_ske.json",
    pair: ["rooster_run_anim", "rooster_idle_anim"],
    rep: "rooster_run_anim",
  },
  { name: "龙", file: "龙_ske.json", pair: null, rep: "stand" },
  { name: "Surface_P1_4", file: "Surface_P1_4_ske.json", pair: null, rep: "idle" },
];

/** 需要两个动画的场景；pair=null 的资产跳过。 */
export const PAIR_SCENARIOS = ["fade_cross", "layer_overlay", "reset_to_pose"];
/** 覆盖全部动画的场景。 */
export const ALL_ANIMATION_SCENARIOS = ["replay", "event_dense"];
/** 每个资产取代表动画的场景。 */
export const REPRESENTATIVE_SCENARIOS = [
  "goto_roundtrip",
  "timescale",
  "dual_armature",
  "dispose",
];
