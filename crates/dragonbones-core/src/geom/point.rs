#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn copy_from(&mut self, other: Point) {
        self.x = other.x;
        self.y = other.y;
    }

    pub fn clear(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
    }
}
