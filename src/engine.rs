use parry2d_f64::math::{Isometry, Real};
use parry2d_f64::na::{Point2, Vector2};
use parry2d_f64::query::{Ray, RayCast, RayIntersection, time_of_impact, TOIStatus};
use parry2d_f64::shape::Ball;

use crate::rect::Rectf;
use crate::vec2::{Square, Vec2f};
use crate::world::{Actor, Aura, Beam, BeamObject, Body, DelayedMagick, DynamicObject, Effect,
                   Element, Magick, Material, StaticObject, World, WorldSettings};

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
        for i in 0..world.beam_objects.len() {
            let beam_object = &world.beam_objects[i];
            let actor = world.actors.iter()
                .find(|v| v.id == beam_object.beam.actor_id)
                .unwrap();
            let direction = actor.current_direction;
            let origin = actor.position + direction * (actor.body.radius + world.settings.margin);
            let magick = beam_object.beam.magick.clone();
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
        update_actors(world.time, duration, &world.settings, &mut world.actors);
        update_dynamic_objects(world.time, duration, &world.settings, &mut world.dynamic_objects);
        update_static_objects(world.time, duration, &world.settings, &mut world.static_objects);
        self.beam_collider.update(world);
        collide_objects(duration, world);
        world.actors.iter_mut().for_each(|v| v.dynamic_force = Vec2f::ZERO);
        world.dynamic_objects.iter_mut().for_each(|v| v.dynamic_force = Vec2f::ZERO);
        let bounds = world.bounds.clone();
        world.actors.retain(|v| is_active(&bounds, &v.body, v.position, v.health));
        world.dynamic_objects.retain(|v| is_active(&bounds, &v.body, v.position, v.health));
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
        return;
    } else if magick.power[Element::Earth as usize] > 0.0 {
        add_delayed_magick(magick, actor_index, world);
    } else if magick.power[Element::Ice as usize] > 0.0 {
        return;
    } else if magick.power[Element::Arcane as usize] > 0.0
        || magick.power[Element::Life as usize] > 0.0 {
        add_beam_object(magick, actor_index, world);
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

fn add_delayed_magick(magick: Magick, actor_index: usize, world: &mut World) {
    world.actors[actor_index].delayed_magick = Some(DelayedMagick {
        actor_id: world.actors[actor_index].id,
        started: world.time,
        completed: false,
        power: magick.power,
    });
}

fn add_beam_object(magick: Magick, actor_index: usize, world: &mut World) {
    world.beam_objects.push(BeamObject {
        id: get_next_id(&mut world.id_counter),
        beam: Beam { actor_id: world.actors[actor_index].id, magick },
    });
}

pub fn complete_directed_magick(actor_index: usize, world: &mut World) {
    let actor_id = world.actors[actor_index].id;
    if remove_count(&mut world.beam_objects, |v| v.beam.actor_id == actor_id) > 0 {
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

pub fn get_body_mass(body: &Body) -> f64 {
    get_mass(get_body_volume(body), body.material)
}

pub fn get_mass(volume: f64, material: Material) -> f64 {
    volume * get_material_density(material)
}

pub fn get_body_volume(body: &Body) -> f64 {
    get_circle_volume(body.radius)
}

pub fn get_circle_volume(radius: f64) -> f64 {
    radius * radius * radius * std::f64::consts::PI
}

pub fn get_material_density(material: Material) -> f64 {
    match material {
        Material::Flesh => 800.0,
        Material::Stone => 2750.0,
    }
}

pub fn get_material_restitution(material: Material) -> f64 {
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

fn update_actors(now: f64, duration: f64, settings: &WorldSettings, actors: &mut Vec<Actor>) {
    for actor in actors.iter_mut() {
        update_actor_current_direction(duration, settings.max_rotation_speed, actor);
        update_actor_dynamic_force(settings.move_force, actor);
        decay_effect(now, duration, settings.decay_factor, &mut actor.effect);
        decay_aura(now, duration, settings.decay_factor, &mut actor.aura);
        damage_health(duration, settings.damage_factor, &actor.body, &actor.effect, &mut actor.health);
        update_position(duration, actor.velocity, &mut actor.position);
        update_velocity(duration, &actor.body, actor.dynamic_force, &mut actor.velocity);
    }
}

fn update_dynamic_objects(now: f64, duration: f64, settings: &WorldSettings, dynamic_objects: &mut Vec<DynamicObject>) {
    for object in dynamic_objects.iter_mut() {
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
        damage_health(duration, settings.damage_factor, &object.body, &object.effect, &mut object.health);
        update_position(duration, object.velocity, &mut object.position);
        update_velocity(duration, &object.body, object.dynamic_force, &mut object.velocity);
    }
}

fn update_static_objects(now: f64, duration: f64, settings: &WorldSettings, static_objects: &mut Vec<StaticObject>) {
    for object in static_objects.iter_mut() {
        decay_effect(now, duration, settings.decay_factor, &mut object.effect);
        decay_aura(now, duration, settings.decay_factor, &mut object.aura);
        damage_health(duration, settings.damage_factor, &object.body, &object.effect, &mut object.health);
    }
}

fn update_actor_current_direction(duration: f64, max_rotation_speed: f64, actor: &mut Actor) {
    actor.current_direction = get_current_direction(actor.current_direction, actor.target_direction, duration, max_rotation_speed);
}

fn update_actor_dynamic_force(move_force: f64, actor: &mut Actor) {
    actor.dynamic_force += actor.current_direction * move_force * actor.moving as i32 as f64;
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
    *health -= get_damage(&effect.power) * damage_factor * duration / get_body_mass(body);
}

fn update_position(duration: f64, velocity: Vec2f, position: &mut Vec2f) {
    *position += velocity * duration;
}

fn update_velocity(duration: f64, body: &Body, dynamic_force: Vec2f, velocity: &mut Vec2f) {
    *velocity += dynamic_force * (duration / get_body_mass(body));
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

fn get_current_direction(current_direction: Vec2f, target_direction: Vec2f, duration: f64, max_rotation_speed: f64) -> Vec2f {
    let diff = normalize_angle(target_direction.angle() - current_direction.angle());
    current_direction.rotated(diff.signum() * diff.abs().min(max_rotation_speed * duration))
}

fn normalize_angle(angle: f64) -> f64 {
    let turns = angle / (2.0 * std::f64::consts::PI) + 0.5;
    return (turns - turns.floor() - 0.5) * (2.0 * std::f64::consts::PI);
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
    for static_object in world.static_objects.iter_mut() {
        for actor in world.actors.iter_mut() {
            collide_dynamic_and_static_objects(
                world.time, duration, world.settings.damage_factor,
                DynamicCollidingObject::from(actor),
                StaticCollidingObject::from(&mut *static_object),
            );
        }
        for dynamic_object in world.dynamic_objects.iter_mut() {
            collide_dynamic_and_static_objects(
                world.time, duration, world.settings.damage_factor,
                DynamicCollidingObject::from(dynamic_object),
                StaticCollidingObject::from(&mut *static_object),
            );
        }
    }
    if !world.actors.is_empty() {
        for i in 0..world.actors.len() - 1 {
            for j in i + 1..world.actors.len() {
                let (left, right) = world.actors.split_at_mut(j);
                collide_dynamic_objects(
                    world.time, duration, world.settings.damage_factor,
                    DynamicCollidingObject::from(&mut left[i]),
                    DynamicCollidingObject::from(&mut right[0]),
                );
            }
        }
    }
    for dynamic_object in world.dynamic_objects.iter_mut() {
        for actor in world.actors.iter_mut() {
            collide_dynamic_objects(
                world.time, duration, world.settings.damage_factor,
                DynamicCollidingObject::from(&mut *dynamic_object),
                DynamicCollidingObject::from(actor),
            );
        }
    }
    if !world.dynamic_objects.is_empty() {
        for i in 0..world.dynamic_objects.len() - 1 {
            for j in i + 1..world.dynamic_objects.len() {
                let (left, right) = world.dynamic_objects.split_at_mut(j);
                collide_dynamic_objects(
                    world.time, duration, world.settings.damage_factor,
                    DynamicCollidingObject::from(&mut left[i]),
                    DynamicCollidingObject::from(&mut right[0]),
                );
            }
        }
    }
}

struct DynamicCollidingObject<'a> {
    body: &'a Body,
    position: &'a mut Vec2f,
    velocity: &'a mut Vec2f,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

impl<'a> From<&'a mut Actor> for DynamicCollidingObject<'a> {
    fn from(value: &'a mut Actor) -> Self {
        DynamicCollidingObject {
            body: &value.body,
            position: &mut value.position,
            velocity: &mut value.velocity,
            effect: &mut value.effect,
            health: &mut value.health,
            aura: &value.aura,
        }
    }
}

impl<'a> From<&'a mut DynamicObject> for DynamicCollidingObject<'a> {
    fn from(value: &'a mut DynamicObject) -> Self {
        DynamicCollidingObject {
            body: &value.body,
            position: &mut value.position,
            velocity: &mut value.velocity,
            effect: &mut value.effect,
            health: &mut value.health,
            aura: &value.aura,
        }
    }
}

fn collide_dynamic_objects(now: f64, duration: f64, damage_factor: f64, lhs: DynamicCollidingObject, rhs: DynamicCollidingObject) {
    let collision = time_of_impact(
        &Isometry::translation(lhs.position.x, lhs.position.y),
        &Vector2::new(lhs.velocity.x, lhs.velocity.y),
        &Ball::new(lhs.body.radius),
        &Isometry::translation(rhs.position.x, rhs.position.y),
        &Vector2::new(rhs.velocity.x, rhs.velocity.y),
        &Ball::new(rhs.body.radius),
        duration,
        0.0,
    ).unwrap();
    if let Some(collision) = collision {
        let lhs_mass = get_body_mass(&lhs.body);
        let rhs_mass = get_body_mass(&rhs.body);
        let lhs_kinetic_energy = get_kinetic_energy(lhs_mass, *lhs.velocity);
        let rhs_kinetic_energy = get_kinetic_energy(rhs_mass, *rhs.velocity);
        let delta_velocity = *lhs.velocity - *rhs.velocity;
        let mass_sum = lhs_mass + rhs_mass;
        if matches!(collision.status, TOIStatus::Penetrating) {
            let delta_position = *rhs.position - *lhs.position;
            let distance = delta_position.norm();
            let penetration = lhs.body.radius + rhs.body.radius - distance;
            let normal = delta_position.normalized();
            *lhs.position = *lhs.position - normal * (penetration * rhs_mass / mass_sum);
            *rhs.position = *rhs.position + normal * (penetration * lhs_mass / mass_sum);
        } else {
            *lhs.position = *lhs.position + *lhs.velocity * collision.toi;
            *rhs.position = *rhs.position + *rhs.velocity * collision.toi;
        }
        let lhs_velocity = *lhs.velocity - delta_velocity * rhs_mass * (1.0 + get_material_restitution(lhs.body.material)) / mass_sum;
        let rhs_velocity = *rhs.velocity + delta_velocity * lhs_mass * (1.0 + get_material_restitution(rhs.body.material)) / mass_sum;
        *lhs.velocity = lhs_velocity;
        *rhs.velocity = rhs_velocity;
        let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
        let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
        *lhs.effect = new_lhs_effect;
        *rhs.effect = new_rhs_effect;
        if !can_absorb_physical_damage(&lhs.aura.elements) {
            *lhs.health -= (get_kinetic_energy(lhs_mass, lhs_velocity) - lhs_kinetic_energy).abs() * damage_factor / lhs_mass;
        }
        if !can_absorb_physical_damage(&rhs.aura.elements) {
            *rhs.health -= (get_kinetic_energy(rhs_mass, rhs_velocity) - rhs_kinetic_energy).abs() * damage_factor / rhs_mass;
        }
    }
}

struct StaticCollidingObject<'a> {
    body: &'a Body,
    position: &'a Vec2f,
    effect: &'a mut Effect,
    health: &'a mut f64,
    aura: &'a Aura,
}

impl<'a> From<&'a mut StaticObject> for StaticCollidingObject<'a> {
    fn from(value: &'a mut StaticObject) -> Self {
        StaticCollidingObject {
            body: &value.body,
            position: &mut value.position,
            effect: &mut value.effect,
            health: &mut value.health,
            aura: &value.aura,
        }
    }
}

fn collide_dynamic_and_static_objects(now: f64, duration: f64, damage_factor: f64, lhs: DynamicCollidingObject, rhs: StaticCollidingObject) {
    let collision = time_of_impact(
        &Isometry::translation(lhs.position.x, lhs.position.y),
        &Vector2::new(lhs.velocity.x, lhs.velocity.y),
        &Ball::new(lhs.body.radius),
        &Isometry::translation(rhs.position.x, rhs.position.y),
        &Vector2::new(0.0, 0.0),
        &Ball::new(rhs.body.radius),
        duration,
        0.0,
    ).unwrap();
    if let Some(collision) = collision {
        let lhs_mass = get_body_mass(&lhs.body);
        let rhs_mass = get_body_mass(&rhs.body);
        let mass_sum = lhs_mass + rhs_mass;
        if matches!(collision.status, TOIStatus::Penetrating) {
            let delta_position = *rhs.position - *lhs.position;
            let distance = delta_position.norm();
            let penetration = lhs.body.radius + rhs.body.radius - distance;
            let mass_sum = lhs_mass + rhs_mass;
            let normal = delta_position.normalized();
            *lhs.position = *lhs.position - normal * (penetration * rhs_mass / mass_sum);
        } else {
            *lhs.position = *lhs.position + *lhs.velocity * collision.toi;
        }
        let delta_velocity = *lhs.velocity;
        let lhs_kinetic_energy = get_kinetic_energy(lhs_mass, *lhs.velocity);
        let lhs_velocity = *lhs.velocity - delta_velocity * rhs_mass * (1.0 + get_material_restitution(lhs.body.material)) / mass_sum;
        *lhs.velocity = lhs_velocity;
        let new_lhs_effect = add_magick_power_to_effect(now, lhs.effect, &rhs.effect.power);
        let new_rhs_effect = add_magick_power_to_effect(now, rhs.effect, &lhs.effect.power);
        *lhs.effect = new_lhs_effect;
        *rhs.effect = new_rhs_effect;
        if !can_absorb_physical_damage(&lhs.aura.elements) {
            *lhs.health -= (get_kinetic_energy(lhs_mass, lhs_velocity) - lhs_kinetic_energy).abs() * damage_factor / lhs_mass;
        }
        if !can_absorb_physical_damage(&rhs.aura.elements) {
            let rhs_velocity = delta_velocity * lhs_mass * (1.0 + get_material_restitution(rhs.body.material)) / mass_sum;
            *rhs.health -= get_kinetic_energy(rhs_mass, rhs_velocity) * damage_factor / rhs_mass;
        }
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
