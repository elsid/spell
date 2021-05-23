use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use lz4_flex::decompress_size_prepended;
use tokio::net::UdpSocket;

use crate::protocol::{
    get_server_message_data_type, ClientMessage, ClientMessageData, GameUpdate, PlayerUpdate,
    ServerMessage, ServerMessageData, HEARTBEAT_PERIOD,
};

#[derive(Debug, Clone)]
pub struct UdpClientSettings {
    pub id: u64,
    pub server_address: String,
}

pub async fn run_udp_client(
    settings: UdpClientSettings,
    sender: Sender<ServerMessage>,
    receiver: Receiver<ClientMessage>,
    stop: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    info!("[{}] Run UDP client: {:?}", settings.id, settings);
    let server_address: SocketAddr = settings.server_address.parse().unwrap();
    let local_address = match server_address {
        SocketAddr::V4(..) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(..) => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
    };
    let socket = UdpSocket::bind(local_address).await?;
    info!(
        "[{}] UDP client is listening on {}",
        settings.id,
        socket.local_addr().unwrap()
    );
    socket.connect(server_address).await?;
    info!(
        "[{}] UDP client is connected to {}",
        settings.id, server_address
    );
    let mut recv_buffer = vec![0u8; 65_507];
    let mut last_update = Instant::now();
    let mut update_period = Duration::from_secs_f64(1.0);
    while !stop.load(Ordering::Acquire) {
        if !send_client_messages(
            &settings,
            &receiver,
            &socket,
            Instant::now() + update_period / 2,
        )
        .await?
        {
            debug!("[{}] UDP client is quitting...", settings.id);
            break;
        }
        let left = Instant::now() - last_update;
        let recv_timeout = if left < update_period {
            update_period - left
        } else {
            Duration::from_millis(1)
        };
        if let Ok(Ok(size)) =
            tokio::time::timeout(recv_timeout, socket.recv(&mut recv_buffer)).await
        {
            let decompressed = match decompress_size_prepended(&recv_buffer[0..size]) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "[{}] Failed to decompress server message: {}",
                        settings.id, e
                    );
                    continue;
                }
            };
            let server_message: ServerMessage = match bincode::deserialize(&decompressed) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "[{}] Failed to deserialize server message: {}",
                        settings.id, e
                    );
                    continue;
                }
            };
            if let ServerMessageData::NewPlayer {
                update_period: v, ..
            } = &server_message.data
            {
                update_period = *v;
            }
            if let Err(e) = sender.send(server_message) {
                debug!(
                    "[{}] UDP client has failed to send a message: {}",
                    settings.id, e
                );
                break;
            }
        }
        last_update = Instant::now();
    }
    info!("[{}] UDP client has stopped", settings.id);
    Ok(())
}

async fn send_client_messages(
    settings: &UdpClientSettings,
    receiver: &Receiver<ClientMessage>,
    socket: &UdpSocket,
    until: Instant,
) -> std::io::Result<bool> {
    while Instant::now() < until {
        if let Ok(client_message) = receiver.try_recv() {
            let buffer = bincode::serialize(&client_message).unwrap();
            if let Err(e) = send_with_retries(&socket, &buffer, 3).await {
                error!(
                    "[{}] UDP client has failed to send message to server: {}",
                    settings.id, e
                );
                return Err(e);
            }
            if matches!(client_message.data, ClientMessageData::Quit) {
                return Ok(false);
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
}

pub struct GameChannel {
    pub sender: Sender<GameUpdate>,
    pub receiver: Receiver<PlayerUpdate>,
}

pub struct ServerChannel {
    pub sender: Sender<ClientMessage>,
    pub receiver: Receiver<ServerMessage>,
}

pub fn run_game_client(
    settings: GameClientSettings,
    server: ServerChannel,
    game: GameChannel,
    stop: Arc<AtomicBool>,
) {
    info!("[{}] Run game client", settings.id);
    let mut client_message_number = 0;
    let mut server_message_number = 0;
    let ServerInfo {
        session_id,
        actor_id,
    } = match try_join_server(
        &settings,
        &server,
        &stop,
        &mut client_message_number,
        &mut server_message_number,
    ) {
        Some(v) => v,
        None => return,
    };
    info!(
        "[{}] Joined to server with session {} as actor {}",
        settings.id, session_id, actor_id
    );
    let ServerChannel {
        sender: server_sender,
        receiver: server_receiver,
    } = server;
    let GameChannel {
        sender: game_sender,
        receiver: game_receiver,
    } = game;
    if let Err(..) = game_sender.send(GameUpdate::SetPlayerId(actor_id)) {
        info!(
            "[{}] Game client has stopped for session {}",
            settings.id, session_id
        );
        return;
    }
    let client_id = settings.id;
    let sender = spawn(move || {
        run_server_sender(
            client_id,
            session_id,
            client_message_number,
            server_sender,
            game_receiver,
        )
    });
    run_server_receiver(
        client_id,
        session_id,
        server_message_number,
        game_sender,
        server_receiver,
        stop,
    );
    sender.join().unwrap();
    info!(
        "[{}] Game client has stopped for session {}",
        settings.id, session_id
    );
}

struct ServerInfo {
    session_id: u64,
    actor_id: u64,
}

fn try_join_server(
    settings: &GameClientSettings,
    server: &ServerChannel,
    stop: &Arc<AtomicBool>,
    client_message_number: &mut u64,
    server_message_number: &mut u64,
) -> Option<ServerInfo> {
    let now = Instant::now();
    let connect_deadline = now + settings.connect_timeout;
    let mut last_send = now - settings.retry_period;
    loop {
        if Instant::now() >= connect_deadline {
            info!(
                "[{}] Game client has timed out to connect to server.",
                settings.id
            );
            return None;
        }
        if stop.load(Ordering::Acquire) {
            return None;
        }
        let since_last_send = Instant::now() - last_send;
        if since_last_send < settings.retry_period {
            sleep(settings.retry_period - since_last_send);
        }
        if stop.load(Ordering::Acquire) {
            return None;
        }
        debug!("[{}] Game client is trying to join server...", settings.id);
        *client_message_number += 1;
        if let Err(e) = server.sender.send(ClientMessage {
            number: *client_message_number,
            session_id: 0,
            data: ClientMessageData::Join,
        }) {
            debug!(
                "[{}] Game client has failed to send join message: {}",
                settings.id, e
            );
            last_send = Instant::now();
            continue;
        }
        last_send = Instant::now();
        *client_message_number += 1;
        debug!(
            "[{}] Game client is waiting for server response...",
            settings.id
        );
        match server.receiver.recv_timeout(settings.retry_period) {
            Ok(message) => {
                if !matches!(message.data, ServerMessageData::GameUpdate(..)) {
                    debug!("[{}] Client handle: {:?}", settings.id, message);
                } else {
                    debug!(
                        "[{}] Client handle: {}",
                        settings.id,
                        get_server_message_data_type(&message.data)
                    );
                }
                if message.number <= *server_message_number {
                    continue;
                }
                *server_message_number = message.number;
                match message.data {
                    ServerMessageData::NewPlayer { actor_id, .. } => {
                        break Some(ServerInfo {
                            session_id: message.session_id,
                            actor_id,
                        });
                    }
                    ServerMessageData::Error(err) => {
                        error!("[{}] Join to server error: {}", settings.id, err);
                        return None;
                    }
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
    session_id: u64,
    mut message_number: u64,
    sender: Sender<ClientMessage>,
    receiver: Receiver<PlayerUpdate>,
) {
    info!(
        "[{}] Run server sender for session {}",
        client_id, session_id
    );
    loop {
        match receiver.recv_timeout(HEARTBEAT_PERIOD) {
            Ok(player_update) => {
                if let Err(e) = sender.send(ClientMessage {
                    session_id,
                    number: message_number,
                    data: ClientMessageData::PlayerUpdate(player_update),
                }) {
                    error!(
                        "[{}] Server sender has failed to send player action for session {}: {}",
                        client_id, session_id, e
                    );
                    break;
                }
                message_number += 1;
            }
            Err(e) => match e {
                RecvTimeoutError::Timeout => {
                    if let Err(e) = sender.send(ClientMessage {
                        session_id,
                        number: message_number,
                        data: ClientMessageData::Heartbeat,
                    }) {
                        error!(
                            "[{}] Server sender has failed to send heartbeat for session {}: {}",
                            client_id, session_id, e
                        );
                        break;
                    }
                    message_number += 1;
                }
                RecvTimeoutError::Disconnected => break,
            },
        }
    }
    debug!(
        "[{}] Server sender is sending quit for session {}...",
        client_id, session_id
    );
    if let Err(e) = sender.send(ClientMessage {
        session_id,
        number: message_number,
        data: ClientMessageData::Quit,
    }) {
        warn!(
            "[{}] Server sender has failed to send quit for session {}: {}",
            client_id, session_id, e
        );
    }
    info!(
        "[{}] Server sender has stopped for session {}",
        client_id, session_id
    );
}

fn run_server_receiver(
    client_id: u64,
    session_id: u64,
    mut message_number: u64,
    sender: Sender<GameUpdate>,
    receiver: Receiver<ServerMessage>,
    stop: Arc<AtomicBool>,
) {
    info!(
        "[{}] Run server receiver for session {}",
        client_id, session_id
    );
    while !stop.load(Ordering::Acquire) {
        match receiver.recv() {
            Ok(message) => {
                if message.session_id != 0 && message.session_id != session_id {
                    continue;
                }
                if message.number <= message_number {
                    continue;
                }
                message_number = message.number;
                match message.data {
                    ServerMessageData::NewPlayer { .. } => (),
                    ServerMessageData::Error(error) => {
                        warn!(
                            "[{}] Server error for session {}: {}",
                            client_id, session_id, error
                        );
                    }
                    ServerMessageData::GameUpdate(update) => match sender.send(update) {
                        Ok(..) => (),
                        Err(e) => {
                            debug!(
                                "[{}] Server receiver has failed to send a message for session {}: {}",
                                client_id, session_id, e
                            );
                        }
                    },
                }
            }
            Err(e) => {
                debug!(
                    "Server receiver has failed to receive a message for session {}: {}",
                    session_id, e
                );
                break;
            }
        }
    }
    info!(
        "[{}] Server receiver has stopped for session {}",
        client_id, session_id
    );
}
