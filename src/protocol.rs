use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::vec2::Vec2f;
use crate::world::{Element, World};

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

#[derive(Debug, Deserialize, Serialize)]
pub enum ServerMessageData {
    Settings {
        update_period: Duration,
    },
    Error(String),
    GameUpdate(GameUpdate),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessageData {
    Join,
    Quit,
    PlayerAction(PlayerAction),
}

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
}
