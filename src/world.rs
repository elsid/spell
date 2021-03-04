use serde::{Deserialize, Serialize};

use crate::rect::Rectf;
use crate::vec2::Vec2f;

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct World {
    pub revision: u64,
    pub settings: WorldSettings,
    pub bounds: Rectf,
    pub time: f64,
    pub id_counter: u64,
    pub actors: Vec<Actor>,
    pub dynamic_objects: Vec<DynamicObject>,
    pub static_objects: Vec<StaticObject>,
    pub beam_objects: Vec<BeamObject>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorldSettings {
    pub max_magic_power: f64,
    pub decay_factor: f64,
    pub margin: f64,
    pub damage_factor: f64,
    pub max_beam_length: f64,
    pub max_rotation_speed: f64,
    pub move_force: f64,
    pub magic_force_multiplier: f64,
    pub max_spell_elements: u8,
    pub max_beam_depth: u8,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            max_magic_power: 5.0,
            decay_factor: 1e-3,
            margin: 0.1,
            damage_factor: 1e-3,
            max_beam_length: 1e3,
            max_rotation_speed: 2.0 * std::f64::consts::PI,
            move_force: 5e4,
            magic_force_multiplier: 5e6,
            max_spell_elements: 5,
            max_beam_depth: 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Actor {
    pub id: u64,
    pub body: Body,
    pub position: Vec2f,
    pub health: f64,
    pub effect: Effect,
    pub aura: Aura,
    pub velocity: Vec2f,
    pub dynamic_force: Vec2f,
    pub current_direction: Vec2f,
    pub target_direction: Vec2f,
    pub spell_elements: Vec<Element>,
    pub moving: bool,
    pub delayed_magick: Option<DelayedMagick>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DynamicObject {
    pub id: u64,
    pub body: Body,
    pub position: Vec2f,
    pub health: f64,
    pub effect: Effect,
    pub aura: Aura,
    pub velocity: Vec2f,
    pub dynamic_force: Vec2f,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticObject {
    pub id: u64,
    pub body: Body,
    pub position: Vec2f,
    pub health: f64,
    pub effect: Effect,
    pub aura: Aura,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BeamObject {
    pub id: u64,
    pub beam: Beam,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Body {
    pub radius: f64,
    pub material: Material,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Magick {
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Beam {
    pub actor_id: u64,
    pub magick: Magick,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Effect {
    pub applied: [f64; 11],
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Aura {
    pub applied: f64,
    pub power: f64,
    pub radius: f64,
    pub elements: [bool; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DelayedMagick {
    pub actor_id: u64,
    pub started: f64,
    pub completed: bool,
    pub power: [f64; 11],
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize)]
pub enum Material {
    Flesh,
    Stone,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize)]
pub enum Element {
    Water,
    Lightning,
    Life,
    Arcane,
    Shield,
    Earth,
    Cold,
    Fire,
    Steam,
    Ice,
    Poison,
}

impl From<usize> for Element {
    fn from(value: usize) -> Self {
        match value {
            0 => Element::Water,
            1 => Element::Lightning,
            2 => Element::Life,
            3 => Element::Arcane,
            4 => Element::Shield,
            5 => Element::Earth,
            6 => Element::Cold,
            7 => Element::Fire,
            8 => Element::Steam,
            9 => Element::Ice,
            10 => Element::Poison,
            _ => unimplemented!(),
        }
    }
}
