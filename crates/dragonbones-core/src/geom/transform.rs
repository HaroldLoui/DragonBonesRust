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
        self.rotation = f64::atan(m.b / m.a);
        let mut skew_x = f64::atan(-m.c / m.d);

        self.scale_x = if self.rotation > -PI_Q && self.rotation < PI_Q {
            m.a / f64::cos(self.rotation)
        } else {
            m.b / f64::sin(self.rotation)
        };
        self.scale_y = if skew_x > -PI_Q && skew_x < PI_Q {
            m.d / f64::cos(skew_x)
        } else {
            -m.c / f64::sin(skew_x)
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
            m.a = f64::cos(self.rotation);
            m.b = f64::sin(self.rotation);
        }

        if self.skew == 0.0 {
            m.c = -m.b;
            m.d = m.a;
        } else {
            m.c = -f64::sin(self.skew + self.rotation);
            m.d = f64::cos(self.skew + self.rotation);
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
