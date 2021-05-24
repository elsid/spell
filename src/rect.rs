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
}

impl PartialEq for Rectf {
    fn eq(&self, rhs: &Self) -> bool {
        (self.min, self.max).eq(&(rhs.min, rhs.max))
    }
}

impl Eq for Rectf {}
