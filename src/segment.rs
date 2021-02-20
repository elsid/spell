use crate::vec2::Vec2f;

#[derive(Debug, Clone)]
pub struct Segment {
    pub begin: Vec2f,
    pub end: Vec2f,
}

impl Segment {
    pub fn new(begin: Vec2f, end: Vec2f) -> Self {
        Segment { begin, end }
    }

    pub fn has_point(&self, point: Vec2f) -> bool {
        let to_end = self.end - point;
        if to_end.dot_self() == 0.0 {
            return true;
        }
        let to_begin = self.begin - point;
        if to_begin.dot_self() == 0.0 {
            return true;
        }
        return (1.0 + to_begin.cos(to_end)).abs() <= f32::EPSILON as f64;
    }

    pub fn nearest(&self, point: Vec2f) -> Vec2f {
        let to_end = self.end - self.begin;
        let to_end_squared_norm = to_end.dot_self();
        if to_end_squared_norm == 0.0 {
            return self.begin;
        }
        let to_point = point - self.begin;
        self.begin + to_end * to_point.dot(to_end) / to_end_squared_norm
    }
}
