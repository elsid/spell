use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use lz4_flex::compress_prepend_size;
use rand::rngs::StdRng;
use rand::{CryptoRng, Rng, SeedableRng};
use tokio::net::UdpSocket;

use crate::control::apply_player_action;
use crate::engine::{get_next_id, remove_actor, Engine};
use crate::generators::generate_player_actor;
use crate::protocol::{
    deserialize_client_message, get_client_message_data_type, is_valid_player_name,
    make_world_update, ClientMessage, ClientMessageData, GameUpdate, PlayerAction, PlayerUpdate,
    ServerMessage, ServerMessageData, WorldUpdate, HEARTBEAT_PERIOD,
};
use crate::world::World;

const MAX_SESSION_MESSAGES_PER_FRAME: u8 = 3;
const MAX_DELAYED_MESSAGES_PER_SESSION: usize = 10;
const MAX_WORLD_HISTORY_SIZE: usize = 120;

pub enum InternalServerMessage {
    Unicast {
        session_id: u64,
        data: ServerMessageData,
    },
    Multicast {
        session_ids: Vec<u64>,
        data: ServerMessageData,
    },
    Broadcast(ServerMessageData),
}

#[derive(Debug)]
pub struct UdpServerSettings {
    pub address: String,
    pub max_sessions: usize,
    pub update_period: Duration,
    pub session_timeout: Duration,
}

pub async fn run_udp_server(
    settings: UdpServerSettings,
    sender: Sender<ClientMessage>,
    receiver: Receiver<InternalServerMessage>,
    stop: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    info!("Run UDP server: {:?}", settings);
    if settings.session_timeout < HEARTBEAT_PERIOD {
        warn!(
            "UDP server session timeout {:?} is less than heartbeat period {:?}",
            settings.session_timeout, HEARTBEAT_PERIOD
        );
    }
    UdpServer {
        socket: UdpSocket::bind(&settings.address).await?,
        stop,
        settings,
        sender,
        receiver,
        recv_buffer: vec![0u8; 65_507],
        sessions: Vec::new(),
        rng: StdRng::from_entropy(),
        message_counter: 0,
    }
    .run()
    .await;
    info!("UDP server has stopped");
    Ok(())
}

struct UdpServer {
    stop: Arc<AtomicBool>,
    settings: UdpServerSettings,
    sender: Sender<ClientMessage>,
    receiver: Receiver<InternalServerMessage>,
    socket: UdpSocket,
    recv_buffer: Vec<u8>,
    sessions: Vec<UdpSession>,
    rng: StdRng,
    message_counter: u64,
}

struct UdpSession {
    peer: SocketAddr,
    session_id: u64,
    last_recv_time: Instant,
    state: UdpSessionState,
}

enum UdpSessionState {
    New,
    Established,
    Done,
}

impl UdpServer {
    async fn run(&mut self) {
        info!(
            "UDP server is listening on {}",
            self.socket.local_addr().unwrap()
        );
        let mut last_update = Instant::now();
        while !self.stop.load(Ordering::Acquire) {
            self.clean_udp_sessions();
            self.handle_game_messages().await;
            self.receive_messages(last_update).await;
            last_update = Instant::now();
        }
    }

    fn clean_udp_sessions(&mut self) {
        let now = Instant::now();
        let session_timeout = self.settings.session_timeout;
        for session in self.sessions.iter() {
            if session_timeout <= now - session.last_recv_time {
                self.sender
                    .send(ClientMessage {
                        session_id: session.session_id,
                        number: u64::MAX,
                        data: ClientMessageData::Quit,
                    })
                    .ok();
                warn!("UDP session {} is timed out", session.session_id);
            }
        }
        self.sessions.retain(|v| {
            !matches!(v.state, UdpSessionState::Done) && now - v.last_recv_time < session_timeout
        });
    }

    async fn handle_game_messages(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            self.message_counter += 1;
            match message {
                InternalServerMessage::Unicast { session_id, data } => {
                    self.send_server_message_for_session(&ServerMessage {
                        session_id,
                        number: self.message_counter,
                        data,
                    })
                    .await;
                }
                InternalServerMessage::Multicast { session_ids, data } => {
                    let mut server_message = ServerMessage {
                        session_id: 0,
                        number: self.message_counter,
                        data,
                    };
                    for session_id in session_ids {
                        server_message.session_id = session_id;
                        self.send_server_message_for_session(&server_message).await;
                    }
                }
                InternalServerMessage::Broadcast(data) => {
                    self.send_broadcast_server_message(&ServerMessage {
                        session_id: 0,
                        number: self.message_counter,
                        data,
                    })
                    .await;
                }
            }
        }
    }

    async fn send_server_message_for_session(&mut self, server_message: &ServerMessage) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|v| v.session_id == server_message.session_id)
        {
            if matches!(server_message.data, ServerMessageData::NewPlayer { .. }) {
                session.state = UdpSessionState::Established;
            }
            let buffer = bincode::serialize(&server_message).unwrap();
            let compressed = compress_prepend_size(&buffer);
            if let Err(e) = self.socket.send_to(&compressed, session.peer).await {
                warn!("Failed to send: {}", e);
            }
        }
    }

    async fn send_broadcast_server_message(&mut self, server_message: &ServerMessage) {
        if self.sessions.is_empty()
            || self
                .sessions
                .iter()
                .all(|v| !matches!(v.state, UdpSessionState::Established))
        {
            return;
        }
        let buffer = bincode::serialize(server_message).unwrap();
        let compressed = compress_prepend_size(&buffer);
        for session in self.sessions.iter() {
            if matches!(session.state, UdpSessionState::Established) {
                if let Err(e) = self.socket.send_to(&compressed, session.peer).await {
                    warn!("Failed to send: {}", e);
                }
            }
        }
    }

    async fn receive_messages(&mut self, last_update: Instant) {
        let mut now = Instant::now();
        loop {
            let recv_timeout = if now - last_update < self.settings.update_period {
                self.settings.update_period - (now - last_update)
            } else {
                Duration::from_millis(1)
            };
            if let Ok(Ok((size, peer))) =
                tokio::time::timeout(recv_timeout, self.socket.recv_from(&mut self.recv_buffer))
                    .await
            {
                let session_id =
                    if let Some(session) = self.sessions.iter_mut().find(|v| v.peer == peer) {
                        session.last_recv_time = Instant::now();
                        session.session_id
                    } else if self.sessions.len() < self.settings.max_sessions {
                        loop {
                            let session_id = self.rng.gen();
                            if self.sessions.iter_mut().any(|v| v.session_id == session_id) {
                                continue;
                            }
                            info!("New UDP session {} from {}", session_id, peer);
                            self.sessions.push(UdpSession {
                                session_id,
                                peer,
                                last_recv_time: Instant::now(),
                                state: UdpSessionState::New,
                            });
                            break session_id;
                        }
                    } else {
                        debug!(
                            "Ignore new session from {}, sessions: {}/{}",
                            peer,
                            self.sessions.len(),
                            self.settings.max_sessions
                        );
                        continue;
                    };
                let mut client_message =
                    match deserialize_client_message(&self.recv_buffer[0..size]) {
                        Ok(v) => v,
                        Err(e) => {
                            debug!("Failed to deserialize client message: {}", e);
                            continue;
                        }
                    };
                if client_message.session_id != session_id
                    && !(matches!(client_message.data, ClientMessageData::Join(..))
                        && client_message.session_id == 0)
                {
                    debug!("Server has received client message {} with invalid session_id: {}, expected: {}",
                           get_client_message_data_type(&client_message.data), client_message.session_id, session_id);
                    continue;
                }
                client_message.session_id = session_id;
                if matches!(&client_message.data, ClientMessageData::Quit) {
                    let session = self
                        .sessions
                        .iter_mut()
                        .find(|v| v.session_id == session_id)
                        .unwrap();
                    session.state = UdpSessionState::Done;
                    info!("UDP session {} is done", session.session_id);
                }
                self.sender.send(client_message).ok();
            }
            now = Instant::now();
            if self.settings.update_period <= now - last_update {
                break;
            }
        }
    }
}

#[derive(Debug)]
pub struct GameServerSettings {
    pub max_players: usize,
    pub update_period: Duration,
    pub session_timeout: Duration,
}

pub fn run_game_server(
    mut world: World,
    settings: GameServerSettings,
    sender: Sender<InternalServerMessage>,
    receiver: Receiver<ClientMessage>,
    stop: Arc<AtomicBool>,
) {
    info!("Run game server: {:?}", settings);
    if settings.session_timeout < HEARTBEAT_PERIOD {
        warn!(
            "Game server session timeout {:?} is less than heartbeat period {:?}",
            settings.session_timeout, HEARTBEAT_PERIOD
        );
    }
    let time_step = settings.update_period.as_secs_f64();
    let mut frame_rate_limiter = FrameRateLimiter::new(settings.update_period, Instant::now());
    let mut sessions: Vec<GameSession> = Vec::new();
    let mut rng = StdRng::from_entropy();
    let mut engine = Engine::default();
    let mut world_history = VecDeque::with_capacity(MAX_WORLD_HISTORY_SIZE);
    world_history.push_back(world.clone());
    sender
        .send(InternalServerMessage::Broadcast(
            ServerMessageData::GameUpdate(GameUpdate::WorldSnapshot(world.clone())),
        ))
        .ok();
    while !stop.load(Ordering::Acquire) {
        handle_delayed_messages(&settings, &sender, &mut sessions, &mut world);
        handle_new_messages(
            &settings,
            &sender,
            &receiver,
            &mut sessions,
            &mut rng,
            &mut world,
        );
        close_timed_out_sessions(settings.session_timeout, &sender, &mut sessions);
        handle_dropped_messages(&mut sessions);
        remove_inactive_actors(&mut sessions, &mut world);
        engine.update(time_step, &mut world);
        sessions.retain(|v| v.active);
        update_actor_index(&world, &mut sessions);
        if world_history.len() >= MAX_WORLD_HISTORY_SIZE {
            world_history.pop_front();
        }
        send_world_messages(&sender, &world, &world_history, &sessions);
        world_history.push_back(world.clone());
        frame_rate_limiter.limit(Instant::now());
    }
    info!("Game server has stopped");
}

#[derive(Debug)]
struct GameSession {
    session_id: u64,
    active: bool,
    actor_id: u64,
    actor_index: Option<usize>,
    last_message_time: Instant,
    last_message_number: u64,
    messages_per_frame: u8,
    dropped_messages: usize,
    delayed_messages: VecDeque<ClientMessage>,
    ack_world_revision: u64,
}

fn handle_delayed_messages(
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    sessions: &mut [GameSession],
    world: &mut World,
) {
    for session in sessions.iter_mut() {
        session.messages_per_frame = 0;
        handle_session_delayed_messages(settings, sender, session, world);
    }
}

fn close_timed_out_sessions(
    session_timeout: Duration,
    sender: &Sender<InternalServerMessage>,
    sessions: &mut [GameSession],
) {
    for session in sessions.iter_mut() {
        if session_timeout <= Instant::now() - session.last_message_time {
            warn!("Game session {} is timed out", session.session_id);
            session.active = false;
            sender
                .send(InternalServerMessage::Unicast {
                    session_id: session.session_id,
                    data: ServerMessageData::Error(String::from("Session is timed out")),
                })
                .ok();
        }
    }
}

fn handle_session_delayed_messages(
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    session: &mut GameSession,
    world: &mut World,
) {
    while session.messages_per_frame < MAX_SESSION_MESSAGES_PER_FRAME {
        if let Some(message) = session.delayed_messages.pop_front() {
            if !handle_session_delayed_message(message, settings, sender, session, world) {
                break;
            }
        } else {
            break;
        }
    }
}

fn handle_session_delayed_message(
    message: ClientMessage,
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    session: &mut GameSession,
    world: &mut World,
) -> bool {
    if message.number <= session.last_message_number {
        return false;
    }
    handle_session_message(message, settings, sender, session, world);
    true
}

fn handle_new_messages<R: Rng + CryptoRng>(
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    receiver: &Receiver<ClientMessage>,
    sessions: &mut Vec<GameSession>,
    rng: &mut R,
    world: &mut World,
) {
    let mut messages_per_frame: usize = 0;
    while let Ok(message) = receiver.try_recv() {
        if let Some(session) = sessions
            .iter_mut()
            .find(|v| v.session_id == message.session_id)
        {
            handle_session_new_message(message, settings, sender, session, world);
        } else if sessions.len() < settings.max_players {
            if let Some(session) =
                create_new_session(settings.update_period, sender, message, world, rng)
            {
                info!(
                    "New player has joined: session_id={} actor_id={}",
                    session.session_id, session.actor_id
                );
                sessions.push(session);
            }
        } else {
            warn!(
                "Rejected new player, server players: {}/{}",
                sessions.len(),
                settings.max_players
            );
            sender
                .send(InternalServerMessage::Unicast {
                    session_id: message.session_id,
                    data: ServerMessageData::Error(String::from("Server is full")),
                })
                .unwrap();
        }
        messages_per_frame += 1;
        if messages_per_frame > sessions.len() + settings.max_players {
            break;
        }
    }
}

fn handle_session_new_message(
    message: ClientMessage,
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    session: &mut GameSession,
    world: &mut World,
) {
    if message.number <= session.last_message_number {
        return;
    }
    if session.messages_per_frame < MAX_SESSION_MESSAGES_PER_FRAME {
        return handle_session_message(message, settings, sender, session, world);
    }
    if session.delayed_messages.len() >= MAX_DELAYED_MESSAGES_PER_SESSION {
        session.dropped_messages += 1;
        return;
    }
    session.delayed_messages.push_back(message);
}

fn handle_session_message(
    message: ClientMessage,
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    session: &mut GameSession,
    world: &mut World,
) {
    session.last_message_time = Instant::now();
    session.last_message_number = message.number;
    session.messages_per_frame += 1;
    match message.data {
        ClientMessageData::Quit => {
            if let Some(actor_index) = session.actor_index {
                remove_actor(actor_index, world);
                session.actor_index = None;
            }
            session.active = false;
            info!("Game session {} is done", session.session_id);
        }
        ClientMessageData::Heartbeat => (),
        ClientMessageData::Join(..) => sender
            .send(InternalServerMessage::Unicast {
                session_id: session.session_id,
                data: ServerMessageData::NewPlayer {
                    update_period: settings.update_period,
                    actor_id: session.actor_id,
                },
            })
            .unwrap(),
        ClientMessageData::PlayerUpdate(player_update) => match player_update {
            PlayerUpdate::Action(mut player_action) => {
                if let Some(actor_index) = session.actor_index {
                    sanitize_player_action(&mut player_action, actor_index, world);
                    apply_player_action(&player_action, actor_index, world);
                } else {
                    warn!(
                        "Player actor is not found for game session: {}",
                        session.session_id
                    );
                }
            }
            PlayerUpdate::AckWorldRevision(revision) => {
                session.ack_world_revision = revision.min(world.revision);
            }
        },
    }
}

fn handle_dropped_messages(sessions: &mut [GameSession]) {
    for session in sessions.iter_mut() {
        if session.dropped_messages > 0 {
            warn!(
                "Dropped {} messages for game session {}",
                session.dropped_messages, session.session_id
            );
            session.dropped_messages = 0;
        }
    }
}

fn remove_inactive_actors(sessions: &mut [GameSession], world: &mut World) {
    for session in sessions.iter_mut() {
        if !session.active {
            if let Some(actor_index) = session.actor_index {
                remove_actor(actor_index, world);
                session.actor_index = None;
            }
        }
    }
}

fn update_actor_index(world: &World, sessions: &mut [GameSession]) {
    for session in sessions.iter_mut() {
        session.actor_index = world.actors.iter().position(|v| v.id == session.actor_id);
    }
}

fn create_new_session<R: CryptoRng + Rng>(
    update_period: Duration,
    sender: &Sender<InternalServerMessage>,
    message: ClientMessage,
    world: &mut World,
    rng: &mut R,
) -> Option<GameSession> {
    match message.data {
        ClientMessageData::Join(name) => {
            if !is_valid_player_name(name.as_str()) {
                sender
                    .send(InternalServerMessage::Unicast {
                        session_id: message.session_id,
                        data: ServerMessageData::Error(String::from("Invalid player name")),
                    })
                    .unwrap();
                return None;
            }
            if let Some(actor_id) = try_add_player_actor(name, world, rng) {
                sender
                    .send(InternalServerMessage::Unicast {
                        session_id: message.session_id,
                        data: ServerMessageData::NewPlayer {
                            update_period,
                            actor_id,
                        },
                    })
                    .unwrap();
                Some(GameSession {
                    session_id: message.session_id,
                    active: true,
                    actor_id,
                    actor_index: None,
                    last_message_time: Instant::now(),
                    last_message_number: message.number,
                    messages_per_frame: 1,
                    delayed_messages: VecDeque::with_capacity(MAX_DELAYED_MESSAGES_PER_SESSION),
                    dropped_messages: 0,
                    ack_world_revision: 0,
                })
            } else {
                sender
                    .send(InternalServerMessage::Unicast {
                        session_id: message.session_id,
                        data: ServerMessageData::Error(String::from("Player name is busy")),
                    })
                    .unwrap();
                None
            }
        }
        ClientMessageData::Quit => None,
        v => {
            warn!(
                "New session invalid message type: {}",
                get_client_message_data_type(&v)
            );
            None
        }
    }
}

struct FrameRateLimiter {
    max_frame_duration: Duration,
    last_measurement: Instant,
}

impl FrameRateLimiter {
    fn new(max_frame_duration: Duration, now: Instant) -> Self {
        Self {
            max_frame_duration,
            last_measurement: now,
        }
    }

    fn limit(&mut self, now: Instant) {
        let passed = now - self.last_measurement;
        if passed < self.max_frame_duration {
            sleep(self.max_frame_duration - passed);
            self.last_measurement += self.max_frame_duration;
        } else {
            self.last_measurement = now;
        }
    }
}

fn try_add_player_actor<R: Rng>(name: String, world: &mut World, rng: &mut R) -> Option<u64> {
    if world.actors.iter().any(|v| v.name == name) {
        return None;
    }
    let actor_id = get_next_id(&mut world.id_counter);
    world
        .actors
        .push(generate_player_actor(actor_id, &world.bounds, name, rng));
    Some(actor_id)
}

fn sanitize_player_action(player_action: &mut PlayerAction, actor_index: usize, world: &mut World) {
    #[allow(clippy::single_match)]
    match player_action {
        PlayerAction::SetTargetDirection(target_direction) => {
            let norm = target_direction.norm();
            if norm > f64::EPSILON {
                *target_direction /= norm;
            } else {
                *target_direction = world.actors[actor_index].target_direction;
            }
        }
        _ => (),
    }
}

fn send_world_messages(
    sender: &Sender<InternalServerMessage>,
    world: &World,
    world_history: &VecDeque<World>,
    sessions: &[GameSession],
) {
    let mut world_snapshot_session_ids = Vec::new();
    let mut world_updates: Vec<(usize, Vec<u64>, WorldUpdate)> = Vec::new();
    for session in sessions.iter() {
        if session.ack_world_revision == 0 {
            world_snapshot_session_ids.push(session.session_id);
            continue;
        }
        let offset = (world.revision - session.ack_world_revision) as usize;
        if offset > world_history.len() {
            world_snapshot_session_ids.push(session.session_id);
            continue;
        }
        if let Some((_, session_ids, _)) = world_updates.iter_mut().find(|(v, _, _)| *v == offset) {
            session_ids.push(session.session_id);
            continue;
        }
        world_updates.push((
            offset,
            vec![session.session_id],
            make_world_update(&world_history[world_history.len() - offset], &world),
        ));
    }
    for (_, session_ids, world_update) in world_updates {
        sender
            .send(InternalServerMessage::Multicast {
                session_ids,
                data: ServerMessageData::GameUpdate(GameUpdate::WorldUpdate(world_update.clone())),
            })
            .ok();
    }
    if !world_snapshot_session_ids.is_empty() {
        sender
            .send(InternalServerMessage::Multicast {
                session_ids: world_snapshot_session_ids,
                data: ServerMessageData::GameUpdate(GameUpdate::WorldSnapshot(world.clone())),
            })
            .ok();
    }
}
