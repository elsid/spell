use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rect::Rectf;
use crate::vec2::Vec2f;

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct World {
    pub frame: u64,
    pub settings: WorldSettings,
    pub bounds: Rectf,
    pub time: f64,
    pub id_counter: u64,
    pub players: Vec<Player>,
    pub actors: Vec<Actor>,
    pub projectiles: Vec<Projectile>,
    pub static_objects: Vec<StaticObject>,
    pub beams: Vec<Beam>,
    pub static_areas: Vec<StaticArea>,
    pub temp_areas: Vec<TempArea>,
    pub bounded_areas: Vec<BoundedArea>,
    pub fields: Vec<Field>,
    pub guns: Vec<Gun>,
    pub shields: Vec<Shield>,
    pub temp_obstacles: Vec<TempObstacle>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WorldSettings {
    pub max_magic_power: f64,
    pub decay_factor: f64,
    pub margin: f64,
    pub physical_damage_factor: f64,
    pub magical_damage_factor: f64,
    pub max_beam_length: f64,
    pub max_rotation_speed: f64,
    pub move_force: f64,
    pub magic_force_multiplier: f64,
    pub max_spell_elements: u8,
    pub max_beam_depth: u8,
    pub gravitational_acceleration: f64,
    pub spray_distance_factor: f64,
    pub spray_angle: f64,
    pub directed_magick_duration: f64,
    pub spray_force_factor: f64,
    pub area_of_effect_magick_duration: f64,
    pub border_width: f64,
    pub min_move_distance: f64,
    pub initial_player_actor_spawn_delay: f64,
    pub player_actor_respawn_delay: f64,
    pub base_gun_fire_period: f64,
    pub gun_bullet_radius: f64,
    pub gun_half_grouping_angle: f64,
    pub temp_obstacle_magick_duration: f64,
    pub temp_area_duration: f64,
    pub max_actor_speed: f64,
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            max_magic_power: 5.0,
            decay_factor: 1.0 / 5.0,
            margin: 0.1,
            physical_damage_factor: 1e-3,
            magical_damage_factor: 1e3,
            max_beam_length: 1e3,
            max_rotation_speed: 2.0 * std::f64::consts::PI,
            move_force: 5e4,
            magic_force_multiplier: 5e6,
            max_spell_elements: 5,
            max_beam_depth: 4,
            gravitational_acceleration: 9.8,
            spray_distance_factor: 2.0,
            spray_angle: std::f64::consts::FRAC_PI_8,
            directed_magick_duration: 3.0,
            spray_force_factor: 1e5,
            area_of_effect_magick_duration: 0.5,
            border_width: 0.1,
            min_move_distance: 1e-3,
            initial_player_actor_spawn_delay: 1.0,
            player_actor_respawn_delay: 5.0,
            base_gun_fire_period: 0.3,
            gun_bullet_radius: 0.2,
            gun_half_grouping_angle: std::f64::consts::PI / 12.0,
            temp_obstacle_magick_duration: 20.0,
            temp_area_duration: 5.0,
            max_actor_speed: 10.0,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct PlayerId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Player {
    pub id: PlayerId,
    pub active: bool,
    pub name: String,
    pub actor_id: Option<ActorId>,
    pub spawn_time: f64,
    pub deaths: u64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct ActorId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Actor {
    pub id: ActorId,
    pub player_id: PlayerId,
    pub active: bool,
    pub name: String,
    pub body: Body<Disk>,
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
    pub position_z: f64,
    pub velocity_z: f64,
    pub occupation: ActorOccupation,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct ProjectileId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Projectile {
    pub id: ProjectileId,
    pub body: Body<Disk>,
    pub position: Vec2f,
    pub health: f64,
    pub magick: Magick,
    pub velocity: Vec2f,
    pub dynamic_force: Vec2f,
    pub position_z: f64,
    pub velocity_z: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct StaticObjectId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StaticObject {
    pub id: StaticObjectId,
    pub body: Body<StaticShape>,
    pub position: Vec2f,
    pub rotation: f64,
    pub health: f64,
    pub effect: Effect,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct BeamId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct Beam {
    pub id: BeamId,
    pub actor_id: ActorId,
    pub magick: Magick,
    pub deadline: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct StaticAreaId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StaticArea {
    pub id: StaticAreaId,
    pub body: Body<StaticAreaShape>,
    pub position: Vec2f,
    pub rotation: f64,
    pub magick: Magick,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct TempAreaId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TempArea {
    pub id: TempAreaId,
    pub body: Body<Disk>,
    pub position: Vec2f,
    pub magick: Magick,
    pub deadline: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct BoundedAreaId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BoundedArea {
    pub id: BoundedAreaId,
    pub actor_id: ActorId,
    pub body: RingSector,
    pub magick: Magick,
    pub deadline: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct FieldId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Field {
    pub id: FieldId,
    pub actor_id: ActorId,
    pub body: RingSector,
    pub force: f64,
    pub deadline: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct GunId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Gun {
    pub id: GunId,
    pub actor_id: ActorId,
    pub shots_left: u64,
    pub shot_period: f64,
    pub bullet_force_factor: f64,
    pub bullet_power: [f64; 11],
    pub last_shot: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct ShieldId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Shield {
    pub id: ShieldId,
    pub actor_id: ActorId,
    pub body: Body<CircleArc>,
    pub position: Vec2f,
    pub created: f64,
    pub power: f64,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct TempObstacleId(pub u64);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TempObstacle {
    pub id: TempObstacleId,
    pub actor_id: ActorId,
    pub body: Body<Disk>,
    pub position: Vec2f,
    pub health: f64,
    pub magick: Magick,
    pub effect: Effect,
    pub deadline: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Body<Shape> {
    pub shape: Shape,
    pub material_type: MaterialType,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum StaticShape {
    CircleArc(CircleArc),
    Disk(Disk),
    Rectangle(Rectangle),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum StaticAreaShape {
    Disk(Disk),
    Rectangle(Rectangle),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Disk {
    pub radius: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CircleArc {
    pub radius: f64,
    pub length: f64,
    pub rotation: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RingSector {
    pub min_radius: f64,
    pub max_radius: f64,
    pub angle: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Rectangle {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Magick {
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Effect {
    pub applied: [f64; 11],
    pub power: [f64; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Aura {
    pub applied: f64,
    pub power: f64,
    pub radius: f64,
    pub elements: [bool; 11],
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DelayedMagick {
    pub started: f64,
    pub status: DelayedMagickStatus,
    pub power: [f64; 11],
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq)]
pub enum DelayedMagickStatus {
    Started,
    Throw,
    Shoot,
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq)]
pub enum ActorOccupation {
    None,
    Shooting(GunId),
    Spraying {
        bounded_area_id: BoundedAreaId,
        field_id: FieldId,
    },
    Beaming(BeamId),
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize)]
pub enum MaterialType {
    None,
    Flesh,
    Stone,
    Grass,
    Dirt,
    Water,
    Ice,
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

pub fn load_world<P: AsRef<Path>>(path: P) -> Result<World, String> {
    let file = match std::fs::File::open(path) {
        Ok(v) => v,
        Err(e) => return Err(format!("{}", e)),
    };
    match serde_json::from_reader(file) {
        Ok(v) => Ok(v),
        Err(e) => return Err(format!("{}", e)),
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    use crate::generators::{generate_static_area, generate_static_object};

    use super::*;

    #[test]
    fn serialized_default_world_size() {
        assert_eq!(bincode::serialize(&World::default()).unwrap().len(), 354);
    }

    #[test]
    fn serialized_actor_size() {
        assert_eq!(
            bincode::serialize(&Actor {
                id: ActorId(1),
                player_id: PlayerId(0),
                active: true,
                name: String::from("actor"),
                body: Body {
                    shape: Disk { radius: 1.0 },
                    material_type: MaterialType::Flesh,
                },
                position: Vec2f::ZERO,
                health: 1.0,
                effect: Effect::default(),
                aura: Aura::default(),
                velocity: Vec2f::ZERO,
                dynamic_force: Vec2f::ZERO,
                current_direction: Vec2f::ZERO,
                target_direction: Vec2f::ZERO,
                spell_elements: Vec::new(),
                moving: false,
                delayed_magick: None,
                position_z: 0.0,
                velocity_z: 0.0,
                occupation: ActorOccupation::None,
            })
            .unwrap()
            .len(),
            371
        );
    }

    #[test]
    fn serialized_projectile_size() {
        assert_eq!(
            bincode::serialize(&Projectile {
                id: ProjectileId(1),
                body: Body {
                    shape: Disk { radius: 1.0 },
                    material_type: MaterialType::Stone,
                },
                position: Vec2f::ZERO,
                health: 1.0,
                magick: Magick::default(),
                velocity: Vec2f::ZERO,
                dynamic_force: Vec2f::ZERO,
                position_z: 1.0,
                velocity_z: 0.0,
            })
            .unwrap()
            .len(),
            180
        );
    }

    #[test]
    fn serialized_static_object_size() {
        assert_eq!(
            bincode::serialize(&generate_static_object(
                MaterialType::Flesh,
                StaticObjectId(1),
                &Rectf::new(Vec2f::ZERO, Vec2f::new(1.0, 1.0)),
                &mut SmallRng::seed_from_u64(42),
            ))
            .unwrap()
            .len(),
            232
        );
    }

    #[test]
    fn serialized_default_beam_size() {
        assert_eq!(bincode::serialize(&Beam::default()).unwrap().len(), 112);
    }

    #[test]
    fn serialized_static_area_size() {
        assert_eq!(
            bincode::serialize(&generate_static_area(
                MaterialType::Flesh,
                Magick::default(),
                StaticAreaId(1),
                &Rectf::new(Vec2f::ZERO, Vec2f::new(1.0, 1.0)),
                &mut SmallRng::seed_from_u64(42),
            ))
            .unwrap()
            .len(),
            136
        );
    }
}
