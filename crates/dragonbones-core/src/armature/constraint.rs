//! `armature/Constraint.ts`（835 行；任务书按旧版记 717 行）——阶段 3 收口：IK 真身。
//!
//! ## F3.1 勘误（本文件存在的原因）
//!
//! 指南 F3.1「9 素材无约束」实测为**错误事实**：`龙`（IK_L/IK_R）与
//! `Rooster_Ani`/`Rooster_Blend`/`Rooster_P1`（bone_ik/bone_ik1）原始 JSON 含
//! `"ik"` 数组，共 4 个 IK；Path 素材为 0。pose_golden 里通过的 5 个素材
//! constraints 为空，属对照组干净，不是事实。
//!
//! ## 结构（阶段 3 形态）
//!
//! TS 的抽象基类 `Constraint`（:27-66）与子类 `IKConstraint`（:67-228）合并为
//! 单结构体 [`Constraint`]：`kind: ConstraintType` 承担 `BaseFactory._buildConstraints`
//! 的 switch 分派（BaseFactory.ts:229-248）；`PathConstraint`（:230-834）**结构在、
//! 不执行**——factory 对 Path 数据 warn 跳过 → 运行期恒不可达（P1-2 阶段 6 接真身）。
//! TS `Constraint.ts:72/87/198` 把 `_scaleEnabled` 整行注释（TODO）→ Rust 同样
//! **忽略 `scale_enabled`**（对齐 TS 现状，[`Self::init`] 内注明）。
//!
//! ## 借用方案（本任务核心）
//!
//! 触发点保持现状：root 骨 `Bone::update` ① 分支选择段（Bone.ts:376-382/402-408；
//! Surface.ts:493-499/519-525 逐字同构），在 transform_dirty 消费（:417）之前，
//! 经 `Bone::run_constraints` 以三段骨表调用 [`Self::update`]：
//!
//! | 区 | 内容 | 约束用途 |
//! |---|---|---|
//! | `left` | `bones[0, root)` | root 的 `updateByConstraint` 祖先切片（root.parent ∈ left，F3.2）、`_computeB` 的 `parent.parent` 矩阵、target、chain.parent < root 时的 parent/宿主 |
//! | `root`（调用方 `self` 自身） | `bones[root]` | `Constraint.root` 骨（双骨变体的 `parentGlobal`） |
//! | `right` | `bones(root, n)` | chain 骨（本库素材 `chain.parent == root` ⟹ F3.2 ⟹ `idx > root`，`checked_sub` 校验） |
//!
//! TS 帧内时序对照（`IKConstraint.update()` Constraint.ts:206-216）：
//!
//! | TS | Rust | 说明 |
//! |---|---|---|
//! | :207 `_root.updateByConstraint()` | `root.update_by_constraint(left, …)` | root 祖先区间 `[0,root)` 单切片可构造 → 走 bone.rs 原签名 |
//! | :210 `_bone.updateByConstraint()` | `bone.update_by_constraint_resolved(parent, host, …)` | chain 祖先区间 `[0,chain)` **跨三段且含 root 自身**，无法构成单一 `&mut [Bone]` → parent/宿主预解析成两个显式借用，TS:448/:450 判定与矩阵回填语义不变 |
//! | :211 `_computeB()` / :214 `_computeA()` | [`compute_b`] / [`compute_a`] | 逐行翻译 |
//!
//! 触发之后：chain 骨在同帧靠后的循环序吃到 `transform_dirty`（:207/:210 的
//! `:453` 无条件置脏），自身 `update` ② 因 `_localDirty == false`（仅
//! `updateByConstraint` 置 false，F3.6）跳过矩阵重算 → 保留约束写下的
//! global/矩阵（龙素材 rotation 修正即走此链：`:450 isCache=true` →
//! `Bone.ts:231-233 global.fromMatrix` 回填）。
//!
//! target 在两次 `updateByConstraint` **之后**拷贝（TS 首读点 :98/:110 亦在其后），
//! 全程只读。病态配置 `target == chain` 时 TS 会在 :173 改写后于 :176 再读——
//! 本库 4 个 IK 的 target 均为独立效果骨（`sortBones` wait_constraint 还保证
//! `idx < root`），不可达；偏差登记见阶段 3 报告。

use super::{Bone, BoneIdx};
use crate::core::enums::ConstraintType;
use crate::geom::transform::{PI, normalize_radian};
use crate::model::{ConstraintData, ConstraintKind};

/// 对齐 `Constraint`（基类 :27-66）+ `IKConstraint`（:67-228）的阶段 3 合体。
///
/// 字段与 TS 对应：
/// - `name` / `order` ← `ConstraintData`（基类 `name` getter :60-62；`order` 随数据登记）；
/// - `target` / `root` / `bone` ← `init`（:192-194）按名解析成 `BoneIdx`（D-3.1 索引化）；
/// - `weight` / `bend_positive` ← `IKConstraintData`（:199-200；时间轴状态位，
///   阶段 5 `IKConstraintTimelineState` 会改写，TimelineState.ts:971-972）；
/// - `kind` ← `ConstraintData.type`（BaseFactory.ts:229 switch 的判定字段）。
///
/// 与旧 stub 的字段变更：`name` / `root` / `order` 保留，新增
/// `target` / `bone` / `weight` / `bend_positive` / `kind`（见阶段 3 报告）。
#[derive(Debug, Clone)]
pub struct Constraint {
    /// 约束名（= `ConstraintData.name`）。
    pub name: String,
    /// 根骨索引（TS `_root`，Constraint.ts:193 按名解析；`Bone::update` 里
    /// `root == self.idx` 才更新，Bone.ts:378/404）。
    pub root: BoneIdx,
    /// 顺序（= `ConstraintData.order`，随数据登记；TS 运行期不排序——
    /// `Armature._addConstraint`（Armature.ts:227-231）仅 push）。
    pub order: i32,
    /// 目标骨索引（TS `_target`，Constraint.ts:192；`sortBones.wait_constraint`
    /// 保证 `idx < root`）。
    pub target: BoneIdx,
    /// 链骨索引（TS `_bone`，Constraint.ts:194；`None` = 单骨 IK 走 `_computeA`）。
    pub bone: Option<BoneIdx>,
    /// 对齐 `_weight`（Constraint.ts:82，`_onClear` = 1.0 :89）。
    pub weight: f64,
    /// 对齐 `_bendPositive`（Constraint.ts:77，`_onClear` = false :88）。
    pub bend_positive: bool,
    /// 运行时类型分派（`BaseFactory._buildConstraints` switch，BaseFactory.ts:229）。
    pub kind: ConstraintType,
}

impl Default for Constraint {
    /// 默认值 = TS `_onClear` 复位值（Constraint.ts:84-91 + 基类 :49-54）：
    /// `_constraintData/target/root/bone = null`（→ 空名/零值/None）、
    /// `_bendPositive = false`、`_weight = 1.0`、type = IK。
    fn default() -> Self {
        Self {
            name: String::new(),
            root: BoneIdx(0),
            order: 0,
            target: BoneIdx(0),
            bone: None,
            weight: 1.0,
            bend_positive: false,
            kind: ConstraintType::IK,
        }
    }
}

impl Constraint {
    /// 对齐 `IKConstraint.init(constraintData, armature)`（Constraint.ts:185-204）。
    ///
    /// - 幂等 guard（:186-188 `_constraintData !== null`）：Rust 不持 data 引用，
    ///   用 `name` 非空判定（与 `Bone::init` 的 `bone_name` 同构）。`name` 在 Path
    ///   分派**之前**置位（R9）→ Path 与 IK 分支同享幂等语义。
    /// - 按名解析 target/root/bone（:192-194，TS `armature.getBone` 线性查找）——
    ///   调用时机在 `BaseFactory::_build_bones` 之后 → 骨表已按 `sortedBones` 建齐。
    /// - :198 `this._scaleEnabled = …` 整行在 TS 被注释（TODO）→ Rust 同样
    ///   **忽略 `scale_enabled`**（对齐 TS 现状，不读该字段）。
    /// - :203 `root._hasConstraint = true`——**只标 root**（`Bone::update` 的触发闸）。
    ///
    /// # Panics
    /// - `target` / `root` 骨名在骨架中不存在：数据/装配错误（`ArmatureData::sortBones`
    ///   的 wait_constraint（:223-232）/ wait_parent 保证二者在 `sortedBones`，
    ///   `_buildBones` 按其创建 → 正常必命中）。
    /// - `root` 下标越界：同上，`sortedBones` ⊆ 骨表。
    ///
    /// `bone`（链骨）解析失败按 TS `getBone → null` 语义降级为单骨 IK（`None`）+ warn。
    pub fn init(&mut self, data: &ConstraintData, bones: &mut [Bone]) {
        if !self.name.is_empty() {
            return; // TS:186-188 已 init 直接返回
        }

        // R9：`name`（幂等 guard 的钥匙）+ `order`/`kind` 置位放在 Path 分派**之前**——
        // 原实现 Path 分支先 return → `name` 恒空 → 幂等 guard 对 Path 恒不生效（与
        // IK 分支不一致）。置位后：Path 首次 init = warn+return、再次 init 被 guard
        // 挡下；`kind = Path` 还使 `update()` 走 Path stub 分支（不误入 IK）。
        self.name.clone_from(&data.name); // TS:190
        self.order = data.order;
        self.kind = data.type_(); // BaseFactory.ts:229 的 switch 判定

        // Path 数据在 `BaseFactory::_buildConstraints` 已被 warn 跳过（BaseFactory.ts
        // :235-239 的 Path 分支阶段 6 接）——此处为防御（TS 两个子类 init 各不相同）。
        // Path 行为仍 stub（不可达：factory 跳过 Path 数据）。
        let ConstraintKind::IK {
            bend_positive,
            weight,
            ..
        } = &data.kind
        else {
            log::warn!(
                "constraint `{}` (type Path) init skipped: Path 阶段 6（P1-2）实现（Constraint.ts:636-668）",
                data.name
            );
            return;
        };

        // TS:192-194 按名解析（`as any` 断言在 Rust 侧由 expect 承担）
        self.target = Self::resolve(&data.target, bones).unwrap_or_else(|| {
            panic!(
                "constraint `{}`: target bone `{}` not found (Constraint.ts:192, F3.2/sortBones)",
                data.name, data.target
            )
        });
        self.root = Self::resolve(&data.root, bones).unwrap_or_else(|| {
            panic!(
                "constraint `{}`: root bone `{}` not found (Constraint.ts:193, F3.2/sortBones)",
                data.name, data.root
            )
        });
        self.bone = data.bone.as_deref().and_then(|name| {
            let idx = Self::resolve(name, bones);
            if idx.is_none() {
                // TS:194 `getBone(...) === null` → `_bone = null` → 走单骨 `_computeA`
                log::warn!(
                    "constraint `{}`: chain bone `{}` not found, degrade to single-bone IK (Constraint.ts:194)",
                    data.name,
                    name
                );
            }
            idx
        });

        // TS:199-200（:198 scaleEnabled 整行被注释 → 忽略，见方法文档）
        self.bend_positive = *bend_positive;
        self.weight = *weight;

        // TS:203 ★ 只标 root（`Bone::update` 的 `has_constraint` 触发闸）
        bones[self.root.index()].has_constraint = true;
    }

    /// 对齐 `Constraint.update()`（IK 分派 = `Constraint.ts:206-216`；Path 分派 =
    /// `PathConstraint.update` :670-829「结构在、不执行」——见模块文档）。
    ///
    /// - `root`：约束根骨（调用方 `Bone::run_constraints` 已按
    ///   `Constraint::root == Bone.idx` 过滤，Bone.ts:378/404）。
    /// - `left` / `right`：`Bone::update` 的 `split_at_mut` 三段之二
    ///   （`left = [0, root)`、`right = (root, n)`，见模块文档借用方案表）。
    ///
    /// # Panics
    /// - chain 骨 `idx <= root` 或 `idx` 越界：`chain.parent == root` ⟹ F3.2 ⟹
    ///   `idx > root`，违反属数据/排序错误（见 [`Self::update_ik`]）。
    /// - root 的父骨不在 `left`：违反 F3.2 拓扑序（[`compute_b`] 内 expect）。
    pub fn update(
        &mut self,
        root: &mut Bone,
        left: &mut [Bone],
        right: &mut [Bone],
        flip_x: bool,
        flip_y: bool,
    ) {
        debug_assert_eq!(
            self.root, root.idx,
            "Bone::run_constraints 已按 Constraint::root == Bone.idx 过滤（Bone.ts:378/404）"
        );

        match self.kind {
            ConstraintType::IK => self.update_ik(root, left, right, flip_x, flip_y),
            // P1-2 阶段 6：Path「结构在、不执行」——`_build_constraints` 对 Path 数据
            // warn 跳过 → 运行期恒不可达（无素材；Constraint.ts:670-829 待接）。
            ConstraintType::Path => {}
        }
    }

    /// `IKConstraint.update()`（Constraint.ts:206-216）逐行 + `_computeA`/`_computeB`
    /// 的调用面组织（借用切分见模块文档）。
    fn update_ik(
        &mut self,
        root: &mut Bone,
        left: &mut [Bone],
        right: &mut [Bone],
        flip_x: bool,
        flip_y: bool,
    ) {
        // 局部拷贝（TS `this._weight`/`this._bendPositive` 的读点在下方各行；
        // 时间轴（阶段 5）写这两个字段，与骨表借用无关）
        let root_idx = self.root;
        let target_idx = self.target;
        let weight = self.weight;
        let bend_positive = self.bend_positive;

        // TS:207 —— root 先行：ancestors = [0, root)（root.parent ∈ left，F3.2）
        root.update_by_constraint(left, flip_x, flip_y);

        // ---- 单骨变体（TS:213-215 else → `_computeA`）----
        let Some(bone_idx) = self.bone else {
            // TS:98 的 ikGlobal 首读在 :207 之后 → 拷贝位置同序
            let (ik_x, ik_y) = ik_target_pos(target_idx, root_idx, root, left);
            compute_a(root, ik_x, ik_y, weight); // TS:214
            return;
        };

        // ---- 双骨变体（TS:209-212）----
        // chain ∈ right：`chain.parent == root`（本库 4 个 IK 实测）+ F3.2 拓扑序
        // ⟹ `idx > root`；违反（chain ≤ root 或越界）→ panic（免费断言）
        let bone_pos = bone_idx.index().checked_sub(root_idx.index() + 1).unwrap_or_else(|| {
            panic!(
                "constraint `{}`: chain bone idx {} not after root idx {}（chain.parent==root ⟹ F3.2；阶段 3 不支持 chain ≤ root）",
                self.name, bone_idx.index(), root_idx.index()
            )
        });
        let bone_parent = right
            .get(bone_pos)
            .unwrap_or_else(|| {
                panic!(
                    "constraint `{}`: chain bone idx {} out of `right` (len {})（F3.2 拓扑序被破坏）",
                    self.name,
                    bone_idx.index(),
                    right.len()
                )
            })
            .parent;

        // 在 chain 位置把 `right` 再切成「chain 前段」与「chain 段起」——
        // parent/host 解析只借前段，chain 骨自身借后段首元 → 两借用不重叠
        let (right_pre, bone_tail) = right.split_at_mut(bone_pos);
        let (parent, surface_bone) =
            resolve_parent_host(root_idx, root, left, right_pre, bone_parent);
        let bone = bone_tail
            .first_mut()
            .expect("split_at_mut(bone_pos) 后右段非空");

        // TS:210 —— chain 的 updateByConstraint（parent/host 预解析，bone.rs
        // `update_by_constraint_resolved` = TS:445-455 同体）
        bone.update_by_constraint_resolved(parent, surface_bone, flip_x, flip_y);

        // TS:110 的 ikGlobal 首读在 :210 之后 → 拷贝位置同序
        let (ik_x, ik_y) = ik_target_pos(target_idx, root_idx, root, left);
        compute_b(root, left, bone, ik_x, ik_y, weight, bend_positive); // TS:211
    }

    /// 按名解析骨索引（TS `Armature.getBone`，Armature.ts:589 线性遍历的索引版）。
    fn resolve(name: &str, bones: &[Bone]) -> Option<BoneIdx> {
        bones
            .iter()
            .position(|bone| bone.bone_name == name)
            .map(|index| BoneIdx(index as u32))
    }
}

/// `_computeA` 的 `ikGlobal` 坐标拷贝（Constraint.ts:94 单骨 / :110 双骨别名）。
///
/// target 全程只读（见模块文档末段）。`ArmatureData::sortBones` 的 wait_constraint
/// （armature_data.rs:223-232，TS 同款）保证 target 先于 root 排序 → 常规恒
/// `idx < root` 落 `left`；`idx == root` 按 TS 对象别名语义返回 root 自身；
/// `idx > root` 属排序被破坏 → panic（免费断言）。
fn ik_target_pos(target: BoneIdx, root_idx: BoneIdx, root: &Bone, left: &[Bone]) -> (f64, f64) {
    let bone = if target == root_idx {
        root
    } else if target.index() < root_idx.index() {
        left.get(target.index())
            .expect("target ∈ [0, root)（sortBones wait_constraint 保证）")
    } else {
        panic!(
            "constraint target idx {} >= root idx {}: sortBones wait_constraint 被破坏（armature_data.rs:223-232 / 数据装配错误）",
            target.index(),
            root_idx.index()
        )
    };
    (bone.global.x, bone.global.y)
}

/// chain 骨 `updateByConstraint` 所需的 parent/宿主**跨三段**解析——
/// 语义对齐 `Bone::resolve_parent_context`（Bone.ts:111-117 + :174-175 的 Rust 形态），
/// 差异只在区域从「[0, idx) 单切片」扩成 left / root / right_pre 三段。
///
/// 宿主（`surfaceBone`）恒为 parent 的祖先（Surface.ts:469-475）→ 下标严格小于
/// parent，故同段内 `split_at_mut` 恒可分离两个可变借用。
///
/// - `parent == None`（chain 无父，防御）→ `(None, None)`（TS `_parent === null`）。
/// - `parent == root`（**本库 4 个 IK 全部命中**）→ parent = root 段自身，
///   宿主 < root → `left`。
/// - `parent < root` → parent 与宿主都在 `left` → 复用 bone.rs 单切片解析。
/// - `root < parent < chain`（本库素材不可达，防御实现）→ 宿主按三分区拆分。
fn resolve_parent_host<'a>(
    root_idx: BoneIdx,
    root: &'a mut Bone,
    left: &'a mut [Bone],
    right_pre: &'a mut [Bone],
    parent: Option<BoneIdx>,
) -> (Option<&'a mut Bone>, Option<&'a mut Bone>) {
    let Some(parent_idx) = parent else {
        return (None, None); // TS `_parent === null`
    };

    if parent_idx == root_idx {
        // parent 即 root 段；宿主 = root（Surface 骨时）的祖先 → left
        let host = root.surface_host_idx().map(|host_idx| {
            left.get_mut(host_idx.index())
                .expect("宿主为 root 的祖先（Surface.ts:469-475）⟹ ∈ left（F3.2）")
        });
        return (Some(root), host);
    }

    if parent_idx.index() < root_idx.index() {
        // parent 与宿主（< parent）都在 left → 复用单切片解析（含 Surface 分裂逻辑）
        return Bone::resolve_parent_context(Some(parent_idx), left);
    }

    // root < parent < chain（防御）：宿主三分区
    let pos = parent_idx.index() - root_idx.index() - 1;
    let host_idx = right_pre.get(pos).and_then(Bone::surface_host_idx);
    let Some(host_idx) = host_idx else {
        return (right_pre.get_mut(pos), None); // parent 非 Surface → 无宿主
    };

    if host_idx.index() < root_idx.index() {
        // 宿主在 left、parent 在 right_pre → 两参数天然不重叠
        let host = left
            .get_mut(host_idx.index())
            .expect("宿主 < parent ⟹ ∈ left（F3.2）");
        let (_, parent_tail) = right_pre.split_at_mut(pos);
        (Some(&mut parent_tail[0]), Some(host))
    } else if host_idx == root_idx {
        let (_, parent_tail) = right_pre.split_at_mut(pos);
        (Some(&mut parent_tail[0]), Some(root))
    } else {
        // 宿主与 parent 同在 right_pre（宿主下标 < parent 下标 → 分裂可分离）
        let host_pos = host_idx.index() - root_idx.index() - 1;
        let (pre, parent_tail) = right_pre.split_at_mut(pos);
        let (_, host_tail) = pre.split_at_mut(host_pos);
        (Some(&mut parent_tail[0]), Some(&mut host_tail[0]))
    }
}

/// 对齐 `IKConstraint._computeA()`（Constraint.ts:93-105）——单骨变体逐行翻译。
///
/// 写点：`root.global.rotation`（:103 权重混合）与 `root.globalTransformMatrix`
///（:104 `toMatrix` 无父拼接——`global` 即世界变换）。
fn compute_a(root: &mut Bone, ik_x: f64, ik_y: f64, weight: f64) {
    // TS:94-96 局部别名（root 自身读写 → 逐字段访问；`global` 拷贝见 :104 处）
    let mut radian = (ik_y - root.global.y).atan2(ik_x - root.global.x); // :98 Math.atan2(y, x)
    if root.global.scale_x < 0.0 {
        radian += PI; // :99-101
    }

    // :103 global.rotation += Transform.normalizeRadian(radian - global.rotation) * weight
    let rotation = root.global.rotation + normalize_radian(radian - root.global.rotation) * weight;
    root.global.rotation = rotation;

    let global = root.global; // :104（Transform: Copy——Deref 借用限制：先拷贝再喂矩阵）
    global.to_matrix(&mut root.global_transform_matrix);
}

/// 对齐 `IKConstraint._computeB()`（Constraint.ts:107-183）——双骨变体逐行翻译，
/// **fp 顺序照抄不化简**（`h`/`r`/`dR`/权重混合同式落笔）。
///
/// - `root` = TS `parent`（:109）、`bone` = TS `_bone`（链骨）、`left` 供
///   `parent.parent` 矩阵（:150-154，root.parent ∈ left，F3.2）。
/// - 写点：`bone.global` x/y/rotation（:157-163/:173-174/:181）、`bone` 矩阵（:182）、
///   `root.global.rotation`（:169）与 `root` 矩阵（:170）。
fn compute_b(
    root: &mut Bone,
    left: &[Bone],
    bone: &mut Bone,
    ik_x: f64,
    ik_y: f64,
    weight: f64,
    bend_positive: bool,
) {
    let bone_length = bone.length; // :108 `_boneData.length`（D-3.3 缓存）
    // :109-113 别名：parentGlobal = root.global、global = bone.global、
    //           globalTransformMatrix = bone.globalTransformMatrix（下方逐处访问）

    let x = bone.global_transform_matrix.a * bone_length; // :115
    let y = bone.global_transform_matrix.b * bone_length; // :116
    let l_ll = x * x + y * y; // :117
    let l_l = l_ll.sqrt(); // :118 Math.sqrt
    let mut d_x = bone.global.x - root.global.x; // :119-120
    let mut d_y = bone.global.y - root.global.y;
    let l_pp = d_x * d_x + d_y * d_y; // :121
    let l_p = l_pp.sqrt(); // :122
    let raw_radian = bone.global.rotation; // :123
    let raw_parent_radian = root.global.rotation; // :124
    let raw_radian_a = d_y.atan2(d_x); // :125

    d_x = ik_x - root.global.x; // :127-128
    d_y = ik_y - root.global.y;
    let l_tt = d_x * d_x + d_y * d_y; // :129
    let l_t = l_tt.sqrt(); // :130

    // :132 TS 初值 0.0 在两分支均被无条件覆盖 → 延迟初始化消 `unused_assignments`
    //（bone.rs TS:122 同款处理）
    let mut radian_a;
    if l_l + l_p <= l_t || l_t + l_l <= l_p || l_t + l_p <= l_l {
        // :133 退化（三角形不成立）分支
        radian_a = (ik_y - root.global.y).atan2(ik_x - root.global.x); // :134
        if l_l + l_p <= l_t {
            // :135-136 TS 空分支（照抄不化简：此情形下 bend 判定不参与）
        } else if l_p < l_l {
            radian_a += PI; // :137-139
        }
    } else {
        let h = (l_pp - l_ll + l_tt) / (2.0 * l_tt); // :142
        let r = (l_pp - h * h * l_tt).sqrt() / l_t; // :143
        let h_x = root.global.x + d_x * h; // :144-145
        let h_y = root.global.y + d_y * h;
        let r_x = -d_y * r; // :146-147
        let r_y = d_x * r;

        let mut is_ppr = false; // :149
        if let Some(parent_parent_idx) = root.parent {
            // :150-154 `parent.parent.globalTransformMatrix` 的行列式符号（反射判定）
            let matrix = left
                .get(parent_parent_idx.index())
                .expect("root 的父骨不在 left（F3.2 拓扑序被破坏，Bone.ts:355）")
                .global_transform_matrix;
            is_ppr = matrix.a * matrix.d - matrix.b * matrix.c < 0.0;
        }

        if is_ppr != bend_positive {
            // :156-159（JS `!==` 对 boolean = Rust `!=`）
            bone.global.x = h_x - r_x;
            bone.global.y = h_y - r_y;
        } else {
            bone.global.x = h_x + r_x; // :161-163
            bone.global.y = h_y + r_y;
        }

        radian_a = (bone.global.y - root.global.y).atan2(bone.global.x - root.global.x); // :165
    }

    let d_r = normalize_radian(radian_a - raw_radian_a); // :168
    root.global.rotation = raw_parent_radian + d_r * weight; // :169（写后 root 即 parentGlobal）
    let parent_global = root.global; // :170 先拷贝（Transform: Copy）再喂矩阵
    parent_global.to_matrix(&mut root.global_transform_matrix);
    //
    let current_radian_a = raw_radian_a + d_r * weight; // :172
    bone.global.x = root.global.x + current_radian_a.cos() * l_p; // :173
    bone.global.y = root.global.y + current_radian_a.sin() * l_p; // :174
    //
    let mut radian_b = (ik_y - bone.global.y).atan2(ik_x - bone.global.x); // :176
    if bone.global.scale_x < 0.0 {
        radian_b += PI; // :177-179
    }

    // :181（`parentGlobal.rotation` 读 :169 写后的新值；加减乘顺序照抄）
    bone.global.rotation = root.global.rotation + raw_radian - raw_parent_radian
        + normalize_radian(radian_b - d_r - raw_radian) * weight;
    let global = bone.global; // :182 先拷贝
    global.to_matrix(&mut bone.global_transform_matrix);
}
