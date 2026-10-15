//! `armature/Surface.ts`（588 行）——任务 4 实现。
//!
//! 形态：`Bone` 的运行时载荷 [`super::bone::BoneRuntime::Surface`]（TS 类继承
//! `Surface extends Bone`，Surface.ts:27 的 Rust 形态）+ 本文件的 [`SurfaceState`]
//! 与 `impl Bone` 的 surface 方法（TS 覆写体）。
//!
//! 要点（`target/stage3_notes/bone_surface.md` §3）：
//! - `_getGlobalTransformMatrix(x, y)`（Surface.ts:165-437）**不是四角双线性插值**
//!   （stage3 指南任务 4 的猜测有误）：是“格子 → 按对角线二分 → 单个三角形
//!   `_getAffineTransform` 拟合仿射矩阵”的分片线性（C0 连续），结果写入静态
//!   `_helpMatrix` 返回，**不写** `self.globalTransformMatrix`（后者只在 4 处
//!   提前返回时作为返回值出现）。
//! - 两处 JS 数组越界靠自动扩容存活，Rust 显式处理（§3.7）：(a) `_hullCache` init
//!   `length = 10`（:463）却读写 `[10]/[11]`（:303-304/354-355/578-579）→ 分配 12；
//!   (b) `_matrixCahce` Left 分支（:194）最大 double 下标 `14*(sx*sy+sx+3sy)-1`
//!   在 `sy > sx` 时超 init 长度（:462 的 `14*(sx*sy+2sx+2sy)`，差 `14*(sy-sx)`；
//!   JS 自动扩容、功能正确，Rust 直接 panic）→ 按
//!   `max(Left 上界, TS 长度)` 扩容（推导见 [`SurfaceState::init`]）。
//! - 静态 help 对象（`Surface._helpMatrix/_helpTransform/_helpPoint`，:186 等）→
//!   局部临时值（transform_object.rs 模块文档的偏差决策，行为恒等：调用点局部独占）。
//! - `matrix_cahce` 元素为 `Cell<f64>`：契约方法 [`Bone::get_global_transform_matrix`]
//!   以 `&self` 被调（slot.rs:720 持 `&Bone`），而 TS 未命中时要写缓存——Cell 让
//!   `&self` 下的读写与 TS 数组逐元素同值。

use std::cell::Cell;

use super::bone::{Bone, BoneRuntime};
use super::BoneIdx;
use crate::core::enums::{binary_offset, BoneType};
use crate::geom::transform::PI;
use crate::geom::{Matrix, Transform};
use crate::model::GeometryData;

/// 对齐 Surface 自有字段（Surface.ts:32-49）。
///
/// `d_x/d_y/k/k_x/k_y/vertices/deform_vertices/hull_cache/matrix_cahce/bone`
/// 逐项对应 TS `_dX/_dY/_k/_kX/_kY/_vertices/_deformVertices/_hullCache/_matrixCahce/_bone`
/// （`_onClear` 见 Surface.ts:51-64）；`segment_x/segment_y/geometry` 为 D-3.3
/// 缓存（TS 每次从 `_boneData as SurfaceData` 取，Surface.ts:173-176/92）。
pub struct SurfaceState {
    /// 对齐 `surfaceData.segmentX`（Surface.ts:174；`_onClear` 语义随 `_boneData = null` 归 0）。
    pub segment_x: i32,
    /// 对齐 `surfaceData.segmentY`（Surface.ts:175）。
    pub segment_y: i32,
    /// 对齐 `_boneData.geometry`（Surface.ts:92/451）——`offset` 定位 IR、`root` 为共享根。
    pub geometry: GeometryData,
    /// 对齐 `_bone`（Surface.ts:49），`_onClear` = null（:63）——宿主骨解析结果
    /// （Surface.ts:469-475：父是普通骨取父、父是 Surface 继承其 `_bone`）。
    pub bone: Option<BoneIdx>,
    /// 对齐 `_dX`（Surface.ts:32），`_onClear` = 0.0（:54）；init = `lA*2/segmentX`（:455）。
    pub d_x: f64,
    /// 对齐 `_dY`（Surface.ts:33 / :55 / :456）。
    pub d_y: f64,
    /// 对齐 `_k`（Surface.ts:34 / :56）——中心格对角线斜率 `-dY/dX`（:457）。
    pub k: f64,
    /// 对齐 `_kX`（Surface.ts:35 / :57）——左右 hull 斜率 `-dY/(lB-lA)`（:458）。
    pub k_x: f64,
    /// 对齐 `_kY`（Surface.ts:36 / :58）——上下 hull 斜率 `-(lB-lA)/dX`（:459）。
    pub k_y: f64,
    /// 对齐 `_vertices`（Surface.ts:38）——世界坐标顶点（`vertexCount * 2`）。
    pub vertices: Vec<f64>,
    /// 对齐 `_deformVertices`（Surface.ts:39）——动画形变偏移，init 全置 0（:465-467）。
    pub deform_vertices: Vec<f64>,
    /// 对齐 `_hullCache`（Surface.ts:43，注释布局 `x1,y1,x2,y2,x3,y3,x4,y4,d1X,d1Y,d2X,d2Y`）——
    /// TS init 只 `length = 10`（:463）却在 :578-579 写 `[10]/[11]`（JS 自动扩容存活），
    /// Rust **显式分配 12**（§3.7(a)），行为与扩容后的 JS 数组逐元素一致。
    pub hull_cache: Vec<f64>,
    /// 对齐 `_matrixCahce`（Surface.ts:47，原文拼写 Cahce）——每条 7 个 double：
    /// `[flag, a, b, c, d, tx, ty]`（Inside/Outside 同布局，:45 注释）；
    /// 分配长度按 §3.7(b) 上界扩容（见 [`SurfaceState::init`]）。
    pub matrix_cahce: Vec<Cell<f64>>,
}

/// 默认值 = `_onClear` 复位值（Surface.ts:51-64；super 部分见 `Bone::default`）。
impl Default for SurfaceState {
    fn default() -> Self {
        Self {
            // Rust 侧（D-3.3 缓存字段）：`_boneData = null` → 无 segment/geometry
            segment_x: 0,
            segment_y: 0,
            geometry: GeometryData::default(), // geometry.clear()（GeometryData.ts）
            bone: None,                        // :63 `_bone = null`
            d_x: 0.0,                          // :54
            d_y: 0.0,                          // :55
            k: 0.0,                            // :56
            k_x: 0.0,                          // :57
            k_y: 0.0,                          // :58
            vertices: Vec::new(),              // :59 `.length = 0`
            deform_vertices: Vec::new(),       // :60 `.length = 0`
            hull_cache: Vec::new(),            // :62 `.length = 0`
            matrix_cahce: Vec::new(),          // :61 `.length = 0`
        }
    }
}

/// `_matrixCahce` 分配长度（`sx/sy` 已 `max(0)`）——TS:462 长度式与各区域最大
/// 下标上界的较大者，推导与行号见 [`SurfaceState::init`] 的注释块。
fn matrix_cahce_alloc_len(sx: i64, sy: i64) -> usize {
    // Left :194 上界所需的槽位数（最大下标+1）= 2*(sx*sy + sx + 3sy)
    let left_slots = sx * sy + sx + 3 * sy;
    // TS:462 槽位数 = sx*sy + 2sx + 2sy
    let ts_slots = sx * sy + 2 * sx + 2 * sy;
    (7 * 2 * left_slots.max(ts_slots)) as usize
}

impl SurfaceState {
    /// 对齐 `Surface.init` 的 super.init 之后部分（Surface.ts:442-477）：
    /// 网格常量（:449-459）、顶点/缓存缓冲（:460-467）；`_bone`（:469-475）由
    /// `Bone::init` 解析后经 `bone` 参数下传（那里持有 existing_bones）。
    pub(super) fn init(
        segment_x: i32,
        segment_y: i32,
        geometry: &GeometryData,
        bone: Option<BoneIdx>,
    ) -> Self {
        // TS:451 `vertexCount = intArray[geometry.offset + GeometryVertexCount]`
        // （`_armature.armatureData.parent.intArray` 两跳 → D-3.3 缓存 geometry.root）
        let vertex_count = match &geometry.root {
            Some(root) => root
                .ir
                .int_array
                .get(geometry.offset as usize + binary_offset::geometry::VERTEX_COUNT)
                .copied()
                .unwrap_or_else(|| {
                    // TS 越界读出 undefined → `.length = NaN` 实际不分配、循环不进 → 等效 0
                    log::warn!(
                        "surface geometry offset {} out of intArray bounds (Surface.ts:451)",
                        geometry.offset
                    );
                    0
                }),
            None => {
                // geometry.root 由 `DragonBonesData::finalize_root` → `ArmatureData::set_root_info`
                //（armature_data.rs:362-369）对所有 Surface 骨统一回填 → None ⟹ 数据未
                // finalize；TS 该路径解引用 null 崩溃，Rust 防御为告警 + 空顶点缓冲
                //（后续 `_update_vertices`/采样对空缓冲防御性跳过，记对照表偏差）。
                log::warn!("surface geometry root not set (Surface.ts:451; finalize_root not run?)");
                0
            }
        };
        let l_b = 1000.0; // :452
        let l_a = 200.0; // :453
        let d_x = l_a * 2.0 / segment_x as f64; // :455（segment 为 0 时得 inf，与 TS 同）
        let d_y = l_a * 2.0 / segment_y as f64; // :456
        let k = -d_y / d_x; // :457
        let k_x = -d_y / (l_b - l_a); // :458
        let k_y = -(l_b - l_a) / d_x; // :459

        // :462 TS 分配 `(sx*sy + 2sx + 2sy) * 2 * 7`；各区域矩阵槽位 base
        // （base/7 = ((…)*2 + isDown)，isDown=1、区域 index 取最大 sy-1/sx-1）推出的
        // 最大 double 下标 `base*7 + 6`：
        //   Left   :194 = ((sx*sy + sx + 2sy + (sy-1))*2+1)*7+6 = 14*(sx*sy+sx+3sy)-1
        //   Right  :245 = 14*(sx*sy+sx+sy)-1
        //   Up     :296 = 14*(sx*sy+sx)-1
        //   Down   :347 = 14*(sx*sy+2sx+sy)-1
        //   Center :394 = 14*(sx*sy)-1
        // Left 在 `sy > sx` 时超 TS 长度（差 14*(sy-sx)；JS 数组自动扩容、功能正确，
        // Rust 直接 panic）→ 取 `max(Left, TS 长)` 显式扩容；Down 恒 ≤ TS 长
        // （差 14*sy），Right/Up/Center 更小，皆被覆盖。
        // sx=1,sy=1（golden 的 `surf`）：Left = TS = 70，恰好不越界——公式对齐无偏差。
        // Left/Down 两区域 base 重合导致的缓存条目 aliasing（§3.7(b)）随原 index 公式保留。
        let (sx, sy) = (i64::from(segment_x.max(0)), i64::from(segment_y.max(0)));
        let matrix_cahce_len = matrix_cahce_alloc_len(sx, sy);
        let vertex_count = vertex_count.max(0) as usize;

        Self {
            segment_x,
            segment_y,
            geometry: geometry.clone(),
            bone, // TS:469-475 的 `_bone`（解析在 Bone::init）
            d_x,
            d_y,
            k,
            k_x,
            k_y,
            vertices: vec![0.0; vertex_count * 2], // :460（JS hole 在读取前必被 _updateVertices 填满，0.0 恒等）
            deform_vertices: vec![0.0; vertex_count * 2], // :461 + :465-467 全置 0.0
            // :463 `length = 10` 但 :578-579 写到 [11] → 显式 12（§3.7(a)）
            hull_cache: vec![0.0; 12],
            // TS `.length = N` 的 hole 读出 undefined → `undefined > 0.0` 为 false ⟺
            // Rust 0.0（`0.0 > 0.0` false）→ 同为“未缓存”（笔记 §3.2）
            matrix_cahce: (0..matrix_cahce_len).map(|_| Cell::new(0.0)).collect(), // :462
        }
    }
}

/// TS 数组读的 Rust 形态：下标为负或越界时返回 NaN——JS `arr[oob]` 读出
/// `undefined` 参与运算得 NaN 并继续流动，不崩（`verticesOffset` 为 undefined 的
/// 路径由调用方用 `i64::MIN` 恒负哨兵走同一 NaN 出口）。
fn ts_read(values: &[f64], index: i64) -> f64 {
    usize::try_from(index)
        .ok()
        .and_then(|i| values.get(i).copied())
        .unwrap_or(f64::NAN)
}

/// TS `_getGlobalTransformMatrix` 缓存命中装载——`helpMatrix.copyFromArray(matrices, matrixIndex + 1)`
///（Matrix.ts:148-157：offset 起 6 个 → a,b,c,d,tx,ty）。Cell 元素无 `&[f64]` 切片 → 逐元素 get。
fn cache_load(help_matrix: &mut Matrix, matrices: &[Cell<f64>], offset: usize) {
    help_matrix.a = matrices[offset].get();
    help_matrix.b = matrices[offset + 1].get();
    help_matrix.c = matrices[offset + 2].get();
    help_matrix.d = matrices[offset + 3].get();
    help_matrix.tx = matrices[offset + 4].get();
    help_matrix.ty = matrices[offset + 5].get();
}

/// TS 未命中写缓存——`:230-236`（Left）/`:281-287`/`:332-338`/`:383-389`/`:426-432`
/// 五处逐字相同：`[flag=1.0, a, b, c, d, tx, ty]`。
fn cache_store(matrices: &[Cell<f64>], matrix_index: usize, help_matrix: &Matrix) {
    matrices[matrix_index].set(1.0);
    matrices[matrix_index + 1].set(help_matrix.a);
    matrices[matrix_index + 2].set(help_matrix.b);
    matrices[matrix_index + 3].set(help_matrix.c);
    matrices[matrix_index + 4].set(help_matrix.d);
    matrices[matrix_index + 5].set(help_matrix.tx);
    matrices[matrix_index + 6].set(help_matrix.ty);
}

/// 对齐 `_getAffineTransform`（Surface.ts:66-88）——三角形拟合的全部数学。
///
/// `(x, y)` = 该三角形在 surface 局部空间的锚点；`(l_x, l_y)` = 局部空间期望边长；
/// `(a, b, c)` = 三个**世界坐标**顶点（a = 锚点对应世界位置）。产出同时写
/// `transform`（TS `Surface._helpTransform`）与 `matrix`（TS `_helpMatrix`），
/// `matrix.tx/ty` 在末两行被改写（先 `toMatrix` 写入 transform 旧平移再覆盖）。
#[allow(clippy::too_many_arguments)] // TS 形参即 13 个，照抄签名
fn get_affine_transform(
    x: f64,
    y: f64,
    l_x: f64,
    l_y: f64,
    a_x: f64,
    a_y: f64,
    b_x: f64,
    b_y: f64,
    c_x: f64,
    c_y: f64,
    transform: &mut Transform,
    matrix: &mut Matrix,
    is_down: bool,
) {
    let dab_x = b_x - a_x; // :71
    let dab_y = b_y - a_y; // :72
    let dac_x = c_x - a_x; // :73
    let dac_y = c_y - a_y; // :74

    transform.rotation = f64::atan2(dab_y, dab_x); // :76 以 a→b 定旋转
    transform.skew = f64::atan2(dac_y, dac_x) - PI * 0.5 - transform.rotation; // :77 a→c 相对 a→b 夹角减 90°

    if is_down {
        transform.rotation += PI; // :80 ★ down 三角整体转 180°（不改 skew/scale）
    }

    transform.scale_x = (dab_x * dab_x + dab_y * dab_y).sqrt() / l_x; // :83 实际边长/期望边长
    transform.scale_y = (dac_x * dac_x + dac_y * dac_y).sqrt() / l_y; // :84
    transform.to_matrix(matrix); // :85
    // :86-87 平移使局部点 (x,y) 落到世界点 (aX,aY)（TS 链式赋值 → 先算后写两处）
    let tx = a_x - (matrix.a * x + matrix.c * y);
    let ty = a_y - (matrix.b * x + matrix.d * y);
    transform.x = tx;
    matrix.tx = tx;
    transform.y = ty;
    matrix.ty = ty;
}

impl Bone {
    /// 对齐 `Surface._updateGlobalTransformMatrix(isCache)`（Surface.ts:133-163）——
    /// 与 Bone 版（Bone.ts:109-330）完全不同，且**忽略 `isCache`**（:135）。
    ///
    /// 取本骨 `_vertices` 的四角（左上/右上/右下/左下），经对角线中点求中心与
    /// b/c 点，用 `get_affine_transform(0,0,lA,lA,…)` 拟合“曲面整体”矩阵与 global。
    pub(super) fn surface_update_global_transform_matrix(&mut self, _is_cache: bool) {
        // :135 `isCache;`（原版显式弃置表达式）——Surface 版完全不消费
        let BoneRuntime::Surface(state) = &mut self.runtime else {
            unreachable!("update 的 is_surface 分派保证 runtime 为 Surface");
        };
        // TS 数组越界读出 undefined → 参与运算得 NaN 继续流动（不崩）；负下标
        //（len-2 在空缓冲时为 -2）同样 undefined → NaN——Rust 用 ts_read 同语义，
        // 不设长度 guard（合法路径的 fp 运算顺序与 TS 逐字相同）。
        let segment_x_d = (state.segment_x * 2) as usize; // :137
        let last_index = state.vertices.len() as i64 - 2; // :138（JS 允许为负）
        let l_a = 200.0; // :139
        //
        let ra_x = ts_read(&state.vertices, 0); // :141 左上角
        let ra_y = ts_read(&state.vertices, 1);
        let rb_x = ts_read(&state.vertices, segment_x_d as i64); // :143 右上角
        let rb_y = ts_read(&state.vertices, segment_x_d as i64 + 1);
        let rc_x = ts_read(&state.vertices, last_index); // :145 右下角
        let rc_y = ts_read(&state.vertices, last_index + 1);
        let rd_x = ts_read(&state.vertices, last_index - segment_x_d as i64); // :147 左下角
        let rd_y = ts_read(&state.vertices, last_index - segment_x_d as i64 + 1);
        //
        let dac_x = ra_x + (rc_x - ra_x) * 0.5; // :150 a-c 对角线中点
        let dac_y = ra_y + (rc_y - ra_y) * 0.5;
        let dbd_x = rb_x + (rd_x - rb_x) * 0.5; // :152 b-d 对角线中点
        let dbd_y = rb_y + (rd_y - rb_y) * 0.5;
        let a_x = dac_x + (dbd_x - dac_x) * 0.5; // :154 中心
        let a_y = dac_y + (dbd_y - dac_y) * 0.5;
        let b_x = rb_x + (rc_x - rb_x) * 0.5; // :156 右边中点
        let b_y = rb_y + (rc_y - rb_y) * 0.5;
        let c_x = rd_x + (rc_x - rd_x) * 0.5; // :158 下边中点
        let c_y = rd_y + (rc_y - rd_y) * 0.5;
        // TODO interpolation（TS:160 原注释保留）
        // Deref 借用限制：`self.global` 与 `self.global_transform_matrix` 都经
        // TransformObject 透传 → 同时取两个 &mut 会整借 `self` → 用局部承接再写回。
        let mut global = self.global;
        get_affine_transform(
            0.0, 0.0, l_a, l_a, // :161 局部原点锚定、期望边长 lA
            a_x, a_y, b_x, b_y, c_x, c_y,
            &mut global,
            &mut self.global_transform_matrix,
            false,
        );
        self.global = global;
        self.global_dirty = false; // :162 ★ Surface 版在此清脏（后续 updateGlobalTransform 变 no-op）
    }

    /// 对齐 `Surface._updateVertices()`（Surface.ts:90-131）——把局部顶点（+ 形变）
    /// 变换到世界坐标写入 `_vertices`。
    ///
    /// `parent`：本骨父骨（update 从祖先切片解析）——三分支（:100-111 父是 Surface /
    /// :112-122 父是普通骨 / :124-130 无父）。
    pub(super) fn surface_update_vertices(&mut self, parent: Option<&Bone>) {
        let BoneRuntime::Surface(state) = &mut self.runtime else {
            unreachable!("runtime 为 Surface 才进本方法");
        };
        // TS:91-94 `data = _armature.armatureData.parent`（IR 根两跳）→ D-3.3 的
        // geometry.root；缺失时 init 已 warn，此处防御性返回（见 SurfaceState::init）
        let Some(root) = state.geometry.root.clone() else {
            return;
        };
        let float_array = &root.ir.float_array;
        // :95-96 TS 直接下标：越界读出 undefined——vertexCount=undefined 时
        // `i < undefined` 恒 false（循环不进，等效读成 0）；verticesOffset=undefined
        // 时 `undefined + iD` → NaN 下标 → floatArray[NaN] → undefined → x/y 全 NaN
        // 继续流动（不崩）——Rust 以 ts_read/None 同语义（仅坏数据可达）。
        let geometry_offset = state.geometry.offset as usize;
        let vertex_count = root
            .ir
            .int_array
            .get(geometry_offset + binary_offset::geometry::VERTEX_COUNT)
            .copied()
            .unwrap_or(0); // :95（TS undefined → 循环不进 ⟺ 0）
        let vertices_offset = root
            .ir
            .int_array
            .get(geometry_offset + binary_offset::geometry::FLOAT_OFFSET)
            .copied()
            .map(i64::from); // :96（None ⟺ undefined → 下标 NaN）
        let deform = &state.deform_vertices; // :98（init 时按同一 int_array 定长，:461 恒在界）
        // verticesOffset=undefined ⟹ 下标恒 NaN（i64::MIN 恒负 → ts_read 读出 NaN）
        let v_base = vertices_offset.unwrap_or(i64::MIN);

        match parent {
            Some(parent) if parent.bone_type == BoneType::Surface => {
                // :100-111 父是 Surface：每顶点按曲面在该点的采样矩阵变换——
                // `get_global_transform_matrix` 返回值拷贝，立即使用后才可能被下次
                // 采样覆盖（对应 TS 静态 _helpMatrix 的“取到就用”约束）
                for i in 0..vertex_count {
                    let i_d = (i * 2) as usize;
                    let x = ts_read(float_array, v_base.saturating_add(i_d as i64)) + deform[i_d]; // :104
                    let y = ts_read(float_array, v_base.saturating_add(i_d as i64 + 1)) + deform[i_d + 1]; // :105
                    let matrix = parent.get_global_transform_matrix(x, y); // :106
                    //
                    state.vertices[i_d] = matrix.a * x + matrix.c * y + matrix.tx; // :108
                    state.vertices[i_d + 1] = matrix.b * x + matrix.d * y + matrix.ty; // :109
                }
            }
            Some(parent) => {
                // :112-122 父是普通骨：矩阵循环外取一次
                let parent_matrix = parent.global_transform_matrix; // :113
                for i in 0..vertex_count {
                    let i_d = (i * 2) as usize;
                    let x = ts_read(float_array, v_base.saturating_add(i_d as i64)) + deform[i_d]; // :116
                    let y = ts_read(float_array, v_base.saturating_add(i_d as i64 + 1)) + deform[i_d + 1]; // :117
                    //
                    state.vertices[i_d] =
                        parent_matrix.a * x + parent_matrix.c * y + parent_matrix.tx; // :119
                    state.vertices[i_d + 1] =
                        parent_matrix.b * x + parent_matrix.d * y + parent_matrix.ty; // :120
                }
            }
            None => {
                // :124-130 无父：世界坐标 = 局部 + 形变（不做矩阵变换）
                for i in 0..vertex_count {
                    let i_d = (i * 2) as usize;
                    let x = ts_read(float_array, v_base.saturating_add(i_d as i64)) + deform[i_d]; // :127
                    let y = ts_read(float_array, v_base.saturating_add(i_d as i64 + 1)) + deform[i_d + 1]; // :128
                    state.vertices[i_d] = x;
                    state.vertices[i_d + 1] = y;
                }
            }
        }
    }

    /// 对齐 Surface.ts:537-539——脏帧清矩阵缓存 flag（-1.0 < 0 → 未命中）。
    pub(super) fn surface_reset_matrix_cache(&mut self) {
        let BoneRuntime::Surface(state) = &mut self.runtime else {
            unreachable!("update 的 is_surface 分派保证 runtime 为 Surface");
        };
        // TS:537 `for (i = 0, l = _matrixCahce.length; i < l; i += 7)`——按当前长度，
        // JS 里被写“长出来”的条目同样被 reset；Rust 上界扩容后一次性覆盖（§3.7(b)）
        for i in (0..state.matrix_cahce.len()).step_by(7) {
            state.matrix_cahce[i].set(-1.0); // :538（-1.0 < 0 → 未缓存）
        }
    }

    /// 对齐 Surface.ts:556-579——填 `_hullCache`（hull 区域采样的端点与方向）。
    ///
    /// update 的顺序契约（笔记 §3.6）：清 flag（:537）→ 顶点（:541）→ 矩阵（:546）→
    /// **hull（本方法）**——依赖当帧算出的 global/矩阵。
    pub(super) fn surface_update_hull_cache(&mut self) {
        let l_b = 1000.0; // :557
        let l_a = 200.0; // :558
        let dd_x = 2.0 * self.global.x; // :559 ★ hull 方向 = 2×global 平移（把 ±1000 端点镜像到骨架空间）
        let dd_y = 2.0 * self.global.y; // :560
        let matrix = self.global_transform_matrix; // Copy：先取副本（Deref 整借 self），再动 runtime
        let BoneRuntime::Surface(state) = &mut self.runtime else {
            unreachable!("update 的 is_surface 分派保证 runtime 为 Surface");
        };
        // TS 静态 _helpPoint → `transform_point` 的返回值（transform_object.rs 偏差决策）

        let (px, py) = matrix.transform_point(l_b, -l_a, false); // :563（delta=false 含 tx/ty）
        state.hull_cache[0] = px; // :564 右上 hull 端点
        state.hull_cache[1] = py; // :565
        state.hull_cache[2] = dd_x - px; // :566 与镜像端点的方向差
        state.hull_cache[3] = dd_y - py; // :567
        let (px, py) = matrix.transform_point(0.0, state.d_y, true); // :568 ★ delta=true（不含 tx/ty）
        state.hull_cache[4] = px; // :569 左/右 hull 的单位方向
        state.hull_cache[5] = py; // :570
        //
        let (px, py) = matrix.transform_point(l_a, l_b, false); // :572
        state.hull_cache[6] = px; // :573 左下 hull 端点
        state.hull_cache[7] = py; // :574
        state.hull_cache[8] = dd_x - px; // :575
        state.hull_cache[9] = dd_y - py; // :576
        let (px, py) = matrix.transform_point(state.d_x, 0.0, true); // :577 ★ delta=true
        state.hull_cache[10] = px; // :578 ★ TS 越界下标（length=10）——Rust 恒 12 元（§3.7(a)）
        state.hull_cache[11] = py; // :579
    }

    /// 对齐 `Surface._getGlobalTransformMatrix(x, y)`（Surface.ts:165-437）——
    /// 在 surface 局部网格坐标 (x, y) 处采样“该点的世界矩阵”（三角形拟合，见模块文档）。
    ///
    /// 返回 `Matrix` **值**（笔记 §0.1/§6.1）：提前返回来源是 `self.global_transform_matrix`
    /// （:169/190/241/292/343），正常路径来源是拟合结果（TS 静态 `_helpMatrix` :436）——
    /// 值拷贝天然消除“静态矩阵被下次调用覆盖”的别名隐患。
    pub(super) fn surface_get_global_transform_matrix(&self, x: f64, y: f64) -> Matrix {
        let state = match &self.runtime {
            BoneRuntime::Surface(state) => state,
            BoneRuntime::Bone => {
                // 防御：契约分派（`get_global_transform_matrix`）保证只在 Surface 上进本方法
                return self.global_transform_matrix;
            }
        };
        if state.vertices.is_empty() {
            // 防御：init 时 geometry.root 缺失 → 顶点缓冲为空（TS 该数据读 undefined →
            // NaN 污染矩阵；Rust 返回自身矩阵保证契约可交付，见 SurfaceState::init）
            return self.global_transform_matrix;
        }

        let l_a = 200.0; // :166
        let l_b = 1000.0; // :167
        if x < -l_b || l_b < x || y < -l_b || l_b < y {
            // :168 ★ 超出 ±1000 → 无 hull 覆盖，退化为整体矩阵
            return self.global_transform_matrix; // :169
        }

        let is_down: bool; // :172（各区域分支内赋值后立即使用，初值不可达）
        let segment_x = state.segment_x as f64; // :174（TS number，公式全程 f64 照抄）
        let segment_y = state.segment_y as f64; // :175
        let segment_x_d = segment_x * 2.0; // :176 ★ 每行 float 数（顶点 x,y 各 1）
        let segment_x_d_us = segment_x_d as usize; // 数组偏移用（整数恒等）
        let d_x = state.d_x; // :177
        let d_y = state.d_y; // :178
        let index_x = f64::floor((x + l_a) / d_x); // :179 -1 ~ segmentX-1（无 clamp，靠区域 guard）
        let index_y = f64::floor((y + l_a) / d_y); // :180
        let matrix_index: f64; // :181（各区域赋值，初值不可达）
        let p_x = index_x * d_x - l_a; // :182 格子左边界（局部坐标）
        let p_y = index_y * d_y - l_a; // :183
        //
        let matrices = &state.matrix_cahce; // :185
        // TS:186 静态 _helpMatrix / 各区域的 _helpTransform → 局部临时（偏差决策见模块文档）
        let mut help_matrix = Matrix::default();
        let mut help_transform = Transform::default();
        let vertices = &state.vertices; // :205 等（提前取只读引用）

        if x < -l_a {
            // :188 —— Left hull 区域
            if y < -l_a || y >= l_a {
                // :189-191 Out.
                return self.global_transform_matrix;
            }
            // :192 Left.
            is_down = y > state.k_x * (x + l_a) + p_y; // :193
            matrix_index = ((segment_x * segment_y
                + segment_x
                + segment_y
                + segment_y
                + index_y)
                * 2.0
                + if is_down { 1.0 } else { 0.0 })
                * 7.0; // :194
            let mi = matrix_index as usize;

            if matrices[mi].get() > 0.0 {
                // :196 ★ flag > 0 视为已缓存
                cache_load(&mut help_matrix, matrices, mi + 1); // :197
            } else {
                let vertex_index = (index_y * (segment_x_d + 2.0)) as usize; // :200 行宽 = segmentXD+2（两端各留 hull 位）
                let dd_x = state.hull_cache[4]; // :201 左/右 hull 单位方向
                let dd_y = state.hull_cache[5]; // :202
                let s_x = state.hull_cache[2] - (segment_y - index_y) * dd_x; // :203 起点 = 端点沿方向回退
                let s_y = state.hull_cache[3] - (segment_y - index_y) * dd_y; // :204

                if is_down {
                    // :207-216 a=左下顶点，b=沿 hull 方向一步，c=左上顶点
                    get_affine_transform(
                        -l_a,
                        p_y + d_y, // :209 局部锚点 (-lA, pY+dY)、期望边长 (lB-lA, dY)
                        l_b - l_a,
                        d_y,
                        vertices[vertex_index + segment_x_d_us + 2], // :210 a
                        vertices[vertex_index + segment_x_d_us + 3],
                        s_x + dd_x, // :212 b
                        s_y + dd_y,
                        vertices[vertex_index], // :214 c
                        vertices[vertex_index + 1],
                        &mut help_transform,
                        &mut help_matrix,
                        true,
                    );
                } else {
                    // :218-227 a=沿 hull 端点起点，b=左上顶点，c=方向一步
                    get_affine_transform(
                        -l_b,
                        p_y, // :220 局部锚点 (-lB, pY)
                        l_b - l_a,
                        d_y,
                        s_x, // :221 a
                        s_y,
                        vertices[vertex_index], // :223 b
                        vertices[vertex_index + 1],
                        s_x + dd_x, // :225 c
                        s_y + dd_y,
                        &mut help_transform,
                        &mut help_matrix,
                        false,
                    );
                }

                cache_store(matrices, mi, &help_matrix); // :230-236
            }
        } else if x >= l_a {
            // :239 —— Right hull 区域
            if y < -l_a || y >= l_a {
                // :240-242 Out.
                return self.global_transform_matrix;
            }
            // :243 Right.
            is_down = y > state.k_x * (x - l_b) + p_y; // :244
            matrix_index = ((segment_x * segment_y + segment_x + index_y) * 2.0
                + if is_down { 1.0 } else { 0.0 })
                * 7.0; // :245
            let mi = matrix_index as usize;

            if matrices[mi].get() > 0.0 {
                cache_load(&mut help_matrix, matrices, mi + 1); // :248
            } else {
                let vertex_index =
                    (((index_y + 1.0) * (segment_x_d + 2.0)) - 2.0) as usize; // :251
                let dd_x = state.hull_cache[4]; // :252
                let dd_y = state.hull_cache[5]; // :253
                let s_x = state.hull_cache[0] + index_y * dd_x; // :254 端点沿方向推进
                let s_y = state.hull_cache[1] + index_y * dd_y; // :255

                if is_down {
                    // :258-267 a=方向一步，b=左下顶点，c=端点起点
                    get_affine_transform(
                        l_b,
                        p_y + d_y, // :260 局部锚点 (lB, pY+dY)
                        l_b - l_a,
                        d_y,
                        s_x + dd_x, // :261 a
                        s_y + dd_y,
                        vertices[vertex_index + segment_x_d_us + 2], // :263 b
                        vertices[vertex_index + segment_x_d_us + 3],
                        s_x, // :265 c
                        s_y,
                        &mut help_transform,
                        &mut help_matrix,
                        true,
                    );
                } else {
                    // :269-278 a=右上顶点，b=端点起点，c=左下顶点
                    get_affine_transform(
                        l_a,
                        p_y, // :271 局部锚点 (lA, pY)
                        l_b - l_a,
                        d_y,
                        vertices[vertex_index], // :272 a
                        vertices[vertex_index + 1],
                        s_x, // :274 b
                        s_y,
                        vertices[vertex_index + segment_x_d_us + 2], // :276 c
                        vertices[vertex_index + segment_x_d_us + 3],
                        &mut help_transform,
                        &mut help_matrix,
                        false,
                    );
                }

                cache_store(matrices, mi, &help_matrix); // :281-287
            }
        } else if y < -l_a {
            // :290 —— Up hull 区域
            if x < -l_a || x >= l_a {
                // :291-293 Out.
                return self.global_transform_matrix;
            }
            // :294 Up.
            is_down = y > state.k_y * (x - p_x - d_x) - l_b; // :295
            matrix_index = ((segment_x * segment_y + index_x) * 2.0
                + if is_down { 1.0 } else { 0.0 })
                * 7.0; // :296
            let mi = matrix_index as usize;

            if matrices[mi].get() > 0.0 {
                cache_load(&mut help_matrix, matrices, mi + 1); // :299
            } else {
                let vertex_index = (index_x * 2.0) as usize; // :302
                // :303-304 ⚠ TS 读 `_hullCache[10]/[11]`——init `length = 10` 的越界读
                //（JS 由 :578-579 的越界写扩容后存活）；Rust hull_cache 恒 12 元（§3.7(a)）
                let dd_x = state.hull_cache[10]; // :303 上/下 hull 单位方向
                let dd_y = state.hull_cache[11]; // :304
                let s_x = state.hull_cache[8] + index_x * dd_x; // :305
                let s_y = state.hull_cache[9] + index_x * dd_y; // :306

                if is_down {
                    // :309-318 a=右上顶点，b=左上顶点，c=方向一步
                    get_affine_transform(
                        p_x + d_x,
                        -l_a, // :311 局部锚点 (pX+dX, -lA)、期望边长 (dX, lB-lA)
                        d_x,
                        l_b - l_a,
                        vertices[vertex_index + 2], // :312 a
                        vertices[vertex_index + 3],
                        vertices[vertex_index], // :314 b
                        vertices[vertex_index + 1],
                        s_x + dd_x, // :316 c
                        s_y + dd_y,
                        &mut help_transform,
                        &mut help_matrix,
                        true,
                    );
                } else {
                    // :320-329 a=方向起点，b=方向一步，c=左上顶点
                    get_affine_transform(
                        p_x,
                        -l_b, // :322 局部锚点 (pX, -lB)
                        d_x,
                        l_b - l_a,
                        s_x, // :323 a
                        s_y,
                        s_x + dd_x, // :325 b
                        s_y + dd_y,
                        vertices[vertex_index], // :327 c
                        vertices[vertex_index + 1],
                        &mut help_transform,
                        &mut help_matrix,
                        false,
                    );
                }

                cache_store(matrices, mi, &help_matrix); // :332-338
            }
        } else if y >= l_a {
            // :341 —— Down hull 区域
            if x < -l_a || x >= l_a {
                // :342-344 Out.
                return self.global_transform_matrix;
            }
            // :345 Down
            is_down = y > state.k_y * (x - p_x - d_x) + l_a; // :346 ⚠ 常数是 lA(200) 不是 lB
            matrix_index = ((segment_x * segment_y
                + segment_x
                + segment_y
                + index_x)
                * 2.0
                + if is_down { 1.0 } else { 0.0 })
                * 7.0; // :347
            let mi = matrix_index as usize;

            if matrices[mi].get() > 0.0 {
                cache_load(&mut help_matrix, matrices, mi + 1); // :350
            } else {
                let vertex_index =
                    ((segment_y * (segment_x_d + 2.0)) + index_x * 2.0) as usize; // :353
                let dd_x = state.hull_cache[10]; // :354
                let dd_y = state.hull_cache[11]; // :355
                let s_x = state.hull_cache[6] - (segment_x - index_x) * dd_x; // :356
                let s_y = state.hull_cache[7] - (segment_x - index_x) * dd_y; // :357

                if is_down {
                    // :360-369 a=方向一步，b=端点起点，c=右下顶点
                    get_affine_transform(
                        p_x + d_x,
                        l_b, // :362 局部锚点 (pX+dX, lB)
                        d_x,
                        l_b - l_a,
                        s_x + dd_x, // :363 a
                        s_y + dd_y,
                        s_x, // :365 b
                        s_y,
                        vertices[vertex_index + 2], // :367 c
                        vertices[vertex_index + 3],
                        &mut help_transform,
                        &mut help_matrix,
                        true,
                    );
                } else {
                    // :371-380 a/b=左下与右下顶点，c=端点起点
                    get_affine_transform(
                        p_x,
                        l_a, // :373 局部锚点 (pX, lA)
                        d_x,
                        l_b - l_a,
                        vertices[vertex_index], // :374 a
                        vertices[vertex_index + 1],
                        vertices[vertex_index + 2], // :376 b
                        vertices[vertex_index + 3],
                        s_x, // :378 c
                        s_y,
                        &mut help_transform,
                        &mut help_matrix,
                        false,
                    );
                }

                cache_store(matrices, mi, &help_matrix); // :383-389
            }
        } else {
            // :392 —— Center（格子内部，按对角线分上下两个三角形）
            is_down = y > state.k * (x - p_x - d_x) + p_y; // :393
            matrix_index = ((segment_x * index_y + index_x) * 2.0
                + if is_down { 1.0 } else { 0.0 })
                * 7.0; // :394
            let mi = matrix_index as usize;

            if matrices[mi].get() > 0.0 {
                cache_load(&mut help_matrix, matrices, mi + 1); // :397
            } else {
                let vertex_index =
                    (index_x * 2.0 + index_y * (segment_x_d + 2.0)) as usize; // :400
                let i = vertex_index;

                if is_down {
                    // :403-412 a=右下，b=左下，c=右上（对角线下三角）
                    get_affine_transform(
                        p_x + d_x,
                        p_y + d_y, // :405 局部锚点 = 格子右下角、期望边长 (dX, dY)
                        d_x,
                        d_y,
                        vertices[i + segment_x_d_us + 4], // :406 a
                        vertices[i + segment_x_d_us + 5],
                        vertices[i + segment_x_d_us + 2], // :408 b
                        vertices[i + segment_x_d_us + 3],
                        vertices[i + 2], // :410 c
                        vertices[i + 3],
                        &mut help_transform,
                        &mut help_matrix,
                        true,
                    );
                } else {
                    // :414-424 a=左上，b=右上，c=左下（对角线上三角）
                    get_affine_transform(
                        p_x,
                        p_y, // :416 局部锚点 = 格子左上角
                        d_x,
                        d_y,
                        vertices[i], // :418 a
                        vertices[i + 1],
                        vertices[i + 2], // :420 b
                        vertices[i + 3],
                        vertices[i + segment_x_d_us + 2], // :422 c
                        vertices[i + segment_x_d_us + 3],
                        &mut help_transform,
                        &mut help_matrix,
                        false,
                    );
                }

                cache_store(matrices, mi, &help_matrix); // :426-432
            }
        }

        help_matrix // :436 ★ 正常路径返回（局部载体替代静态 _helpMatrix，值拷贝）
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P0-1 回归：`_matrixCahce` 分配长必须覆盖 TS 各区域矩阵下标的最大写入。
    /// 区域 base 独立按 Surface.ts 公式重推（isDown=1、区域 index 取最大）：
    ///   Left   :194 base/7 = ((sx*sy + sx + 2sy + (sy-1))*2 + 1)
    ///   Right  :245 base/7 = ((sx*sy + sx + (sy-1))*2 + 1)
    ///   Up     :296 base/7 = ((sx*sy + (sx-1))*2 + 1)
    ///   Down   :347 base/7 = ((sx*sy + sx + sy + (sx-1))*2 + 1)
    ///   Center :394 base/7 = ((sx*(sy-1) + (sx-1))*2 + 1)
    /// 每条缓存 7 个 double → 最大写入下标 = base + 6（五处 cache_store :230-236 等）；
    /// TS 分配长（:462 `(sx*sy+2sx+2sy)*2*7`）在 JS 里同样可被写穿后自动扩容，
    /// 断言时一并纳入“必须覆盖”的最大值。
    #[test]
    fn matrix_cahce_alloc_covers_all_region_max_indices() {
        for sx in 1..=4i64 {
            for sy in 1..=4i64 {
                let left = ((sx * sy + sx + 2 * sy + (sy - 1)) * 2 + 1) * 7 + 6; // :194
                let right = ((sx * sy + sx + (sy - 1)) * 2 + 1) * 7 + 6; // :245
                let up = ((sx * sy + (sx - 1)) * 2 + 1) * 7 + 6; // :296
                let down = ((sx * sy + sx + sy + (sx - 1)) * 2 + 1) * 7 + 6; // :347
                let center = ((sx * (sy - 1) + (sx - 1)) * 2 + 1) * 7 + 6; // :394
                let ts_last = (sx * sy + 2 * sx + 2 * sy) * 2 * 7 - 1; // :462 分配长 - 1

                let max_index = left.max(right).max(up).max(down).max(center).max(ts_last);
                let alloc = matrix_cahce_alloc_len(sx, sy) as i64;
                assert!(
                    max_index < alloc,
                    "sx={sx} sy={sy}: 最大下标 {max_index} ≥ 分配长 {alloc}"
                );
            }
        }
    }

    /// sx=1,sy=2 具体回归值（旧公式 7*2*(sx*sy+2sx+max(sx,sy)+sy) = 112，
    /// 左区最大下标 125 写穿 → panic；新公式 = TS 长度式与 Left 上界取大）。
    #[test]
    fn matrix_cahce_alloc_covers_left_region_when_sy_exceeds_sx() {
        assert_eq!(matrix_cahce_alloc_len(1, 1), 70); // golden 的 `surf`：恰 = TS 长度
        assert_eq!(matrix_cahce_alloc_len(1, 2), 126); // Left 上界 14*(2+1+6)-1 = 125 > 旧公式 112
    }
}
