#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Barrier};
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};

use portpicker::pick_unused_port;

use spell::client::{GameClientSettings, UdpClientSettings};
use spell::protocol::{GameUpdate, PlayerAction};
use spell::{run_server, with_background_client, ServerParams};

#[test]
fn server_should_terminate() {
    init_logger();
    let stop = Arc::new(AtomicBool::new(true));
    let server_params = ServerParams {
        address: String::from("127.0.0.2"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
    };
    run_background_server(server_params, stop).join().unwrap();
}

#[test]
fn server_should_provide_player_id() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.3"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 60.0,
        random_seed: Some(42),
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
        },
        |_action_sender, update_receiver| {
            let game_update = update_receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert!(
                matches!(game_update, GameUpdate::SetPlayerId(..)),
                "{:?}",
                game_update
            );
        },
    );
}

#[test]
fn server_should_move_player() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.4"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 60.0,
        random_seed: Some(42),
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
        },
        |action_sender, update_receiver| {
            let set_player_id = update_receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert!(
                matches!(set_player_id, GameUpdate::SetPlayerId(..)),
                "{:?}",
                set_player_id
            );
            let player_id = if let GameUpdate::SetPlayerId(v) = set_player_id {
                v
            } else {
                unreachable!()
            };
            let start = Instant::now();
            action_sender.send(PlayerAction::Move(true)).unwrap();
            let mut moving = false;
            while !moving && Instant::now() - start < Duration::from_secs(3) {
                let world_update = update_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                assert!(
                    matches!(world_update, GameUpdate::World(..)),
                    "{:?}",
                    world_update
                );
                if let GameUpdate::World(world) = world_update {
                    moving = world
                        .actors
                        .iter()
                        .find(|v| v.id == player_id)
                        .map(|v| v.moving)
                        .unwrap();
                }
            }
            assert!(moving);
        },
    );
}

#[test]
fn server_should_limit_number_of_sessions() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.5"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 11.0,
        game_session_timeout: 10.0,
        update_frequency: 60.0,
        random_seed: Some(42),
    };
    let mut game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
    };
    let mut udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port),
    };
    with_background_server(server_params, || {
        let barrier1 = Arc::new(Barrier::new(2));
        let barrier2 = Arc::new(Barrier::new(2));
        let first_session = {
            let session_game_client_settings = game_client_settings.clone();
            let session_udp_client_settings = udp_client_settings.clone();
            let session_barrier1 = barrier1.clone();
            let session_barrier2 = barrier2.clone();
            spawn(move || {
                with_background_client(
                    session_game_client_settings,
                    session_udp_client_settings,
                    |action_sender, update_receiver| {
                        let game_update = update_receiver
                            .recv_timeout(Duration::from_secs(3))
                            .unwrap();
                        assert!(
                            matches!(game_update, GameUpdate::SetPlayerId(..)),
                            "{:?}",
                            game_update
                        );
                        session_barrier1.wait();
                        session_barrier2.wait();
                        drop(action_sender);
                    },
                );
            })
        };
        barrier1.wait();
        game_client_settings.id = 2;
        udp_client_settings.id = 2;
        with_background_client(
            game_client_settings,
            udp_client_settings,
            |action_sender, update_receiver| {
                assert_eq!(
                    update_receiver.recv_timeout(Duration::from_secs(3)),
                    Err(RecvTimeoutError::Timeout)
                );
                barrier2.wait();
                drop(action_sender);
            },
        );
        first_session.join().unwrap();
    });
}

#[test]
fn server_should_limit_number_of_players() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.6"),
        port: pick_unused_port().unwrap(),
        max_sessions: 2,
        max_players: 1,
        udp_session_timeout: 11.0,
        game_session_timeout: 10.0,
        update_frequency: 60.0,
        random_seed: Some(42),
    };
    let mut game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
    };
    let mut udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port),
    };
    with_background_server(server_params, || {
        let barrier1 = Arc::new(Barrier::new(2));
        let barrier2 = Arc::new(Barrier::new(2));
        let first_session = {
            let session_game_client_settings = game_client_settings.clone();
            let session_udp_client_settings = udp_client_settings.clone();
            let session_barrier1 = barrier1.clone();
            let session_barrier2 = barrier2.clone();
            spawn(move || {
                with_background_client(
                    session_game_client_settings,
                    session_udp_client_settings,
                    |action_sender, update_receiver| {
                        let game_update = update_receiver
                            .recv_timeout(Duration::from_secs(3))
                            .unwrap();
                        assert!(
                            matches!(game_update, GameUpdate::SetPlayerId(..)),
                            "{:?}",
                            game_update
                        );
                        session_barrier1.wait();
                        session_barrier2.wait();
                        drop(action_sender);
                    },
                );
            })
        };
        barrier1.wait();
        game_client_settings.id = 2;
        udp_client_settings.id = 2;
        with_background_client(
            game_client_settings,
            udp_client_settings,
            |action_sender, update_receiver| {
                assert_eq!(
                    update_receiver.recv_timeout(Duration::from_secs(3)),
                    Err(RecvTimeoutError::Disconnected)
                );
                barrier2.wait();
                drop(action_sender);
            },
        );
        first_session.join().unwrap();
    });
}

#[test]
fn server_should_support_multiple_players() {
    init_logger();
    let players_number = 3;
    let server_params = ServerParams {
        address: String::from("127.0.0.7"),
        port: pick_unused_port().unwrap(),
        max_sessions: players_number,
        max_players: players_number,
        udp_session_timeout: 11.0,
        game_session_timeout: 10.0,
        update_frequency: 60.0,
        random_seed: Some(42),
    };
    let game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
    };
    let udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port),
    };
    with_background_server(server_params, || {
        let barrier = Arc::new(Barrier::new(players_number));
        let mut sessions = Vec::with_capacity(players_number);
        for i in 0..players_number {
            let session = {
                let mut session_game_client_settings = game_client_settings.clone();
                let mut session_udp_client_settings = udp_client_settings.clone();
                let session_barrier = barrier.clone();
                session_game_client_settings.id = i as u64 + 1;
                session_udp_client_settings.id = i as u64 + 1;
                spawn(move || {
                    with_background_client(
                        session_game_client_settings,
                        session_udp_client_settings,
                        |action_sender, update_receiver| {
                            let game_update = update_receiver
                                .recv_timeout(Duration::from_secs(3))
                                .unwrap();
                            assert!(
                                matches!(game_update, GameUpdate::SetPlayerId(..)),
                                "{:?}",
                                game_update
                            );
                            session_barrier.wait();
                            drop(action_sender);
                        },
                    );
                })
            };
            sessions.push(session);
        }
        sessions.into_iter().for_each(|v| v.join().unwrap());
    });
}

fn init_logger() {
    env_logger::try_init().ok();
}

fn with_background_server_and_client<F>(
    server_params: ServerParams,
    game_client_settings: GameClientSettings,
    f: F,
) where
    F: FnOnce(Sender<PlayerAction>, Receiver<GameUpdate>),
{
    let upd_client_settings = UdpClientSettings {
        id: game_client_settings.id,
        server_address: format!("{}:{}", server_params.address, server_params.port),
    };
    let w = move || {
        with_background_client(game_client_settings, upd_client_settings, f);
    };
    with_background_server(server_params, w);
}

fn with_background_server<F: FnOnce()>(params: ServerParams, f: F) {
    let stop = Arc::new(AtomicBool::new(false));
    let server = run_background_server(params, stop.clone());
    f();
    info!("Stopping server...");
    stop.store(true, Ordering::Release);
    server.join().unwrap();
}

fn run_background_server(params: ServerParams, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    spawn(move || run_server(params, stop))
}
