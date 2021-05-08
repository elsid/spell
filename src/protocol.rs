use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::vec2::Vec2f;
use crate::world::{Element, World};

pub const HEARTBEAT_PERIOD: Duration = Duration::from_secs(1);

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
#[derive(Debug, Deserialize, Serialize)]
pub enum ServerMessageData {
    Settings { update_period: Duration },
    Error(String),
    GameUpdate(GameUpdate),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessageData {
    Join,
    Quit,
    Heartbeat,
    PlayerAction(PlayerAction),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize)]
pub enum GameUpdate {
    SetPlayerId(u64),
    World(World),
    GameOver,
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
        ServerMessageData::Settings { .. } => "Settings",
        ServerMessageData::Error(..) => "Error",
        ServerMessageData::GameUpdate(..) => "GameUpdate",
    }
}

pub fn get_client_message_data_type(value: &ClientMessageData) -> &'static str {
    match value {
        ClientMessageData::Join => "Join",
        ClientMessageData::Quit => "Quit",
        ClientMessageData::Heartbeat => "Heartbeat",
        ClientMessageData::PlayerAction(..) => "PlayerAction",
    }
}
