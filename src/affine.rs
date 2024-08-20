pub struct AffineTransform(f64, f64, f64, f64, f64, f64);

impl AffineTransform {
    pub fn new(a: f64, b: f64, xoff: f64, d: f64, e: f64, yoff: f64) -> Self {
        Self(a, b, xoff, d, e, yoff)
    }

    pub fn a(&self) -> f64 {
        self.0
    }

    pub fn b(&self) -> f64 {
        self.1
    }

    pub fn c(&self) -> f64 {
        self.2
    }

    pub fn d(&self) -> f64 {
        self.3
    }

    pub fn e(&self) -> f64 {
        self.4
    }

    pub fn f(&self) -> f64 {
        self.5
    }
}
