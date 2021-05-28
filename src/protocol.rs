use std::convert::TryInto;
use std::fmt::Formatter;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::vec2::Vec2f;
use crate::world::{
    Actor, Aura, Beam, BoundedArea, DelayedMagick, DynamicObject, Effect, Element, Field,
    StaticArea, StaticObject, TempArea, World,
};

pub const HEARTBEAT_PERIOD: Duration = Duration::from_secs(1);
pub const MIN_PLAYER_NAME_LEN: usize = 3;
pub const MAX_PLAYER_NAME_LEN: usize = 16;
pub const MAX_SERVER_MESSAGE_SIZE: usize = 65_507;
pub const MAX_CLIENT_MESSAGE_SIZE: usize = 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerMessage {
    pub session_id: u64,
    pub number: u64,
    pub data: ServerMessageData,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ClientMessage {
    pub session_id: u64,
    pub number: u64,
    pub data: ClientMessageData,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub enum ServerMessageData {
    NewPlayer {
        update_period: Duration,
        actor_id: u64,
    },
    Error(String),
    GameUpdate(GameUpdate),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessageData {
    Join(String),
    Quit,
    Heartbeat,
    PlayerUpdate(PlayerUpdate),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub enum GameUpdate {
    SetPlayerId(u64),
    WorldSnapshot(World),
    WorldUpdate(WorldUpdate),
    GameOver,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct WorldUpdate {
    pub before_revision: u64,
    pub after_revision: u64,
    pub time: f64,
    pub added_actors: Option<Vec<Actor>>,
    pub updated_actors: Option<Vec<ActorUpdate>>,
    pub removed_actors: Option<Vec<u64>>,
    pub added_dynamic_objects: Option<Vec<DynamicObject>>,
    pub updated_dynamic_objects: Option<Vec<DynamicObjectUpdate>>,
    pub removed_dynamic_objects: Option<Vec<u64>>,
    pub added_static_objects: Option<Vec<StaticObject>>,
    pub updated_static_objects: Option<Vec<StaticObjectUpdate>>,
    pub removed_static_objects: Option<Vec<u64>>,
    pub added_beams: Option<Vec<Beam>>,
    pub removed_beams: Option<Vec<u64>>,
    pub added_static_areas: Option<Vec<StaticArea>>,
    pub removed_static_areas: Option<Vec<u64>>,
    pub added_temp_areas: Option<Vec<TempArea>>,
    pub updated_temp_areas: Option<Vec<TempAreaUpdate>>,
    pub removed_temp_areas: Option<Vec<u64>>,
    pub added_bounded_areas: Option<Vec<BoundedArea>>,
    pub removed_bounded_areas: Option<Vec<u64>>,
    pub added_fields: Option<Vec<Field>>,
    pub removed_fields: Option<Vec<u64>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct ActorUpdate {
    pub id: u64,
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
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct DynamicObjectUpdate {
    pub id: u64,
    pub position: Option<Vec2f>,
    pub health: Option<f64>,
    pub effect: Option<Effect>,
    pub aura: Option<Aura>,
    pub velocity: Option<Vec2f>,
    pub dynamic_force: Option<Vec2f>,
    pub position_z: Option<f64>,
    pub velocity_z: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct StaticObjectUpdate {
    pub id: u64,
    pub health: Option<f64>,
    pub effect: Option<Effect>,
    pub aura: Option<Aura>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct TempAreaUpdate {
    pub id: u64,
    pub effect: Option<Effect>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PlayerUpdate {
    Action(PlayerAction),
    AckWorldRevision(u64),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum PlayerAction {
    Move(bool),
    SetTargetDirection(Vec2f),
    AddSpellElement(Element),
    StartDirectedMagick,
    CompleteDirectedMagick,
    SelfMagick,
    StartAreaOfEffectMagick,
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
        ClientMessageData::PlayerUpdate(..) => "PlayerUpdate",
    }
}

pub fn make_world_update(before: &World, after: &World) -> WorldUpdate {
    let actors = get_actors_difference(&before.actors, &after.actors);
    let dynamic_objects =
        get_dynamic_objects_difference(&before.dynamic_objects, &after.dynamic_objects);
    let static_objects =
        get_static_objects_difference(&before.static_objects, &after.static_objects);
    let beams = get_beams_difference(&before.beams, &after.beams);
    let static_areas = get_static_areas_difference(&before.static_areas, &after.static_areas);
    let temp_areas = get_temp_areas_difference(&before.temp_areas, &after.temp_areas);
    let bounded_areas = get_bounded_areas_difference(&before.bounded_areas, &after.bounded_areas);
    let fields = get_fields_difference(&before.fields, &after.fields);
    WorldUpdate {
        before_revision: before.revision,
        after_revision: after.revision,
        time: after.time,
        added_actors: actors.added,
        updated_actors: actors.updated,
        removed_actors: actors.removed,
        added_dynamic_objects: dynamic_objects.added,
        updated_dynamic_objects: dynamic_objects.updated,
        removed_dynamic_objects: dynamic_objects.removed,
        added_static_objects: static_objects.added,
        updated_static_objects: static_objects.updated,
        removed_static_objects: static_objects.removed,
        added_beams: beams.added,
        removed_beams: beams.removed,
        added_static_areas: static_areas.added,
        removed_static_areas: static_areas.removed,
        added_temp_areas: temp_areas.added,
        updated_temp_areas: temp_areas.updated,
        removed_temp_areas: temp_areas.removed,
        added_bounded_areas: bounded_areas.added,
        removed_bounded_areas: bounded_areas.removed,
        added_fields: fields.added,
        removed_fields: fields.removed,
    }
}

fn get_actors_difference(before: &[Actor], after: &[Actor]) -> Difference<Actor, ActorUpdate> {
    get_difference(before, after, |v| v.id, make_actor_update)
}

fn get_dynamic_objects_difference(
    before: &[DynamicObject],
    after: &[DynamicObject],
) -> Difference<DynamicObject, DynamicObjectUpdate> {
    get_difference(before, after, |v| v.id, make_dynamic_object_update)
}

fn get_static_objects_difference(
    before: &[StaticObject],
    after: &[StaticObject],
) -> Difference<StaticObject, StaticObjectUpdate> {
    get_difference(before, after, |v| v.id, make_static_object_update)
}

fn get_beams_difference(before: &[Beam], after: &[Beam]) -> ExistenceDifference<Beam> {
    get_existence_difference(before, after, |v| v.id)
}

fn get_static_areas_difference(
    before: &[StaticArea],
    after: &[StaticArea],
) -> ExistenceDifference<StaticArea> {
    get_existence_difference(before, after, |v| v.id)
}

fn get_temp_areas_difference(
    before: &[TempArea],
    after: &[TempArea],
) -> Difference<TempArea, TempAreaUpdate> {
    get_difference(before, after, |v| v.id, make_temp_area_update)
}

fn get_bounded_areas_difference(
    before: &[BoundedArea],
    after: &[BoundedArea],
) -> ExistenceDifference<BoundedArea> {
    get_existence_difference(before, after, |v| v.id)
}

fn get_fields_difference(before: &[Field], after: &[Field]) -> ExistenceDifference<Field> {
    get_existence_difference(before, after, |v| v.id)
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
    if d {
        r.id = a.id;
        Some(r)
    } else {
        None
    }
}

fn make_dynamic_object_update(b: &DynamicObject, a: &DynamicObject) -> Option<DynamicObjectUpdate> {
    let mut r = DynamicObjectUpdate::default();
    let mut d = false;
    d = clone_if_different(&b.position, &a.position, &mut r.position) || d;
    d = clone_if_different(&b.health, &a.health, &mut r.health) || d;
    d = clone_if_different(&b.effect, &a.effect, &mut r.effect) || d;
    d = clone_if_different(&b.aura, &a.aura, &mut r.aura) || d;
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

#[derive(Default, Debug, PartialEq)]
struct Difference<T: std::fmt::Debug + PartialEq, U: std::fmt::Debug + PartialEq> {
    added: Option<Vec<T>>,
    updated: Option<Vec<U>>,
    removed: Option<Vec<u64>>,
}

fn get_difference<T, U, GetId, MakeUpdate>(
    before: &[T],
    after: &[T],
    get_id: GetId,
    make_update: MakeUpdate,
) -> Difference<T, U>
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
    Difference {
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
    }
}

#[derive(Debug, PartialEq)]
struct ExistenceDifference<T: std::fmt::Debug + PartialEq> {
    added: Option<Vec<T>>,
    removed: Option<Vec<u64>>,
}

fn get_existence_difference<T, GetId>(
    before: &[T],
    after: &[T],
    get_id: GetId,
) -> ExistenceDifference<T>
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
    ExistenceDifference {
        added: if added.is_empty() { None } else { Some(added) },
        removed: if removed.is_empty() {
            None
        } else {
            Some(removed)
        },
    }
}

pub fn apply_world_update(update: WorldUpdate, world: &mut World) {
    if update.after_revision < world.revision {
        return;
    }
    world.revision = update.after_revision;
    world.time = update.time;
    remove_from(update.removed_actors, |v| v.id, &mut world.actors);
    remove_from(
        update.removed_dynamic_objects,
        |v| v.id,
        &mut world.dynamic_objects,
    );
    remove_from(
        update.removed_static_objects,
        |v| v.id,
        &mut world.static_objects,
    );
    remove_from(update.removed_beams, |v| v.id, &mut world.beams);
    remove_from(
        update.removed_static_areas,
        |v| v.id,
        &mut world.static_areas,
    );
    remove_from(update.removed_temp_areas, |v| v.id, &mut world.temp_areas);
    remove_from(
        update.removed_bounded_areas,
        |v| v.id,
        &mut world.bounded_areas,
    );
    remove_from(update.removed_fields, |v| v.id, &mut world.fields);
    update_from(
        update.updated_actors,
        |a, b| a.id == b.id,
        apply_actor_update,
        &mut world.actors,
    );
    update_from(
        update.updated_dynamic_objects,
        |a, b| a.id == b.id,
        apply_dynamic_object_update,
        &mut world.dynamic_objects,
    );
    update_from(
        update.updated_static_objects,
        |a, b| a.id == b.id,
        apply_static_object_update,
        &mut world.static_objects,
    );
    update_from(
        update.updated_temp_areas,
        |a, b| a.id == b.id,
        apply_temp_area_update,
        &mut world.temp_areas,
    );
    add_or_update_from(update.added_actors, |v| v.id, &mut world.actors);
    add_or_update_from(
        update.added_dynamic_objects,
        |v| v.id,
        &mut world.dynamic_objects,
    );
    add_or_update_from(
        update.added_static_objects,
        |v| v.id,
        &mut world.static_objects,
    );
    add_or_update_from(update.added_beams, |v| v.id, &mut world.beams);
    add_or_update_from(update.added_static_areas, |v| v.id, &mut world.static_areas);
    add_or_update_from(update.added_temp_areas, |v| v.id, &mut world.temp_areas);
    add_or_update_from(
        update.added_bounded_areas,
        |v| v.id,
        &mut world.bounded_areas,
    );
    add_or_update_from(update.added_fields, |v| v.id, &mut world.fields);
}

fn add_or_update_from<GetId, T>(src: Option<Vec<T>>, get_id: GetId, dst: &mut Vec<T>)
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
    equal_by_id: EqualById,
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

fn remove_from<GetId, T>(src: Option<Vec<u64>>, get_id: GetId, dst: &mut Vec<T>)
where
    GetId: Fn(&T) -> u64,
{
    if let Some(removed) = src {
        dst.retain(|v| !removed.contains(&get_id(v)));
    }
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
}

fn apply_dynamic_object_update(src: &DynamicObjectUpdate, dst: &mut DynamicObject) {
    clone_if_some(&src.position, &mut dst.position);
    clone_if_some(&src.health, &mut dst.health);
    clone_if_some(&src.effect, &mut dst.effect);
    clone_if_some(&src.aura, &mut dst.aura);
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

fn clone_if_some<T: Clone>(src: &Option<T>, dst: &mut T) {
    if let Some(value) = src.as_ref() {
        *dst = value.clone();
    }
}

pub fn is_valid_player_name(value: &str) -> bool {
    MIN_PLAYER_NAME_LEN <= value.len()
        && value.len() <= MAX_PLAYER_NAME_LEN
        && value.chars().all(|v| v.is_alphabetic())
}

#[derive(Debug)]
pub enum DeserializeError {
    CompressedMessageTooLong(usize),
    InvalidCompressedMessageFormat,
    DeclaredDecompressedMessageTooLong(usize),
    DecompressError(lz4_flex::block::DecompressError),
    DecompressedMessageTooLong(usize),
    DeserializeError(bincode::Error),
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            DeserializeError::CompressedMessageTooLong(v) => {
                write!(f, "Compressed message is too long: {} bytes", v)
            }
            DeserializeError::InvalidCompressedMessageFormat => {
                write!(f, "Invalid compressed message format")
            }
            DeserializeError::DeclaredDecompressedMessageTooLong(v) => {
                write!(f, "Declared decompressed message is too long: {} bytes", v)
            }
            DeserializeError::DecompressError(e) => write!(f, "{}", e),
            DeserializeError::DecompressedMessageTooLong(v) => {
                write!(f, "Decompressed message is tool long: {} bytes", v)
            }
            DeserializeError::DeserializeError(e) => write!(f, "{}", e),
        }
    }
}

pub fn deserialize_client_message(input: &[u8]) -> Result<ClientMessage, DeserializeError> {
    if input.len() > MAX_CLIENT_MESSAGE_SIZE {
        return Err(DeserializeError::DecompressedMessageTooLong(input.len()));
    }
    match bincode::deserialize(input) {
        Ok(v) => Ok(v),
        Err(e) => Err(DeserializeError::DeserializeError(e)),
    }
}

pub fn deserialize_server_message(input: &[u8]) -> Result<ServerMessage, DeserializeError> {
    let decompressed = decompress_message(input, MAX_SERVER_MESSAGE_SIZE)?;
    match bincode::deserialize(&decompressed) {
        Ok(v) => Ok(v),
        Err(e) => Err(DeserializeError::DeserializeError(e)),
    }
}

fn decompress_message(input: &[u8], max_size: usize) -> Result<Vec<u8>, DeserializeError> {
    if input.len() > max_size {
        return Err(DeserializeError::CompressedMessageTooLong(input.len()));
    }
    let decompressed_size = decompressed_size(input)?;
    if decompressed_size > max_size {
        return Err(DeserializeError::DeclaredDecompressedMessageTooLong(
            decompressed_size,
        ));
    }
    let decompressed = match lz4_flex::decompress_size_prepended(input) {
        Ok(v) => v,
        Err(e) => return Err(DeserializeError::DecompressError(e)),
    };
    if decompressed.len() > max_size {
        return Err(DeserializeError::DecompressedMessageTooLong(
            decompressed.len(),
        ));
    }
    Ok(decompressed)
}

fn decompressed_size(input: &[u8]) -> Result<usize, DeserializeError> {
    let size = input
        .get(..4)
        .ok_or(DeserializeError::InvalidCompressedMessageFormat)?;
    let size: &[u8; 4] = size.try_into().unwrap();
    Ok(u32::from_le_bytes(*size) as usize)
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
            44
        );
    }

    #[test]
    fn serialized_default_actor_update_size() {
        assert_eq!(
            bincode::serialize(&ActorUpdate::default()).unwrap().len(),
            21
        );
    }

    #[test]
    fn serialized_default_dynamic_object_update_size() {
        assert_eq!(
            bincode::serialize(&DynamicObjectUpdate::default())
                .unwrap()
                .len(),
            16
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
            Difference::default()
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
            Difference {
                added: Some(vec![TestObject { id: 2, value: 2.7 }]),
                updated: None,
                removed: None,
            }
        );
    }

    #[test]
    fn get_difference_should_return_update_when_value_has_changed() {
        let before = vec![TestObject { id: 1, value: 3.15 }];
        let after = vec![TestObject { id: 1, value: 2.7 }];
        assert_eq!(
            get_difference(&before, &after, |v| v.id, make_test_object_update),
            Difference {
                added: None,
                updated: Some(vec![TestObjectUpdate {
                    id: 1,
                    value: Some(2.7),
                }]),
                removed: None,
            }
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
            Difference {
                added: None,
                updated: None,
                removed: Some(vec![1]),
            }
        );
    }

    #[test]
    fn word_update_should_be_empty_for_equal_worlds() {
        let mut empty_world_update = WorldUpdate::default();
        let mut rng = SmallRng::seed_from_u64(42);
        let world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
        empty_world_update.before_revision = world.revision;
        empty_world_update.after_revision = world.revision;
        empty_world_update.time = world.time;
        assert_eq!(make_world_update(&world, &world), empty_world_update);
    }

    #[test]
    fn apply_word_update_should_make_worlds_equal() {
        let mut rng = SmallRng::seed_from_u64(42);
        let mut world_before =
            generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
        let mut world_after = world_before.clone();
        world_after.revision += 1;
        world_after.time += 0.1;
        world_after.actors.remove(5);
        world_after.actors.remove(0);
        world_after.actors[4].position =
            Vec2f::new(rng.gen_range(-1e1..1e1), rng.gen_range(-1e1..1e1));
        world_after.actors[3].health *= 0.8;
        world_after.dynamic_objects.remove(4);
        world_after.dynamic_objects.remove(0);
        world_after.static_objects.remove(3);
        world_after.static_objects.remove(0);
        world_after.static_areas.remove(2);
        world_after.static_areas.remove(0);
        let world_update = make_world_update(&world_before, &world_after);
        apply_world_update(world_update, &mut world_before);
        assert_eq!(world_before, world_after);
    }
}
