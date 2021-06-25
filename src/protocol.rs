use std::fmt::Formatter;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::vec2::Vec2f;
use crate::world::{
    Actor, ActorId, ActorOccupation, Aura, Beam, BoundedArea, DelayedMagick, Effect, Element,
    Field, Gun, GunId, Player, PlayerId, Projectile, ProjectileId, StaticArea, StaticObject,
    StaticObjectId, TempArea, TempAreaId, World,
};

pub const HEARTBEAT_PERIOD: Duration = Duration::from_secs(1);
pub const MIN_PLAYER_NAME_LEN: usize = 3;
pub const MAX_PLAYER_NAME_LEN: usize = 16;
pub const MAX_SERVER_MESSAGE_SIZE: usize = 65_507;
pub const MAX_SERVER_MESSAGE_DATA_SIZE: usize = 32_768;
pub const MAX_CLIENT_MESSAGE_SIZE: usize = 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerMessage {
    pub session_id: u64,
    pub number: u64,
    pub decompressed_data_size: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ClientMessage {
    pub session_id: u64,
    pub number: u64,
    pub data: ClientMessageData,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub enum ServerMessageData {
    NewPlayer {
        update_period: Duration,
        player_id: PlayerId,
    },
    Error(String),
    GameUpdate(GameUpdate),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessageData {
    Join(String),
    Quit,
    Heartbeat,
    PlayerControl(PlayerControl),
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub enum GameUpdate {
    SetPlayerId(PlayerId),
    WorldSnapshot {
        ack_actor_action_world_frame: u64,
        ack_cast_action_world_frame: u64,
        world: Box<World>,
    },
    WorldUpdate {
        ack_actor_action_world_frame: u64,
        ack_cast_action_world_frame: u64,
        world_update: Box<WorldUpdate>,
    },
    GameOver(String),
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WorldUpdate {
    pub before_frame: u64,
    pub after_frame: u64,
    pub time: f64,
    pub players: Option<Difference<Player, PlayerUpdate>>,
    pub actors: Option<Difference<Actor, ActorUpdate>>,
    pub projectiles: Option<Difference<Projectile, ProjectileUpdate>>,
    pub static_objects: Option<Difference<StaticObject, StaticObjectUpdate>>,
    pub beams: Option<ExistenceDifference<Beam>>,
    pub static_areas: Option<ExistenceDifference<StaticArea>>,
    pub temp_areas: Option<Difference<TempArea, TempAreaUpdate>>,
    pub bounded_areas: Option<ExistenceDifference<BoundedArea>>,
    pub fields: Option<ExistenceDifference<Field>>,
    pub guns: Option<Difference<Gun, GunUpdate>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct PlayerUpdate {
    pub id: PlayerId,
    pub actor_id: Option<Option<ActorId>>,
    pub spawn_time: Option<f64>,
    pub deaths: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct ActorUpdate {
    pub id: ActorId,
    pub position: Option<Vec2f>,
    pub health: Option<f64>,
    pub effect: Option<Effect>,
    pub aura: Option<Aura>,
    pub velocity: Option<Vec2f>,
    pub dynamic_force: Option<Vec2f>,
    pub current_direction: Option<Vec2f>,
    pub target_direction: Option<Vec2f>,
    pub spell_elements: Option<Vec<Element>>,
    pub moving: Option<bool>,
    pub delayed_magick: Option<Option<DelayedMagick>>,
    pub position_z: Option<f64>,
    pub velocity_z: Option<f64>,
    pub occupation: Option<ActorOccupation>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct ProjectileUpdate {
    pub id: ProjectileId,
    pub position: Option<Vec2f>,
    pub health: Option<f64>,
    pub effect: Option<Effect>,
    pub velocity: Option<Vec2f>,
    pub dynamic_force: Option<Vec2f>,
    pub position_z: Option<f64>,
    pub velocity_z: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct StaticObjectUpdate {
    pub id: StaticObjectId,
    pub health: Option<f64>,
    pub effect: Option<Effect>,
    pub aura: Option<Aura>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct TempAreaUpdate {
    pub id: TempAreaId,
    pub effect: Option<Effect>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct GunUpdate {
    pub id: GunId,
    pub shots_left: Option<u64>,
    pub last_shot: Option<f64>,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct PlayerControl {
    pub ack_world_frame: u64,
    pub cast_action_world_frame: u64,
    pub actor_action: ActorAction,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
pub struct ActorAction {
    pub moving: bool,
    pub target_direction: Vec2f,
    pub cast_action: Option<CastAction>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum CastAction {
    AddSpellElement(Element),
    StartDirectedMagick,
    CompleteDirectedMagick,
    SelfMagick,
    StartAreaOfEffectMagick,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum HttpMessage {
    Ok,
    Error { message: String },
    Sessions { sessions: Vec<Session> },
    Status { status: ServerStatus },
    World { world: Box<World> },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Session {
    pub session_id: u64,
    pub peer: String,
    pub last_recv_time: f64,
    pub state: UdpSessionState,
    pub game: Option<GameSessionInfo>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct GameSessionInfo {
    pub session_id: u64,
    pub player_id: u64,
    pub last_message_time: f64,
    pub last_message_number: u64,
    pub messages_per_frame: u8,
    pub dropped_messages: usize,
    pub delayed_messages: usize,
    pub ack_world_frame: u64,
    pub ack_cast_action_frame: u64,
    pub since_last_message: f64,
    pub world_frame_delay: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone)]
pub enum UdpSessionState {
    New,
    Established,
    Done,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ServerStatus {
    pub fps: Metric,
    pub frame_duration: Metric,
    pub sessions: usize,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Metric {
    pub min: f64,
    pub mean: f64,
    pub max: f64,
}

pub fn get_server_message_data_type(value: &ServerMessageData) -> &'static str {
    match value {
        ServerMessageData::NewPlayer { .. } => "NewPlayer",
        ServerMessageData::Error(..) => "Error",
        ServerMessageData::GameUpdate(..) => "GameUpdate",
    }
}

pub fn get_client_message_data_type(value: &ClientMessageData) -> &'static str {
    match value {
        ClientMessageData::Join(..) => "Join",
        ClientMessageData::Quit => "Quit",
        ClientMessageData::Heartbeat => "Heartbeat",
        ClientMessageData::PlayerControl(..) => "PlayerControl",
    }
}

pub fn make_world_update(before: &World, after: &World) -> WorldUpdate {
    WorldUpdate {
        before_frame: before.frame,
        after_frame: after.frame,
        time: after.time,
        players: get_players_difference(&before.players, &after.players),
        actors: get_actors_difference(&before.actors, &after.actors),
        projectiles: get_projectiles_difference(&before.projectiles, &after.projectiles),
        static_objects: get_static_objects_difference(
            &before.static_objects,
            &after.static_objects,
        ),
        beams: get_beams_difference(&before.beams, &after.beams),
        static_areas: get_static_areas_difference(&before.static_areas, &after.static_areas),
        temp_areas: get_temp_areas_difference(&before.temp_areas, &after.temp_areas),
        bounded_areas: get_bounded_areas_difference(&before.bounded_areas, &after.bounded_areas),
        fields: get_fields_difference(&before.fields, &after.fields),
        guns: get_guns_difference(&before.guns, &after.guns),
    }
}

fn get_players_difference(
    before: &[Player],
    after: &[Player],
) -> Option<Difference<Player, PlayerUpdate>> {
    get_difference(before, after, |v| v.id.0, make_player_update)
}

fn get_actors_difference(
    before: &[Actor],
    after: &[Actor],
) -> Option<Difference<Actor, ActorUpdate>> {
    get_difference(before, after, |v| v.id.0, make_actor_update)
}

fn get_projectiles_difference(
    before: &[Projectile],
    after: &[Projectile],
) -> Option<Difference<Projectile, ProjectileUpdate>> {
    get_difference(before, after, |v| v.id.0, make_projectile_update)
}

fn get_static_objects_difference(
    before: &[StaticObject],
    after: &[StaticObject],
) -> Option<Difference<StaticObject, StaticObjectUpdate>> {
    get_difference(before, after, |v| v.id.0, make_static_object_update)
}

fn get_beams_difference(before: &[Beam], after: &[Beam]) -> Option<ExistenceDifference<Beam>> {
    get_existence_difference(before, after, |v| v.id.0)
}

fn get_static_areas_difference(
    before: &[StaticArea],
    after: &[StaticArea],
) -> Option<ExistenceDifference<StaticArea>> {
    get_existence_difference(before, after, |v| v.id.0)
}

fn get_temp_areas_difference(
    before: &[TempArea],
    after: &[TempArea],
) -> Option<Difference<TempArea, TempAreaUpdate>> {
    get_difference(before, after, |v| v.id.0, make_temp_area_update)
}

fn get_bounded_areas_difference(
    before: &[BoundedArea],
    after: &[BoundedArea],
) -> Option<ExistenceDifference<BoundedArea>> {
    get_existence_difference(before, after, |v| v.id.0)
}

fn get_fields_difference(before: &[Field], after: &[Field]) -> Option<ExistenceDifference<Field>> {
    get_existence_difference(before, after, |v| v.id.0)
}

fn get_guns_difference(before: &[Gun], after: &[Gun]) -> Option<Difference<Gun, GunUpdate>> {
    get_difference(before, after, |v| v.id.0, make_gun_update)
}

fn make_player_update(b: &Player, a: &Player) -> Option<PlayerUpdate> {
    let mut r = PlayerUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.actor_id, &a.actor_id, &mut r.actor_id) || d;
    d = clone_if_different(&b.spawn_time, &a.spawn_time, &mut r.spawn_time) || d;
    d = clone_if_different(&b.deaths, &a.deaths, &mut r.deaths) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_actor_update(b: &Actor, a: &Actor) -> Option<ActorUpdate> {
    let mut r = ActorUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.position, &a.position, &mut r.position) || d;
    d = clone_if_different(&b.health, &a.health, &mut r.health) || d;
    d = clone_if_different(&b.effect, &a.effect, &mut r.effect) || d;
    d = clone_if_different(&b.aura, &a.aura, &mut r.aura) || d;
    d = clone_if_different(&b.velocity, &a.velocity, &mut r.velocity) || d;
    d = clone_if_different(&b.dynamic_force, &a.dynamic_force, &mut r.dynamic_force) || d;
    d = clone_if_different(
        &b.current_direction,
        &a.current_direction,
        &mut r.current_direction,
    ) || d;
    d = clone_if_different(
        &b.target_direction,
        &a.target_direction,
        &mut r.target_direction,
    ) || d;
    d = clone_if_different(&b.spell_elements, &a.spell_elements, &mut r.spell_elements) || d;
    d = clone_if_different(&b.moving, &a.moving, &mut r.moving) || d;
    d = clone_if_different(&b.delayed_magick, &a.delayed_magick, &mut r.delayed_magick) || d;
    d = clone_if_different(&b.position_z, &a.position_z, &mut r.position_z) || d;
    d = clone_if_different(&b.velocity_z, &a.velocity_z, &mut r.velocity_z) || d;
    d = clone_if_different(&b.occupation, &a.occupation, &mut r.occupation) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_projectile_update(b: &Projectile, a: &Projectile) -> Option<ProjectileUpdate> {
    let mut r = ProjectileUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.position, &a.position, &mut r.position) || d;
    d = clone_if_different(&b.health, &a.health, &mut r.health) || d;
    d = clone_if_different(&b.effect, &a.effect, &mut r.effect) || d;
    d = clone_if_different(&b.velocity, &a.velocity, &mut r.velocity) || d;
    d = clone_if_different(&b.dynamic_force, &a.dynamic_force, &mut r.dynamic_force) || d;
    d = clone_if_different(&b.position_z, &a.position_z, &mut r.position_z) || d;
    d = clone_if_different(&b.velocity_z, &a.velocity_z, &mut r.velocity_z) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_static_object_update(b: &StaticObject, a: &StaticObject) -> Option<StaticObjectUpdate> {
    let mut r = StaticObjectUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.health, &a.health, &mut r.health) || d;
    d = clone_if_different(&b.effect, &a.effect, &mut r.effect) || d;
    d = clone_if_different(&b.aura, &a.aura, &mut r.aura) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_temp_area_update(b: &TempArea, a: &TempArea) -> Option<TempAreaUpdate> {
    let mut r = TempAreaUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.effect, &a.effect, &mut r.effect) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_gun_update(b: &Gun, a: &Gun) -> Option<GunUpdate> {
    let mut r = GunUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.shots_left, &a.shots_left, &mut r.shots_left) || d;
    d = clone_if_different(&b.last_shot, &a.last_shot, &mut r.last_shot) || d;
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn clone_if_different<T>(before: &T, after: &T, out: &mut Option<T>) -> bool
where
    T: PartialEq + Clone,
{
    if *before != *after {
        *out = Some(after.clone());
        true
    } else {
        false
    }
}

#[derive(Default, Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct Difference<T: std::fmt::Debug + PartialEq, U: std::fmt::Debug + PartialEq> {
    pub added: Option<Vec<T>>,
    pub updated: Option<Vec<U>>,
    pub removed: Option<Vec<u64>>,
}

fn get_difference<T, U, GetId, MakeUpdate>(
    before: &[T],
    after: &[T],
    get_id: GetId,
    make_update: MakeUpdate,
) -> Option<Difference<T, U>>
where
    T: Clone + PartialEq + std::fmt::Debug,
    U: PartialEq + std::fmt::Debug,
    GetId: Fn(&T) -> u64,
    MakeUpdate: Fn(&T, &T) -> Option<U>,
{
    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut removed = Vec::new();
    let mut before_index = 0;
    let mut after_index = 0;
    while before_index != before.len() {
        if after_index == after.len() {
            before[before_index..before.len()]
                .iter()
                .for_each(|v| removed.push(get_id(v)));
            break;
        }
        match get_id(&before[before_index]).cmp(&get_id(&after[after_index])) {
            std::cmp::Ordering::Less => {
                removed.push(get_id(&before[before_index]));
                before_index += 1;
            }
            std::cmp::Ordering::Equal => {
                if let Some(update) = make_update(&before[before_index], &after[after_index]) {
                    updated.push(update);
                }
                before_index += 1;
                after_index += 1;
            }
            std::cmp::Ordering::Greater => {
                added.push(after[after_index].clone());
                after_index += 1;
            }
        }
    }
    added.extend_from_slice(&after[after_index..after.len()]);
    if added.is_empty() && updated.is_empty() && removed.is_empty() {
        return None;
    }
    Some(Difference {
        added: if added.is_empty() { None } else { Some(added) },
        updated: if updated.is_empty() {
            None
        } else {
            Some(updated)
        },
        removed: if removed.is_empty() {
            None
        } else {
            Some(removed)
        },
    })
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
pub struct ExistenceDifference<T: std::fmt::Debug + PartialEq> {
    pub added: Option<Vec<T>>,
    pub removed: Option<Vec<u64>>,
}

fn get_existence_difference<T, GetId>(
    before: &[T],
    after: &[T],
    get_id: GetId,
) -> Option<ExistenceDifference<T>>
where
    T: Clone + PartialEq + std::fmt::Debug,
    GetId: Fn(&T) -> u64,
{
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut before_index = 0;
    let mut after_index = 0;
    while before_index != before.len() {
        if after_index == after.len() {
            before[before_index..before.len()]
                .iter()
                .for_each(|v| removed.push(get_id(v)));
            break;
        }
        match get_id(&before[before_index]).cmp(&get_id(&after[after_index])) {
            std::cmp::Ordering::Less => {
                removed.push(get_id(&before[before_index]));
                before_index += 1;
            }
            std::cmp::Ordering::Equal => {
                before_index += 1;
                after_index += 1;
            }
            std::cmp::Ordering::Greater => {
                added.push(after[after_index].clone());
                after_index += 1;
            }
        }
    }
    added.extend_from_slice(&after[after_index..after.len()]);
    if added.is_empty() && removed.is_empty() {
        return None;
    }
    Some(ExistenceDifference {
        added: if added.is_empty() { None } else { Some(added) },
        removed: if removed.is_empty() {
            None
        } else {
            Some(removed)
        },
    })
}

pub fn apply_world_update(update: WorldUpdate, world: &mut World) {
    if update.after_frame < world.frame {
        return;
    }
    world.frame = update.after_frame;
    world.time = update.time;
    apply_difference(
        update.players,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_player_update,
        &mut world.players,
    );
    apply_difference(
        update.actors,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_actor_update,
        &mut world.actors,
    );
    apply_difference(
        update.projectiles,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_projectile_update,
        &mut world.projectiles,
    );
    apply_difference(
        update.static_objects,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_static_object_update,
        &mut world.static_objects,
    );
    apply_existence_difference(update.beams, &|v| v.id.0, &mut world.beams);
    apply_existence_difference(update.static_areas, &|v| v.id.0, &mut world.static_areas);
    apply_difference(
        update.temp_areas,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_temp_area_update,
        &mut world.temp_areas,
    );
    apply_existence_difference(update.bounded_areas, &|v| v.id.0, &mut world.bounded_areas);
    apply_existence_difference(update.fields, &|v| v.id.0, &mut world.fields);
    apply_difference(
        update.guns,
        &|v| v.id.0,
        &|a, b| a.id == b.id,
        apply_gun_update,
        &mut world.guns,
    );
}

fn apply_difference<T, U, GetId, EqualById, ApplyUpdate>(
    difference: Option<Difference<T, U>>,
    get_id: &GetId,
    equal_by_id: &EqualById,
    apply_update: ApplyUpdate,
    dst: &mut Vec<T>,
) where
    T: std::fmt::Debug + PartialEq,
    U: std::fmt::Debug + PartialEq,
    GetId: Fn(&T) -> u64,
    EqualById: Fn(&U, &T) -> bool,
    ApplyUpdate: Fn(&U, &mut T),
{
    if let Some(value) = difference {
        remove_from(value.removed, get_id, dst);
        update_from(value.updated, equal_by_id, apply_update, dst);
        add_or_update_from(value.added, get_id, dst);
    }
}

fn apply_existence_difference<T, GetId>(
    difference: Option<ExistenceDifference<T>>,
    get_id: &GetId,
    dst: &mut Vec<T>,
) where
    T: std::fmt::Debug + PartialEq,
    GetId: Fn(&T) -> u64,
{
    if let Some(value) = difference {
        remove_from(value.removed, get_id, dst);
        add_or_update_from(value.added, get_id, dst);
    }
}

fn add_or_update_from<GetId, T>(src: Option<Vec<T>>, get_id: &GetId, dst: &mut Vec<T>)
where
    GetId: Fn(&T) -> u64,
{
    if let Some(values) = src {
        for value in values {
            if let Some(v) = dst.iter_mut().find(|v| get_id(*v) == get_id(&value)) {
                *v = value;
            } else {
                dst.push(value);
            }
        }
    }
}

fn update_from<U, EqualById, ApplyUpdate, T>(
    src: Option<Vec<U>>,
    equal_by_id: &EqualById,
    apply_update: ApplyUpdate,
    dst: &mut Vec<T>,
) where
    EqualById: Fn(&U, &T) -> bool,
    ApplyUpdate: Fn(&U, &mut T),
{
    if let Some(updates) = src {
        for update in updates {
            if let Some(value) = dst.iter_mut().find(|v| equal_by_id(&update, *v)) {
                apply_update(&update, value);
            }
        }
    }
}

fn remove_from<GetId, T>(ids: Option<Vec<u64>>, get_id: &GetId, values: &mut Vec<T>)
where
    GetId: Fn(&T) -> u64,
{
    if let Some(removed) = ids {
        values.retain(|v| !removed.contains(&get_id(v)));
    }
}

fn apply_player_update(src: &PlayerUpdate, dst: &mut Player) {
    clone_if_some(&src.actor_id, &mut dst.actor_id);
    clone_if_some(&src.spawn_time, &mut dst.spawn_time);
    clone_if_some(&src.deaths, &mut dst.deaths);
}

fn apply_actor_update(src: &ActorUpdate, dst: &mut Actor) {
    clone_if_some(&src.position, &mut dst.position);
    clone_if_some(&src.health, &mut dst.health);
    clone_if_some(&src.effect, &mut dst.effect);
    clone_if_some(&src.aura, &mut dst.aura);
    clone_if_some(&src.velocity, &mut dst.velocity);
    clone_if_some(&src.dynamic_force, &mut dst.dynamic_force);
    clone_if_some(&src.current_direction, &mut dst.current_direction);
    clone_if_some(&src.target_direction, &mut dst.target_direction);
    clone_if_some(&src.spell_elements, &mut dst.spell_elements);
    clone_if_some(&src.moving, &mut dst.moving);
    clone_if_some(&src.delayed_magick, &mut dst.delayed_magick);
    clone_if_some(&src.position_z, &mut dst.position_z);
    clone_if_some(&src.velocity_z, &mut dst.velocity_z);
    clone_if_some(&src.occupation, &mut dst.occupation);
}

fn apply_projectile_update(src: &ProjectileUpdate, dst: &mut Projectile) {
    clone_if_some(&src.position, &mut dst.position);
    clone_if_some(&src.health, &mut dst.health);
    clone_if_some(&src.effect, &mut dst.effect);
    clone_if_some(&src.velocity, &mut dst.velocity);
    clone_if_some(&src.dynamic_force, &mut dst.dynamic_force);
    clone_if_some(&src.position_z, &mut dst.position_z);
    clone_if_some(&src.velocity_z, &mut dst.velocity_z);
}

fn apply_static_object_update(src: &StaticObjectUpdate, dst: &mut StaticObject) {
    clone_if_some(&src.health, &mut dst.health);
    clone_if_some(&src.effect, &mut dst.effect);
    clone_if_some(&src.aura, &mut dst.aura);
}

fn apply_temp_area_update(src: &TempAreaUpdate, dst: &mut TempArea) {
    clone_if_some(&src.effect, &mut dst.effect);
}

fn apply_gun_update(src: &GunUpdate, dst: &mut Gun) {
    clone_if_some(&src.shots_left, &mut dst.shots_left);
    clone_if_some(&src.last_shot, &mut dst.last_shot);
}

fn clone_if_some<T: Clone>(src: &Option<T>, dst: &mut T) {
    if let Some(value) = src.as_ref() {
        *dst = value.clone();
    }
}

pub fn add_all_removed<'a, I>(src: I, dst: &mut WorldUpdate)
where
    I: Iterator<Item = &'a WorldUpdate>,
{
    let mut sort_actors = false;
    let mut sort_projectiles = false;
    let mut sort_static_objects = false;
    let mut sort_beams = false;
    let mut sort_static_areas = false;
    let mut sort_temp_areas = false;
    let mut sort_bounded_areas = false;
    let mut sort_fields = false;
    let mut sort_guns = false;
    for v in src {
        add_removed_difference(&v.actors, &mut dst.actors, &mut sort_actors);
        add_removed_difference(&v.projectiles, &mut dst.projectiles, &mut sort_projectiles);
        add_removed_difference(
            &v.static_objects,
            &mut dst.static_objects,
            &mut sort_static_objects,
        );
        add_removed_existence_difference(&v.beams, &mut dst.beams, &mut sort_beams);
        add_removed_existence_difference(
            &v.static_areas,
            &mut dst.static_areas,
            &mut sort_static_areas,
        );
        add_removed_difference(&v.temp_areas, &mut dst.temp_areas, &mut sort_temp_areas);
        add_removed_existence_difference(
            &v.bounded_areas,
            &mut dst.bounded_areas,
            &mut sort_bounded_areas,
        );
        add_removed_existence_difference(&v.fields, &mut dst.fields, &mut sort_fields);
        add_removed_difference(&v.guns, &mut dst.guns, &mut sort_guns);
    }
    sort_and_dedup(sort_actors, dst.actors.as_mut().map(|v| v.removed.as_mut()));
    sort_and_dedup(
        sort_projectiles,
        dst.projectiles.as_mut().map(|v| v.removed.as_mut()),
    );
    sort_and_dedup(
        sort_static_objects,
        dst.static_objects.as_mut().map(|v| v.removed.as_mut()),
    );
    sort_and_dedup(sort_beams, dst.beams.as_mut().map(|v| v.removed.as_mut()));
    sort_and_dedup(
        sort_static_areas,
        dst.static_areas.as_mut().map(|v| v.removed.as_mut()),
    );
    sort_and_dedup(
        sort_temp_areas,
        dst.temp_areas.as_mut().map(|v| v.removed.as_mut()),
    );
    sort_and_dedup(
        sort_bounded_areas,
        dst.bounded_areas.as_mut().map(|v| v.removed.as_mut()),
    );
    sort_and_dedup(sort_fields, dst.fields.as_mut().map(|v| v.removed.as_mut()));
    sort_and_dedup(sort_guns, dst.guns.as_mut().map(|v| v.removed.as_mut()));
}

fn add_removed_difference<T, U>(
    src: &Option<Difference<T, U>>,
    dst: &mut Option<Difference<T, U>>,
    sort: &mut bool,
) where
    T: std::fmt::Debug + PartialEq,
    U: std::fmt::Debug + PartialEq,
{
    if let Some(src_diff) = src.as_ref() {
        if let Some(src_removed) = src_diff.removed.as_ref() {
            if let Some(dst_diff) = dst.as_mut() {
                if let Some(dst_removed) = dst_diff.removed.as_mut() {
                    dst_removed.extend_from_slice(src_removed);
                    *sort = true;
                } else {
                    dst_diff.removed = Some(src_removed.clone());
                }
            } else {
                *dst = Some(Difference {
                    added: None,
                    updated: None,
                    removed: Some(src_removed.clone()),
                });
            }
        }
    }
}

fn add_removed_existence_difference<T>(
    src: &Option<ExistenceDifference<T>>,
    dst: &mut Option<ExistenceDifference<T>>,
    sort: &mut bool,
) where
    T: std::fmt::Debug + PartialEq,
{
    if let Some(src_diff) = src.as_ref() {
        if let Some(src_removed) = src_diff.removed.as_ref() {
            if let Some(dst_diff) = dst.as_mut() {
                if let Some(dst_removed) = dst_diff.removed.as_mut() {
                    dst_removed.extend_from_slice(src_removed);
                    *sort = true;
                } else {
                    dst_diff.removed = Some(src_removed.clone());
                }
            } else {
                *dst = Some(ExistenceDifference {
                    added: None,
                    removed: Some(src_removed.clone()),
                });
            }
        }
    }
}

fn sort_and_dedup(sort: bool, values: Option<Option<&mut Vec<u64>>>) {
    if sort {
        let v = values.unwrap().unwrap();
        v.sort_unstable();
        v.dedup();
    }
}

pub fn is_valid_player_name(value: &str) -> bool {
    MIN_PLAYER_NAME_LEN <= value.len()
        && value.len() <= MAX_PLAYER_NAME_LEN
        && value.chars().all(|v| v.is_alphabetic())
}

#[derive(Debug)]
pub enum DeserializeError {
    SerializedServerMessageTooLong(usize),
    CompressedServerMessageDataTooLong(usize),
    DeclaredDecompressedServerMessageDataTooLong(usize),
    DecompressError(lz4_flex::block::DecompressError),
    DecompressedServerMessageDataTooLong(usize),
    ClientMessageTooLong(usize),
    DeserializeError(bincode::Error),
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeserializeError::SerializedServerMessageTooLong(v) => {
                write!(f, "Serialized server message is too long: {} bytes", v)
            }
            DeserializeError::CompressedServerMessageDataTooLong(v) => {
                write!(f, "Compressed server message data is too long: {} bytes", v)
            }
            DeserializeError::DeclaredDecompressedServerMessageDataTooLong(v) => {
                write!(
                    f,
                    "Declared decompressed server message data is too long: {} bytes",
                    v
                )
            }
            DeserializeError::DecompressError(e) => write!(f, "{}", e),
            DeserializeError::DecompressedServerMessageDataTooLong(v) => {
                write!(f, "Decompressed message is tool long: {} bytes", v)
            }
            DeserializeError::ClientMessageTooLong(v) => {
                write!(f, "Client message is tool long: {} bytes", v)
            }
            DeserializeError::DeserializeError(e) => write!(f, "{}", e),
        }
    }
}

pub fn make_server_message(
    session_id: u64,
    number: u64,
    data: &ServerMessageData,
) -> ServerMessage {
    let serialized = bincode::serialize(data).unwrap();
    if serialized.len() > MAX_SERVER_MESSAGE_DATA_SIZE {
        warn!(
            "Serialized data size is greater than limit: {} > {}",
            serialized.len(),
            MAX_SERVER_MESSAGE_DATA_SIZE
        );
    }
    let compressed = lz4_flex::compress(&serialized);
    ServerMessage {
        session_id,
        number,
        decompressed_data_size: serialized.len() as u64,
        data: compressed,
    }
}

pub fn deserialize_client_message(input: &[u8]) -> Result<ClientMessage, DeserializeError> {
    if input.len() > MAX_CLIENT_MESSAGE_SIZE {
        return Err(DeserializeError::ClientMessageTooLong(input.len()));
    }
    match bincode::deserialize(input) {
        Ok(v) => Ok(v),
        Err(e) => Err(DeserializeError::DeserializeError(e)),
    }
}

pub fn serialize_server_message(value: &ServerMessage) -> Vec<u8> {
    bincode::serialize(value).unwrap()
}

pub fn deserialize_server_message(input: &[u8]) -> Result<ServerMessage, DeserializeError> {
    if input.len() > MAX_SERVER_MESSAGE_SIZE {
        return Err(DeserializeError::SerializedServerMessageTooLong(
            input.len(),
        ));
    }
    match bincode::deserialize(&input) {
        Ok(v) => Ok(v),
        Err(e) => Err(DeserializeError::DeserializeError(e)),
    }
}

pub fn deserialize_server_message_data(
    input: &[u8],
    decompressed_size: usize,
) -> Result<ServerMessageData, DeserializeError> {
    if input.len() > MAX_SERVER_MESSAGE_DATA_SIZE {
        return Err(DeserializeError::CompressedServerMessageDataTooLong(
            input.len(),
        ));
    }
    if decompressed_size > MAX_SERVER_MESSAGE_DATA_SIZE {
        return Err(
            DeserializeError::DeclaredDecompressedServerMessageDataTooLong(decompressed_size),
        );
    }
    let decompressed = match lz4_flex::decompress(input, decompressed_size) {
        Ok(v) => v,
        Err(e) => return Err(DeserializeError::DecompressError(e)),
    };
    if decompressed.len() > MAX_SERVER_MESSAGE_DATA_SIZE {
        return Err(DeserializeError::DecompressedServerMessageDataTooLong(
            decompressed.len(),
        ));
    }
    match bincode::deserialize(&decompressed) {
        Ok(v) => Ok(v),
        Err(e) => Err(DeserializeError::DeserializeError(e)),
    }
}

#[cfg(test)]
mod tests {
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    use crate::generators::generate_world;
    use crate::rect::Rectf;
    use crate::vec2::Vec2f;

    use super::*;

    #[test]
    fn serialized_default_world_update_size() {
        assert_eq!(
            bincode::serialize(&WorldUpdate::default()).unwrap().len(),
            34
        );
    }

    #[test]
    fn serialized_default_actor_update_size() {
        assert_eq!(
            bincode::serialize(&ActorUpdate::default()).unwrap().len(),
            22
        );
    }

    #[test]
    fn serialized_default_projectile_update_size() {
        assert_eq!(
            bincode::serialize(&ProjectileUpdate::default())
                .unwrap()
                .len(),
            15
        );
    }

    #[test]
    fn serialized_default_static_object_update_size() {
        assert_eq!(
            bincode::serialize(&StaticObjectUpdate::default())
                .unwrap()
                .len(),
            11
        );
    }

    #[test]
    fn serialized_default_temp_area_update_size() {
        assert_eq!(
            bincode::serialize(&TempAreaUpdate::default())
                .unwrap()
                .len(),
            9
        );
    }

    #[derive(Default, Clone, PartialEq, Debug)]
    struct TestObject {
        id: u64,
        value: f64,
    }

    #[derive(Default, PartialEq, Debug)]
    struct TestObjectUpdate {
        id: u64,
        value: Option<f64>,
    }

    fn make_test_object_update(b: &TestObject, a: &TestObject) -> Option<TestObjectUpdate> {
        let mut r = TestObjectUpdate::default();
        let mut d = false;
        d = clone_if_different(&b.value, &a.value, &mut r.value) || d;
        if d {
            r.id = a.id;
            Some(r)
        } else {
            None
        }
    }

    #[test]
    fn get_difference_should_return_empty_difference_for_equal_vectors() {
        let values = vec![TestObject { id: 1, value: 3.15 }];
        assert_eq!(
            get_difference(&values, &values, |v| v.id, make_test_object_update),
            None
        );
    }

    #[test]
    fn get_difference_should_return_added_when_value_is_missing_from_before() {
        let before = vec![TestObject { id: 1, value: 3.15 }];
        let after = vec![
            TestObject { id: 1, value: 3.15 },
            TestObject { id: 2, value: 2.7 },
        ];
        assert_eq!(
            get_difference(&before, &after, |v| v.id, make_test_object_update),
            Some(Difference {
                added: Some(vec![TestObject { id: 2, value: 2.7 }]),
                updated: None,
                removed: None,
            })
        );
    }

    #[test]
    fn get_difference_should_return_update_when_value_has_changed() {
        let before = vec![TestObject { id: 1, value: 3.15 }];
        let after = vec![TestObject { id: 1, value: 2.7 }];
        assert_eq!(
            get_difference(&before, &after, |v| v.id, make_test_object_update),
            Some(Difference {
                added: None,
                updated: Some(vec![TestObjectUpdate {
                    id: 1,
                    value: Some(2.7),
                }]),
                removed: None,
            })
        );
    }

    #[test]
    fn get_difference_should_return_removed_when_value_is_missing_from_after() {
        let before = vec![
            TestObject { id: 1, value: 3.15 },
            TestObject { id: 2, value: 2.7 },
        ];
        let after = vec![TestObject { id: 2, value: 2.7 }];
        assert_eq!(
            get_difference(&before, &after, |v| v.id, make_test_object_update),
            Some(Difference {
                added: None,
                updated: None,
                removed: Some(vec![1]),
            })
        );
    }

    #[test]
    fn word_update_should_be_empty_for_equal_worlds() {
        let mut empty_world_update = WorldUpdate::default();
        let mut rng = SmallRng::seed_from_u64(42);
        let world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
        empty_world_update.before_frame = world.frame;
        empty_world_update.after_frame = world.frame;
        empty_world_update.time = world.time;
        assert_eq!(make_world_update(&world, &world), empty_world_update);
    }

    #[test]
    fn apply_word_update_should_make_worlds_equal() {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut world_before =
            generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
        let mut world_after = world_before.clone();
        world_after.frame += 1;
        world_after.time += 0.1;
        world_after.actors.remove(5);
        world_after.actors.remove(0);
        world_after.actors[4].position =
            Vec2f::new(rng.gen_range(-1e1..1e1), rng.gen_range(-1e1..1e1));
        world_after.actors[3].health *= 0.8;
        world_after.static_objects.remove(3);
        world_after.static_objects.remove(0);
        world_after.static_areas.remove(2);
        world_after.static_areas.remove(0);
        let world_update = make_world_update(&world_before, &world_after);
        apply_world_update(world_update, &mut world_before);
        assert_eq!(world_before, world_after);
    }

    #[test]
    fn serialized_default_player_control_size() {
        assert_eq!(
            bincode::serialize(&PlayerControl::default()).unwrap().len(),
            34
        );
    }
}
