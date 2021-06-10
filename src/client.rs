use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;

use crate::protocol::{
    deserialize_server_message, deserialize_server_message_data, get_server_message_data_type,
    ClientMessage, ClientMessageData, GameUpdate, PlayerUpdate, ServerMessageData,
    HEARTBEAT_PERIOD,
};

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
    info!("[{}] UDP client has stopped", settings.id);
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
    pub player_name: String,
}

pub struct GameChannel {
    pub sender: Sender<GameUpdate>,
    pub receiver: Receiver<PlayerUpdate>,
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
) {
    info!("[{}] Run game client", settings.id);
    let actor_id = match try_join_server(&settings, &server, &stop) {
        Some(v) => v,
        None => return,
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
        info!("[{}] Game client has stopped", settings.id);
        return;
    }
    let client_id = settings.id;
    let sender = spawn(move || run_server_sender(client_id, server_sender, game_receiver));
    run_server_receiver(client_id, game_sender, server_receiver, stop);
    sender.join().unwrap();
    info!("[{}] Game client has stopped", settings.id);
}

fn try_join_server(
    settings: &GameClientSettings,
    server: &ServerChannel,
    stop: &Arc<AtomicBool>,
) -> Option<u64> {
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
                        break Some(actor_id);
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
    sender: Sender<ClientMessageData>,
    receiver: Receiver<PlayerUpdate>,
) {
    info!("[{}] Run server sender", client_id);
    loop {
        match receiver.recv_timeout(HEARTBEAT_PERIOD) {
            Ok(player_update) => {
                if let Err(e) = sender.send(ClientMessageData::PlayerUpdate(player_update)) {
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
