use serde::{Deserialize, Serialize};

use crate::vec2::Vec2f;

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialOrd)]
pub struct Rectf {
    pub min: Vec2f,
    pub max: Vec2f,
}

impl Rectf {
    pub fn new(min: Vec2f, max: Vec2f) -> Self {
        Self { min, max }
    }

    pub fn overlaps(&self, other: &Rectf) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
    }
}

impl PartialEq for Rectf {
    fn eq(&self, rhs: &Self) -> bool {
        (self.min, self.min).eq(&(rhs.max, rhs.max))
    }
}

impl Eq for Rectf {}
