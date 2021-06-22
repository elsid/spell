use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clap::Clap;
use egui::{Color32, CtxRef};
use macroquad::prelude::{
    clear_background, draw_line, draw_poly, draw_rectangle, draw_rectangle_lines, draw_text_ex,
    get_internal_gl, is_key_down, is_key_pressed, is_mouse_button_down, load_ttf_font,
    measure_text, mouse_position_local, mouse_wheel, next_frame, screen_height, screen_width,
    set_camera, set_default_camera, vec2, Camera2D, Color, DrawMode, Font, KeyCode, Mat4,
    MouseButton, Quat, TextParams, Vec3, Vertex, BLACK, WHITE,
};
use rand::prelude::SmallRng;
use rand::Rng;
use yata::methods::{StDev, SMA};
use yata::prelude::Method;

use crate::client::{Client, GameClientSettings, UdpClientSettings};
use crate::control::{apply_actor_action, apply_cast_action};
use crate::engine::{get_next_id, normalize_angle, Engine};
use crate::generators::{generate_world, make_rng};
use crate::meters::{measure, DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{
    apply_world_update, is_valid_player_name, ActorAction, CastAction, GameUpdate, PlayerControl,
    WorldUpdate, MAX_PLAYER_NAME_LEN, MIN_PLAYER_NAME_LEN,
};
use crate::rect::Rectf;
use crate::vec2::Vec2f;
use crate::world::{
    Aura, DelayedMagickStatus, Disk, Element, Material, Player, PlayerId, RingSector, StaticShape,
    World,
};

const NAME_FONT_SIZE: u16 = 36;
const NAME_FONT_SCALE: f32 = 0.03;
const BORDER_FACTOR: f64 = 0.85;
const HALF_WIDTH: f64 = 0.66;
const HALF_HEIGHT: f64 = 0.1;
const BORDER_WIDTH: f64 = HALF_HEIGHT * (1.0 - BORDER_FACTOR);
const HUD_ELEMENT_RADIUS: f64 = 32.0;
const HUD_ELEMENT_WIDTH: f64 = HUD_ELEMENT_RADIUS * 2.2;
const HUD_ELEMENT_BORDER_WIDTH: f64 = HUD_ELEMENT_RADIUS * (1.0 - BORDER_FACTOR);
const HUD_MARGIN: f64 = 12.0;
const HUD_FONT_SIZE: u16 = 24;
const MESSAGE_FONT_SIZE: u16 = 48;

#[derive(Clap, Debug)]
pub struct GameSettings {
    #[clap(long)]
    pub random_seed: Option<u64>,
    #[clap(long, default_value = "127.0.0.1")]
    pub default_server_address: String,
    #[clap(long, default_value = "21227")]
    pub default_server_port: u16,
    #[clap(long, default_value = "Player")]
    pub default_player_name: String,
    #[clap(long, default_value = "3")]
    pub connect_timeout: f64,
    #[clap(long, default_value = "3")]
    pub read_timeout: f64,
    #[clap(long, default_value = "0.25")]
    pub retry_period: f64,
    #[clap(long, default_value = "15")]
    pub max_world_frame_delay: u64,
    #[clap(long, default_value = "0")]
    pub world_updates_delay: usize,
}

struct GameState {
    rng: SmallRng,
    fps: FpsMovingAverage,
    input_duration: DurationMovingAverage,
    update_duration: DurationMovingAverage,
    ui_update_duration: DurationMovingAverage,
    ui_draw_duration: DurationMovingAverage,
    draw_duration: DurationMovingAverage,
    debug_hud_duration: DurationMovingAverage,
    draw_ui: bool,
    show_debug_hud: bool,
    menu: Menu,
    next_client_id: u64,
    server_address: String,
    server_port: u16,
    player_name: String,
    connect_timeout: Duration,
    read_timeout: Duration,
    retry_period: Duration,
    client_dropper: Dropper<Client>,
    debug_hud_font: Font,
    name_font: Font,
    max_world_frame_delay: u64,
    world_updates_delay: usize,
    control_hud_font: Font,
    show_control_hud: bool,
    message_font: Font,
    show_player_list: bool,
    player_list_font: Font,
}

enum Menu {
    None,
    Main,
    Multiplayer,
    Joining,
    Error(String),
}

enum FrameType {
    Initial,
    SinglePlayer(Box<Scene>),
    Multiplayer(Box<Multiplayer>),
    None,
}

struct Scene {
    time_step: f64,
    world: Box<World>,
    engine: Engine,
    player_id: Option<PlayerId>,
    actor_id: Option<u64>,
    actor_index: Option<usize>,
    camera_zoom: f64,
    camera_target: Vec2f,
    pointer: Vec2f,
}

struct Multiplayer {
    client: AsyncDrop<Client>,
    scene: Scene,
    local_world_frame: u64,
    local_world_time: f64,
    world_updates: VecDeque<Box<WorldUpdate>>,
    world_frame_delay: SMA,
    world_frame_diff: SMA,
    world_frame_st_dev: StDev,
    last_world_frame_st_dev: f64,
    input_delay: SMA,
    ack_cast_action_world_frame: u64,
    actor_action: ActorAction,
    delayed_cast_actions: VecDeque<CastAction>,
}

pub async fn run_game(settings: GameSettings) {
    let ubuntu_mono = load_ttf_font("fonts/UbuntuMono-R.ttf").await.unwrap();
    let mut game_state = GameState {
        rng: make_rng(settings.random_seed),
        fps: FpsMovingAverage::new(100, Duration::from_secs(1)),
        input_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        update_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        ui_update_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        ui_draw_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        draw_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        debug_hud_duration: DurationMovingAverage::new(100, Duration::from_secs(1)),
        draw_ui: false,
        show_debug_hud: false,
        menu: Menu::Main,
        next_client_id: 1,
        server_address: settings.default_server_address,
        server_port: settings.default_server_port,
        player_name: settings.default_player_name,
        connect_timeout: Duration::from_secs_f64(settings.connect_timeout),
        read_timeout: Duration::from_secs_f64(settings.read_timeout),
        retry_period: Duration::from_secs_f64(settings.retry_period),
        client_dropper: {
            let (sender, receiver) = channel();
            Dropper {
                sender,
                handle: std::thread::spawn(move || run_dropper(receiver)),
            }
        },
        debug_hud_font: ubuntu_mono,
        name_font: ubuntu_mono,
        max_world_frame_delay: settings.max_world_frame_delay,
        world_updates_delay: settings.world_updates_delay,
        control_hud_font: ubuntu_mono,
        show_control_hud: true,
        message_font: ubuntu_mono,
        show_player_list: false,
        player_list_font: ubuntu_mono,
    };
    let mut frame_type = FrameType::Initial;
    while !matches!(frame_type, FrameType::None) {
        clear_background(BLACK);
        prepare_frame(&mut game_state, &mut frame_type);
        next_frame().await;
        game_state.fps.add(Instant::now());
    }
    game_state
        .client_dropper
        .sender
        .send(DropperMessage::Stop)
        .ok();
    game_state.client_dropper.handle.join().ok();
}

fn run_dropper<T>(receiver: Receiver<DropperMessage<T>>) {
    while let Ok(message) = receiver.recv() {
        match message {
            DropperMessage::Stop => break,
            DropperMessage::Drop(v) => {
                drop(v);
            }
        }
    }
}

fn prepare_frame(game_state: &mut GameState, frame_type: &mut FrameType) {
    game_state.draw_ui = false;
    let input_duration = measure(|| handle_input(game_state, frame_type));
    game_state.input_duration.add(input_duration);
    let update_duration = measure(|| update(game_state, frame_type));
    game_state.update_duration.add(update_duration);
    let ui_update_duration = measure(|| update_ui(game_state, frame_type));
    game_state.ui_update_duration.add(ui_update_duration);
    let draw_duration = measure(|| draw(game_state, frame_type));
    game_state.draw_duration.add(draw_duration);
    game_state.ui_draw_duration.add(measure(|| {
        if game_state.draw_ui {
            egui_macroquad::draw();
        }
    }));
    let debug_hud_duration = measure(|| {
        if game_state.show_debug_hud {
            draw_debug_hud(game_state, &frame_type);
        }
    });
    game_state.debug_hud_duration.add(debug_hud_duration);
}

fn handle_input(game_state: &mut GameState, frame_type: &mut FrameType) {
    match frame_type {
        FrameType::SinglePlayer(v) => {
            let mut actor_action = ActorAction::default();
            let mut cast_actions = Vec::new();
            handle_scene_input(game_state, v, &mut actor_action, |v| cast_actions.push(v));
            if let Some(actor_index) = v.actor_index {
                apply_actor_action(actor_action, actor_index, &mut v.world);
                for cast_action in cast_actions {
                    apply_cast_action(cast_action, actor_index, &mut v.world);
                }
            }
        }
        FrameType::Multiplayer(v) => {
            let scene = &mut v.scene;
            let actor_action = &mut v.actor_action;
            let delayed_cast_actions = &mut v.delayed_cast_actions;
            handle_scene_input(game_state, scene, actor_action, |v| {
                delayed_cast_actions.push_back(v)
            });
            if actor_action.cast_action.is_none() {
                actor_action.cast_action = delayed_cast_actions.pop_front();
            }
        }
        _ => (),
    }
    if is_key_pressed(KeyCode::F1) {
        game_state.show_control_hud = !game_state.show_control_hud;
    }
    if is_key_pressed(KeyCode::F2) {
        game_state.show_debug_hud = !game_state.show_debug_hud;
    }
    game_state.show_player_list = is_key_down(KeyCode::Tab);
}

fn handle_scene_input<F>(
    game_state: &mut GameState,
    scene: &mut Scene,
    actor_action: &mut ActorAction,
    apply_cast_action: F,
) where
    F: FnMut(CastAction),
{
    if matches!(game_state.menu, Menu::None) {
        scene.pointer = Vec2f::from(mouse_position_local())
            / Vec2f::new(
                scene.camera_zoom,
                scene.camera_zoom * (screen_width() / screen_height()) as f64,
            );
        handle_actor_input(scene, actor_action, apply_cast_action);
    }
    if is_key_pressed(KeyCode::Escape) {
        game_state.menu = if matches!(game_state.menu, Menu::None) {
            Menu::Main
        } else {
            Menu::None
        };
    }
}

fn handle_actor_input<F>(scene: &mut Scene, actor_action: &mut ActorAction, apply_cast_action: F)
where
    F: FnMut(CastAction),
{
    scene.camera_zoom *= 1.0 + mouse_wheel().1 as f64 * 0.1;
    actor_action.moving = is_mouse_button_down(MouseButton::Left);
    if let Some(target_direction) = scene.pointer.safe_normalized() {
        actor_action.target_direction = target_direction;
    }
    for_each_cast_action(apply_cast_action);
}

fn update_ui(game_state: &mut GameState, frame_type: &mut FrameType) {
    if matches!(game_state.menu, Menu::None) {
        return;
    }
    egui_macroquad::ui(|ctx| {
        let mut visuals = egui::Visuals::default();
        let bg_fill = visuals.widgets.noninteractive.bg_fill;
        visuals.widgets.noninteractive.bg_fill =
            Color32::from_rgba_premultiplied(bg_fill.r(), bg_fill.g(), bg_fill.b(), 127);
        ctx.set_visuals(visuals);
        match &game_state.menu {
            Menu::None => (),
            Menu::Main => main_menu(ctx, game_state, frame_type),
            Menu::Multiplayer => multiplayer_menu(ctx, game_state, frame_type),
            Menu::Joining => joining_menu(ctx, game_state, frame_type),
            Menu::Error(message) => error_menu(ctx, message.clone(), game_state),
        }
    });
    game_state.draw_ui = true;
}

fn main_menu(ctx: &CtxRef, game_state: &mut GameState, frame_type: &mut FrameType) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.heading("Main menu");
            ui.separator();
            let playing = matches!(
                frame_type,
                FrameType::SinglePlayer(..) | FrameType::Multiplayer(..)
            );
            if playing && ui.button("Resume").clicked() {
                game_state.menu = Menu::None;
            }
            if playing && ui.button("Logout").clicked() {
                game_state.menu = if matches!(frame_type, FrameType::Multiplayer(..)) {
                    Menu::Multiplayer
                } else {
                    Menu::Main
                };
                *frame_type = FrameType::Initial;
            }
            if ui.button("Single player").clicked() {
                *frame_type = FrameType::SinglePlayer(Box::new(make_single_player_scene(
                    &mut game_state.rng,
                )));
                game_state.menu = Menu::None;
            }
            if ui.button("Multiplayer").clicked() {
                game_state.menu = Menu::Multiplayer;
            }
            if ui.button("Quit").clicked() {
                *frame_type = FrameType::None;
            }
        });
    });
}

fn multiplayer_menu(ctx: &CtxRef, game_state: &mut GameState, frame_type: &mut FrameType) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            let valid_player_name = is_valid_player_name(game_state.player_name.as_str());
            let server_address = make_server_address(game_state.server_address.as_str(), game_state.server_port);
            ui.heading("Multiplayer");
            ui.separator();
            ui.label("Player name:");
            ui.text_edit_singleline(&mut game_state.player_name);
            if !valid_player_name {
                ui.label(format!("Player name should contain only alphabetic characters, be at least {} and not longer than {} symbols", MIN_PLAYER_NAME_LEN, MAX_PLAYER_NAME_LEN));
            }
            ui.label("Server address:");
            ui.text_edit_singleline(&mut game_state.server_address);
            if server_address.is_none() {
                ui.label("Server address should be IPv4 or IPv6 address with or without a port");
            }
            if ui.button("Join").clicked() {
                if let (true, Some(server_address)) = (valid_player_name, server_address) {
                    *frame_type = FrameType::Multiplayer(Box::new(Multiplayer {
                        client: AsyncDrop::new(
                            game_state.client_dropper.sender.clone(),
                            Client::new(
                                GameClientSettings {
                                    id: game_state.next_client_id,
                                    connect_timeout: game_state.connect_timeout,
                                    retry_period: game_state.retry_period,
                                    player_name: game_state.player_name.clone(),
                                },
                                UdpClientSettings {
                                    id: game_state.next_client_id,
                                    server_address,
                                    read_timeout: game_state.read_timeout,
                                },
                            )),
                        scene: make_empty_scene(),
                        local_world_frame: 0,
                        local_world_time: 0.0,
                        world_updates: VecDeque::new(),
                        world_frame_delay: SMA::new(100, 0.0).unwrap(),
                        world_frame_diff: SMA::new(100, 0.0).unwrap(),
                        world_frame_st_dev: StDev::new(100, 0.0).unwrap(),
                        last_world_frame_st_dev: 0.0,
                        input_delay: SMA::new(100, 0.0).unwrap(),
                        ack_cast_action_world_frame: 0,
                        actor_action: ActorAction::default(),
                        delayed_cast_actions: VecDeque::new(),
                    }));
                    game_state.next_client_id += 1;
                    game_state.menu = Menu::Joining;
                }
            }
            if ui.button("Back").clicked() {
                game_state.menu = Menu::Main;
            }
        });
    });
}

fn joining_menu(ctx: &CtxRef, game_state: &mut GameState, frame_type: &mut FrameType) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.heading("Joining the server...");
            if ui.button("Cancel").clicked() {
                game_state.menu = Menu::Multiplayer;
                *frame_type = FrameType::Initial;
            }
        });
    });
}

fn error_menu(ctx: &CtxRef, message: String, game_state: &mut GameState) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.heading(message);
            if ui.button("Back").clicked() {
                game_state.menu = Menu::Multiplayer;
            }
        });
    });
}

fn update(game_state: &mut GameState, frame_type: &mut FrameType) {
    let new_frame_type = match frame_type {
        FrameType::SinglePlayer(v) => {
            update_single_player(v, &mut game_state.rng);
            None
        }
        FrameType::Multiplayer(v) => update_multiplayer(game_state, v),
        _ => None,
    };
    if let Some(v) = new_frame_type {
        *frame_type = v;
    }
}

fn draw(game_state: &GameState, frame_type: &mut FrameType) {
    match frame_type {
        FrameType::SinglePlayer(scene) => draw_scene(game_state, scene),
        FrameType::Multiplayer(v) => draw_scene(game_state, &mut v.scene),
        _ => (),
    }
}

fn make_single_player_scene<R: Rng>(rng: &mut R) -> Scene {
    let mut world = generate_world(Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2)), rng);
    let player_id = PlayerId(get_next_id(&mut world.id_counter));
    world.players.push(Player {
        id: player_id,
        active: true,
        name: "Player".to_string(),
        actor_id: None,
        spawn_time: world.time,
        deaths: 0,
    });
    Scene {
        time_step: 1.0 / 60.0,
        engine: Engine::default(),
        player_id: Some(player_id),
        actor_id: None,
        actor_index: None,
        camera_zoom: 0.05,
        camera_target: Vec2f::ZERO,
        pointer: Vec2f::ZERO,
        world: Box::new(world),
    }
}

fn make_empty_scene() -> Scene {
    Scene {
        time_step: 1.0 / 60.0,
        engine: Engine::default(),
        player_id: None,
        actor_id: None,
        actor_index: None,
        camera_zoom: 0.05,
        camera_target: Vec2f::ZERO,
        pointer: Vec2f::ZERO,
        world: Box::new(World::default()),
    }
}

fn update_single_player<R: Rng>(scene: &mut Scene, rng: &mut R) {
    scene.engine.update(scene.time_step, &mut scene.world, rng);
    update_scene_actor_index(scene);
}

fn update_multiplayer(game_state: &mut GameState, data: &mut Multiplayer) -> Option<FrameType> {
    let world_frame = data.scene.world.frame;
    let mut apply_all_updates = false;
    while let Some(update) = data.client.receiver().try_recv().ok() {
        match update {
            GameUpdate::GameOver(message) => {
                data.scene.player_id = None;
                game_state.menu = Menu::Error(format!("Disconnected from server: {}", message));
                return Some(FrameType::Initial);
            }
            GameUpdate::SetPlayerId(v) => {
                data.scene.player_id = Some(v);
                apply_all_updates = true;
            }
            GameUpdate::WorldSnapshot {
                ack_actor_action_world_frame,
                ack_cast_action_world_frame,
                world,
            } => {
                if matches!(game_state.menu, Menu::Joining) {
                    game_state.menu = Menu::None;
                }
                data.world_updates.clear();
                data.scene.world = world;
                update_scene_actor_index(&mut data.scene);
                ack_actor_action(
                    ack_actor_action_world_frame,
                    ack_cast_action_world_frame,
                    data,
                );
            }
            GameUpdate::WorldUpdate {
                ack_actor_action_world_frame,
                ack_cast_action_world_frame,
                world_update,
            } => {
                data.world_frame_delay
                    .next((world_update.after_frame - world_update.before_frame) as f64);
                data.world_updates.push_back(world_update);
                ack_actor_action(
                    ack_actor_action_world_frame,
                    ack_cast_action_world_frame,
                    data,
                );
            }
        }
    }
    if !data.client.is_done() && !data.client.is_running() {
        data.client.stop();
    }
    if data.client.is_done() && matches!(game_state.menu, Menu::None | Menu::Joining) {
        if let Err(e) = data.client.join() {
            game_state.menu = Menu::Error(e);
        } else {
            game_state.menu = Menu::Multiplayer;
        }
        return Some(FrameType::Initial);
    }
    if let Some(max_frame_diff) = data
        .world_updates
        .iter()
        .map(|v| v.after_frame - v.before_frame)
        .min()
    {
        let apply_updates = if apply_all_updates {
            data.world_updates.len()
        } else if max_frame_diff >= game_state.max_world_frame_delay {
            max_frame_diff as usize / 2
        } else if max_frame_diff >= game_state.max_world_frame_delay / 2 {
            max_frame_diff as usize / 4
        } else {
            1
        };
        for _ in 0..apply_updates.max(1) {
            if !apply_all_updates && data.world_updates.len() <= game_state.world_updates_delay {
                break;
            }
            if let Some(world_update) = data.world_updates.pop_front() {
                apply_world_update(*world_update, &mut data.scene.world);
                update_scene_actor_index(&mut data.scene);
            } else {
                break;
            }
        }
    }
    data.client
        .sender()
        .send(PlayerControl {
            actor_action: data.actor_action.clone(),
            cast_action_world_frame: data.ack_cast_action_world_frame + 1,
            ack_world_frame: data.scene.world.frame,
        })
        .ok();
    let frame_diff = (data.scene.world.frame - world_frame) as f64;
    data.world_frame_diff.next(frame_diff);
    data.last_world_frame_st_dev = data.world_frame_st_dev.next(frame_diff);
    data.local_world_frame = (data.local_world_frame + 1).max(data.scene.world.frame);
    data.local_world_time += data.scene.time_step;
    if data.local_world_time < data.scene.world.time {
        data.local_world_time = data.scene.world.time;
    }
    data.scene.engine.update_visual(&mut data.scene.world);
    None
}

fn ack_actor_action(
    ack_actor_action_world_frame: u64,
    ack_cast_action_world_frame: u64,
    data: &mut Multiplayer,
) {
    if data.ack_cast_action_world_frame < ack_cast_action_world_frame {
        data.ack_cast_action_world_frame = ack_cast_action_world_frame;
        data.actor_action.cast_action = data.delayed_cast_actions.pop_front();
    }
    data.input_delay
        .next((data.scene.world.frame - ack_actor_action_world_frame) as f64);
}

fn draw_scene(game_state: &GameState, scene: &mut Scene) {
    if let Some(actor_index) = scene.actor_index {
        scene.camera_target = scene.world.actors[actor_index].position;
    }

    set_camera(&Camera2D {
        zoom: vec2(
            scene.camera_zoom as f32,
            -scene.camera_zoom as f32 * screen_width() / screen_height(),
        ),
        target: vec2(scene.camera_target.x as f32, scene.camera_target.y as f32),
        ..Default::default()
    });

    for v in scene.world.static_areas.iter() {
        draw_disk_body_and_magick(
            &v.body.shape,
            v.body.material,
            &v.magick.power,
            scene.world.settings.border_width,
            v.position,
        );
    }

    for v in scene.world.temp_areas.iter() {
        draw_disk_body_and_magick(
            &v.body.shape,
            v.body.material,
            &v.effect.power,
            scene.world.settings.border_width,
            v.position,
        );
    }

    for area in scene.world.bounded_areas.iter() {
        let owner = scene
            .world
            .actors
            .iter()
            .find(|v| v.id == area.actor_id)
            .unwrap();
        draw_ring_sector_body_and_magick(
            &area.body,
            &area.effect.power,
            owner.position,
            normalize_angle(owner.current_direction.angle()),
        );
    }

    if let Some(actor_index) = scene.actor_index {
        let actor = &scene.world.actors[actor_index];
        draw_line(
            actor.position.x as f32,
            actor.position.y as f32,
            (actor.position.x + scene.pointer.x) as f32,
            (actor.position.y + scene.pointer.y) as f32,
            0.1,
            Color::new(0.0, 0.0, 0.0, 0.5),
        );
        let current_target =
            actor.position + actor.current_direction * actor.body.shape.radius * 2.0;
        draw_line(
            actor.position.x as f32,
            actor.position.y as f32,
            current_target.x as f32,
            current_target.y as f32,
            0.1,
            Color::new(0.0, 0.0, 0.0, 0.5),
        );
    }

    for beam in scene
        .engine
        .initial_emitted_beams()
        .iter()
        .chain(scene.engine.reflected_emitted_beams().iter())
    {
        let end = beam.origin + beam.direction * beam.length;
        let color = get_magick_power_color(&beam.magick.power);
        let sum_power = beam.magick.power.iter().sum::<f64>() / 20.0;
        draw_line(
            beam.origin.x as f32,
            beam.origin.y as f32,
            end.x as f32,
            end.y as f32,
            sum_power as f32,
            color,
        );
    }

    for v in scene.world.actors.iter() {
        draw_disk_body_and_magick(
            &v.body.shape,
            v.body.material,
            &v.effect.power,
            scene.world.settings.border_width,
            v.position,
        );
    }

    for v in scene.world.dynamic_objects.iter() {
        draw_disk_body_and_magick(
            &v.body.shape,
            v.body.material,
            &v.effect.power,
            scene.world.settings.border_width,
            v.position,
        );
    }

    for v in scene.world.static_objects.iter() {
        match &v.body.shape {
            StaticShape::CircleArc(arc) => {
                let ring_sector = RingSector {
                    min_radius: arc.radius - scene.world.settings.border_width,
                    max_radius: arc.radius + scene.world.settings.border_width,
                    angle: arc.length,
                };
                draw_ring_sector_body_and_magick(
                    &ring_sector,
                    &v.effect.power,
                    v.position,
                    arc.rotation,
                );
                draw_ring_sector_body_and_magick(
                    &ring_sector,
                    &v.aura.elements,
                    v.position,
                    arc.rotation,
                );
            }
            StaticShape::Disk(shape) => {
                draw_disk_body_and_magick(
                    shape,
                    v.body.material,
                    &v.effect.power,
                    scene.world.settings.border_width,
                    v.position,
                );
            }
        }
    }

    for v in scene.world.actors.iter() {
        draw_aura(&v.aura, v.position);
    }

    for v in scene.world.dynamic_objects.iter() {
        draw_aura(&v.aura, v.position);
    }

    for v in scene.world.static_objects.iter() {
        draw_aura(&v.aura, v.position);
    }

    for v in scene.world.actors.iter() {
        draw_health(v.health, v.body.shape.radius, v.position);
        draw_aura_power(
            v.aura.power / scene.world.settings.max_magic_power,
            v.body.shape.radius,
            v.position,
        );
        if let Some(delayed_magick) = v.delayed_magick.as_ref() {
            if matches!(delayed_magick.status, DelayedMagickStatus::Started) {
                draw_delayed_magic_power(
                    (scene.world.time - delayed_magick.started)
                        .min(scene.world.settings.max_magic_power)
                        / scene.world.settings.max_magic_power,
                    v.body.shape.radius,
                    v.position,
                );
            }
        }
    }

    for v in scene.world.dynamic_objects.iter() {
        draw_health(v.health, v.body.shape.radius, v.position);
        draw_aura_power(
            v.aura.power / scene.world.settings.max_magic_power,
            v.body.shape.radius,
            v.position,
        );
    }

    for v in scene.world.static_objects.iter() {
        let radius = match &v.body.shape {
            StaticShape::CircleArc(v) => v.radius,
            StaticShape::Disk(v) => v.radius,
        };
        draw_health(v.health, radius, v.position);
        draw_aura_power(
            v.aura.power / scene.world.settings.max_magic_power,
            radius,
            v.position,
        );
    }

    for v in scene.world.actors.iter() {
        draw_spell_elements(
            &v.spell_elements,
            v.position + Vec2f::new(-HALF_WIDTH, v.body.shape.radius + 0.2),
            HALF_HEIGHT,
            (2.0 * HALF_WIDTH) / 5.0,
        );
    }

    for v in scene.world.actors.iter() {
        if Some(v.id) != scene.actor_id {
            draw_name(
                v.name.as_str(),
                v.position,
                v.body.shape.radius,
                game_state.name_font,
            );
        }
    }

    draw_rectangle_lines(
        scene.world.bounds.min.x as f32,
        scene.world.bounds.min.y as f32,
        scene.world.bounds.width() as f32,
        scene.world.bounds.height() as f32,
        1.0,
        Color::new(1.0, 0.0, 0.0, 0.5),
    );

    if game_state.show_control_hud {
        let spell_elements = if let Some(actor_index) = scene.actor_index {
            scene.world.actors[actor_index].spell_elements.as_slice()
        } else {
            &[]
        };
        draw_control_hud(spell_elements, game_state.control_hud_font);
    }

    if scene.actor_index.is_none() {
        if let Some(player_id) = scene.player_id {
            if let Some(player) = scene.world.players.iter().find(|v| v.id == player_id) {
                draw_spawn_message(
                    player.spawn_time - scene.world.time,
                    game_state.message_font,
                );
            }
        }
    }

    if game_state.show_player_list {
        draw_player_list(&scene.world.players, game_state.player_list_font);
    }
}

fn draw_debug_hud(game_state: &GameState, frame_type: &FrameType) {
    set_default_camera();

    let mut text_counter = 0;
    draw_debug_game_state_text(&mut text_counter, game_state);

    draw_debug_text(
        &mut text_counter,
        game_state.debug_hud_font,
        format!(
            "Frame type: {}",
            match frame_type {
                FrameType::Initial => "Initial",
                FrameType::SinglePlayer(..) => "SinglePlayer",
                FrameType::Multiplayer { .. } => "Multiplayer",
                FrameType::None => "None",
            }
        )
        .as_str(),
    );

    match frame_type {
        FrameType::SinglePlayer(scene) => {
            draw_debug_scene_text(&mut text_counter, scene, game_state.debug_hud_font)
        }
        FrameType::Multiplayer(v) => {
            draw_debug_multiplayer_text(&mut text_counter, game_state, v);
            draw_debug_scene_text(&mut text_counter, &v.scene, game_state.debug_hud_font);
        }
        _ => (),
    }
}

fn draw_debug_game_state_text(counter: &mut usize, game_state: &GameState) {
    draw_debug_texts(
        counter,
        game_state.debug_hud_font,
        &[
            {
                let minmax = game_state.fps.minmax();
                format!(
                    "FPS: {:.3} [{:.3}, {:.3}]",
                    game_state.fps.get(),
                    minmax.0,
                    minmax.1
                )
            },
            format_duration_metric("Input", &game_state.input_duration),
            format_duration_metric("Update UI", &game_state.ui_update_duration),
            format_duration_metric("Update", &game_state.update_duration),
            format_duration_metric("Draw", &game_state.draw_duration),
            format_duration_metric("Draw UI", &game_state.ui_draw_duration),
            format_duration_metric("Draw debug HUD", &game_state.debug_hud_duration),
            format!("Screen width: {}", screen_width()),
            format!("Screen height: {}", screen_height()),
        ],
    );
}

fn format_duration_metric(title: &str, duration: &DurationMovingAverage) -> String {
    let minmax = duration.minmax();
    format!(
        "{}: {:.3} [{:.3}, {:.3}] ms",
        title,
        duration.get() * 1000.0,
        minmax.0 * 1000.0,
        minmax.1 * 1000.0
    )
}

fn draw_debug_multiplayer_text(counter: &mut usize, game_state: &GameState, data: &Multiplayer) {
    draw_debug_texts(
        counter,
        game_state.debug_hud_font,
        &[
            String::from("Multiplayer:"),
            format!("Server: {}", game_state.server_address),
            format!(
                "World frame delay: {}",
                data.local_world_frame - data.scene.world.frame
            ),
            format!(
                "World time delay: {:.3}",
                data.local_world_time - data.scene.world.time
            ),
            format!("World updates buffer: {}", data.world_updates.len()),
            format!(
                "Mean world update delay: {:.3}",
                data.world_frame_delay.get_last_value()
            ),
            format!(
                "Mean world frame diff: {:.3}",
                data.world_frame_diff.get_last_value()
            ),
            format!(
                "St dev world frame diff: {:.3}",
                data.last_world_frame_st_dev
            ),
            format!("Mean input delay: {:.3}", data.input_delay.get_last_value()),
            format!("Delayed cast actions: {}", data.delayed_cast_actions.len()),
            String::from("Actor action:"),
            format!("Moving: {}", data.actor_action.moving),
            format!(
                "Target direction: {:.3} {:.3}",
                data.actor_action.target_direction.x, data.actor_action.target_direction.y
            ),
            format!("Cast action: {:?}", data.actor_action.cast_action),
        ],
    );
}

fn draw_debug_scene_text(counter: &mut usize, scene: &Scene, font: Font) {
    draw_debug_texts(
        counter,
        font,
        &[
            String::from("Scene:"),
            format!("World frame: {}", scene.world.frame),
            format!("World time: {:.3}", scene.world.time),
            format!("Player: id={:?}", scene.player_id.map(|v| v.0)),
            format!(
                "Actor: id={:?} index={:?}",
                scene.actor_id, scene.actor_index
            ),
            format!("Camera zoom: {}", scene.camera_zoom),
            format!(
                "Camera target: {:.3} {:.3}",
                scene.camera_target.x, scene.camera_target.y
            ),
            format!("Pointer: {:.3} {:.3}", scene.pointer.x, scene.pointer.y),
            format!("Actors: {}", scene.world.actors.len()),
            format!("Dynamic objects: {}", scene.world.dynamic_objects.len()),
            format!("Static objects: {}", scene.world.static_objects.len()),
            format!("Beams: {}", scene.world.beams.len()),
            format!("Static areas: {}", scene.world.static_areas.len()),
            format!("Temp areas: {}", scene.world.temp_areas.len()),
            format!("Bounded areas: {}", scene.world.bounded_areas.len()),
            format!("Fields: {}", scene.world.fields.len()),
            format!("Guns: {}", scene.world.guns.len()),
        ],
    );
}

fn draw_debug_texts(counter: &mut usize, font: Font, texts: &[String]) {
    for text in texts.iter() {
        draw_debug_text(counter, font, text.as_str());
    }
}

fn draw_debug_text(counter: &mut usize, font: Font, text: &str) {
    *counter += 1;
    draw_text_ex(
        text,
        HUD_MARGIN as f32,
        (4 + *counter * HUD_FONT_SIZE as usize) as f32,
        TextParams {
            font,
            font_size: HUD_FONT_SIZE,
            font_scale: 1.0,
            color: WHITE,
            font_scale_aspect: 1.0,
        },
    );
}

fn for_each_cast_action<F>(mut f: F)
where
    F: FnMut(CastAction),
{
    use macroquad::input::*;
    if is_mouse_button_pressed(MouseButton::Right) {
        if is_key_down(KeyCode::LeftShift) {
            f(CastAction::StartAreaOfEffectMagick);
        } else {
            f(CastAction::StartDirectedMagick);
        }
    }
    if is_mouse_button_released(MouseButton::Right) {
        f(CastAction::CompleteDirectedMagick);
    }
    if is_mouse_button_released(MouseButton::Middle) {
        f(CastAction::SelfMagick);
    }
    if is_key_pressed(KeyCode::Q) {
        f(CastAction::AddSpellElement(Element::Water));
    }
    if is_key_pressed(KeyCode::A) {
        f(CastAction::AddSpellElement(Element::Lightning));
    }
    if is_key_pressed(KeyCode::W) {
        f(CastAction::AddSpellElement(Element::Life));
    }
    if is_key_pressed(KeyCode::S) {
        f(CastAction::AddSpellElement(Element::Arcane));
    }
    if is_key_pressed(KeyCode::E) {
        f(CastAction::AddSpellElement(Element::Shield));
    }
    if is_key_pressed(KeyCode::D) {
        f(CastAction::AddSpellElement(Element::Earth));
    }
    if is_key_pressed(KeyCode::R) {
        f(CastAction::AddSpellElement(Element::Cold));
    }
    if is_key_pressed(KeyCode::F) {
        f(CastAction::AddSpellElement(Element::Fire));
    }
}

fn draw_disk_body_and_magick(
    shape: &Disk,
    material: Material,
    power: &[f64; 11],
    border_width: f64,
    position: Vec2f,
) {
    let has_power = power.iter().sum::<f64>() > 0.0;
    if has_power {
        draw_poly(
            position.x as f32,
            position.y as f32,
            75,
            shape.radius as f32,
            0.0,
            get_magick_power_color(power),
        );
    }
    draw_poly(
        position.x as f32,
        position.y as f32,
        75,
        (shape.radius - border_width * has_power as i32 as f64) as f32,
        0.0,
        get_material_color(material, 1.0),
    );
}

fn draw_ring_sector_body_and_magick<T>(
    body: &RingSector,
    power: &[T; 11],
    position: Vec2f,
    rotation: f64,
) where
    T: Default + PartialEq,
{
    const BASE_RESOLUTION: f64 = HUD_MARGIN;
    let color = get_magick_power_color(power);
    let resolution = (body.angle * BASE_RESOLUTION).round() as usize;
    let min_angle_step = body.angle / (resolution - 1) as f64;
    let max_angle_step = body.angle / resolution as f64;
    let mut vertices = Vec::with_capacity(2 * resolution + 1);
    let mut indices = Vec::with_capacity(3 * (2 * resolution - 1));
    for i in 0..resolution {
        let max =
            Vec2f::only_x(body.max_radius).rotated(i as f64 * max_angle_step - body.angle / 2.0);
        let min =
            Vec2f::only_x(body.min_radius).rotated(i as f64 * min_angle_step - body.angle / 2.0);
        vertices.push(Vertex::new(
            max.x as f32,
            max.y as f32,
            0.0,
            0.0,
            0.0,
            color,
        ));
        vertices.push(Vertex::new(
            min.x as f32,
            min.y as f32,
            0.0,
            0.0,
            0.0,
            color,
        ));
    }
    let last = Vec2f::only_x(body.max_radius).rotated(body.angle / 2.0);
    vertices.push(Vertex::new(
        last.x as f32,
        last.y as f32,
        0.0,
        0.0,
        0.0,
        color,
    ));
    for i in 0..2 * resolution as u16 - 1 {
        indices.push(i);
        indices.push(i + 1);
        indices.push(i + 2);
    }
    draw_triangles(
        Mat4::from_rotation_translation(
            Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), rotation as f32),
            Vec3::new(position.x as f32, position.y as f32, 0.0),
        ),
        &vertices,
        &indices,
    );
}

fn draw_triangles(matrix: Mat4, vertices: &[Vertex], indices: &[u16]) {
    let context = unsafe { get_internal_gl() };
    context.quad_gl.push_model_matrix(matrix);
    context.quad_gl.texture(None);
    context.quad_gl.draw_mode(DrawMode::Triangles);
    context.quad_gl.geometry(vertices, indices);
    context.quad_gl.pop_model_matrix();
}

fn get_material_color(material: Material, alpha: f32) -> Color {
    match material {
        Material::None => Color::new(0.0, 0.0, 0.0, alpha),
        Material::Flesh => Color::new(0.93, 0.89, 0.69, alpha),
        Material::Stone => Color::new(0.76, 0.76, 0.76, alpha),
        Material::Dirt => Color::new(0.5, 0.38, 0.26, alpha),
        Material::Grass => Color::new(0.44, 0.69, 0.15, alpha),
        Material::Water => Color::new(0.1, 0.1, 0.9, alpha),
        Material::Ice => Color::new(0.83, 0.94, 0.97, alpha),
    }
}

fn get_magick_power_color<T: Default + PartialEq>(power: &[T; 11]) -> Color {
    let mut result: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
    let mut colors = 0;
    power
        .iter()
        .enumerate()
        .filter(|(_, p)| **p != T::default())
        .for_each(|(i, _)| {
            let color = get_element_color(Element::from(i));
            result
                .iter_mut()
                .zip(color.iter())
                .for_each(|(r, c)| *r += *c);
            colors += 1;
        });
    if colors == 0 {
        return Color::new(0.0, 0.0, 0.0, 0.0);
    }
    result.iter_mut().for_each(|v| *v /= colors as f32);
    Color::from(result)
}

fn get_element_color(element: Element) -> [f32; 4] {
    match element {
        Element::Water => [0.0, 0.0, 1.0, 0.8],
        Element::Lightning => [1.0, 0.0, 1.0, 0.8],
        Element::Life => [0.0, 1.0, 0.0, 1.0],
        Element::Arcane => [1.0, 0.0, 0.0, 1.0],
        Element::Shield => [1.0, 1.0, 0.0, 0.5],
        Element::Earth => [0.7, 0.7, 0.7, 1.0],
        Element::Cold => [0.5, 0.75, 1.0, 0.8],
        Element::Fire => [1.0, 0.5, 0.0, 0.8],
        Element::Steam => [0.7, 0.7, 0.7, 0.5],
        Element::Ice => [0.0, 0.75, 1.0, 0.8],
        Element::Poison => [0.5, 1.0, 0.0, 0.8],
    }
}

fn draw_aura(aura: &Aura, position: Vec2f) {
    draw_poly(
        position.x as f32,
        position.y as f32,
        75,
        aura.radius as f32,
        0.0,
        get_magick_power_color(&aura.elements),
    );
}

fn draw_health(value: f64, radius: f64, position: Vec2f) {
    draw_meter(value, radius, position, 0.5, Color::new(1.0, 0.0, 0.0, 1.0));
}

fn draw_aura_power(value: f64, radius: f64, position: Vec2f) {
    draw_meter(value, radius, position, 0.8, Color::new(0.0, 0.0, 1.0, 1.0));
}

fn draw_delayed_magic_power(value: f64, radius: f64, position: Vec2f) {
    draw_meter(value, radius, position, 1.1, Color::new(0.0, 1.0, 0.0, 1.0));
}

fn draw_meter(value: f64, radius: f64, position: Vec2f, y: f64, color: Color) {
    draw_rectangle(
        (position.x - HALF_WIDTH) as f32,
        (position.y + radius + y - HALF_HEIGHT) as f32,
        (2.0 * HALF_WIDTH) as f32,
        (2.0 * HALF_HEIGHT) as f32,
        Color::new(0.0, 0.0, 0.0, 0.8),
    );
    draw_rectangle(
        (position.x - HALF_WIDTH + BORDER_WIDTH) as f32,
        (position.y + radius + y - HALF_HEIGHT + BORDER_WIDTH) as f32,
        (2.0 * (HALF_WIDTH - BORDER_WIDTH) * value) as f32,
        (2.0 * (HALF_HEIGHT - BORDER_WIDTH)) as f32,
        color,
    );
}

fn draw_control_hud(spell_elements: &[Element], font: Font) {
    set_default_camera();
    draw_spell_elements(
        spell_elements,
        Vec2f::new(
            (screen_width() as f64 - 5.0 * HUD_ELEMENT_WIDTH) / 2.0,
            screen_height() as f64 - (3.0 * HUD_MARGIN + 5.0 * HUD_ELEMENT_RADIUS),
        ),
        HUD_ELEMENT_RADIUS,
        HUD_ELEMENT_WIDTH,
    );
    let elements_position = Vec2f::new(
        (screen_width() as f64 - 8.0 * HUD_ELEMENT_WIDTH) / 2.0,
        screen_height() as f64 - (2.0 * HUD_MARGIN + 3.0 * HUD_ELEMENT_RADIUS),
    );
    const ELEMENT_KEYS: &[&str] = &["Q", "A", "W", "S", "E", "D", "R", "F"];
    for (i, name) in ELEMENT_KEYS.iter().enumerate() {
        let element_position =
            elements_position + Vec2f::only_x((i as f64 + 0.5) * HUD_ELEMENT_WIDTH);
        draw_element(Element::from(i), element_position, HUD_ELEMENT_RADIUS);
        draw_keyboard_button(
            name,
            HUD_ELEMENT_RADIUS,
            font,
            Vec2f::new(
                element_position.x - HUD_ELEMENT_RADIUS,
                element_position.y + HUD_ELEMENT_RADIUS + HUD_MARGIN,
            ),
        );
    }
    const CONTROL_KEYS: &[(&str, &str, f64)] = &[
        ("L.Shift", "Area of effect", 2.0),
        ("Tab", "Player's list", 1.5),
        ("F2", "Debug HUD", 1.0),
        ("F1", "Control HUD", 1.0),
        ("Esc", "Main menu", 1.0),
    ];
    for (i, v) in CONTROL_KEYS.iter().enumerate() {
        draw_control_button(
            v.0,
            v.1,
            HUD_ELEMENT_RADIUS * v.2,
            font,
            Vec2f::new(
                HUD_MARGIN,
                screen_height() as f64 - (2.0 * HUD_ELEMENT_RADIUS + HUD_MARGIN) * (i + 1) as f64,
            ),
        );
    }
    const MOUSE_KEYS: &[(MouseButton, &str)] = &[
        (MouseButton::Middle, "Self cast"),
        (MouseButton::Right, "Cast spell"),
        (MouseButton::Left, "Move"),
        (MouseButton::Unknown, "Rotate"),
    ];
    for (i, v) in MOUSE_KEYS.iter().enumerate() {
        draw_mouse(
            v.0,
            v.1,
            font,
            Vec2f::new(
                screen_width() as f64 - HUD_MARGIN - 100.0,
                screen_height() as f64 - (2.0 * HUD_ELEMENT_RADIUS + HUD_MARGIN) * (i + 1) as f64,
            ),
        );
    }
}

fn draw_control_button(name: &str, action: &str, half_width: f64, font: Font, position: Vec2f) {
    draw_keyboard_button(name, half_width, font, position);
    draw_text_ex(
        action,
        (position.x + 2.0 * half_width + HUD_MARGIN) as f32,
        (position.y + 1.25 * HUD_ELEMENT_RADIUS) as f32,
        TextParams {
            font,
            font_size: HUD_FONT_SIZE,
            font_scale: 1.0,
            color: WHITE,
            font_scale_aspect: 1.0,
        },
    );
}

fn draw_keyboard_button(name: &str, half_width: f64, font: Font, position: Vec2f) {
    let width = 2.0 * half_width;
    draw_rectangle(
        position.x as f32,
        position.y as f32,
        width as f32,
        (2.0 * HUD_ELEMENT_RADIUS) as f32,
        BLACK,
    );
    draw_rectangle(
        (position.x + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (position.y + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (width - 2.0 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        (2.0 * (HUD_ELEMENT_RADIUS - HUD_ELEMENT_BORDER_WIDTH)) as f32,
        WHITE,
    );
    let text_dimensions = measure_text(name, Some(font), HUD_FONT_SIZE, 1.0);
    draw_text_ex(
        name,
        (position.x + half_width) as f32 - text_dimensions.width / 2.0,
        (position.y + 1.25 * HUD_ELEMENT_RADIUS) as f32,
        TextParams {
            font,
            font_size: HUD_FONT_SIZE,
            font_scale: 1.0,
            color: BLACK,
            font_scale_aspect: 1.0,
        },
    );
}

fn draw_mouse(highlight: MouseButton, action: &str, font: Font, position: Vec2f) {
    const HIGHLIGHT_COLOR: Color = Color::new(0.9, 0.9, 0.0, 1.0);
    draw_rectangle(
        position.x as f32,
        position.y as f32,
        (2.0 * HUD_ELEMENT_RADIUS) as f32,
        (2.0 * HUD_ELEMENT_RADIUS) as f32,
        BLACK,
    );
    draw_rectangle(
        (position.x + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (position.y + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (HUD_ELEMENT_RADIUS - 1.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        (HUD_ELEMENT_RADIUS - 1.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        if matches!(highlight, MouseButton::Left) {
            HIGHLIGHT_COLOR
        } else {
            WHITE
        },
    );
    draw_rectangle(
        (position.x + HUD_ELEMENT_RADIUS + 0.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        (position.y + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (HUD_ELEMENT_RADIUS - 1.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        (HUD_ELEMENT_RADIUS - 1.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        if matches!(highlight, MouseButton::Right) {
            HIGHLIGHT_COLOR
        } else {
            WHITE
        },
    );
    draw_rectangle(
        (position.x + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (position.y + HUD_ELEMENT_RADIUS + 0.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        2.0 * (HUD_ELEMENT_RADIUS - HUD_ELEMENT_BORDER_WIDTH) as f32,
        (HUD_ELEMENT_RADIUS - 1.5 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        if matches!(highlight, MouseButton::Unknown) {
            HIGHLIGHT_COLOR
        } else {
            WHITE
        },
    );
    draw_rectangle(
        (position.x + 0.75 * HUD_ELEMENT_RADIUS) as f32,
        position.y as f32,
        (0.5 * HUD_ELEMENT_RADIUS) as f32,
        (0.66 * HUD_ELEMENT_RADIUS) as f32,
        BLACK,
    );
    draw_rectangle(
        (position.x + 0.75 * HUD_ELEMENT_RADIUS + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (position.y + HUD_ELEMENT_BORDER_WIDTH) as f32,
        (0.5 * HUD_ELEMENT_RADIUS - 2.0 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        (0.66 * HUD_ELEMENT_RADIUS - 2.0 * HUD_ELEMENT_BORDER_WIDTH) as f32,
        if matches!(highlight, MouseButton::Middle) {
            HIGHLIGHT_COLOR
        } else {
            WHITE
        },
    );
    let text_dimensions = measure_text(action, Some(font), HUD_FONT_SIZE, 1.0);
    draw_text_ex(
        action,
        (position.x - HUD_MARGIN) as f32 - text_dimensions.width,
        (position.y + 1.25 * HUD_ELEMENT_RADIUS) as f32,
        TextParams {
            font,
            font_size: HUD_FONT_SIZE,
            font_scale: 1.0,
            color: WHITE,
            font_scale_aspect: 1.0,
        },
    );
}

fn draw_spell_elements(
    spell_elements: &[Element],
    position: Vec2f,
    element_radius: f64,
    element_width: f64,
) {
    for (i, element) in spell_elements.iter().enumerate() {
        draw_element(
            *element,
            position + Vec2f::only_x((i as f64 + 0.5) * element_width),
            element_radius,
        );
    }
}

fn draw_element(element: Element, position: Vec2f, radius: f64) {
    draw_poly(
        position.x as f32,
        position.y as f32,
        20,
        radius as f32,
        0.0,
        BLACK,
    );
    draw_poly(
        position.x as f32,
        position.y as f32,
        20,
        (radius * BORDER_FACTOR) as f32,
        0.0,
        Color::from(get_element_color(element)),
    );
}

fn draw_name(text: &str, position: Vec2f, radius: f64, font: Font) {
    let text_dimensions = measure_text(text, Some(font), NAME_FONT_SIZE, NAME_FONT_SCALE);
    draw_text_ex(
        text,
        position.x as f32 - text_dimensions.width / 2.0,
        (position.y - radius - 0.3) as f32,
        TextParams {
            font,
            font_size: NAME_FONT_SIZE,
            font_scale: NAME_FONT_SCALE,
            color: Color::new(1.0, 1.0, 1.0, 0.8),
            font_scale_aspect: 1.0,
        },
    );
}

fn make_server_address(address: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(v) = address.parse::<SocketAddr>() {
        return Some(v);
    }
    if let Ok(v) = format!("{}:{}", address, port).parse::<SocketAddr>() {
        return Some(v);
    }
    if let Ok(v) = format!("[{}]:{}", address, port).parse::<SocketAddr>() {
        return Some(v);
    }
    None
}

struct Dropper<T> {
    sender: Sender<DropperMessage<T>>,
    handle: JoinHandle<()>,
}

enum DropperMessage<T> {
    Stop,
    Drop(T),
}

struct AsyncDrop<T> {
    sender: Sender<DropperMessage<T>>,
    value: Option<T>,
}

impl<T> AsyncDrop<T> {
    pub fn new(sender: Sender<DropperMessage<T>>, value: T) -> Self {
        Self {
            sender,
            value: Some(value),
        }
    }
}

impl<T> Drop for AsyncDrop<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.sender.send(DropperMessage::Drop(value)).ok();
        }
    }
}

impl<T> std::ops::Deref for AsyncDrop<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap()
    }
}

impl<T> std::ops::DerefMut for AsyncDrop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap()
    }
}

fn update_scene_actor_index(scene: &mut Scene) {
    if let Some(player_id) = scene.player_id {
        scene.actor_id = scene
            .world
            .players
            .iter()
            .find(|v| v.id == player_id)
            .and_then(|v| v.actor_id);
        scene.actor_index = scene
            .actor_id
            .and_then(|actor_id| scene.world.actors.iter().position(|v| v.id == actor_id));
    } else {
        scene.actor_id = None;
        scene.actor_index = None;
    }
}

fn draw_spawn_message(time_left: f64, font: Font) {
    set_default_camera();
    let text = format!("Spawn in {}s", time_left.ceil());
    let text_dimensions = measure_text(&text, Some(font), MESSAGE_FONT_SIZE, 1.0);
    draw_text_ex(
        &text,
        (screen_width() - text_dimensions.width) / 2.0,
        screen_height() / 2.0,
        TextParams {
            font,
            font_size: MESSAGE_FONT_SIZE,
            font_scale: 1.0,
            color: WHITE,
            font_scale_aspect: 1.0,
        },
    );
}

fn draw_player_list(players: &[Player], font: Font) {
    const MAX_ROW_HEIGHT: f32 = 48.0;
    const FONT_SCALE: f32 = 1.0;
    const NAME: &str = "name";
    const DEATHS: &str = "deaths";
    set_default_camera();
    let mut order: Vec<usize> = (0..players.len()).collect();
    order.sort_by_key(|v| (players[*v].deaths, &players[*v].name));
    let x = screen_width() / 4.0;
    let y = screen_height() / 4.0;
    let width = screen_width() / 2.0;
    let height = screen_height() / 2.0;
    let row_size = (height / players.len() as f32).min(MAX_ROW_HEIGHT);
    let font_size = (row_size * 2.0 / 3.0).round() as u16;
    let text_params = TextParams {
        font,
        font_size,
        font_scale: FONT_SCALE,
        color: WHITE,
        font_scale_aspect: 1.0,
    };
    draw_rectangle(x, y, width, height, Color::new(0.0, 0.0, 0.0, 0.25));
    draw_text_ex(
        NAME,
        x + (width / 2.0 - measure_text(NAME, Some(font), font_size, 1.0).width) / 2.0,
        y + row_size,
        text_params,
    );
    draw_text_ex(
        DEATHS,
        x + width / 2.0
            + (width / 2.0 - measure_text(DEATHS, Some(font), font_size, 1.0).width) / 2.0,
        y + row_size,
        text_params,
    );
    let line_y = y + 1.25 * row_size;
    draw_line(
        x + row_size,
        line_y,
        x + width / 2.0 - row_size,
        line_y,
        2.0,
        WHITE,
    );
    draw_line(
        x + width / 2.0 + row_size,
        line_y,
        x + width - row_size,
        line_y,
        2.0,
        WHITE,
    );
    for i in order {
        draw_text_ex(
            &players[i].name,
            x + (width / 2.0 - measure_text(&players[i].name, Some(font), font_size, 1.0).width)
                / 2.0,
            y + (i + 2) as f32 * row_size,
            text_params,
        );
        let deaths_text = format!("{}", players[i].deaths);
        draw_text_ex(
            &deaths_text,
            x + width / 2.0
                + (width / 2.0 - measure_text(&deaths_text, Some(font), font_size, 1.0).width)
                    / 2.0,
            y + (i + 2) as f32 * row_size,
            text_params,
        );
    }
}
