#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Barrier};
use std::thread::{sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

use portpicker::pick_unused_port;
use reqwest::blocking::{RequestBuilder, Response};

use spell::client::{Client, GameClientSettings, UdpClientSettings};
use spell::protocol::{
    apply_world_update, ActorAction, CastAction, GameUpdate, HttpMessage, PlayerControl,
    ServerStatus,
};
use spell::server::{run_server, ServerParams};
use spell::vec2::Vec2f;
use spell::world::{Actor, Element, PlayerId, World};

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
        http_address: String::from("127.0.0.2"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
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
        http_address: String::from("127.0.0.3"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
            player_name: String::from("test"),
        },
        |_, game_update_receiver| {
            let game_update = game_update_receiver
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
        http_address: String::from("127.0.0.4"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
            player_name: String::from("test"),
        },
        |player_control_sender, game_update_receiver| {
            let player_id = recv_player_id(game_update_receiver);
            let start = Instant::now();
            let mut moving = false;
            let mut world = World::default();
            while !moving && Instant::now() - start < Duration::from_secs(3) {
                player_control_sender
                    .send(PlayerControl {
                        ack_world_frame: world.frame,
                        cast_action_world_frame: 0,
                        actor_action: ActorAction {
                            moving: true,
                            target_direction: Vec2f::ZERO,
                            cast_action: None,
                        },
                    })
                    .unwrap();
                match game_update_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap()
                {
                    GameUpdate::WorldUpdate { world_update, .. } => {
                        apply_world_update(*world_update, &mut world);
                    }
                    GameUpdate::WorldSnapshot { world: w, .. } => {
                        world = *w;
                    }
                    _ => (),
                }
                if let Some(actor) = find_player_actor(player_id, &world) {
                    moving = actor.moving;
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
        http_address: String::from("127.0.0.5"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    let mut game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
        player_name: String::from("test"),
    };
    let mut udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port)
            .parse()
            .unwrap(),
        read_timeout: Duration::from_secs(3),
    };
    with_background_server(server_params, |_| {
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
                    |_, game_update_receiver| {
                        let game_update = game_update_receiver
                            .recv_timeout(Duration::from_secs(3))
                            .unwrap();
                        assert!(
                            matches!(game_update, GameUpdate::SetPlayerId(..)),
                            "{:?}",
                            game_update
                        );
                        session_barrier1.wait();
                        session_barrier2.wait();
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
            |_, game_update_receiver| {
                assert_eq!(
                    game_update_receiver.recv_timeout(Duration::from_secs(3)),
                    Err(RecvTimeoutError::Timeout)
                );
                barrier2.wait();
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
        http_address: String::from("127.0.0.6"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    let mut game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
        player_name: String::from("test"),
    };
    let mut udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port)
            .parse()
            .unwrap(),
        read_timeout: Duration::from_secs(3),
    };
    with_background_server(server_params, |_| {
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
                    |_, game_update_receiver| {
                        let game_update = game_update_receiver
                            .recv_timeout(Duration::from_secs(3))
                            .unwrap();
                        assert!(
                            matches!(game_update, GameUpdate::SetPlayerId(..)),
                            "{:?}",
                            game_update
                        );
                        session_barrier1.wait();
                        session_barrier2.wait();
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
            |_, game_update_receiver| {
                assert_eq!(
                    game_update_receiver.recv_timeout(Duration::from_secs(3)),
                    Err(RecvTimeoutError::Disconnected)
                );
                barrier2.wait();
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
        http_address: String::from("127.0.0.7"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    let game_client_settings = GameClientSettings {
        id: 1,
        connect_timeout: Duration::from_secs(3),
        retry_period: Duration::from_secs_f64(0.25),
        player_name: String::from("test"),
    };
    let udp_client_settings = UdpClientSettings {
        id: 1,
        server_address: format!("{}:{}", server_params.address, server_params.port)
            .parse()
            .unwrap(),
        read_timeout: Duration::from_secs(3),
    };
    with_background_server(server_params, |_| {
        let barrier = Arc::new(Barrier::new(players_number));
        let mut sessions = Vec::with_capacity(players_number);
        for i in 0..players_number {
            let session = {
                let mut session_game_client_settings = game_client_settings.clone();
                let mut session_udp_client_settings = udp_client_settings.clone();
                let session_barrier = barrier.clone();
                session_game_client_settings.id = i as u64 + 1;
                session_game_client_settings.player_name =
                    format!("test{}", (b'a' + i as u8) as char);
                session_udp_client_settings.id = i as u64 + 1;
                spawn(move || {
                    with_background_client(
                        session_game_client_settings,
                        session_udp_client_settings,
                        |_, game_update_receiver| {
                            let game_update = game_update_receiver
                                .recv_timeout(Duration::from_secs(3))
                                .unwrap();
                            assert!(
                                matches!(game_update, GameUpdate::SetPlayerId(..)),
                                "{:?}",
                                game_update
                            );
                            session_barrier.wait();
                        },
                    );
                })
            };
            sessions.push(session);
        }
        sessions.into_iter().for_each(|v| v.join().unwrap());
    });
}

#[test]
fn server_should_send_world_update_after_ack() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.8"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 60.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.8"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
            player_name: String::from("test"),
        },
        |player_control_sender, game_update_receiver| {
            let mut last_server_message = game_update_receiver
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert!(
                matches!(last_server_message, GameUpdate::SetPlayerId(..)),
                "{:?}",
                last_server_message
            );
            let start = Instant::now();
            while Instant::now() - start < Duration::from_secs(3) {
                last_server_message = game_update_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                match &last_server_message {
                    GameUpdate::WorldUpdate { .. } => break,
                    GameUpdate::WorldSnapshot { world, .. } => {
                        player_control_sender
                            .send(PlayerControl {
                                ack_world_frame: world.frame,
                                cast_action_world_frame: 0,
                                actor_action: ActorAction::default(),
                            })
                            .unwrap();
                    }
                    _ => (),
                }
            }
            assert!(
                matches!(last_server_message, GameUpdate::WorldUpdate { .. }),
                "{:?}",
                last_server_message
            );
        },
    );
}

#[test]
fn server_should_response_to_http_ping() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.9"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.9"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        assert_eq!(http_client.ping(), HttpMessage::Ok);
    });
}

#[test]
fn server_should_response_to_http_status() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.10"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.10"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        let result = http_client.status();
        assert!(
            matches!(
                result,
                HttpMessage::Status {
                    status: ServerStatus { sessions: 0, .. }
                }
            ),
            "{:?}",
            result
        );
    });
}

#[test]
fn server_should_response_to_http_sessions() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.11"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.11"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        assert_eq!(
            http_client.sessions(),
            HttpMessage::Sessions {
                sessions: Vec::new()
            }
        );
    });
}

#[test]
fn server_should_response_to_http_remove_session() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.12"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.12"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        assert_eq!(
            http_client.remove_session(1),
            HttpMessage::Error {
                message: String::from("Session is not found")
            }
        );
    });
}

#[test]
fn server_should_response_to_http_world() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.12"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.12"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        let result = http_client.world();
        assert!(matches!(result, HttpMessage::World { .. }), "{:?}", result);
    });
}

#[test]
fn server_should_response_to_http_stop() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.13"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 1.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.13"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server(server_params, |http_client| {
        assert_eq!(http_client.stop(), HttpMessage::Ok);
    });
}

#[test]
fn server_should_add_spell_on_client_request() {
    init_logger();
    let server_params = ServerParams {
        address: String::from("127.0.0.14"),
        port: pick_unused_port().unwrap(),
        max_sessions: 1,
        max_players: 1,
        udp_session_timeout: 4.0,
        game_session_timeout: 3.0,
        update_frequency: 60.0,
        random_seed: Some(42),
        http_address: String::from("127.0.0.14"),
        http_port: pick_unused_port().unwrap(),
        http_max_connections: 1,
        world: None,
    };
    with_background_server_and_client(
        server_params,
        GameClientSettings {
            id: 1,
            connect_timeout: Duration::from_secs(3),
            retry_period: Duration::from_secs_f64(0.25),
            player_name: String::from("test"),
        },
        |player_control_sender, game_update_receiver| {
            let player_id = recv_player_id(game_update_receiver);
            let start = Instant::now();
            let mut world = World::default();
            let mut spell_elements = Vec::new();
            let mut cast_action_world_frame = 0;
            while Instant::now() - start < Duration::from_secs(3) && spell_elements.is_empty() {
                let server_message = game_update_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                match server_message {
                    GameUpdate::WorldUpdate { world_update, .. } => {
                        apply_world_update(*world_update, &mut world);
                    }
                    GameUpdate::WorldSnapshot { world: w, .. } => {
                        world = *w;
                    }
                    _ => (),
                }
                let cast_action = if cast_action_world_frame == 0 {
                    cast_action_world_frame = world.frame;
                    None
                } else {
                    Some(CastAction::AddSpellElement(Element::Earth))
                };
                player_control_sender
                    .send(PlayerControl {
                        ack_world_frame: world.frame,
                        cast_action_world_frame,
                        actor_action: ActorAction {
                            moving: false,
                            target_direction: Vec2f::ZERO,
                            cast_action,
                        },
                    })
                    .unwrap();
                if let Some(actor) = find_player_actor(player_id, &world) {
                    spell_elements = actor.spell_elements.clone();
                }
            }
            assert_eq!(spell_elements, vec![Element::Earth]);
        },
    );
}

fn init_logger() {
    env_logger::try_init().ok();
}

fn with_background_server_and_client<F>(
    server_params: ServerParams,
    game_client_settings: GameClientSettings,
    f: F,
) where
    F: FnOnce(&Sender<PlayerControl>, &Receiver<GameUpdate>),
{
    let upd_client_settings = UdpClientSettings {
        id: game_client_settings.id,
        server_address: format!("{}:{}", server_params.address, server_params.port)
            .parse()
            .unwrap(),
        read_timeout: Duration::from_secs(3),
    };
    let w = move |_| {
        with_background_client(game_client_settings, upd_client_settings, f);
    };
    with_background_server(server_params, w);
}

fn with_background_client<F>(
    game_client_settings: GameClientSettings,
    udp_client_settings: UdpClientSettings,
    f: F,
) where
    F: FnOnce(&Sender<PlayerControl>, &Receiver<GameUpdate>),
{
    let client = Client::new(game_client_settings, udp_client_settings);
    f(client.sender(), client.receiver());
}

fn with_background_server<F: FnOnce(HttpClient)>(params: ServerParams, f: F) {
    let stop = Arc::new(AtomicBool::new(false));
    let http_server_address = params.http_address.clone();
    let http_server_port = params.http_port;
    let server = run_background_server(params, stop.clone());
    f(HttpClient::new(http_server_address, http_server_port));
    info!("Stopping server...");
    stop.store(true, Ordering::Release);
    server.join().unwrap();
}

fn run_background_server(params: ServerParams, stop: Arc<AtomicBool>) -> JoinHandle<()> {
    spawn(move || run_server(params, stop))
}

struct HttpClient {
    address: String,
    port: u16,
    client: reqwest::blocking::Client,
}

impl HttpClient {
    fn new(address: String, port: u16) -> Self {
        Self {
            address,
            port,
            client: reqwest::blocking::Client::builder().build().unwrap(),
        }
    }

    fn ping(&self) -> HttpMessage {
        send_with_retries(
            self.client
                .get(self.url("ping").as_str())
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn status(&self) -> HttpMessage {
        send_with_retries(
            self.client
                .get(self.url("status").as_str())
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn sessions(&self) -> HttpMessage {
        send_with_retries(
            self.client
                .get(self.url("sessions").as_str())
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn remove_session(&self, session_id: u64) -> HttpMessage {
        send_with_retries(
            self.client
                .post(self.url("remove_session").as_str())
                .query(&[("session_id", session_id)])
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn world(&self) -> HttpMessage {
        send_with_retries(
            self.client
                .get(self.url("world").as_str())
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn stop(&self) -> HttpMessage {
        send_with_retries(
            self.client
                .post(self.url("stop").as_str())
                .timeout(Duration::from_secs(5)),
        )
        .json()
        .unwrap()
    }

    fn url(&self, endpoint: &str) -> String {
        format!("http://{}:{}/{}", self.address, self.port, endpoint)
    }
}

fn send_with_retries(request: RequestBuilder) -> Response {
    let mut try_num: usize = 0;
    loop {
        let result = request.try_clone().unwrap().send();
        if let Ok(v) = result {
            return v;
        }
        try_num += 1;
        if try_num >= 3 {
            result.unwrap();
        }
        sleep(Duration::from_millis(100));
    }
}

fn recv_player_id(game_update_receiver: &Receiver<GameUpdate>) -> PlayerId {
    let set_player_id = game_update_receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(
        matches!(set_player_id, GameUpdate::SetPlayerId(..)),
        "{:?}",
        set_player_id
    );
    if let GameUpdate::SetPlayerId(v) = set_player_id {
        v
    } else {
        unreachable!()
    }
}

fn find_player_actor(player_id: PlayerId, world: &World) -> Option<&Actor> {
    world.actors.iter().find(|v| v.player_id == player_id)
}
