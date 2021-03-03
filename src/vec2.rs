use serde::{Deserialize, Serialize};

pub trait Square: std::ops::Mul + Copy {
    fn square(self) -> Self::Output {
        self * self
    }
}

impl Square for f64 {}

#[derive(Default, Clone, Copy, Debug, Deserialize, Serialize, PartialOrd)]
pub struct Vec2f {
    pub x: f64,
    pub y: f64,
}

impl Vec2f {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const I: Self = Self { x: 1.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub const fn both(value: f64) -> Self {
        Self { x: value, y: value }
    }

    pub const fn only_x(x: f64) -> Self {
        Self { x, y: 0.0 }
    }

    pub fn norm(&self) -> f64 {
        (self.x.square() + self.y.square()).sqrt()
    }

    pub fn normalize(&mut self) {
        *self /= self.norm()
    }

    pub fn normalized(&self) -> Self {
        let mut result = *self;
        result.normalize();
        result
    }

    pub fn rotated(&self, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self { x: self.x * cos - self.y * sin, y: self.y * cos + self.x * sin }
    }

    pub fn cos(&self, other: Self) -> f64 {
        (self.dot(other) / (self.norm() * other.norm())).clamp(-1.0, 1.0)
    }

    pub fn dot(&self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn dot_self(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn angle(&self) -> f64 {
        self.y.atan2(self.x)
    }

    pub fn distance(&self, other: Self) -> f64 {
        (other - *self).norm()
    }
}

impl std::ops::Add for Vec2f {
    type Output = Vec2f;

    fn add(self, rhs: Self) -> Self::Output {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl std::ops::Sub for Vec2f {
    type Output = Vec2f;

    fn sub(self, rhs: Self) -> Self::Output {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl std::ops::Mul<f64> for Vec2f {
    type Output = Vec2f;

    fn mul(self, rhs: f64) -> Self::Output {
        Self { x: self.x * rhs, y: self.y * rhs }
    }
}

impl std::ops::Div<f64> for Vec2f {
    type Output = Vec2f;

    fn div(self, rhs: f64) -> Self::Output {
        Self { x: self.x / rhs, y: self.y / rhs }
    }
}

impl std::ops::Neg for Vec2f {
    type Output = Vec2f;

    fn neg(self) -> Self::Output {
        Self { x: -self.x, y: -self.y }
    }
}

impl std::ops::AddAssign for Vec2f {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl std::ops::SubAssign for Vec2f {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl std::ops::MulAssign<f64> for Vec2f {
    fn mul_assign(&mut self, other: f64) {
        self.x *= other;
        self.y *= other;
    }
}

impl std::ops::DivAssign<f64> for Vec2f {
    fn div_assign(&mut self, other: f64) {
        self.x /= other;
        self.y /= other;
    }
}

impl PartialEq for Vec2f {
    fn eq(&self, rhs: &Self) -> bool {
        (self.x, self.y).eq(&(rhs.x, rhs.y))
    }
}

impl Eq for Vec2f {}

impl Into<[f64; 2]> for Vec2f {
    fn into(self) -> [f64; 2] {
        [self.x, self.y]
    }
}
