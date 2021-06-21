use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::engine::get_next_id;
use crate::rect::Rectf;
use crate::vec2::Vec2f;
use crate::world::{
    Actor, Aura, Body, Disk, DynamicObject, Effect, Element, Magick, Material, PlayerId,
    StaticArea, StaticObject, StaticShape, World, WorldSettings,
};

pub fn make_rng(random_seed: Option<u64>) -> SmallRng {
    if let Some(value) = random_seed {
        SeedableRng::seed_from_u64(value)
    } else {
        SeedableRng::from_entropy()
    }
}

pub fn generate_world<R: Rng>(bounds: Rectf, rng: &mut R) -> World {
    let settings = WorldSettings::default();
    let mut id_counter = 1;
    let mut actors = Vec::new();
    generate_actors(
        Material::Flesh,
        rng.gen_range(8..12),
        &bounds,
        &mut id_counter,
        &mut actors,
        rng,
    );
    let mut dynamic_objects = Vec::new();
    let mut static_objects = Vec::new();
    for material in &[Material::Flesh, Material::Stone] {
        generate_dynamic_objects(
            *material,
            rng.gen_range(8..12),
            &bounds,
            &mut id_counter,
            &mut dynamic_objects,
            rng,
        );
        generate_static_objects(
            *material,
            rng.gen_range(8..12),
            &bounds,
            &mut id_counter,
            &mut static_objects,
            rng,
        );
    }
    let mut static_areas = vec![StaticArea {
        id: get_next_id(&mut id_counter),
        body: Body {
            shape: Disk {
                radius: bounds.min.distance(bounds.max) * 0.5,
            },
            material: Material::Dirt,
        },
        position: Vec2f::ZERO,
        magick: Magick::default(),
    }];
    generate_static_areas(
        Material::Grass,
        Magick::default(),
        rng.gen_range(8..12),
        &bounds,
        &mut id_counter,
        &mut static_areas,
        rng,
    );
    let water_magick = {
        let mut v = Magick::default();
        v.power[Element::Water as usize] = 1.0;
        v
    };
    generate_static_areas(
        Material::Water,
        water_magick,
        rng.gen_range(8..12),
        &bounds,
        &mut id_counter,
        &mut static_areas,
        rng,
    );
    World {
        frame: 0,
        settings,
        bounds,
        time: 0.0,
        id_counter,
        players: Vec::new(),
        actors,
        dynamic_objects,
        static_objects,
        beams: Vec::new(),
        static_areas,
        temp_areas: Vec::new(),
        bounded_areas: Vec::new(),
        fields: Vec::new(),
    }
}

pub fn generate_player_actor<R: Rng>(
    id: u64,
    player_id: PlayerId,
    name: String,
    bounds: &Rectf,
    rng: &mut R,
) -> Actor {
    let material = Material::Flesh;
    let delta = bounds.max - bounds.min;
    let middle = (bounds.max + bounds.min) / 2.0;
    let radius = 1.0;
    Actor {
        id,
        player_id,
        active: true,
        name,
        body: Body {
            shape: Disk { radius },
            material,
        },
        position: Vec2f::new(
            rng.gen_range(middle.x - delta.x * 0.25..middle.x + delta.x * 0.25),
            rng.gen_range(middle.y - delta.y * 0.25..middle.y + delta.y * 0.25),
        ),
        health: 1.0,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
        current_direction: Vec2f::I,
        target_direction: Vec2f::I,
        spell_elements: Vec::new(),
        moving: false,
        delayed_magick: None,
        position_z: radius,
        velocity_z: 0.0,
    }
}

pub fn generate_actors<R: Rng>(
    material: Material,
    number: usize,
    bounds: &Rectf,
    id_counter: &mut u64,
    actors: &mut Vec<Actor>,
    rng: &mut R,
) {
    for _ in 0..number {
        actors.push(generate_actor(
            material,
            get_next_id(id_counter),
            bounds,
            rng,
        ));
    }
}

pub fn generate_actor<R: Rng>(material: Material, id: u64, bounds: &Rectf, rng: &mut R) -> Actor {
    let radius = 1.0;
    Actor {
        id,
        player_id: PlayerId(0),
        active: true,
        name: format!("bot {}", id),
        body: Body {
            shape: Disk { radius },
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: 1.0,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
        current_direction: Vec2f::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0))
            .normalized(),
        target_direction: Vec2f::new(rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0))
            .normalized(),
        spell_elements: Vec::new(),
        moving: false,
        delayed_magick: None,
        position_z: radius,
        velocity_z: 0.0,
    }
}

pub fn generate_dynamic_objects<R: Rng>(
    material: Material,
    number: usize,
    bounds: &Rectf,
    id_counter: &mut u64,
    dynamic_objects: &mut Vec<DynamicObject>,
    rng: &mut R,
) {
    for _ in 0..number {
        dynamic_objects.push(generate_dynamic_object(
            material,
            get_next_id(id_counter),
            bounds,
            rng,
        ));
    }
}

pub fn generate_dynamic_object<R: Rng>(
    material: Material,
    id: u64,
    bounds: &Rectf,
    rng: &mut R,
) -> DynamicObject {
    let radius = rng.gen_range(0.8..1.2);
    DynamicObject {
        id,
        body: Body {
            shape: Disk { radius },
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: 1.0,
        effect: Effect::default(),
        aura: Aura::default(),
        velocity: Vec2f::ZERO,
        dynamic_force: Vec2f::ZERO,
        position_z: radius,
        velocity_z: 0.0,
    }
}

pub fn generate_static_objects<R: Rng>(
    material: Material,
    number: usize,
    bounds: &Rectf,
    id_counter: &mut u64,
    static_objects: &mut Vec<StaticObject>,
    rng: &mut R,
) {
    for _ in 0..number {
        static_objects.push(generate_static_object(
            material,
            get_next_id(id_counter),
            bounds,
            rng,
        ));
    }
}

pub fn generate_static_object<R: Rng>(
    material: Material,
    id: u64,
    bounds: &Rectf,
    rng: &mut R,
) -> StaticObject {
    StaticObject {
        id,
        body: Body {
            shape: StaticShape::Disk(Disk {
                radius: rng.gen_range(0.8..1.2),
            }),
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        health: 1.0,
        effect: Effect::default(),
        aura: Aura::default(),
    }
}

pub fn generate_static_areas<R: Rng>(
    material: Material,
    magick: Magick,
    number: usize,
    bounds: &Rectf,
    id_counter: &mut u64,
    static_areas: &mut Vec<StaticArea>,
    rng: &mut R,
) {
    for _ in 0..number {
        static_areas.push(generate_static_area(
            material,
            magick.clone(),
            get_next_id(id_counter),
            bounds,
            rng,
        ));
    }
}

pub fn generate_static_area<R: Rng>(
    material: Material,
    magick: Magick,
    id: u64,
    bounds: &Rectf,
    rng: &mut R,
) -> StaticArea {
    StaticArea {
        id,
        body: Body {
            shape: Disk {
                radius: rng.gen_range(10.0..15.0),
            },
            material,
        },
        position: Vec2f::new(
            rng.gen_range(bounds.min.x..bounds.max.x),
            rng.gen_range(bounds.min.y..bounds.max.y),
        ),
        magick,
    }
}
