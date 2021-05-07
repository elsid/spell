use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::spawn;
use std::time::{Duration, Instant};

use lz4_flex::decompress_size_prepended;
use tokio::net::UdpSocket;

use crate::protocol::{
    get_server_message_data_type, ClientMessage, ClientMessageData, GameUpdate, PlayerAction,
    ServerMessage, ServerMessageData,
};

#[derive(Debug)]
pub struct UdpClientSettings {
    pub server_address: String,
}

pub async fn run_udp_client(
    settings: UdpClientSettings,
    sender: Sender<ServerMessage>,
    receiver: Receiver<ClientMessage>,
    stop: Arc<AtomicBool>,
) -> Result<(), std::io::Error> {
    info!("Run UDP client: {:?}", settings);
    let server_address: SocketAddr = settings.server_address.parse().unwrap();
    let local_address = match server_address {
        SocketAddr::V4(..) => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(..) => SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
    };
    let socket = UdpSocket::bind(local_address).await?;
    info!(
        "UDP client is listening on {}",
        socket.local_addr().unwrap()
    );
    socket.connect(server_address).await?;
    info!("UDP client is connected to {}", server_address);
    let mut recv_buffer = vec![0u8; 65_507];
    let mut last_update = Instant::now();
    let mut update_period = Duration::from_secs_f64(1.0);
    while !stop.load(Ordering::Acquire) {
        while let Ok(client_message) = receiver.try_recv() {
            let buffer = bincode::serialize(&client_message).unwrap();
            socket.send(&buffer).await?;
        }
        let now = Instant::now();
        let recv_timeout = if now - last_update < update_period {
            update_period - (now - last_update)
        } else {
            Duration::from_millis(1)
        };
        if let Ok(Ok(size)) =
            tokio::time::timeout(recv_timeout, socket.recv(&mut recv_buffer)).await
        {
            let decompressed = match decompress_size_prepended(&recv_buffer[0..size]) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to decompress server message: {}", e);
                    continue;
                }
            };
            let server_message: ServerMessage = match bincode::deserialize(&decompressed) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to deserialize server message: {}", e);
                    continue;
                }
            };
            if let ServerMessageData::Settings { update_period: v } = &server_message.data {
                update_period = *v;
            }
            if let Err(e) = sender.send(server_message) {
                debug!("UDP client has failed to send a message: {}", e);
                break;
            }
        }
        last_update = Instant::now();
    }
    info!("UDP client has stopped");
    Ok(())
}

#[derive(Debug)]
pub struct GameClientSettings {
    pub connect_timeout: Duration,
}

pub struct GameChannel {
    pub sender: Sender<GameUpdate>,
    pub receiver: Receiver<PlayerAction>,
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
    info!("Run game client");
    let mut client_message_number = 0;
    let mut server_message_number = 0;
    let session_id = loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        info!("Game client is trying to join server...");
        server
            .sender
            .send(ClientMessage {
                number: client_message_number,
                session_id: 0,
                data: ClientMessageData::Join,
            })
            .ok();
        client_message_number += 1;
        if let Ok(message) = server.receiver.recv_timeout(settings.connect_timeout) {
            if message.number <= server_message_number {
                continue;
            }
            server_message_number = message.number;
            match message.data {
                ServerMessageData::Settings { .. } => break message.session_id,
                ServerMessageData::Error(err) => {
                    error!("Join to server error: {}", err);
                    return;
                }
                v => warn!(
                    "Game client has received invalid server response type: {}",
                    get_server_message_data_type(&v)
                ),
            }
        }
    };
    info!("Joined to server with session {}", session_id);
    let ServerChannel {
        sender: server_sender,
        receiver: server_receiver,
    } = server;
    let GameChannel {
        sender: game_sender,
        receiver: game_receiver,
    } = game;
    let sender = spawn(move || {
        run_server_sender(
            session_id,
            client_message_number,
            server_sender,
            game_receiver,
        )
    });
    run_server_receiver(
        session_id,
        server_message_number,
        game_sender,
        server_receiver,
        stop,
    );
    sender.join().unwrap();
    info!("Game client has stopped for session {}", session_id);
}

fn run_server_sender(
    session_id: u64,
    mut message_number: u64,
    sender: Sender<ClientMessage>,
    receiver: Receiver<PlayerAction>,
) {
    info!("Run server sender for session {}", session_id);
    while let Ok(player_action) = receiver.recv() {
        sender
            .send(ClientMessage {
                session_id,
                number: message_number,
                data: ClientMessageData::PlayerAction(player_action),
            })
            .ok();
        message_number += 1;
    }
    debug!(
        "Server sender is sending quit for session {}...",
        session_id
    );
    if let Err(e) = sender.send(ClientMessage {
        session_id,
        number: message_number,
        data: ClientMessageData::Quit,
    }) {
        warn!(
            "Server sender has failed to send quit for session {}: {}",
            session_id, e
        );
    }
    info!("Server sender has stopped for session {}", session_id);
}

fn run_server_receiver(
    session_id: u64,
    mut message_number: u64,
    sender: Sender<GameUpdate>,
    receiver: Receiver<ServerMessage>,
    stop: Arc<AtomicBool>,
) {
    info!("Run server receiver for session {}", session_id);
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
                    ServerMessageData::Settings { .. } => (),
                    ServerMessageData::Error(error) => {
                        warn!("Server error for session {}: {}", session_id, error);
                    }
                    ServerMessageData::GameUpdate(update) => match sender.send(update) {
                        Ok(..) => (),
                        Err(e) => {
                            debug!(
                                "Server receiver has failed to send a message for session {}: {}",
                                session_id, e
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
    info!("Server receiver has stopped for session {}", session_id);
}
