//! 对齐 `model/BoundingBoxData.ts`（P1-3）。
//!
//! 三种子类（Rectangle / Ellipse / Polygon）的 `containsPoint` /
//! `intersectsSegment` 逐式翻译；`intersectsSegment` 的出参用
//! `Option<&mut Point>` 对应 TS 的 nullable 对象（A/B 交点、法线弧度）。

use std::f64::consts::PI;

use crate::core::enums::BoundingBoxType;
use crate::geom::Point;

/// 从 Option<&mut Point> 重借用（避免 s_deref_mut 的 clippy 告警）。
fn opt_point_mut<'b>(value: &'b mut Option<&mut Point>) -> Option<&'b mut Point> {
    value.as_mut().map(|point| &mut **point)
}

/// - 边界框数据（`BoundingBoxData.ts`，抽象类的单一落地实现）。
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBoxData {
    pub color: i32,
    pub width: f64,
    pub height: f64,
    pub kind: BoundingBoxKind,
}

/// 三个子类的变体字段（对应 `Rectangle` / `Ellipse` / `PolygonBoundingBoxData`）。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundingBoxKind {
    Rectangle,
    Ellipse,
    Polygon { x: f64, y: f64, vertices: Vec<f64> },
}

impl Default for BoundingBoxData {
    fn default() -> Self {
        Self {
            color: 0x000000,
            width: 0.0,
            height: 0.0,
            kind: BoundingBoxKind::Rectangle,
        }
    }
}

impl BoundingBoxData {
    /// 对齐 TS 子类通过 `_onClear` 设置的 `type` 字段。
    #[must_use]
    pub fn type_(&self) -> BoundingBoxType {
        match self.kind {
            BoundingBoxKind::Rectangle => BoundingBoxType::Rectangle,
            BoundingBoxKind::Ellipse => BoundingBoxType::Ellipse,
            BoundingBoxKind::Polygon { .. } => BoundingBoxType::Polygon,
        }
    }

    /// 对齐 `BoundingBoxData.containsPoint` 的虚分派。
    #[must_use]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        match &self.kind {
            BoundingBoxKind::Rectangle => {
                let width_h = self.width * 0.5;
                if x >= -width_h && x <= width_h {
                    let height_h = self.height * 0.5;
                    return y >= -height_h && y <= height_h;
                }
                false
            }
            BoundingBoxKind::Ellipse => {
                let width_h = self.width * 0.5;
                if x >= -width_h && x <= width_h {
                    let height_h = self.height * 0.5;
                    let y = y * (width_h / height_h);
                    return (x * x + y * y).sqrt() <= width_h;
                }
                false
            }
            BoundingBoxKind::Polygon { x: min_x, y: min_y, vertices } => {
                if x < *min_x || x > self.width || y < *min_y || y > self.height {
                    return false;
                }
                let mut is_in_side = false;
                let mut previous = vertices.len().wrapping_sub(2);
                let mut index = 0;
                while index < vertices.len() {
                    let y_a = vertices[previous + 1];
                    let y_b = vertices[index + 1];
                    if (y_b < y && y_a >= y) || (y_a < y && y_b >= y) {
                        let x_a = vertices[previous];
                        let x_b = vertices[index];
                        if (y - y_b) * (x_a - x_b) / (y_a - y_b) + x_b < x {
                            is_in_side = !is_in_side;
                        }
                    }
                    previous = index;
                    index += 2;
                }
                is_in_side
            }
        }
    }

    /// 对齐 `BoundingBoxData.intersectsSegment` 的虚分派。
    ///
    /// 返回：-1（线段完全在内部）、0（不相交）、1/2/3（交点计数）；
    /// 出参语义照 TS（A 近/ B 远；只有 1 个交点时 A/B 同点）。
    #[allow(clippy::too_many_arguments)] // TS 形参即 7 个
    pub fn intersects_segment(
        &self,
        x_a: f64,
        y_a: f64,
        x_b: f64,
        y_b: f64,
        point_a: Option<&mut Point>,
        point_b: Option<&mut Point>,
        normal_radians: Option<&mut Point>,
    ) -> i32 {
        match &self.kind {
            BoundingBoxKind::Rectangle => {
                let width_h = self.width * 0.5;
                let height_h = self.height * 0.5;
                rectangle_intersects_segment(
                    x_a,
                    y_a,
                    x_b,
                    y_b,
                    -width_h,
                    -height_h,
                    width_h,
                    height_h,
                    point_a,
                    point_b,
                    normal_radians,
                )
            }
            BoundingBoxKind::Ellipse => ellipse_intersects_segment(
                x_a,
                y_a,
                x_b,
                y_b,
                0.0,
                0.0,
                self.width * 0.5,
                self.height * 0.5,
                point_a,
                point_b,
                normal_radians,
            ),
            BoundingBoxKind::Polygon { x, y, vertices } => {
                if rectangle_intersects_segment(
                    x_a,
                    y_a,
                    x_b,
                    y_b,
                    *x,
                    *y,
                    *x + self.width,
                    *y + self.height,
                    None,
                    None,
                    None,
                ) == 0
                {
                    return 0;
                }
                polygon_intersects_segment(
                    x_a,
                    y_a,
                    x_b,
                    y_b,
                    vertices,
                    point_a,
                    point_b,
                    normal_radians,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rectangle（Cohen–Sutherland）
// ---------------------------------------------------------------------------

const OUT_LEFT: i32 = 1;
const OUT_RIGHT: i32 = 2;
const OUT_TOP: i32 = 4;
const OUT_BOTTOM: i32 = 8;

fn compute_out_code(x: f64, y: f64, x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> i32 {
    let mut code = 0;
    if x < x_min {
        code |= OUT_LEFT;
    } else if x > x_max {
        code |= OUT_RIGHT;
    }
    if y < y_min {
        code |= OUT_TOP;
    } else if y > y_max {
        code |= OUT_BOTTOM;
    }
    code
}

/// 对齐 `RectangleBoundingBoxData.rectangleIntersectsSegment`（BoundingBoxData.ts:145-345）。
///
/// 注意：`inSideA` 分支的 `intersectionPointB.y = xB` 是官方笔误（:257），
/// 本实现照抄以保持可观察行为（F6.11）。
#[allow(clippy::too_many_arguments)]
fn rectangle_intersects_segment(
    mut x_a: f64,
    mut y_a: f64,
    mut x_b: f64,
    mut y_b: f64,
    x_min: f64,
    y_min: f64,
    x_max: f64,
    y_max: f64,
    mut point_a: Option<&mut Point>,
    mut point_b: Option<&mut Point>,
    mut normal_radians: Option<&mut Point>,
) -> i32 {
    let in_side_a = x_a > x_min && x_a < x_max && y_a > y_min && y_a < y_max;
    let in_side_b = x_b > x_min && x_b < x_max && y_b > y_min && y_b < y_max;
    if in_side_a && in_side_b {
        return -1;
    }

    let mut intersection_count = 0;
    let mut outcode_0 = compute_out_code(x_a, y_a, x_min, y_min, x_max, y_max);
    let mut outcode_1 = compute_out_code(x_b, y_b, x_min, y_min, x_max, y_max);
    loop {
        if (outcode_0 | outcode_1) == 0 {
            intersection_count = 2;
            break;
        }
        if (outcode_0 & outcode_1) != 0 {
            break;
        }

        let outcode_out = if outcode_0 != 0 { outcode_0 } else { outcode_1 };
        let mut x = 0.0;
        let mut y = 0.0;
        let mut normal_radian = 0.0;
        if (outcode_out & OUT_TOP) != 0 {
            x = x_a + (x_b - x_a) * (y_min - y_a) / (y_b - y_a);
            y = y_min;
            normal_radian = -PI * 0.5;
        } else if (outcode_out & OUT_BOTTOM) != 0 {
            x = x_a + (x_b - x_a) * (y_max - y_a) / (y_b - y_a);
            y = y_max;
            normal_radian = PI * 0.5;
        } else if (outcode_out & OUT_RIGHT) != 0 {
            y = y_a + (y_b - y_a) * (x_max - x_a) / (x_b - x_a);
            x = x_max;
            normal_radian = 0.0;
        } else if (outcode_out & OUT_LEFT) != 0 {
            y = y_a + (y_b - y_a) * (x_min - x_a) / (x_b - x_a);
            x = x_min;
            normal_radian = PI;
        }

        if outcode_out == outcode_0 {
            x_a = x;
            y_a = y;
            outcode_0 = compute_out_code(x_a, y_a, x_min, y_min, x_max, y_max);
            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                normal.x = normal_radian;
            }
        } else {
            x_b = x;
            y_b = y;
            outcode_1 = compute_out_code(x_b, y_b, x_min, y_min, x_max, y_max);
            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                normal.y = normal_radian;
            }
        }
    }

    if intersection_count != 0 {
        if in_side_a {
            intersection_count = 2;
            if let Some(point) = opt_point_mut(&mut point_a) {
                point.x = x_b;
                point.y = y_b;
            }
            if let Some(point) = opt_point_mut(&mut point_b) {
                point.x = x_b;
                // F6.11：官方 `y = xB`（不是 yB），照抄。
                point.y = x_b;
            }
            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                normal.x = normal.y + PI;
            }
        } else if in_side_b {
            intersection_count = 1;
            if let Some(point) = opt_point_mut(&mut point_a) {
                point.x = x_a;
                point.y = y_a;
            }
            if let Some(point) = opt_point_mut(&mut point_b) {
                point.x = x_a;
                point.y = y_a;
            }
            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                normal.y = normal.x + PI;
            }
        } else {
            intersection_count = 3;
            if let Some(point) = opt_point_mut(&mut point_a) {
                point.x = x_a;
                point.y = y_a;
            }
            if let Some(point) = opt_point_mut(&mut point_b) {
                point.x = x_b;
                point.y = y_b;
            }
        }
    }
    intersection_count
}

// ---------------------------------------------------------------------------
// Ellipse
// ---------------------------------------------------------------------------

/// 对齐 `EllipseBoundingBoxData.ellipseIntersectsSegment`（BoundingBoxData.ts:347-505）。
#[allow(clippy::too_many_arguments)]
fn ellipse_intersects_segment(
    mut x_a: f64,
    mut y_a: f64,
    mut x_b: f64,
    mut y_b: f64,
    x_c: f64,
    y_c: f64,
    width_h: f64,
    height_h: f64,
    mut point_a: Option<&mut Point>,
    mut point_b: Option<&mut Point>,
    mut normal_radians: Option<&mut Point>,
) -> i32 {
    let d = width_h / height_h;
    let dd = d * d;
    y_a *= d;
    y_b *= d;
    let d_x = x_b - x_a;
    let d_y = y_b - y_a;
    let l_ab = (d_x * d_x + d_y * d_y).sqrt();
    let x_d = d_x / l_ab;
    let y_d = d_y / l_ab;
    let a = (x_c - x_a) * x_d + (y_c - y_a) * y_d;
    let aa = a * a;
    let ee = x_a * x_a + y_a * y_a;
    let rr = width_h * width_h;
    let d_r = rr - ee + aa;
    let mut intersection_count = 0;
    if d_r >= 0.0 {
        let d_t = d_r.sqrt();
        let s_a = a - d_t;
        let s_b = a + d_t;
        let in_side_a = if s_a < 0.0 {
            -1
        } else if s_a <= l_ab {
            0
        } else {
            1
        };
        let in_side_b = if s_b < 0.0 {
            -1
        } else if s_b <= l_ab {
            0
        } else {
            1
        };
        let side_ab = in_side_a * in_side_b;
        if side_ab < 0 {
            return -1;
        } else if side_ab == 0 {
            if in_side_a == -1 {
                intersection_count = 2;
                x_b = x_a + s_b * x_d;
                y_b = (y_a + s_b * y_d) / d;
                if let Some(point) = opt_point_mut(&mut point_a) {
                    point.x = x_b;
                    point.y = y_b;
                }
                if let Some(point) = opt_point_mut(&mut point_b) {
                    point.x = x_b;
                    point.y = y_b;
                }
                if let Some(normal) = opt_point_mut(&mut normal_radians) {
                    normal.x = (y_b / rr * dd).atan2(x_b / rr);
                    normal.y = normal.x + PI;
                }
            } else if in_side_b == 1 {
                intersection_count = 1;
                x_a += s_a * x_d;
                y_a = (y_a + s_a * y_d) / d;
                if let Some(point) = opt_point_mut(&mut point_a) {
                    point.x = x_a;
                    point.y = y_a;
                }
                if let Some(point) = opt_point_mut(&mut point_b) {
                    point.x = x_a;
                    point.y = y_a;
                }
                if let Some(normal) = opt_point_mut(&mut normal_radians) {
                    normal.x = (y_a / rr * dd).atan2(x_a / rr);
                    normal.y = normal.x + PI;
                }
            } else {
                intersection_count = 3;
                if let Some(point) = opt_point_mut(&mut point_a) {
                    point.x = x_a + s_a * x_d;
                    point.y = (y_a + s_a * y_d) / d;
                    if let Some(normal) = opt_point_mut(&mut normal_radians) {
                        normal.x = (point.y / rr * dd).atan2(point.x / rr);
                    }
                }
                if let Some(point) = opt_point_mut(&mut point_b) {
                    point.x = x_a + s_b * x_d;
                    point.y = (y_a + s_b * y_d) / d;
                    if let Some(normal) = opt_point_mut(&mut normal_radians) {
                        normal.y = (point.y / rr * dd).atan2(point.x / rr);
                    }
                }
            }
        }
    }
    intersection_count
}

// ---------------------------------------------------------------------------
// Polygon
// ---------------------------------------------------------------------------

/// 对齐 `PolygonBoundingBoxData.polygonIntersectsSegment`（BoundingBoxData.ts:570-760）。
///
/// `point_b == None` 时官方走“首个交点即 break”的分支；此处用 `collect_all`
/// 复刻该差异（出参 B 是否提供会影响是否继续扫描）。
#[allow(clippy::too_many_arguments)]
fn polygon_intersects_segment(
    mut x_a: f64,
    mut y_a: f64,
    x_b: f64,
    y_b: f64,
    vertices: &[f64],
    mut point_a: Option<&mut Point>,
    mut point_b: Option<&mut Point>,
    mut normal_radians: Option<&mut Point>,
) -> i32 {
    if x_a == x_b {
        x_a = x_b + 0.000001;
    }
    if y_a == y_b {
        y_a = y_b + 0.000001;
    }
    let collect_all = point_b.is_some();
    let count = vertices.len();
    let d_xab = x_a - x_b;
    let d_yab = y_a - y_b;
    let ll_ab = x_a * y_b - y_a * x_b;
    let mut intersection_count = 0;
    let mut x_c = vertices[count - 2];
    let mut y_c = vertices[count - 1];
    let mut d_min = 0.0;
    let mut d_max = 0.0;
    let mut x_min = 0.0;
    let mut y_min = 0.0;
    let mut x_max = 0.0;
    let mut y_max = 0.0;

    let mut index = 0;
    while index < count {
        let x_d = vertices[index];
        let y_d = vertices[index + 1];
        if x_c == x_d {
            x_c = x_d + 0.0001;
        }
        if y_c == y_d {
            y_c = y_d + 0.0001;
        }
        let d_xcd = x_c - x_d;
        let d_ycd = y_c - y_d;
        let ll_cd = x_c * y_d - y_c * x_d;
        let ll = d_xab * d_ycd - d_yab * d_xcd;
        let x = (ll_ab * d_xcd - d_xab * ll_cd) / ll;
        if ((x >= x_c && x <= x_d) || (x >= x_d && x <= x_c))
            && (d_xab == 0.0 || (x >= x_a && x <= x_b) || (x >= x_b && x <= x_a))
        {
            let y = (ll_ab * d_ycd - d_yab * ll_cd) / ll;
            if ((y >= y_c && y <= y_d) || (y >= y_d && y <= y_c))
                && (d_yab == 0.0 || (y >= y_a && y <= y_b) || (y >= y_b && y <= y_a))
            {
                if collect_all {
                    let mut d = x - x_a;
                    if d < 0.0 {
                        d = -d;
                    }
                    if intersection_count == 0 {
                        d_min = d;
                        d_max = d;
                        x_min = x;
                        y_min = y;
                        x_max = x;
                        y_max = y;
                        if let Some(normal) = opt_point_mut(&mut normal_radians) {
                            normal.x = (y_d - y_c).atan2(x_d - x_c) - PI * 0.5;
                            normal.y = normal.x;
                        }
                    } else {
                        if d < d_min {
                            d_min = d;
                            x_min = x;
                            y_min = y;
                            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                                normal.x = (y_d - y_c).atan2(x_d - x_c) - PI * 0.5;
                            }
                        }
                        if d > d_max {
                            d_max = d;
                            x_max = x;
                            y_max = y;
                            if let Some(normal) = opt_point_mut(&mut normal_radians) {
                                normal.y = (y_d - y_c).atan2(x_d - x_c) - PI * 0.5;
                            }
                        }
                    }
                    intersection_count += 1;
                } else {
                    x_min = x;
                    y_min = y;
                    x_max = x;
                    y_max = y;
                    intersection_count += 1;
                    if let Some(normal) = opt_point_mut(&mut normal_radians) {
                        normal.x = (y_d - y_c).atan2(x_d - x_c) - PI * 0.5;
                        normal.y = normal.x;
                    }
                    break;
                }
            }
        }
        x_c = x_d;
        y_c = y_d;
        index += 2;
    }

    if intersection_count == 1 {
        if let Some(point) = opt_point_mut(&mut point_a) {
            point.x = x_min;
            point.y = y_min;
        }
        if let Some(point) = opt_point_mut(&mut point_b) {
            point.x = x_min;
            point.y = y_min;
        }
        if let Some(normal) = opt_point_mut(&mut normal_radians) {
            normal.y = normal.x + PI;
        }
    } else if intersection_count > 1 {
        intersection_count += 1;
        if let Some(point) = opt_point_mut(&mut point_a) {
            point.x = x_min;
            point.y = y_min;
        }
        if let Some(point) = opt_point_mut(&mut point_b) {
            point.x = x_max;
            point.y = y_max;
        }
    }
    intersection_count
}
