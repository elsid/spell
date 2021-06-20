use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::runtime::Builder;

use crate::protocol::{
    deserialize_server_message, deserialize_server_message_data, get_server_message_data_type,
    ClientMessage, ClientMessageData, GameUpdate, PlayerControl, ServerMessageData,
    HEARTBEAT_PERIOD,
};

pub struct Client {
    game_client: Option<GameClient>,
    udp_client: Option<UdpClient>,
}

impl Client {
    pub fn new(
        game_client_settings: GameClientSettings,
        udp_client_settings: UdpClientSettings,
    ) -> Self {
        let mut udp_client = UdpClient::new(udp_client_settings);
        Self {
            game_client: Some(GameClient::new(
                game_client_settings,
                udp_client.client_sender.take().unwrap(),
                udp_client.server_receiver.take().unwrap(),
            )),
            udp_client: Some(udp_client),
        }
    }

    pub fn is_running(&self) -> bool {
        !self.game_client.as_ref().unwrap().is_done()
            && !self.udp_client.as_ref().unwrap().is_done()
    }

    pub fn stop(&self) {
        if !self.game_client.as_ref().unwrap().is_stopping() {
            self.game_client.as_ref().unwrap().stop();
        }
        if !self.game_client.as_ref().unwrap().is_stopping() {
            self.udp_client.as_ref().unwrap().stop();
        }
    }

    pub fn is_done(&self) -> bool {
        self.game_client.as_ref().unwrap().is_done() && self.udp_client.as_ref().unwrap().is_done()
    }

    pub fn join(&mut self) -> Result<(), String> {
        let mut error = String::new();
        if let Err(e) = self.game_client.as_mut().unwrap().join() {
            error = e;
        }
        if let Err(e) = self.udp_client.as_mut().unwrap().join() {
            if error.is_empty() {
                error = format!("{}", e);
            } else {
                error = format!("{}, {}", error, e);
            }
        }
        if error.is_empty() {
            return Ok(());
        }
        Err(error)
    }

    pub fn sender(&self) -> &Sender<PlayerControl> {
        self.game_client.as_ref().unwrap().sender()
    }

    pub fn receiver(&self) -> &Receiver<GameUpdate> {
        self.game_client.as_ref().unwrap().receiver()
    }
}

pub struct GameClient {
    id: u64,
    player_control_sender: Option<Sender<PlayerControl>>,
    game_update_receiver: Option<Receiver<GameUpdate>>,
    handle: Option<JoinHandle<Result<(), String>>>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

impl GameClient {
    pub fn new(
        settings: GameClientSettings,
        client_sender: Sender<ClientMessageData>,
        server_receiver: Receiver<ServerMessageData>,
    ) -> Self {
        let (game_update_sender, game_update_receiver) = channel();
        let (player_control_sender, player_control_receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        Self {
            id: settings.id,
            player_control_sender: Some(player_control_sender),
            game_update_receiver: Some(game_update_receiver),
            handle: Some(run_background_game_client(
                settings,
                game_update_sender,
                player_control_receiver,
                client_sender,
                server_receiver,
                stop.clone(),
                done.clone(),
            )),
            stop,
            done,
        }
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        info!("[{}] Stopping game client...", self.id);
        self.stop.store(true, Ordering::Release);
    }

    pub fn join(&mut self) -> Result<(), String> {
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap()
        } else {
            Ok(())
        }
    }

    pub fn sender(&self) -> &Sender<PlayerControl> {
        self.player_control_sender.as_ref().unwrap()
    }

    pub fn receiver(&self) -> &Receiver<GameUpdate> {
        self.game_update_receiver.as_ref().unwrap()
    }
}

impl Drop for GameClient {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            info!("[{}] Stopping game client...", self.id);
            self.stop.store(true, Ordering::Release);
            handle.join().ok();
        }
    }
}

pub struct UdpClient {
    id: u64,
    client_sender: Option<Sender<ClientMessageData>>,
    server_receiver: Option<Receiver<ServerMessageData>>,
    handle: Option<JoinHandle<Result<(), std::io::Error>>>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

impl UdpClient {
    pub fn new(settings: UdpClientSettings) -> Self {
        let (server_sender, server_receiver) = channel();
        let (client_sender, client_receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        Self {
            id: settings.id,
            client_sender: Some(client_sender),
            server_receiver: Some(server_receiver),
            handle: Some(run_background_udp_client(
                settings,
                server_sender,
                client_receiver,
                stop.clone(),
                done.clone(),
            )),
            stop,
            done,
        }
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        info!("[{}] Stopping UDP client...", self.id);
        self.stop.store(true, Ordering::Release);
    }

    pub fn join(&mut self) -> Result<(), std::io::Error> {
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap()
        } else {
            Ok(())
        }
    }
}

impl Drop for UdpClient {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            info!("[{}] Stopping UDP client...", self.id);
            self.stop.store(true, Ordering::Release);
            handle.join().ok();
        }
    }
}

pub fn run_background_game_client(
    settings: GameClientSettings,
    update_sender: Sender<GameUpdate>,
    player_control_receiver: Receiver<PlayerControl>,
    client_sender: Sender<ClientMessageData>,
    server_receiver: Receiver<ServerMessageData>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
) -> JoinHandle<Result<(), String>> {
    let game_channel = GameChannel {
        sender: update_sender,
        receiver: player_control_receiver,
    };
    let server_channel = ServerChannel {
        sender: client_sender,
        receiver: server_receiver,
    };
    spawn(move || {
        let id = settings.id;
        let result = run_game_client(settings, server_channel, game_channel, stop);
        done.store(true, Ordering::Release);
        info!("[{}] Game client has stopped with result: {:?}", id, result);
        result
    })
}

pub fn run_background_udp_client(
    settings: UdpClientSettings,
    server_sender: Sender<ServerMessageData>,
    client_receiver: Receiver<ClientMessageData>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
) -> JoinHandle<Result<(), std::io::Error>> {
    spawn(move || {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        let id = settings.id;
        let result = runtime.block_on(run_udp_client(
            settings,
            server_sender,
            client_receiver,
            stop,
        ));
        done.store(true, Ordering::Release);
        info!("[{}] UDP client has stopped with result: {:?}", id, result);
        result
    })
}

#[derive(Debug, Clone)]
pub struct UdpClientSettings {
    pub id: u64,
    pub server_address: SocketAddr,
    pub read_timeout: Duration,
}

pub async fn run_udp_client(
    settings: UdpClientSettings,
    sender: Sender<ServerMessageData>,
    receiver: Receiver<ClientMessageData>,
    stop: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    info!("[{}] Run UDP client: {:?}", settings.id, settings);
    let local_address = match settings.server_address {
        SocketAddr::V4(..) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(..) => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
    };
    let socket = UdpSocket::bind(local_address).await?;
    info!(
        "[{}] UDP client is listening on {}",
        settings.id,
        socket.local_addr().unwrap()
    );
    socket.connect(settings.server_address).await?;
    info!(
        "[{}] UDP client is connected to {}",
        settings.id, settings.server_address
    );
    let mut recv_buffer = vec![0u8; 65_507];
    let mut last_update = Instant::now();
    let mut last_recv = last_update;
    let mut update_period = Duration::from_secs_f64(1.0);
    let mut prev_received_message_number = 0;
    let mut session_id = None;
    let mut client_message_number = 0;
    while !stop.load(Ordering::Acquire) {
        if !send_client_messages(
            &settings,
            &receiver,
            &socket,
            Instant::now() + update_period / 2,
            session_id.unwrap_or(0),
            &mut client_message_number,
        )
        .await?
        {
            debug!("[{}] UDP client is quitting...", settings.id);
            break;
        }
        let now = Instant::now();
        if now - last_recv >= settings.read_timeout {
            sender
                .send(ServerMessageData::GameUpdate(GameUpdate::GameOver(
                    String::from("Timeout"),
                )))
                .ok();
            break;
        }
        let passed = now - last_update;
        let recv_timeout = if passed < update_period {
            update_period - passed
        } else {
            Duration::from_millis(1)
        };
        if let Ok(Ok(size)) =
            tokio::time::timeout(recv_timeout, socket.recv(&mut recv_buffer)).await
        {
            last_recv = Instant::now();
            let server_message = match deserialize_server_message(&recv_buffer[0..size]) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "[{}] Failed to deserialize server message: {}",
                        settings.id, e
                    );
                    continue;
                }
            };
            if session_id.is_some() && session_id.unwrap() != server_message.session_id {
                warn!(
                    "[{}] Received server message for invalid session: received={} expected={}",
                    settings.id,
                    session_id.unwrap(),
                    server_message.session_id
                );
                continue;
            }
            if prev_received_message_number >= server_message.number {
                warn!(
                    "[{}] Received outdated server message: prev received number={} new received number={}",
                    settings.id, prev_received_message_number, server_message.number
                );
                continue;
            }
            prev_received_message_number = server_message.number;
            let data = match deserialize_server_message_data(
                &server_message.data,
                server_message.decompressed_data_size as usize,
            ) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "[{}] Failed to deserialize server message data: {}",
                        settings.id, e
                    );
                    continue;
                }
            };
            if session_id.is_none() {
                session_id = Some(server_message.session_id);
            }
            if let ServerMessageData::NewPlayer {
                update_period: v, ..
            } = &data
            {
                update_period = *v;
            }
            if let Err(e) = sender.send(data) {
                debug!(
                    "[{}] UDP client has failed to send a message: {}",
                    settings.id, e
                );
                break;
            }
        }
        last_update = Instant::now();
    }
    Ok(())
}

async fn send_client_messages(
    settings: &UdpClientSettings,
    receiver: &Receiver<ClientMessageData>,
    socket: &UdpSocket,
    until: Instant,
    session_id: u64,
    number: &mut u64,
) -> std::io::Result<bool> {
    while Instant::now() < until {
        if let Ok(data) = receiver.try_recv() {
            *number += 1;
            let client_message = ClientMessage {
                session_id,
                number: *number,
                data,
            };
            let buffer = bincode::serialize(&client_message).unwrap();
            if let Err(e) = send_with_retries(&socket, &buffer, 3).await {
                error!(
                    "[{}] UDP client has failed to send message to server: {}",
                    settings.id, e
                );
                return Err(e);
            }
        } else {
            break;
        }
    }
    Ok(true)
}

async fn send_with_retries(
    socket: &UdpSocket,
    buffer: &[u8],
    max_retries: usize,
) -> std::io::Result<usize> {
    let mut retries: usize = 0;
    loop {
        match socket.send(&buffer).await {
            Err(e) => {
                if retries >= max_retries {
                    break Err(e);
                }
                retries += 1;
            }
            v => break v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameClientSettings {
    pub id: u64,
    pub connect_timeout: Duration,
    pub retry_period: Duration,
    pub player_name: String,
}

pub struct GameChannel {
    pub sender: Sender<GameUpdate>,
    pub receiver: Receiver<PlayerControl>,
}

pub struct ServerChannel {
    pub sender: Sender<ClientMessageData>,
    pub receiver: Receiver<ServerMessageData>,
}

pub fn run_game_client(
    settings: GameClientSettings,
    server: ServerChannel,
    game: GameChannel,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    info!("[{}] Run game client", settings.id);
    let actor_id = match try_join_server(&settings, &server, &stop) {
        Ok(v) => v,
        Err(e) => return Err(format!("Failed to join the server: {}", e)),
    };
    info!("[{}] Joined to server as actor {}", settings.id, actor_id);
    let ServerChannel {
        sender: server_sender,
        receiver: server_receiver,
    } = server;
    let GameChannel {
        sender: game_sender,
        receiver: game_receiver,
    } = game;
    if let Err(..) = game_sender.send(GameUpdate::SetActorId(actor_id)) {
        return Err(String::from("Failed to request actor id."));
    }
    let client_id = settings.id;
    let stop_receiver = Arc::new(AtomicBool::new(false));
    let receiver = run_background_server_receiver(
        client_id,
        game_sender,
        server_receiver,
        stop_receiver.clone(),
    );
    run_server_sender(client_id, server_sender, game_receiver, stop);
    stop_receiver.store(true, Ordering::Release);
    receiver.join().ok();
    Ok(())
}

fn run_background_server_receiver(
    client_id: u64,
    sender: Sender<GameUpdate>,
    receiver: Receiver<ServerMessageData>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    spawn(move || run_server_receiver(client_id, sender, receiver, stop))
}

fn try_join_server(
    settings: &GameClientSettings,
    server: &ServerChannel,
    stop: &Arc<AtomicBool>,
) -> Result<u64, String> {
    let now = Instant::now();
    let connect_deadline = now + settings.connect_timeout;
    let mut last_send = now - settings.retry_period;
    loop {
        if Instant::now() >= connect_deadline {
            info!(
                "[{}] Game client has timed out to connect to server.",
                settings.id
            );
            return Err(String::from("Timeout"));
        }
        if stop.load(Ordering::Acquire) {
            return Err(String::from("Aborted"));
        }
        let since_last_send = Instant::now() - last_send;
        if since_last_send < settings.retry_period {
            sleep(settings.retry_period - since_last_send);
        }
        if stop.load(Ordering::Acquire) {
            return Err(String::from("Aborted"));
        }
        debug!("[{}] Game client is trying to join server...", settings.id);
        if let Err(e) = server
            .sender
            .send(ClientMessageData::Join(settings.player_name.clone()))
        {
            debug!(
                "[{}] Game client has failed to send join message: {}",
                settings.id, e
            );
            last_send = Instant::now();
            continue;
        }
        last_send = Instant::now();
        debug!(
            "[{}] Game client is waiting for server response...",
            settings.id
        );
        match server.receiver.recv_timeout(settings.retry_period) {
            Ok(data) => {
                if !matches!(data, ServerMessageData::GameUpdate(..)) {
                    debug!("[{}] Client handle: {:?}", settings.id, data);
                } else {
                    debug!(
                        "[{}] Client handle: {}",
                        settings.id,
                        get_server_message_data_type(&data)
                    );
                }
                match data {
                    ServerMessageData::NewPlayer { actor_id, .. } => {
                        break Ok(actor_id);
                    }
                    ServerMessageData::Error(err) => return Err(err),
                    v => warn!(
                        "[{}] Game client has received invalid server response type: {}",
                        settings.id,
                        get_server_message_data_type(&v)
                    ),
                }
            }
            Err(e) => debug!(
                "[{}] Game client has failed to receive message: {}",
                settings.id, e
            ),
        }
    }
}

fn run_server_sender(
    client_id: u64,
    sender: Sender<ClientMessageData>,
    receiver: Receiver<PlayerControl>,
    stop: Arc<AtomicBool>,
) {
    info!("[{}] Run server sender", client_id);
    while !stop.load(Ordering::Acquire) {
        match receiver.recv_timeout(HEARTBEAT_PERIOD) {
            Ok(player_control) => {
                if let Err(e) = sender.send(ClientMessageData::PlayerControl(player_control)) {
                    error!(
                        "[{}] Server sender has failed to send player action: {}",
                        client_id, e
                    );
                    break;
                }
            }
            Err(e) => match e {
                RecvTimeoutError::Timeout => {
                    if let Err(e) = sender.send(ClientMessageData::Heartbeat) {
                        error!(
                            "[{}] Server sender has failed to send heartbeat: {}",
                            client_id, e
                        );
                        break;
                    }
                }
                RecvTimeoutError::Disconnected => break,
            },
        }
    }
    debug!("[{}] Server sender is sending quit...", client_id);
    if let Err(e) = sender.send(ClientMessageData::Quit) {
        warn!(
            "[{}] Server sender has failed to send quit: {}",
            client_id, e
        );
    }
    info!("[{}] Server sender has stopped", client_id);
}

fn run_server_receiver(
    client_id: u64,
    sender: Sender<GameUpdate>,
    receiver: Receiver<ServerMessageData>,
    stop: Arc<AtomicBool>,
) {
    info!("[{}] Run server receiver", client_id);
    while !stop.load(Ordering::Acquire) {
        match receiver.recv() {
            Ok(data) => match data {
                ServerMessageData::NewPlayer { .. } => (),
                ServerMessageData::Error(error) => {
                    warn!("[{}] Server error: {}", client_id, error);
                }
                ServerMessageData::GameUpdate(update) => match sender.send(update) {
                    Ok(..) => (),
                    Err(e) => {
                        debug!(
                            "[{}] Server receiver has failed to send a message: {}",
                            client_id, e
                        );
                    }
                },
            },
            Err(e) => {
                debug!("Server receiver has failed to receive a message: {}", e);
                break;
            }
        }
    }
    info!("[{}] Server receiver has stopped", client_id);
}
