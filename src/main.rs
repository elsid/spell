#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::spawn;
use std::time::Duration;

use clap::Clap;
use rand::thread_rng;
use tokio::runtime::Builder;

#[cfg(feature = "render")]
use crate::client::{
    run_game_client, run_udp_client, GameChannel, GameClientSettings, ServerChannel,
    UdpClientSettings,
};
#[cfg(feature = "render")]
use crate::engine::get_next_id;
#[cfg(feature = "render")]
use crate::game::run_game;
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

#[derive(Clap)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Clap)]
enum Command {
    #[cfg(feature = "render")]
    SinglePlayer,
    #[cfg(feature = "render")]
    MultiPlayer(MultiPlayerParams),
    Server(ServerParams),
}

#[cfg(feature = "render")]
#[derive(Clap)]
struct MultiPlayerParams {
    server_address: String,
    #[clap(long, default_value = "21227")]
    server_port: u16,
    #[clap(long, default_value = "3")]
    connect_timeout: f64,
}

#[derive(Clap)]
struct ServerParams {
    #[clap(long, default_value = "127.0.0.1")]
    address: String,
    #[clap(long, default_value = "21227")]
    port: u16,
    #[clap(long, default_value = "20")]
    max_sessions: usize,
    #[clap(long, default_value = "10")]
    max_players: usize,
    #[clap(long, default_value = "11")]
    udp_session_timeout: f64,
    #[clap(long, default_value = "10")]
    game_session_timeout: f64,
    #[clap(long, default_value = "60")]
    update_frequency: f64,
}

fn main() {
    env_logger::init();
    match Args::parse().command {
        #[cfg(feature = "render")]
        Command::SinglePlayer => run_single_player(),
        #[cfg(feature = "render")]
        Command::MultiPlayer(params) => run_multi_player(params),
        Command::Server(params) => run_server(params),
    }
}

#[cfg(feature = "render")]
fn run_single_player() {
    let mut rng = thread_rng();
    let mut world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
    let id = get_next_id(&mut world.id_counter);
    world
        .actors
        .push(generate_player_actor(id, &world.bounds, &mut rng));
    let (sender, receiver) = channel();
    sender.send(GameUpdate::SetPlayerId(id)).unwrap();
    run_game(world, None, receiver);
}

#[cfg(feature = "render")]
fn run_multi_player(params: MultiPlayerParams) {
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
    run_game(World::default(), Some(action_sender), update_receiver);
    stop_game_client.store(true, Ordering::Release);
    game_client.join().unwrap();
    stop_udp_client.store(true, Ordering::Release);
    udp_server.join().unwrap();
}

fn run_server(params: ServerParams) {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let world = generate_world(
        Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)),
        &mut thread_rng(),
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
    stop_game_server.store(true, Ordering::Release);
    server.join().unwrap();
}
