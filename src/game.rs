use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use glfw_window::GlfwWindow;
use graphics::*;
use itertools::Itertools;
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

use crate::engine::{add_actor_spell_element, complete_directed_magick, Engine, self_magick, set_actor_moving, start_directed_magick};
use crate::meters::{DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{GameUpdate, PlayerAction};
use crate::vec2::Vec2f;
use crate::world::{
    Aura,
    Body,
    Effect,
    Element,
    Material,
    World,
};

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
                        sender.as_ref().map(|v| v.send(PlayerAction::StartDirectedMagick).unwrap());
                        start_directed_magick(player_index, &mut world);
                    }
                }
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
                            last_player_index = world.actors.iter()
                                .find_position(|v| v.id == player_id)
                                .map(|(i, _)| i);
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
                last_player_index = world.actors.iter()
                    .find_position(|v| v.id == player_id)
                    .map(|(i, _)| i);
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

                clear([1.0, 1.0, 1.0, 1.0], g);

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
                    draw_body_and_effect(&v.body, &v.effect, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    draw_body_and_effect(&v.body, &v.effect, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    draw_body_and_effect(&v.body, &v.effect, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.actors.iter() {
                    draw_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    draw_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    draw_aura(&v.aura, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.actors.iter() {
                    draw_health(&v.body, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.dynamic_objects.iter() {
                    draw_health(&v.body, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for v in world.static_objects.iter() {
                    draw_health(&v.body, v.health, |shape, rect| {
                        shape.draw(rect, &ctx.draw_state, base_transform.trans(v.position.x, v.position.y), g);
                    });
                }

                for actor in world.actors.iter() {
                    let half_width = actor.body.radius * 0.66;
                    let spell_position = actor.position + Vec2f::new(-half_width, actor.body.radius + 1.0);
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

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("EPS: {0:.3}", eps.get())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 0.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("Render: {0:.3} ms", render_duration.get() * 1000.0)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 1.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("Update: {0:.3} ms", update_duration.get() * 1000.0)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 2.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("Player: {:?} {:?}", player_id, last_player_index)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 3.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("World revision: {} (+{})", world.revision, world.revision - last_received_world_revision)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 4.0 * 24.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("World time: {:.3} (+{:.3})", world.time, world.time - last_received_world_time)[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0 + 5.0 * 24.0), g)
                    .unwrap();
            });

            render_duration.add(Instant::now() - start);
        }

        eps.add(Instant::now());
    }
}

fn draw_body_and_effect<F: FnMut(ellipse::Ellipse, [f64; 4])>(body: &Body, effect: &Effect, mut f: F) {
    let shape = ellipse::Ellipse::new(get_material_color(&body.material, 1.0))
        .border(ellipse::Border { color: get_magick_power_color(&effect.power), radius: 0.1 });
    let rect = rectangle::centered_square(0.0, 0.0, body.radius);
    f(shape, rect);
}

fn draw_aura<F: FnMut(ellipse::Ellipse, [f64; 4])>(aura: &Aura, mut f: F) {
    let shape = ellipse::Ellipse::new(get_magick_power_color(&aura.elements));
    let rect = rectangle::centered_square(0.0, 0.0, aura.radius);
    f(shape, rect);
}

fn draw_health<F: FnMut(rectangle::Rectangle, [f64; 4])>(body: &Body, health: f64, mut f: F) {
    let shift = body.radius + 0.5;
    let half_width = body.radius * 0.66;
    let bar_right = -half_width + 2.0 * half_width * health;
    let background = rectangle::Rectangle::new([0.0, 0.0, 0.0, 0.8]);
    let background_rect = rectangle::rectangle_by_corners(-half_width, shift - body.radius * 0.1, half_width, shift + body.radius * 0.1);
    f(background, background_rect);
    let health_bar = rectangle::Rectangle::new([1.0, 0.0, 0.0, 1.0]);
    let health_bar_rect = rectangle::rectangle_by_corners(-half_width, shift - body.radius * 0.1, bar_right, shift + body.radius * 0.1);
    f(health_bar, health_bar_rect);
}

fn get_material_color(material: &Material, alpha: f32) -> [f32; 4] {
    match material {
        Material::Flesh => [0.93, 0.89, 0.69, alpha],
        Material::Stone => [0.76, 0.76, 0.76, alpha],
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
