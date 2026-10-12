use std::mem::swap;

use crate::geom::rectangle::Rectangle;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Matrix {
    pub fn new(a: f64, b: f64, c: f64, d: f64, tx: f64, ty: f64) -> Self {
        Self { a, b, c, d, tx, ty }
    }

    pub fn copy_from(&mut self, other: &Matrix) {
        self.a = other.a;
        self.b = other.b;
        self.c = other.c;
        self.d = other.d;
        self.tx = other.tx;
        self.ty = other.ty;
    }

    pub fn copy_from_array(&mut self, other: &[f64], offset: usize) {
        self.a = other[offset];
        self.b = other[offset + 1];
        self.c = other[offset + 2];
        self.d = other[offset + 3];
        self.tx = other[offset + 4];
        self.ty = other[offset + 5];
    }

    pub fn identity(&mut self) {
        self.a = 1.0;
        self.b = 0.0;
        self.c = 0.0;
        self.d = 1.0;
        self.tx = 0.0;
        self.ty = 0.0;
    }

    /// 将当前矩阵与另一个矩阵相乘
    pub fn concat(&mut self, other: &Matrix) -> &mut Self {
        let mut a1 = self.a * other.a;
        let mut b1 = 0.0;
        let mut c1 = 0.0;
        let mut d1 = self.d * other.d;
        let mut tx1 = self.tx * other.a + other.tx;
        let mut ty1 = self.ty * other.d + other.ty;

        if self.b != 0.0 || self.c != 0.0 {
            a1 += self.b * other.c;
            b1 += self.b * other.d;
            c1 += self.c * other.a;
            d1 += self.c * other.b;
        }

        if other.b != 0.0 || other.c != 0.0 {
            b1 += self.a * other.b;
            c1 += self.d * other.c;
            tx1 += self.ty * other.c;
            ty1 += self.tx * other.b;
        }

        self.a = a1;
        self.b = b1;
        self.c = c1;
        self.d = d1;
        self.tx = tx1;
        self.ty = ty1;

        self
    }

    /// 将当前矩阵转换为逆矩阵
    pub fn invert(&mut self) -> &mut Self {
        let mut a1 = self.a;
        let mut b1 = self.b;
        let mut c1 = self.c;
        let mut d1 = self.d;
        let tx1 = self.tx;
        let ty1 = self.ty;

        if b1 == 0.0 && c1 == 0.0 {
            self.b = 0.0;
            self.c = 0.0;
            if a1 == 0.0 || d1 == 0.0 {
                self.a = 0.0;
                self.b = 0.0;
                self.tx = 0.0;
                self.ty = 0.0;
            } else {
                let i_a1 = 1.0 / a1;
                let i_d1 = 1.0 / d1;
                a1 = i_a1;
                self.a = i_a1;
                d1 = i_d1;
                self.d = i_d1;
                self.tx = -a1 * tx1;
                self.ty = -d1 * ty1;
            }
            return self;
        }

        let determinant = a1 * d1 - b1 * c1;
        if determinant == 0.0 {
            self.identity();
            return self;
        }

        let determinant = 1.0 / determinant;
        let k = d1 * determinant;
        self.a = k;

        b1 = -b1 * determinant;
        self.b = b1;

        c1 = -c1 * determinant;
        self.c = c1;

        d1 = a1 * determinant;
        self.d = d1;

        self.tx = -(k * tx1 + c1 * ty1);
        self.ty = -(b1 * tx1 + d1 * ty1);

        self
    }

    pub fn transform_point(&self, x: f64, y: f64, delta: bool) -> (f64, f64) {
        let mut x1 = self.a * x + self.c * y;
        let mut y1 = self.b * x + self.d * y;

        if !delta {
            x1 += self.tx;
            y1 += self.ty;
        }

        (x1, y1)
    }

    pub fn transform_rectangle(&self, rect: &mut Rectangle, delta: bool) {
        let (a, b, c, d) = (self.a, self.b, self.c, self.d);
        let (tx, ty) = if delta {
            (0.0, 0.0)
        } else {
            (self.tx, self.ty)
        };

        let (x, y) = (rect.x, rect.y);
        let (x_max, y_max) = (x + rect.width, y + rect.height);

        let mut x0 = a * x + c * y + tx;
        let mut y0 = b * x + d * y + ty;
        let mut x1 = a * x_max + c * y + tx;
        let mut y1 = b * x_max + d * y + ty;
        let mut x2 = a * x_max + c * y_max + tx;
        let mut y2 = b * x_max + d * y_max + ty;
        let mut x3 = a * x + c * y_max + tx;
        let mut y3 = b * x + d * y_max + ty;

        if x0 > x1 {
            swap(&mut x0, &mut x1);
        }
        if x2 > x3 {
            swap(&mut x2, &mut x3);
        }

        rect.x = f64::floor(if x0 < x2 { x0 } else { x2 });
        rect.width = f64::ceil(if x1 > x3 { x1 } else { x3 } - rect.x);

        if y0 > y1 {
            swap(&mut y0, &mut y1);
        }
        if y2 > y3 {
            swap(&mut y2, &mut y3);
        }

        rect.y = f64::floor(if y0 < y2 { y0 } else { y2 });
        rect.height = f64::ceil(if y1 > y3 { y1 } else { y3 } - rect.y);
    }
}

impl Default for Matrix {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}
