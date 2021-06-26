use std::cell::RefCell;

use parry2d_f64::math::{Isometry, Real};
use parry2d_f64::na::{Point2, Vector2};
use parry2d_f64::query;
use parry2d_f64::query::{Contact, Ray, TOI};
use parry2d_f64::shape::{Ball, Cuboid, Polyline, Shape, Triangle};
use rand::Rng;

use crate::generators::generate_player_actor;
use crate::rect::Rectf;
use crate::vec2::Vec2f;
#[cfg(feature = "server")]
use crate::world::PlayerId;
use crate::world::{
    Actor, ActorId, ActorOccupation, Aura, Beam, BeamId, Body, BoundedArea, BoundedAreaId,
    CircleArc, DelayedMagick, DelayedMagickStatus, Disk, Effect, Element, Field, FieldId, Gun,
    GunId, Magick, Material, Projectile, ProjectileId, RingSector, Shield, ShieldId, StaticArea,
    StaticObject, StaticShape, TempArea, TempAreaId, TempObstacle, TempObstacleId, World,
    WorldSettings,
};

const RESOLUTION_FACTOR: f64 = 4.0;
const DEFAULT_AURA: Aura = Aura {
    applied: 0.0,
    power: 0.0,
    radius: 0.0,
    elements: [false; 11],
};
const DEFAULT_EFFECT: Effect = Effect {
    applied: [0.0; 11],
    power: [0.0; 11],
};
const DEFAULT_MAGICK: Magick = Magick { power: [0.0; 11] };
const DEFAULT_RESISTANCE: [bool; 11] = [false; 11];

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum Index {
    Actor(usize),
    Projectile(usize),
    StaticObject(usize),
    Shield(usize),
    TempObstacle(usize),
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

    pub fn update<R: Rng>(&mut self, duration: f64, world: &mut World, rng: &mut R) {
        world.frame += 1;
        world.time += duration;
        let now = world.time;
        world.bounded_areas.retain(|v| v.deadline >= now);
        world.fields.retain(|v| v.deadline >= now);
        world.beams.retain(|v| v.deadline >= now);
        world.temp_areas.retain(|v| v.deadline >= now);
        world.guns.retain(|v| v.shots_left > 0);
        world.temp_obstacles.retain(|v| v.deadline >= now);
        update_actor_occupations(world);
        spawn_player_actors(world, rng);
        shoot_from_guns(world, rng);
        intersect_objects_with_areas(world, &self.shape_cache);
        intersect_objects_with_all_fields(world);
        update_actors(world.time, duration, &world.settings, &mut world.actors);
        update_projectiles(duration, &world.settings, &mut world.projectiles);
        update_static_objects(
            world.time,
            duration,
            &world.settings,
            &mut world.static_objects,
        );
        update_shields(duration, &world.settings, &mut world.shields);
        update_temp_obstacles(duration, &world.settings, &mut world.temp_obstacles);
        self.update_beams(world);
        move_objects(duration, world, &self.shape_cache);
        world
            .actors
            .iter_mut()
            .for_each(|v| v.dynamic_force = Vec2f::ZERO);
        world
            .projectiles
            .iter_mut()
            .for_each(|v| v.dynamic_force = Vec2f::ZERO);
        let bounds = world.bounds.clone();
        world.actors.iter_mut().for_each(|v| {
            v.active = is_active(&bounds, &v.body.shape.as_shape(), v.position, v.health)
        });
        remove_inactive_actors_occupation_results(world);
        world.actors.retain(|v| v.active);
        world.projectiles.retain(|v| {
            (v.velocity_z > f64::EPSILON || v.velocity.norm() > f64::EPSILON)
                && is_active(&bounds, &v.body.shape.as_shape(), v.position, v.health)
        });
        world.static_objects.retain(|v| {
            v.body.shape.with_shape(&self.shape_cache, |shape| {
                is_active(&bounds, shape, v.position, v.health)
            })
        });
        world.shields.retain(|v| v.power > 0.0);
        world.temp_obstacles.retain(|v| v.health > 0.0);
        handle_completed_magicks(world);
        update_player_spawn_time(world);
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
pub fn remove_player(player_id: PlayerId, world: &mut World) {
    if let Some(player) = world.players.iter_mut().find(|v| v.id == player_id) {
        player.active = false;
    }
}

pub fn add_actor_spell_element(actor_index: usize, element: Element, world: &mut World) {
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None) {
        return;
    }
    if matches!(element, Element::Lightning)
        && world.actors[actor_index].effect.power[Element::Water as usize] > 0.0
    {
        world.actors[actor_index].effect.power[Element::Lightning as usize] =
            world.actors[actor_index].effect.power[Element::Lightning as usize].min(1.0);
        return;
    }
    Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .add(element);
}

#[allow(clippy::needless_return)]
pub fn start_directed_magick(actor_index: usize, world: &mut World) {
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None) {
        return;
    }
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        cast_shield(magick, actor_index, world);
    } else if magick.power[Element::Earth as usize] > 0.0
        || magick.power[Element::Ice as usize] > 0.0
    {
        add_delayed_magick(magick, actor_index, world);
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
            magick,
            actor_index,
            world,
        );
    }
}

#[allow(clippy::needless_return)]
#[allow(clippy::if_same_then_else)]
pub fn start_area_of_effect_magick(actor_index: usize, world: &mut World) {
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None) {
        return;
    }
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
            magick,
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
            magick,
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

fn cast_earth_based_shield(mut magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    magick.power[Element::Shield as usize] = 0.0;
    magick.power[Element::Earth as usize] = 0.0;
    for i in -2..=2 {
        world.temp_obstacles.push(TempObstacle {
            id: TempObstacleId(get_next_id(&mut world.id_counter)),
            actor_id: actor.id,
            body: Body {
                shape: Disk {
                    radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                },
                material: Material::Stone,
            },
            position: actor.position
                + actor
                    .current_direction
                    .rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64)
                    * distance,
            health: 1.0,
            magick: magick.clone(),
            effect: Effect::default(),
            deadline: world.time + world.settings.temp_obstacle_magick_duration,
        });
    }
}

fn cast_spray_based_shield(magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    for i in -2..=2 {
        world.temp_areas.push(TempArea {
            id: TempAreaId(get_next_id(&mut world.id_counter)),
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
            magick: magick.clone(),
            deadline: world.time + world.settings.temp_area_duration,
        });
    }
}

fn cast_reflecting_shield(length: f64, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    world.shields.push(Shield {
        id: ShieldId(get_next_id(&mut world.id_counter)),
        actor_id: actor.id,
        body: Body {
            shape: CircleArc {
                radius: 5.0,
                length,
                rotation: normalize_angle(actor.current_direction.angle()),
            },
            material: Material::None,
        },
        position: actor.position,
        power: 1.0,
    });
}

fn add_delayed_magick(magick: Magick, actor_index: usize, world: &mut World) {
    world.actors[actor_index].delayed_magick = Some(DelayedMagick {
        started: world.time,
        status: DelayedMagickStatus::Started,
        power: magick.power,
    });
}

fn add_beam(magick: Magick, actor_index: usize, world: &mut World) {
    let beam_id = BeamId(get_next_id(&mut world.id_counter));
    world.beams.push(Beam {
        id: beam_id,
        actor_id: world.actors[actor_index].id,
        magick,
        deadline: world.time + world.settings.directed_magick_duration,
    });
    world.actors[actor_index].occupation = ActorOccupation::Beaming(beam_id);
}

pub fn complete_directed_magick(actor_index: usize, world: &mut World) {
    match world.actors[actor_index].occupation {
        ActorOccupation::None => (),
        ActorOccupation::Beaming(beam_id) => {
            if let Some(v) = world.beams.iter_mut().find(|v| v.id == beam_id) {
                v.deadline = world.time;
            }
        }
        ActorOccupation::Spraying {
            bounded_area_id,
            field_id,
        } => {
            if let Some(v) = world
                .bounded_areas
                .iter_mut()
                .find(|v| v.id == bounded_area_id)
            {
                v.deadline = world.time;
            }
            if let Some(v) = world.fields.iter_mut().find(|v| v.id == field_id) {
                v.deadline = world.time;
            }
        }
        ActorOccupation::Shooting(gun_id) => {
            if let Some(v) = world.guns.iter_mut().find(|v| v.id == gun_id) {
                v.shots_left = 0;
            }
        }
    }
    if let Some(delayed_magick) = world.actors[actor_index].delayed_magick.as_mut() {
        delayed_magick.status = if delayed_magick.power[Element::Earth as usize] == 0.0
            && delayed_magick.power[Element::Ice as usize] > 0.0
        {
            DelayedMagickStatus::Shoot
        } else {
            DelayedMagickStatus::Throw
        };
    }
}

pub fn self_magick(actor_index: usize, world: &mut World) {
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None) {
        return;
    }
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] == 0.0 {
        world.actors[actor_index].effect = add_magick_to_effect(
            world.time,
            &world.actors[actor_index].effect,
            &magick,
            &world.actors[actor_index].aura.elements,
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
        if elements.iter().filter(|v| **v).count() > 1 {
            elements[Element::Shield as usize] = false;
        }
        world.actors[actor_index].aura = Aura {
            applied: world.time,
            power,
            radius: radius_factor * world.actors[actor_index].body.shape.radius,
            elements,
        };
    }
}

fn cast_spray(angle: f64, duration: f64, magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let total_power = magick.power.iter().sum::<f64>();
    let body = RingSector {
        min_radius: actor.body.shape.radius + world.settings.margin,
        max_radius: actor.body.shape.radius
            * (1.0 + total_power)
            * world.settings.spray_distance_factor,
        angle,
    };
    let field_id = if (magick.power[Element::Water as usize] - total_power).abs() <= f64::EPSILON {
        let field_id = FieldId(get_next_id(&mut world.id_counter));
        world.fields.push(Field {
            id: field_id,
            actor_id: actor.id,
            body: body.clone(),
            force: world.settings.spray_force_factor * magick.power[Element::Water as usize],
            deadline: world.time + duration,
        });
        field_id
    } else {
        FieldId(0)
    };
    let bounded_area_id = BoundedAreaId(get_next_id(&mut world.id_counter));
    world.bounded_areas.push(BoundedArea {
        id: bounded_area_id,
        actor_id: actor.id,
        body,
        magick,
        deadline: world.time + duration,
    });
    world.actors[actor_index].occupation = ActorOccupation::Spraying {
        bounded_area_id,
        field_id,
    };
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

impl CircleArc {
    fn with_shape<R, F: FnMut(&dyn Shape) -> R>(&self, cache: &ShapeCache, f: F) -> R {
        cache.with(
            &CircleArcKey {
                radius: self.radius,
                length: self.length,
            },
            f,
        )
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
            Material::Ice => 900.0,
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
            Material::Ice => 0.01,
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
            Material::Ice => 0.05,
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
                resistance: &actor.aura.elements,
                dynamic_force: &mut actor.dynamic_force,
                effect: &mut actor.effect,
            },
        );
    }
    for v in world.projectiles.iter_mut() {
        let mut effect = Effect::default();
        intersect_static_object_with_all_bounded_areas(
            world.time,
            &world.bounded_areas,
            &world.actors,
            &mut IntersectingStaticObject {
                shape: &Ball::new(v.body.shape.radius),
                isometry: Isometry::translation(v.position.x, v.position.y),
                resistance: &v.magick.power,
                effect: &mut effect,
            },
        );
        intersect_with_temp_and_static_areas(
            world.time,
            world.settings.gravitational_acceleration,
            &world.temp_areas,
            &world.static_areas,
            &mut IntersectingDynamicObject {
                shape: &Ball::new(v.body.shape.radius),
                velocity: v.velocity,
                isometry: Isometry::translation(v.position.x, v.position.y),
                levitating: (v.position_z - v.body.shape.radius) > f64::EPSILON,
                mass: v.body.mass(),
                resistance: &DEFAULT_RESISTANCE,
                dynamic_force: &mut v.dynamic_force,
                effect: &mut effect,
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
                        resistance: &DEFAULT_RESISTANCE,
                        effect: &mut object.effect,
                    },
                );
            });
    }
    for temp_obstacle in world.temp_obstacles.iter_mut() {
        intersect_static_object_with_all_bounded_areas(
            world.time,
            &world.bounded_areas,
            &world.actors,
            &mut IntersectingStaticObject {
                shape: &Ball::new(temp_obstacle.body.shape.radius),
                isometry: Isometry::translation(temp_obstacle.position.x, temp_obstacle.position.y),
                resistance: &DEFAULT_RESISTANCE,
                effect: &mut temp_obstacle.effect,
            },
        );
    }
}

struct IntersectingDynamicObject<'a, T: Default + PartialEq> {
    shape: &'a dyn Shape,
    velocity: Vec2f,
    isometry: Isometry<Real>,
    levitating: bool,
    mass: f64,
    resistance: &'a [T; 11],
    dynamic_force: &'a mut Vec2f,
    effect: &'a mut Effect,
}

fn intersect_with_temp_and_static_areas<T>(
    now: f64,
    gravitational_acceleration: f64,
    temp_areas: &[TempArea],
    static_areas: &[StaticArea],
    object: &mut IntersectingDynamicObject<T>,
) where
    T: Default + PartialEq,
{
    intersect_with_temp_areas(now, temp_areas, object);
    if !object.levitating {
        intersect_with_last_static_area(now, gravitational_acceleration, static_areas, object);
    }
}

fn intersect_with_temp_areas<T>(
    now: f64,
    temp_areas: &[TempArea],
    object: &mut IntersectingDynamicObject<T>,
) where
    T: Default + PartialEq,
{
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
                add_magick_to_effect(now, object.effect, &temp_area.magick, object.resistance);
        }
    }
}

fn intersect_with_last_static_area<T>(
    now: f64,
    gravitational_acceleration: f64,
    static_areas: &[StaticArea],
    object: &mut IntersectingDynamicObject<T>,
) where
    T: Default + PartialEq,
{
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
        *object.effect =
            add_magick_to_effect(now, object.effect, &static_area.magick, object.resistance);
    }
}

struct IntersectingStaticObject<'a, T: Default + PartialEq> {
    shape: &'a dyn Shape,
    isometry: Isometry<Real>,
    resistance: &'a [T; 11],
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
            resistance: &right[0].aura.elements,
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
            resistance: &left[actor_index].aura.elements,
            effect: &mut left[actor_index].effect,
        },
    );
}

fn intersect_static_object_with_all_bounded_areas<T>(
    now: f64,
    bounded_areas: &[BoundedArea],
    actors: &[Actor],
    object: &mut IntersectingStaticObject<T>,
) where
    T: Default + PartialEq,
{
    for bounded_area in bounded_areas {
        if let Some(owner) = actors.iter().find(|v| v.id == bounded_area.actor_id) {
            intersect_static_object_with_bounded_area(now, bounded_area, owner, object);
        }
    }
}

fn intersect_static_object_with_bounded_area<T>(
    now: f64,
    area: &BoundedArea,
    owner: &Actor,
    object: &mut IntersectingStaticObject<T>,
) where
    T: Default + PartialEq,
{
    let isometry = Isometry::translation(owner.position.x, owner.position.y);
    if intersection_test(
        &object.isometry,
        object.shape,
        &isometry,
        &area.body,
        owner.current_direction,
    ) {
        *object.effect = add_magick_to_effect(now, object.effect, &area.magick, object.resistance);
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
    for v in world.projectiles.iter_mut() {
        intersect_object_with_all_fields(
            &world.fields,
            &world.actors,
            &mut PushedObject {
                shape: Ball::new(v.body.shape.radius),
                position: v.position,
                dynamic_force: &mut v.dynamic_force,
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

fn update_actors(now: f64, duration: f64, settings: &WorldSettings, actors: &mut Vec<Actor>) {
    for actor in actors.iter_mut() {
        update_actor_current_direction(duration, settings.max_rotation_speed, actor);
        update_actor_dynamic_force(settings.move_force, actor);
        resist_magick(&actor.aura.elements, &mut actor.effect.power);
        damage_health(
            duration,
            settings.magical_damage_factor,
            actor.body.mass(),
            &actor.effect.power,
            &mut actor.health,
        );
        decay_effect(now, &mut actor.effect);
        decay_aura(duration, settings.decay_factor, &mut actor.aura);
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

fn update_projectiles(duration: f64, settings: &WorldSettings, projectiles: &mut Vec<Projectile>) {
    for projectile in projectiles.iter_mut() {
        update_velocity(
            duration,
            projectile.body.mass(),
            projectile.dynamic_force,
            settings.min_move_distance,
            &mut projectile.velocity,
        );
        update_position_z(
            duration,
            projectile.body.shape.radius,
            projectile.velocity_z,
            &mut projectile.position_z,
        );
        update_velocity_z(
            duration,
            projectile.body.shape.radius,
            settings.gravitational_acceleration,
            projectile.position_z,
            &mut projectile.velocity_z,
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
        decay_effect(now, &mut object.effect);
        damage_health(
            duration,
            settings.magical_damage_factor,
            object.body.mass(),
            &object.effect.power,
            &mut object.health,
        );
    }
}

fn update_shields(duration: f64, settings: &WorldSettings, shields: &mut Vec<Shield>) {
    for shield in shields.iter_mut() {
        shield.power -= duration * settings.decay_factor;
    }
}

fn update_temp_obstacles(
    duration: f64,
    settings: &WorldSettings,
    temp_obstacles: &mut Vec<TempObstacle>,
) {
    for temp_obstacle in temp_obstacles.iter_mut() {
        resist_magick(&temp_obstacle.magick.power, &mut temp_obstacle.effect.power);
        damage_health(
            duration,
            settings.magical_damage_factor,
            temp_obstacle.mass(),
            &temp_obstacle.effect.power,
            &mut temp_obstacle.health,
        );
    }
}

fn update_actor_current_direction(duration: f64, max_rotation_speed: f64, actor: &mut Actor) {
    if is_actor_immobilized(actor) {
        return;
    }
    actor.current_direction = get_current_direction(
        actor.current_direction,
        actor.target_direction,
        duration,
        max_rotation_speed,
    );
}

fn update_actor_dynamic_force(move_force: f64, actor: &mut Actor) {
    let moving = actor.moving
        && actor.delayed_magick.is_none()
        && matches!(actor.occupation, ActorOccupation::None)
        && !is_actor_immobilized(actor);
    actor.dynamic_force += actor.current_direction * move_force * moving as i32 as f64;
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

fn decay_effect(now: f64, effect: &mut Effect) {
    for i in 0..effect.power.len() {
        if now - effect.applied[i] >= get_element_duration(Element::from(i)) {
            effect.power[i] = 0.0;
        }
    }
}

fn get_element_duration(element: Element) -> f64 {
    match element {
        Element::Water => f64::MAX * f64::EPSILON,
        Element::Lightning => 0.25,
        Element::Life => 1.0,
        Element::Arcane => 0.25,
        Element::Shield => 5.0,
        Element::Earth => 0.25,
        Element::Cold => 5.0,
        Element::Fire => 5.0,
        Element::Steam => 0.25,
        Element::Ice => 5.0,
        Element::Poison => 5.0,
    }
}

fn decay_aura(duration: f64, decay_factor: f64, aura: &mut Aura) {
    aura.power -= duration * decay_factor;
    if aura.power <= 0.0 {
        aura.power = 0.0;
        aura.elements.fill(false);
    }
}

fn damage_health(
    duration: f64,
    damage_factor: f64,
    mass: f64,
    power: &[f64; 11],
    health: &mut f64,
) {
    *health = (*health - get_damage(power) * damage_factor * duration / mass).min(1.0);
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

fn add_magick_to_effect<T>(
    now: f64,
    target: &Effect,
    magick: &Magick,
    resistance: &[T; 11],
) -> Effect
where
    T: Default + PartialEq,
{
    let mut power = target.power;
    let mut applied = target.applied;
    let shield = (resistance[Element::Shield as usize] == T::default()) as i32 as f64;
    for i in 0..power.len() {
        if magick.power[i] > 0.0 {
            power[i] = power[i].max(magick.power[i])
                * (resistance[i] == T::default()) as i32 as f64
                * shield;
            applied[i] = now;
        }
    }
    let target_power = power;
    if target_power[Element::Water as usize] > 0.0 && target_power[Element::Fire as usize] > 0.0 {
        power[Element::Water as usize] = 0.0;
        power[Element::Fire as usize] = 0.0;
        power[Element::Steam as usize] = target_power[Element::Water as usize];
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
            target_power[Element::Water as usize].max(target_power[Element::Cold as usize]);
        applied[Element::Ice as usize] = now;
        power[Element::Water as usize] = 0.0;
        power[Element::Cold as usize] = 0.0;
    }
    Effect { applied, power }
}

fn resist_magick<T>(resistance: &[T; 11], power: &mut [f64; 11])
where
    T: Default + PartialEq,
{
    let shield = (resistance[Element::Shield as usize] == T::default()) as i32 as f64;
    for i in 0..power.len() {
        power[i] *= (resistance[i] == T::default()) as i32 as f64 * shield;
    }
}

fn get_damage(power: &[f64; 11]) -> f64 {
    (1.0 + power[Element::Water as usize]) * power[Element::Lightning as usize]
        / get_element_duration(Element::Lightning)
        - power[Element::Life as usize] / get_element_duration(Element::Life)
        + power[Element::Arcane as usize] / get_element_duration(Element::Arcane)
        + power[Element::Cold as usize] / get_element_duration(Element::Cold)
        + power[Element::Fire as usize] / get_element_duration(Element::Fire)
        + power[Element::Steam as usize] / get_element_duration(Element::Steam)
        + power[Element::Poison as usize] / get_element_duration(Element::Poison)
}

fn can_absorb_physical_damage(elements: &[bool; 11]) -> bool {
    elements[Element::Shield as usize] || elements[Element::Earth as usize]
}

fn can_reflect_beams(elements: &[bool; 11]) -> bool {
    elements[Element::Shield as usize]
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
    nearest_hit =
        find_beam_nearest_intersection(origin, direction, &world.projectiles, length, shape_cache)
            .map(|(i, n)| (Index::Projectile(i), n))
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
    nearest_hit =
        find_beam_nearest_intersection(origin, direction, &world.shields, length, shape_cache)
            .map(|(i, n)| (Index::Shield(i), n))
            .or(nearest_hit);
    nearest_hit = find_beam_nearest_intersection(
        origin,
        direction,
        &world.temp_obstacles,
        length,
        shape_cache,
    )
    .map(|(i, n)| (Index::TempObstacle(i), n))
    .or(nearest_hit);
    if let Some((index, mut normal)) = nearest_hit {
        let can_reflect = match index {
            Index::Actor(i) => {
                world.actors[i].effect = add_magick_to_effect(
                    world.time,
                    &world.actors[i].effect,
                    &magick,
                    &world.actors[i].aura.elements,
                );
                can_reflect_beams(&world.actors[i].aura.elements)
            }
            Index::Projectile(..) => false,
            Index::StaticObject(i) => {
                world.static_objects[i].effect = add_magick_to_effect(
                    world.time,
                    &world.static_objects[i].effect,
                    &magick,
                    &DEFAULT_RESISTANCE,
                );
                false
            }
            Index::Shield(..) => true,
            Index::TempObstacle(..) => false,
        };
        if depth < world.settings.max_beam_depth as usize && can_reflect {
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

impl WithIsometry for Projectile {
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

impl WithIsometry for Shield {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::new(
            Vector2::new(self.position.x, self.position.y),
            self.body.shape.rotation,
        )
    }
}

impl WithIsometry for TempObstacle {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
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

impl WithShape for Projectile {
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

impl WithShape for Shield {
    fn with_shape(&self, shape_cache: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        self.body
            .shape
            .clone()
            .with_shape(shape_cache, |shape| (*f)(shape))
    }
}

impl WithShape for TempObstacle {
    fn with_shape(&self, _: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        (*f)(&self.body.shape.as_shape())
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
            for (j, projectile) in world.projectiles.iter().enumerate() {
                if let Some(toi) =
                    time_of_impact(duration_left, shape_cache, static_object, projectile)
                {
                    update_earliest_collision(
                        Index::StaticObject(i),
                        Index::Projectile(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
        }
        for (i, shield) in world.shields.iter().enumerate() {
            for (j, actor) in world.actors.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, shield, actor) {
                    update_earliest_collision(
                        Index::Shield(i),
                        Index::Actor(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
            for (j, projectile) in world.projectiles.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, shield, projectile) {
                    update_earliest_collision(
                        Index::Shield(i),
                        Index::Projectile(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
        }
        for (i, temp_obstacle) in world.temp_obstacles.iter().enumerate() {
            for (j, actor) in world.actors.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, temp_obstacle, actor)
                {
                    update_earliest_collision(
                        Index::TempObstacle(i),
                        Index::Actor(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
            for (j, projectile) in world.projectiles.iter().enumerate() {
                if let Some(toi) =
                    time_of_impact(duration_left, shape_cache, temp_obstacle, projectile)
                {
                    update_earliest_collision(
                        Index::TempObstacle(i),
                        Index::Projectile(j),
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
        for (i, projectile) in world.projectiles.iter().enumerate() {
            for (j, actor) in world.actors.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, projectile, actor) {
                    update_earliest_collision(
                        Index::Projectile(i),
                        Index::Actor(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
            for (j, shield) in world.shields.iter().enumerate() {
                if let Some(toi) = time_of_impact(duration_left, shape_cache, projectile, shield) {
                    update_earliest_collision(
                        Index::Projectile(i),
                        Index::Shield(j),
                        toi,
                        &mut earliest_collision,
                    );
                }
            }
        }
        if !world.projectiles.is_empty() {
            for i in 0..world.projectiles.len() - 1 {
                for j in i + 1..world.projectiles.len() {
                    if let Some(toi) = time_of_impact(
                        duration_left,
                        shape_cache,
                        &world.projectiles[i],
                        &world.projectiles[j],
                    ) {
                        update_earliest_collision(
                            Index::Projectile(i),
                            Index::Projectile(j),
                            toi,
                            &mut earliest_collision,
                        );
                    }
                }
            }
        }
        if let Some(collision) = earliest_collision.as_ref() {
            let apply_impact = ApplyImpact {
                now: world.time + (duration - duration_left),
                damage_factor: world.settings.physical_damage_factor,
                shape_cache,
                epsilon_duration: duration / 100.0,
                toi: &collision.toi,
            };
            collide_objects(collision.lhs, collision.rhs, &apply_impact, world);
            for (i, actor) in world.actors.iter_mut().enumerate() {
                if Index::Actor(i) != collision.lhs && Index::Actor(i) != collision.rhs {
                    update_position(collision.toi.toi, actor.velocity, &mut actor.position)
                }
            }
            for (i, projectile) in world.projectiles.iter_mut().enumerate() {
                if Index::Projectile(i) != collision.lhs && Index::Projectile(i) != collision.rhs {
                    update_position(
                        collision.toi.toi,
                        projectile.velocity,
                        &mut projectile.position,
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
                .projectiles
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

impl WithVelocity for Projectile {
    fn velocity(&self) -> Vec2f {
        self.velocity
    }
}

impl WithVelocity for StaticObject {
    fn velocity(&self) -> Vec2f {
        Vec2f::ZERO
    }
}

impl WithVelocity for Shield {
    fn velocity(&self) -> Vec2f {
        Vec2f::ZERO
    }
}

impl WithVelocity for TempObstacle {
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

trait CollidingObject<T: Default + PartialEq>: WithVelocity + WithShape + WithIsometry {
    fn material(&self) -> Material;
    fn mass(&self) -> f64;
    fn position(&self) -> Vec2f;
    fn set_position(&mut self, value: Vec2f);
    fn set_velocity(&mut self, value: Vec2f);
    fn magick(&self) -> &Magick;
    fn resistance(&self) -> &[T; 11];
    fn effect(&self) -> &Effect;
    fn set_effect(&mut self, value: Effect);
    fn health(&self) -> f64;
    fn set_health(&mut self, value: f64);
    fn aura(&self) -> &Aura;
    fn is_static(&self) -> bool;
}

fn collide_objects(lhs: Index, rhs: Index, apply_impact: &ApplyImpact, world: &mut World) {
    if lhs > rhs {
        collide_ordered_objects(rhs, lhs, apply_impact, world)
    } else {
        collide_ordered_objects(lhs, rhs, apply_impact, world)
    }
}

fn collide_ordered_objects(lhs: Index, rhs: Index, f: &ApplyImpact, world: &mut World) {
    match lhs {
        Index::Actor(lhs) => match rhs {
            Index::Actor(rhs) => {
                let (left, right) = world.actors.split_at_mut(rhs);
                f.call(&mut left[lhs], &mut right[0])
            }
            Index::Projectile(rhs) => f.call(&mut world.actors[lhs], &mut world.projectiles[rhs]),
            Index::StaticObject(rhs) => {
                f.call(&mut world.actors[lhs], &mut world.static_objects[rhs])
            }
            Index::Shield(rhs) => f.call(&mut world.actors[lhs], &mut world.shields[rhs]),
            Index::TempObstacle(rhs) => {
                f.call(&mut world.actors[lhs], &mut world.temp_obstacles[rhs])
            }
        },
        Index::Projectile(lhs) => match rhs {
            Index::Actor(rhs) => f.call(&mut world.projectiles[lhs], &mut world.actors[rhs]),
            Index::Projectile(rhs) => {
                let (left, right) = world.projectiles.split_at_mut(rhs);
                f.call(&mut left[lhs], &mut right[0])
            }
            Index::StaticObject(rhs) => {
                f.call(&mut world.projectiles[lhs], &mut world.static_objects[rhs])
            }
            Index::Shield(rhs) => f.call(&mut world.projectiles[lhs], &mut world.shields[rhs]),
            Index::TempObstacle(rhs) => {
                f.call(&mut world.projectiles[lhs], &mut world.temp_obstacles[rhs])
            }
        },
        Index::StaticObject(lhs) => match rhs {
            Index::Actor(rhs) => f.call(&mut world.static_objects[lhs], &mut world.actors[rhs]),
            Index::Projectile(rhs) => {
                f.call(&mut world.static_objects[lhs], &mut world.projectiles[rhs])
            }
            Index::StaticObject(rhs) => {
                let (left, right) = world.static_objects.split_at_mut(rhs);
                f.call(&mut left[lhs], &mut right[0])
            }
            Index::Shield(rhs) => f.call(&mut world.static_objects[lhs], &mut world.shields[rhs]),
            Index::TempObstacle(rhs) => f.call(
                &mut world.static_objects[lhs],
                &mut world.temp_obstacles[rhs],
            ),
        },
        Index::Shield(lhs) => match rhs {
            Index::Actor(rhs) => f.call(&mut world.shields[lhs], &mut world.actors[rhs]),
            Index::Projectile(rhs) => f.call(&mut world.shields[lhs], &mut world.projectiles[rhs]),
            Index::StaticObject(rhs) => {
                f.call(&mut world.shields[lhs], &mut world.static_objects[rhs])
            }
            Index::Shield(rhs) => {
                let (left, right) = world.shields.split_at_mut(rhs);
                f.call(&mut left[lhs], &mut right[0])
            }
            Index::TempObstacle(rhs) => {
                f.call(&mut world.shields[lhs], &mut world.temp_obstacles[rhs])
            }
        },
        Index::TempObstacle(lhs) => match rhs {
            Index::Actor(rhs) => f.call(&mut world.temp_obstacles[lhs], &mut world.actors[rhs]),
            Index::Projectile(rhs) => {
                f.call(&mut world.temp_obstacles[lhs], &mut world.projectiles[rhs])
            }
            Index::StaticObject(rhs) => f.call(
                &mut world.temp_obstacles[lhs],
                &mut world.static_objects[rhs],
            ),
            Index::Shield(rhs) => f.call(&mut world.temp_obstacles[lhs], &mut world.shields[rhs]),
            Index::TempObstacle(rhs) => {
                let (left, right) = world.temp_obstacles.split_at_mut(rhs);
                f.call(&mut left[lhs], &mut right[0])
            }
        },
    }
}

struct ApplyImpact<'a> {
    now: f64,
    damage_factor: f64,
    epsilon_duration: f64,
    shape_cache: &'a ShapeCache,
    toi: &'a TOI,
}

impl<'a> ApplyImpact<'a> {
    fn call<L, R>(&self, lhs: &mut dyn CollidingObject<L>, rhs: &mut dyn CollidingObject<R>)
    where
        L: Default + PartialEq,
        R: Default + PartialEq,
    {
        apply_impact(
            self.now,
            self.damage_factor,
            self.epsilon_duration,
            self.shape_cache,
            self.toi,
            lhs,
            rhs,
        );
    }
}

fn apply_impact<L, R>(
    now: f64,
    damage_factor: f64,
    epsilon_duration: f64,
    shape_cache: &ShapeCache,
    toi: &TOI,
    lhs: &mut dyn CollidingObject<L>,
    rhs: &mut dyn CollidingObject<R>,
) where
    L: Default + PartialEq,
    R: Default + PartialEq,
{
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
    let new_lhs_effect = add_magick_to_effect(now, lhs.effect(), rhs.magick(), lhs.resistance());
    let new_rhs_effect = add_magick_to_effect(now, rhs.effect(), lhs.magick(), rhs.resistance());
    lhs.set_effect(new_lhs_effect);
    rhs.set_effect(new_rhs_effect);
    handle_collision_damage(lhs_kinetic_energy, damage_factor, lhs_velocity, lhs);
    handle_collision_damage(rhs_kinetic_energy, damage_factor, rhs_velocity, rhs);
}

fn get_contact<L, R>(
    shape_cache: &ShapeCache,
    lhs: &dyn CollidingObject<L>,
    rhs: &dyn CollidingObject<R>,
) -> Option<Contact>
where
    L: Default + PartialEq,
    R: Default + PartialEq,
{
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

impl CollidingObject<bool> for Actor {
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

    fn magick(&self) -> &Magick {
        &DEFAULT_MAGICK
    }

    fn resistance(&self) -> &[bool; 11] {
        &self.aura.elements
    }

    fn effect(&self) -> &Effect {
        &self.effect
    }

    fn set_effect(&mut self, value: Effect) {
        self.effect = value;
    }

    fn health(&self) -> f64 {
        self.health
    }

    fn set_health(&mut self, value: f64) {
        self.health = value;
    }

    fn aura(&self) -> &Aura {
        &self.aura
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject<f64> for Projectile {
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

    fn magick(&self) -> &Magick {
        &self.magick
    }

    fn resistance(&self) -> &[f64; 11] {
        &self.magick.power
    }

    fn effect(&self) -> &Effect {
        &DEFAULT_EFFECT
    }

    fn set_effect(&mut self, _: Effect) {}

    fn health(&self) -> f64 {
        self.health
    }

    fn set_health(&mut self, value: f64) {
        self.health = value;
    }

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject<bool> for StaticObject {
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

    fn magick(&self) -> &Magick {
        &DEFAULT_MAGICK
    }

    fn resistance(&self) -> &[bool; 11] {
        &DEFAULT_RESISTANCE
    }

    fn effect(&self) -> &Effect {
        &self.effect
    }

    fn set_effect(&mut self, value: Effect) {
        self.effect = value;
    }

    fn health(&self) -> f64 {
        self.health
    }

    fn set_health(&mut self, value: f64) {
        self.health = value;
    }

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

impl CollidingObject<bool> for Shield {
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

    fn magick(&self) -> &Magick {
        &DEFAULT_MAGICK
    }

    fn resistance(&self) -> &[bool; 11] {
        &[true; 11]
    }

    fn effect(&self) -> &Effect {
        &DEFAULT_EFFECT
    }

    fn set_effect(&mut self, _: Effect) {}

    fn health(&self) -> f64 {
        0.0
    }

    fn set_health(&mut self, _: f64) {}

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

impl CollidingObject<f64> for TempObstacle {
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

    fn magick(&self) -> &Magick {
        &self.magick
    }

    fn resistance(&self) -> &[f64; 11] {
        &self.magick.power
    }

    fn effect(&self) -> &Effect {
        &self.effect
    }

    fn set_effect(&mut self, value: Effect) {
        self.effect = value;
    }

    fn health(&self) -> f64 {
        self.health
    }

    fn set_health(&mut self, value: f64) {
        self.health = value;
    }

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

fn handle_collision_damage<T>(
    prev_kinetic_energy: f64,
    damage_factor: f64,
    velocity: Vec2f,
    object: &mut dyn CollidingObject<T>,
) where
    T: Default + PartialEq,
{
    if !can_absorb_physical_damage(&object.aura().elements) {
        let health = object.health();
        object.set_health(
            health
                - (get_kinetic_energy(object.mass(), velocity) - prev_kinetic_energy).abs()
                    * damage_factor
                    / object.mass(),
        );
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
        match actor.delayed_magick.as_ref().map(|v| v.status) {
            Some(DelayedMagickStatus::Started) => (),
            Some(DelayedMagickStatus::Throw) => {
                let delayed_magick = actor.delayed_magick.take().unwrap();
                let radius = delayed_magick.power.iter().sum::<f64>() * actor.body.shape.radius
                    / world.settings.max_magic_power;
                let material = Material::Stone;
                let mut power = delayed_magick.power;
                power[Element::Earth as usize] = 0.0;
                world.projectiles.push(Projectile {
                    id: ProjectileId(get_next_id(&mut world.id_counter)),
                    body: Body {
                        shape: Disk { radius },
                        material,
                    },
                    position: actor.position
                        + actor.current_direction
                            * (actor.body.shape.radius + radius + world.settings.margin),
                    health: 1.0,
                    magick: Magick { power },
                    velocity: actor.velocity,
                    dynamic_force: actor.current_direction
                        * ((world.time - delayed_magick.started)
                            .min(world.settings.max_magic_power)
                            * world.settings.magic_force_multiplier),
                    position_z: 1.5 * actor.body.shape.radius,
                    velocity_z: 0.0,
                });
                actor.delayed_magick = None;
            }
            Some(DelayedMagickStatus::Shoot) => {
                let delayed_magick = actor.delayed_magick.take().unwrap();
                let gun_id = GunId(get_next_id(&mut world.id_counter));
                let shots = (delayed_magick.power[Element::Ice as usize] + 2.0).round();
                let mut bullet_power = delayed_magick.power;
                bullet_power.iter_mut().for_each(|v| *v /= shots);
                world.guns.push(Gun {
                    id: gun_id,
                    actor_id: actor.id,
                    shots_left: shots as u64,
                    shot_period: world.settings.base_gun_fire_period / shots,
                    bullet_force_factor: ((world.time - delayed_magick.started)
                        .min(world.settings.max_magic_power)
                        * world.settings.magic_force_multiplier)
                        / shots,
                    bullet_power,
                    last_shot: 0.0,
                });
                actor.delayed_magick = None;
                actor.occupation = ActorOccupation::Shooting(gun_id);
            }
            None => (),
        }
    }
}

fn update_actor_occupations(world: &mut World) {
    for actor in world.actors.iter_mut() {
        match actor.occupation {
            ActorOccupation::None => (),
            ActorOccupation::Shooting(gun_id) => {
                if !world.guns.iter().any(|v| v.id == gun_id) {
                    actor.occupation = ActorOccupation::None;
                }
            }
            ActorOccupation::Spraying {
                bounded_area_id, ..
            } => {
                if !world.bounded_areas.iter().any(|v| v.id == bounded_area_id) {
                    actor.occupation = ActorOccupation::None;
                }
            }
            ActorOccupation::Beaming(beam_id) => {
                if !world.beams.iter().any(|v| v.id == beam_id) {
                    actor.occupation = ActorOccupation::None;
                }
            }
        }
    }
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

fn spawn_player_actors<R: Rng>(world: &mut World, rng: &mut R) {
    for player in world.players.iter_mut() {
        if player.active && player.actor_id.is_none() && player.spawn_time <= world.time {
            let actor_id = ActorId(get_next_id(&mut world.id_counter));
            world.actors.push(generate_player_actor(
                actor_id,
                player.id,
                player.name.clone(),
                &world.bounds,
                rng,
            ));
            player.actor_id = Some(actor_id);
        }
    }
}

fn update_player_spawn_time(world: &mut World) {
    for player in world.players.iter_mut() {
        if let Some(actor_id) = player.actor_id {
            if !world.actors.iter().any(|v| v.id == actor_id) {
                player.actor_id = None;
                player.spawn_time = world.time + world.settings.player_actor_respawn_delay;
                player.deaths += 1;
            }
        }
    }
}

fn shoot_from_guns<R: Rng>(world: &mut World, rng: &mut R) {
    for gun in world.guns.iter_mut() {
        if gun.last_shot + gun.shot_period > world.time {
            continue;
        }
        if let Some(actor) = world.actors.iter().find(|v| v.id == gun.actor_id) {
            gun.last_shot = world.time;
            gun.shots_left -= 1;
            let radius = world.settings.gun_bullet_radius;
            world.projectiles.push(Projectile {
                id: ProjectileId(get_next_id(&mut world.id_counter)),
                body: Body {
                    shape: Disk { radius },
                    material: Material::Ice,
                },
                position: actor.position
                    + actor.current_direction
                        * (actor.body.shape.radius + radius + world.settings.margin),
                health: 1.0,
                magick: Magick {
                    power: gun.bullet_power,
                },
                velocity: actor.velocity,
                dynamic_force: (actor.current_direction * gun.bullet_force_factor).rotated(
                    rng.gen_range(
                        -world.settings.gun_half_grouping_angle
                            ..world.settings.gun_half_grouping_angle,
                    ),
                ),
                position_z: 1.5 * actor.body.shape.radius,
                velocity_z: 0.0,
            });
        } else {
            gun.shots_left = 0;
        }
    }
}

fn is_actor_immobilized(actor: &Actor) -> bool {
    actor.effect.power[Element::Ice as usize] > 0.0
}

fn remove_inactive_actors_occupation_results(world: &mut World) {
    for actor in world.actors.iter() {
        if actor.active {
            continue;
        }
        match actor.occupation {
            ActorOccupation::None => (),
            ActorOccupation::Shooting(gun_id) => world.guns.retain(|v| v.id != gun_id),
            ActorOccupation::Spraying {
                bounded_area_id,
                field_id,
            } => {
                world.bounded_areas.retain(|v| v.id != bounded_area_id);
                world.fields.retain(|v| v.id != field_id);
            }
            ActorOccupation::Beaming(beam_id) => world.beams.retain(|v| v.id != beam_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::distance;
    use parry2d_f64::na::Unit;
    use parry2d_f64::query::TOIStatus;

    use crate::engine::*;

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
    fn time_of_impact_should_find_impact_for_shield_circle_arc_and_moving_disk() {
        use std::f64::consts::FRAC_PI_2;
        let duration = 10.0;
        let shape_cache = ShapeCache::default();
        let shield = Shield {
            id: ShieldId(1),
            actor_id: ActorId(0),
            body: Body {
                shape: CircleArc {
                    radius: 5.0,
                    length: FRAC_PI_2,
                    rotation: -2.6539321938108684,
                },
                material: Material::None,
            },
            position: Vec2f::new(-33.23270204831895, -32.3454131103618),
            power: 0.0,
        };
        let projectile = Projectile {
            id: ProjectileId(2),
            body: Body {
                shape: Disk { radius: 0.2 },
                material: Material::Stone,
            },
            position: Vec2f::new(-34.41147614376544, -32.89358428062188),
            health: 1.0,
            magick: Magick::default(),
            velocity: Vec2f::new(-737.9674461149048, -343.18066550098706),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.5,
            velocity_z: -0.08166666666666667,
        };
        let toi = time_of_impact(duration, &shape_cache, &shield, &projectile).unwrap();
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
