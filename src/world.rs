use itertools::Itertools;

use crate::circle::Circle;
use crate::rect::Rectf;
use crate::segment::Segment;
use crate::vec2::{Square, Vec2f};

pub const MAX_MAGIC_POWER: f64 = 5.0;
pub const DECAY_FACTOR: f64 = 0.001;
pub const HEALTH_FACTOR: f64 = 1000.0;
pub const SHIFT_FACTOR: f64 = 1.25;
pub const DAMAGE_FACTOR: f64 = 0.1;
pub const MAX_BEAM_LENGTH: f64 = 1e3;
pub const MAX_ROTATION_SPEED: f64 = 2.0 * std::f64::consts::PI;
pub const CONST_FORCE_MULTIPLIER: f64 = 2e3;
pub const MAGIC_FORCE_MULTIPLIER: f64 = 5e6;
pub const MAX_SPELL_ELEMENTS: usize = 5;
pub const MAX_BEAM_DEPTH: usize = 4;

pub trait WithId {
    fn ids(&self) -> &Vec<u64>;

    fn get_id(&self, index: usize) -> u64 {
        self.ids()[index]
    }

    fn get_index(&self, id: u64) -> usize {
        self.find_index(id).unwrap()
    }

    fn find_index(&self, id: u64) -> Option<usize> {
        self.ids().iter().find_position(|v| **v == id).map(|(i, _)| i)
    }
}

macro_rules! with_id_impl {
    ($type: ty) => {
        impl WithId for $type {
            fn ids(&self) -> &Vec<u64> {
                &self.ids
            }
        }
    }
}

pub trait WithBody {
    fn get_body(&self, index: usize) -> &Body;
}

macro_rules! with_body_impl {
    ($type: ty) => {
        impl WithBody for $type {
            fn get_body(&self, index: usize) -> &Body {
                &self.bodies[index]
            }
        }
    }
}

pub trait WithPosition {
    fn get_position(&self, index: usize) -> Vec2f;
}

macro_rules! with_position_impl {
    ($type: ty) => {
        impl WithPosition for $type {
            fn get_position(&self, index: usize) -> Vec2f {
                self.positions[index]
            }
        }
    }
}

pub trait WithEffect {
    fn get_effect(&self, index: usize) -> &Effect;
}

macro_rules! with_effect_impl {
    ($type: ty) => {
        impl WithEffect for $type {
            fn get_effect(&self, index: usize) -> &Effect {
                &self.effects[index]
            }
        }
    }
}

pub trait WithAura {
    fn get_aura(&self, index: usize) -> &Aura;
}

macro_rules! with_aura_impl {
    ($type: ty) => {
        impl WithAura for $type {
            fn get_aura(&self, index: usize) -> &Aura {
                &self.auras[index]
            }
        }
    }
}

pub trait WithHealth {
    fn get_health(&self, index: usize) -> f64;
}

macro_rules! with_health_impl {
    ($type: ty) => {
        impl WithHealth for $type {
            fn get_health(&self, index: usize) -> f64 {
                self.healths[index]
            }
        }
    }
}

pub trait WithActivity {
    fn is_active(&self, index: usize) -> bool;
}

macro_rules! with_activity_impl {
    ($type: ty) => {
        impl WithActivity for $type {
            fn is_active(&self, index: usize) -> bool {
                self.active[index]
            }
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum Material {
    Flesh,
    Stone,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
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

#[derive(Debug, Clone)]
pub struct Body {
    pub radius: f64,
    pub mass: f64,
    pub restitution: f64,
    pub material: Material,
}

#[derive(Debug, Clone, Default)]
pub struct Spell {
    elements: Vec<Element>,
}

impl Spell {
    pub fn elements(&self) -> &Vec<Element> {
        &self.elements
    }

    pub fn add(&mut self, element: Element) {
        for i in 1..self.elements.len() + 1 {
            let target = self.elements.len() - i;
            if let Some(combination) = combine_elements(self.elements[target], element) {
                self.elements[target] = combination;
                return;
            } else if can_cancel_element(self.elements[target], element) {
                self.elements.remove(target);
                return;
            }
        }
        if self.elements.len() < MAX_SPELL_ELEMENTS {
            self.elements.push(element);
        }
    }

    pub fn cast(&mut self) -> Magick {
        let mut power: [f64; 11] = Default::default();
        for element in self.elements.iter() {
            power[*element as usize] += 1.0;
        }
        self.elements.clear();
        Magick { power }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Magick {
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Default)]
pub struct DelayedMagick {
    pub actor_id: u64,
    pub started: f64,
    pub power: [f64; 11],
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum Id {
    Actor(u64),
    Beam,
}

#[derive(Debug, Copy, Clone)]
pub enum Index {
    Actor(usize),
    DynamicBody(usize),
    StaticBody(usize),
}

#[derive(Debug, Clone)]
pub struct Beam {
    pub source: Id,
    pub magick: Magick,
}

#[derive(Debug, Clone)]
pub struct Collision {
    pub lhs: Index,
    pub rhs: Index,
    pub lhs_physical_damage: f64,
    pub rhs_physical_damage: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Effect {
    pub applied: [f64; 11],
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Default)]
pub struct Aura {
    pub applied: f64,
    pub power: f64,
    pub radius: f64,
    pub elements: [bool; 11],
}

#[derive(Debug, Clone, Default)]
pub struct IdCounter {
    next: u64,
}

impl IdCounter {
    pub fn next(&mut self) -> u64 {
        let result = self.next;
        self.next += 1;
        result
    }
}

#[derive(Debug, Default)]
pub struct ReflectedBeam {
    pub begin: Vec2f,
    pub direction: Vec2f,
    pub depth: usize,
}

#[derive(Default)]
pub struct Actors {
    ids: Vec<u64>,
    bodies: Vec<Body>,
    positions: Vec<Vec2f>,
    current_directions: Vec<Vec2f>,
    target_directions: Vec<Vec2f>,
    velocities: Vec<Vec2f>,
    const_forces: Vec<f64>,
    dynamic_forces: Vec<Vec2f>,
    effects: Vec<Effect>,
    auras: Vec<Aura>,
    healths: Vec<f64>,
    spells: Vec<Spell>,
    active: Vec<bool>,
}

impl Actors {
    pub fn get_position(&self, index: usize) -> Vec2f {
        self.positions[index]
    }

    pub fn get_current_direction(&self, index: usize) -> Vec2f {
        self.current_directions[index]
    }

    pub fn get_spell(&self, index: usize) -> &Spell {
        &self.spells[index]
    }

    pub fn add_spell_element(&mut self, index: usize, element: Element) {
        self.spells[index].add(element);
    }

    pub fn add(&mut self, id: u64, value: Body) -> usize {
        self.ids.push(id);
        self.healths.push(value.mass * HEALTH_FACTOR);
        self.bodies.push(value);
        self.positions.push(Vec2f::ZERO);
        self.current_directions.push(Vec2f::I);
        self.target_directions.push(Vec2f::I);
        self.velocities.push(Vec2f::ZERO);
        self.const_forces.push(0.0);
        self.dynamic_forces.push(Vec2f::ZERO);
        self.effects.push(Effect::default());
        self.auras.push(Aura::default());
        self.spells.push(Spell::default());
        self.active.push(true);
        self.ids.len() - 1
    }

    pub fn remove(&mut self, index: usize) {
        self.active.remove(index);
        self.spells.remove(index);
        self.healths.remove(index);
        self.auras.remove(index);
        self.effects.remove(index);
        self.dynamic_forces.remove(index);
        self.const_forces.remove(index);
        self.velocities.remove(index);
        self.target_directions.remove(index);
        self.current_directions.remove(index);
        self.positions.remove(index);
        self.bodies.remove(index);
        self.ids.remove(index);
    }

    fn retain(&mut self) {
        let len = self.ids.len();
        let mut del = 0;
        {
            let ids = &mut *self.ids;
            let bodies = &mut *self.bodies;
            let positions = &mut *self.positions;
            let current_directions = &mut *self.current_directions;
            let target_directions = &mut *self.target_directions;
            let velocities = &mut *self.velocities;
            let const_forces = &mut *self.const_forces;
            let dynamic_forces = &mut *self.dynamic_forces;
            let effects = &mut *self.effects;
            let auras = &mut *self.auras;
            let healths = &mut *self.healths;
            let spells = &mut *self.spells;
            let active = &mut *self.active;
            for i in 0..len {
                if !active[i] {
                    del += 1;
                } else if del > 0 {
                    ids.swap(i - del, i);
                    bodies.swap(i - del, i);
                    positions.swap(i - del, i);
                    current_directions.swap(i - del, i);
                    target_directions.swap(i - del, i);
                    velocities.swap(i - del, i);
                    const_forces.swap(i - del, i);
                    dynamic_forces.swap(i - del, i);
                    effects.swap(i - del, i);
                    auras.swap(i - del, i);
                    healths.swap(i - del, i);
                    spells.swap(i - del, i);
                    active.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.ids.truncate(len - del);
            self.bodies.truncate(len - del);
            self.positions.truncate(len - del);
            self.current_directions.truncate(len - del);
            self.target_directions.truncate(len - del);
            self.velocities.truncate(len - del);
            self.const_forces.truncate(len - del);
            self.dynamic_forces.truncate(len - del);
            self.effects.truncate(len - del);
            self.auras.truncate(len - del);
            self.healths.truncate(len - del);
            self.spells.truncate(len - del);
            self.active.truncate(len - del);
        }
    }
}

with_id_impl!(Actors);
with_body_impl!(Actors);
with_position_impl!(Actors);
with_effect_impl!(Actors);
with_aura_impl!(Actors);
with_health_impl!(Actors);
with_activity_impl!(Actors);

#[derive(Default)]
pub struct DynamicBodies {
    ids: Vec<u64>,
    bodies: Vec<Body>,
    positions: Vec<Vec2f>,
    velocities: Vec<Vec2f>,
    dynamic_forces: Vec<Vec2f>,
    effects: Vec<Effect>,
    auras: Vec<Aura>,
    healths: Vec<f64>,
    active: Vec<bool>,
}

impl DynamicBodies {
    pub fn add(&mut self, id: u64, value: Body) -> usize {
        self.ids.push(id);
        self.healths.push(value.mass * HEALTH_FACTOR);
        self.bodies.push(value);
        self.positions.push(Vec2f::ZERO);
        self.velocities.push(Vec2f::ZERO);
        self.dynamic_forces.push(Vec2f::ZERO);
        self.effects.push(Effect::default());
        self.auras.push(Aura::default());
        self.active.push(true);
        self.ids.len() - 1
    }

    pub fn remove(&mut self, index: usize) {
        self.active.remove(index);
        self.healths.remove(index);
        self.auras.remove(index);
        self.effects.remove(index);
        self.dynamic_forces.remove(index);
        self.velocities.remove(index);
        self.positions.remove(index);
        self.bodies.remove(index);
        self.ids.remove(index);
    }

    fn retain(&mut self) {
        let len = self.ids.len();
        let mut del = 0;
        {
            let ids = &mut *self.ids;
            let bodies = &mut *self.bodies;
            let positions = &mut *self.positions;
            let velocities = &mut *self.velocities;
            let dynamic_forces = &mut *self.dynamic_forces;
            let effects = &mut *self.effects;
            let auras = &mut *self.auras;
            let healths = &mut *self.healths;
            let active = &mut *self.active;
            for i in 0..len {
                if !active[i] {
                    del += 1;
                } else if del > 0 {
                    ids.swap(i - del, i);
                    bodies.swap(i - del, i);
                    positions.swap(i - del, i);
                    velocities.swap(i - del, i);
                    dynamic_forces.swap(i - del, i);
                    effects.swap(i - del, i);
                    auras.swap(i - del, i);
                    healths.swap(i - del, i);
                    active.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.ids.truncate(len - del);
            self.bodies.truncate(len - del);
            self.positions.truncate(len - del);
            self.velocities.truncate(len - del);
            self.dynamic_forces.truncate(len - del);
            self.effects.truncate(len - del);
            self.auras.truncate(len - del);
            self.healths.truncate(len - del);
            self.active.truncate(len - del);
        }
    }
}

with_id_impl!(DynamicBodies);
with_body_impl!(DynamicBodies);
with_position_impl!(DynamicBodies);
with_effect_impl!(DynamicBodies);
with_aura_impl!(DynamicBodies);
with_health_impl!(DynamicBodies);
with_activity_impl!(DynamicBodies);

#[derive(Default)]
pub struct StaticBodies {
    ids: Vec<u64>,
    bodies: Vec<Body>,
    positions: Vec<Vec2f>,
    effects: Vec<Effect>,
    auras: Vec<Aura>,
    healths: Vec<f64>,
    active: Vec<bool>,
}

impl StaticBodies {
    pub fn add(&mut self, id: u64, value: Body) -> usize {
        self.ids.push(id);
        self.healths.push(value.mass * HEALTH_FACTOR);
        self.bodies.push(value);
        self.positions.push(Vec2f::ZERO);
        self.effects.push(Effect::default());
        self.auras.push(Aura::default());
        self.active.push(true);
        self.ids.len() - 1
    }

    pub fn remove(&mut self, index: usize) {
        self.active.remove(index);
        self.healths.remove(index);
        self.auras.remove(index);
        self.effects.remove(index);
        self.positions.remove(index);
        self.bodies.remove(index);
        self.ids.remove(index);
    }

    fn retain(&mut self) {
        let len = self.ids.len();
        let mut del = 0;
        {
            let ids = &mut *self.ids;
            let bodies = &mut *self.bodies;
            let positions = &mut *self.positions;
            let effects = &mut *self.effects;
            let auras = &mut *self.auras;
            let healths = &mut *self.healths;
            let active = &mut *self.active;
            for i in 0..len {
                if !active[i] {
                    del += 1;
                } else if del > 0 {
                    ids.swap(i - del, i);
                    bodies.swap(i - del, i);
                    positions.swap(i - del, i);
                    effects.swap(i - del, i);
                    auras.swap(i - del, i);
                    healths.swap(i - del, i);
                    active.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.ids.truncate(len - del);
            self.bodies.truncate(len - del);
            self.positions.truncate(len - del);
            self.effects.truncate(len - del);
            self.auras.truncate(len - del);
            self.healths.truncate(len - del);
            self.active.truncate(len - del);
        }
    }
}

with_id_impl!(StaticBodies);
with_body_impl!(StaticBodies);
with_position_impl!(StaticBodies);
with_effect_impl!(StaticBodies);
with_aura_impl!(StaticBodies);
with_health_impl!(StaticBodies);
with_activity_impl!(StaticBodies);

#[derive(Default)]
pub struct Beams {
    ids: Vec<u64>,
    values: Vec<Beam>,
    lengths: Vec<f64>,
    reflected_beams: Vec<ReflectedBeam>,
    reflected_shift: usize,
}

impl Beams {
    pub fn get_value(&self, index: usize) -> &Beam {
        &self.values[index]
    }

    pub fn get_length(&self, index: usize) -> f64 {
        self.lengths[index]
    }

    pub fn get_reflected_beam(&self, index: usize) -> &ReflectedBeam {
        &self.reflected_beams[index - self.reflected_shift]
    }

    pub fn find_by_actor_id(&self, actor_id: u64) -> Option<usize> {
        self.values.iter()
            .find_position(|beam| beam.source == Id::Actor(actor_id))
            .map(|(i, _)| i)
    }

    pub fn add(&mut self, id: u64, value: Beam) -> usize {
        self.ids.push(id);
        self.values.push(value);
        self.lengths.push(MAX_BEAM_LENGTH);
        self.ids.len() - 1
    }

    pub fn add_reflected(&mut self, id: u64, value: Beam, temp_beam: ReflectedBeam) -> usize {
        self.reflected_beams.push(temp_beam);
        self.add(id, value)
    }

    pub fn remove(&mut self, index: usize) {
        self.ids.remove(index);
        self.values.remove(index);
        self.lengths.remove(index);
        self.reflected_shift -= 1;
    }

    fn retain(&mut self, actor_ids: &Vec<u64>) {
        let len = self.values.len();
        let mut del = 0;
        {
            let ids = &mut *self.ids;
            let values = &mut *self.values;
            let lengths = &mut *self.lengths;
            for i in 0..len {
                match values[i].source {
                    Id::Actor(actor_id) => if !actor_ids.contains(&actor_id) {
                        del += 1;
                    } else if del > 0 {
                        ids.swap(i - del, i);
                        values.swap(i - del, i);
                        lengths.swap(i - del, i);
                    }
                    Id::Beam => del += 1,
                }
            }
        }
        if del > 0 {
            self.ids.truncate(len - del);
            self.values.truncate(len - del);
            self.lengths.truncate(len - del);
        }
    }
}

with_id_impl!(Beams);

pub struct World {
    bounds: Rectf,
    id_counter: IdCounter,
    now: f64,
    actors: Actors,
    dynamic_bodies: DynamicBodies,
    static_bodies: StaticBodies,
    beams: Beams,
    delayed_magics: Vec<DelayedMagick>,
}

impl World {
    pub fn new(bounds: Rectf) -> Self {
        World {
            bounds,
            id_counter: IdCounter::default(),
            now: 0.0,
            actors: Actors::default(),
            dynamic_bodies: DynamicBodies::default(),
            static_bodies: StaticBodies::default(),
            beams: Beams::default(),
            delayed_magics: Vec::new(),
        }
    }

    pub fn bounds(&self) -> &Rectf {
        &self.bounds
    }

    pub fn actors(&self) -> &Actors {
        &self.actors
    }

    pub fn dynamic_bodies(&self) -> &DynamicBodies {
        &self.dynamic_bodies
    }

    pub fn static_bodies(&self) -> &StaticBodies {
        &self.static_bodies
    }

    pub fn beams(&self) -> &Beams {
        &self.beams
    }

    pub fn set_actor_target_direction(&mut self, index: usize, value: Vec2f) {
        self.actors.target_directions[index] = value;
    }

    pub fn set_actor_const_force(&mut self, index: usize, value: f64) {
        self.actors.const_forces[index] = value;
    }

    pub fn add_actor(&mut self, value: Body, position: Vec2f) -> (u64, usize) {
        let id = self.id_counter.next();
        let index = self.actors.add(id, value);
        self.actors.positions[index] = position;
        (id, index)
    }

    pub fn add_dynamic_body(&mut self, value: Body, position: Vec2f) -> (u64, usize) {
        let id = self.id_counter.next();
        let index = self.dynamic_bodies.add(id, value);
        self.dynamic_bodies.positions[index] = position;
        (id, index)
    }

    pub fn add_static_body(&mut self, value: Body, position: Vec2f) -> (u64, usize) {
        let id = self.id_counter.next();
        let index = self.static_bodies.add(id, value);
        self.static_bodies.positions[index] = position;
        (id, index)
    }

    pub fn add_actor_spell_element(&mut self, index: usize, element: Element) {
        self.actors.add_spell_element(index, element);
    }

    pub fn start_directed_magick(&mut self, index: usize) {
        let magick = self.actors.spells[index].cast();
        if magick.power[Element::Shield as usize] > 0.0 {
            return;
        } else if magick.power[Element::Earth as usize] > 0.0 {
            self.delayed_magics.push(DelayedMagick {
                actor_id: self.actors.ids[index],
                started: self.now,
                power: magick.power,
            });
        } else if magick.power[Element::Ice as usize] > 0.0 {
            return;
        } else if magick.power[Element::Arcane as usize] > 0.0
            || magick.power[Element::Life as usize] > 0.0 {
            self.beams.add(
                self.id_counter.next(),
                Beam { magick, source: Id::Actor(self.actors.ids[index]) },
            );
        } else if magick.power[Element::Lightning as usize] > 0.0 {
            return;
        } else if magick.power[Element::Water as usize] > 0.0
            || magick.power[Element::Cold as usize] > 0.0
            || magick.power[Element::Fire as usize] > 0.0
            || magick.power[Element::Steam as usize] > 0.0
            || magick.power[Element::Poison as usize] > 0.0 {
            return;
        }
    }

    pub fn complete_directed_magick(&mut self, index: usize) {
        let actor_id = self.actors.get_id(index);
        if let Some(index) = self.beams.find_by_actor_id(actor_id) {
            self.beams.remove(index);
        } else if let Some(magick_index) = self.delayed_magics.iter()
            .find_position(|v| v.actor_id == actor_id).map(|(v, _)| v) {
            let delayed_magick = self.delayed_magics.remove(magick_index);
            let actor_radius = self.actors.get_body(index).radius;
            let radius = delayed_magick.power.iter().sum::<f64>() * actor_radius / MAX_MAGIC_POWER;
            let material = Material::Stone;
            let direction = self.actors.current_directions[index];
            let position = self.actors.positions[index]
                + direction * (self.actors.bodies[index].radius + radius) * SHIFT_FACTOR;
            let (_, new_body_index) = self.add_dynamic_body(
                Body {
                    radius,
                    mass: get_body_mass(get_circle_volume(radius), &material),
                    restitution: get_material_restitution(&material),
                    material,
                },
                position,
            );
            self.dynamic_bodies.velocities[new_body_index] = self.actors.velocities[index];
            self.dynamic_bodies.dynamic_forces[new_body_index] = self.actors.current_directions[index]
                * (self.now - delayed_magick.started).min(MAX_MAGIC_POWER) * MAGIC_FORCE_MULTIPLIER;
            self.dynamic_bodies.effects[new_body_index].power = delayed_magick.power.clone();
            for applied in self.dynamic_bodies.effects[new_body_index].applied.iter_mut() {
                *applied = self.now;
            }
        }
    }

    pub fn self_magick(&mut self, index: usize) {
        let magick = self.actors.spells[index].cast();
        if magick.power[Element::Shield as usize] == 0.0 {
            self.actors.effects[index] = add_magick_power_to_effect(self.now, &self.actors.effects[index], &magick.power);
        } else {
            let mut elements = [false; 11];
            for i in 0..magick.power.len() {
                elements[i] = magick.power[i] > 0.0;
            }
            let power = magick.power.iter().sum();
            let radius = if elements[Element::Earth as usize] || elements[Element::Ice as usize]
                || (elements[Element::Shield as usize] && elements.iter().filter(|v| **v).count() == 1) {
                self.actors.bodies[index].radius
            } else {
                self.actors.bodies[index].radius * power
            };
            self.actors.auras[index] = Aura { applied: self.now, power, radius, elements };
        }
    }

    pub fn update(&mut self, duration: f64) {
        self.now += duration;
        self.actors.retain();
        self.dynamic_bodies.retain();
        self.static_bodies.retain();
        self.beams.retain(&self.actors.ids);
        self.beams.reflected_beams.clear();
        self.beams.reflected_shift = self.beams.ids.len();
        update_current_directions(duration, &self.actors.target_directions, &mut self.actors.current_directions);
        update_dynamic_forces(&self.actors.current_directions, &self.actors.const_forces, &mut self.actors.dynamic_forces);
        decay_effects(self.now, duration, &mut self.actors.effects);
        decay_effects(self.now, duration, &mut self.dynamic_bodies.effects);
        decay_effects(self.now, duration, &mut self.static_bodies.effects);
        decay_auras(self.now, duration, &mut self.actors.auras);
        decay_auras(self.now, duration, &mut self.dynamic_bodies.auras);
        decay_auras(self.now, duration, &mut self.static_bodies.auras);
        self.collide_beams();
        update_positions(duration, &self.actors.velocities, &mut self.actors.positions);
        update_positions(duration, &self.dynamic_bodies.velocities, &mut self.dynamic_bodies.positions);
        update_velocities(duration, &self.actors.bodies, &self.actors.dynamic_forces, &mut self.actors.velocities);
        update_velocities(duration, &self.dynamic_bodies.bodies, &self.dynamic_bodies.dynamic_forces, &mut self.dynamic_bodies.velocities);
        if !self.actors.ids.is_empty() {
            for i in 0..self.actors.ids.len() {
                for j in 0..self.static_bodies.ids.len() {
                    self.collide_actor_and_static_body(i, j);
                }
            }
        }
        if !self.actors.ids.is_empty() {
            for i in 0..self.actors.ids.len() - 1 {
                for j in i + 1..self.actors.ids.len() {
                    self.collide_actors(i, j);
                }
            }
        }
        for i in 0..self.dynamic_bodies.ids.len() {
            for j in 0..self.static_bodies.ids.len() {
                self.collide_dynamic_body_and_static_body(i, j);
            }
            for j in 0..self.actors.ids.len() {
                self.collide_dynamic_body_and_actor(i, j);
            }
        }
        if !self.dynamic_bodies.ids.is_empty() {
            for i in 0..self.dynamic_bodies.ids.len() - 1 {
                for j in i + 1..self.dynamic_bodies.ids.len() {
                    self.collide_dynamic_bodies(i, j);
                }
            }
        }
        self.actors.dynamic_forces.fill(Vec2f::ZERO);
        self.dynamic_bodies.dynamic_forces.fill(Vec2f::ZERO);
        damage_health(&self.actors.bodies, &self.actors.effects, &mut self.actors.healths);
        damage_health(&self.dynamic_bodies.bodies, &self.dynamic_bodies.effects, &mut self.dynamic_bodies.healths);
        damage_health(&self.static_bodies.bodies, &self.static_bodies.effects, &mut self.static_bodies.healths);
        mark_inactive(&self.bounds, &self.actors.bodies, &self.actors.positions, &self.actors.healths, &mut self.actors.active);
        mark_inactive(&self.bounds, &self.dynamic_bodies.bodies, &self.dynamic_bodies.positions, &self.dynamic_bodies.healths, &mut self.dynamic_bodies.active);
        for i in 0..self.static_bodies.active.len() {
            self.static_bodies.active[i] = self.static_bodies.healths[i] > 0.0;
        }
    }

    fn collide_actors(&mut self, lhs: usize, rhs: usize) {
        let (lhs_positions, rhs_positions) = self.actors.positions.split_at_mut(rhs);
        let (lhs_velocities, rhs_velocities) = self.actors.velocities.split_at_mut(rhs);
        let (lhs_effects, rhs_effects) = self.actors.effects.split_at_mut(rhs);
        let (lhs_healths, rhs_healths) = self.actors.healths.split_at_mut(rhs);
        collide_dynamic(
            self.now,
            &mut DynamicCollidingBody {
                body: &self.actors.bodies[lhs],
                position: &mut lhs_positions[lhs],
                velocity: &mut lhs_velocities[lhs],
                effect: &mut lhs_effects[lhs],
                health: &mut lhs_healths[lhs],
                aura: &self.actors.auras[lhs],
            },
            &mut DynamicCollidingBody {
                body: &self.actors.bodies[rhs],
                position: &mut rhs_positions[0],
                velocity: &mut rhs_velocities[0],
                effect: &mut rhs_effects[0],
                health: &mut rhs_healths[0],
                aura: &self.actors.auras[rhs],
            },
        );
    }

    fn collide_actor_and_static_body(&mut self, actor: usize, static_body: usize) {
        collide_with_static(
            self.now,
            &mut DynamicCollidingBody {
                body: &self.actors.bodies[actor],
                position: &mut self.actors.positions[actor],
                velocity: &mut self.actors.velocities[actor],
                effect: &mut self.actors.effects[actor],
                health: &mut self.actors.healths[actor],
                aura: &self.actors.auras[actor],
            },
            &mut StaticCollidingBody {
                body: &self.static_bodies.bodies[static_body],
                position: &self.static_bodies.positions[static_body],
                effect: &mut self.static_bodies.effects[static_body],
                health: &mut self.static_bodies.healths[static_body],
                aura: &self.static_bodies.auras[static_body],
            },
        );
    }

    fn collide_dynamic_body_and_static_body(&mut self, dynamic_body: usize, static_body: usize) {
        collide_with_static(
            self.now,
            &mut DynamicCollidingBody {
                body: &self.dynamic_bodies.bodies[dynamic_body],
                position: &mut self.dynamic_bodies.positions[dynamic_body],
                velocity: &mut self.dynamic_bodies.velocities[dynamic_body],
                effect: &mut self.dynamic_bodies.effects[dynamic_body],
                health: &mut self.dynamic_bodies.healths[dynamic_body],
                aura: &self.dynamic_bodies.auras[dynamic_body],
            },
            &mut StaticCollidingBody {
                body: &self.static_bodies.bodies[static_body],
                position: &self.static_bodies.positions[static_body],
                effect: &mut self.static_bodies.effects[static_body],
                health: &mut self.static_bodies.healths[static_body],
                aura: &self.static_bodies.auras[static_body],
            },
        );
    }

    fn collide_dynamic_body_and_actor(&mut self, dynamic_body: usize, actor: usize) {
        collide_dynamic(
            self.now,
            &mut DynamicCollidingBody {
                body: &self.dynamic_bodies.bodies[dynamic_body],
                position: &mut self.dynamic_bodies.positions[dynamic_body],
                velocity: &mut self.dynamic_bodies.velocities[dynamic_body],
                effect: &mut self.dynamic_bodies.effects[dynamic_body],
                health: &mut self.dynamic_bodies.healths[dynamic_body],
                aura: &self.dynamic_bodies.auras[dynamic_body],
            },
            &mut DynamicCollidingBody {
                body: &self.actors.bodies[actor],
                position: &mut self.actors.positions[actor],
                velocity: &mut self.actors.velocities[actor],
                effect: &mut self.actors.effects[actor],
                health: &mut self.actors.healths[actor],
                aura: &self.actors.auras[actor],
            },
        );
    }

    fn collide_dynamic_bodies(&mut self, lhs: usize, rhs: usize) {
        let (lhs_positions, rhs_positions) = self.dynamic_bodies.positions.split_at_mut(rhs);
        let (lhs_velocities, rhs_velocities) = self.dynamic_bodies.velocities.split_at_mut(rhs);
        let (lhs_effects, rhs_effects) = self.dynamic_bodies.effects.split_at_mut(rhs);
        let (lhs_healths, rhs_healths) = self.dynamic_bodies.healths.split_at_mut(rhs);
        collide_dynamic(
            self.now,
            &mut DynamicCollidingBody {
                body: &self.dynamic_bodies.bodies[lhs],
                position: &mut lhs_positions[lhs],
                velocity: &mut lhs_velocities[lhs],
                effect: &mut lhs_effects[lhs],
                health: &mut lhs_healths[lhs],
                aura: &self.dynamic_bodies.auras[lhs],
            },
            &mut DynamicCollidingBody {
                body: &self.dynamic_bodies.bodies[rhs],
                position: &mut rhs_positions[0],
                velocity: &mut rhs_velocities[0],
                effect: &mut rhs_effects[0],
                health: &mut rhs_healths[0],
                aura: &self.dynamic_bodies.auras[rhs],
            },
        );
    }

    fn collide_beams(&mut self) {
        let temp_beam_shift = self.beams.reflected_shift;
        let mut beam_index = 0;
        while beam_index < self.beams.ids.len() {
            let i = beam_index;
            let (length, hit_index, reflection) = match self.beams.values[i].source {
                Id::Actor(actor_id) => {
                    let actor_index = self.actors.get_index(actor_id);
                    let direction = self.actors.current_directions[actor_index];
                    let begin = self.actors.positions[actor_index] + direction * SHIFT_FACTOR;
                    self.collide_beam(begin, direction, 0)
                }
                Id::Beam => {
                    let temp_beam = &self.beams.reflected_beams[i - temp_beam_shift];
                    let add_length = SHIFT_FACTOR - 1.0;
                    let result = self.collide_beam(temp_beam.begin + temp_beam.direction * add_length, temp_beam.direction, temp_beam.depth);
                    (result.0 + add_length, result.1, result.2)
                }
            };
            self.beams.lengths[i] = length;
            if let Some(hit_index) = hit_index {
                if let Some(temp_beam) = reflection {
                    self.beams.add_reflected(
                        self.id_counter.next(),
                        Beam { magick: self.beams.values[i].magick.clone(), source: Id::Beam },
                        temp_beam,
                    );
                } else {
                    match hit_index {
                        Index::Actor(index) => {
                            self.actors.effects[index] = add_magick_power_to_effect(
                                self.now,
                                &self.actors.effects[index],
                                &self.beams.values[i].magick.power,
                            );
                        }
                        Index::DynamicBody(index) => {
                            self.dynamic_bodies.effects[index] = add_magick_power_to_effect(
                                self.now,
                                &self.dynamic_bodies.effects[index],
                                &self.beams.values[i].magick.power,
                            );
                        }
                        Index::StaticBody(index) => {
                            self.static_bodies.effects[index] = add_magick_power_to_effect(
                                self.now,
                                &self.static_bodies.effects[index],
                                &self.beams.values[i].magick.power,
                            );
                        }
                    }
                }
            }
            beam_index += 1;
        }
    }

    fn collide_beam(&self, begin: Vec2f, direction: Vec2f, depth: usize) -> (f64, Option<Index>, Option<ReflectedBeam>) {
        let mut length = MAX_BEAM_LENGTH;
        let mut nearest_hit = collide_beam_with_bodies(
            begin, direction,
            &self.actors.bodies, &self.actors.positions,
            &mut length,
        ).map(|v| Index::Actor(v));
        nearest_hit = collide_beam_with_bodies(
            begin, direction,
            &self.dynamic_bodies.bodies, &self.dynamic_bodies.positions,
            &mut length,
        ).map(|v| Index::DynamicBody(v)).or(nearest_hit);
        nearest_hit = collide_beam_with_bodies(
            begin, direction,
            &self.static_bodies.bodies, &self.static_bodies.positions,
            &mut length,
        ).map(|v| Index::StaticBody(v)).or(nearest_hit);
        if let Some(hit_body_index) = nearest_hit {
            if depth < MAX_BEAM_DEPTH && can_reflect_beams(&self.get_aura(hit_body_index).elements) {
                let end = begin + direction * length;
                let normal = (self.get_position(hit_body_index) - end).normalized();
                let temp_beam = ReflectedBeam {
                    begin: end,
                    direction: direction - normal * 2.0 * direction.cos(normal),
                    depth: depth + 1,
                };
                (length, Some(hit_body_index), Some(temp_beam))
            } else {
                (length, Some(hit_body_index), None)
            }
        } else {
            (length, None, None)
        }
    }

    fn get_aura(&self, index: Index) -> &Aura {
        match index {
            Index::Actor(v) => &self.actors.auras[v],
            Index::DynamicBody(v) => &self.dynamic_bodies.auras[v],
            Index::StaticBody(v) => &self.static_bodies.auras[v],
        }
    }

    fn get_position(&self, index: Index) -> Vec2f {
        match index {
            Index::Actor(v) => self.actors.positions[v],
            Index::DynamicBody(v) => self.dynamic_bodies.positions[v],
            Index::StaticBody(v) => self.static_bodies.positions[v],
        }
    }
}

pub fn get_circle_volume(radius: f64) -> f64 {
    radius * radius * radius * std::f64::consts::PI
}

pub fn get_body_mass(volume: f64, material: &Material) -> f64 {
    volume * get_material_density(material)
}

pub fn get_material_density(material: &Material) -> f64 {
    match material {
        Material::Flesh => 800.0,
        Material::Stone => 2750.0,
    }
}

pub fn get_material_restitution(material: &Material) -> f64 {
    match material {
        Material::Flesh => 0.05,
        Material::Stone => 0.2,
    }
}

fn combine_elements(target: Element, element: Element) -> Option<Element> {
    if (target == Element::Water && element == Element::Fire)
        || (target == Element::Fire && element == Element::Water) {
        Some(Element::Steam)
    } else if (target == Element::Water && element == Element::Cold)
        || (target == Element::Cold && element == Element::Water) {
        Some(Element::Ice)
    } else if (target == Element::Water && element == Element::Arcane)
        || (target == Element::Arcane && element == Element::Water) {
        Some(Element::Poison)
    } else if target == Element::Ice && element == Element::Fire {
        Some(Element::Water)
    } else {
        None
    }
}

fn can_cancel_element(target: Element, element: Element) -> bool {
    (target == Element::Water && element == Element::Lightning)
        || (target == Element::Lightning && (element == Element::Earth || element == Element::Water))
        || (target == Element::Life && element == Element::Arcane)
        || (target == Element::Arcane && element == Element::Life)
        || (target == Element::Shield && element == Element::Shield)
        || (target == Element::Earth && element == Element::Lightning)
        || (target == Element::Cold && element == Element::Fire)
        || (target == Element::Fire && element == Element::Cold)
        || (target == Element::Steam && element == Element::Cold)
        || (target == Element::Ice && element == Element::Fire)
        || (target == Element::Poison && element == Element::Life)
}

fn update_current_directions(duration: f64, target_directions: &Vec<Vec2f>, current_directions: &mut Vec<Vec2f>) {
    for i in 0..current_directions.len() {
        current_directions[i] = get_current_direction(current_directions[i], target_directions[i], duration);
    }
}

fn update_dynamic_forces(current_directions: &Vec<Vec2f>, const_forces: &Vec<f64>, dynamic_forces: &mut Vec<Vec2f>) {
    for i in 0..dynamic_forces.len() {
        dynamic_forces[i] += current_directions[i] * (const_forces[i] * CONST_FORCE_MULTIPLIER);
    }
}

fn decay_effects(now: f64, duration: f64, effects: &mut Vec<Effect>) {
    for effect in effects.iter_mut() {
        decay_effect(now, duration, effect);
    }
}

fn decay_effect(now: f64, duration: f64, effect: &mut Effect) {
    for i in 0..effect.power.len() {
        if is_instant_effect_element(Element::from(i)) {
            effect.power[i] = 0.0;
        } else {
            let passed = now - effect.applied[i];
            let initial = effect.power[i] - DECAY_FACTOR * (passed - duration).square();
            effect.power[i] = (initial - DECAY_FACTOR * passed.square()).max(0.0);
        }
    }
}

fn decay_auras(now: f64, duration: f64, auras: &mut Vec<Aura>) {
    for aura in auras.iter_mut() {
        decay_aura(now, duration, aura);
    }
}

fn decay_aura(now: f64, duration: f64, aura: &mut Aura) {
    let passed = now - aura.applied;
    let initial = aura.power - DECAY_FACTOR * (passed - duration).square();
    aura.power = initial - DECAY_FACTOR * passed.square();
    if aura.power <= 0.0 {
        aura.power = 0.0;
        aura.elements.fill(false);
    }
}

fn add_magick_power_to_effect(now: f64, target: &Effect, other: &[f64; 11]) -> Effect {
    let mut power = target.power.clone();
    let mut applied = target.applied.clone();
    for i in 0..power.len() {
        if other[i] > 0.0 {
            power[i] = power[i].max(other[i]);
            applied[i] = now;
        }
    }
    let target_power = power.clone();
    if target_power[Element::Water as usize] > 0.0 && target_power[Element::Fire as usize] > 0.0 {
        power[Element::Water as usize] = 0.0;
        power[Element::Fire as usize] = 0.0;
        power[Element::Steam as usize] = target_power[Element::Water as usize].max(target_power[Element::Fire as usize]);
        applied[Element::Steam as usize] = now;
    }
    if target_power[Element::Poison as usize] > 0.0 && target_power[Element::Life as usize] > 0.0 {
        power[Element::Poison as usize] = 0.0;
        power[Element::Life as usize] = 0.0;
    }
    if target_power[Element::Cold as usize] > 0.0 && target_power[Element::Fire as usize] > 0.0 {
        power[Element::Cold as usize] = 0.0;
        power[Element::Fire as usize] = 0.0;
    }
    if target_power[Element::Ice as usize] > 0.0 && target_power[Element::Fire as usize] > 0.0 {
        power[Element::Ice as usize] = 0.0;
        power[Element::Fire as usize] = 0.0;
    }
    if target_power[Element::Water as usize] > 0.0 && target_power[Element::Cold as usize] > 0.0 {
        power[Element::Ice as usize] = target_power[Element::Water as usize].max(target_power[Element::Water as usize]);
        applied[Element::Ice as usize] = now;
    }
    if target_power[Element::Water as usize] > 0.0 && target_power[Element::Arcane as usize] > 0.0 {
        power[Element::Poison as usize] = target_power[Element::Water as usize].max(target_power[Element::Arcane as usize]);
        applied[Element::Poison as usize] = now;
    }
    Effect { power, applied }
}

fn get_damage(power: &[f64; 11]) -> f64 {
    power[Element::Lightning as usize]
        - power[Element::Life as usize]
        + power[Element::Arcane as usize]
        + power[Element::Cold as usize]
        + power[Element::Fire as usize]
        + power[Element::Steam as usize]
        + power[Element::Poison as usize]
}

fn is_instant_effect_element(element: Element) -> bool {
    matches!(element, Element::Lightning | Element::Arcane | Element::Earth | Element::Steam)
}

fn can_absorb_physical_damage(elements: &[bool; 11]) -> bool {
    let expect = elements[Element::Shield as usize] as i32 + elements[Element::Earth as usize] as i32;
    elements[Element::Shield as usize] && expect == elements.iter().map(|v| *v as i32).sum()
}

fn can_reflect_beams(elements: &[bool; 11]) -> bool {
    elements[Element::Shield as usize] && 1 == elements.iter().map(|v| *v as i32).sum()
}

fn get_current_direction(current_direction: Vec2f, target_direction: Vec2f, duration: f64) -> Vec2f {
    let diff = normalize_angle(target_direction.angle() - current_direction.angle());
    current_direction.rotated(diff.signum() * diff.abs().min(MAX_ROTATION_SPEED * duration))
}

fn normalize_angle(angle: f64) -> f64 {
    let turns = angle / (2.0 * std::f64::consts::PI) + 0.5;
    return (turns - turns.floor() - 0.5) * (2.0 * std::f64::consts::PI);
}

fn collide_beam_with_bodies(begin: Vec2f, direction: Vec2f, bodies: &Vec<Body>, positions: &Vec<Vec2f>, length: &mut f64) -> Option<usize> {
    let mut nearest_hit = None;
    for i in 0..bodies.len() {
        let segment = Segment::new(begin, begin + direction * *length);
        let circle = Circle::new(positions[i], bodies[i].radius);
        if let Some(intersection) = circle.get_first_intersection_with_segment(&segment) {
            *length = begin.distance(intersection);
            nearest_hit = Some(i);
        }
    }
    nearest_hit
}

fn update_positions(duration: f64, velocities: &Vec<Vec2f>, positions: &mut Vec<Vec2f>) {
    for i in 0..positions.len() {
        positions[i] += velocities[i] * duration;
    }
}

fn update_velocities(duration: f64, bodies: &Vec<Body>, dynamic_forces: &Vec<Vec2f>, velocities: &mut Vec<Vec2f>) {
    for i in 0..velocities.len() {
        velocities[i] += dynamic_forces[i] * (duration / bodies[i].mass);
    }
}

struct DynamicCollidingBody<'a> {
    body: &'a Body,
    position: &'a mut Vec2f,
    velocity: &'a mut Vec2f,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

fn collide_dynamic(now: f64, lhs: &mut DynamicCollidingBody, rhs: &mut DynamicCollidingBody) {
    let delta_position = *rhs.position - *lhs.position;
    let distance = delta_position.norm();
    let penetration = lhs.body.radius + rhs.body.radius - distance;
    if penetration <= 0.0 {
        return;
    }
    let mass_sum = lhs.body.mass + rhs.body.mass;
    let normal = delta_position.normalized();
    let delta_velocity = *lhs.velocity - *rhs.velocity;
    let lhs_kinetic_energy = get_kinetic_energy(lhs.body.mass, *lhs.velocity);
    let rhs_kinetic_energy = get_kinetic_energy(rhs.body.mass, *rhs.velocity);
    let lhs_velocity = *lhs.velocity - delta_velocity * rhs.body.mass * (1.0 + lhs.body.restitution) / mass_sum;
    let rhs_velocity = *rhs.velocity + delta_velocity * lhs.body.mass * (1.0 + rhs.body.restitution) / mass_sum;
    *lhs.position = *lhs.position - normal * (penetration * rhs.body.mass / mass_sum);
    *lhs.velocity = lhs_velocity;
    *rhs.position = *rhs.position + normal * (penetration * lhs.body.mass / mass_sum);
    *rhs.velocity = rhs_velocity;
    let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
    let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
    *lhs.effect = new_lhs_effect;
    *rhs.effect = new_rhs_effect;
    if !can_absorb_physical_damage(&lhs.aura.elements) {
        *lhs.health -= (get_kinetic_energy(lhs.body.mass, lhs_velocity) - lhs_kinetic_energy).abs();
    }
    if !can_absorb_physical_damage(&rhs.aura.elements) {
        *rhs.health -= (get_kinetic_energy(rhs.body.mass, rhs_velocity) - rhs_kinetic_energy).abs();
    }
}

struct StaticCollidingBody<'a> {
    body: &'a Body,
    position: &'a Vec2f,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

fn collide_with_static(now: f64, lhs: &mut DynamicCollidingBody, rhs: &mut StaticCollidingBody) {
    let delta_position = *rhs.position - *lhs.position;
    let distance = delta_position.norm();
    let penetration = lhs.body.radius + rhs.body.radius - distance;
    if penetration <= 0.0 {
        return;
    }
    let mass_sum = lhs.body.mass + rhs.body.mass;
    let normal = delta_position.normalized();
    let delta_velocity = *lhs.velocity;
    let lhs_kinetic_energy = get_kinetic_energy(lhs.body.mass, *lhs.velocity);
    let lhs_velocity = *lhs.velocity - delta_velocity * rhs.body.mass * (1.0 + lhs.body.restitution) / mass_sum;
    let rhs_velocity = delta_velocity * lhs.body.mass * (1.0 + rhs.body.restitution) / mass_sum;
    *lhs.position = *lhs.position - normal * (penetration * rhs.body.mass / mass_sum);
    *lhs.velocity = lhs_velocity;
    let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
    let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
    *lhs.effect = new_lhs_effect;
    *rhs.effect = new_rhs_effect;
    if !can_absorb_physical_damage(&lhs.aura.elements) {
        *lhs.health -= (get_kinetic_energy(lhs.body.mass, lhs_velocity) - lhs_kinetic_energy).abs();
    }
    if !can_absorb_physical_damage(&rhs.aura.elements) {
        *rhs.health -= get_kinetic_energy(rhs.body.mass, rhs_velocity);
    }
}

fn get_kinetic_energy(mass: f64, velocity: Vec2f) -> f64 {
    mass * velocity.dot_self() / 2.0
}

fn damage_health(bodies: &Vec<Body>, effects: &Vec<Effect>, healths: &mut Vec<f64>) {
    for i in 0..healths.len() {
        healths[i] -= get_damage(&effects[i].power) * bodies[i].mass * DAMAGE_FACTOR;
    }
}

fn mark_inactive(bounds: &Rectf, bodies: &Vec<Body>, positions: &Vec<Vec2f>, healths: &Vec<f64>, active: &mut Vec<bool>) {
    for i in 0..active.len() {
        let radius = bodies[i].radius;
        let position = positions[i];
        let rect = Rectf::new(
            position - Vec2f::both(radius),
            position + Vec2f::both(radius),
        );
        active[i] = healths[i] > 0.0 && bounds.overlaps(&rect);
    }
}
