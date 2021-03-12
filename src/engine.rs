use parry2d_f64::math::{Isometry, Real};
use parry2d_f64::na::{Point2, Vector2};
use parry2d_f64::query;
use parry2d_f64::query::{Ray, RayCast, RayIntersection};
use parry2d_f64::shape::{Ball, Shape, Triangle};

use crate::rect::Rectf;
use crate::vec2::{Square, Vec2f};
use crate::world::{Actor, Aura, Beam, Body, BoundedArea, DelayedMagick, DynamicObject,
                   Effect, Element, Field, Magick, Material, RingSector, StaticArea,
                   StaticObject, TempArea, World, WorldSettings};

#[derive(Debug, Copy, Clone)]
pub enum Index {
    Actor(usize),
    DynamicBody(usize),
    StaticBody(usize),
}

struct Spell<'a> {
    max_elements: usize,
    elements: &'a mut Vec<Element>,
}

impl<'a> Spell<'a> {
    fn on(max_elements: usize, elements: &'a mut Vec<Element>) -> Self {
        Self { max_elements, elements }
    }

    fn add(&mut self, element: Element) {
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
        if self.elements.len() < self.max_elements {
            self.elements.push(element);
        }
    }

    fn cast(&mut self) -> Magick {
        let mut power: [f64; 11] = Default::default();
        for element in self.elements.iter() {
            power[*element as usize] += 1.0;
        }
        self.elements.clear();
        Magick { power }
    }
}

#[derive(Default)]
pub struct EmittedBeam {
    pub origin: Vec2f,
    pub direction: Vec2f,
    pub length: f64,
    pub depth: usize,
    pub magick: Magick,
}

#[derive(Default)]
pub struct BeamCollider {
    initial_beams: Vec<EmittedBeam>,
    reflected_beams: Vec<EmittedBeam>,
}

impl BeamCollider {
    fn update(&mut self, world: &mut World) {
        self.initial_beams.clear();
        self.reflected_beams.clear();
        for i in 0..world.beams.len() {
            let beam = &world.beams[i];
            let actor = world.actors.iter()
                .find(|v| v.id == beam.actor_id)
                .unwrap();
            let direction = actor.current_direction;
            let origin = actor.position + direction * (actor.body.radius + world.settings.margin);
            let magick = beam.magick.clone();
            let mut length = world.settings.max_beam_length;
            if let Some(r) = intersect_beam(&magick, origin, direction, 0, &mut length, world) {
                self.reflected_beams.push(r);
            }
            self.initial_beams.push(EmittedBeam { origin, direction, length, depth: 0, magick });
        }
        let mut beam_index = 0;
        while beam_index < self.reflected_beams.len() {
            let beam = &mut self.reflected_beams[beam_index];
            let origin = beam.origin + beam.direction * world.settings.margin;
            if let Some(r) = intersect_beam(&beam.magick, origin, beam.direction, beam.depth, &mut beam.length, world) {
                beam.length += world.settings.margin;
                self.reflected_beams.push(r);
            }
            beam_index += 1;
        }
    }
}

#[derive(Default)]
pub struct Engine {
    beam_collider: BeamCollider,
}

impl Engine {
    #[cfg(feature = "render")]
    pub fn initial_emitted_beams(&self) -> &Vec<EmittedBeam> {
        &self.beam_collider.initial_beams
    }

    #[cfg(feature = "render")]
    pub fn reflected_emitted_beams(&self) -> &Vec<EmittedBeam> {
        &self.beam_collider.reflected_beams
    }

    pub fn update(&mut self, duration: f64, world: &mut World) {
        world.revision += 1;
        world.time += duration;
        world.temp_areas.retain(|v| v.effect.power.iter().sum::<f64>() > 0.0);
        let now = world.time;
        world.bounded_areas.retain(|v| v.deadline >= now);
        world.fields.retain(|v| v.deadline >= now);
        world.beams.retain(|v| v.deadline >= now);
        intersect_objects_with_areas(world);
        intersect_objects_with_all_fields(world);
        update_temp_areas(world.time, duration, &world.settings, &mut world.temp_areas);
        update_actors(world.time, duration, &world.settings, &mut world.actors);
        update_dynamic_objects(world.time, duration, &world.settings, &mut world.dynamic_objects);
        update_static_objects(world.time, duration, &world.settings, &mut world.static_objects);
        self.beam_collider.update(world);
        collide_objects(duration, world);
        world.actors.iter_mut().for_each(|v| v.dynamic_force = Vec2f::ZERO);
        world.dynamic_objects.iter_mut().for_each(|v| v.dynamic_force = Vec2f::ZERO);
        let bounds = world.bounds.clone();
        world.actors.retain(|v| is_active(&bounds, &v.body, v.position, v.health));
        world.dynamic_objects.retain(|v| {
            (v.velocity_z > f64::EPSILON || v.velocity.norm() > f64::EPSILON)
                && is_active(&bounds, &v.body, v.position, v.health)
        });
        world.static_objects.retain(|v| is_active(&bounds, &v.body, v.position, v.health));
        handle_completed_magicks(world);
    }
}

pub fn get_next_id(counter: &mut u64) -> u64 {
    let result = *counter;
    *counter += 1;
    result
}

pub fn remove_actor(actor_index: usize, world: &mut World) {
    world.actors.remove(actor_index);
}

pub fn set_actor_moving(actor_index: usize, value: bool, world: &mut World) {
    world.actors[actor_index].moving = value;
}

pub fn add_actor_spell_element(actor_index: usize, element: Element, world: &mut World) {
    Spell::on(world.settings.max_spell_elements as usize, &mut world.actors[actor_index].spell_elements).add(element);
}

pub fn start_directed_magick(actor_index: usize, world: &mut World) {
    let magick = Spell::on(world.settings.max_spell_elements as usize, &mut world.actors[actor_index].spell_elements).cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        cast_shield(magick, actor_index, world);
    } else if magick.power[Element::Earth as usize] > 0.0 {
        add_delayed_magick(magick, actor_index, world);
    } else if magick.power[Element::Ice as usize] > 0.0 {
        return;
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0 {
        add_beam(magick, actor_index, world);
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0 {
        cast_spray(world.settings.spray_angle, world.settings.directed_magick_duration,
                   &magick, actor_index, world);
    }
}

pub fn start_area_of_effect_magick(actor_index: usize, world: &mut World) {
    let magick = Spell::on(world.settings.max_spell_elements as usize,
                           &mut world.actors[actor_index].spell_elements).cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        return;
    } else if magick.power[Element::Earth as usize] > 0.0 {
        return;
    } else if magick.power[Element::Ice as usize] > 0.0 {
        return;
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0 {
        cast_spray(std::f64::consts::TAU, world.settings.area_of_effect_magick_duration,
                   &magick, actor_index, world);
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0 {
        cast_spray(std::f64::consts::TAU, world.settings.area_of_effect_magick_duration,
                   &magick, actor_index, world);
    }
}

fn cast_shield(magick: Magick, actor_index: usize, world: &mut World) {
    if magick.power[Element::Earth as usize] > 0.0 {
        cast_earth_based_shield(magick, actor_index, world);
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0 {
        return;
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0 {
        cast_spray_based_shield(magick, actor_index, world);
    }
}

fn cast_earth_based_shield(magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    for i in -2..=2 {
        world.static_objects.push(StaticObject {
            id: get_next_id(&mut world.id_counter),
            body: Body {
                radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                material: Material::Stone,
            },
            position: actor.position + actor.current_direction.rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64) * distance,
            health: 1.0,
            effect: add_magick_power_to_effect(world.time, &Effect::default(), &magick.power),
            aura: Aura::default(),
        });
    }
}

fn cast_spray_based_shield(magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    for i in -2..=2 {
        world.temp_areas.push(TempArea {
            id: get_next_id(&mut world.id_counter),
            body: Body {
                radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                material: Material::Dirt,
            },
            position: actor.position + actor.current_direction.rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64) * distance,
            effect: add_magick_power_to_effect(world.time, &Effect::default(), &magick.power),
        });
    }
}

fn add_delayed_magick(magick: Magick, actor_index: usize, world: &mut World) {
    world.actors[actor_index].delayed_magick = Some(DelayedMagick {
        actor_id: world.actors[actor_index].id,
        started: world.time,
        completed: false,
        power: magick.power,
    });
}

fn add_beam(magick: Magick, actor_index: usize, world: &mut World) {
    world.beams.push(Beam {
        id: get_next_id(&mut world.id_counter),
        actor_id: world.actors[actor_index].id,
        magick,
        deadline: world.time + world.settings.directed_magick_duration,
    });
}

pub fn complete_directed_magick(actor_index: usize, world: &mut World) {
    let actor_id = world.actors[actor_index].id;
    if remove_count(&mut world.beams, |v| v.actor_id == actor_id) > 0 {
        return;
    }
    if remove_count(&mut world.bounded_areas, |v| v.actor_id == actor_id) > 0 {
        world.fields.retain(|v| v.actor_id != actor_id);
        return;
    }
    if let Some(delayed_magick) = world.actors[actor_index].delayed_magick.as_mut() {
        delayed_magick.completed = true;
    }
}

pub fn self_magick(actor_index: usize, world: &mut World) {
    let magick = Spell::on(world.settings.max_spell_elements as usize, &mut world.actors[actor_index].spell_elements).cast();
    if magick.power[Element::Shield as usize] == 0.0 {
        world.actors[actor_index].effect = add_magick_power_to_effect(world.time, &world.actors[actor_index].effect, &magick.power);
    } else {
        let mut elements = [false; 11];
        for i in 0..magick.power.len() {
            elements[i] = magick.power[i] > 0.0;
        }
        let power = magick.power.iter().sum();
        let radius_factor = if elements[Element::Earth as usize] || elements[Element::Ice as usize]
            || (elements[Element::Shield as usize] && elements.iter().filter(|v| **v).count() == 1) {
            1.0
        } else {
            power
        };
        world.actors[actor_index].aura = Aura {
            applied: world.time,
            power,
            radius: radius_factor * world.actors[actor_index].body.radius,
            elements,
        };
    }
}

fn cast_spray(angle: f64, duration: f64, magick: &Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let effect = add_magick_power_to_effect(world.time, &Effect::default(), &magick.power);
    let body = RingSector {
        min_radius: actor.body.radius + world.settings.margin,
        max_radius: actor.body.radius * (1.0 + effect.power.iter().sum::<f64>()) * world.settings.spray_distance_factor,
        angle,
    };
    if effect.power[Element::Water as usize] == effect.power.iter().sum::<f64>() {
        world.fields.push(Field {
            id: get_next_id(&mut world.id_counter),
            actor_id: actor.id,
            body: body.clone(),
            force: world.settings.spray_force_factor * effect.power[Element::Water as usize],
            deadline: world.time + duration,
        });
    }
    world.bounded_areas.push(BoundedArea {
        id: get_next_id(&mut world.id_counter),
        actor_id: actor.id,
        body,
        effect,
        deadline: world.time + duration,
    });
}

impl Body {
    fn mass(&self) -> f64 {
        self.volume() * self.material.density()
    }

    fn volume(&self) -> f64 {
        self.radius * self.radius * self.radius * std::f64::consts::PI
    }
}

impl Material {
    fn density(self) -> f64 {
        match self {
            Material::Flesh => 800.0,
            Material::Stone => 2750.0,
            Material::Grass => 500.0,
            Material::Dirt => 1500.0,
            Material::Water => 1000.0,
        }
    }

    fn restitution(self) -> f64 {
        match self {
            Material::Flesh => 0.05,
            Material::Stone => 0.2,
            Material::Grass => 0.01,
            Material::Dirt => 0.01,
            Material::Water => 0.0,
        }
    }

    fn friction(self) -> f64 {
        match self {
            Material::Flesh => 1.0,
            Material::Stone => 1.0,
            Material::Grass => 0.5,
            Material::Dirt => 1.0,
            Material::Water => 1.0,
        }
    }
}

fn combine_elements(target: Element, element: Element) -> Option<Element> {
    if (target == Element::Water && element == Element::Fire)
        || (target == Element::Fire && element == Element::Water) {
        Some(Element::Steam)
    } else if (target == Element::Water && element == Element::Cold)
        || (target == Element::Cold && element == Element::Water) {
        Some(Element::Ice)
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

fn intersect_objects_with_areas(world: &mut World) {
    for i in 0..world.actors.len() {
        intersect_actor_with_all_bounded_areas(world.time, i, &world.bounded_areas, &mut world.actors);
        let actor = &mut world.actors[i];
        intersect_with_temp_and_static_areas(world.time, world.settings.gravitational_acceleration, &world.temp_areas, &world.static_areas, &mut IntersectingDynamicObject {
            shape: &Ball::new(actor.body.radius),
            velocity: actor.velocity,
            isometry: Isometry::translation(actor.position.x, actor.position.y),
            levitating: (actor.position_z - actor.body.radius) > f64::EPSILON,
            mass: actor.body.mass(),
            dynamic_force: &mut actor.dynamic_force,
            effect: &mut actor.effect,
        });
    }
    for object in world.dynamic_objects.iter_mut() {
        intersect_static_object_with_all_bounded_areas(world.time, &world.bounded_areas, &mut world.actors, &mut IntersectingStaticObject {
            shape: &Ball::new(object.body.radius),
            isometry: Isometry::translation(object.position.x, object.position.y),
            effect: &mut object.effect,
        });
        intersect_with_temp_and_static_areas(world.time, world.settings.gravitational_acceleration, &world.temp_areas, &world.static_areas, &mut IntersectingDynamicObject {
            shape: &Ball::new(object.body.radius),
            velocity: object.velocity,
            isometry: Isometry::translation(object.position.x, object.position.y),
            levitating: (object.position_z - object.body.radius) > f64::EPSILON,
            mass: object.body.mass(),
            dynamic_force: &mut object.dynamic_force,
            effect: &mut object.effect,
        });
    }
    for object in world.static_objects.iter_mut() {
        intersect_static_object_with_all_bounded_areas(world.time, &world.bounded_areas, &world.actors, &mut IntersectingStaticObject {
            shape: &Ball::new(object.body.radius),
            isometry: Isometry::translation(object.position.x, object.position.y),
            effect: &mut object.effect,
        });
    }
}

struct IntersectingDynamicObject<'a> {
    shape: &'a dyn Shape,
    velocity: Vec2f,
    isometry: Isometry<Real>,
    levitating: bool,
    mass: f64,
    dynamic_force: &'a mut Vec2f,
    effect: &'a mut Effect,
}

fn intersect_with_temp_and_static_areas(now: f64, gravitational_acceleration: f64, temp_areas: &[TempArea],
                                        static_areas: &[StaticArea], object: &mut IntersectingDynamicObject) {
    intersect_with_temp_areas(now, temp_areas, object);
    if !object.levitating {
        intersect_with_last_static_area(now, gravitational_acceleration, static_areas, object);
    }
}

fn intersect_with_temp_areas(now: f64, temp_areas: &[TempArea], object: &mut IntersectingDynamicObject)  {
    for temp_area in temp_areas.iter() {
        let isometry = Isometry::translation(temp_area.position.x, temp_area.position.y);
        if query::intersection_test(&object.isometry, object.shape, &isometry, &Ball::new(temp_area.body.radius)).unwrap() {
            *object.effect = add_magick_power_to_effect(now, object.effect, &temp_area.effect.power);
        }
    }
}

fn intersect_with_last_static_area(now: f64, gravitational_acceleration: f64, static_areas: &[StaticArea],
                                   object: &mut IntersectingDynamicObject) {
    if let Some(static_area) = static_areas.iter().rev()
        .find(|v| {
            let isometry = Isometry::translation(v.position.x, v.position.y);
            query::intersection_test(&object.isometry, object.shape, &isometry, &Ball::new(v.body.radius)).unwrap()
        }) {
        add_dry_friction_force(object.mass, object.velocity, static_area.body.material, gravitational_acceleration, object.dynamic_force);
        *object.effect = add_magick_power_to_effect(now, object.effect, &static_area.magick.power);
    }
}

struct IntersectingStaticObject<'a> {
    shape: &'a dyn Shape,
    isometry: Isometry<Real>,
    effect: &'a mut Effect,
}

fn intersect_actor_with_all_bounded_areas(now: f64, actor_index: usize, bounded_areas: &[BoundedArea], actors: &mut [Actor]) {
    let (left, right) = actors.split_at_mut(actor_index);
    intersect_static_object_with_all_bounded_areas(now, bounded_areas, left, &mut IntersectingStaticObject {
        shape: &Ball::new(right[0].body.radius),
        isometry: Isometry::translation(right[0].position.x, right[0].position.y),
        effect: &mut right[0].effect,
    });
    let (left, right) = actors.split_at_mut(actor_index + 1);
    intersect_static_object_with_all_bounded_areas(now, bounded_areas, right, &mut IntersectingStaticObject {
        shape: &Ball::new(left[actor_index].body.radius),
        isometry: Isometry::translation(left[actor_index].position.x, left[actor_index].position.y),
        effect: &mut left[actor_index].effect,
    });
}

fn intersect_static_object_with_all_bounded_areas(now: f64, bounded_areas: &[BoundedArea], actors: &[Actor], object: &mut IntersectingStaticObject) {
    for bounded_area in bounded_areas {
        if let Some(owner) = actors.iter().find(|v| v.id == bounded_area.actor_id) {
            intersect_static_object_with_bounded_area(now, bounded_area, owner, object);
        }
    }
}

fn intersect_static_object_with_bounded_area(now: f64, area: &BoundedArea, owner: &Actor, object: &mut IntersectingStaticObject) {
    let isometry = Isometry::translation(owner.position.x, owner.position.y);
    if intersection_test(&object.isometry, object.shape, &isometry, &area.body, owner.current_direction) {
        *object.effect = add_magick_power_to_effect(now, object.effect, &area.effect.power);
    }
}

fn intersect_objects_with_all_fields(world: &mut World) {
    for i in 0..world.actors.len() {
        let (left, right) = world.actors.split_at_mut(i);
        intersect_object_with_all_fields(&world.fields, left, &mut PushedObject {
            shape: Ball::new(right[0].body.radius),
            position: right[0].position,
            dynamic_force: &mut right[0].dynamic_force,
        });
        let (left, right) = world.actors.split_at_mut(i + 1);
        intersect_object_with_all_fields(&world.fields, right, &mut PushedObject {
            shape: Ball::new(left[i].body.radius),
            position: left[i].position,
            dynamic_force: &mut left[i].dynamic_force,
        });
    }
    for object in world.dynamic_objects.iter_mut() {
        intersect_object_with_all_fields(&world.fields, &world.actors, &mut PushedObject {
            shape: Ball::new(object.body.radius),
            position: object.position,
            dynamic_force: &mut object.dynamic_force,
        });
    }
}

struct PushedObject<'a, S: Shape> {
    shape: S,
    position: Vec2f,
    dynamic_force: &'a mut Vec2f,
}

fn intersect_object_with_all_fields<S>(fields: &[Field], actors: &[Actor], object: &mut PushedObject<S>)
    where S: Shape
{
    for field in fields {
        if let Some(owner) = actors.iter().find(|v| v.id == field.actor_id) {
            intersect_object_with_field(field, owner, object);
        }
    }
}

fn intersect_object_with_field<S>(field: &Field, owner: &Actor, object: &mut PushedObject<S>)
    where S: Shape
{
    let isometry = Isometry::translation(object.position.x, object.position.y);
    if intersection_test(&isometry, &object.shape, &owner.get_isometry(), &field.body, owner.current_direction) {
        push_object(owner.position, field.force, field.body.max_radius, object.position, object.dynamic_force);
    }
}

fn push_object(from: Vec2f, force: f64, max_distance: f64, position: Vec2f, dynamic_force: &mut Vec2f) {
    let to_position = position - from;
    *dynamic_force += to_position * ((1.0 / to_position.norm() - 1.0 / max_distance) * force);
}

fn update_temp_areas(now: f64, duration: f64, settings: &WorldSettings, temp_area_objects: &mut Vec<TempArea>) {
    for object in temp_area_objects.iter_mut() {
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
    }
}

fn update_actors(now: f64, duration: f64, settings: &WorldSettings, actors: &mut Vec<Actor>) {
    for actor in actors.iter_mut() {
        update_actor_current_direction(duration, settings.max_rotation_speed, actor);
        update_actor_dynamic_force(settings.move_force, actor);
        damage_health(duration, settings.magical_damage_factor, &actor.body, &actor.effect, &mut actor.health);
        decay_effect(now, duration, settings.decay_factor, &mut actor.effect);
        decay_aura(now, duration, settings.decay_factor, &mut actor.aura);
        update_position(duration, actor.velocity, &mut actor.position);
        update_velocity(duration, &actor.body, actor.dynamic_force, &mut actor.velocity);
        update_position_z(duration, actor.body.radius, actor.velocity_z, &mut actor.position_z);
        update_velocity_z(duration, actor.body.radius, settings.gravitational_acceleration, actor.position_z, &mut actor.velocity_z);
    }
}

fn update_dynamic_objects(now: f64, duration: f64, settings: &WorldSettings, dynamic_objects: &mut Vec<DynamicObject>) {
    for object in dynamic_objects.iter_mut() {
        damage_health(duration, settings.magical_damage_factor, &object.body, &object.effect, &mut object.health);
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
        update_position(duration, object.velocity, &mut object.position);
        update_velocity(duration, &object.body, object.dynamic_force, &mut object.velocity);
        update_position_z(duration, object.body.radius, object.velocity_z, &mut object.position_z);
        update_velocity_z(duration, object.body.radius, settings.gravitational_acceleration, object.position_z, &mut object.velocity_z);
    }
}

fn update_static_objects(now: f64, duration: f64, settings: &WorldSettings, static_objects: &mut Vec<StaticObject>) {
    for object in static_objects.iter_mut() {
        damage_health(duration, settings.magical_damage_factor, &object.body, &object.effect, &mut object.health);
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
    }
}

fn update_actor_current_direction(duration: f64, max_rotation_speed: f64, actor: &mut Actor) {
    actor.current_direction = get_current_direction(actor.current_direction, actor.target_direction, duration, max_rotation_speed);
}

fn update_actor_dynamic_force(move_force: f64, actor: &mut Actor) {
    actor.dynamic_force += actor.current_direction * move_force * actor.moving as i32 as f64;
}

fn add_dry_friction_force(mass: f64, velocity: Vec2f, surface: Material, gravitational_acceleration: f64, dynamic_force: &mut Vec2f) {
    let speed = velocity.norm();
    if speed != 0.0 {
        *dynamic_force -= velocity * (mass * surface.friction() * gravitational_acceleration / speed);
    }
}

fn decay_effect(now: f64, duration: f64, decay_factor: f64, effect: &mut Effect) {
    for i in 0..effect.power.len() {
        if is_instant_effect_element(Element::from(i)) {
            effect.power[i] = 0.0;
        } else {
            let passed = now - effect.applied[i];
            let initial = effect.power[i] - decay_factor * (passed - duration).square();
            effect.power[i] = (initial - decay_factor * passed.square()).max(0.0);
        }
    }
}

fn decay_aura(now: f64, duration: f64, decay_factor: f64, aura: &mut Aura) {
    let passed = now - aura.applied;
    let initial = aura.power - decay_factor * (passed - duration).square();
    aura.power = initial - decay_factor * passed.square();
    if aura.power <= 0.0 {
        aura.power = 0.0;
        aura.elements.fill(false);
    }
}

fn damage_health(duration: f64, damage_factor: f64, body: &Body, effect: &Effect, health: &mut f64) {
    *health = (*health - get_damage(&effect.power) * damage_factor * duration / body.mass()).min(1.0);
}

fn update_position(duration: f64, velocity: Vec2f, position: &mut Vec2f) {
    *position += velocity * duration;
}

fn update_velocity(duration: f64, body: &Body, dynamic_force: Vec2f, velocity: &mut Vec2f) {
    let dynamic_force_norm = dynamic_force.norm();
    let velocity_norm = velocity.norm();
    if dynamic_force_norm > 0.0 && velocity_norm > 0.0 {
        *velocity += (dynamic_force / dynamic_force_norm) * (dynamic_force_norm * duration / body.mass()).min(velocity_norm);
    } else {
        *velocity += dynamic_force * (duration / body.mass());
    }
}

fn update_position_z(duration: f64, height: f64, velocity_z: f64, position_z: &mut f64) {
    *position_z = height.max(*position_z + duration * velocity_z);
}

fn update_velocity_z(duration: f64, height: f64, gravitational_acceleration: f64, position_z: f64, velocity_z: &mut f64) {
    if position_z - height > f64::EPSILON {
        *velocity_z -= duration * gravitational_acceleration;
    } else {
        *velocity_z = 0.0;
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

fn get_current_direction(current_direction: Vec2f, target_direction: Vec2f, duration: f64, max_rotation_speed: f64) -> Vec2f {
    let diff = normalize_angle(target_direction.angle() - current_direction.angle());
    current_direction.rotated(diff.signum() * diff.abs().min(max_rotation_speed * duration))
}

fn normalize_angle(angle: f64) -> f64 {
    let turns = angle / std::f64::consts::TAU + 0.5;
    return (turns - turns.floor() - 0.5) * std::f64::consts::TAU;
}

fn intersect_beam(magick: &Magick, origin: Vec2f, direction: Vec2f, depth: usize, length: &mut f64, world: &mut World) -> Option<EmittedBeam> {
    let mut nearest_hit = find_beam_nearest_intersection(origin, direction, &world.actors, length)
        .map(|(i, n)| (Index::Actor(i), n));
    nearest_hit = find_beam_nearest_intersection(origin, direction, &world.dynamic_objects, length)
        .map(|(i, n)| (Index::DynamicBody(i), n)).or(nearest_hit);
    nearest_hit = find_beam_nearest_intersection(origin, direction, &world.static_objects, length)
        .map(|(i, n)| (Index::StaticBody(i), n)).or(nearest_hit);
    if let Some((index, normal)) = nearest_hit {
        let (aura, effect) = match index {
            Index::Actor(i) => {
                let object = &mut world.actors[i];
                (&object.aura, &mut object.effect)
            }
            Index::DynamicBody(i) => {
                let object = &mut world.dynamic_objects[i];
                (&object.aura, &mut object.effect)
            }
            Index::StaticBody(i) => {
                let object = &mut world.static_objects[i];
                (&object.aura, &mut object.effect)
            }
        };
        *effect = add_magick_power_to_effect(world.time, effect, &magick.power);
        if depth < world.settings.max_beam_depth as usize && can_reflect_beams(&aura.elements) {
            Some(EmittedBeam {
                origin: origin + direction * *length,
                direction: direction - normal * 2.0 * direction.cos(normal),
                length: world.settings.max_beam_length,
                depth: depth + 1,
                magick: magick.clone(),
            })
        } else {
            None
        }
    } else {
        None
    }
}

trait WithIsometry {
    fn get_isometry(&self) -> Isometry<Real>;
}

impl WithIsometry for Actor {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
    }
}

impl WithIsometry for DynamicObject {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
    }
}

impl WithIsometry for StaticObject {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
    }
}

impl RayCast for Actor {
    fn cast_local_ray_and_get_normal(&self, ray: &Ray, max_toi: f64, solid: bool) -> Option<RayIntersection> {
        Ball::new(self.body.radius).cast_local_ray_and_get_normal(&ray, max_toi, solid)
    }
}

impl RayCast for DynamicObject {
    fn cast_local_ray_and_get_normal(&self, ray: &Ray, max_toi: f64, solid: bool) -> Option<RayIntersection> {
        Ball::new(self.body.radius).cast_local_ray_and_get_normal(&ray, max_toi, solid)
    }
}

impl RayCast for StaticObject {
    fn cast_local_ray_and_get_normal(&self, ray: &Ray, max_toi: f64, solid: bool) -> Option<RayIntersection> {
        Ball::new(self.body.radius).cast_local_ray_and_get_normal(&ray, max_toi, solid)
    }
}

fn find_beam_nearest_intersection<T>(origin: Vec2f, direction: Vec2f, objects: &[T], length: &mut f64) -> Option<(usize, Vec2f)>
    where T: WithIsometry + RayCast
{
    let mut nearest = None;
    for i in 0..objects.len() {
        let result = objects[i].cast_ray_and_get_normal(
            &objects[i].get_isometry(),
            &Ray::new(Point2::new(origin.x, origin.y), Vector2::new(direction.x, direction.y)),
            *length,
            true,
        );
        if let Some(intersection) = result {
            *length = intersection.toi;
            nearest = Some((i, Vec2f::new(intersection.normal.x, intersection.normal.y)));
        }
    }
    nearest
}

fn collide_objects(duration: f64, world: &mut World) {
    let time = world.time;
    let physical_damage_factor = world.settings.physical_damage_factor;
    for static_object in world.static_objects.iter_mut() {
        let mut static_object = StaticCollidingObject {
            shape: &Ball::new(static_object.body.radius),
            material: static_object.body.material,
            mass: static_object.body.mass(),
            isometry: &static_object.get_isometry(),
            effect: &mut static_object.effect,
            health: &mut static_object.health,
            aura: &static_object.aura,
        };
        for actor in world.actors.iter_mut() {
            let mut actor_object = DynamicCollidingObject {
                shape: &Ball::new(actor.body.radius),
                material: actor.body.material,
                mass: actor.body.mass(),
                position: &mut actor.position,
                velocity: &mut actor.velocity,
                effect: &mut actor.effect,
                health: &mut actor.health,
                aura: &actor.aura,
            };
            collide_dynamic_and_static_objects(
                time, duration, physical_damage_factor,
                &mut actor_object, &mut static_object,
            );
        }
        for dynamic_object in world.dynamic_objects.iter_mut() {
            let mut dynamic_object = DynamicCollidingObject {
                shape: &Ball::new(dynamic_object.body.radius),
                material: dynamic_object.body.material,
                mass: dynamic_object.body.mass(),
                position: &mut dynamic_object.position,
                velocity: &mut dynamic_object.velocity,
                effect: &mut dynamic_object.effect,
                health: &mut dynamic_object.health,
                aura: &dynamic_object.aura,
            };
            collide_dynamic_and_static_objects(
                time, duration, physical_damage_factor,
                &mut dynamic_object, &mut static_object,
            );
        }
    }
    if !world.actors.is_empty() {
        for i in 0..world.actors.len() - 1 {
            for j in i + 1..world.actors.len() {
                let (left, right) = world.actors.split_at_mut(j);
                collide_dynamic_objects(
                    world.time, duration, world.settings.physical_damage_factor,
                    DynamicCollidingObject {
                        shape: &Ball::new(left[i].body.radius),
                        material: left[i].body.material,
                        mass: left[i].body.mass(),
                        position: &mut left[i].position,
                        velocity: &mut left[i].velocity,
                        effect: &mut left[i].effect,
                        health: &mut left[i].health,
                        aura: &left[i].aura,
                    },
                    DynamicCollidingObject {
                        shape: &Ball::new(right[0].body.radius),
                        material: right[0].body.material,
                        mass: right[0].body.mass(),
                        position: &mut right[0].position,
                        velocity: &mut right[0].velocity,
                        effect: &mut right[0].effect,
                        health: &mut right[0].health,
                        aura: &right[0].aura,
                    },
                );
            }
        }
    }
    for dynamic_object in world.dynamic_objects.iter_mut() {
        for actor in world.actors.iter_mut() {
            collide_dynamic_objects(
                world.time, duration, world.settings.physical_damage_factor,
                DynamicCollidingObject {
                    shape: &Ball::new(dynamic_object.body.radius),
                    material: dynamic_object.body.material,
                    mass: dynamic_object.body.mass(),
                    position: &mut dynamic_object.position,
                    velocity: &mut dynamic_object.velocity,
                    effect: &mut dynamic_object.effect,
                    health: &mut dynamic_object.health,
                    aura: &dynamic_object.aura,
                },
                DynamicCollidingObject {
                    shape: &Ball::new(actor.body.radius),
                    material: actor.body.material,
                    mass: actor.body.mass(),
                    position: &mut actor.position,
                    velocity: &mut actor.velocity,
                    effect: &mut actor.effect,
                    health: &mut actor.health,
                    aura: &actor.aura,
                },
            );
        }
    }
    if !world.dynamic_objects.is_empty() {
        for i in 0..world.dynamic_objects.len() - 1 {
            for j in i + 1..world.dynamic_objects.len() {
                let (left, right) = world.dynamic_objects.split_at_mut(j);
                collide_dynamic_objects(
                    world.time, duration, world.settings.physical_damage_factor,
                    DynamicCollidingObject {
                        shape: &Ball::new(left[i].body.radius),
                        material: left[i].body.material,
                        mass: left[i].body.mass(),
                        position: &mut left[i].position,
                        velocity: &mut left[i].velocity,
                        effect: &mut left[i].effect,
                        health: &mut left[i].health,
                        aura: &left[i].aura,
                    },
                    DynamicCollidingObject {
                        shape: &Ball::new(right[0].body.radius),
                        material: right[0].body.material,
                        mass: right[0].body.mass(),
                        position: &mut right[0].position,
                        velocity: &mut right[0].velocity,
                        effect: &mut right[0].effect,
                        health: &mut right[0].health,
                        aura: &right[0].aura,
                    },
                );
            }
        }
    }
}

struct DynamicCollidingObject<'a> {
    shape: &'a dyn Shape,
    material: Material,
    mass: f64,
    position: &'a mut Vec2f,
    velocity: &'a mut Vec2f,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

fn collide_dynamic_objects(now: f64, duration: f64, damage_factor: f64,
                           mut lhs: DynamicCollidingObject, mut rhs: DynamicCollidingObject) {
    let collision = query::time_of_impact(
        &Isometry::translation(lhs.position.x, lhs.position.y),
        &Vector2::new(lhs.velocity.x, lhs.velocity.y),
        lhs.shape,
        &Isometry::translation(rhs.position.x, rhs.position.y),
        &Vector2::new(rhs.velocity.x, rhs.velocity.y),
        rhs.shape,
        duration,
    ).unwrap();
    if let Some(collision) = collision {
        let lhs_kinetic_energy = get_kinetic_energy(lhs.mass, *lhs.velocity);
        let rhs_kinetic_energy = get_kinetic_energy(rhs.mass, *rhs.velocity);
        let delta_velocity = *lhs.velocity - *rhs.velocity;
        let mass_sum = lhs.mass + rhs.mass;
        if let Some(contact) = query::contact(
            &Isometry::translation(lhs.position.x, lhs.position.y),
            lhs.shape,
            &Isometry::translation(rhs.position.x, rhs.position.y),
            rhs.shape,
            0.0,
        ).unwrap() {
            *lhs.position += Vec2f::new(contact.normal1.x, contact.normal1.y) * (contact.dist * rhs.mass / mass_sum);
            *rhs.position += Vec2f::new(contact.normal2.x, contact.normal2.y) * (contact.dist * lhs.mass / mass_sum);
        } else {
            *lhs.position += *lhs.velocity * collision.toi;
            *rhs.position -= *rhs.velocity * collision.toi;
        }
        let lhs_velocity = *lhs.velocity - delta_velocity * rhs.mass * (1.0 + lhs.material.restitution()) / mass_sum;
        let rhs_velocity = *rhs.velocity + delta_velocity * lhs.mass * (1.0 + rhs.material.restitution()) / mass_sum;
        *lhs.velocity = lhs_velocity;
        *rhs.velocity = rhs_velocity;
        let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
        let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
        *lhs.effect = new_lhs_effect;
        *rhs.effect = new_rhs_effect;
        handle_dynamic_object_collision_damage(lhs_kinetic_energy, damage_factor, &mut lhs);
        handle_dynamic_object_collision_damage(rhs_kinetic_energy, damage_factor, &mut rhs);
    }
}

struct StaticCollidingObject<'a> {
    shape: &'a dyn Shape,
    material: Material,
    mass: f64,
    isometry: &'a Isometry<Real>,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

fn collide_dynamic_and_static_objects(now: f64, duration: f64, damage_factor: f64,
                                      lhs: &mut DynamicCollidingObject, rhs: &mut StaticCollidingObject) {
    let collision = query::time_of_impact(
        &Isometry::translation(lhs.position.x, lhs.position.y),
        &Vector2::new(lhs.velocity.x, lhs.velocity.y),
        lhs.shape,
        rhs.isometry,
        &Vector2::new(0.0, 0.0),
        rhs.shape,
        duration,
    ).unwrap();
    if let Some(collision) = collision {
        let mass_sum = lhs.mass + rhs.mass;
        if let Some(contact) = query::contact(
            &Isometry::translation(lhs.position.x, lhs.position.y),
            lhs.shape,
            rhs.isometry,
            rhs.shape,
            0.0,
        ).unwrap() {
            *lhs.position += Vec2f::new(contact.normal1.x, contact.normal1.y) * contact.dist;
        } else {
            *lhs.position += *lhs.velocity * collision.toi;
        }
        let delta_velocity = *lhs.velocity;
        let lhs_kinetic_energy = get_kinetic_energy(lhs.mass, *lhs.velocity);
        let lhs_velocity = *lhs.velocity - delta_velocity * rhs.mass * (1.0 + lhs.material.restitution()) / mass_sum;
        *lhs.velocity = lhs_velocity;
        let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
        let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
        *lhs.effect = new_lhs_effect;
        *rhs.effect = new_rhs_effect;
        handle_dynamic_object_collision_damage(lhs_kinetic_energy, damage_factor, lhs);
        if !can_absorb_physical_damage(&rhs.aura.elements) {
            let rhs_velocity = delta_velocity * lhs.mass * (1.0 + rhs.material.restitution()) / mass_sum;
            *rhs.health -= get_kinetic_energy(rhs.mass, rhs_velocity) * damage_factor / rhs.mass;
        }
    }
}

fn handle_dynamic_object_collision_damage(prev_kinetic_energy: f64, damage_factor: f64, object: &mut DynamicCollidingObject) {
    if !can_absorb_physical_damage(&object.aura.elements) {
        *object.health -= (get_kinetic_energy(object.mass, *object.velocity) - prev_kinetic_energy).abs() * damage_factor / object.mass;
    }
}

fn get_kinetic_energy(mass: f64, velocity: Vec2f) -> f64 {
    mass * velocity.dot_self() / 2.0
}

fn is_active(bounds: &Rectf, body: &Body, position: Vec2f, health: f64) -> bool {
    let rect = Rectf::new(
        position - Vec2f::both(body.radius),
        position + Vec2f::both(body.radius),
    );
    health > 0.0 && bounds.overlaps(&rect)
}

fn handle_completed_magicks(world: &mut World) {
    for actor in world.actors.iter_mut() {
        if actor.delayed_magick.as_ref().map(|v| v.completed).unwrap_or(false) {
            let delayed_magick = actor.delayed_magick.take().unwrap();
            let radius = delayed_magick.power.iter().sum::<f64>() * actor.body.radius / world.settings.max_magic_power;
            let material = Material::Stone;
            world.dynamic_objects.push(DynamicObject {
                id: get_next_id(&mut world.id_counter),
                body: Body { radius, material },
                position: actor.position
                    + actor.current_direction * (actor.body.radius + radius + world.settings.margin),
                health: 1.0,
                effect: Effect {
                    applied: [world.time; 11],
                    power: delayed_magick.power.clone(),
                },
                aura: Default::default(),
                velocity: actor.velocity,
                dynamic_force: actor.current_direction * (
                    (world.time - delayed_magick.started).min(world.settings.max_magic_power)
                        * world.settings.magic_force_multiplier
                ),
                position_z: 1.5 * actor.body.radius,
                velocity_z: 0.0,
            });
        }
    }
}

fn remove_count<T, F>(vec: &mut Vec<T>, mut f: F) -> usize
    where F: FnMut(&T) -> bool
{
    let mut removed = 0;
    vec.retain(|v| {
        let retain = !f(v);
        removed += !retain as usize;
        retain
    });
    removed
}

fn intersection_test(shape_pos: &Isometry<Real>, shape: &dyn Shape, body_pos: &Isometry<Real>, body: &RingSector, direction: Vec2f) -> bool {
    if query::intersection_test(shape_pos, shape, body_pos, &Ball::new(body.min_radius)).unwrap()
        || !query::intersection_test(shape_pos, shape, body_pos, &Ball::new(body.max_radius)).unwrap() {
        return false;
    }
    if body.angle == std::f64::consts::TAU {
        return true;
    }
    let radius = direction * body.max_radius;
    let left = radius.rotated(body.angle / 2.0);
    let right = radius.rotated(-body.angle / 2.0);
    let triangle = Triangle::new(Point2::new(0.0, 0.0), Point2::new(left.x, left.y), Point2::new(right.x, right.y));
    query::intersection_test(shape_pos, shape, body_pos, &triangle).unwrap()
}

#[cfg(test)]
mod tests {
    use crate::engine::remove_count;

    #[test]
    fn remove_count_should_return_number_of_removed_items() {
        assert_eq!(remove_count(&mut vec![1, 2, 3, 2, 1], |v| *v == 2), 2);
    }

    #[test]
    fn remove_count_should_return_zero_when_nothing_is_removed() {
        assert_eq!(remove_count(&mut vec![1, 2, 3, 2, 1], |v| *v == 4), 0);
    }

    #[test]
    fn remove_count_should_remove_items_matching_predicate_preserving_order() {
        let mut values = vec![1, 2, 3, 2, 1];
        remove_count(&mut values, |v| *v == 2);
        assert_eq!(values, &[1, 3, 1]);
    }
}
