//! `armature/Constraint.ts` 的 `PathConstraint`（P1-2）。
//!
//! 本次落地覆盖 DragonBones Pro Path 的常见拓扑：path slot 的父骨与链骨父骨
//! 都是 constraint root（合成夹具与典型路径骨架均如此）；其余拓扑返回/跳过并
//! 在 `docs/model_mapping.md` D-6.5 登记。数值公式（顶点更新、Bézier 采样、
//! tangent/chain/chainScale、translate/rotate mix）逐行照 TS。

use std::f64::consts::PI;

use super::{Bone, BoneIdx, Slot, SlotIdx};
use crate::core::enums::{PositionMode, RotateMode, SpacingMode, binary_offset};
use crate::geom::transform::{DEG_RAD, PI_D};
use crate::model::{ConstraintData, ConstraintKind, DisplayKind, GeometryData};

/// `PathConstraint.ts:230-834` 的 Rust 形态。
#[derive(Debug, Clone)]
pub struct PathConstraint {
    pub path_slot: SlotIdx,
    pub bones: Vec<BoneIdx>,
    pub root: BoneIdx,
    pub target: BoneIdx,
    pub position_mode: PositionMode,
    pub spacing_mode: SpacingMode,
    pub rotate_mode: RotateMode,
    pub dirty: bool,
    pub path_offset: i32,
    pub position: f64,
    pub spacing: f64,
    pub rotate_offset: f64,
    pub rotate_mix: f64,
    pub translate_mix: f64,
    spaces: Vec<f64>,
    positions: Vec<f64>,
    bone_lengths: Vec<f64>,
    path_global_vertices: Vec<f64>,
    segments: [f64; 10],
}

impl PathConstraint {
    /// 对齐 `PathConstraint.init`（Constraint.ts:636-668）。
    #[must_use]
    pub fn init(data: &ConstraintData, bones: &[Bone], slots: &[Slot]) -> Option<Self> {
        let ConstraintKind::Path {
            path_slot,
            path_display: _,
            bones: bone_names,
            position_mode,
            spacing_mode,
            rotate_mode,
            position,
            spacing,
            rotate_offset,
            rotate_mix,
            translate_mix,
        } = &data.kind
        else {
            return None;
        };
        let path_slot = slots.iter().position(|slot| slot.slot_name == *path_slot)?;
        let path_offset = slots
            .get(path_slot)
            .and_then(slot_raw_display)
            .and_then(|display| match display.kind {
                DisplayKind::Path { geometry, .. } => Some(geometry.offset),
                _ => None,
            })?;
        let path_slot = SlotIdx(u32::try_from(path_slot).ok()?);
        let root_name = &data.root;
        let root = bones
            .iter()
            .position(|bone| bone.bone_name == *root_name)
            .map(|index| BoneIdx(u32::try_from(index).unwrap_or(0)))?;
        let target = bones
            .iter()
            .position(|bone| bone.bone_name == data.target)
            .map(|index| BoneIdx(u32::try_from(index).unwrap_or(0)))
            .unwrap_or(root);
        let bones = bone_names
            .iter()
            .filter_map(|name| {
                bones
                    .iter()
                    .position(|bone| bone.bone_name == *name)
                    .map(|index| BoneIdx(u32::try_from(index).unwrap_or(0)))
            })
            .collect::<Vec<_>>();
        if bones.is_empty() {
            return None;
        }
        Some(Self {
            path_slot,
            bones,
            root,
            target,
            position_mode: *position_mode,
            spacing_mode: *spacing_mode,
            rotate_mode: *rotate_mode,
            dirty: true,
            path_offset,
            position: *position,
            spacing: *spacing,
            rotate_offset: *rotate_offset,
            rotate_mix: *rotate_mix,
            translate_mix: *translate_mix,
            spaces: Vec::new(),
            positions: Vec::new(),
            bone_lengths: Vec::new(),
            path_global_vertices: Vec::new(),
            segments: [0.0; 10],
        })
    }

    /// 对齐 `PathConstraint.update`（Constraint.ts:670-829）。
    ///
    /// 前置：`root.update_by_constraint(left)` 已由 [`super::Constraint::update_path`]
    /// 完成；本函数更新链骨、路径顶点、Bézier 采样并改写链骨矩阵。
    pub fn update(
        &mut self,
        root: &mut Bone,
        left: &mut [Bone],
        right: &mut [Bone],
        slots: &mut [Slot],
        flip_x: bool,
        flip_y: bool,
    ) {
        let Some(slot_pos) = slots.iter().position(|slot| slot.idx == self.path_slot) else {
            return;
        };
        let Some(geometry) = slots[slot_pos].geometry_data.clone() else {
            return;
        };
        if geometry.offset != self.path_offset {
            return;
        }
        let Some(raw_display) = slot_raw_display(&slots[slot_pos]) else {
            return;
        };
        let DisplayKind::Path {
            closed,
            constant_speed,
            curve_lengths,
            ..
        } = &raw_display.kind
        else {
            return;
        };
        let closed = *closed;
        let constant_speed = *constant_speed;
        let curve_lengths = curve_lengths.clone();
        let path_info = PathInfo {
            closed,
            constant_speed,
            curve_lengths: curve_lengths.clone(),
        };

        // 链骨必须先 updateByConstraint（TS :707/:791），这里只支持 parent == root 的拓扑。
        for &bone_idx in &self.bones {
            let Some(pos) = bone_idx.index().checked_sub(root.idx.index() + 1) else {
                log::warn!("PathConstraint: chain bone before root is unsupported (D-6.5)");
                continue;
            };
            let Some(bone) = right.get(pos) else {
                log::warn!("PathConstraint: chain bone out of right slice (D-6.5)");
                continue;
            };
            if bone.parent.is_none() {
                continue;
            }
            update_chain_bone(right, root, pos, flip_x, flip_y);
        }
        // path slot 父骨可能早于/晚于 root，先更新它再取矩阵。
        let path_parent_idx = slots[slot_pos].parent;
        let path_parent_matrix = if path_parent_idx == root.idx {
            root.global_transform_matrix
        } else if path_parent_idx.index() < root.idx.index() {
            let (ancestors, tail) = left.split_at_mut(path_parent_idx.index());
            let Some(parent) = tail.get_mut(0) else {
                return;
            };
            parent.update_by_constraint(ancestors, flip_x, flip_y);
            parent.global_transform_matrix
        } else {
            let Some(pos) = path_parent_idx.index().checked_sub(root.idx.index() + 1) else {
                return;
            };
            if pos >= right.len() {
                return;
            }
            update_chain_bone(right, root, pos, flip_x, flip_y);
            right[pos].global_transform_matrix
        };

        let is_path_vertices_dirty = if root.children_transform_dirty {
            self.update_path_vertices(
                slots,
                slot_pos,
                &geometry,
                path_parent_matrix,
                root,
                left,
                right,
                flip_x,
                flip_y,
            );
            true
        } else if slots[slot_pos].vertices_dirty || slots[slot_pos].is_bones_update(&[]) {
            self.update_path_vertices(
                slots,
                slot_pos,
                &geometry,
                path_parent_matrix,
                root,
                left,
                right,
                flip_x,
                flip_y,
            );
            slots[slot_pos].vertices_dirty = false;
            true
        } else {
            false
        };
        if !is_path_vertices_dirty && !self.dirty {
            return;
        }

        let is_length_mode = self.spacing_mode == SpacingMode::Length;
        let is_chain_scale_mode = self.rotate_mode == RotateMode::ChainScale;
        let is_tangent_mode = self.rotate_mode == RotateMode::Tangent;
        let bone_count = self.bones.len();
        let spaces_count = if is_tangent_mode {
            bone_count
        } else {
            bone_count + 1
        };
        if is_chain_scale_mode {
            self.bone_lengths.resize(bone_count, 0.0);
        }
        self.spaces.resize(spaces_count, 0.0);
        if is_chain_scale_mode || is_length_mode {
            self.spaces[0] = 0.0;
            for i in 0..spaces_count.saturating_sub(1) {
                let Some(bone_idx) = self.bones.get(i).copied() else {
                    break;
                };
                let bone = if bone_idx == root.idx {
                    &*root
                } else {
                    let Some(pos) = bone_idx.index().checked_sub(root.idx.index() + 1) else {
                        continue;
                    };
                    let Some(bone) = right.get(pos) else {
                        continue;
                    };
                    bone
                };
                let bone_length = bone.length;
                let matrix = bone.global_transform_matrix;
                let x = bone_length * matrix.a;
                let y = bone_length * matrix.b;
                let len = (x * x + y * y).sqrt();
                if is_chain_scale_mode {
                    self.bone_lengths[i] = len;
                }
                self.spaces[i + 1] = (bone_length + self.spacing) * len / bone_length;
            }
        } else {
            self.spaces.fill(self.spacing);
        }

        self.compute_bezier_curve(
            &path_info,
            spaces_count,
            is_tangent_mode,
            self.position_mode == PositionMode::Percent,
            self.spacing_mode == SpacingMode::Percent,
        );

        let positions = self.positions.clone();
        let mut rotate_offset = self.rotate_offset;
        let tip = if rotate_offset == 0.0 {
            self.rotate_mode == RotateMode::Chain
        } else {
            let matrix = path_parent_matrix;
            rotate_offset *= if matrix.a * matrix.d - matrix.b * matrix.c > 0.0 {
                DEG_RAD
            } else {
                -DEG_RAD
            };
            false
        };
        let rotate_mix = self.rotate_mix;
        let translate_mix = self.translate_mix;
        let mut bone_x = positions[0];
        let mut bone_y = positions[1];
        macro_rules! sample_bone {
            ($bone:expr, $index:expr) => {{
                let bone = &mut *$bone;
                let i = $index;
                let p = 3 + i * 3;
                let mut matrix = bone.global_transform_matrix;
                matrix.tx += (bone_x - matrix.tx) * translate_mix;
                matrix.ty += (bone_y - matrix.ty) * translate_mix;
                let x = positions[p];
                let y = positions[p + 1];
                let dx = x - bone_x;
                let dy = y - bone_y;
                if is_chain_scale_mode {
                    let length = self.bone_lengths[i];
                    let s = ((dx * dx + dy * dy).sqrt() / length - 1.0) * rotate_mix + 1.0;
                    matrix.a *= s;
                    matrix.b *= s;
                }
                bone_x = x;
                bone_y = y;
                if rotate_mix > 0.0 {
                    let (a, b, c, d) = (matrix.a, matrix.b, matrix.c, matrix.d);
                    let mut r = if is_tangent_mode {
                        positions[p - 1]
                    } else {
                        dy.atan2(dx)
                    };
                    r -= b.atan2(a);
                    if tip {
                        let cos = r.cos();
                        let sin = r.sin();
                        let length = bone.length;
                        bone_x += (length * (cos * a - sin * b) - dx) * rotate_mix;
                        bone_y += (length * (sin * a + cos * b) - dy) * rotate_mix;
                    } else {
                        r += rotate_offset;
                    }
                    if r > PI {
                        r -= PI_D;
                    } else if r < -PI {
                        r += PI_D;
                    }
                    r *= rotate_mix;
                    let cos = r.cos();
                    let sin = r.sin();
                    matrix.a = cos * a - sin * b;
                    matrix.b = sin * a + cos * b;
                    matrix.c = cos * c - sin * d;
                    matrix.d = sin * c + cos * d;
                }
                bone.global_transform_matrix = matrix;
                bone.global.from_matrix(&matrix);
                bone.transform_dirty = true;
            }};
        }
        for (i, &bone_idx) in self.bones.iter().enumerate() {
            if bone_idx == root.idx {
                sample_bone!(root, i);
                continue;
            }
            let Some(pos) = bone_idx.index().checked_sub(root.idx.index() + 1) else {
                continue;
            };
            update_chain_bone(right, root, pos, flip_x, flip_y);
            let Some(bone) = right.get_mut(pos) else {
                continue;
            };
            sample_bone!(bone, i);
        }
        self.dirty = false;
    }

    /// `Constraint._updatePathVertices`（Constraint.ts:278-348）。
    ///
    /// - 无权重（:294-312）：父骨矩阵逐顶点变换（Rust 既有实现；循环上界差异
    ///   见 D-6.12，**不改**——golden 全等）。
    /// - 有权重（:314-347）：`geometry_bones` 逐顶点加权混合（D-6.13 收口）。
    ///
    /// 权重分支里的 `bone.updateByConstraint()`（:334）由 [`update_geometry_bone`]
    /// 按三段骨表借位解析后执行（TS 用 `_parent` 对象引用，无切片问题）。
    #[allow(clippy::too_many_arguments)] // 三段骨表 + flip + 矩阵均为 TS 同点上下文
    fn update_path_vertices(
        &mut self,
        slots: &[Slot],
        slot_pos: usize,
        geometry: &GeometryData,
        matrix: crate::geom::Matrix,
        root: &mut Bone,
        left: &mut [Bone],
        right: &mut [Bone],
        flip_x: bool,
        flip_y: bool,
    ) {
        let Some(data) = geometry
            .root
            .as_ref()
            .or_else(|| slots[slot_pos].data.root.as_ref())
        else {
            return;
        };
        let int_array = &data.ir.int_array;
        let float_array = &data.ir.float_array;
        let Some(vertex_count) = int_array
            .get(geometry.offset as usize + binary_offset::geometry::VERTEX_COUNT)
            .copied()
        else {
            return;
        };
        let scale = slots[slot_pos].armature_data().scale;
        self.path_global_vertices
            .resize(usize::try_from(vertex_count * 2).unwrap_or(0), 0.0);

        // :292/:314 `weightData !== null` → 权重分支；`null` 时走 :294-312 父骨矩阵。
        if let Some(weight) = geometry.weight.as_deref() {
            let geometry_bones = slots[slot_pos].geometry_bones.clone(); // :315
            self.update_weighted_path_vertices(
                int_array,
                float_array,
                vertex_count,
                scale,
                weight,
                &geometry_bones,
                root,
                left,
                right,
                flip_x,
                flip_y,
            );
            return;
        }

        let Some(float_offset) = int_array
            .get(geometry.offset as usize + binary_offset::geometry::FLOAT_OFFSET)
            .copied()
        else {
            return;
        };
        // TS:294-312（D-6.12：官方循环 `i < pathVertexCount; i += 2` 只写前
        // ceil(n/2) 个顶点，Rust 按 n 个顶点写满——既有行为，不改）。
        for i in 0..vertex_count {
            let index = float_offset as usize + (i as usize) * 2;
            let vx = float_array.get(index).copied().unwrap_or(0.0) * scale;
            let vy = float_array.get(index + 1).copied().unwrap_or(0.0) * scale;
            let x = matrix.a * vx + matrix.c * vy + matrix.tx;
            let y = matrix.b * vx + matrix.d * vy + matrix.ty;
            self.path_global_vertices[(i as usize) * 2] = x;
            self.path_global_vertices[(i as usize) * 2 + 1] = y;
        }
    }

    /// `Constraint.ts:314-347` 的加权路径顶点分支逐行翻译。
    ///
    /// 读指针与 TS 完全同序：`iB` 从 `weightOffset + WeigthBoneIndices +
    /// weightBoneCount` 起（每顶点先读 `vertexBoneCount`，再逐骨读 `boneIndex`）；
    /// `iV` 从 `intArray[weightOffset + WeigthFloatOffset]` 起（逐骨读
    /// weight/vx/vy）。`bone === null`（Rust `geometry_bones[i] == None`）时
    /// TS:330-332 **连浮点也不消费**——照抄，不擅自“修正”坏数据的错位语义。
    #[allow(clippy::too_many_arguments)]
    fn update_weighted_path_vertices(
        &mut self,
        int_array: &[i32],
        float_array: &[f64],
        vertex_count: i32,
        scale: f64,
        weight: &crate::model::WeightData,
        geometry_bones: &[Option<BoneIdx>],
        root: &mut Bone,
        left: &mut [Bone],
        right: &mut [Bone],
        flip_x: bool,
        flip_y: bool,
    ) {
        let weight_bone_count = weight.bones.len(); // :316
        let weight_offset = usize::try_from(weight.offset).unwrap_or(usize::MAX); // :318
        let Some(float_offset) = weight_offset
            .checked_add(binary_offset::weight::FLOAT_OFFSET)
            .and_then(|index| int_array.get(index))
            .copied()
        else {
            return;
        };
        let Some(mut i_v) = usize::try_from(float_offset).ok() else {
            return;
        };
        let Some(mut i_b) = weight_offset
            .checked_add(binary_offset::weight::BONE_INDICES)
            .and_then(|base| base.checked_add(weight_bone_count))
        else {
            return;
        };

        for i in 0..vertex_count {
            let vertex_bone_count = int_array.get(i_b).copied().unwrap_or(0); // :323
            i_b = i_b.saturating_add(1);
            let mut x_g = 0.0; // :325
            let mut y_g = 0.0;
            for _ in 0..vertex_bone_count {
                let bone_index = int_array.get(i_b).copied().unwrap_or(-1); // :327
                i_b = i_b.saturating_add(1);
                let bone_idx = usize::try_from(bone_index)
                    .ok()
                    .and_then(|index| geometry_bones.get(index))
                    .copied()
                    .flatten();
                let Some(bone_idx) = bone_idx else {
                    continue; // :329-332 `if (bone === null) continue`（不消费浮点）
                };
                update_geometry_bone(bone_idx, root, left, right, flip_x, flip_y); // :334
                let matrix = geometry_bone_matrix(bone_idx, root, left, right); // :335
                let bone_weight = float_array.get(i_v).copied().unwrap_or(0.0); // :336
                i_v = i_v.saturating_add(1);
                let vx = float_array.get(i_v).copied().unwrap_or(0.0) * scale; // :337
                i_v = i_v.saturating_add(1);
                let vy = float_array.get(i_v).copied().unwrap_or(0.0) * scale; // :338
                i_v = i_v.saturating_add(1);
                x_g += (matrix.a * vx + matrix.c * vy + matrix.tx) * bone_weight; // :339
                y_g += (matrix.b * vx + matrix.d * vy + matrix.ty) * bone_weight; // :340
            }
            let i_w = (i.max(0) as usize) * 2; // :343-344 `_pathGlobalVertices[iW++]`
            self.path_global_vertices[i_w] = x_g;
            self.path_global_vertices[i_w + 1] = y_g;
        }
    }

    /// 对齐 `PathConstraint._computeVertices`（Constraint.ts:350-357）。
    ///
    /// 注意参数语义：`start` 是 `_pathGlobalVertices` 的读起点，`count` 是
    /// `out` 的写终点，`offset` 是 `out` 的写起点（TS 命名反直觉，照抄）。
    fn compute_vertices(&self, start: usize, count: usize, offset: usize, out: &mut [f64]) {
        let mut i = offset;
        let mut iw = start;
        while i < count {
            out[i] = self.path_global_vertices[iw];
            out[i + 1] = self.path_global_vertices[iw + 1];
            i += 2;
            iw += 2;
        }
    }

    /// 对齐 `PathConstraint._computeBezierCurve`（Constraint.ts:357-634）。
    fn compute_bezier_curve(
        &mut self,
        path: &PathInfo,
        space_count: usize,
        tangents: bool,
        percent_position: bool,
        percent_spacing: bool,
    ) {
        let vertex_count = self.path_global_vertices.len() / 2;
        let is_closed = path.closed;
        let mut vertices_length = vertex_count * 2;
        let mut curve_count = vertices_length / 6;
        let mut pre_curve: isize = -1;
        let mut position = self.position;
        self.positions.resize(space_count * 3 + 2, 0.0);
        let mut curve_vertices = Vec::new();

        if !path.constant_speed {
            let lengths = &path.curve_lengths;
            curve_count = curve_count.saturating_sub(if is_closed { 1 } else { 2 });
            let path_length = lengths.get(curve_count).copied().unwrap_or(0.0);
            if percent_position {
                position *= path_length;
            }
            if percent_spacing {
                for i in 0..space_count {
                    self.spaces[i] *= path_length;
                }
            }
            curve_vertices.resize(8, 0.0);
            let mut curve = 0usize;
            for i in 0..space_count {
                let o = i * 3;
                position += self.spaces[i];
                if is_closed {
                    position %= path_length;
                    if position < 0.0 {
                        position += path_length;
                    }
                    curve = 0;
                } else if position < 0.0 || position > path_length {
                    continue;
                }
                let percent = loop {
                    let len = lengths.get(curve).copied().unwrap_or(0.0);
                    if position > len {
                        curve += 1;
                        continue;
                    }
                    break if curve == 0 {
                        position / len
                    } else {
                        let pre_len = lengths.get(curve - 1).copied().unwrap_or(0.0);
                        (position - pre_len) / (len - pre_len)
                    };
                };
                if curve as isize != pre_curve {
                    pre_curve = curve as isize;
                    if is_closed && curve == curve_count {
                        self.compute_vertices(vertices_length - 4, 4, 0, &mut curve_vertices);
                        self.compute_vertices(0, 4, 4, &mut curve_vertices);
                    } else {
                        self.compute_vertices(curve * 6 + 2, 8, 0, &mut curve_vertices);
                    }
                }
                add_curve_position(
                    percent,
                    curve_vertices[0],
                    curve_vertices[1],
                    curve_vertices[2],
                    curve_vertices[3],
                    curve_vertices[4],
                    curve_vertices[5],
                    curve_vertices[6],
                    curve_vertices[7],
                    &mut self.positions,
                    o,
                    tangents,
                );
            }
            return;
        }

        if is_closed {
            vertices_length += 2;
            curve_vertices.resize(vertex_count, 0.0);
            self.compute_vertices(2, vertices_length - 4, 0, &mut curve_vertices);
            self.compute_vertices(0, 2, vertices_length - 4, &mut curve_vertices);
            curve_vertices[vertices_length - 2] = curve_vertices[0];
            curve_vertices[vertices_length - 1] = curve_vertices[1];
        } else {
            curve_count = curve_count.saturating_sub(1);
            vertices_length -= 4;
            curve_vertices.resize(vertices_length, 0.0);
            self.compute_vertices(2, vertices_length, 0, &mut curve_vertices);
        }

        let mut curves = vec![0.0; curve_count];
        let mut path_length = 0.0;
        let mut x1 = curve_vertices[0];
        let mut y1 = curve_vertices[1];
        let mut w = 2;
        for curve in curves.iter_mut() {
            let cx1 = curve_vertices[w];
            let cy1 = curve_vertices[w + 1];
            let cx2 = curve_vertices[w + 2];
            let cy2 = curve_vertices[w + 3];
            let x2 = curve_vertices[w + 4];
            let y2 = curve_vertices[w + 5];
            w += 6;
            let tmpx = (x1 - cx1 * 2.0 + cx2) * 0.1875;
            let tmpy = (y1 - cy1 * 2.0 + cy2) * 0.1875;
            let dddfx = ((cx1 - cx2) * 3.0 - x1 + x2) * 0.09375;
            let dddfy = ((cy1 - cy2) * 3.0 - y1 + y2) * 0.09375;
            let mut ddfx = tmpx * 2.0 + dddfx;
            let mut ddfy = tmpy * 2.0 + dddfy;
            let mut dfx = (cx1 - x1) * 0.75 + tmpx + dddfx * 0.16666667;
            let mut dfy = (cy1 - y1) * 0.75 + tmpy + dddfy * 0.16666667;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx;
            dfy += ddfy;
            ddfx += dddfx;
            ddfy += dddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx;
            dfy += ddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            dfx += ddfx + dddfx;
            dfy += ddfy + dddfy;
            path_length += (dfx * dfx + dfy * dfy).sqrt();
            *curve = path_length;
            x1 = x2;
            y1 = y2;
        }
        if percent_position {
            position *= path_length;
        }
        if percent_spacing {
            for i in 0..space_count {
                self.spaces[i] *= path_length;
            }
        }

        let mut curve_length = 0.0;
        let mut curve = 0usize;
        let mut segment = 0usize;
        for i in 0..space_count {
            let o = i * 3;
            position += self.spaces[i];
            let mut p = position;
            if is_closed {
                p %= path_length;
                if p < 0.0 {
                    p += path_length;
                }
                curve = 0;
            } else if p < 0.0 || p > path_length {
                continue;
            }
            loop {
                let length = curves.get(curve).copied().unwrap_or(0.0);
                if p > length {
                    curve += 1;
                    continue;
                }
                if curve == 0 {
                    p /= length;
                } else {
                    let prev = curves[curve - 1];
                    p = (p - prev) / (length - prev);
                }
                break;
            }
            if curve as isize != pre_curve {
                pre_curve = curve as isize;
                let ii = curve * 6;
                let (cx1, cy1, cx2, cy2, x2, y2) = (
                    curve_vertices[ii + 2],
                    curve_vertices[ii + 3],
                    curve_vertices[ii + 4],
                    curve_vertices[ii + 5],
                    curve_vertices[ii + 6],
                    curve_vertices[ii + 7],
                );
                let (ax1, ay1) = (curve_vertices[ii], curve_vertices[ii + 1]);
                let tmpx = (ax1 - cx1 * 2.0 + cx2) * 0.03;
                let tmpy = (ay1 - cy1 * 2.0 + cy2) * 0.03;
                let dddfx = ((cx1 - cx2) * 3.0 - ax1 + x2) * 0.006;
                let dddfy = ((cy1 - cy2) * 3.0 - ay1 + y2) * 0.006;
                let mut ddfx = tmpx * 2.0 + dddfx;
                let mut ddfy = tmpy * 2.0 + dddfy;
                let mut dfx = (cx1 - ax1) * 0.3 + tmpx + dddfx * 0.16666667;
                let mut dfy = (cy1 - ay1) * 0.3 + tmpy + dddfy * 0.16666667;
                curve_length = (dfx * dfx + dfy * dfy).sqrt();
                self.segments[0] = curve_length;
                for index in 1..8 {
                    dfx += ddfx;
                    dfy += ddfy;
                    ddfx += dddfx;
                    ddfy += dddfy;
                    curve_length += (dfx * dfx + dfy * dfy).sqrt();
                    self.segments[index] = curve_length;
                }
                dfx += ddfx;
                dfy += ddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                self.segments[8] = curve_length;
                dfx += ddfx + dddfx;
                dfy += ddfy + dddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                self.segments[9] = curve_length;
                segment = 0;
            }
            p *= curve_length;
            loop {
                let length = self.segments.get(segment).copied().unwrap_or(0.0);
                if p > length {
                    segment += 1;
                    continue;
                }
                if segment == 0 {
                    p /= length;
                } else {
                    let prev = self.segments[segment - 1];
                    p = segment as f64 + (p - prev) / (length - prev);
                }
                break;
            }
            add_curve_position(
                p * 0.1,
                curve_vertices[curve * 6],
                curve_vertices[curve * 6 + 1],
                curve_vertices[curve * 6 + 2],
                curve_vertices[curve * 6 + 3],
                curve_vertices[curve * 6 + 4],
                curve_vertices[curve * 6 + 5],
                curve_vertices[curve * 6 + 6],
                curve_vertices[curve * 6 + 7],
                &mut self.positions,
                o,
                tangents,
            );
        }
    }
}

fn update_chain_bone(right: &mut [Bone], root: &mut Bone, pos: usize, flip_x: bool, flip_y: bool) {
    let Some(parent_idx) = right.get(pos).and_then(|bone| bone.parent) else {
        return;
    };
    if parent_idx == root.idx {
        right[pos].update_by_constraint_resolved(Some(root), None, flip_x, flip_y);
        return;
    }
    if parent_idx.index() <= root.idx.index() {
        return;
    }
    let parent_pos = parent_idx.index() - root.idx.index() - 1;
    if parent_pos >= pos {
        return;
    }
    let (pre, tail) = right.split_at_mut(pos);
    let (_, parent_tail) = pre.split_at_mut(parent_pos);
    if let (Some(parent), Some(bone)) = (parent_tail.first_mut(), tail.first_mut()) {
        bone.update_by_constraint_resolved(Some(parent), None, flip_x, flip_y);
    }
}

/// `Constraint.ts:334` 的 `bone.updateByConstraint()`（Bone.ts:445-455）——
/// 加权路径几何骨的三段骨表借位版本（TS 直接沿 `_parent` 对象引用解析）。
///
/// - `idx == root`：约束根骨自身，祖先 = `left` → [`Bone::update_by_constraint`]；
/// - `idx < root`：祖先区间 `[0, idx)` 落在 `left` 单切片 → 同上；
/// - `idx > root`：祖先区间跨 left/root/right 三段 → 复用
///   `constraint.rs::resolve_parent_host` 预解析 parent/宿主后走
///   [`Bone::update_by_constraint_resolved`]。
fn update_geometry_bone(
    bone_idx: BoneIdx,
    root: &mut Bone,
    left: &mut [Bone],
    right: &mut [Bone],
    flip_x: bool,
    flip_y: bool,
) {
    // TS:446 `if (this._localDirty)` —— 已更新过的骨整段早退（父骨都不看），
    // 与 bone.rs 同名外置早退同序；三段骨表下先判脏可避免对已更新骨做多余解析。
    let local_dirty = if bone_idx == root.idx {
        root.local_dirty
    } else if bone_idx.index() < root.idx.index() {
        left.get(bone_idx.index())
            .is_some_and(|bone| bone.local_dirty)
    } else {
        bone_idx
            .index()
            .checked_sub(root.idx.index() + 1)
            .and_then(|pos| right.get(pos))
            .is_some_and(|bone| bone.local_dirty)
    };
    if !local_dirty {
        return;
    }

    if bone_idx == root.idx {
        root.update_by_constraint(left, flip_x, flip_y);
        return;
    }
    if bone_idx.index() < root.idx.index() {
        let (ancestors, tail) = left.split_at_mut(bone_idx.index());
        if let Some(bone) = tail.first_mut() {
            bone.update_by_constraint(ancestors, flip_x, flip_y);
        }
        return;
    }
    let Some(pos) = bone_idx.index().checked_sub(root.idx.index() + 1) else {
        return;
    };
    if pos >= right.len() {
        return;
    }
    let (right_pre, tail) = right.split_at_mut(pos);
    let parent = tail.first().and_then(|bone| bone.parent);
    let (parent, surface_bone) =
        super::constraint::resolve_parent_host(root.idx, root, left, right_pre, parent);
    if let Some(bone) = tail.first_mut() {
        bone.update_by_constraint_resolved(parent, surface_bone, flip_x, flip_y);
    }
}

/// 读几何骨 `globalTransformMatrix`（`Constraint.ts:335 const matrix = bone...`）。
fn geometry_bone_matrix(
    bone_idx: BoneIdx,
    root: &Bone,
    left: &[Bone],
    right: &[Bone],
) -> crate::geom::Matrix {
    if bone_idx == root.idx {
        return root.global_transform_matrix;
    }
    if bone_idx.index() < root.idx.index() {
        return left
            .get(bone_idx.index())
            .map_or_else(Default::default, |bone| bone.global_transform_matrix);
    }
    bone_idx
        .index()
        .checked_sub(root.idx.index() + 1)
        .and_then(|pos| right.get(pos))
        .map_or_else(Default::default, |bone| bone.global_transform_matrix)
}

struct PathInfo {
    closed: bool,
    constant_speed: bool,
    curve_lengths: Vec<f64>,
}

fn slot_raw_display(slot: &Slot) -> Option<crate::model::DisplayData> {
    let index = slot
        .display_frame
        .or_else(|| (!slot.display_frames.is_empty()).then_some(0))?;
    slot.display_frames.get(index)?.raw_display_data.clone()
}

#[allow(clippy::too_many_arguments)]
fn add_curve_position(
    t: f64,
    x1: f64,
    y1: f64,
    cx1: f64,
    cy1: f64,
    cx2: f64,
    cy2: f64,
    x2: f64,
    y2: f64,
    out: &mut [f64],
    offset: usize,
    tangents: bool,
) {
    if t == 0.0 {
        out[offset] = x1;
        out[offset + 1] = y1;
        out[offset + 2] = 0.0;
        return;
    }
    if t == 1.0 {
        out[offset] = x2;
        out[offset + 1] = y2;
        out[offset + 2] = 0.0;
        return;
    }
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;
    let a = mt2 * mt;
    let b = mt2 * t * 3.0;
    let c = mt * t2 * 3.0;
    let d = t * t2;
    let x = a * x1 + b * cx1 + c * cx2 + d * x2;
    let y = a * y1 + b * cy1 + c * cy2 + d * y2;
    out[offset] = x;
    out[offset + 1] = y;
    out[offset + 2] = if tangents {
        (y - (a * y1 + b * cy1 + c * cy2)).atan2(x - (a * x1 + b * cx1 + c * cx2))
    } else {
        0.0
    };
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::armature::{BoneIdx, SlotIdx};
    use crate::geom::Matrix;
    use crate::model::{ArmatureData, DragonBonesData, IrArrays, RootInfo, WeightData};
    use std::sync::Arc;

    /// 合成的“加权 Path”上下文：`scale = 2` 的骨架 + 带 `WeightData` 的几何。
    ///
    /// TS 参照：加权路径顶点 `Constraint.ts:314-347`（D-6.13）。
    fn weighted_path_fixture(
        int_array: Vec<i32>,
        float_array: Vec<f64>,
        weight_bones: Vec<&str>,
        geometry_bones: Vec<Option<BoneIdx>>,
    ) -> (PathConstraint, Slot, GeometryData) {
        let mut data = DragonBonesData::default();
        data.add_armature(ArmatureData {
            name: "weighted_path".into(),
            scale: 2.0,
            ..Default::default()
        });
        let slot = Slot {
            data: Arc::new(data),
            armature_name: "weighted_path".into(),
            geometry_bones,
            ..Slot::default()
        };

        let geometry = GeometryData {
            offset: 0,
            root: Some(Arc::new(RootInfo {
                name: "root".into(),
                ir: IrArrays {
                    int_array,
                    float_array,
                    ..Default::default()
                },
            })),
            weight: Some(Box::new(WeightData {
                count: 3,
                offset: 8,
                bones: weight_bones.into_iter().map(str::to_string).collect(),
            })),
            ..Default::default()
        };
        let path = PathConstraint {
            path_slot: SlotIdx(0),
            bones: Vec::new(),
            root: BoneIdx(1),
            target: BoneIdx(1),
            position_mode: PositionMode::Fixed,
            spacing_mode: SpacingMode::Length,
            rotate_mode: RotateMode::Tangent,
            dirty: true,
            path_offset: 0,
            position: 0.0,
            spacing: 0.0,
            rotate_offset: 0.0,
            rotate_mix: 1.0,
            translate_mix: 1.0,
            spaces: Vec::new(),
            positions: Vec::new(),
            bone_lengths: Vec::new(),
            path_global_vertices: Vec::new(),
            segments: [0.0; 10],
        };
        (path, slot, geometry)
    }

    fn preset_bone(idx: u32, matrix: Matrix) -> Bone {
        // 预设矩阵 + localDirty = false → `updateByConstraint` 早退（TS:446），
        // 混合结果只由下面给定的矩阵/权重决定。
        Bone {
            idx: BoneIdx(idx),
            base: crate::armature::TransformObject {
                global_transform_matrix: matrix,
                ..crate::armature::TransformObject::default()
            },
            local_dirty: false,
            ..Bone::default()
        }
    }

    /// 加权路径：`Constraint.ts:314-347` 的逐顶点加权矩阵混合 + `scale` 乘法，
    /// 三段骨表（`idx < root` / `idx == root` / `idx > root`）全部覆盖。
    #[test]
    fn weighted_path_vertices_blend_matrix_by_bone_weight() {
        // int_array 布局：几何头在 0（vertexCount=4, floatOffset=0, weightOffset=8），
        // 权重头在 8（BONE_COUNT=3, FLOAT_OFFSET=100, BONE_INDICES[0,1,2]），
        // 逐顶点数据从 8+2+3=13 起。
        let int_array = vec![
            4, 0, 0, 8, 0, 0, 0, 0, // 几何头
            3, 100, 0, 1, 2, // 权重头（BONE_COUNT=3, floatOffset=100, bone indices）
            2, 0,
            2, // 顶点 0：2 骨 → [geometry_bones[0]=bone0, geometry_bones[2]=bone2]
            1, 0, // 顶点 1：1 骨 → [bone0]
            1, 1, // 顶点 2：1 骨 → [geometry_bones[1]=bone1=root]
            1, 2, // 顶点 3：1 骨 → [bone2]
        ];
        // 浮点：几何顶点 4 个（*2），权重流从 100 起。
        let mut float_array = vec![0.0; 120];
        float_array[0] = 3.0;
        float_array[1] = 4.0; // 顶点 0
        float_array[2] = -2.0;
        float_array[3] = 5.0; // 顶点 1
        float_array[4] = 1.0;
        float_array[5] = 0.0; // 顶点 2
        float_array[6] = 1.0;
        float_array[7] = 0.0; // 顶点 3
        // 顶点 0：bone0 w=.25, vx=3, vy=4；bone2 w=.75, vx=3, vy=4
        float_array[100] = 0.25;
        float_array[101] = 3.0;
        float_array[102] = 4.0;
        float_array[103] = 0.75;
        float_array[104] = 3.0;
        float_array[105] = 4.0;
        // 顶点 1：bone0 w=1, vx=-2, vy=5
        float_array[106] = 1.0;
        float_array[107] = -2.0;
        float_array[108] = 5.0;
        // 顶点 2：root w=1, vx=1, vy=0
        float_array[109] = 1.0;
        float_array[110] = 1.0;
        float_array[111] = 0.0;
        // 顶点 3：bone2 w=1, vx=1, vy=0
        float_array[112] = 1.0;
        float_array[113] = 1.0;
        float_array[114] = 0.0;

        let (mut path, slot, geometry) = weighted_path_fixture(
            int_array,
            float_array,
            vec!["bone0", "bone1", "bone2"],
            vec![Some(BoneIdx(0)), Some(BoneIdx(1)), Some(BoneIdx(2))],
        );
        // 三段骨表：left = [bone0]、root = bone1、right = [bone2]。
        // root 骨矩阵用 90° 旋转；几何骨矩阵刻意取不同仿射。
        let mut bones = [
            preset_bone(0, Matrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 20.0)),
            preset_bone(1, Matrix::new(0.0, 1.0, -1.0, 0.0, 100.0, 200.0)),
            preset_bone(2, Matrix::new(2.0, 0.0, 0.0, 2.0, -5.0, -7.0)),
        ];
        let (left, rest) = bones.split_at_mut(1);
        let (root_slice, right) = rest.split_at_mut(1);
        let root = &mut root_slice[0];
        path.update_path_vertices(
            std::slice::from_ref(&slot),
            0,
            &geometry,
            Matrix::default(),
            root,
            left,
            right,
            false,
            false,
        );

        // scale = 2：顶点 (3,4) → (6,8)。
        // 顶点 0：bone0 → (16,28)*.25 = (4,7)；bone2 → (7,9)*.75 = (5.25,6.75)
        //        → (9.25,13.75)
        // 顶点 1：bone0, (-4,10) → (6,30)
        // 顶点 2：root, (2,0) → (100,202)
        // 顶点 3：bone2, (2,0) → (-1,-7)
        let expected = [9.25, 13.75, 6.0, 30.0, 100.0, 202.0, -1.0, -7.0];
        assert_eq!(path.path_global_vertices.len(), expected.len());
        for (index, want) in expected.iter().enumerate() {
            let got = path.path_global_vertices[index];
            assert!(
                (got - want).abs() <= 1e-12,
                "path_global_vertices[{index}]: want={want} got={got}"
            );
        }
    }

    /// `Constraint.ts:330-332` 的怪癖：`bones[boneIndex] === null` 时 `continue`
    /// **连 weight/vx/vy 三个浮点都不消费**（数组指针错位与官方一致，不“修正”）。
    #[test]
    fn weighted_path_null_geometry_bone_does_not_consume_floats() {
        let int_array = vec![
            1, 0, 0, 8, 0, 0, 0, 0, // 几何头（1 顶点）
            2, 100, 0, 2, // 权重头
            2, 0, 1, // 顶点 0：2 骨 → [geometry_bones[0], geometry_bones[1]]
        ];
        let mut float_array = vec![0.0; 120];
        float_array[0] = 1.0;
        float_array[1] = 0.0; // 顶点 (1,0) → scale 2 → (2,0)
        // 第 1 骨（None）的 3 个浮点不消费；第 2 骨（存在）从 100 起读。
        float_array[100] = 1.0;
        float_array[101] = 1.0;
        float_array[102] = 0.0;

        let (mut path, slot, geometry) = weighted_path_fixture(
            int_array,
            float_array,
            vec!["bone0", "missing"],
            vec![Some(BoneIdx(0)), None], // 第 2 骨解析失败 → TS null
        );
        let mut bones = [
            preset_bone(0, Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)),
            preset_bone(1, Matrix::default()),
        ];
        let (left, rest) = bones.split_at_mut(1);
        let (root_slice, right) = rest.split_at_mut(1);
        let root = &mut root_slice[0];
        path.update_path_vertices(
            std::slice::from_ref(&slot),
            0,
            &geometry,
            Matrix::default(),
            root,
            left,
            right,
            false,
            false,
        );

        // 只累加存在的 bone0：w=1, (2,0) → (2,0)。
        assert_eq!(path.path_global_vertices, vec![2.0, 0.0]);
    }

    #[test]
    fn percent_position_sampling_produces_finite_positions() {
        let mut path = PathConstraint {
            path_slot: SlotIdx(0),
            bones: Vec::new(),
            root: BoneIdx(0),
            target: BoneIdx(0),
            position_mode: PositionMode::Percent,
            spacing_mode: SpacingMode::Length,
            rotate_mode: RotateMode::Tangent,
            dirty: true,
            path_offset: 0,
            position: 0.1,
            spacing: 5.0,
            rotate_offset: 0.0,
            rotate_mix: 1.0,
            translate_mix: 1.0,
            spaces: vec![0.0, 35.0, 35.0],
            positions: Vec::new(),
            bone_lengths: Vec::new(),
            path_global_vertices: Vec::new(),
            segments: [0.0; 10],
        };
        // 15 个点（30 个 float），与 `_computeBezierCurve` 的顶点布局一致。
        for index in 0..15 {
            path.path_global_vertices.push(index as f64 * 10.0);
            path.path_global_vertices
                .push((index as f64 * 0.7).sin() * 20.0);
        }
        let info = PathInfo {
            closed: false,
            constant_speed: true,
            curve_lengths: Vec::new(),
        };
        path.compute_bezier_curve(&info, 3, true, true, false);
        for value in &path.positions[..9] {
            assert!(
                value.is_finite(),
                "percent positions must be finite: {value}"
            );
        }
        assert!(
            path.positions[..9]
                .iter()
                .any(|value| value.abs() > f64::EPSILON)
        );
    }
}
