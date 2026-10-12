#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorTransform {
    pub alpha_multiplier: f64,
    pub red_multiplier: f64,
    pub green_multiplier: f64,
    pub blue_multiplier: f64,
    pub alpha_offset: f64,
    pub red_offset: f64,
    pub green_offset: f64,
    pub blue_offset: f64,
}

impl ColorTransform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        alpha_multiplier: f64,
        red_multiplier: f64,
        green_multiplier: f64,
        blue_multiplier: f64,
        alpha_offset: f64,
        red_offset: f64,
        green_offset: f64,
        blue_offset: f64,
    ) -> Self {
        Self {
            alpha_multiplier,
            red_multiplier,
            green_multiplier,
            blue_multiplier,
            alpha_offset,
            red_offset,
            green_offset,
            blue_offset,
        }
    }

    pub fn copy_from(&mut self, other: ColorTransform) {
        self.alpha_multiplier = other.alpha_multiplier;
        self.red_multiplier = other.red_multiplier;
        self.green_multiplier = other.green_multiplier;
        self.blue_multiplier = other.blue_multiplier;
        self.alpha_offset = other.alpha_offset;
        self.red_offset = other.red_offset;
        self.green_offset = other.green_offset;
        self.blue_offset = other.blue_offset;
    }

    pub fn clear(&mut self) {
        self.alpha_multiplier = 1.0;
        self.red_multiplier = 1.0;
        self.green_multiplier = 1.0;
        self.blue_multiplier = 1.0;
        self.alpha_offset = 0.0;
        self.red_offset = 0.0;
        self.green_offset = 0.0;
        self.blue_offset = 0.0;
    }
}

impl Default for ColorTransform {
    fn default() -> Self {
        Self {
            alpha_multiplier: 1.0,
            red_multiplier: 1.0,
            green_multiplier: 1.0,
            blue_multiplier: 1.0,
            alpha_offset: 0.0,
            red_offset: 0.0,
            green_offset: 0.0,
            blue_offset: 0.0,
        }
    }
}
