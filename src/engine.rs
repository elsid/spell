use std::cell::RefCell;

use parry2d_f64::math::{Isometry, Real, Vector};
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
    GunId, Magick, MaterialType, Projectile, ProjectileId, Rectangle, RingSector, Shield, ShieldId,
    StaticArea, StaticAreaShape, StaticObject, StaticShape, TempArea, TempAreaId, TempObstacle,
    TempObstacleId, World, WorldSettings,
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
    events: Vec<EngineEvent>,
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
        remove_intersecting_objects(world, &self.shape_cache);
        world.bounded_areas.retain(|v| v.deadline >= now);
        world.fields.retain(|v| v.deadline >= now);
        world.beams.retain(|v| v.deadline >= now);
        world.temp_areas.retain(|v| v.deadline >= now);
        world.guns.retain(|v| v.shots_left > 0);
        world.shields.retain(|v| v.power > 0.0);
        world.temp_obstacles.retain(|v| v.deadline >= now);
        self.events.clear();
        update_actor_occupations(world);
        spawn_player_actors(world, rng);
        shoot_from_guns(world, rng);
        intersect_objects_with_areas(world, &self.shape_cache);
        intersect_objects_with_all_fields(world);
        update_actors(
            world.time,
            duration,
            &world.settings,
            &mut world.actors,
            &mut self.events,
        );
        update_projectiles(duration, &world.settings, &mut world.projectiles);
        update_static_objects(
            world.time,
            duration,
            &world.settings,
            &mut world.static_objects,
            &mut self.events,
        );
        update_shields(duration, &world.settings, &mut world.shields);
        update_temp_obstacles(
            duration,
            &world.settings,
            &mut world.temp_obstacles,
            &mut self.events,
        );
        self.update_beams(world);
        move_objects(duration, world, &self.shape_cache, &mut self.events);
        handle_events(&self.events, world);
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
            v.active =
                v.active && is_active(&bounds, &v.body.shape.as_shape(), v.position, v.health)
        });
        remove_inactive_actors_occupation_results(world);
        world.players.retain(|v| v.active);
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
        if let Some(actor) = world.actors.iter_mut().find(|v| v.player_id == player_id) {
            actor.active = false;
        }
    }
}

pub fn add_actor_spell_element(actor_index: usize, element: Element, world: &mut World) {
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None)
        || is_actor_flying(&world.actors[actor_index])
    {
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
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None)
        || is_actor_flying(&world.actors[actor_index])
    {
        return;
    }
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        cast_shield(std::f64::consts::FRAC_PI_2, magick, actor_index, world);
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
    if !matches!(world.actors[actor_index].occupation, ActorOccupation::None)
        || is_actor_flying(&world.actors[actor_index])
    {
        return;
    }
    let magick = Spell::on(
        world.settings.max_spell_elements as usize,
        &mut world.actors[actor_index].spell_elements,
    )
    .cast();
    if magick.power[Element::Shield as usize] > 0.0 {
        cast_shield(std::f64::consts::TAU, magick, actor_index, world);
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
fn cast_shield(angle: f64, magick: Magick, actor_index: usize, world: &mut World) {
    if magick.power[Element::Earth as usize] > 0.0 {
        cast_obstacle_based_shield(
            angle,
            magick,
            Element::Earth,
            MaterialType::Stone,
            actor_index,
            world,
        );
    } else if magick.power[Element::Ice as usize] > 0.0 {
        cast_obstacle_based_shield(
            angle,
            magick,
            Element::Ice,
            MaterialType::Ice,
            actor_index,
            world,
        );
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
        cast_spray_based_shield(angle, magick, actor_index, world);
    } else {
        cast_reflecting_shield(angle, actor_index, world);
    }
}

fn cast_obstacle_based_shield(
    angle: f64,
    mut magick: Magick,
    element: Element,
    material_type: MaterialType,
    actor_index: usize,
    world: &mut World,
) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    magick.power[Element::Shield as usize] = 0.0;
    magick.power[element as usize] = 0.0;
    let number = get_number_of_shield_objects(angle);
    let half = number / 2;
    for i in -half..number - half {
        world.temp_obstacles.push(TempObstacle {
            id: TempObstacleId(get_next_id(&mut world.id_counter)),
            actor_id: actor.id,
            body: Body {
                shape: Disk {
                    radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                },
                material_type,
            },
            position: actor.position
                + actor
                    .current_direction
                    .rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64)
                    * (distance + 0.1),
            health: 1.0,
            magick: magick.clone(),
            effect: Effect::default(),
            deadline: world.time + world.settings.temp_obstacle_magick_duration,
        });
    }
}

fn cast_spray_based_shield(angle: f64, mut magick: Magick, actor_index: usize, world: &mut World) {
    let actor = &world.actors[actor_index];
    let distance = 5.0;
    magick.power[Element::Shield as usize] = 0.0;
    let number = get_number_of_shield_objects(angle);
    let half = number / 2;
    for i in -half..number - half {
        world.temp_areas.push(TempArea {
            id: TempAreaId(get_next_id(&mut world.id_counter)),
            body: Body {
                shape: Disk {
                    radius: distance * std::f64::consts::PI / (2 * 5 * 2) as f64,
                },
                material_type: MaterialType::None,
            },
            position: actor.position
                + actor
                    .current_direction
                    .rotated(i as f64 * std::f64::consts::PI / (2 * 5) as f64)
                    * (distance + 0.1),
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
            material_type: MaterialType::None,
        },
        position: actor.position,
        created: world.time,
        power: 1.0,
    });
}

fn add_delayed_magick(magick: Magick, actor_index: usize, world: &mut World) {
    if is_actor_in_panic(&world.actors[actor_index]) {
        return;
    }
    world.actors[actor_index].delayed_magick = Some(DelayedMagick {
        started: world.time,
        status: DelayedMagickStatus::Started,
        power: magick.power,
    });
}

fn add_beam(mut magick: Magick, actor_index: usize, world: &mut World) {
    if is_actor_in_panic(&world.actors[actor_index]) {
        return;
    }
    let beam_id = BeamId(get_next_id(&mut world.id_counter));
    magick
        .power
        .iter_mut()
        .enumerate()
        .for_each(|(e, v)| *v *= get_element_duration(Element::from(e)));
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
    if is_actor_in_panic(&world.actors[actor_index]) {
        return;
    }
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
        self.shape.volume() * self.material_type.density()
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
            StaticShape::Rectangle(v) => v.volume(),
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

impl WithVolume for Rectangle {
    fn volume(&self) -> f64 {
        self.width * self.height
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
            StaticShape::Rectangle(v) => f(&v.as_shape()),
        }
    }
}

impl StaticAreaShape {
    fn with_shape<R, F: FnMut(&dyn Shape) -> R>(&self, _: &ShapeCache, mut f: F) -> R {
        match &self {
            StaticAreaShape::Disk(v) => f(&v.as_shape()),
            StaticAreaShape::Rectangle(v) => f(&v.as_shape()),
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

impl Rectangle {
    fn as_shape(&self) -> Cuboid {
        Cuboid::new(Vector::new(self.width * 0.5, self.height * 0.5))
    }
}

impl MaterialType {
    fn density(self) -> f64 {
        match self {
            MaterialType::None => 1.0,
            MaterialType::Flesh => 800.0,
            MaterialType::Stone => 2750.0,
            MaterialType::Grass => 500.0,
            MaterialType::Dirt => 1500.0,
            MaterialType::Water => 1000.0,
            MaterialType::Ice => 900.0,
        }
    }

    fn restitution(self) -> f64 {
        match self {
            MaterialType::None => 1.0,
            MaterialType::Flesh => 0.05,
            MaterialType::Stone => 0.2,
            MaterialType::Grass => 0.01,
            MaterialType::Dirt => 0.01,
            MaterialType::Water => 0.0,
            MaterialType::Ice => 0.01,
        }
    }

    fn sliding_resistance(self) -> f64 {
        match self {
            MaterialType::None => 0.0,
            MaterialType::Flesh => 1.0,
            MaterialType::Stone => 1.0,
            MaterialType::Grass => 0.5,
            MaterialType::Dirt => 1.0,
            MaterialType::Water => 1.0,
            MaterialType::Ice => 0.05,
        }
    }

    fn walking_resistance(self) -> f64 {
        match self {
            MaterialType::None => 0.0,
            MaterialType::Flesh => 1.0,
            MaterialType::Stone => 0.025,
            MaterialType::Grass => 0.05,
            MaterialType::Dirt => 0.1,
            MaterialType::Water => 0.5,
            MaterialType::Ice => 0.05,
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

fn remove_intersecting_objects(world: &mut World, shape_cache: &ShapeCache) {
    if !world.temp_obstacles.is_empty() {
        for i in 0..world.temp_obstacles.len() - 1 {
            for j in i + 1..world.temp_obstacles.len() {
                if intersection_test(
                    &world.temp_obstacles[i],
                    &world.temp_obstacles[j],
                    shape_cache,
                ) {
                    world.temp_obstacles[i].deadline = 0.0;
                    break;
                }
            }
        }
    }
    if !world.shields.is_empty() {
        for i in 0..world.shields.len() - 1 {
            for j in i + 1..world.shields.len() {
                if intersection_test(&world.shields[i], &world.shields[j], shape_cache) {
                    world.shields[i].power = 0.0;
                    break;
                }
            }
        }
    }
    if !world.temp_areas.is_empty() {
        for i in 0..world.temp_areas.len() - 1 {
            for j in i + 1..world.temp_areas.len() {
                if intersection_test(&world.temp_areas[i], &world.temp_areas[j], shape_cache) {
                    world.temp_areas[i].deadline = 0.0;
                    break;
                }
            }
        }
    }
    for temp_obstacle in world.temp_obstacles.iter_mut() {
        if temp_obstacle.deadline == 0.0 {
            continue;
        }
        for static_object in world.static_objects.iter() {
            if intersection_test(temp_obstacle, static_object, shape_cache) {
                temp_obstacle.deadline = 0.0;
                break;
            }
        }
        if temp_obstacle.deadline == 0.0 {
            continue;
        }
        for shield in world.shields.iter_mut() {
            if shield.power == 0.0 {
                continue;
            }
            if intersection_test(temp_obstacle, shield, shape_cache) {
                if shield.created
                    >= temp_obstacle.deadline - world.settings.temp_obstacle_magick_duration
                {
                    temp_obstacle.deadline = 0.0;
                } else {
                    shield.power = 0.0;
                    break;
                }
            }
        }
        if temp_obstacle.deadline == 0.0 {
            continue;
        }
        for temp_area in world.temp_areas.iter_mut() {
            if temp_area.deadline == 0.0 {
                continue;
            }
            if intersection_test(temp_obstacle, temp_area, shape_cache) {
                if temp_area.deadline - world.settings.temp_area_duration
                    >= temp_obstacle.deadline - world.settings.temp_obstacle_magick_duration
                {
                    temp_obstacle.deadline = 0.0;
                    break;
                } else {
                    temp_area.deadline = 0.0;
                }
            }
        }
    }
    for shield in world.shields.iter_mut() {
        if shield.power == 0.0 {
            continue;
        }
        for static_object in world.static_objects.iter() {
            if intersection_test(shield, static_object, shape_cache) {
                shield.power = 0.0;
                break;
            }
        }
        if shield.power == 0.0 {
            continue;
        }
        for temp_area in world.temp_areas.iter_mut() {
            if temp_area.deadline == 0.0 {
                continue;
            }
            if intersection_test(shield, temp_area, shape_cache) {
                if shield.created
                    >= temp_area.deadline - world.settings.temp_obstacle_magick_duration
                {
                    temp_area.deadline = 0.0;
                } else {
                    shield.power = 0.0;
                    break;
                }
            }
        }
    }
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
                movement_type: get_actor_movement_type(actor),
                mass: actor.body.mass(),
                resistance: &actor.aura.elements,
                dynamic_force: &mut actor.dynamic_force,
                effect: &mut actor.effect,
            },
            shape_cache,
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
                movement_type: get_projectile_movement_type(v),
                mass: v.body.mass(),
                resistance: &DEFAULT_RESISTANCE,
                dynamic_force: &mut v.dynamic_force,
                effect: &mut effect,
            },
            shape_cache,
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

fn get_actor_movement_type(actor: &Actor) -> MovementType {
    if is_actor_flying(actor) {
        return MovementType::Flying;
    }
    if actor.moving {
        MovementType::Walking
    } else {
        MovementType::Sliding
    }
}

fn get_projectile_movement_type(projectile: &Projectile) -> MovementType {
    if (projectile.position_z - projectile.body.shape.radius) > f64::EPSILON {
        return MovementType::Flying;
    }
    MovementType::Sliding
}

struct IntersectingDynamicObject<'a, T: Default + PartialEq> {
    shape: &'a dyn Shape,
    velocity: Vec2f,
    isometry: Isometry<Real>,
    movement_type: MovementType,
    mass: f64,
    resistance: &'a [T; 11],
    dynamic_force: &'a mut Vec2f,
    effect: &'a mut Effect,
}

enum MovementType {
    Flying,
    Sliding,
    Walking,
}

fn intersect_with_temp_and_static_areas<T>(
    now: f64,
    gravitational_acceleration: f64,
    temp_areas: &[TempArea],
    static_areas: &[StaticArea],
    object: &mut IntersectingDynamicObject<T>,
    shape_cache: &ShapeCache,
) where
    T: Default + PartialEq,
{
    intersect_with_temp_areas(now, temp_areas, object);
    if !matches!(object.movement_type, MovementType::Flying) {
        intersect_with_last_static_area(
            now,
            gravitational_acceleration,
            static_areas,
            object,
            shape_cache,
        );
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
    shape_cache: &ShapeCache,
) where
    T: Default + PartialEq,
{
    if let Some(static_area) = static_areas
        .iter()
        .rev()
        .find(|v| intersection_test(object, *v, shape_cache))
    {
        match object.movement_type {
            MovementType::Flying => (),
            MovementType::Sliding => {
                add_sliding_resistance(
                    object.mass,
                    object.velocity,
                    static_area.body.material_type,
                    gravitational_acceleration,
                    object.dynamic_force,
                );
                *object.effect = add_magick_to_effect(
                    now,
                    object.effect,
                    &static_area.magick,
                    object.resistance,
                );
            }
            MovementType::Walking => {
                add_walking_resistance(
                    object.mass,
                    object.velocity,
                    static_area.body.material_type,
                    gravitational_acceleration,
                    object.dynamic_force,
                );
                *object.effect = add_magick_to_effect(
                    now,
                    object.effect,
                    &static_area.magick,
                    object.resistance,
                );
            }
        }
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
    if intersection_test_with_ring_sector(
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
                gravity_force: right[0].mass() * world.settings.gravitational_acceleration,
                height: 2.0 * right[0].body.shape.radius,
                dynamic_force: &mut right[0].dynamic_force,
                position_z: &mut right[0].position_z,
            },
        );
        let (left, right) = world.actors.split_at_mut(i + 1);
        intersect_object_with_all_fields(
            &world.fields,
            right,
            &mut PushedObject {
                shape: Ball::new(left[i].body.shape.radius),
                position: left[i].position,
                gravity_force: left[i].mass() * world.settings.gravitational_acceleration,
                height: 2.0 * left[i].body.shape.radius,
                dynamic_force: &mut left[i].dynamic_force,
                position_z: &mut left[i].position_z,
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
                gravity_force: v.mass() * world.settings.gravitational_acceleration,
                height: 2.0 * v.body.shape.radius,
                dynamic_force: &mut v.dynamic_force,
                position_z: &mut v.position_z,
            },
        );
    }
}

struct PushedObject<'a, S: Shape> {
    shape: S,
    position: Vec2f,
    gravity_force: f64,
    height: f64,
    dynamic_force: &'a mut Vec2f,
    position_z: &'a mut f64,
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
    if intersection_test_with_ring_sector(
        &isometry,
        &object.shape,
        &owner.get_isometry(),
        &field.body,
        owner.current_direction,
    ) {
        push_object(owner.position, field.force, field.body.max_radius, object);
    }
}

fn push_object<S: Shape>(from: Vec2f, force: f64, max_distance: f64, object: &mut PushedObject<S>) {
    let to_position = object.position - from;
    let add_force = to_position * ((1.0 / to_position.norm() - 1.0 / max_distance) * force);
    if add_force.norm() > object.gravity_force {
        *object.position_z = 1.1 * object.height;
    }
    *object.dynamic_force += add_force;
}

fn update_actors(
    now: f64,
    duration: f64,
    settings: &WorldSettings,
    actors: &mut Vec<Actor>,
    events: &mut Vec<EngineEvent>,
) {
    for (index, actor) in actors.iter_mut().enumerate() {
        update_actor_current_direction(now, duration, settings.max_rotation_speed, actor);
        update_actor_dynamic_force(
            duration,
            settings.move_force,
            settings.max_actor_speed,
            actor,
        );
        resist_magick(&actor.aura.elements, &mut actor.effect.power);
        add_damage(
            Index::Actor(index),
            duration,
            settings.magical_damage_factor,
            actor.body.mass(),
            &actor.effect.power,
            events,
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
    events: &mut Vec<EngineEvent>,
) {
    for (index, object) in static_objects.iter_mut().enumerate() {
        decay_effect(now, &mut object.effect);
        add_damage(
            Index::StaticObject(index),
            duration,
            settings.magical_damage_factor,
            object.body.mass(),
            &object.effect.power,
            events,
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
    events: &mut Vec<EngineEvent>,
) {
    for (index, temp_obstacle) in temp_obstacles.iter_mut().enumerate() {
        resist_magick(&temp_obstacle.magick.power, &mut temp_obstacle.effect.power);
        add_damage(
            Index::TempObstacle(index),
            duration,
            settings.magical_damage_factor,
            temp_obstacle.mass(),
            &temp_obstacle.effect.power,
            events,
        );
    }
}

fn update_actor_current_direction(
    now: f64,
    duration: f64,
    max_rotation_speed: f64,
    actor: &mut Actor,
) {
    if is_actor_immobilized(actor) {
        return;
    }
    let target_direction = if is_actor_in_panic(actor) {
        actor
            .current_direction
            .rotated((0.075_f64).copysign((0.25 * now).sin()))
    } else {
        actor.target_direction
    };
    actor.current_direction = get_current_direction(
        actor.current_direction,
        target_direction,
        duration,
        max_rotation_speed,
    );
}

fn update_actor_dynamic_force(duration: f64, move_force: f64, max_speed: f64, actor: &mut Actor) {
    if is_actor_immobilized(actor) || is_actor_flying(actor) {
        return;
    }
    let speed = actor.velocity.norm();
    if (actor.moving || is_actor_in_panic(actor))
        && actor.delayed_magick.is_none()
        && matches!(actor.occupation, ActorOccupation::None)
    {
        if speed < max_speed {
            actor.dynamic_force +=
                actor.current_direction * (move_force - speed * move_force / max_speed);
        }
    } else if speed > f64::EPSILON {
        let force_factor = duration / (2.0 * actor.mass());
        let base_stop_factor = 0.25 * (move_force - speed * move_force / max_speed) / speed;
        let stop_factor = if speed > base_stop_factor * force_factor {
            base_stop_factor
        } else {
            0.25 / force_factor
        };
        actor.dynamic_force -= actor.velocity * stop_factor;
    }
}

fn add_sliding_resistance(
    mass: f64,
    velocity: Vec2f,
    surface: MaterialType,
    gravitational_acceleration: f64,
    dynamic_force: &mut Vec2f,
) {
    let speed = velocity.norm();
    if speed != 0.0 {
        *dynamic_force -=
            velocity * (mass * surface.sliding_resistance() * gravitational_acceleration / speed);
    }
}

fn add_walking_resistance(
    mass: f64,
    velocity: Vec2f,
    surface: MaterialType,
    gravitational_acceleration: f64,
    dynamic_force: &mut Vec2f,
) {
    let speed = velocity.norm();
    if speed != 0.0 {
        *dynamic_force -= velocity
            * (mass * surface.walking_resistance() * gravitational_acceleration * speed.max(1.0)
                / speed);
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

fn add_damage(
    target: Index,
    duration: f64,
    damage_factor: f64,
    mass: f64,
    power: &[f64; 11],
    events: &mut Vec<EngineEvent>,
) {
    let damage = get_damage(power) * damage_factor * duration / mass;
    if damage != 0.0 {
        events.push(EngineEvent::Damage { target, damage })
    }
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
                    magick,
                    &world.actors[i].aura.elements,
                );
                can_reflect_beams(&world.actors[i].aura.elements)
            }
            Index::Projectile(..) => false,
            Index::StaticObject(i) => {
                world.static_objects[i].effect = add_magick_to_effect(
                    world.time,
                    &world.static_objects[i].effect,
                    magick,
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
            StaticShape::CircleArc(v) => Isometry::new(
                Vector2::new(self.position.x, self.position.y),
                v.rotation + self.rotation,
            ),
            StaticShape::Disk(..) => Isometry::translation(self.position.x, self.position.y),
            StaticShape::Rectangle(..) => Isometry::new(
                Vector2::new(self.position.x, self.position.y),
                self.rotation,
            ),
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

impl WithIsometry for TempArea {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
    }
}

impl WithIsometry for StaticArea {
    fn get_isometry(&self) -> Isometry<Real> {
        Isometry::translation(self.position.x, self.position.y)
    }
}

impl<'a, T: Default + PartialEq> WithIsometry for IntersectingDynamicObject<'a, T> {
    fn get_isometry(&self) -> Isometry<Real> {
        self.isometry
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

impl WithShape for TempArea {
    fn with_shape(&self, _: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        (*f)(&self.body.shape.as_shape())
    }
}

impl WithShape for StaticArea {
    fn with_shape(&self, shape_cache: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        self.body
            .shape
            .clone()
            .with_shape(shape_cache, |shape| (*f)(shape))
    }
}

impl<'a, T: Default + PartialEq> WithShape for IntersectingDynamicObject<'a, T> {
    fn with_shape(&self, _: &ShapeCache, f: &mut dyn FnMut(&dyn Shape)) {
        (*f)(self.shape)
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

fn move_objects(
    duration: f64,
    world: &mut World,
    shape_cache: &ShapeCache,
    events: &mut Vec<EngineEvent>,
) {
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
            let (lhs_damage, rhs_damage) =
                collide_objects(collision.lhs, collision.rhs, &apply_impact, world);
            if lhs_damage != 0.0 {
                events.push(EngineEvent::Damage {
                    target: collision.lhs,
                    damage: lhs_damage,
                });
            }
            if rhs_damage != 0.0 {
                events.push(EngineEvent::Damage {
                    target: collision.rhs,
                    damage: rhs_damage,
                });
            }
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
    fn material_type(&self) -> MaterialType;
    fn mass(&self) -> f64;
    fn position(&self) -> Vec2f;
    fn set_position(&mut self, value: Vec2f);
    fn set_velocity(&mut self, value: Vec2f);
    fn magick(&self) -> &Magick;
    fn resistance(&self) -> &[T; 11];
    fn effect(&self) -> &Effect;
    fn set_effect(&mut self, value: Effect);
    fn aura(&self) -> &Aura;
    fn is_static(&self) -> bool;
}

fn collide_objects(
    lhs: Index,
    rhs: Index,
    apply_impact: &ApplyImpact,
    world: &mut World,
) -> (f64, f64) {
    if lhs > rhs {
        let (rhs_damage, lhs_damage) = collide_ordered_objects(rhs, lhs, apply_impact, world);
        (lhs_damage, rhs_damage)
    } else {
        collide_ordered_objects(lhs, rhs, apply_impact, world)
    }
}

fn collide_ordered_objects(
    lhs: Index,
    rhs: Index,
    f: &ApplyImpact,
    world: &mut World,
) -> (f64, f64) {
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
    fn call<L, R>(
        &self,
        lhs: &mut dyn CollidingObject<L>,
        rhs: &mut dyn CollidingObject<R>,
    ) -> (f64, f64)
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
        )
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
) -> (f64, f64)
where
    L: Default + PartialEq,
    R: Default + PartialEq,
{
    let lhs_kinetic_energy = get_kinetic_energy(lhs.mass(), lhs.velocity());
    let rhs_kinetic_energy = get_kinetic_energy(rhs.mass(), rhs.velocity());
    let lhs_material = lhs.material_type();
    let rhs_material = rhs.material_type();
    let (lhs_velocity, rhs_velocity) = get_velocity_after_impact(
        toi,
        (lhs_material.restitution() + rhs_material.restitution()) * 0.5,
        &make_moving_object(lhs),
        &make_moving_object(rhs),
    );
    lhs.set_position(lhs.position() + lhs.velocity() * toi.toi + lhs_velocity * epsilon_duration);
    rhs.set_position(rhs.position() + rhs.velocity() * toi.toi + rhs_velocity * epsilon_duration);
    lhs.set_velocity(lhs_velocity);
    rhs.set_velocity(rhs_velocity);
    if let Some(contact) = get_contact(shape_cache, lhs, rhs) {
        if lhs.is_static() {
            rhs.set_position(
                rhs.position()
                    + Vec2f::from(&(rhs.get_isometry() * contact.normal2).xy())
                        * contact.dist.min(-epsilon_duration),
            );
        } else if rhs.is_static() {
            lhs.set_position(
                lhs.position()
                    + Vec2f::from(&(lhs.get_isometry() * contact.normal1).xy())
                        * contact.dist.min(-epsilon_duration),
            );
        } else {
            let half_distance = contact.dist.min(-epsilon_duration) / 2.0;
            let mass_sum = lhs.mass() + rhs.mass();
            lhs.set_position(
                lhs.position()
                    + Vec2f::from(&(lhs.get_isometry() * contact.normal1).xy())
                        * (half_distance * rhs.mass() / mass_sum),
            );
            rhs.set_position(
                rhs.position()
                    + Vec2f::from(&(rhs.get_isometry() * contact.normal2).xy())
                        * (half_distance * lhs.mass() / mass_sum),
            );
        }
    }
    let new_lhs_effect = add_magick_to_effect(now, lhs.effect(), rhs.magick(), lhs.resistance());
    let new_rhs_effect = add_magick_to_effect(now, rhs.effect(), lhs.magick(), rhs.resistance());
    lhs.set_effect(new_lhs_effect);
    rhs.set_effect(new_rhs_effect);
    let damage_energy = ((lhs_kinetic_energy + rhs_kinetic_energy)
        - (get_kinetic_energy(lhs.mass(), lhs_velocity)
            + get_kinetic_energy(rhs.mass(), rhs_velocity)))
    .max(0.0);
    let sum_density = lhs_material.density() + rhs_material.density();
    (
        get_collision_damage(
            damage_energy * rhs_material.density() / sum_density,
            damage_factor,
            lhs,
        ),
        get_collision_damage(
            damage_energy * lhs_material.density() / sum_density,
            damage_factor,
            rhs,
        ),
    )
}

struct MovingObject {
    isometry: Isometry<Real>,
    velocity: Vec2f,
    mass: f64,
}

fn make_moving_object<T>(colliding_object: &dyn CollidingObject<T>) -> MovingObject
where
    T: Default + PartialEq,
{
    MovingObject {
        isometry: colliding_object.get_isometry(),
        velocity: colliding_object.velocity(),
        mass: colliding_object.mass(),
    }
}

fn get_velocity_after_impact(
    toi: &TOI,
    restitution: f64,
    lhs: &MovingObject,
    rhs: &MovingObject,
) -> (Vec2f, Vec2f) {
    let lhs_normal = Vec2f::from(&(lhs.isometry * toi.normal1).xy());
    let rhs_normal = Vec2f::from(&(rhs.isometry * toi.normal2).xy());
    let normal = (rhs_normal - lhs_normal).normalized();
    let lhs_velocity_components = get_velocity_components(lhs.velocity, normal);
    let rhs_velocity_components = get_velocity_components(rhs.velocity, normal);
    let delta_normal_velocity = lhs_velocity_components.normal - rhs_velocity_components.normal;
    let mass_sum = lhs.mass + rhs.mass;
    let lhs_axis_velocity = lhs_velocity_components.normal
        - delta_normal_velocity * (rhs.mass * (1.0 + restitution) / mass_sum);
    let rhs_axis_velocity = rhs_velocity_components.normal
        + delta_normal_velocity * (lhs.mass * (1.0 + restitution) / mass_sum);
    (
        normal * lhs_axis_velocity + lhs_velocity_components.tangent,
        normal * rhs_axis_velocity + rhs_velocity_components.tangent,
    )
}

#[derive(Debug)]
struct VelocityComponents {
    normal: f64,
    tangent: Vec2f,
}

fn get_velocity_components(velocity: Vec2f, normal: Vec2f) -> VelocityComponents {
    let normal_velocity = velocity.dot(normal);
    VelocityComponents {
        normal: normal_velocity,
        tangent: velocity - normal * normal_velocity,
    }
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
    fn material_type(&self) -> MaterialType {
        self.body.material_type
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

    fn aura(&self) -> &Aura {
        &self.aura
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject<f64> for Projectile {
    fn material_type(&self) -> MaterialType {
        self.body.material_type
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

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        false
    }
}

impl CollidingObject<bool> for StaticObject {
    fn material_type(&self) -> MaterialType {
        self.body.material_type
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

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

impl CollidingObject<bool> for Shield {
    fn material_type(&self) -> MaterialType {
        self.body.material_type
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

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

impl CollidingObject<f64> for TempObstacle {
    fn material_type(&self) -> MaterialType {
        self.body.material_type
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

    fn aura(&self) -> &Aura {
        &DEFAULT_AURA
    }

    fn is_static(&self) -> bool {
        true
    }
}

fn get_collision_damage<T>(
    damage_energy: f64,
    damage_factor: f64,
    object: &mut dyn CollidingObject<T>,
) -> f64
where
    T: Default + PartialEq,
{
    if can_absorb_physical_damage(&object.aura().elements) {
        return 0.0;
    }
    (damage_energy * damage_factor) / object.mass()
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
                let mut power = delayed_magick.power;
                power[Element::Earth as usize] = 0.0;
                world.projectiles.push(Projectile {
                    id: ProjectileId(get_next_id(&mut world.id_counter)),
                    body: Body {
                        shape: Disk { radius },
                        material_type: MaterialType::Stone,
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

fn intersection_test<L, R>(lhs: &L, rhs: &R, shape_cache: &ShapeCache) -> bool
where
    L: WithIsometry + WithShape,
    R: WithIsometry + WithShape,
{
    let mut result = false;
    lhs.with_shape(shape_cache, &mut |lhs_shape| {
        rhs.with_shape(shape_cache, &mut |rhs_shape| {
            result = query::intersection_test(
                &lhs.get_isometry(),
                lhs_shape,
                &rhs.get_isometry(),
                rhs_shape,
            )
            .unwrap();
        });
    });
    result
}

fn intersection_test_with_ring_sector(
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
                    material_type: MaterialType::Ice,
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

fn is_actor_in_panic(actor: &Actor) -> bool {
    actor.effect.power[Element::Fire as usize] > 0.0
}

fn is_actor_flying(actor: &Actor) -> bool {
    (actor.position_z - actor.body.shape.radius) > f64::EPSILON
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

fn get_number_of_shield_objects(angle: f64) -> i32 {
    (angle / (std::f64::consts::FRAC_PI_2 / 5.0)).round() as i32
}

enum EngineEvent {
    Damage { target: Index, damage: f64 },
}

fn handle_events(events: &[EngineEvent], world: &mut World) {
    for event in events {
        match event {
            EngineEvent::Damage { target, damage } => match target {
                Index::Actor(i) => {
                    world.actors[*i].delayed_magick = None;
                    complete_directed_magick(*i, world);
                    damage_health(*damage, &mut world.actors[*i].health)
                }
                Index::Projectile(i) => damage_health(*damage, &mut world.projectiles[*i].health),
                Index::StaticObject(i) => {
                    damage_health(*damage, &mut world.static_objects[*i].health)
                }
                Index::Shield(..) => (),
                Index::TempObstacle(i) => {
                    damage_health(*damage, &mut world.temp_obstacles[*i].health)
                }
            },
        }
    }
}

fn damage_health(damage: f64, health: &mut f64) {
    *health = (*health - damage).clamp(0.0, 1.0);
}

#[cfg(test)]
mod tests {
    use nalgebra::distance;
    use parry2d_f64::na::Unit;
    use parry2d_f64::query::TOIStatus;

    use crate::engine::*;
    use crate::world::StaticObjectId;

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
                polyline.vertices().first().unwrap(),
                &Point2::new(SQRT_2, -SQRT_2),
            ) <= f64::EPSILON
        );
        assert!(
            distance(
                polyline.vertices().last().unwrap(),
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
                material_type: MaterialType::None,
            },
            position: Vec2f::new(-33.23270204831895, -32.3454131103618),
            created: 0.0,
            power: 0.0,
        };
        let projectile = Projectile {
            id: ProjectileId(2),
            body: Body {
                shape: Disk { radius: 0.2 },
                material_type: MaterialType::Stone,
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

    #[test]
    fn apply_impact_for_standing_actor_and_moving_projectile() {
        let duration = 1.0 / 60.0;
        let shape_cache = ShapeCache::default();
        let mut actor = Actor {
            id: ActorId(1),
            player_id: PlayerId(2),
            active: true,
            name: String::new(),
            body: Body {
                shape: Disk { radius: 1.0 },
                material_type: MaterialType::Flesh,
            },
            position: Vec2f::new(0.0, 0.0),
            health: 1.0,
            effect: Effect::default(),
            aura: Aura::default(),
            velocity: Vec2f::ZERO,
            dynamic_force: Vec2f::ZERO,
            current_direction: Vec2f::only_x(1.0),
            target_direction: Vec2f::only_x(1.0),
            spell_elements: Vec::new(),
            moving: false,
            delayed_magick: None,
            position_z: 1.0,
            velocity_z: 0.0,
            occupation: ActorOccupation::None,
        };
        let mut projectile = Projectile {
            id: Default::default(),
            body: Body {
                shape: Disk { radius: 0.1 },
                material_type: MaterialType::Stone,
            },
            position: Vec2f::only_x(2.0),
            health: 1.0,
            magick: Magick::default(),
            velocity: Vec2f::only_x(-500.0),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.0,
            velocity_z: 0.0,
        };
        let toi = time_of_impact(duration, &shape_cache, &actor, &projectile);
        assert!(toi.is_some());
        let damage = ApplyImpact {
            now: 42.0,
            damage_factor: 1e-3,
            epsilon_duration: 1e-3,
            shape_cache: &shape_cache,
            toi: &toi.unwrap(),
        }
        .call(&mut actor, &mut projectile);
        assert_eq!(damage, (0.326533173268825, 27.633881770849325));
        assert_eq!(
            actor,
            Actor {
                id: ActorId(1),
                player_id: PlayerId(2),
                active: true,
                name: String::new(),
                body: Body {
                    shape: Disk { radius: 1.0 },
                    material_type: MaterialType::Flesh,
                },
                position: Vec2f::only_x(-0.001926969791342261),
                health: 1.0,
                effect: Effect::default(),
                aura: Aura::default(),
                velocity: Vec2f::only_x(-1.926969791342261),
                dynamic_force: Vec2f::ZERO,
                current_direction: Vec2f::only_x(1.0),
                target_direction: Vec2f::only_x(1.0),
                spell_elements: Vec::new(),
                moving: false,
                delayed_magick: None,
                position_z: 1.0,
                velocity_z: 0.0,
                occupation: ActorOccupation::None,
            }
        );
        assert_eq!(
            projectile,
            Projectile {
                id: Default::default(),
                body: Body {
                    shape: Disk { radius: 0.1 },
                    material_type: MaterialType::Stone,
                },
                position: Vec2f::only_x(1.1605730302086577),
                health: 1.0,
                magick: Magick::default(),
                velocity: Vec2f::only_x(60.573030208657656),
                dynamic_force: Vec2f::ZERO,
                position_z: 1.0,
                velocity_z: 0.0,
            }
        );
    }

    #[test]
    fn apply_impact_for_moving_projectile_and_rectangle_static_object() {
        let duration = 1.0;
        let shape_cache = ShapeCache::default();
        let mut projectile = Projectile {
            id: Default::default(),
            body: Body {
                shape: Disk { radius: 1.0 },
                material_type: MaterialType::Stone,
            },
            position: Vec2f::new(5.0, 5.0),
            health: 1.0,
            magick: Magick::default(),
            velocity: Vec2f::new(-50.0, -50.0),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.0,
            velocity_z: 0.0,
        };
        let mut static_object = StaticObject {
            id: StaticObjectId(1),
            body: Body {
                shape: StaticShape::Rectangle(Rectangle {
                    width: 20.0,
                    height: 1.0,
                }),
                material_type: MaterialType::Stone,
            },
            position: Vec2f::new(0.0, 0.0),
            rotation: std::f64::consts::FRAC_PI_6,
            health: 1.0,
            effect: Effect::default(),
        };
        let toi = time_of_impact(duration, &shape_cache, &projectile, &static_object);
        assert!(toi.is_some());
        let damage = ApplyImpact {
            now: 42.0,
            damage_factor: 1e-3,
            epsilon_duration: 1e-3,
            shape_cache: &shape_cache,
            toi: &toi.unwrap(),
        }
        .call(&mut projectile, &mut static_object);
        assert_eq!(damage, (0.06947210324960644, 0.010912652459919757));
        assert_eq!(
            projectile,
            Projectile {
                id: Default::default(),
                body: Body {
                    shape: Disk { radius: 1.0 },
                    material_type: MaterialType::Stone,
                },
                position: Vec2f::new(4.0385862888455994, 4.064513631098341),
                health: 1.0,
                magick: Magick::default(),
                velocity: Vec2f::new(-59.49006596389085, -33.562723711148514),
                dynamic_force: Vec2f::ZERO,
                position_z: 1.0,
                velocity_z: 0.0,
            }
        );
        assert_eq!(
            static_object,
            StaticObject {
                id: StaticObjectId(1),
                body: Body {
                    shape: StaticShape::Rectangle(Rectangle {
                        width: 20.0,
                        height: 1.0,
                    }),
                    material_type: MaterialType::Stone,
                },
                position: Vec2f::new(0.0, 0.0),
                rotation: std::f64::consts::FRAC_PI_6,
                health: 1.0,
                effect: Effect::default(),
            }
        );
    }

    #[test]
    fn get_velocity_after_impact_for_projectiles() {
        let duration = 1.0 / 60.0;
        let shape_cache = ShapeCache::default();
        let projectile1 = Projectile {
            id: Default::default(),
            body: Body {
                shape: Disk { radius: 1.0 },
                material_type: MaterialType::Stone,
            },
            position: Vec2f::new(4.0, 4.0),
            health: 1.0,
            magick: Magick::default(),
            velocity: Vec2f::new(-500.0, -400.0),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.0,
            velocity_z: 0.0,
        };
        let projectile2 = Projectile {
            id: Default::default(),
            body: Body {
                shape: Disk { radius: 2.0 },
                material_type: MaterialType::Stone,
            },
            position: Vec2f::new(-4.0, -4.0),
            health: 1.0,
            magick: Magick::default(),
            velocity: Vec2f::new(300.0, 200.0),
            dynamic_force: Vec2f::ZERO,
            position_z: 1.0,
            velocity_z: 0.0,
        };
        let toi = time_of_impact(duration, &shape_cache, &projectile1, &projectile2);
        assert!(toi.is_some());
        let restitutions_and_results = &[
            (
                0.0,
                (
                    Vec2f::new(-273.68390731385875, 192.6896541962566),
                    Vec2f::new(224.56130243795292, 2.4367819345810915),
                ),
            ),
            (
                0.5,
                (
                    Vec2f::new(-160.5258609707881, 489.03448129438505),
                    Vec2f::new(186.84195365692938, -96.34482709812832),
                ),
            ),
            (
                1.0,
                (
                    Vec2f::new(-47.367814627717536, 785.3793083925132),
                    Vec2f::new(149.12260487590584, -195.1264361308378),
                ),
            ),
        ];
        for (restitution, results) in restitutions_and_results {
            let object1 = MovingObject {
                isometry: Isometry::identity(),
                velocity: projectile1.velocity,
                mass: 1.0,
            };
            let object2 = MovingObject {
                isometry: Isometry::identity(),
                velocity: projectile2.velocity,
                mass: 3.0,
            };
            assert_eq!(
                get_velocity_after_impact(toi.as_ref().unwrap(), *restitution, &object1, &object2),
                *results,
                "restitution: {:?}",
                restitution
            )
        }
    }
}
