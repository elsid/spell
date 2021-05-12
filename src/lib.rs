#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
#[cfg(feature = "render")]
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::spawn;
#[cfg(feature = "render")]
use std::thread::JoinHandle;
use std::time::Duration;

use clap::Clap;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use tokio::runtime::Builder;

#[cfg(feature = "render")]
use crate::client::{
    run_game_client, run_udp_client, GameChannel, GameClientSettings, ServerChannel,
    UdpClientSettings,
};
#[cfg(feature = "render")]
use crate::engine::get_next_id;
#[cfg(feature = "render")]
use crate::game::{run_game, Server};
#[cfg(feature = "render")]
use crate::generators::generate_player_actor;
use crate::generators::generate_world;
#[cfg(feature = "render")]
use crate::protocol::{ClientMessage, GameUpdate, PlayerAction, ServerMessage};
use crate::rect::Rectf;
use crate::server::{run_game_server, run_udp_server, GameServerSettings, UdpServerSettings};
use crate::vec2::Vec2f;
#[cfg(feature = "render")]
use crate::world::World;

#[cfg(feature = "render")]
mod client;
mod control;
mod engine;
#[cfg(feature = "render")]
mod game;
mod generators;
#[cfg(feature = "render")]
mod meters;
mod protocol;
mod rect;
mod server;
mod vec2;
mod world;

#[cfg(feature = "render")]
#[derive(Clap, Debug)]
pub struct SinglePlayerParams {
    #[clap(long)]
    pub random_seed: Option<u64>,
}

#[cfg(feature = "render")]
#[derive(Clap, Debug)]
pub struct MultiPlayerParams {
    pub server_address: String,
    #[clap(long, default_value = "21227")]
    pub server_port: u16,
    #[clap(long, default_value = "3")]
    pub connect_timeout: f64,
    #[clap(long, default_value = "0.25")]
    pub retry_period: f64,
}

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
}

#[cfg(feature = "render")]
pub fn run_single_player(params: SinglePlayerParams) {
    info!("Run single player: {:?}", params);
    let mut rng = make_rng(params.random_seed);
    let mut world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
    let id = get_next_id(&mut world.id_counter);
    world
        .actors
        .push(generate_player_actor(id, &world.bounds, &mut rng));
    let (sender, receiver) = channel();
    sender.send(GameUpdate::SetPlayerId(id)).unwrap();
    run_game(world, None, receiver);
    info!("Exit single player");
}

#[cfg(feature = "render")]
pub fn run_multi_player(params: MultiPlayerParams) {
    info!("Run multiplayer: {:?}", params);
    with_background_client(
        GameClientSettings {
            connect_timeout: Duration::from_secs_f64(params.connect_timeout),
            retry_period: Duration::from_secs_f64(params.retry_period),
        },
        UdpClientSettings {
            server_address: format!("{}:{}", params.server_address, params.server_port),
        },
        move |action_sender, update_receiver| {
            run_game(
                World::default(),
                Some(Server {
                    address: params.server_address,
                    port: params.server_port,
                    sender: action_sender,
                }),
                update_receiver,
            );
        },
    );
    info!("Exit multiplayer");
}

#[cfg(feature = "render")]
pub fn with_background_client<F>(
    game_client_settings: GameClientSettings,
    udp_client_settings: UdpClientSettings,
    f: F,
) where
    F: FnOnce(Sender<PlayerAction>, Receiver<GameUpdate>),
{
    let w = move |client_sender, server_receiver| {
        with_background_game_client(game_client_settings, client_sender, server_receiver, f);
    };
    with_background_udp_client(udp_client_settings, w);
}

#[cfg(feature = "render")]
pub fn with_background_game_client<F>(
    settings: GameClientSettings,
    client_sender: Sender<ClientMessage>,
    server_receiver: Receiver<ServerMessage>,
    f: F,
) where
    F: FnOnce(Sender<PlayerAction>, Receiver<GameUpdate>),
{
    let (update_sender, update_receiver) = channel();
    let (action_sender, action_receiver) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let game_client = run_background_game_client(
        settings,
        update_sender,
        action_receiver,
        client_sender,
        server_receiver,
        stop.clone(),
    );
    f(action_sender, update_receiver);
    info!("Stopping game client...");
    stop.store(true, Ordering::Release);
    game_client.join().unwrap();
}

#[cfg(feature = "render")]
pub fn with_background_udp_client<F>(settings: UdpClientSettings, f: F)
where
    F: FnOnce(Sender<ClientMessage>, Receiver<ServerMessage>),
{
    let (server_sender, server_receiver) = channel();
    let (client_sender, client_receiver) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let udp_client =
        run_background_udp_client(settings, server_sender, client_receiver, stop.clone());
    f(client_sender, server_receiver);
    info!("Stopping UDP client...");
    stop.store(true, Ordering::Release);
    udp_client.join().unwrap();
}

#[cfg(feature = "render")]
pub fn run_background_game_client(
    settings: GameClientSettings,
    update_sender: Sender<GameUpdate>,
    action_receiver: Receiver<PlayerAction>,
    client_sender: Sender<ClientMessage>,
    server_receiver: Receiver<ServerMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let game_channel = GameChannel {
        sender: update_sender,
        receiver: action_receiver,
    };
    let server_channel = ServerChannel {
        sender: client_sender,
        receiver: server_receiver,
    };
    spawn(move || run_game_client(settings, server_channel, game_channel, stop))
}

#[cfg(feature = "render")]
pub fn run_background_udp_client(
    settings: UdpClientSettings,
    server_sender: Sender<ServerMessage>,
    client_receiver: Receiver<ClientMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    spawn(move || {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        runtime
            .block_on(run_udp_client(
                settings,
                server_sender,
                client_receiver,
                stop,
            ))
            .unwrap();
    })
}

pub fn run_server(params: ServerParams, stop: Arc<AtomicBool>) {
    info!("Run server: {:?}", params);
    if params.udp_session_timeout < params.game_session_timeout {
        warn!(
            "UDP server session timeout {:?} is less than game session timeout {:?}",
            params.udp_session_timeout, params.game_session_timeout
        );
    }
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let world = generate_world(
        Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)),
        &mut make_rng(params.random_seed),
    );
    let (server_sender, server_receiver) = channel();
    let (client_sender, client_receiver) = channel();
    let stop_game_server = Arc::new(AtomicBool::new(false));
    let update_period = Duration::from_secs_f64(1.0 / params.update_frequency);
    let server = {
        let settings = GameServerSettings {
            max_players: params.max_players,
            update_period,
            session_timeout: Duration::from_secs_f64(params.game_session_timeout),
        };
        let stop = stop_game_server.clone();
        spawn(move || run_game_server(world, settings, server_sender, client_receiver, stop))
    };
    let settings = UdpServerSettings {
        address: format!("{}:{}", params.address, params.port),
        max_sessions: params.max_sessions,
        update_period,
        session_timeout: Duration::from_secs_f64(params.udp_session_timeout),
    };
    runtime
        .block_on(run_udp_server(
            settings,
            client_sender,
            server_receiver,
            stop,
        ))
        .unwrap();
    info!("Stopping game server...");
    stop_game_server.store(true, Ordering::Release);
    server.join().unwrap();
    info!("Exit server");
}

fn make_rng(random_seed: Option<u64>) -> SmallRng {
    if let Some(value) = random_seed {
        SeedableRng::seed_from_u64(value)
    } else {
        SeedableRng::from_entropy()
    }
}
