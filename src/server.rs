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
    get_client_message_data_type, ClientMessage, ClientMessageData, GameUpdate, PlayerAction,
    ServerMessage, ServerMessageData, HEARTBEAT_PERIOD,
};
use crate::world::World;

const MAX_SESSION_MESSAGES_PER_FRAME: u8 = 3;
const MAX_DELAYED_MESSAGES_PER_SESSION: usize = 10;

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
    receiver: Receiver<ServerMessage>,
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
    receiver: Receiver<ServerMessage>,
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
        while let Ok(mut server_message) = self.receiver.try_recv() {
            server_message.number = self.message_counter;
            self.message_counter += 1;
            if matches!(
                server_message.data,
                ServerMessageData::GameUpdate(GameUpdate::World(..))
            ) {
                if self.sessions.is_empty()
                    || self
                        .sessions
                        .iter()
                        .all(|v| !matches!(v.state, UdpSessionState::Established))
                {
                    continue;
                }
                let buffer = bincode::serialize(&server_message).unwrap();
                let compressed = compress_prepend_size(&buffer);
                for session in self.sessions.iter() {
                    if matches!(session.state, UdpSessionState::Established) {
                        if let Err(e) = self.socket.send_to(&compressed, session.peer).await {
                            warn!("Failed to send: {}", e);
                        }
                    }
                }
            } else if let Some(session) = self
                .sessions
                .iter_mut()
                .find(|v| v.session_id == server_message.session_id)
            {
                if matches!(
                    server_message.data,
                    ServerMessageData::GameUpdate(GameUpdate::SetPlayerId(..))
                ) {
                    session.state = UdpSessionState::Established;
                }
                let buffer = bincode::serialize(&server_message).unwrap();
                let compressed = compress_prepend_size(&buffer);
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
                        warn!(
                            "Ignore new session from {}, sessions: {}/{}",
                            peer,
                            self.sessions.len(),
                            self.settings.max_sessions
                        );
                        continue;
                    };
                let mut client_message: ClientMessage =
                    match bincode::deserialize(&self.recv_buffer[0..size]) {
                        Ok(v) => v,
                        Err(e) => {
                            debug!("Failed to deserialize client message: {}", e);
                            continue;
                        }
                    };
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
    sender: Sender<ServerMessage>,
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
    while !stop.load(Ordering::Acquire) {
        handle_delayed_messages(&mut sessions, &mut world);
        handle_new_messages(
            &settings,
            &sender,
            &receiver,
            &mut sessions,
            &mut rng,
            &mut world,
        );
        close_timed_outed_sessions(settings.session_timeout, &sender, &mut sessions);
        handle_dropped_messages(&mut sessions);
        remove_inactive_actors(&mut sessions, &mut world);
        engine.update(time_step, &mut world);
        sessions.retain(|v| v.active);
        update_actor_index(&world, &mut sessions);
        sender
            .send(ServerMessage {
                session_id: 0,
                number: 0,
                data: ServerMessageData::GameUpdate(GameUpdate::World(world.clone())),
            })
            .ok();
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
}

fn handle_delayed_messages(sessions: &mut [GameSession], world: &mut World) {
    for session in sessions.iter_mut() {
        session.messages_per_frame = 0;
        handle_delayed_session_messages(session, world);
    }
}

fn close_timed_outed_sessions(
    session_timeout: Duration,
    sender: &Sender<ServerMessage>,
    sessions: &mut [GameSession],
) {
    for session in sessions.iter_mut() {
        if session_timeout <= Instant::now() - session.last_message_time {
            warn!("Game session {} is timed out", session.session_id);
            session.active = false;
            sender
                .send(ServerMessage {
                    session_id: session.session_id,
                    number: 0,
                    data: ServerMessageData::Error(String::from("Session is timed out")),
                })
                .ok();
        }
    }
}

fn handle_delayed_session_messages(session: &mut GameSession, world: &mut World) {
    while session.messages_per_frame < MAX_SESSION_MESSAGES_PER_FRAME {
        if let Some(message) = session.delayed_messages.pop_front() {
            if !handle_delayed_session_message(message, session, world) {
                break;
            }
        } else {
            break;
        }
    }
}

fn handle_delayed_session_message(
    message: ClientMessage,
    session: &mut GameSession,
    world: &mut World,
) -> bool {
    if message.number <= session.last_message_number {
        return false;
    }
    handle_session_message(message, session, world);
    true
}

fn handle_new_messages<R: Rng + CryptoRng>(
    settings: &GameServerSettings,
    sender: &Sender<ServerMessage>,
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
            handle_new_session_message(message, session, world);
        } else if sessions.len() < settings.max_players {
            if let Some(session) = create_new_session(
                settings.update_period,
                sender,
                message,
                &sessions,
                world,
                rng,
            ) {
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
                .send(ServerMessage {
                    number: 0,
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

fn handle_new_session_message(
    message: ClientMessage,
    session: &mut GameSession,
    world: &mut World,
) {
    if message.number <= session.last_message_number {
        return;
    }
    if session.messages_per_frame < MAX_SESSION_MESSAGES_PER_FRAME {
        return handle_session_message(message, session, world);
    }
    if session.delayed_messages.len() >= MAX_DELAYED_MESSAGES_PER_SESSION {
        session.dropped_messages += 1;
        return;
    }
    session.delayed_messages.push_back(message);
}

fn handle_session_message(message: ClientMessage, session: &mut GameSession, world: &mut World) {
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
        ClientMessageData::PlayerAction(mut player_action) => {
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
        v => warn!(
            "Existing session {} invalid message data: {}",
            session.session_id,
            get_client_message_data_type(&v),
        ),
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
    sender: &Sender<ServerMessage>,
    message: ClientMessage,
    sessions: &[GameSession],
    world: &mut World,
    rng: &mut R,
) -> Option<GameSession> {
    match message.data {
        ClientMessageData::Join => {
            let session_id = if message.session_id == 0 {
                loop {
                    let session_id = rng.gen();
                    if sessions.iter().all(|v| v.session_id != session_id) {
                        break session_id;
                    }
                }
            } else {
                message.session_id
            };
            sender
                .send(ServerMessage {
                    session_id,
                    number: 0,
                    data: ServerMessageData::Settings { update_period },
                })
                .unwrap();
            let actor_id = add_player_actor(world, rng);
            sender
                .send(ServerMessage {
                    session_id,
                    number: 0,
                    data: ServerMessageData::GameUpdate(GameUpdate::SetPlayerId(actor_id)),
                })
                .unwrap();
            Some(GameSession {
                session_id,
                active: true,
                actor_id,
                actor_index: None,
                last_message_time: Instant::now(),
                last_message_number: message.number,
                messages_per_frame: 1,
                delayed_messages: VecDeque::with_capacity(MAX_DELAYED_MESSAGES_PER_SESSION),
                dropped_messages: 0,
            })
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

fn add_player_actor<R: Rng>(world: &mut World, rng: &mut R) -> u64 {
    let actor_id = get_next_id(&mut world.id_counter);
    world
        .actors
        .push(generate_player_actor(actor_id, &world.bounds, rng));
    actor_id
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
