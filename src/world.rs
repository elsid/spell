use itertools::Itertools;

use crate::circle::Circle;
use crate::rect::Rectf;
use crate::segment::Segment;
use crate::vec2::{Square, Vec2f};

pub const GRAVITY_CONST: f64 = 6.67430e-11;
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
    pub body_id: u64,
    pub started: f64,
    pub power: [f64; 11],
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum Id {
    Body(u64),
    Beam(u64),
}

#[derive(Debug, Copy, Clone)]
pub enum Index {
    Body(usize),
    Beam(usize),
}

#[derive(Debug, Clone)]
pub struct Beam {
    pub source: Id,
    pub magick: Magick,
}

#[derive(Debug, Clone)]
pub struct Collision {
    pub lhs: usize,
    pub rhs: usize,
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
pub struct TempBeam {
    pub begin: Vec2f,
    pub direction: Vec2f,
}

#[derive(Debug)]
pub struct World {
    bounds: Rectf,
    gravity_const: f64,
    id_counter: IdCounter,
    now: f64,
    body_ids: Vec<u64>,
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
    collisions: Vec<Collision>,
    beams: Vec<Beam>,
    beam_ids: Vec<u64>,
    beam_lengths: Vec<f64>,
    delayed_magics: Vec<DelayedMagick>,
    temp_beams: Vec<TempBeam>,
    temp_beam_shift: usize,
}

impl World {
    pub fn new(bounds: Rectf, gravity_const: f64) -> Self {
        World {
            bounds,
            gravity_const,
            id_counter: IdCounter::default(),
            now: 0.0,
            body_ids: Vec::new(),
            bodies: Vec::new(),
            positions: Vec::new(),
            current_directions: Vec::new(),
            target_directions: Vec::new(),
            velocities: Vec::new(),
            const_forces: Vec::new(),
            dynamic_forces: Vec::new(),
            effects: Vec::new(),
            auras: Vec::new(),
            healths: Vec::new(),
            spells: Vec::new(),
            active: Vec::new(),
            collisions: Vec::new(),
            beams: Vec::new(),
            beam_ids: Vec::new(),
            beam_lengths: Vec::new(),
            delayed_magics: Vec::new(),
            temp_beams: Vec::new(),
            temp_beam_shift: 0,
        }
    }

    pub fn bounds(&self) -> &Rectf {
        &self.bounds
    }

    pub fn bodies(&self) -> &Vec<Body> {
        &self.bodies
    }

    pub fn collisions(&self) -> &Vec<Collision> {
        &self.collisions
    }

    pub fn beams(&self) -> &Vec<Beam> {
        &self.beams
    }

    pub fn get_body(&self, index: usize) -> &Body {
        &self.bodies[index]
    }

    pub fn get_position(&self, index: usize) -> Vec2f {
        self.positions[index]
    }

    pub fn get_current_direction(&self, index: usize) -> Vec2f {
        self.current_directions[index]
    }

    pub fn get_target_direction(&self, index: usize) -> Vec2f {
        self.target_directions[index]
    }

    pub fn get_velocity(&self, index: usize) -> Vec2f {
        self.velocities[index]
    }

    pub fn get_const_force(&self, index: usize) -> f64 {
        self.const_forces[index]
    }

    pub fn get_dynamic_force(&self, index: usize) -> Vec2f {
        self.dynamic_forces[index]
    }

    pub fn get_effect(&self, index: usize) -> &Effect {
        &self.effects[index]
    }

    pub fn get_aura(&self, index: usize) -> &Aura {
        &self.auras[index]
    }

    pub fn get_health(&self, index: usize) -> f64 {
        self.healths[index]
    }

    pub fn get_spell(&self, index: usize) -> &Spell {
        &self.spells[index]
    }

    pub fn is_active(&self, index: usize) -> bool {
        self.active[index]
    }

    pub fn get_beam_length(&self, index: usize) -> f64 {
        self.beam_lengths[index]
    }

    pub fn get_temp_beam(&self, index: usize) -> &TempBeam {
        &self.temp_beams[index - self.temp_beam_shift]
    }

    pub fn get_index(&self, id: u64) -> usize {
        self.find_index(id).unwrap()
    }

    pub fn find_index(&self, id: u64) -> Option<usize> {
        self.body_ids.iter().find_position(|v| **v == id).map(|(i, _)| i)
    }

    pub fn set_position(&mut self, index: usize, value: Vec2f) {
        self.positions[index] = value;
    }

    pub fn set_target_direction(&mut self, index: usize, value: Vec2f) {
        self.target_directions[index] = value;
    }

    pub fn set_const_force(&mut self, index: usize, value: f64) {
        self.const_forces[index] = value;
    }

    pub fn add_body(&mut self, value: Body) -> (u64, usize) {
        let id = self.id_counter.next();
        self.body_ids.push(id);
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
        (id, self.bodies.len() - 1)
    }

    pub fn remove_body(&mut self, index: usize) {
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
        self.body_ids.remove(index);
    }

    pub fn add_spell_element(&mut self, index: usize, element: Element) {
        self.spells[index].add(element)
    }

    pub fn start_directed_magick(&mut self, index: usize) {
        let magick = self.spells[index].cast();
        if magick.power[Element::Shield as usize] > 0.0 {
            return;
        } else if magick.power[Element::Earth as usize] > 0.0 {
            self.delayed_magics.push(DelayedMagick {
                body_id: self.body_ids[index],
                started: self.now,
                power: magick.power,
            });
        } else if magick.power[Element::Ice as usize] > 0.0 {
            return;
        } else if magick.power[Element::Arcane as usize] > 0.0
            || magick.power[Element::Life as usize] > 0.0 {
            self.beams.push(Beam { magick, source: Id::Body(self.body_ids[index]) });
            self.beam_ids.push(self.id_counter.next());
            self.beam_lengths.push(MAX_BEAM_LENGTH);
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
        let body_id = self.body_ids[index];
        if let Some((index, _)) = self.beams.iter()
            .find_position(|beam| beam.source == Id::Body(body_id)) {
            self.beams.remove(index);
            self.beam_lengths.remove(index);
            self.temp_beam_shift -= 1;
        } else if let Some(magick_index) = self.delayed_magics.iter()
            .find_position(|v| v.body_id == body_id).map(|(v, _)| v) {
            let delayed_magick = self.delayed_magics.remove(magick_index);
            let source_body_radius = self.bodies[index].radius;
            let radius = delayed_magick.power.iter().sum::<f64>() * source_body_radius / MAX_MAGIC_POWER;
            let material = Material::Stone;
            let (_, new_body_index) = self.add_body(Body {
                radius,
                mass: get_body_mass(get_circle_volume(radius), &material),
                restitution: get_material_restitution(&material),
                material,
            });
            let direction = self.current_directions[index];
            self.positions[new_body_index] = self.positions[index]
                + direction * (self.bodies[index].radius + radius) * SHIFT_FACTOR;
            self.velocities[new_body_index] = self.velocities[index];
            self.dynamic_forces[new_body_index] = self.current_directions[index]
                * (self.now - delayed_magick.started).min(MAX_MAGIC_POWER) * MAGIC_FORCE_MULTIPLIER;
            self.effects[new_body_index].power = delayed_magick.power.clone();
            for applied in self.effects[new_body_index].applied.iter_mut() {
                *applied = self.now;
            }
        }
    }

    pub fn self_magick(&mut self, index: usize) {
        let magick = self.spells[index].cast();
        if magick.power[Element::Shield as usize] == 0.0 {
            self.effects[index] = add_magick_power_to_effect(self.now, &self.effects[index], &magick.power);
        } else {
            let mut elements = [false; 11];
            for i in 0..magick.power.len() {
                elements[i] = magick.power[i] > 0.0;
            }
            self.auras[index] = Aura {
                applied: self.now,
                power: magick.power.iter().sum(),
                elements,
            };
        }
    }

    pub fn update(&mut self, duration: f64) {
        self.now += duration;
        self.retain_active_bodies();
        self.retain_beams();
        self.collisions.clear();
        self.temp_beams.clear();
        self.temp_beam_shift = self.beams.len();
        if self.bodies.is_empty() {
            return;
        }
        for i in 0..self.bodies.len() {
            self.current_directions[i] = get_current_direction(self.current_directions[i], self.target_directions[i], duration);
            self.dynamic_forces[i] += self.current_directions[i] * (self.const_forces[i] * CONST_FORCE_MULTIPLIER);
            decay_effect(self.now, duration, &mut self.effects[i]);
            decay_aura(self.now, duration, &mut self.auras[i]);
        }
        let temp_beam_shift = self.temp_beam_shift;
        let mut beam_index = 0;
        while beam_index < self.beams.len() {
            let i = beam_index;
            let (length, hit_index, reflection) = match self.beams[i].source {
                Id::Body(body_id) => {
                    let body_index = self.get_index(body_id);
                    let direction = self.current_directions[body_index];
                    let begin = self.positions[body_index] + direction * SHIFT_FACTOR;
                    self.collide_beams(begin, direction)
                }
                Id::Beam(..) => {
                    let temp_beam = &self.temp_beams[i - temp_beam_shift];
                    let add_length = SHIFT_FACTOR - 1.0;
                    let result = self.collide_beams(temp_beam.begin + temp_beam.direction * add_length, temp_beam.direction);
                    (result.0 + add_length, result.1, result.2)
                }
            };
            self.beam_lengths[i] = length;
            match hit_index {
                Some(Index::Body(body_index)) => {
                    if let Some(temp_beam) = reflection {
                        self.beams.push(Beam { magick: self.beams[i].magick.clone(), source: Id::Beam(self.beam_ids[i]) });
                        self.beam_ids.push(self.id_counter.next());
                        self.beam_lengths.push(MAX_BEAM_LENGTH);
                        self.temp_beams.push(temp_beam);
                    } else {
                        self.effects[body_index] = add_magick_power_to_effect(self.now, &self.effects[body_index], &self.beams[i].magick.power);
                    }
                }
                Some(Index::Beam(..)) => {}
                None => (),
            }
            beam_index += 1;
        }
        for i in 0..self.bodies.len() - 1 {
            for j in i + 1..self.bodies.len() {
                if !self.has_body_collision(i, j) {
                    let (lhs_gravity, rhs_gravity) = gravity_force(
                        &self.make_gravitational_body(i),
                        &self.make_gravitational_body(j),
                        self.gravity_const,
                    );
                    self.dynamic_forces[i] += lhs_gravity;
                    self.dynamic_forces[j] += rhs_gravity;
                }
            }
        }
        for i in 0..self.bodies.len() {
            self.positions[i] += self.velocities[i] * duration;
            self.velocities[i] += self.dynamic_forces[i] / self.bodies[i].mass * duration;
        }
        for i in 0..self.bodies.len() - 1 {
            for j in i + 1..self.bodies.len() {
                self.collide_bodies(i, j);
            }
        }
        for collision in self.collisions.iter() {
            let new_lhs_effect = add_magick_power_to_effect(self.now, &self.effects[collision.lhs], &self.effects[collision.rhs].power);
            let new_rhs_effect = add_magick_power_to_effect(self.now, &self.effects[collision.rhs], &self.effects[collision.lhs].power);
            self.effects[collision.lhs] = new_lhs_effect;
            self.effects[collision.rhs] = new_rhs_effect;
            if !can_absorb_physical_damage(&self.auras[collision.lhs].elements) {
                self.healths[collision.lhs] -= collision.lhs_physical_damage;
            }
            if !can_absorb_physical_damage(&self.auras[collision.rhs].elements) {
                self.healths[collision.rhs] -= collision.rhs_physical_damage;
            }
        }
        for i in 0..self.bodies.len() {
            self.dynamic_forces[i] = Vec2f::ZERO;
            self.healths[i] -= get_damage(&self.effects[i].power) * self.bodies[i].mass * DAMAGE_FACTOR;
        }
        for i in 0..self.bodies.len() {
            self.active[i] = self.healths[i] > 0.0
                && self.bounds.overlaps(&self.make_body_rect(i));
        }
    }

    fn make_body_rect(&self, index: usize) -> Rectf {
        let radius = self.bodies[index].radius;
        let position = self.positions[index];
        Rectf::new(
            position - Vec2f::both(radius),
            position + Vec2f::both(radius),
        )
    }

    fn make_colliding_body(&self, index: usize) -> CollidingBody {
        let body = &self.bodies[index];
        CollidingBody {
            radius: body.radius,
            mass: body.mass,
            restitution: body.restitution,
            position: self.positions[index],
            velocity: self.velocities[index],
        }
    }

    fn make_gravitational_body(&self, index: usize) -> GravitationalBody {
        let body = &self.bodies[index];
        GravitationalBody {
            mass: body.mass,
            position: self.positions[index],
        }
    }

    fn has_body_collision(&self, lhs: usize, rhs: usize) -> bool {
        has_collision(
            &PenetratingBody { radius: self.bodies[lhs].radius, position: self.positions[lhs] },
            &PenetratingBody { radius: self.bodies[rhs].radius, position: self.positions[rhs] },
        )
    }

    fn collide_bodies(&mut self, lhs: usize, rhs: usize) {
        let lhs_body = self.make_colliding_body(lhs);
        let rhs_body = self.make_colliding_body(rhs);
        if let Some((lhs_collided, rhs_collided)) = collide(&lhs_body, &rhs_body) {
            self.positions[lhs] = lhs_collided.position;
            self.velocities[lhs] = lhs_collided.velocity;
            self.positions[rhs] = rhs_collided.position;
            self.velocities[rhs] = rhs_collided.velocity;
            self.collisions.push(Collision {
                lhs,
                rhs,
                lhs_physical_damage: lhs_collided.physical_damage,
                rhs_physical_damage: rhs_collided.physical_damage,
            });
        }
    }

    fn retain_active_bodies(&mut self) {
        let len = self.body_ids.len();
        let mut del = 0;
        {
            let body_ids = &mut *self.body_ids;
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
                    body_ids.swap(i - del, i);
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
            self.body_ids.truncate(len - del);
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

    fn retain_beams(&mut self) {
        let len = self.beams.len();
        let mut del = 0;
        {
            let beams = &mut *self.beams;
            let beam_ids = &mut *self.beam_ids;
            let beam_lengths = &mut *self.beam_lengths;
            for i in 0..len {
                match beams[i].source {
                    Id::Body(body_id) => if !self.body_ids.contains(&body_id) {
                        del += 1;
                    } else if del > 0 {
                        beams.swap(i - del, i);
                        beam_ids.swap(i - del, i);
                        beam_lengths.swap(i - del, i);
                    }
                    Id::Beam(..) => del += 1,
                }
            }
        }
        if del > 0 {
            self.beams.truncate(len - del);
            self.beam_ids.truncate(len - del);
            self.beam_lengths.truncate(len - del);
        }
    }

    fn collide_beams(&self, begin: Vec2f, direction: Vec2f) -> (f64, Option<Index>, Option<TempBeam>) {
        let mut length = MAX_BEAM_LENGTH;
        let mut nearest_hit: Option<usize> = None;
        for j in 0..self.bodies.len() {
            let segment = Segment::new(begin, begin + direction * length);
            let circle = Circle::new(self.positions[j], self.bodies[j].radius);
            if let Some(intersection) = circle.get_first_intersection_with_segment(&segment) {
                length = begin.distance(intersection);
                nearest_hit = Some(j);
            }
        }
        if let Some(hit_body_index) = nearest_hit {
            if can_reflect_beams(&self.auras[hit_body_index].elements) {
                let end = begin + direction * length;
                let normal = (self.positions[hit_body_index] - end).normalized();
                let temp_beam = TempBeam {
                    begin: end,
                    direction: direction - normal * 2.0 * direction.cos(normal),
                };
                (length, Some(Index::Body(hit_body_index)), Some(temp_beam))
            } else {
                (length, Some(Index::Body(hit_body_index)), None)
            }
        } else {
            (length, None, None)
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

struct GravitationalBody {
    mass: f64,
    position: Vec2f,
}

fn gravity_force(lhs: &GravitationalBody, rhs: &GravitationalBody, gravity_const: f64) -> (Vec2f, Vec2f) {
    if lhs.position == rhs.position {
        return (Vec2f::ZERO, Vec2f::ZERO);
    }
    let scalar = scalar_gravity_force(lhs, rhs, gravity_const);
    let mass_center = (lhs.position * lhs.mass + rhs.position * rhs.mass) / (lhs.mass + rhs.mass);
    (
        (mass_center - lhs.position).normalized() * scalar,
        (mass_center - rhs.position).normalized() * scalar,
    )
}

fn scalar_gravity_force(lhs: &GravitationalBody, rhs: &GravitationalBody, gravity_const: f64) -> f64 {
    let distance = lhs.position.distance(rhs.position);
    gravity_const * (lhs.mass + rhs.mass) / (distance * distance)
}

struct PenetratingBody {
    radius: f64,
    position: Vec2f,
}

fn has_collision(lhs: &PenetratingBody, rhs: &PenetratingBody) -> bool {
    get_penetration(lhs, rhs) > 0.0
}

fn get_penetration(lhs: &PenetratingBody, rhs: &PenetratingBody) -> f64 {
    let delta_position = rhs.position - lhs.position;
    let distance = delta_position.norm();
    lhs.radius + rhs.radius - distance
}

struct CollidingBody {
    mass: f64,
    radius: f64,
    restitution: f64,
    position: Vec2f,
    velocity: Vec2f,
}

struct CollidedBody {
    position: Vec2f,
    velocity: Vec2f,
    physical_damage: f64,
}

fn collide(lhs: &CollidingBody, rhs: &CollidingBody) -> Option<(CollidedBody, CollidedBody)> {
    let delta_position = rhs.position - lhs.position;
    let distance = delta_position.norm();
    let penetration = lhs.radius + rhs.radius - distance;
    if penetration <= 0.0 {
        return None;
    }
    let mass_sum = lhs.mass + rhs.mass;
    let normal = delta_position.normalized();
    let delta_velocity = lhs.velocity - rhs.velocity;
    let lhs_kinetic_energy = get_kinetic_energy(lhs.mass, lhs.velocity);
    let rhs_kinetic_energy = get_kinetic_energy(rhs.mass, rhs.velocity);
    let lhs_velocity = lhs.velocity - delta_velocity * rhs.mass * (1.0 + lhs.restitution) / mass_sum;
    let rhs_velocity = rhs.velocity + delta_velocity * lhs.mass * (1.0 + rhs.restitution) / mass_sum;
    Some((
        CollidedBody {
            position: lhs.position - normal * (penetration * rhs.mass / mass_sum),
            velocity: lhs_velocity,
            physical_damage: (get_kinetic_energy(lhs.mass, lhs_velocity) - lhs_kinetic_energy).abs(),
        },
        CollidedBody {
            position: rhs.position + normal * (penetration * lhs.mass / mass_sum),
            velocity: rhs_velocity,
            physical_damage: (get_kinetic_energy(rhs.mass, rhs_velocity) - rhs_kinetic_energy).abs(),
        },
    ))
}

fn get_kinetic_energy(mass: f64, velocity: Vec2f) -> f64 {
    mass * velocity.dot_self() / 2.0
}
