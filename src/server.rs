use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

use actix_web::{web, HttpResponse};
use clap::Clap;
use rand::prelude::SmallRng;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;
use tokio::net::UdpSocket;

use crate::control::apply_actor_action;
use crate::engine::{get_next_id, remove_player, Engine};
use crate::generators::{generate_world, make_rng};
use crate::meters::{measure, DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{
    add_all_removed, deserialize_client_message, get_client_message_data_type,
    is_valid_player_name, make_server_message, make_world_update, serialize_server_message,
    ActorAction, ClientMessage, ClientMessageData, GameSessionInfo, GameUpdate, HttpMessage,
    Metric, ServerMessage, ServerMessageData, ServerStatus, Session, UdpSessionState, WorldUpdate,
    HEARTBEAT_PERIOD,
};
use crate::rect::Rectf;
use crate::vec2::Vec2f;
use crate::world::{Player, PlayerId, World};

const MAX_SESSION_MESSAGES_PER_FRAME: u8 = 3;
const MAX_DELAYED_MESSAGES_PER_SESSION: usize = 10;
const MAX_WORLD_HISTORY_SIZE: usize = 120;

#[derive(Clap, Debug)]
pub struct ServerParams {
    #[clap(long, default_value = "127.0.0.1")]
    pub address: String,
    #[clap(long, default_value = "21227")]
    pub port: u16,
    #[clap(long, default_value = "20")]
    pub max_sessions: usize,
    #[clap(long, default_value = "10")]
    pub max_players: usize,
    #[clap(long, default_value = "11")]
    pub udp_session_timeout: f64,
    #[clap(long, default_value = "10")]
    pub game_session_timeout: f64,
    #[clap(long, default_value = "60")]
    pub update_frequency: f64,
    #[clap(long)]
    pub random_seed: Option<u64>,
    #[clap(long, default_value = "127.0.0.1")]
    pub http_address: String,
    #[clap(long, default_value = "21228")]
    pub http_port: u16,
    #[clap(long, default_value = "10")]
    pub http_max_connections: usize,
}

pub fn run_server(params: ServerParams, stop: Arc<AtomicBool>) {
    info!("Run server: {:?}", params);
    if params.udp_session_timeout < params.game_session_timeout {
        warn!(
            "UDP server session timeout {:?} is less than game session timeout {:?}",
            params.udp_session_timeout, params.game_session_timeout
        );
    }
    let (udp_admin_sender, udp_admin_receiver) = channel();
    let (game_admin_sender, game_admin_receiver) = channel();
    let (http_server, http_server_handler) = run_background_http_server(
        HttpServerSettings {
            address: format!("{}:{}", params.http_address, params.http_port),
            max_connections: params.http_max_connections,
        },
        udp_admin_sender,
        game_admin_sender,
    );
    let (server_sender, server_receiver) = channel();
    let (client_sender, client_receiver) = channel();
    let stop_udp_server = Arc::new(AtomicBool::new(false));
    let update_period = Duration::from_secs_f64(1.0 / params.update_frequency);
    let udp_server = run_background_udp_sever(
        UdpServerSettings {
            address: format!("{}:{}", params.address, params.port),
            max_sessions: params.max_sessions,
            update_period,
            session_timeout: Duration::from_secs_f64(params.udp_session_timeout),
        },
        client_sender,
        server_receiver,
        udp_admin_receiver,
        stop_udp_server.clone(),
    );
    let mut world_rng = make_rng(params.random_seed);
    let world = generate_world(
        Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)),
        &mut world_rng,
    );
    run_game_server(
        world,
        world_rng,
        GameServerSettings {
            max_players: params.max_players,
            update_period,
            session_timeout: Duration::from_secs_f64(params.game_session_timeout),
        },
        server_sender,
        client_receiver,
        game_admin_receiver,
        stop,
    );
    info!("Stopping UDP server...");
    stop_udp_server.store(true, Ordering::Release);
    info!(
        "UDP server has stopped with result: {:?}",
        udp_server.join()
    );
    info!("Stopping HTTP server...");
    actix_rt::System::new().block_on(http_server.stop(true));
    info!(
        "HTTP server has stopped with result: {:?}",
        http_server_handler.join()
    );
}

pub enum UdpAdminMessage {
    GetSessions(tokio::sync::mpsc::Sender<Vec<UdpSession>>),
}

pub enum GameAdminMessage {
    Stop(tokio::sync::mpsc::Sender<()>),
    GetSessions(tokio::sync::mpsc::Sender<Vec<GameSessionInfo>>),
    RemoveSession {
        session_id: u64,
        response: tokio::sync::mpsc::Sender<Result<(), String>>,
    },
    GetStatus(tokio::sync::mpsc::Sender<ServerStatus>),
    GetWorld(tokio::sync::mpsc::Sender<Box<World>>),
}

pub enum InternalServerMessage {
    Unicast {
        session_id: u64,
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

pub fn run_background_udp_sever(
    settings: UdpServerSettings,
    sender: Sender<ClientMessage>,
    client_receiver: Receiver<InternalServerMessage>,
    admin_receiver: Receiver<UdpAdminMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<Result<(), std::io::Error>> {
    spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(run_udp_server(
            settings,
            sender,
            client_receiver,
            admin_receiver,
            stop,
        ))
    })
}

pub async fn run_udp_server(
    settings: UdpServerSettings,
    sender: Sender<ClientMessage>,
    client_receiver: Receiver<InternalServerMessage>,
    admin_receiver: Receiver<UdpAdminMessage>,
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
        client_receiver,
        admin_receiver,
        recv_buffer: vec![0u8; 65_507],
        sessions: Vec::new(),
        rng: StdRng::from_entropy(),
        message_counter: 0,
    }
    .run()
    .await;
    Ok(())
}

struct UdpServer {
    stop: Arc<AtomicBool>,
    settings: UdpServerSettings,
    sender: Sender<ClientMessage>,
    client_receiver: Receiver<InternalServerMessage>,
    admin_receiver: Receiver<UdpAdminMessage>,
    socket: UdpSocket,
    recv_buffer: Vec<u8>,
    sessions: Vec<UdpSession>,
    rng: StdRng,
    message_counter: u64,
}

#[derive(Clone)]
pub struct UdpSession {
    peer: SocketAddr,
    session_id: u64,
    last_recv_time: Instant,
    state: UdpSessionState,
}

impl UdpServer {
    async fn run(&mut self) {
        info!(
            "UDP server is listening on {}",
            self.socket.local_addr().unwrap()
        );
        let mut last_update = Instant::now();
        loop {
            let stop = self.stop.load(Ordering::Acquire);
            if stop && self.sessions.is_empty() {
                break;
            }
            self.clean_udp_sessions(stop).await;
            self.handle_game_messages().await;
            self.handle_admin_messages().await;
            if !stop {
                self.receive_messages(last_update).await;
            }
            last_update = Instant::now();
        }
    }

    async fn clean_udp_sessions(&mut self, stop: bool) {
        let now = Instant::now();
        let session_timeout = self.settings.session_timeout;
        for session in self.sessions.iter() {
            let quit = if session_timeout <= now - session.last_recv_time {
                warn!("UDP session {} is timed out", session.session_id);
                true
            } else {
                stop
            };
            if quit {
                self.sender
                    .send(ClientMessage {
                        session_id: session.session_id,
                        number: u64::MAX,
                        data: ClientMessageData::Quit,
                    })
                    .ok();
            }
        }
        if stop {
            self.send_broadcast_server_message(&ServerMessageData::GameUpdate(
                GameUpdate::GameOver(String::from("Server is stopped")),
            ))
            .await;
            self.sessions.clear();
        } else {
            self.sessions.retain(|v| {
                !matches!(v.state, UdpSessionState::Done)
                    && now - v.last_recv_time < session_timeout
            });
        }
    }

    async fn handle_game_messages(&mut self) {
        while let Ok(message) = self.client_receiver.try_recv() {
            match message {
                InternalServerMessage::Unicast { session_id, data } => {
                    self.send_unicast_server_message(session_id, &data).await;
                }
                InternalServerMessage::Broadcast(data) => {
                    self.send_broadcast_server_message(&data).await;
                }
            }
        }
    }

    async fn handle_admin_messages(&mut self) {
        while let Ok(message) = self.admin_receiver.try_recv() {
            match message {
                UdpAdminMessage::GetSessions(response) => {
                    response.try_send(self.sessions.clone()).ok();
                }
            }
        }
    }

    async fn send_unicast_server_message(&mut self, session_id: u64, data: &ServerMessageData) {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|v| v.session_id == session_id)
        {
            match data {
                ServerMessageData::NewPlayer { .. } => session.state = UdpSessionState::Established,
                ServerMessageData::GameUpdate(GameUpdate::GameOver(..)) => {
                    session.state = UdpSessionState::Done
                }
                _ => (),
            }
            self.message_counter += 1;
            send_server_message(
                &self.socket,
                session,
                &make_server_message(session.session_id, self.message_counter, &data),
            )
            .await;
        }
    }

    async fn send_broadcast_server_message(&mut self, data: &ServerMessageData) {
        if self
            .sessions
            .iter()
            .any(|v| matches!(v.state, UdpSessionState::Established))
        {
            self.message_counter += 1;
            let mut server_message = make_server_message(0, self.message_counter, data);
            for session in self
                .sessions
                .iter()
                .filter(|v| matches!(v.state, UdpSessionState::Established))
            {
                server_message.session_id = session.session_id;
                send_server_message(&self.socket, session, &server_message).await;
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

async fn send_server_message(
    socket: &UdpSocket,
    session: &UdpSession,
    server_message: &ServerMessage,
) {
    if let Err(e) = socket
        .send_to(&serialize_server_message(server_message), session.peer)
        .await
    {
        warn!(
            "Failed to send server message for session {}: {}",
            session.session_id, e
        );
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
    mut rng: SmallRng,
    settings: GameServerSettings,
    sender: Sender<InternalServerMessage>,
    client_receiver: Receiver<ClientMessage>,
    admin_receiver: Receiver<GameAdminMessage>,
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
    let mut engine = Engine::default();
    let mut world_history = VecDeque::with_capacity(MAX_WORLD_HISTORY_SIZE);
    let mut world_updates_history = VecDeque::with_capacity(MAX_WORLD_HISTORY_SIZE - 1);
    world_history.push_back(world.clone());
    sender
        .send(InternalServerMessage::Broadcast(
            ServerMessageData::GameUpdate(GameUpdate::WorldSnapshot {
                ack_actor_action_world_frame: 0,
                ack_cast_action_world_frame: 0,
                world: Box::new(world.clone()),
            }),
        ))
        .ok();
    let mut meters = Meters {
        fps: FpsMovingAverage::new(100, Duration::from_secs(1)),
        frame_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
    };
    while !stop.load(Ordering::Acquire) {
        meters.fps.add(Instant::now());
        meters.frame_duration.add(measure(|| {
            handle_delayed_messages(&settings, &sender, &mut sessions, &mut world);
            handle_new_client_messages(
                &settings,
                &sender,
                &client_receiver,
                &mut sessions,
                &mut world,
            );
            close_timed_out_sessions(settings.session_timeout, &sender, &mut sessions);
            handle_dropped_messages(&mut sessions);
            remove_inactive_actors(&mut sessions, &mut world);
            engine.update(time_step, &mut world, &mut rng);
            sessions.retain(|v| v.active);
            if world_history.len() >= MAX_WORLD_HISTORY_SIZE {
                world_history.pop_front();
                world_updates_history.pop_front();
            }
            if let Some(last) = world_history.back() {
                let world_update = make_world_update(last, &world);
                world_updates_history.push_back(world_update);
            }
            send_world_messages(
                &sender,
                &world,
                &world_history,
                &world_updates_history,
                &sessions,
            );
            world_history.push_back(world.clone());
            handle_admin_messages(
                &admin_receiver,
                frame_rate_limiter.left(Instant::now()),
                &sender,
                &meters,
                &mut sessions,
                &mut world,
                &stop,
            );
        }));
        frame_rate_limiter.limit(Instant::now());
    }
}

struct Meters {
    fps: FpsMovingAverage,
    frame_duration: DurationMovingAverage,
}

#[derive(Debug)]
struct GameSession {
    session_id: u64,
    active: bool,
    player_id: PlayerId,
    last_message_time: Instant,
    last_message_number: u64,
    messages_per_frame: u8,
    dropped_messages: usize,
    delayed_messages: VecDeque<ClientMessage>,
    ack_world_frame: u64,
    ack_cast_action_frame: u64,
}

fn handle_admin_messages(
    receiver: &Receiver<GameAdminMessage>,
    left: Duration,
    sender: &Sender<InternalServerMessage>,
    meters: &Meters,
    sessions: &mut Vec<GameSession>,
    world: &mut World,
    stop: &Arc<AtomicBool>,
) {
    let deadline = Instant::now() + left;
    while let Ok(message) = receiver.try_recv() {
        match message {
            GameAdminMessage::Stop(response) => {
                stop.store(true, Ordering::Release);
                response.try_send(()).ok();
                info!("Server has stopped by admin");
            }
            GameAdminMessage::GetSessions(response) => {
                response
                    .try_send(
                        sessions
                            .iter()
                            .map(|v| GameSessionInfo {
                                session_id: v.session_id,
                                player_id: v.player_id.0,
                                last_message_time: v.last_message_time.elapsed().as_secs_f64(),
                                last_message_number: v.last_message_number,
                                messages_per_frame: v.messages_per_frame,
                                dropped_messages: v.dropped_messages,
                                delayed_messages: v.delayed_messages.len(),
                                ack_world_frame: v.ack_world_frame,
                                ack_cast_action_frame: v.ack_cast_action_frame,
                                since_last_message: (Instant::now() - v.last_message_time)
                                    .as_secs_f64(),
                                world_frame_delay: world.frame - v.ack_world_frame,
                            })
                            .collect(),
                    )
                    .ok();
            }
            GameAdminMessage::RemoveSession {
                session_id,
                response,
            } => {
                if let Some(session) = sessions.iter_mut().find(|v| v.session_id == session_id) {
                    remove_player(session.player_id, world);
                    sender
                        .send(InternalServerMessage::Unicast {
                            session_id: session.session_id,
                            data: ServerMessageData::GameUpdate(GameUpdate::GameOver(
                                String::from("Kicked by admin"),
                            )),
                        })
                        .ok();
                    session.active = false;
                    response.try_send(Ok(())).ok();
                    info!("Game session {} is removed by admin", session.session_id);
                } else {
                    response
                        .try_send(Err(String::from("Session is not found")))
                        .ok();
                }
            }
            GameAdminMessage::GetStatus(response) => {
                let (fps_min, fps_max) = meters.fps.minmax();
                let (frame_duration_min, frame_duration_max) = meters.frame_duration.minmax();
                response
                    .try_send(ServerStatus {
                        fps: Metric {
                            mean: meters.fps.get(),
                            min: fps_min,
                            max: fps_max,
                        },
                        frame_duration: Metric {
                            mean: meters.frame_duration.get(),
                            min: frame_duration_min,
                            max: frame_duration_max,
                        },
                        sessions: sessions.len(),
                    })
                    .ok();
            }
            GameAdminMessage::GetWorld(response) => {
                response.try_send(Box::new(world.clone())).ok();
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
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

fn handle_new_client_messages(
    settings: &GameServerSettings,
    sender: &Sender<InternalServerMessage>,
    receiver: &Receiver<ClientMessage>,
    sessions: &mut Vec<GameSession>,
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
                create_new_session(settings.update_period, sender, message, world)
            {
                info!(
                    "New player has joined: session_id={} player_id={}",
                    session.session_id, session.player_id.0
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
            remove_player(session.player_id, world);
            session.active = false;
            info!("Game session {} is done", session.session_id);
        }
        ClientMessageData::Heartbeat => (),
        ClientMessageData::Join(..) => sender
            .send(InternalServerMessage::Unicast {
                session_id: session.session_id,
                data: ServerMessageData::NewPlayer {
                    update_period: settings.update_period,
                    player_id: session.player_id,
                },
            })
            .unwrap(),
        ClientMessageData::PlayerControl(mut player_control) => {
            session.ack_world_frame = player_control
                .ack_world_frame
                .max(session.ack_world_frame)
                .min(world.frame);
            if let Some(actor_index) = world
                .players
                .iter()
                .find(|v| v.id == session.player_id)
                .and_then(|v| v.actor_id)
                .and_then(|actor_id| world.actors.iter().position(|v| v.id == actor_id))
            {
                sanitize_actor_action(&mut player_control.actor_action, actor_index, world);
                if player_control.actor_action.cast_action.is_some()
                    && session.ack_cast_action_frame < player_control.cast_action_world_frame
                    && player_control.cast_action_world_frame <= session.ack_world_frame
                {
                    session.ack_cast_action_frame = session.ack_world_frame;
                } else {
                    player_control.actor_action.cast_action = None;
                }
                apply_actor_action(player_control.actor_action, actor_index, world);
            }
        }
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
            remove_player(session.player_id, world);
        }
    }
}

fn create_new_session(
    update_period: Duration,
    sender: &Sender<InternalServerMessage>,
    message: ClientMessage,
    world: &mut World,
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
            if let Some(player_id) = try_add_player(name, world) {
                sender
                    .send(InternalServerMessage::Unicast {
                        session_id: message.session_id,
                        data: ServerMessageData::NewPlayer {
                            update_period,
                            player_id,
                        },
                    })
                    .unwrap();
                Some(GameSession {
                    session_id: message.session_id,
                    active: true,
                    player_id,
                    last_message_time: Instant::now(),
                    last_message_number: message.number,
                    messages_per_frame: 1,
                    delayed_messages: VecDeque::with_capacity(MAX_DELAYED_MESSAGES_PER_SESSION),
                    dropped_messages: 0,
                    ack_world_frame: 0,
                    ack_cast_action_frame: world.frame,
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
            debug!(
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
        let passed = self.passed(now);
        if passed < self.max_frame_duration {
            sleep(self.max_frame_duration - passed);
            self.last_measurement += self.max_frame_duration;
        } else {
            self.last_measurement = now;
        }
    }

    fn left(&self, now: Instant) -> Duration {
        let passed = self.passed(now);
        if passed < self.max_frame_duration {
            self.max_frame_duration - passed
        } else {
            Duration::new(0, 0)
        }
    }

    fn passed(&self, now: Instant) -> Duration {
        now - self.last_measurement
    }
}

fn try_add_player(name: String, world: &mut World) -> Option<PlayerId> {
    if world.players.iter().any(|v| v.name == name) || world.actors.iter().any(|v| v.name == name) {
        return None;
    }
    let player_id = PlayerId(get_next_id(&mut world.id_counter));
    world.players.push(Player {
        id: player_id,
        active: true,
        name,
        actor_id: None,
        spawn_time: world.time + world.settings.initial_player_actor_spawn_delay,
        deaths: 0,
    });
    Some(player_id)
}

fn sanitize_actor_action(actor_action: &mut ActorAction, actor_index: usize, world: &World) {
    let norm = actor_action.target_direction.norm();
    if norm > f64::EPSILON {
        actor_action.target_direction /= norm;
    } else {
        actor_action.target_direction = world.actors[actor_index].target_direction;
    }
}

fn send_world_messages(
    sender: &Sender<InternalServerMessage>,
    world: &World,
    world_history: &VecDeque<World>,
    world_updates_history: &VecDeque<WorldUpdate>,
    sessions: &[GameSession],
) {
    let mut world_snapshot_session_indices = Vec::new();
    let mut world_updates: Vec<(usize, Vec<usize>, WorldUpdate)> = Vec::new();
    for (session_index, session) in sessions.iter().enumerate() {
        if session.ack_world_frame == 0 {
            world_snapshot_session_indices.push(session_index);
            continue;
        }
        let offset = (world.frame - session.ack_world_frame) as usize;
        if offset > world_history.len() {
            world_snapshot_session_indices.push(session_index);
            continue;
        }
        if let Some((_, session_indices, _)) =
            world_updates.iter_mut().find(|(v, _, _)| *v == offset)
        {
            session_indices.push(session_index);
            continue;
        }
        let mut world_update =
            make_world_update(&world_history[world_history.len() - offset], &world);
        add_all_removed(
            world_updates_history
                .iter()
                .skip(world_updates_history.len() - offset),
            &mut world_update,
        );
        world_updates.push((offset, vec![session_index], world_update));
    }
    for (_, session_indices, world_update) in world_updates {
        for session_index in session_indices {
            let session = &sessions[session_index];
            sender
                .send(InternalServerMessage::Unicast {
                    session_id: session.session_id,
                    data: ServerMessageData::GameUpdate(GameUpdate::WorldUpdate {
                        ack_actor_action_world_frame: session.ack_world_frame,
                        ack_cast_action_world_frame: session.ack_cast_action_frame,
                        world_update: Box::new(world_update.clone()),
                    }),
                })
                .ok();
        }
    }
    for session_index in world_snapshot_session_indices {
        let session = &sessions[session_index];
        sender
            .send(InternalServerMessage::Unicast {
                session_id: session.session_id,
                data: ServerMessageData::GameUpdate(GameUpdate::WorldSnapshot {
                    ack_actor_action_world_frame: session.ack_world_frame,
                    ack_cast_action_world_frame: session.ack_cast_action_frame,
                    world: Box::new(world.clone()),
                }),
            })
            .ok();
    }
}

#[derive(Debug)]
pub struct HttpServerSettings {
    pub address: String,
    pub max_connections: usize,
}

fn run_background_http_server(
    settings: HttpServerSettings,
    udp_admin_sender: Sender<UdpAdminMessage>,
    game_admin_sender: Sender<GameAdminMessage>,
) -> (actix_web::dev::Server, JoinHandle<std::io::Result<()>>) {
    let (server_sender, receiver) = channel();
    let handler = spawn(move || {
        run_http_server(settings, udp_admin_sender, game_admin_sender, server_sender)
    });
    (receiver.recv().unwrap(), handler)
}

fn run_http_server(
    settings: HttpServerSettings,
    udp_admin_sender: Sender<UdpAdminMessage>,
    game_admin_sender: Sender<GameAdminMessage>,
    server_sender: Sender<actix_web::dev::Server>,
) -> std::io::Result<()> {
    use actix_web::{middleware, App, HttpServer};

    let http_server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .data(udp_admin_sender.clone())
            .data(game_admin_sender.clone())
            .service(web::resource("/ping").route(web::get().to(ping)))
            .service(web::resource("/stop").route(web::post().to(stop)))
            .service(web::resource("/sessions").route(web::get().to(sessions)))
            .service(web::resource("/remove_session").route(web::post().to(remove_sessions)))
            .service(web::resource("/status").route(web::get().to(status)))
            .service(web::resource("/world").route(web::get().to(world)))
            .default_service(web::resource("").to(HttpResponse::NotFound))
    })
    .workers(1)
    .max_connections(settings.max_connections)
    .disable_signals()
    .bind(settings.address)?;
    actix_rt::System::new().block_on(async move { server_sender.send(http_server.run()).unwrap() });
    Ok(())
}

async fn ping() -> HttpResponse {
    HttpResponse::Ok().json(HttpMessage::Ok)
}

async fn stop(game_admin_sender: web::Data<Sender<GameAdminMessage>>) -> HttpResponse {
    let (response, mut request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = game_admin_sender.send(GameAdminMessage::Stop(response)) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("{}", e),
        });
    }
    HttpResponse::Ok().json(match request.recv().await {
        Some(..) => HttpMessage::Ok,
        None => HttpMessage::Error {
            message: String::from("Failed to get response"),
        },
    })
}

async fn sessions(
    udp_admin_sender: web::Data<Sender<UdpAdminMessage>>,
    game_admin_sender: web::Data<Sender<GameAdminMessage>>,
) -> HttpResponse {
    let (udp_response, mut udp_request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = udp_admin_sender.send(UdpAdminMessage::GetSessions(udp_response)) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("Failed to send UDP admin request: {}", e),
        });
    }
    let (game_response, mut game_request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = game_admin_sender.send(GameAdminMessage::GetSessions(game_response)) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("Failed to send game admin request: {}", e),
        });
    }
    let udp_sessions = match udp_request.recv().await {
        Some(v) => v,
        None => {
            return HttpResponse::Ok().json(HttpMessage::Error {
                message: String::from("Failed to get UDP admin response"),
            });
        }
    };
    let game_sessions = match game_request.recv().await {
        Some(v) => v,
        None => {
            return HttpResponse::Ok().json(HttpMessage::Error {
                message: String::from("Failed to get game admin response"),
            });
        }
    };
    HttpResponse::Ok().json(HttpMessage::Sessions {
        sessions: udp_sessions
            .iter()
            .map(|udp| Session {
                session_id: udp.session_id,
                peer: udp.peer.to_string(),
                last_recv_time: udp.last_recv_time.elapsed().as_secs_f64(),
                state: udp.state,
                game: game_sessions
                    .iter()
                    .find(|v| v.session_id == udp.session_id)
                    .cloned(),
            })
            .collect(),
    })
}

#[derive(Deserialize)]
struct RemoveSession {
    session_id: u64,
}

async fn remove_sessions(
    game_admin_sender: web::Data<Sender<GameAdminMessage>>,
    query: web::Query<RemoveSession>,
) -> HttpResponse {
    let (response, mut request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = game_admin_sender.send(GameAdminMessage::RemoveSession {
        session_id: query.session_id,
        response,
    }) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("{}", e),
        });
    }
    HttpResponse::Ok().json(match request.recv().await {
        Some(v) => match v {
            Ok(..) => HttpMessage::Ok,
            Err(e) => HttpMessage::Error { message: e },
        },
        None => HttpMessage::Error {
            message: String::from("Failed to get response"),
        },
    })
}

async fn status(game_admin_sender: web::Data<Sender<GameAdminMessage>>) -> HttpResponse {
    let (response, mut request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = game_admin_sender.send(GameAdminMessage::GetStatus(response)) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("{}", e),
        });
    }
    HttpResponse::Ok().json(match request.recv().await {
        Some(v) => HttpMessage::Status { status: v },
        None => HttpMessage::Error {
            message: String::from("Failed to get response"),
        },
    })
}

async fn world(game_admin_sender: web::Data<Sender<GameAdminMessage>>) -> HttpResponse {
    let (response, mut request) = tokio::sync::mpsc::channel(1);
    if let Err(e) = game_admin_sender.send(GameAdminMessage::GetWorld(response)) {
        return HttpResponse::Ok().json(HttpMessage::Error {
            message: format!("{}", e),
        });
    }
    HttpResponse::Ok().json(match request.recv().await {
        Some(v) => HttpMessage::World { world: v },
        None => HttpMessage::Error {
            message: String::from("Failed to get response"),
        },
    })
}
