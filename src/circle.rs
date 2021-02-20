use crate::segment::Segment;
use crate::vec2::{Square, Vec2f};

#[derive(Default, Clone, Copy, Debug, PartialOrd)]
pub struct Circle {
    pub center: Vec2f,
    pub radius: f64,
}

impl Circle {
    pub fn new(center: Vec2f, radius: f64) -> Self {
        Circle { center, radius }
    }

    pub fn get_first_intersection_with_segment(&self, segment: &Segment) -> Option<Vec2f> {
        let is_begin_inside_circle = segment.begin.distance(self.center) - self.radius < -f32::EPSILON as f64;
        if is_begin_inside_circle && segment.end.distance(self.center) - self.radius < -f32::EPSILON as f64 {
            return None;
        }
        let intersection = if is_begin_inside_circle {
            self.get_first_intersection_with_line(&Segment::new(segment.end, segment.begin))
        } else {
            self.get_first_intersection_with_line(&segment)
        };
        if let Some(point) = intersection {
            if segment.has_point(point) {
                Some(point)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn get_first_intersection_with_line(&self, line: &Segment) -> Option<Vec2f> {
        if line.begin == line.end {
            return None;
        }
        let nearest = line.nearest(self.center);
        let far_cathetus = self.center.distance(nearest);
        if (far_cathetus - self.radius).abs() <= f32::EPSILON as f64 {
            return Some(nearest);
        }
        if far_cathetus > self.radius {
            return None;
        }
        let near_cathetus = if far_cathetus == 0.0 {
            self.radius
        } else {
            (self.radius.square() - far_cathetus.square()).sqrt()
        };
        let path = if line.begin == nearest {
            (nearest - line.end) * 2.0
        } else {
            nearest - line.begin
        };
        let length = path.norm() - near_cathetus;
        let end = path.normalized() * length;
        Some(line.begin + end)
    }
}

impl PartialEq for Circle {
    fn eq(&self, other: &Circle) -> bool {
        (self.center, self.radius).eq(&(other.center, other.radius))
    }
}

impl Eq for Circle {}
