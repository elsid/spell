use rand::Rng;

use crate::engine::{get_body_mass, get_circle_volume, get_material_restitution, get_next_id};
use crate::rect::Rectf;
use crate::vec2::Vec2f;
use crate::world::{
    Actor,
    Aura,
    Body,
    DynamicObject,
    Effect,
    Material,
    StaticObject,
    World,
    WorldSettings,
};

pub fn generate_world<R: Rng>(bounds: Rectf, rng: &mut R) -> World {
    let settings = WorldSettings::default();
    let mut id_counter = 1;
    let mut actors = Vec::new();
    generate_actors(&Material::Flesh, 10, &bounds, &settings, &mut id_counter, &mut actors, rng);
    let mut dynamic_objects = Vec::new();
    let mut static_objects = Vec::new();
    for material in &[Material::Flesh, Material::Stone] {
        generate_dynamic_objects(material, 10, &bounds, &settings, &mut id_counter, &mut dynamic_objects, rng);
        generate_static_objects(material, 10, &bounds, &settings, &mut id_counter, &mut static_objects, rng);
    }
    World {
        revision: 0,
        settings,
        bounds,
        time: 0.0,
        id_counter,
        actors,
        dynamic_objects,
        static_objects,
        beam_objects: Vec::new(),
    }
}

pub fn generate_player_actor<R: Rng>(id: u64, bounds: &Rectf, settings: &WorldSettings, rng: &mut R) -> Actor {
    let material = Material::Flesh;
    let mass = get_body_mass(get_circle_volume(1.0), material);
    let delta = bounds.max - bounds.min;
    let middle = (bounds.max + bounds.min) / 2.0;
    Actor {
        id,
        body: Body {
            mass,
            radius: 1.0,
            restitution: get_material_restitution(material),
            material,
        },
        position: Vec2f::new(
            rng.gen_range(middle.x - delta.x * 0.25..middle.x + delta.x * 0.25),
            rng.gen_range(middle.y - delta.y * 0.25..middle.y + delta.y * 0.25),
        ),
        health: mass * settings.health_factor,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
        current_direction: Vec2f::I,
        target_direction: Vec2f::I,
        spell_elements: Vec::new(),
        moving: false,
        delayed_magick: None,
    }
}

pub fn generate_actors<R: Rng>(material: &Material, number: usize, bounds: &Rectf, settings: &WorldSettings, id_counter: &mut u64, actors: &mut Vec<Actor>, rng: &mut R) {
    for _ in 0..number {
        actors.push(generate_actor(material.clone(), get_next_id(id_counter), bounds, settings, rng));
    }
}

pub fn generate_actor<R: Rng>(material: Material, id: u64, bounds: &Rectf, settings: &WorldSettings, rng: &mut R) -> Actor {
    let mass = get_body_mass(get_circle_volume(1.0), material);
    Actor {
        id,
        body: Body {
            mass,
            radius: rng.gen_range(0.8..1.2),
            restitution: get_material_restitution(material),
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: mass * settings.health_factor,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
        current_direction: Vec2f::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0)).normalized(),
        target_direction: Vec2f::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0)).normalized(),
        spell_elements: Vec::new(),
        moving: false,
        delayed_magick: None,
    }
}

pub fn generate_dynamic_objects<R: Rng>(material: &Material, number: usize, bounds: &Rectf, settings: &WorldSettings, id_counter: &mut u64, dynamic_objects: &mut Vec<DynamicObject>, rng: &mut R) {
    for _ in 0..number {
        dynamic_objects.push(generate_dynamic_object(material.clone(), get_next_id(id_counter), bounds, settings, rng));
    }
}

pub fn generate_dynamic_object<R: Rng>(material: Material, id: u64, bounds: &Rectf, settings: &WorldSettings, rng: &mut R) -> DynamicObject {
    let mass = get_body_mass(get_circle_volume(1.0), material);
    DynamicObject {
        id,
        body: Body {
            mass,
            radius: rng.gen_range(0.8..1.2),
            restitution: get_material_restitution(material),
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: mass * settings.health_factor,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
    }
}

pub fn generate_static_objects<R: Rng>(material: &Material, number: usize, bounds: &Rectf, settings: &WorldSettings, id_counter: &mut u64, static_objects: &mut Vec<StaticObject>, rng: &mut R) {
    for _ in 0..number {
        static_objects.push(generate_static_object(material.clone(), get_next_id(id_counter), bounds, settings, rng));
    }
}

pub fn generate_static_object<R: Rng>(material: Material, id: u64, bounds: &Rectf, settings: &WorldSettings, rng: &mut R) -> StaticObject {
    let mass = get_body_mass(get_circle_volume(1.0), material);
    StaticObject {
        id,
        body: Body {
            mass,
            radius: rng.gen_range(0.8..1.2),
            restitution: get_material_restitution(material),
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: mass * settings.health_factor,
        effect: Effect::default(),
        aura: Aura::default(),
    }
}
