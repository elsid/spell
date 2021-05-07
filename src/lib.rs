#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::spawn;
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
use crate::protocol::GameUpdate;
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
    let (update_sender, update_receiver) = channel();
    let (action_sender, action_receiver) = channel();
    let (server_sender, server_receiver) = channel();
    let (client_sender, client_receiver) = channel();
    let stop_game_client = Arc::new(AtomicBool::new(false));
    let game_client = {
        let settings = GameClientSettings {
            connect_timeout: Duration::from_secs_f64(params.connect_timeout),
        };
        let game_channel = GameChannel {
            sender: update_sender,
            receiver: action_receiver,
        };
        let server_channel = ServerChannel {
            sender: client_sender,
            receiver: server_receiver,
        };
        let stop = stop_game_client.clone();
        spawn(move || run_game_client(settings, server_channel, game_channel, stop))
    };
    let settings = UdpClientSettings {
        server_address: format!("{}:{}", params.server_address, params.server_port),
    };
    let stop_udp_client = Arc::new(AtomicBool::new(false));
    let udp_server = {
        let stop = stop_udp_client.clone();
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
    };
    run_game(
        World::default(),
        Some(Server {
            address: params.server_address,
            port: params.server_port,
            sender: action_sender,
        }),
        update_receiver,
    );
    info!("Stopping game client...");
    stop_game_client.store(true, Ordering::Release);
    game_client.join().unwrap();
    info!("Stopping UDP client...");
    stop_udp_client.store(true, Ordering::Release);
    udp_server.join().unwrap();
    info!("Exit multiplayer");
}

pub fn run_server(params: ServerParams) {
    info!("Run server: {:?}", params);
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
            Arc::new(AtomicBool::new(false)),
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
