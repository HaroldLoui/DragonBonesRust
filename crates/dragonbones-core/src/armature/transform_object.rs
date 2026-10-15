//! `armature/TransformObject.ts`（155 行）——运行时变换对象基类。
//!
//! TS 的继承（`Bone/Slot extends TransformObject`）在 Rust 落成**组合**：
//! 子类持 `base: TransformObject` 字段并实现 `Deref`/`DerefMut`，使
//! `bone.global.x`、`self.origin = Some(…)` 等访问与 TS 逐行同形。
//!
//! 静态 help 对象 `_helpMatrix/_helpTransform/_helpPoint`（TS:37-39）**不复刻**——
//! 调用点均为局部临时值，避免 `thread_local` 样板（纯实现细节，记
//! `docs/model_mapping.md` §4 偏差）。

use crate::armature::ArmatureIdx;
use crate::geom::{Matrix, Transform};
use serde_json::Value;

/// 对齐 `TransformObject`（TransformObject.ts:36）。
///
/// 字段名 = TS 去下划线（`_globalDirty` → `global_dirty`），默认值逐行对齐
/// `_onClear`（TransformObject.ts:100-111）。
#[derive(Debug, Clone, PartialEq)]
pub struct TransformObject {
    /// 对齐 `globalTransformMatrix`（TS:50）——相对骨架坐标系的矩阵，`_onClear` identity。
    pub global_transform_matrix: Matrix,
    /// 对齐 `global`（TS:63）——矩阵分解回填的变换（惰性同步，见 [`Self::update_global_transform`]）。
    pub global: Transform,
    /// 对齐 `offset`（TS:76）——相对父骨/骨架的用户偏移，`_onClear` identity。
    pub offset: Transform,
    /// 对齐 `origin`（TS:80）——F3.3 只读：init 时 `clone` 自 data（data 解析后不可变）。
    pub origin: Option<Transform>,
    /// 对齐 `userData`（TS:84，TS 类型 `any`）。
    pub user_data: Option<Value>,
    /// 对齐 `_globalDirty`（TS:85）——**默认 false**（与 alpha 相反）。
    pub global_dirty: bool,
    /// 对齐 `_alpha`（TS:89），默认 1.0。
    pub alpha: f64,
    /// 对齐 `_globalAlpha`（TS:93），默认 1.0。
    pub global_alpha: f64,
    /// 对齐 `_armature`（TS:97）——D-3.1 不存对象，存 arena 索引；`_onClear` 为 `None`。
    pub armature: Option<ArmatureIdx>,
}

impl TransformObject {
    /// 对齐 `TransformObject.updateGlobalTransform`（TransformObject.ts:135-140）：
    /// **惰性回填**——仅当 `global_dirty` 时把矩阵分解回 `global`。
    ///
    /// 注意 `Transform::from_matrix` 会读 `global.scaleX/scaleY` 旧值作
    /// "backup scale"（Transform.ts:201-226），故必须写回 `self.global`
    /// 本身，不能用默认值承接。
    pub fn update_global_transform(&mut self) {
        if self.global_dirty {
            self.global_dirty = false;
            self.global.from_matrix(&self.global_transform_matrix);
        }
    }
}

/// 默认值 = `_onClear` 复位值（TransformObject.ts:100-111）。
impl Default for TransformObject {
    fn default() -> Self {
        Self {
            global_transform_matrix: Matrix::default(), // .identity()
            global: Transform::default(),               // .identity()
            offset: Transform::default(),               // .identity()
            origin: None,                               // = null
            user_data: None,                            // = null
            global_dirty: false,                        // = false
            alpha: 1.0,                                 // = 1.0
            global_alpha: 1.0,                          // = 1.0
            armature: None,                             // = null
        }
    }
}
