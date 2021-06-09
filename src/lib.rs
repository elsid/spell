#[cfg(any(feature = "client", feature = "server"))]
#[macro_use]
extern crate log;

#[cfg(feature = "client")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "client")]
use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(feature = "client")]
use std::sync::Arc;
#[cfg(feature = "client")]
use std::thread::{spawn, JoinHandle};
#[cfg(feature = "client")]
use std::time::Duration;

#[cfg(feature = "client")]
use clap::Clap;

#[cfg(feature = "client")]
use crate::client::{
    run_game_client, run_udp_client, GameChannel, GameClientSettings, ServerChannel,
    UdpClientSettings,
};
#[cfg(feature = "client")]
use crate::engine::get_next_id;
#[cfg(feature = "client")]
use crate::game::{run_game, Server};
#[cfg(feature = "client")]
use crate::generators::{generate_player_actor, generate_world, make_rng};
#[cfg(feature = "client")]
use crate::protocol::{
    is_valid_player_name, ClientMessage, GameUpdate, PlayerUpdate, ServerMessage,
    MAX_PLAYER_NAME_LEN, MIN_PLAYER_NAME_LEN,
};
#[cfg(feature = "client")]
use crate::rect::Rectf;
#[cfg(feature = "client")]
use crate::vec2::Vec2f;
#[cfg(feature = "client")]
use crate::world::World;

#[cfg(feature = "client")]
pub mod client;
#[cfg(any(feature = "client", feature = "server"))]
mod control;
#[cfg(any(feature = "client", feature = "server"))]
mod engine;
#[cfg(feature = "client")]
mod game;
#[cfg(any(feature = "client", feature = "server"))]
mod generators;
#[cfg(feature = "client")]
mod meters;
#[cfg(any(feature = "client", feature = "server"))]
pub mod protocol;
#[cfg(any(feature = "client", feature = "server"))]
mod rect;
#[cfg(feature = "server")]
pub mod server;
#[cfg(any(feature = "client", feature = "server"))]
mod vec2;
#[cfg(any(feature = "client", feature = "server"))]
mod world;

#[cfg(feature = "client")]
#[derive(Clap, Debug)]
pub struct SinglePlayerParams {
    #[clap(long)]
    pub random_seed: Option<u64>,
}

#[cfg(feature = "client")]
#[derive(Clap, Debug)]
pub struct MultiPlayerParams {
    pub server_address: String,
    pub player_name: String,
    #[clap(long, default_value = "21227")]
    pub server_port: u16,
    #[clap(long, default_value = "3")]
    pub connect_timeout: f64,
    #[clap(long, default_value = "0.25")]
    pub retry_period: f64,
}

#[cfg(feature = "client")]
pub fn run_single_player(params: SinglePlayerParams) {
    info!("Run single player: {:?}", params);
    let mut rng = make_rng(params.random_seed);
    let mut world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), &mut rng);
    let id = get_next_id(&mut world.id_counter);
    world.actors.push(generate_player_actor(
        id,
        &world.bounds,
        format!("{}", id),
        &mut rng,
    ));
    let (sender, receiver) = channel();
    sender.send(GameUpdate::SetActorId(id)).unwrap();
    run_game(world, None, receiver);
    info!("Exit single player");
}

#[cfg(feature = "client")]
pub fn run_multi_player(params: MultiPlayerParams) {
    info!("Run multiplayer: {:?}", params);
    if !is_valid_player_name(params.player_name.as_str()) {
        error!("Player name should contain only alphabetic characters, be at least {} and not longer than {} symbols", MIN_PLAYER_NAME_LEN, MAX_PLAYER_NAME_LEN);
        return;
    }
    let server_address = params.server_address;
    let server_port = params.server_port;
    with_background_client(
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs_f64(params.connect_timeout),
            retry_period: Duration::from_secs_f64(params.retry_period),
            player_name: params.player_name,
        },
        UdpClientSettings {
            id: 1,
            server_address: format!("{}:{}", server_address, server_port)
                .parse()
                .unwrap(),
        },
        move |action_sender, update_receiver| {
            run_game(
                World::default(),
                Some(Server {
                    address: server_address,
                    port: server_port,
                    sender: action_sender,
                }),
                update_receiver,
            );
        },
    );
    info!("Exit multiplayer");
}

#[cfg(feature = "client")]
pub fn with_background_client<F>(
    game_client_settings: GameClientSettings,
    udp_client_settings: UdpClientSettings,
    f: F,
) where
    F: FnOnce(Sender<PlayerUpdate>, Receiver<GameUpdate>),
{
    let w = move |client_sender, server_receiver| {
        with_background_game_client(game_client_settings, client_sender, server_receiver, f);
    };
    with_background_udp_client(udp_client_settings, w);
}

#[cfg(feature = "client")]
pub fn with_background_game_client<F>(
    settings: GameClientSettings,
    client_sender: Sender<ClientMessage>,
    server_receiver: Receiver<ServerMessage>,
    f: F,
) where
    F: FnOnce(Sender<PlayerUpdate>, Receiver<GameUpdate>),
{
    let (update_sender, update_receiver) = channel();
    let (action_sender, action_receiver) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let id = settings.id;
    let game_client = run_background_game_client(
        settings,
        update_sender,
        action_receiver,
        client_sender,
        server_receiver,
        stop.clone(),
    );
    f(action_sender, update_receiver);
    info!("[{}] Stopping game client...", id);
    stop.store(true, Ordering::Release);
    game_client.join().unwrap();
}

#[cfg(feature = "client")]
pub fn with_background_udp_client<F>(settings: UdpClientSettings, f: F)
where
    F: FnOnce(Sender<ClientMessage>, Receiver<ServerMessage>),
{
    let (server_sender, server_receiver) = channel();
    let (client_sender, client_receiver) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let id = settings.id;
    let udp_client =
        run_background_udp_client(settings, server_sender, client_receiver, stop.clone());
    f(client_sender, server_receiver);
    info!("[{}] Stopping UDP client...", id);
    stop.store(true, Ordering::Release);
    udp_client.join().unwrap();
}

#[cfg(feature = "client")]
pub fn run_background_game_client(
    settings: GameClientSettings,
    update_sender: Sender<GameUpdate>,
    action_receiver: Receiver<PlayerUpdate>,
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

#[cfg(feature = "client")]
pub fn run_background_udp_client(
    settings: UdpClientSettings,
    server_sender: Sender<ServerMessage>,
    client_receiver: Receiver<ClientMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
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
