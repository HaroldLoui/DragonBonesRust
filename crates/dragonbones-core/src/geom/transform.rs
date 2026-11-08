use std::ops::{AddAssign, SubAssign};

use crate::geom::matrix::Matrix;

pub const PI: f64 = std::f64::consts::PI;
pub const PI_D: f64 = std::f64::consts::TAU;
pub const PI_H: f64 = std::f64::consts::FRAC_PI_2;
pub const PI_Q: f64 = std::f64::consts::FRAC_PI_4;
pub const RAD_DEG: f64 = 180.0 / std::f64::consts::PI;
pub const DEG_RAD: f64 = std::f64::consts::PI / 180.0;

pub fn normalize_radian(value: f64) -> f64 {
    let mut value = (value + PI) % PI_D;
    value += if value > 0.0 { -PI } else { PI };

    value
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub skew: f64,
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
}

impl Transform {
    pub fn new(x: f64, y: f64, skew: f64, rotation: f64, scale_x: f64, scale_y: f64) -> Self {
        Self {
            x,
            y,
            skew,
            rotation,
            scale_x,
            scale_y,
        }
    }

    pub fn copy_from(&mut self, other: &Transform) {
        self.x = other.x;
        self.y = other.y;
        self.skew = other.skew;
        self.rotation = other.rotation;
        self.scale_x = other.scale_x;
        self.scale_y = other.scale_y;
    }

    pub fn identity(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
        self.skew = 0.0;
        self.rotation = 0.0;
        self.scale_x = 1.0;
        self.scale_y = 1.0;
    }

    pub fn from_matrix(&mut self, m: &Matrix) {
        let backup_scale_x = self.scale_x;
        let backup_scale_y = self.scale_y;

        self.x = m.tx;
        self.y = m.ty;

        // 退化矩阵保护（有意偏离 TS `Transform.fromMatrix`）：当
        // `a*d - b*c == 0` 时基向量线性相关，`atan(-c/d)` 在 `c == d == 0`
        // （典型来源：骨骼 scaleY 过零后的 IK 约束重算）处得到 `NaN`，
        // 随后 `toMatrix` 会把 NaN 写回矩阵并污染整个全局变换。TS 在
        // 该数据下同样产生 NaN（`Math.atan(0 / 0)`）；本库选择确定性回退：
        // 保留矩阵的 a/b 列决定 rotation/scaleX，令 scaleY = 0、skew = 0，
        // 保证 `toMatrix` 回写后矩阵有限。详见 docs/model_mapping.md 偏差表。
        if m.a * m.d - m.b * m.c == 0.0 {
            self.rotation = f64::atan2(m.b, m.a);
            self.scale_x = f64::hypot(m.a, m.b);
            self.scale_y = 0.0;
            self.skew = 0.0;
            return;
        }
        self.rotation = f64::atan(m.b / m.a);
        let mut skew_x = f64::atan(-m.c / m.d);

        // 一次 `sin_cos` 同时取正余弦（§2.5 三角函数优化）：与分开调用
        // `sin`/`cos` 数值等价（glibc/MSVC 的 sincos 与独立调用结果一致），
        // 但省掉一次角参数规约（range reduction）。
        let (sin_rotation, cos_rotation) = self.rotation.sin_cos();
        self.scale_x = if self.rotation > -PI_Q && self.rotation < PI_Q {
            m.a / cos_rotation
        } else {
            m.b / sin_rotation
        };
        let (sin_skew, cos_skew) = skew_x.sin_cos();
        self.scale_y = if skew_x > -PI_Q && skew_x < PI_Q {
            m.d / cos_skew
        } else {
            -m.c / sin_skew
        };

        if backup_scale_x >= 0.0 && self.scale_x < 0.0 {
            self.scale_x = -self.scale_x;
            self.rotation -= PI;
        }

        if backup_scale_y >= 0.0 && self.scale_y < 0.0 {
            self.scale_y = -self.scale_y;
            skew_x -= PI;
        }

        self.skew = skew_x - self.rotation;
    }

    pub fn to_matrix(&self, m: &mut Matrix) {
        if self.rotation == 0.0 {
            m.a = 1.0;
            m.b = 0.0;
        } else {
            let (sin_rotation, cos_rotation) = self.rotation.sin_cos();
            m.a = cos_rotation;
            m.b = sin_rotation;
        }

        if self.skew == 0.0 {
            m.c = -m.b;
            m.d = m.a;
        } else {
            let (sin_skew, cos_skew) = (self.skew + self.rotation).sin_cos();
            m.c = -sin_skew;
            m.d = cos_skew;
        }

        if self.scale_x != 1.0 {
            m.a *= self.scale_x;
            m.b *= self.scale_x;
        }

        if self.scale_y != 1.0 {
            m.c *= self.scale_y;
            m.d *= self.scale_y;
        }

        m.tx = self.x;
        m.ty = self.y;
    }
}

impl AddAssign<Transform> for Transform {
    fn add_assign(&mut self, rhs: Transform) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.skew += rhs.skew;
        self.rotation += rhs.rotation;
        self.scale_x *= rhs.scale_x;
        self.scale_y *= rhs.scale_y;
    }
}

impl SubAssign<Transform> for Transform {
    fn sub_assign(&mut self, rhs: Transform) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.skew -= rhs.skew;
        self.rotation -= rhs.rotation;
        self.scale_x /= rhs.scale_x;
        self.scale_y /= rhs.scale_y;
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            skew: 0.0,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}
