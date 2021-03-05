use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::sleep;
use std::time::{Duration, Instant};

use lz4_flex::compress_prepend_size;
use rand::{CryptoRng, Rng, SeedableRng};
use rand::rngs::StdRng;
use tokio::net::UdpSocket;

use crate::engine::{add_actor_spell_element, complete_directed_magick, Engine, get_next_id, remove_actor, self_magick, start_directed_magick};
use crate::generators::generate_player_actor;
use crate::protocol::{ClientMessage, ClientMessageData, GameUpdate, PlayerAction, ServerMessage, ServerMessageData};
use crate::world::World;

#[derive(Debug)]
pub struct UdpServerSettings {
    pub address: String,
    pub max_sessions: usize,
    pub update_period: Duration,
    pub session_timeout: Duration,
}

pub async fn run_udp_server(settings: UdpServerSettings, sender: Sender<ClientMessage>,
                            receiver: Receiver<ServerMessage>, stop: Arc<AtomicBool>) -> Result<(), std::io::Error> {
    info!("Run UDP server: {:?}", settings);
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
    }.run().await;
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
        info!("Listening on {}", self.socket.local_addr().unwrap());
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
                self.sender.send(ClientMessage {
                    session_id: session.session_id,
                    number: u64::MAX,
                    data: ClientMessageData::Quit,
                }).ok();
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
            if matches!(server_message.data, ServerMessageData::GameUpdate(GameUpdate::World(..))) {
                if self.sessions.is_empty() || self.sessions.iter().all(|v| !matches!(v.state, UdpSessionState::Established)) {
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
            } else {
                if let Some(session) = self.sessions.iter_mut().find(|v| v.session_id == server_message.session_id) {
                    if matches!(server_message.data, ServerMessageData::GameUpdate(GameUpdate::SetPlayerId(..))) {
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
    }

    async fn receive_messages(&mut self, last_update: Instant) {
        let mut now = Instant::now();
        loop {
            let recv_timeout = if now - last_update < self.settings.update_period {
                self.settings.update_period - (now - last_update)
            } else {
                Duration::from_millis(1)
            };
            if let Ok(Ok((size, peer))) = tokio::time::timeout(recv_timeout, self.socket.recv_from(&mut self.recv_buffer)).await {
                let session_id = if let Some(session) = self.sessions.iter_mut().find(|v| v.peer == peer) {
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
                    continue;
                };
                let mut client_message: ClientMessage = match bincode::deserialize(&self.recv_buffer[0..size]) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("Failed to deserialize client message: {}", e);
                        continue;
                    }
                };
                client_message.session_id = session_id;
                if matches!(&client_message.data, ClientMessageData::Quit) {
                    let session = self.sessions.iter_mut().find(|v| v.session_id == session_id).unwrap();
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

pub fn run_game_server(mut world: World, settings: GameServerSettings, sender: Sender<ServerMessage>, receiver: Receiver<ClientMessage>, stop: Arc<AtomicBool>) {
    info!("Run game server: {:?}", settings);
    let time_step = settings.update_period.as_secs_f64();
    let mut frame_rate_limiter = FrameRateLimiter::new(settings.update_period, Instant::now());
    let mut sessions: Vec<GameSession> = Vec::new();
    let mut rng = StdRng::from_entropy();
    let mut engine = Engine::default();
    let mut messages_per_frame: usize = 0;
    while !stop.load(Ordering::Acquire) {
        for session in sessions.iter_mut() {
            session.messages_per_frame = 0;
            while let Some(message) = session.delayed_messages.pop_front() {
                handle_existing_session(message, session, &mut world);
                session.messages_per_frame += 1;
                if session.messages_per_frame > 3 {
                    break;
                }
            }
            if settings.session_timeout <= Instant::now() - session.last_message_time {
                warn!("Game session {} is timed out", session.session_id);
                session.active = false;
                sender.send(ServerMessage {
                    session_id: session.session_id,
                    number: 0,
                    data: ServerMessageData::Error(String::from("Session is timed out")),
                }).ok();
            }
        }
        while let Ok(message) = receiver.try_recv() {
            if let Some(session) = sessions.iter_mut()
                .find(|v| v.session_id == message.session_id) {
                handle_existing_session(message, session, &mut world);
            } else if sessions.len() < settings.max_players {
                if let Some(session) = create_new_session(settings.update_period, &sender, message, &sessions, &mut world, &mut rng) {
                    info!("New player has joined: session_id={} actor_id={}", session.session_id, session.actor_id);
                    sessions.push(session);
                }
            } else {
                warn!("Rejected new player, server players: {}/{}", sessions.len(), settings.max_players);
                sender.send(ServerMessage {
                    number: 0,
                    session_id: 0,
                    data: ServerMessageData::Error(String::from("Server is full")),
                }).ok();
            }
            messages_per_frame += 1;
            if messages_per_frame > sessions.len() + settings.max_players {
                break;
            }
        }
        engine.update(time_step, &mut world);
        sessions.retain(|v| v.active);
        for session in sessions.iter_mut() {
            session.actor_index = world.actors.iter().position(|v| v.id == session.actor_id);
        }
        messages_per_frame = 0;
        sender.send(ServerMessage {
            session_id: 0,
            number: 0,
            data: ServerMessageData::GameUpdate(GameUpdate::World(world.clone())),
        }).ok();
        frame_rate_limiter.limit(Instant::now());
    }
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
    delayed_messages: VecDeque<ClientMessage>,
}

fn handle_existing_session(message: ClientMessage, session: &mut GameSession, world: &mut World) {
    if message.number <= session.last_message_number {
        return;
    }
    if session.messages_per_frame > 3 {
        session.delayed_messages.push_back(message);
        return;
    }
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
        ClientMessageData::PlayerAction(player_action) => {
            if let Some(actor_index) = session.actor_index {
                match player_action {
                    PlayerAction::Move(moving) => {
                        world.actors[actor_index].moving = moving;
                    }
                    PlayerAction::SetTargetDirection(target_direction) => {
                        world.actors[actor_index].target_direction = target_direction;
                    }
                    PlayerAction::AddSpellElement(element) => {
                        add_actor_spell_element(actor_index, element, world);
                    }
                    PlayerAction::StartDirectedMagick => {
                        start_directed_magick(actor_index, world);
                    }
                    PlayerAction::CompleteDirectedMagick => {
                        complete_directed_magick(actor_index, world);
                    }
                    PlayerAction::SelfMagick => {
                        self_magick(actor_index, world);
                    }
                }
            } else {
                warn!("Player actor is not found for session: {}", session.session_id);
            }
        }
        v => warn!("Existing session invalid message data: {:?}", v),
    }
}

fn create_new_session<R: CryptoRng + Rng>(update_period: Duration, sender: &Sender<ServerMessage>, message: ClientMessage, sessions: &Vec<GameSession>, world: &mut World, rng: &mut R) -> Option<GameSession> {
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
            sender.send(ServerMessage {
                session_id,
                number: 0,
                data: ServerMessageData::Settings { update_period },
            }).unwrap();
            let actor_id = add_player_actor(world, rng);
            sender.send(ServerMessage {
                session_id,
                number: 0,
                data: ServerMessageData::GameUpdate(GameUpdate::SetPlayerId(actor_id)),
            }).unwrap();
            Some(GameSession {
                session_id,
                active: true,
                actor_id,
                actor_index: None,
                last_message_time: Instant::now(),
                last_message_number: message.number,
                messages_per_frame: 1,
                delayed_messages: VecDeque::new(),
            })
        }
        ClientMessageData::Quit => None,
        v => {
            warn!("New session invalid message data: {:?}", v);
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
        Self { max_frame_duration, last_measurement: now }
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
    world.actors.push(generate_player_actor(actor_id, &world.bounds, rng));
    actor_id
}
