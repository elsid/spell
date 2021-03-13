use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use glfw_window::GlfwWindow;
use graphics::*;
use opengl_graphics::{
    Filter,
    GlGraphics,
    GlyphCache,
    OpenGL,
    TextureSettings,
};
use piston::event_loop::{
    Events,
    EventSettings,
};
use piston::EventLoop;
use piston::input::{
    Button,
    Key,
    MouseButton,
    MouseCursorEvent,
    MouseScrollEvent,
    PressEvent,
    ReleaseEvent,
    RenderEvent,
    UpdateEvent,
};
use piston::window::{
    Window,
    WindowSettings,
};

use crate::engine::{add_actor_spell_element, complete_directed_magick, Engine, self_magick,
                    set_actor_moving, start_area_of_effect_magick, start_directed_magick};
use crate::meters::{DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{GameUpdate, PlayerAction};
use crate::vec2::Vec2f;
use crate::world::{Aura, Body, Element, Material, RingSector, World};

pub fn run_game(mut world: World, sender: Option<Sender<PlayerAction>>, receiver: Receiver<GameUpdate>) {
    info!("Run game");
    let opengl = OpenGL::V2_1;
    let mut window: GlfwWindow = WindowSettings::new("spell", [1920, 1080])
        .graphics_api(opengl)
        .exit_on_esc(true)
        .build()
        .unwrap();
    let mut gl = GlGraphics::new(opengl);
    let mut engine = Engine::default();
    let mut events = Events::new(EventSettings::new().max_fps(60).ups(60));
    let mut scale = window.size().height / 20.0;
    let time_step = 1.0 / 60.0;
    let mut last_mouse_pos = Vec2f::ZERO;
    let mut last_viewport_shift = Vec2f::ZERO;
    let mut last_player_position = Vec2f::ZERO;
    let mut last_player_index = None;
    let texture_settings = TextureSettings::new().filter(Filter::Linear);
    let mut glyphs = GlyphCache::new("fonts/UbuntuMono-R.ttf", (), texture_settings)
        .expect("Could not load font");
    let mut eps = FpsMovingAverage::new(100, Duration::from_secs(1));
    let mut render_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut update_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut player_id = None;
    let mut last_received_world_revision = 0;
    let mut last_received_world_time = 0.0;
    let mut lshift = false;

    while let Some(e) = events.next(&mut window) {
        if let Some(v) = e.press_args() {
            match v {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::Move(true)).unwrap());
                        set_actor_moving(player_index, true, &mut world);
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        if lshift {
                            sender.as_ref().map(|v| v.send(PlayerAction::StartAreaOfEffectMagick).unwrap());
                            start_area_of_effect_magick(player_index, &mut world);
                        } else {
                            sender.as_ref().map(|v| v.send(PlayerAction::StartDirectedMagick).unwrap());
                            start_directed_magick(player_index, &mut world);
                        }
                    }
                }
                Button::Keyboard(Key::LShift) => lshift = true,
                _ => (),
            }
        }

        if let Some(v) = e.release_args() {
            match v {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::Move(false)).unwrap());
                        set_actor_moving(player_index, false, &mut world);
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::CompleteDirectedMagick).unwrap());
                        complete_directed_magick(player_index, &mut world);
                    }
                }
                Button::Mouse(MouseButton::Middle) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::SelfMagick).unwrap());
                        self_magick(player_index, &mut world);
                    }
                }
                Button::Keyboard(Key::LShift) => lshift = false,
                Button::Keyboard(Key::Q) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Water)).unwrap());
                        add_actor_spell_element(player_index, Element::Water, &mut world);
                    }
                }
                Button::Keyboard(Key::A) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Lightning)).unwrap());
                        add_actor_spell_element(player_index, Element::Lightning, &mut world);
                    }
                }
                Button::Keyboard(Key::W) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Life)).unwrap());
                        add_actor_spell_element(player_index, Element::Life, &mut world);
                    }
                }
                Button::Keyboard(Key::S) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Arcane)).unwrap());
                        add_actor_spell_element(player_index, Element::Arcane, &mut world);
                    }
                }
                Button::Keyboard(Key::E) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Shield)).unwrap());
                        add_actor_spell_element(player_index, Element::Shield, &mut world);
                    }
                }
                Button::Keyboard(Key::D) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Earth)).unwrap());
                        add_actor_spell_element(player_index, Element::Earth, &mut world);
                    }
                }
                Button::Keyboard(Key::R) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Cold)).unwrap());
                        add_actor_spell_element(player_index, Element::Cold, &mut world);
                    }
                }
                Button::Keyboard(Key::F) => {
                    if let Some(player_index) = last_player_index {
                        sender.as_ref().map(|v| v.send(PlayerAction::AddSpellElement(Element::Fire)).unwrap());
                        add_actor_spell_element(player_index, Element::Fire, &mut world);
                    }
                }
                _ => (),
            }
        }

        if let Some(scroll) = e.mouse_scroll_args() {
            scale *= 1.0 + scroll[1] * 0.1;
        }

        if let Some(args) = e.mouse_cursor_args() {
            last_mouse_pos = Vec2f::new(args[0], args[1]);
        }

        if let Some(_) = e.update_args() {
            while let Ok(update) = receiver.try_recv() {
                match update {
                    GameUpdate::GameOver => player_id = None,
                    GameUpdate::SetPlayerId(v) => player_id = Some(v),
                    GameUpdate::World(v) => {
                        last_received_world_revision = v.revision;
                        last_received_world_time = v.time;
                        world = v;
                        if let Some(player_id) = player_id {
                            last_player_index = world.actors.iter().position(|v| v.id == player_id);
                        }
                    }
                }
            }
            let start = Instant::now();
            if let Some(player_index) = last_player_index {
                let target_direction = (last_mouse_pos - last_viewport_shift) / scale;
                let norm = target_direction.norm();
                if norm <= f64::EPSILON {
                    world.actors[player_index].target_direction = world.actors[player_index].current_direction;
                    sender.as_ref().map(|v| v.send(PlayerAction::SetTargetDirection(world.actors[player_index].current_direction)).unwrap());
                } else {
                    world.actors[player_index].target_direction = target_direction / norm;
                    sender.as_ref().map(|v| v.send(PlayerAction::SetTargetDirection(target_direction / norm)).unwrap());
                }
            }
            engine.update(time_step, &mut world);
            if let Some(player_id) = player_id {
                last_player_index = world.actors.iter().position(|v| v.id == player_id);
            }
            if let Some(player_index) = last_player_index {
                last_player_position = world.actors[player_index].position;
            }
            update_duration.add(Instant::now() - start);
        }

        if let Some(args) = e.render_args() {
            let start = Instant::now();
            let viewport = args.viewport();

            last_viewport_shift = Vec2f::new(viewport.window_size[0] / 2.0, viewport.window_size[1] / 2.0);

            gl.draw(viewport, |ctx, g| {
                let base_transform = ctx.transform
                    .trans(last_viewport_shift.x, last_viewport_shift.y)
                    .scale(scale, scale)
                    .trans(-last_player_position.x, -last_player_position.y);

                clear([0.0, 0.0, 0.0, 1.0], g);

                for v in world.static_areas.iter() {
                    with_body_and_magick(&v.body, &v.magick.power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.temp_areas.iter() {
                    with_body_and_magick(&v.body, &v.effect.power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for area in world.bounded_areas.iter() {
                    let owner = world.actors.iter().find(|v| v.id == area.actor_id).unwrap();
                    with_ring_sector_body_and_magick(&area.body, &area.effect.power, |shape, vertices| {
                        let transform = base_transform.trans(owner.position.x, owner.position.y)
                            .orient(owner.current_direction.x, owner.current_direction.y);
                        draw_ring_sector(shape, vertices, &ctx.draw_state, transform, g);
                    });
                }

                if let Some(player_index) = last_player_index {
                    let target = last_player_position + (last_mouse_pos - last_viewport_shift) / scale;
                    line_from_to(
                        [0.0, 0.0, 0.0, 0.5], 1.0 / scale,
                        [last_player_position.x, last_player_position.y],
                        [target.x, target.y],
                        base_transform,
                        g,
                    );

                    let player = &world.actors[player_index];
                    let current_target = player.position + player.current_direction * player.body.radius * 2.0;
                    line_from_to(
                        [0.0, 0.0, 0.0, 0.5], 1.0 / scale,
                        [last_player_position.x, last_player_position.y],
                        [current_target.x, current_target.y],
                        base_transform,
                        g,
                    );
                }

                for beam in engine.initial_emitted_beams().iter().chain(engine.reflected_emitted_beams().iter()) {
                    let end = beam.origin + beam.direction * beam.length;
                    let line = [beam.origin.x, beam.origin.y, end.x, end.y];
                    let color = get_magick_power_color(&beam.magick.power);
                    let sum_power = beam.magick.power.iter().sum::<f64>() / 20.0;
                    line::Line::new_round(color, sum_power).draw(line, &Default::default(), base_transform, g);
                }

                for v in world.actors.iter() {
                    with_body_and_magick(&v.body, &v.effect.power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    with_body_and_magick(&v.body, &v.effect.power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    with_body_and_magick(&v.body, &v.effect.power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.actors.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.actors.iter() {
                    with_health(v.body.radius, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                    with_power(v.body.radius, v.aura.power / world.settings.max_magic_power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    with_health(v.body.radius, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                    with_power(v.body.radius, v.aura.power / world.settings.max_magic_power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    with_health(v.body.radius, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                    with_power(v.body.radius, v.aura.power / world.settings.max_magic_power, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for actor in world.actors.iter() {
                    let half_width = actor.body.radius * 0.66;
                    let spell_position = actor.position + Vec2f::new(-half_width, actor.body.radius + 0.3);
                    let spell_transform = base_transform.trans(spell_position.x, spell_position.y);
                    let square = rectangle::centered_square(0.0, 0.0, actor.body.radius * 0.1);
                    let element_width = (2.0 * half_width) / 5.0;
                    for (i, element) in actor.spell_elements.iter().enumerate() {
                        let element_position = Vec2f::new((i as f64 + 0.5) * element_width, -actor.body.radius * 0.1);
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(ellipse::Border { color: [0.0, 0.0, 0.0, 1.0], radius: actor.body.radius * 0.01 })
                            .draw(square, &ctx.draw_state, spell_transform.trans(element_position.x, element_position.y), g);
                    }
                }

                rectangle::Rectangle::new_border([1.0, 0.0, 0.0, 0.5], 1.0).draw(
                    rectangle::rectangle_by_corners(
                        world.bounds.min.x - 1.0,
                        world.bounds.min.y - 1.0,
                        world.bounds.max.x + 1.0,
                        world.bounds.max.y + 1.0,
                    ),
                    &ctx.draw_state,
                    base_transform,
                    g,
                );

                if let Some(player_index) = last_player_index {
                    let radius = 20.0;
                    let square = rectangle::centered_square(0.0, 0.0, radius);
                    for (i, element) in world.actors[player_index].spell_elements.iter().enumerate() {
                        let position = last_viewport_shift + Vec2f::new(-5.0 * 2.0 * (radius + 10.0) * 0.5 + (i as f64 + 0.5) * 2.0 * (radius + 10.0), last_viewport_shift.y - 100.0);
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(ellipse::Border { color: [0.0, 0.0, 0.0, 1.0], radius: radius * 0.1 })
                            .draw(square, &ctx.draw_state, ctx.transform.trans(position.x, position.y), g);
                    }
                }

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("EPS: {0:.3}", eps.get())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 1.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Render: {0:.3} ms", render_duration.get() * 1000.0)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 2.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Update: {0:.3} ms", update_duration.get() * 1000.0)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 3.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Player: {:?} {:?}", player_id, last_player_index)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 4.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("World revision: {} (+{})", world.revision, world.revision - last_received_world_revision)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 5.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("World time: {:.3} (+{:.3})", world.time, world.time - last_received_world_time)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 6.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Actors: {}", world.actors.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 7.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Dynamic objects: {}", world.dynamic_objects.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 8.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Static objects: {}", world.static_objects.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 9.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Beams: {}", world.beams.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 10.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Static areas: {}", world.static_areas.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 11.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Temp areas: {}", world.temp_areas.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 12.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Bounded areas: {}", world.bounded_areas.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 13.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                    .draw(&format!("Fields: {}", world.fields.len())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 14.0 * 24.0), g)
                    .unwrap();
            });

            render_duration.add(Instant::now() - start);
        }

        eps.add(Instant::now());
    }
}

fn with_body_and_magick<F: FnMut(ellipse::Ellipse, [f64; 4])>(body: &Body, power: &[f64; 11], mut f: F) {
    let mut shape = ellipse::Ellipse::new(get_material_color(&body.material, 1.0));
    if power.iter().sum::<f64>() > 0.0 {
        shape = shape.border(ellipse::Border { color: get_magick_power_color(power), radius: 0.1 });
    }
    let rect = rectangle::centered_square(0.0, 0.0, body.radius);
    f(shape, rect);
}

fn with_ring_sector_body_and_magick<F: FnMut(polygon::Polygon, types::Polygon)>(body: &RingSector, power: &[f64; 11], mut f: F) {
    const BASE_RESOLUTION: f64 = 12.0;
    let shape = polygon::Polygon::new(get_magick_power_color(power));
    let resolution = (body.angle * BASE_RESOLUTION).round() as usize;
    let min_angle_step = body.angle / (resolution - 1) as f64;
    let max_angle_step = body.angle / resolution as f64;
    let mut vertices = [[0.0, 0.0]; 2 * (std::f64::consts::TAU * BASE_RESOLUTION) as usize + 3];
    for i in 0..resolution {
        let from = Vec2f::only_x(body.max_radius).rotated(i as f64 * max_angle_step - body.angle / 2.0);
        let to = Vec2f::only_x(body.min_radius).rotated(i as f64 * min_angle_step - body.angle / 2.0);
        vertices[2 * i] = [from.x, from.y];
        vertices[2 * i + 1] = [to.x, to.y];
    }
    let last = Vec2f::only_x(body.max_radius).rotated(body.angle / 2.0);
    vertices[2 * resolution] = [last.x, last.y];
    f(shape, &vertices[0..2 * resolution + 1]);
}

fn with_aura<F: FnMut(ellipse::Ellipse, [f64; 4])>(aura: &Aura, mut f: F) {
    let shape = ellipse::Ellipse::new(get_magick_power_color(&aura.elements));
    let rect = rectangle::centered_square(0.0, 0.0, aura.radius);
    f(shape, rect);
}

fn with_health<F: FnMut(rectangle::Rectangle, [f64; 4])>(radius: f64, health: f64, f: F) {
    with_meter(radius, health, 0.5, [1.0, 0.0, 0.0, 1.0], f);
}

fn with_power<F: FnMut(rectangle::Rectangle, [f64; 4])>(radius: f64, power: f64, f: F) {
    with_meter(radius, power, 0.8, [0.0, 0.0, 1.0, 1.0], f);
}

fn with_meter<F>(radius: f64, value: f64, y: f64, color: [f32; 4], mut f: F)
    where F: FnMut(rectangle::Rectangle, [f64; 4])
{
    let half_width = 0.66;
    let background = rectangle::Rectangle::new([0.0, 0.0, 0.0, 0.8]);
    let background_rect = rectangle::rectangle_by_corners(-half_width, radius + y - 0.1, half_width, radius + y + 0.1);
    f(background, background_rect);
    let bar_right = -half_width + 2.0 * half_width * value;
    let health_bar = rectangle::Rectangle::new(color);
    let health_bar_rect = rectangle::rectangle_by_corners(-half_width, radius + y - 0.1, bar_right, radius + y + 0.1);
    f(health_bar, health_bar_rect);
}

fn get_material_color(material: &Material, alpha: f32) -> [f32; 4] {
    match material {
        Material::Flesh => [0.93, 0.89, 0.69, alpha],
        Material::Stone => [0.76, 0.76, 0.76, alpha],
        Material::Dirt => [0.5, 0.38, 0.26, alpha],
        Material::Grass => [0.44, 0.69, 0.15, alpha],
        Material::Water => [0.1, 0.1, 0.9, alpha],
    }
}

fn get_magick_power_color<T: Default + PartialEq>(power: &[T; 11]) -> [f32; 4] {
    let mut result: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let mut colors = 1;
    for i in 0..power.len() {
        if power[i] != T::default() {
            let color = get_element_color(Element::from(i));
            for i in 0..4 {
                result[i] += color[i];
            }
            colors += 1;
        }
    }
    for i in 0..4 {
        result[i] /= colors as f32;
    }
    result
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

fn draw_ring_sector<G>(shape: Polygon, vertices: types::Polygon, draw_state: &DrawState, transform: math::Matrix2d, g: &mut G)
    where G: Graphics
{
    g.tri_list(
        draw_state,
        &shape.color,
        |f| {
            use graphics::triangulation::{tx, ty};
            for i in 1..vertices.len() - 1 {
                let buffer = &vertices[i - 1..i + 2];
                let mut draw_buffer = [[0.0; 2]; 3];
                for i in 0..buffer.len() {
                    let v = &buffer[i];
                    draw_buffer[i] = [tx(transform, v[0], v[1]), ty(transform, v[0], v[1])];
                }
                f(&draw_buffer);
            }
        },
    );
}
