use std::cell::RefCell;

use parry2d_f64::math::{Isometry, Real};
use parry2d_f64::na::{Point2, Vector2};
use parry2d_f64::query;
use parry2d_f64::query::{Contact, Ray, TOI};
use parry2d_f64::shape::{Ball, Cuboid, Polyline, Shape, Triangle};

use crate::rect::Rectf;
use crate::vec2::{Square, Vec2f};
use crate::world::{
    Actor, Aura, Beam, Body, BoundedArea, CircleArc, DelayedMagick, Disk, DynamicObject, Effect,
    Element, Field, Magick, Material, RingSector, StaticArea, StaticObject, StaticShape, TempArea,
    World, WorldSettings,
};

const RESOLUTION_FACTOR: f64 = 4.0;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum Index {
    Actor(usize),
    DynamicObject(usize),
    StaticObject(usize),
}

struct Spell<'a> {
    max_elements: usize,
    elements: &'a mut Vec<Element>,
}

impl<'a> Spell<'a> {
    fn on(max_elements: usize, elements: &'a mut Vec<Element>) -> Self {
        Self {
            max_elements,
            elements,
        }
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
    fn update(&mut self, world: &mut World, shape_cache: &ShapeCache) {
        self.initial_beams.clear();
        self.reflected_beams.clear();
        for i in 0..world.beams.len() {
            let beam = &world.beams[i];
            let actor = world.actors.iter().find(|v| v.id == beam.actor_id).unwrap();
            let direction = actor.current_direction;
            let origin =
                actor.position + direction * (actor.body.shape.radius + world.settings.margin);
            let magick = beam.magick.clone();
            let mut length = world.settings.max_beam_length;
            if let Some(r) = intersect_beam(
                &magick,
                origin,
                direction,
                0,
                &mut length,
                world,
                shape_cache,
            ) {
                self.reflected_beams.push(r);
            }
            self.initial_beams.push(EmittedBeam {
                origin,
                direction,
                length,
                depth: 0,
                magick,
            });
        }
        let mut beam_index = 0;
        while beam_index < self.reflected_beams.len() {
            let beam = &mut self.reflected_beams[beam_index];
            let origin = beam.origin + beam.direction * world.settings.margin;
            if let Some(r) = intersect_beam(
                &beam.magick,
                origin,
                beam.direction,
                beam.depth,
                &mut beam.length,
                world,
                shape_cache,
            ) {
                beam.length += world.settings.margin;
                self.reflected_beams.push(r);
            }
            beam_index += 1;
        }
    }
}

#[derive(Clone, PartialEq)]
struct CircleArcKey {
    radius: f64,
    length: f64,
}

#[derive(Default)]
struct ShapeCache {
    polylines: RefCell<Vec<(CircleArcKey, Polyline)>>,
}

impl ShapeCache {
    fn with<R, F>(&self, key: &CircleArcKey, mut f: F) -> R
    where
        F: FnMut(&dyn Shape) -> R,
    {
        if let Some((_, v)) = self.polylines.borrow().iter().find(|(k, _)| k == key) {
            return f(v);
        }
        let polyline = make_circle_arc_polyline(key);
        let r = f(&polyline);
        self.polylines.borrow_mut().push((key.clone(), polyline));
        r
    }
}

#[derive(Default)]
pub struct Engine {
    beam_collider: BeamCollider,
    shape_cache: ShapeCache,
}

impl Engine {
    #[cfg(feature = "client")]
    pub fn initial_emitted_beams(&self) -> &Vec<EmittedBeam> {
        &self.beam_collider.initial_beams
    }

    #[cfg(feature = "client")]
    pub fn reflected_emitted_beams(&self) -> &Vec<EmittedBeam> {
        &self.beam_collider.reflected_beams
    }

    pub fn update(&mut self, duration: f64, world: &mut World) {
        world.frame += 1;
        world.time += duration;
        world
            .temp_areas
            .retain(|v| v.effect.power.iter().sum::<f64>() > 0.0);
        let now = world.time;
        world.bounded_areas.retain(|v| v.deadline >= now);
        world.fields.retain(|v| v.deadline >= now);
        world.beams.retain(|v| v.deadline >= now);
        intersect_objects_with_areas(world, &self.shape_cache);
        intersect_objects_with_all_fields(world);
        update_temp_areas(world.time, duration, &world.settings, &mut world.temp_areas);
        update_actors(world.time, duration, &world.settings, &mut world.actors);
        update_dynamic_objects(
            world.time,
            duration,
            &world.settings,
            &mut world.dynamic_objects,
        );
        update_static_objects(
            world.time,
            duration,
            &world.settings,
            &mut world.static_objects,
        );
        self.update_beams(world);
        move_objects(duration, world, &self.shape_cache);
        world
            .actors
            .iter_mut()
            .for_each(|v| v.dynamic_force = Vec2f::ZERO);
        world
            .dynamic_objects
            .iter_mut()
            .for_each(|v| v.dynamic_force = Vec2f::ZERO);
        let bounds = world.bounds.clone();
        world.actors.retain(|v| {
            v.active && is_active(&bounds, &v.body.shape.as_shape(), v.position, v.health)
        });
        world.dynamic_objects.retain(|v| {
            (v.velocity_z > f64::EPSILON || v.velocity.norm() > f64::EPSILON)
                && is_active(&bounds, &v.body.shape.as_shape(), v.position, v.health)
        });
        world.static_objects.retain(|v| {
            v.aura.power > 0.0
                || v.body.shape.with_shape(&self.shape_cache, |shape| {
                    is_active(&bounds, shape, v.position, v.health)
                })
        });
        handle_completed_magicks(world);
    }

    #[cfg(feature = "client")]
    pub fn update_visual(&mut self, world: &mut World) {
        self.update_beams(world);
    }

    fn update_beams(&mut self, world: &mut World) {
        self.beam_collider.update(world, &self.shape_cache);
    }
}

pub fn get_next_id(counter: &mut u64) -> u64 {
    let result = *counter;
    *counter += 1;
    result
}

#[cfg(feature = "server")]
pub fn remove_actor(actor_index: usize, world: &mut World) {
    world.actors[actor_index].active = false;
}

pub fn add_actor_spell_element(actor_index: usize, element: Element, world: &mut World) {
    Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .add(element);
}

#[allow(clippy::needless_return)]
pub fn start_directed_magick(actor_index: usize, world: &mut World) {
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        cast_shield(magick, actor_index, world);
    } else if magick.power[Element::Earth as usize] > 0.0 {
        add_delayed_magick(magick, actor_index, world);
    } else if magick.power[Element::Ice as usize] > 0.0 {
        return;
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0
    {
        add_beam(magick, actor_index, world);
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0
    {
        cast_spray(
            world.settings.spray_angle,
            world.settings.directed_magick_duration,
            &magick,
            actor_index,
            world,
        );
    }
}

#[allow(clippy::needless_return)]
#[allow(clippy::if_same_then_else)]
pub fn start_area_of_effect_magick(actor_index: usize, world: &mut World) {
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        return;
    } else if magick.power[Element::Earth as usize] > 0.0 {
        return;
    } else if magick.power[Element::Ice as usize] > 0.0 {
        return;
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0
    {
        cast_spray(
            std::f64::consts::TAU,
            world.settings.area_of_effect_magick_duration,
            &magick,
            actor_index,
            world,
        );
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0
    {
        cast_spray(
            std::f64::consts::TAU,
            world.settings.area_of_effect_magick_duration,
            &magick,
            actor_index,
            world,
        );
    }
}

#[allow(clippy::needless_return)]
#[allow(clippy::if_same_then_else)]
fn cast_shield(magick: Magick, actor_index: usize, world: &mut World) {
    if magick.power[Element::Earth as usize] > 0.0 {
        cast_earth_based_shield(magick, actor_index, world);
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0
    {
        return;
    } else if magick.power[Element::Lightning as usize] > 0.0 {
        return;
    } else if magick.power[Element::Water as usize] > 0.0
        || magick.power[Element::Cold as usize] > 0.0
        || magick.power[Element::Fire as usize] > 0.0
        || magick.power[Element::Steam as usize] > 0.0
        || magick.power[Element::Poison as usize] > 0.0
    {
        cast_spray_based_shield(magick, actor_index, world);
    } else {
        cast_reflecting_shield(std::f64::consts::FRAC_PI_2, actor_index, world);
    }
}

fn cast_earth_based_shield(magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    for i in -2..=2 {
        world.static_objects.push(StaticObject {
            id: get_next_id(&mut world.id_counter),
            body: Body {
                shape: StaticShape::Disk(Disk {
                    radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                }),
                material: Material::Stone,
            },
            position: actor.position
                + actor
                    .current_direction
                    .rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64)
                    * distance,
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
                shape: Disk {
                    radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                },
                material: Material::Dirt,
            },
            position: actor.position
                + actor
                    .current_direction
                    .rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64)
                    * distance,
            effect: add_magick_power_to_effect(world.time, &Effect::default(), &magick.power),
        });
    }
}

fn cast_reflecting_shield(length: f64, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let radius = 5.0;
    let mut elements = [false; 11];
    elements[Element::Shield as usize] = true;
    world.static_objects.push(StaticObject {
        id: get_next_id(&mut world.id_counter),
        body: Body {
            shape: StaticShape::CircleArc(CircleArc {
                radius,
                length,
                rotation: normalize_angle(actor.current_direction.angle()),
            }),
            material: Material::None,
        },
        position: actor.position,
        health: 0.0,
        effect: Effect::default(),
        aura: Aura {
            applied: world.time,
            power: 5.0,
            radius: 0.0,
            elements,
        },
    });
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
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] == 0.0 {
        world.actors[actor_index].effect = add_magick_power_to_effect(
            world.time,
            &world.actors[actor_index].effect,
            &magick.power,
        );
    } else {
        let mut elements = [false; 11];
        elements
            .iter_mut()
            .zip(magick.power.iter())
            .for_each(|(e, p)| *e = *p > 0.0);
        let power = magick.power.iter().sum();
        let radius_factor = if elements[Element::Earth as usize]
            || elements[Element::Ice as usize]
            || (elements[Element::Shield as usize] && elements.iter().filter(|v| **v).count() == 1)
        {
            1.0
        } else {
            power
        };
        world.actors[actor_index].aura = Aura {
            applied: world.time,
            power,
            radius: radius_factor * world.actors[actor_index].body.shape.radius,
            elements,
        };
    }
}

fn cast_spray(angle: f64, duration: f64, magick: &Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let effect = add_magick_power_to_effect(world.time, &Effect::default(), &magick.power);
    let body = RingSector {
        min_radius: actor.body.shape.radius + world.settings.margin,
        max_radius: actor.body.shape.radius
            * (1.0 + effect.power.iter().sum::<f64>())
            * world.settings.spray_distance_factor,
        angle,
    };
    if (effect.power[Element::Water as usize] - effect.power.iter().sum::<f64>()).abs()
        <= f64::EPSILON
    {
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

trait WithMass {
    fn mass(&self) -> f64;
}

impl<Shape: WithVolume> WithMass for Body<Shape> {
    fn mass(&self) -> f64 {
        self.shape.volume() * self.material.density()
    }
}

trait WithVolume {
    fn volume(&self) -> f64;
}

impl WithVolume for StaticShape {
    fn volume(&self) -> f64 {
        match self {
            StaticShape::CircleArc(v) => v.volume(),
            StaticShape::Disk(v) => v.volume(),
        }
    }
}

impl WithVolume for Disk {
    fn volume(&self) -> f64 {
        self.radius * self.radius * self.radius * std::f64::consts::PI
    }
}

impl WithVolume for CircleArc {
    fn volume(&self) -> f64 {
        self.radius * self.length * std::f64::consts::PI
    }
}

impl StaticShape {
    fn with_shape<R, F: FnMut(&dyn Shape) -> R>(&self, cache: &ShapeCache, mut f: F) -> R {
        match &self {
            StaticShape::CircleArc(arc) => cache.with(
                &CircleArcKey {
                    radius: arc.radius,
                    length: arc.length,
                },
                f,
            ),
            StaticShape::Disk(disk) => f(&disk.as_shape()),
        }
    }
}

impl Disk {
    fn as_shape(&self) -> Ball {
        Ball::new(self.radius)
    }
}

impl Material {
    fn density(self) -> f64 {
        match self {
            Material::None => 1.0,
            Material::Flesh => 800.0,
            Material::Stone => 2750.0,
            Material::Grass => 500.0,
            Material::Dirt => 1500.0,
            Material::Water => 1000.0,
        }
    }

    fn restitution(self) -> f64 {
        match self {
            Material::None => 1.0,
            Material::Flesh => 0.05,
            Material::Stone => 0.2,
            Material::Grass => 0.01,
            Material::Dirt => 0.01,
            Material::Water => 0.0,
        }
    }

    fn friction(self) -> f64 {
        match self {
            Material::None => 0.0,
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
        || (target == Element::Fire && element == Element::Water)
    {
        Some(Element::Steam)
    } else if (target == Element::Water && element == Element::Cold)
        || (target == Element::Cold && element == Element::Water)
    {
        Some(Element::Ice)
    } else if target == Element::Ice && element == Element::Fire {
        Some(Element::Water)
    } else {
        None
    }
}

fn can_cancel_element(target: Element, element: Element) -> bool {
    (target == Element::Water && element == Element::Lightning)
        || (target == Element::Lightning
            && (element == Element::Earth || element == Element::Water))
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

fn intersect_objects_with_areas(world: &mut World, shape_cache: &ShapeCache) {
    for i in 0..world.actors.len() {
        intersect_actor_with_all_bounded_areas(
            world.time,
            i,
            &world.bounded_areas,
            &mut world.actors,
        );
        let actor = &mut world.actors[i];
        intersect_with_temp_and_static_areas(
            world.time,
            world.settings.gravitational_acceleration,
            &world.temp_areas,
            &world.static_areas,
            &mut IntersectingDynamicObject {
                shape: &Ball::new(actor.body.shape.radius),
                velocity: actor.velocity,
                isometry: Isometry::translation(actor.position.x, actor.position.y),
                levitating: (actor.position_z - actor.body.shape.radius) > f64::EPSILON,
                mass: actor.body.mass(),
                dynamic_force: &mut actor.dynamic_force,
                effect: &mut actor.effect,
            },
        );
    }
    for object in world.dynamic_objects.iter_mut() {
        intersect_static_object_with_all_bounded_areas(
            world.time,
            &world.bounded_areas,
            &world.actors,
            &mut IntersectingStaticObject {
                shape: &Ball::new(object.body.shape.radius),
                isometry: Isometry::translation(object.position.x, object.position.y),
                effect: &mut object.effect,
            },
        );
        intersect_with_temp_and_static_areas(
            world.time,
            world.settings.gravitational_acceleration,
            &world.temp_areas,
            &world.static_areas,
            &mut IntersectingDynamicObject {
                shape: &Ball::new(object.body.shape.radius),
                velocity: object.velocity,
                isometry: Isometry::translation(object.position.x, object.position.y),
                levitating: (object.position_z - object.body.shape.radius) > f64::EPSILON,
                mass: object.body.mass(),
                dynamic_force: &mut object.dynamic_force,
                effect: &mut object.effect,
            },
        );
    }
    for i in 0..world.static_objects.len() {
        world.static_objects[i]
            .body
            .shape
            .clone()
            .with_shape(shape_cache, |shape| {
                let object = &mut world.static_objects[i];
                intersect_static_object_with_all_bounded_areas(
                    world.time,
                    &world.bounded_areas,
                    &world.actors,
                    &mut IntersectingStaticObject {
                        shape,
                        isometry: Isometry::translation(object.position.x, object.position.y),
                        effect: &mut object.effect,
                    },
                );
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

fn intersect_with_temp_and_static_areas(
    now: f64,
    gravitational_acceleration: f64,
    temp_areas: &[TempArea],
    static_areas: &[StaticArea],
    object: &mut IntersectingDynamicObject,
) {
    intersect_with_temp_areas(now, temp_areas, object);
    if !object.levitating {
        intersect_with_last_static_area(now, gravitational_acceleration, static_areas, object);
    }
}

fn intersect_with_temp_areas(
    now: f64,
    temp_areas: &[TempArea],
    object: &mut IntersectingDynamicObject,
) {
    for temp_area in temp_areas.iter() {
        let isometry = Isometry::translation(temp_area.position.x, temp_area.position.y);
        if query::intersection_test(
            &object.isometry,
            object.shape,
            &isometry,
            &Ball::new(temp_area.body.shape.radius),
        )
        .unwrap()
        {
            *object.effect =
                add_magick_power_to_effect(now, object.effect, &temp_area.effect.power);
        }
    }
}

fn intersect_with_last_static_area(
    now: f64,
    gravitational_acceleration: f64,
    static_areas: &[StaticArea],
    object: &mut IntersectingDynamicObject,
) {
    if let Some(static_area) = static_areas.iter().rev().find(|v| {
        let isometry = Isometry::translation(v.position.x, v.position.y);
        query::intersection_test(
            &object.isometry,
            object.shape,
            &isometry,
            &Ball::new(v.body.shape.radius),
        )
        .unwrap()
    }) {
        add_dry_friction_force(
            object.mass,
            object.velocity,
            static_area.body.material,
            gravitational_acceleration,
            object.dynamic_force,
        );
        *object.effect = add_magick_power_to_effect(now, object.effect, &static_area.magick.power);
    }
}

struct IntersectingStaticObject<'a> {
    shape: &'a dyn Shape,
    isometry: Isometry<Real>,
    effect: &'a mut Effect,
}

fn intersect_actor_with_all_bounded_areas(
    now: f64,
    actor_index: usize,
    bounded_areas: &[BoundedArea],
    actors: &mut [Actor],
) {
    let (left, right) = actors.split_at_mut(actor_index);
    intersect_static_object_with_all_bounded_areas(
        now,
        bounded_areas,
        left,
        &mut IntersectingStaticObject {
            shape: &Ball::new(right[0].body.shape.radius),
            isometry: Isometry::translation(right[0].position.x, right[0].position.y),
            effect: &mut right[0].effect,
        },
    );
    let (left, right) = actors.split_at_mut(actor_index + 1);
    intersect_static_object_with_all_bounded_areas(
        now,
        bounded_areas,
        right,
        &mut IntersectingStaticObject {
            shape: &Ball::new(left[actor_index].body.shape.radius),
            isometry: Isometry::translation(
                left[actor_index].position.x,
                left[actor_index].position.y,
            ),
            effect: &mut left[actor_index].effect,
        },
    );
}

fn intersect_static_object_with_all_bounded_areas(
    now: f64,
    bounded_areas: &[BoundedArea],
    actors: &[Actor],
    object: &mut IntersectingStaticObject,
) {
    for bounded_area in bounded_areas {
        if let Some(owner) = actors.iter().find(|v| v.id == bounded_area.actor_id) {
            intersect_static_object_with_bounded_area(now, bounded_area, owner, object);
        }
    }
}

fn intersect_static_object_with_bounded_area(
    now: f64,
    area: &BoundedArea,
    owner: &Actor,
    object: &mut IntersectingStaticObject,
) {
    let isometry = Isometry::translation(owner.position.x, owner.position.y);
    if intersection_test(
        &object.isometry,
        object.shape,
        &isometry,
        &area.body,
        owner.current_direction,
    ) {
        *object.effect = add_magick_power_to_effect(now, object.effect, &area.effect.power);
    }
}

fn intersect_objects_with_all_fields(world: &mut World) {
    for i in 0..world.actors.len() {
        let (left, right) = world.actors.split_at_mut(i);
        intersect_object_with_all_fields(
            &world.fields,
            left,
            &mut PushedObject {
                shape: Ball::new(right[0].body.shape.radius),
                position: right[0].position,
                dynamic_force: &mut right[0].dynamic_force,
            },
        );
        let (left, right) = world.actors.split_at_mut(i + 1);
        intersect_object_with_all_fields(
            &world.fields,
            right,
            &mut PushedObject {
                shape: Ball::new(left[i].body.shape.radius),
                position: left[i].position,
                dynamic_force: &mut left[i].dynamic_force,
            },
        );
    }
    for object in world.dynamic_objects.iter_mut() {
        intersect_object_with_all_fields(
            &world.fields,
            &world.actors,
            &mut PushedObject {
                shape: Ball::new(object.body.shape.radius),
                position: object.position,
                dynamic_force: &mut object.dynamic_force,
            },
        );
    }
}

struct PushedObject<'a, S: Shape> {
    shape: S,
    position: Vec2f,
    dynamic_force: &'a mut Vec2f,
}

fn intersect_object_with_all_fields<S>(
    fields: &[Field],
    actors: &[Actor],
    object: &mut PushedObject<S>,
) where
    S: Shape,
{
    for field in fields {
        if let Some(owner) = actors.iter().find(|v| v.id == field.actor_id) {
            intersect_object_with_field(field, owner, object);
        }
    }
}

fn intersect_object_with_field<S>(field: &Field, owner: &Actor, object: &mut PushedObject<S>)
where
    S: Shape,
{
    let isometry = Isometry::translation(object.position.x, object.position.y);
    if intersection_test(
        &isometry,
        &object.shape,
        &owner.get_isometry(),
        &field.body,
        owner.current_direction,
    ) {
        push_object(
            owner.position,
            field.force,
            field.body.max_radius,
            object.position,
            object.dynamic_force,
        );
    }
}

fn push_object(
    from: Vec2f,
    force: f64,
    max_distance: f64,
    position: Vec2f,
    dynamic_force: &mut Vec2f,
) {
    let to_position = position - from;
    *dynamic_force += to_position * ((1.0 / to_position.norm() - 1.0 / max_distance) * force);
}

fn update_temp_areas(
    now: f64,
    duration: f64,
    settings: &WorldSettings,
    temp_area_objects: &mut Vec<TempArea>,
) {
    for object in temp_area_objects.iter_mut() {
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
    }
}

fn update_actors(now: f64, duration: f64, settings: &WorldSettings, actors: &mut Vec<Actor>) {
    for actor in actors.iter_mut() {
        update_actor_current_direction(duration, settings.max_rotation_speed, actor);
        update_actor_dynamic_force(settings.move_force, actor);
        damage_health(
            duration,
            settings.magical_damage_factor,
            actor.body.mass(),
            &actor.effect,
            &mut actor.health,
        );
        decay_effect(now, duration, settings.decay_factor, &mut actor.effect);
        decay_aura(now, duration, settings.decay_factor, &mut actor.aura);
        update_velocity(
            duration,
            actor.body.mass(),
            actor.dynamic_force,
            settings.min_move_distance,
            &mut actor.velocity,
        );
        update_velocity_z(
            duration,
            actor.body.shape.radius,
            settings.gravitational_acceleration,
            actor.position_z,
            &mut actor.velocity_z,
        );
        update_position_z(
            duration,
            actor.body.shape.radius,
            actor.velocity_z,
            &mut actor.position_z,
        );
    }
}

fn update_dynamic_objects(
    now: f64,
    duration: f64,
    settings: &WorldSettings,
    dynamic_objects: &mut Vec<DynamicObject>,
) {
    for object in dynamic_objects.iter_mut() {
        damage_health(
            duration,
            settings.magical_damage_factor,
            object.body.mass(),
            &object.effect,
            &mut object.health,
        );
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
        update_velocity(
            duration,
            object.body.mass(),
            object.dynamic_force,
            settings.min_move_distance,
            &mut object.velocity,
        );
        update_position_z(
            duration,
            object.body.shape.radius,
            object.velocity_z,
            &mut object.position_z,
        );
        update_velocity_z(
            duration,
            object.body.shape.radius,
            settings.gravitational_acceleration,
            object.position_z,
            &mut object.velocity_z,
        );
    }
}

fn update_static_objects(
    now: f64,
    duration: f64,
    settings: &WorldSettings,
    static_objects: &mut Vec<StaticObject>,
) {
    for object in static_objects.iter_mut() {
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
    }
}

fn update_actor_current_direction(duration: f64, max_rotation_speed: f64, actor: &mut Actor) {
    actor.current_direction = get_current_direction(
        actor.current_direction,
        actor.target_direction,
        duration,
        max_rotation_speed,
    );
}

fn update_actor_dynamic_force(move_force: f64, actor: &mut Actor) {
    actor.dynamic_force += actor.current_direction * move_force * actor.moving as i32 as f64;
}

fn add_dry_friction_force(
    mass: f64,
    velocity: Vec2f,
    surface: Material,
    gravitational_acceleration: f64,
    dynamic_force: &mut Vec2f,
) {
    let speed = velocity.norm();
    if speed != 0.0 {
        *dynamic_force -=
            velocity * (mass * surface.friction() * gravitational_acceleration / speed);
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

fn damage_health(duration: f64, damage_factor: f64, mass: f64, effect: &Effect, health: &mut f64) {
    *health = (*health - get_damage(&effect.power) * damage_factor * duration / mass).min(1.0);
}

fn update_position(duration: f64, velocity: Vec2f, position: &mut Vec2f) {
    *position += velocity * duration;
}

fn update_velocity(
    duration: f64,
    mass: f64,
    dynamic_force: Vec2f,
    min_move_distance: f64,
    velocity: &mut Vec2f,
) {
    *velocity += dynamic_force * (duration / (2.0 * mass));
    if velocity.norm() * duration <= min_move_distance {
        *velocity = Vec2f::ZERO;
    }
}

fn update_position_z(duration: f64, height: f64, velocity_z: f64, position_z: &mut f64) {
    *position_z = height.max(*position_z + duration * velocity_z);
}

fn update_velocity_z(
    duration: f64,
    height: f64,
    gravitational_acceleration: f64,
    position_z: f64,
    velocity_z: &mut f64,
) {
    if position_z - height > f64::EPSILON {
        *velocity_z -= duration * gravitational_acceleration / 2.0;
    } else {
        *velocity_z = 0.0;
    }
}

fn add_magick_power_to_effect(now: f64, target: &Effect, other: &[f64; 11]) -> Effect {
    let mut power = target.power;
    let mut applied = target.applied;
    for i in 0..power.len() {
        if other[i] > 0.0 {
            power[i] = power[i].max(other[i]);
            applied[i] = now;
        }
    }
    let target_power = power;
    if target_power[Element::Water as usize] > 0.0 && target_power[Element::Fire as usize] > 0.0 {
        power[Element::Water as usize] = 0.0;
        power[Element::Fire as usize] = 0.0;
        power[Element::Steam as usize] =
            target_power[Element::Water as usize].max(target_power[Element::Fire as usize]);
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
        power[Element::Ice as usize] =
            target_power[Element::Water as usize].max(target_power[Element::Water as usize]);
        applied[Element::Ice as usize] = now;
    }
    Effect { applied, power }
}

fn get_damage(power: &[f64; 11]) -> f64 {
    power[Element::Lightning as usize] - power[Element::Life as usize]
        + power[Element::Arcane as usize]
        + power[Element::Cold as usize]
        + power[Element::Fire as usize]
        + power[Element::Steam as usize]
        + power[Element::Poison as usize]
}

fn is_instant_effect_element(element: Element) -> bool {
    matches!(
        element,
        Element::Lightning | Element::Arcane | Element::Earth | Element::Steam
    )
}

fn can_absorb_physical_damage(elements: &[bool; 11]) -> bool {
    let expect =
        elements[Element::Shield as usize] as i32 + elements[Element::Earth as usize] as i32;
    elements[Element::Shield as usize] && expect == elements.iter().map(|v| *v as i32).sum::<i32>()
}

fn can_reflect_beams(elements: &[bool; 11]) -> bool {
    elements[Element::Shield as usize] && 1 == elements.iter().map(|v| *v as i32).sum::<i32>()
}

fn get_current_direction(
    current_direction: Vec2f,
    target_direction: Vec2f,
    duration: f64,
    max_rotation_speed: f64,
) -> Vec2f {
    let diff = normalize_angle(target_direction.angle() - current_direction.angle());
    current_direction.rotated(diff.signum() * diff.abs().min(max_rotation_speed * duration))
}

pub fn normalize_angle(angle: f64) -> f64 {
    let turns = angle / std::f64::consts::TAU + 0.5;
    (turns - turns.floor() - 0.5) * std::f64::consts::TAU
}

fn intersect_beam(
    magick: &Magick,
    origin: Vec2f,
    direction: Vec2f,
    depth: usize,
    length: &mut f64,
    world: &mut World,
    shape_cache: &ShapeCache,
) -> Option<EmittedBeam> {
    let mut nearest_hit =
        find_beam_nearest_intersection(origin, direction, &world.actors, length, shape_cache)
            .map(|(i, n)| (Index::Actor(i), n));
    nearest_hit = find_beam_nearest_intersection(
        origin,
        direction,
        &world.dynamic_objects,
        length,
        shape_cache,
    )
    .map(|(i, n)| (Index::DynamicObject(i), n))
    .or(nearest_hit);
    nearest_hit = find_beam_nearest_intersection(
        origin,
        direction,
        &world.static_objects,
        length,
        shape_cache,
    )
    .map(|(i, n)| (Index::StaticObject(i), n))
    .or(nearest_hit);
    if let Some((index, mut normal)) = nearest_hit {
        let (aura, effect) = match index {
            Index::Actor(i) => {
                let object = &mut world.actors[i];
                (&object.aura, &mut object.effect)
            }
            Index::DynamicObject(i) => {
                let object = &mut world.dynamic_objects[i];
                (&object.aura, &mut object.effect)
            }
            Index::StaticObject(i) => {
                let object = &mut world.static_objects[i];
                (&object.aura, &mut object.effect)
            }
        };
        *effect = add_magick_power_to_effect(world.time, effect, &magick.power);
        if depth < world.settings.max_beam_depth as usize && can_reflect_beams(&aura.elements) {
            // RayCast::cast_ray_and_get_normal returns not normalized normal
            normal.normalize();
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
        match &self.body.shape {
            StaticShape::CircleArc(v) => {
                Isometry::new(Vector2::new(self.position.x, self.position.y), v.rotation)
            }
            StaticShape::Disk(..) => Isometry::translation(self.position.x, self.position.y),
        }
    }
}

trait WithShape {
    fn with_shape(&self, shape_cache: &ShapeCache, f: &mut dyn FnMut(&dyn Shape));
}

impl WithShape for Actor {
    fn with_shape(&self, _: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        (*f)(&self.body.shape.as_shape())
    }
}

impl WithShape for DynamicObject {
    fn with_shape(&self, _: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        (*f)(&self.body.shape.as_shape())
    }
}

impl WithShape for StaticObject {
    fn with_shape(&self, shape_cache: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        self.body
            .shape
            .clone()
            .with_shape(shape_cache, |shape| (*f)(shape))
    }
}

fn with_shape<T, R, F>(v: &T, shape_cache: &ShapeCache, mut f: F) -> R
where
    T: WithShape,
    F: FnMut(&dyn Shape) -> R,
{
    let mut r = None;
    v.with_shape(shape_cache, &mut |shape| {
        r = Some(f(shape));
    });
    r.unwrap()
}

fn find_beam_nearest_intersection<T>(
    origin: Vec2f,
    direction: Vec2f,
    objects: &[T],
    length: &mut f64,
    shape_cache: &ShapeCache,
) -> Option<(usize, Vec2f)>
where
    T: WithIsometry + WithShape,
{
    let mut nearest = None;
    for (i, object) in objects.iter().enumerate() {
        let isometry = object.get_isometry();
        let result = with_shape(object, shape_cache, |shape| {
            shape.cast_ray_and_get_normal(
                &isometry,
                &Ray::new(
                    Point2::new(origin.x, origin.y),
                    Vector2::new(direction.x, direction.y),
                ),
                *length,
                true,
            )
        });
        if let Some(intersection) = result {
            *length = intersection.toi;
            nearest = Some((i, Vec2f::new(intersection.normal.x, intersection.normal.y)));
        }
    }
    nearest
}

fn move_objects(duration: f64, world: &mut World, shape_cache: &ShapeCache) {
    let mut earliest_collision = None;
    let mut duration_left = duration;
    loop {
        for (i, static_object) in world.static_objects.iter().enumerate() {
            for (j, actor) in world.actors.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, static_object, actor)
                {
                    update_earliest_collision(
                        Index::StaticObject(i),
                        Index::Actor(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
            for (j, dynamic_object) in world.dynamic_objects.iter().enumerate() {
                if let Some(toi) =
                    time_of_impact(duration_left, shape_cache, static_object, dynamic_object)
                {
                    update_earliest_collision(
                        Index::StaticObject(i),
                        Index::DynamicObject(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
        }
        if !world.actors.is_empty() {
            for i in 0..world.actors.len() - 1 {
                for j in i + 1..world.actors.len() {
                    if let Some(toi) = time_of_impact(
                        duration_left,
                        shape_cache,
                        &world.actors[i],
                        &world.actors[j],
                    ) {
                        update_earliest_collision(
                            Index::Actor(i),
                            Index::Actor(j),
                            toi,
                            &mut earliest_collision,
                        );
                    }
                }
            }
        }
        for (i, dynamic_object) in world.dynamic_objects.iter().enumerate() {
            for (j, actor) in world.actors.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, dynamic_object, actor)
                {
                    update_earliest_collision(
                        Index::DynamicObject(i),
                        Index::Actor(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
        }
        if !world.dynamic_objects.is_empty() {
            for i in 0..world.dynamic_objects.len() - 1 {
                for j in i + 1..world.dynamic_objects.len() {
                    if let Some(toi) = time_of_impact(
                        duration_left,
                        shape_cache,
                        &world.dynamic_objects[i],
                        &world.dynamic_objects[j],
                    ) {
                        update_earliest_collision(
                            Index::DynamicObject(i),
                            Index::DynamicObject(j),
                            toi,
                            &mut earliest_collision,
                        );
                    }
                }
            }
        }
        if let Some(collision) = earliest_collision.as_ref() {
            let now = world.time + (duration - duration_left);
            let physical_damage_factor = world.settings.physical_damage_factor;
            with_colliding_objects_mut(collision.lhs, collision.rhs, world, |lhs, rhs| {
                apply_impact(
                    now,
                    physical_damage_factor,
                    duration / 100.0,
                    shape_cache,
                    &collision.toi,
                    lhs,
                    rhs,
                );
            });
            for (i, actor) in world.actors.iter_mut().enumerate() {
                if Index::Actor(i) != collision.lhs && Index::Actor(i) != collision.rhs {
                    update_position(collision.toi.toi, actor.velocity, &mut actor.position)
                }
            }
            for (i, dynamic_object) in world.dynamic_objects.iter_mut().enumerate() {
                if Index::DynamicObject(i) != collision.lhs
                    && Index::DynamicObject(i) != collision.rhs
                {
                    update_position(
                        collision.toi.toi,
                        dynamic_object.velocity,
                        &mut dynamic_object.position,
                    )
                }
            }
            duration_left -= collision.toi.toi.max(duration / 10.0);
            if duration_left <= f64::EPSILON {
                break;
            }
            earliest_collision = None;
        } else {
            world
                .actors
                .iter_mut()
                .for_each(|v| update_position(duration, v.velocity, &mut v.position));
            world
                .dynamic_objects
                .iter_mut()
                .for_each(|v| update_position(duration, v.velocity, &mut v.position));
            break;
        }
    }
}

trait WithVelocity {
    fn velocity(&self) -> Vec2f;
}

fn time_of_impact<L, R>(duration: f64, shape_cache: &ShapeCache, lhs: &L, rhs: &R) -> Option<TOI>
where
    L: WithVelocity + WithIsometry + WithShape,
    R: WithVelocity + WithIsometry + WithShape,
{
    let mut toi = None;
    lhs.with_shape(shape_cache, &mut |lhs_shape| {
        rhs.with_shape(shape_cache, &mut |rhs_shape| {
            toi = query::time_of_impact(
                &lhs.get_isometry(),
                &Vector2::new(lhs.velocity().x, lhs.velocity().y),
                lhs_shape,
                &rhs.get_isometry(),
                &Vector2::new(rhs.velocity().x, rhs.velocity().y),
                rhs_shape,
                duration,
            )
            .unwrap();
        });
    });
    toi
}

impl WithVelocity for Actor {
    fn velocity(&self) -> Vec2f {
        self.velocity
    }
}

impl WithVelocity for DynamicObject {
    fn velocity(&self) -> Vec2f {
        self.velocity
    }
}

impl WithVelocity for StaticObject {
    fn velocity(&self) -> Vec2f {
        Vec2f::ZERO
    }
}

struct Collision {
    lhs: Index,
    rhs: Index,
    toi: TOI,
}

fn update_earliest_collision(lhs: Index, rhs: Index, toi: TOI, collision: &mut Option<Collision>) {
    if collision.is_none() || collision.as_ref().unwrap().toi.toi > toi.toi {
        *collision = Some(Collision { lhs, rhs, toi });
    }
}

trait CollidingObject: WithVelocity + WithShape + WithIsometry {
    fn material(&self) -> Material;
    fn mass(&self) -> f64;
    fn position(&self) -> Vec2f;
    fn set_position(&mut self, value: Vec2f);
    fn set_velocity(&mut self, value: Vec2f);
    fn effect(&mut self) -> &mut Effect;
    fn health(&mut self) -> &mut f64;
    fn aura(&self) -> &Aura;
    fn is_static(&self) -> bool;
}

fn with_colliding_objects_mut<F>(lhs: Index, rhs: Index, world: &mut World, mut f: F)
where
    F: FnMut(&mut dyn CollidingObject, &mut dyn CollidingObject),
{
    if lhs > rhs {
        with_colliding_objects_ord_mut(rhs, lhs, world, |l, r| f(r, l))
    } else {
        with_colliding_objects_ord_mut(lhs, rhs, world, f)
    }
}

fn with_colliding_objects_ord_mut<F>(lhs: Index, rhs: Index, world: &mut World, mut f: F)
where
    F: FnMut(&mut dyn CollidingObject, &mut dyn CollidingObject),
{
    match lhs {
        Index::Actor(lhs) => match rhs {
            Index::Actor(rhs) => {
                let (left, right) = world.actors.split_at_mut(rhs);
                f(&mut left[lhs], &mut right[0])
            }
            Index::DynamicObject(rhs) => f(&mut world.actors[lhs], &mut world.dynamic_objects[rhs]),
            Index::StaticObject(rhs) => f(&mut world.actors[lhs], &mut world.static_objects[rhs]),
        },
        Index::DynamicObject(lhs) => match rhs {
            Index::Actor(rhs) => f(&mut world.dynamic_objects[lhs], &mut world.actors[rhs]),
            Index::DynamicObject(rhs) => {
                let (left, right) = world.dynamic_objects.split_at_mut(rhs);
                f(&mut left[lhs], &mut right[0])
            }
            Index::StaticObject(rhs) => f(
                &mut world.dynamic_objects[lhs],
                &mut world.static_objects[rhs],
            ),
        },
        Index::StaticObject(lhs) => match rhs {
            Index::Actor(rhs) => f(&mut world.static_objects[lhs], &mut world.actors[rhs]),
            Index::DynamicObject(rhs) => f(
                &mut world.static_objects[lhs],
                &mut world.dynamic_objects[rhs],
            ),
            Index::StaticObject(rhs) => {
                let (left, right) = world.static_objects.split_at_mut(rhs);
                f(&mut left[lhs], &mut right[0])
            }
        },
    }
}

fn apply_impact(
    now: f64,
    damage_factor: f64,
    epsilon_duration: f64,
    shape_cache: &ShapeCache,
    toi: &TOI,
    lhs: &mut dyn CollidingObject,
    rhs: &mut dyn CollidingObject,
) {
    let lhs_kinetic_energy = get_kinetic_energy(lhs.mass(), lhs.velocity());
    let rhs_kinetic_energy = get_kinetic_energy(rhs.mass(), rhs.velocity());
    let delta_velocity = lhs.velocity() - rhs.velocity();
    let mass_sum = lhs.mass() + rhs.mass();
    let lhs_velocity = lhs.velocity()
        - delta_velocity * rhs.mass() * (1.0 + lhs.material().restitution()) / mass_sum;
    let rhs_velocity = rhs.velocity()
        + delta_velocity * lhs.mass() * (1.0 + rhs.material().restitution()) / mass_sum;
    lhs.set_position(lhs.position() + lhs.velocity() * toi.toi + lhs_velocity * epsilon_duration);
    rhs.set_position(rhs.position() + rhs.velocity() * toi.toi + rhs_velocity * epsilon_duration);
    lhs.set_velocity(lhs_velocity);
    rhs.set_velocity(rhs_velocity);
    if let Some(contact) = get_contact(shape_cache, lhs, rhs) {
        if lhs.is_static() {
            rhs.set_position(
                rhs.position()
                    + Vec2f::new(contact.normal2.x, contact.normal2.y)
                        * contact.dist.min(-epsilon_duration),
            );
        } else if rhs.is_static() {
            lhs.set_position(
                lhs.position()
                    + Vec2f::new(contact.normal1.x, contact.normal1.y)
                        * contact.dist.min(-epsilon_duration),
            );
        } else {
            let half_distance = contact.dist.min(-epsilon_duration) / 2.0;
            lhs.set_position(
                lhs.position()
                    + Vec2f::new(contact.normal1.x, contact.normal1.y)
                        * (half_distance * rhs.mass() / mass_sum),
            );
            rhs.set_position(
                rhs.position()
                    + Vec2f::new(contact.normal2.x, contact.normal2.y)
                        * (half_distance * lhs.mass() / mass_sum),
            );
        }
    }
    let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect(), &rhs.effect().power);
    let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect(), &lhs.effect().power);
    *lhs.effect() = new_lhs_effect;
    *rhs.effect() = new_rhs_effect;
    handle_collision_damage(lhs_kinetic_energy, damage_factor, lhs_velocity, lhs);
    handle_collision_damage(rhs_kinetic_energy, damage_factor, rhs_velocity, rhs);
}

fn get_contact(
    shape_cache: &ShapeCache,
    lhs: &dyn CollidingObject,
    rhs: &dyn CollidingObject,
) -> Option<Contact> {
    let mut contact = None;
    lhs.with_shape(shape_cache, &mut |lhs_shape| {
        rhs.with_shape(shape_cache, &mut |rhs_shape| {
            contact = query::contact(
                &lhs.get_isometry(),
                lhs_shape,
                &rhs.get_isometry(),
                rhs_shape,
                0.0,
            )
            .unwrap();
        })
    });
    contact
}

impl CollidingObject for Actor {
    fn material(&self) -> Material {
        self.body.material
    }

    fn mass(&self) -> f64 {
        self.body.mass()
    }

    fn position(&self) -> Vec2f {
        self.position
    }

    fn set_position(&mut self, value: Vec2f) {
        self.position = value;
    }

    fn set_velocity(&mut self, value: Vec2f) {
        self.velocity = value;
    }

    fn effect(&mut self) -> &mut Effect {
        &mut self.effect
    }

    fn health(&mut self) -> &mut f64 {
        &mut self.health
    }

    fn aura(&self) -> &Aura {
        &self.aura
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject for DynamicObject {
    fn material(&self) -> Material {
        self.body.material
    }

    fn mass(&self) -> f64 {
        self.body.mass()
    }

    fn position(&self) -> Vec2f {
        self.position
    }

    fn set_position(&mut self, value: Vec2f) {
        self.position = value;
    }

    fn set_velocity(&mut self, value: Vec2f) {
        self.velocity = value;
    }

    fn effect(&mut self) -> &mut Effect {
        &mut self.effect
    }

    fn health(&mut self) -> &mut f64 {
        &mut self.health
    }

    fn aura(&self) -> &Aura {
        &self.aura
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject for StaticObject {
    fn material(&self) -> Material {
        self.body.material
    }

    fn mass(&self) -> f64 {
        self.body.mass()
    }

    fn position(&self) -> Vec2f {
        self.position
    }

    fn set_position(&mut self, _: Vec2f) {}

    fn set_velocity(&mut self, _: Vec2f) {}

    fn effect(&mut self) -> &mut Effect {
        &mut self.effect
    }

    fn health(&mut self) -> &mut f64 {
        &mut self.health
    }

    fn aura(&self) -> &Aura {
        &self.aura
    }

    fn is_static(&self) -> bool {
        true
    }
}

fn handle_collision_damage(
    prev_kinetic_energy: f64,
    damage_factor: f64,
    velocity: Vec2f,
    object: &mut dyn CollidingObject,
) {
    if !can_absorb_physical_damage(&object.aura().elements) {
        *object.health() -= (get_kinetic_energy(object.mass(), velocity) - prev_kinetic_energy)
            .abs()
            * damage_factor
            / object.mass();
    }
}

fn get_kinetic_energy(mass: f64, velocity: Vec2f) -> f64 {
    mass * velocity.dot_self() / 2.0
}

fn is_active(bounds: &Rectf, shape: &dyn Shape, position: Vec2f, health: f64) -> bool {
    health > 0.0 && {
        let bounds_position = (bounds.max + bounds.min) / 2.0;
        let half_extents = (bounds.max - bounds.min) / 2.0;
        query::intersection_test(
            &Isometry::translation(bounds_position.x, bounds_position.y),
            &Cuboid::new(Vector2::new(half_extents.x, half_extents.y)),
            &Isometry::translation(position.x, position.y),
            shape,
        )
        .unwrap()
    }
}

fn handle_completed_magicks(world: &mut World) {
    for actor in world.actors.iter_mut() {
        if actor
            .delayed_magick
            .as_ref()
            .map(|v| v.completed)
            .unwrap_or(false)
        {
            let delayed_magick = actor.delayed_magick.take().unwrap();
            let radius = delayed_magick.power.iter().sum::<f64>() * actor.body.shape.radius
                / world.settings.max_magic_power;
            let material = Material::Stone;
            world.dynamic_objects.push(DynamicObject {
                id: get_next_id(&mut world.id_counter),
                body: Body {
                    shape: Disk { radius },
                    material,
                },
                position: actor.position
                    + actor.current_direction
                        * (actor.body.shape.radius + radius + world.settings.margin),
                health: 1.0,
                effect: Effect {
                    applied: [world.time; 11],
                    power: delayed_magick.power,
                },
                aura: Default::default(),
                velocity: actor.velocity,
                dynamic_force: actor.current_direction
                    * ((world.time - delayed_magick.started).min(world.settings.max_magic_power)
                        * world.settings.magic_force_multiplier),
                position_z: 1.5 * actor.body.shape.radius,
                velocity_z: 0.0,
            });
        }
    }
}

fn remove_count<T, F>(vec: &mut Vec<T>, mut f: F) -> usize
where
    F: FnMut(&T) -> bool,
{
    let mut removed = 0;
    vec.retain(|v| {
        let retain = !f(v);
        removed += !retain as usize;
        retain
    });
    removed
}

fn intersection_test(
    shape_pos: &Isometry<Real>,
    shape: &dyn Shape,
    body_pos: &Isometry<Real>,
    body: &RingSector,
    direction: Vec2f,
) -> bool {
    if query::intersection_test(shape_pos, shape, body_pos, &Ball::new(body.min_radius)).unwrap()
        || !query::intersection_test(shape_pos, shape, body_pos, &Ball::new(body.max_radius))
            .unwrap()
    {
        return false;
    }
    if body.angle == std::f64::consts::TAU {
        return true;
    }
    let radius = direction * body.max_radius;
    let left = radius.rotated(body.angle / 2.0);
    let right = radius.rotated(-body.angle / 2.0);
    let triangle = Triangle::new(
        Point2::new(0.0, 0.0),
        Point2::new(left.x, left.y),
        Point2::new(right.x, right.y),
    );
    query::intersection_test(shape_pos, shape, body_pos, &triangle).unwrap()
}

fn make_circle_arc_polyline(arc: &CircleArcKey) -> Polyline {
    let mut vertices = Vec::new();
    let resolution = (arc.radius * RESOLUTION_FACTOR).round();
    let angle_step = arc.length / resolution;
    let half_length = arc.length / 2.0;
    for i in 0..=resolution as usize {
        let position = Vec2f::only_x(arc.radius).rotated(angle_step * i as f64 - half_length);
        vertices.push(Point2::new(position.x, position.y));
    }
    Polyline::new(vertices, None)
}

#[cfg(test)]
mod tests {
    use nalgebra::distance;
    use parry2d_f64::na::Unit;
    use parry2d_f64::query::TOIStatus;

    use crate::engine::*;

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

    #[test]
    fn make_circle_arc_polyline_should_generate_vertices_along_arc_circle() {
        use std::f64::consts::{FRAC_PI_2, SQRT_2};
        let polyline = make_circle_arc_polyline(&CircleArcKey {
            radius: 2.0,
            length: FRAC_PI_2,
        });
        assert_eq!(polyline.num_segments(), 8);
        assert!(
            distance(
                &polyline.vertices().first().unwrap(),
                &Point2::new(SQRT_2, -SQRT_2),
            ) <= f64::EPSILON
        );
        assert!(
            distance(
                &polyline.vertices().last().unwrap(),
                &Point2::new(SQRT_2, SQRT_2),
            ) <= f64::EPSILON
        );
    }

    #[test]
    fn time_of_impact_should_find_impact_for_static_circle_arc_and_moving_disk() {
        use std::f64::consts::FRAC_PI_2;
        let duration = 10.0;
        let shape_cache = ShapeCache::default();
        let static_object = StaticObject {
            id: 1,
            body: Body {
                shape: StaticShape::CircleArc(CircleArc {
                    radius: 5.0,
                    length: FRAC_PI_2,
                    rotation: -2.6539321938108684,
                }),
                material: Material::None,
            },
            position: Vec2f::new(-33.23270204831895, -32.3454131103618),
            health: 0.0,
            effect: Effect::default(),
            aura: Aura::default(),
        };
        let dynamic_object = DynamicObject {
            id: 2,
            body: Body {
                shape: Disk { radius: 0.2 },
                material: Material::Stone,
            },
            position: Vec2f::new(-34.41147614376544, -32.89358428062188),
            health: 1.0,
            effect: Effect::default(),
            aura: Aura::default(),
            velocity: Vec2f::new(-737.9674461149048, -343.18066550098706),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.5,
            velocity_z: -0.08166666666666667,
        };
        let toi = time_of_impact(duration, &shape_cache, &static_object, &dynamic_object).unwrap();
        assert!(
            (toi.toi - 0.004296260825975674) <= f64::EPSILON,
            "{}",
            toi.toi
        );
        assert_eq!(
            toi.witness1,
            Point2::new(4.989825741741153, -0.2589521646469748)
        );
        assert_eq!(
            toi.witness2,
            Point2::new(-0.18022918657654352, -0.08670317356335425)
        );
        assert_eq!(
            toi.normal1,
            Unit::new_unchecked(Vector2::<f64>::new(
                -0.9992290361820114,
                0.03925981725337541,
            ))
        );
        assert_eq!(
            toi.normal2,
            Unit::new_unchecked(Vector2::<f64>::new(
                -0.9011459910530735,
                -0.4335157468985109,
            ))
        );
        assert_eq!(toi.status, TOIStatus::Converged);
    }
}
