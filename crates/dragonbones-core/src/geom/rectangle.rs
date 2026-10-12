#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Rectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rectangle {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn copy_from(&mut self, other: Rectangle) {
        self.x = other.x;
        self.y = other.y;
        self.width = other.width;
        self.height = other.height;
    }

    pub fn clear(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
        self.width = 0.0;
        self.height = 0.0;
    }
}
