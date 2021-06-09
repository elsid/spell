use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use glfw_window::GlfwWindow;
use graphics::types::FontSize;
use graphics::{
    ellipse, line, math, polygon, rectangle, text, types, CharacterCache, DrawState, Graphics,
    Polygon, Transformed,
};
use opengl_graphics::{Filter, GlGraphics, GlyphCache, OpenGL, TextureSettings};
use piston::event_loop::{EventSettings, Events};
use piston::input::{
    Button, Key, MouseButton, MouseCursorEvent, MouseScrollEvent, PressEvent, ReleaseEvent,
    RenderEvent, UpdateEvent,
};
use piston::window::{Window, WindowSettings};
use piston::EventLoop;

use crate::control::apply_actor_action;
use crate::engine::Engine;
use crate::meters::{DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{apply_world_update, ActorAction, GameUpdate, PlayerUpdate};
use crate::vec2::Vec2f;
use crate::world::{Actor, Aura, Disk, Element, Material, RingSector, StaticShape, World};

const NAME_FONT_SIZE: FontSize = 32;
const NAME_SCALE: f64 = 0.02;

pub struct Server {
    pub address: String,
    pub port: u16,
    pub sender: Sender<PlayerUpdate>,
}

pub fn run_game(initial_world: World, server: Option<Server>, receiver: Receiver<GameUpdate>) {
    info!("Run game");
    let mut world = Box::new(initial_world);
    let opengl = OpenGL::V2_1;
    let mut window: GlfwWindow = WindowSettings::new("spell", [640, 480])
        .graphics_api(opengl)
        .exit_on_esc(true)
        .build()
        .unwrap();
    window.window.maximize();
    let mut gl = GlGraphics::new(opengl);
    let mut engine = Engine::default();
    let mut events = Events::new(EventSettings::new().max_fps(60).ups(60));
    let mut scale = window.size().height / 20.0;
    let time_step = 1.0 / 60.0;
    let mut last_mouse_pos = Vec2f::ZERO;
    let mut last_viewport_shift = Vec2f::ZERO;
    let mut last_actor_position = Vec2f::ZERO;
    let mut last_actor_index = None;
    let texture_settings = TextureSettings::new().filter(Filter::Linear);
    let mut glyphs = GlyphCache::new("fonts/UbuntuMono-R.ttf", (), texture_settings)
        .expect("Could not load font");
    let mut eps = FpsMovingAverage::new(100, Duration::from_secs(1));
    let mut render_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut update_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut actor_id = None;
    let mut local_world_frame = 0;
    let mut local_world_time = 0.0;
    let mut lshift = false;
    let mut show_debug_info = false;
    let sender = server.as_ref().map(|v| &v.sender);

    while let Some(event) = events.next(&mut window) {
        if let Some(button) = event.press_args() {
            match button {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::Move(true),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(actor_index) = last_actor_index {
                        if lshift {
                            send_or_apply_actor_action(
                                sender,
                                ActorAction::StartAreaOfEffectMagick,
                                actor_index,
                                &mut world,
                            );
                        } else {
                            send_or_apply_actor_action(
                                sender,
                                ActorAction::StartDirectedMagick,
                                actor_index,
                                &mut world,
                            );
                        }
                    }
                }
                Button::Keyboard(Key::LShift) => lshift = true,
                _ => (),
            }
        }

        if let Some(button) = event.release_args() {
            match button {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::Move(false),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::CompleteDirectedMagick,
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Middle) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::SelfMagick,
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::LShift) => lshift = false,
                Button::Keyboard(Key::Q) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Water),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::A) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Lightning),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::W) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Life),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::S) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Arcane),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::E) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Shield),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::D) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Earth),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::R) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Cold),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::F) => {
                    if let Some(actor_index) = last_actor_index {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::AddSpellElement(Element::Fire),
                            actor_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::F2) => {
                    show_debug_info = !show_debug_info;
                }
                _ => (),
            }
        }

        if let Some(scroll) = event.mouse_scroll_args() {
            scale *= 1.0 + scroll[1] * 0.1;
        }

        if let Some(args) = event.mouse_cursor_args() {
            last_mouse_pos = Vec2f::new(args[0], args[1]);
        }

        if event.update_args().is_some() {
            let start = Instant::now();
            while let Ok(update) = receiver.try_recv() {
                match update {
                    GameUpdate::GameOver(..) => actor_id = None,
                    GameUpdate::SetActorId(v) => actor_id = Some(v),
                    GameUpdate::WorldSnapshot(v) => {
                        world = v;
                        if let Some(player_id) = actor_id {
                            last_actor_index = world.actors.iter().position(|v| v.id == player_id);
                        }
                    }
                    GameUpdate::WorldUpdate(world_update) => {
                        apply_world_update(*world_update, &mut world);
                        if let Some(player_id) = actor_id {
                            last_actor_index = world.actors.iter().position(|v| v.id == player_id);
                        }
                    }
                }
            }
            if let Some(sender) = sender.as_ref() {
                if let Err(..) = sender.send(PlayerUpdate::AckWorldFrame(world.frame)) {
                    break;
                }
            }
            if let Some(actor_index) = last_actor_index {
                let target_direction = (last_mouse_pos - last_viewport_shift) / scale;
                let norm = target_direction.norm();
                if norm > f64::EPSILON {
                    let direction = target_direction / norm;
                    if direction != world.actors[actor_index].target_direction {
                        send_or_apply_actor_action(
                            sender,
                            ActorAction::SetTargetDirection(direction),
                            actor_index,
                            &mut world,
                        );
                    }
                }
            }
            if sender.is_some() {
                local_world_frame = (local_world_frame + 1).max(world.frame);
                local_world_time += time_step;
                if local_world_time < world.time {
                    local_world_time = world.time;
                }
                engine.update_visual(&mut world);
            } else {
                engine.update(time_step, &mut world);
            }
            if let Some(player_id) = actor_id {
                last_actor_index = world.actors.iter().position(|v| v.id == player_id);
            }
            if let Some(actor_index) = last_actor_index {
                last_actor_position = world.actors[actor_index].position;
            }
            update_duration.add(Instant::now() - start);
        }

        if let Some(render_args) = event.render_args() {
            let start = Instant::now();
            let viewport = render_args.viewport();

            last_viewport_shift =
                Vec2f::new(viewport.window_size[0] / 2.0, viewport.window_size[1] / 2.0);

            gl.draw(viewport, |ctx, g| {
                let base_transform = ctx
                    .transform
                    .trans(last_viewport_shift.x, last_viewport_shift.y)
                    .scale(scale, scale)
                    .trans(-last_actor_position.x, -last_actor_position.y);

                graphics::clear([0.0, 0.0, 0.0, 1.0], g);

                for v in world.static_areas.iter() {
                    with_disk_body_and_magick(
                        v.body.material,
                        &v.body.shape,
                        &v.magick.power,
                        world.settings.border_width,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for v in world.temp_areas.iter() {
                    with_disk_body_and_magick(
                        v.body.material,
                        &v.body.shape,
                        &v.effect.power,
                        world.settings.border_width,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for area in world.bounded_areas.iter() {
                    let owner = world.actors.iter().find(|v| v.id == area.actor_id).unwrap();
                    with_ring_sector_body_and_magick(
                        &area.body,
                        &area.effect.power,
                        |shape, vertices| {
                            let transform = base_transform
                                .trans(owner.position.x, owner.position.y)
                                .orient(owner.current_direction.x, owner.current_direction.y);
                            draw_ring_sector(shape, vertices, &ctx.draw_state, transform, g);
                        },
                    );
                }

                if let Some(actor_index) = last_actor_index {
                    let target =
                        last_actor_position + (last_mouse_pos - last_viewport_shift) / scale;
                    graphics::line_from_to(
                        [0.0, 0.0, 0.0, 0.5],
                        1.0 / scale,
                        [last_actor_position.x, last_actor_position.y],
                        [target.x, target.y],
                        base_transform,
                        g,
                    );

                    let player = &world.actors[actor_index];
                    let current_target =
                        player.position + player.current_direction * player.body.shape.radius * 2.0;
                    graphics::line_from_to(
                        [0.0, 0.0, 0.0, 0.5],
                        1.0 / scale,
                        [last_actor_position.x, last_actor_position.y],
                        [current_target.x, current_target.y],
                        base_transform,
                        g,
                    );
                }

                for beam in engine
                    .initial_emitted_beams()
                    .iter()
                    .chain(engine.reflected_emitted_beams().iter())
                {
                    let end = beam.origin + beam.direction * beam.length;
                    let line = [beam.origin.x, beam.origin.y, end.x, end.y];
                    let color = get_magick_power_color(&beam.magick.power);
                    let sum_power = beam.magick.power.iter().sum::<f64>() / 20.0;
                    line::Line::new_round(color, sum_power).draw(
                        line,
                        &Default::default(),
                        base_transform,
                        g,
                    );
                }

                for v in world.actors.iter() {
                    with_disk_body_and_magick(
                        v.body.material,
                        &v.body.shape,
                        &v.effect.power,
                        world.settings.border_width,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for v in world.dynamic_objects.iter() {
                    with_disk_body_and_magick(
                        v.body.material,
                        &v.body.shape,
                        &v.effect.power,
                        world.settings.border_width,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for v in world.static_objects.iter() {
                    match &v.body.shape {
                        StaticShape::CircleArc(arc) => {
                            let ring_sector = RingSector {
                                min_radius: arc.radius - world.settings.border_width,
                                max_radius: arc.radius + world.settings.border_width,
                                angle: arc.length,
                            };
                            with_ring_sector_body_and_magick(
                                &ring_sector,
                                &v.effect.power,
                                |shape, vertices| {
                                    let transform = base_transform
                                        .trans(v.position.x, v.position.y)
                                        .rot_rad(arc.rotation);
                                    draw_ring_sector(
                                        shape,
                                        vertices,
                                        &ctx.draw_state,
                                        transform,
                                        g,
                                    );
                                },
                            );
                            with_ring_sector_body_and_magick(
                                &ring_sector,
                                &v.aura.elements,
                                |shape, vertices| {
                                    let transform = base_transform
                                        .trans(v.position.x, v.position.y)
                                        .rot_rad(arc.rotation);
                                    draw_ring_sector(
                                        shape,
                                        vertices,
                                        &ctx.draw_state,
                                        transform,
                                        g,
                                    );
                                },
                            );
                        }
                        StaticShape::Disk(shape) => {
                            with_disk_body_and_magick(
                                v.body.material,
                                shape,
                                &v.effect.power,
                                world.settings.border_width,
                                |shape, rect| {
                                    shape.draw(
                                        rect,
                                        &ctx.draw_state,
                                        base_transform.trans(v.position.x, v.position.y),
                                        g,
                                    );
                                },
                            );
                        }
                    }
                }

                for v in world.actors.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                }

                for v in world.dynamic_objects.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                }

                for v in world.static_objects.iter() {
                    with_aura(&v.aura, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                }

                for v in world.actors.iter() {
                    with_health(v.body.shape.radius, v.health, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                    with_power(
                        v.body.shape.radius,
                        v.aura.power / world.settings.max_magic_power,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for v in world.dynamic_objects.iter() {
                    with_health(v.body.shape.radius, v.health, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                    with_power(
                        v.body.shape.radius,
                        v.aura.power / world.settings.max_magic_power,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for v in world.static_objects.iter() {
                    let radius = match &v.body.shape {
                        StaticShape::CircleArc(v) => v.radius,
                        StaticShape::Disk(v) => v.radius,
                    };
                    with_health(radius, v.health, |shape, rect| {
                        shape.draw(
                            rect,
                            &ctx.draw_state,
                            base_transform.trans(v.position.x, v.position.y),
                            g,
                        );
                    });
                    with_power(
                        radius,
                        v.aura.power / world.settings.max_magic_power,
                        |shape, rect| {
                            shape.draw(
                                rect,
                                &ctx.draw_state,
                                base_transform.trans(v.position.x, v.position.y),
                                g,
                            );
                        },
                    );
                }

                for actor in world.actors.iter() {
                    let half_width = actor.body.shape.radius * 0.66;
                    let spell_position =
                        actor.position + Vec2f::new(-half_width, actor.body.shape.radius + 0.3);
                    let spell_transform = base_transform.trans(spell_position.x, spell_position.y);
                    let square =
                        rectangle::centered_square(0.0, 0.0, actor.body.shape.radius * 0.1);
                    let element_width = (2.0 * half_width) / 5.0;
                    for (i, element) in actor.spell_elements.iter().enumerate() {
                        let element_position = Vec2f::new(
                            (i as f64 + 0.5) * element_width,
                            -actor.body.shape.radius * 0.1,
                        );
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(ellipse::Border {
                                color: [0.0, 0.0, 0.0, 1.0],
                                radius: actor.body.shape.radius * 0.01,
                            })
                            .draw(
                                square,
                                &ctx.draw_state,
                                spell_transform.trans(element_position.x, element_position.y),
                                g,
                            );
                    }
                }

                for actor in world.actors.iter() {
                    if Some(actor.id) != actor_id {
                        draw_name(&actor, &ctx.draw_state, &base_transform, &mut glyphs, g)
                            .unwrap();
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

                if let Some(actor_index) = last_actor_index {
                    let radius = 20.0;
                    let square = rectangle::centered_square(0.0, 0.0, radius);
                    for (i, element) in world.actors[actor_index].spell_elements.iter().enumerate()
                    {
                        let position = last_viewport_shift
                            + Vec2f::new(
                                -5.0 * 2.0 * (radius + 10.0) * 0.5
                                    + (i as f64 + 0.5) * 2.0 * (radius + 10.0),
                                last_viewport_shift.y - 100.0,
                            );
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(ellipse::Border {
                                color: [0.0, 0.0, 0.0, 1.0],
                                radius: radius * 0.1,
                            })
                            .draw(
                                square,
                                &ctx.draw_state,
                                ctx.transform.trans(position.x, position.y),
                                g,
                            );
                    }
                }

                if show_debug_info {
                    let format_eps = || Some(format!("Events/s: {0:.3}", eps.get()));
                    let format_render =
                        || Some(format!("Render: {0:.3} ms", render_duration.get() * 1000.0));
                    let format_update =
                        || Some(format!("Update: {0:.3} ms", update_duration.get() * 1000.0));
                    let format_server = || {
                        server
                            .as_ref()
                            .map(|v| format!("Server: {}:{}", v.address, v.port))
                    };
                    let format_world_frame = || {
                        Some(if server.is_some() {
                            format!(
                                "World frame: {} (+{})",
                                world.frame,
                                local_world_frame - world.frame
                            )
                        } else {
                            format!("World frame: {}", world.frame)
                        })
                    };
                    let format_world_time = || {
                        Some(if server.is_some() {
                            format!(
                                "World time: {:.3} (+{:.3})",
                                world.time,
                                local_world_time - world.time
                            )
                        } else {
                            format!("World time: {:.3}", world.time)
                        })
                    };
                    let format_player =
                        || Some(format!("Player: {:?} {:?}", actor_id, last_actor_index));
                    let format_actors = || Some(format!("Actors: {}", world.actors.len()));
                    let format_dynamic_objects =
                        || Some(format!("Dynamic objects: {}", world.dynamic_objects.len()));
                    let format_static_objects =
                        || Some(format!("Static objects: {}", world.static_objects.len()));
                    let format_beams = || Some(format!("Beams: {}", world.beams.len()));
                    let format_static_areas =
                        || Some(format!("Static areas: {}", world.static_areas.len()));
                    let format_temp_areas =
                        || Some(format!("Temp areas: {}", world.temp_areas.len()));
                    let format_bounded_areas =
                        || Some(format!("Bounded areas: {}", world.bounded_areas.len()));
                    let format_fields = || Some(format!("Fields: {}", world.fields.len()));

                    type FormatRef<'a> = &'a dyn Fn() -> Option<String>;

                    let formats: &[FormatRef] = &[
                        &format_eps as FormatRef,
                        &format_render as FormatRef,
                        &format_update as FormatRef,
                        &format_server as FormatRef,
                        &format_world_frame as FormatRef,
                        &format_world_time as FormatRef,
                        &format_player as FormatRef,
                        &format_actors as FormatRef,
                        &format_dynamic_objects as FormatRef,
                        &format_static_objects as FormatRef,
                        &format_beams as FormatRef,
                        &format_static_areas as FormatRef,
                        &format_temp_areas as FormatRef,
                        &format_bounded_areas as FormatRef,
                        &format_fields as FormatRef,
                    ];

                    let mut text_counter = 0;
                    for f in formats.iter() {
                        if let Some(text) = f() {
                            text_counter += 1;
                            text::Text::new_color([1.0, 1.0, 1.0, 1.0], 20)
                                .draw(
                                    &text[..],
                                    &mut glyphs,
                                    &ctx.draw_state,
                                    ctx.transform.trans(10.0, (4 + text_counter * 24) as f64),
                                    g,
                                )
                                .unwrap();
                        }
                    }
                }
            });

            render_duration.add(Instant::now() - start);
        }

        eps.add(Instant::now());
    }
    info!("Game has stopped");
}

fn with_disk_body_and_magick<F>(
    material: Material,
    shape: &Disk,
    power: &[f64; 11],
    border_width: f64,
    mut f: F,
) where
    F: FnMut(&ellipse::Ellipse, types::Rectangle),
{
    let mut drawable = ellipse::Ellipse::new(get_material_color(material, 1.0));
    if power.iter().sum::<f64>() > 0.0 {
        drawable = drawable.border(ellipse::Border {
            color: get_magick_power_color(power),
            radius: border_width,
        });
    }
    let rect = rectangle::centered_square(0.0, 0.0, shape.radius);
    f(&drawable, rect);
}

fn with_ring_sector_body_and_magick<T, F>(body: &RingSector, power: &[T; 11], mut f: F)
where
    F: FnMut(polygon::Polygon, types::Polygon),
    T: Default + PartialEq,
{
    const BASE_RESOLUTION: f64 = 12.0;
    let shape = polygon::Polygon::new(get_magick_power_color(power));
    let resolution = (body.angle * BASE_RESOLUTION).round() as usize;
    let min_angle_step = body.angle / (resolution - 1) as f64;
    let max_angle_step = body.angle / resolution as f64;
    let mut vertices = [[0.0, 0.0]; 2 * (std::f64::consts::TAU * BASE_RESOLUTION) as usize + 3];
    for i in 0..resolution {
        let from =
            Vec2f::only_x(body.max_radius).rotated(i as f64 * max_angle_step - body.angle / 2.0);
        let to =
            Vec2f::only_x(body.min_radius).rotated(i as f64 * min_angle_step - body.angle / 2.0);
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
where
    F: FnMut(rectangle::Rectangle, [f64; 4]),
{
    let half_width = 0.66;
    let background = rectangle::Rectangle::new([0.0, 0.0, 0.0, 0.8]);
    let background_rect = rectangle::rectangle_by_corners(
        -half_width,
        radius + y - 0.1,
        half_width,
        radius + y + 0.1,
    );
    f(background, background_rect);
    let bar_right = -half_width + 2.0 * half_width * value;
    let health_bar = rectangle::Rectangle::new(color);
    let health_bar_rect =
        rectangle::rectangle_by_corners(-half_width, radius + y - 0.1, bar_right, radius + y + 0.1);
    f(health_bar, health_bar_rect);
}

fn get_material_color(material: Material, alpha: f32) -> [f32; 4] {
    match material {
        Material::None => [0.0, 0.0, 0.0, alpha],
        Material::Flesh => [0.93, 0.89, 0.69, alpha],
        Material::Stone => [0.76, 0.76, 0.76, alpha],
        Material::Dirt => [0.5, 0.38, 0.26, alpha],
        Material::Grass => [0.44, 0.69, 0.15, alpha],
        Material::Water => [0.1, 0.1, 0.9, alpha],
    }
}

fn get_magick_power_color<T: Default + PartialEq>(power: &[T; 11]) -> [f32; 4] {
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
        return [0.0, 0.0, 0.0, 0.0];
    }
    result.iter_mut().for_each(|v| *v /= colors as f32);
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

fn draw_ring_sector<G>(
    shape: Polygon,
    vertices: types::Polygon,
    draw_state: &DrawState,
    transform: math::Matrix2d,
    g: &mut G,
) where
    G: Graphics,
{
    g.tri_list(draw_state, &shape.color, |f| {
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
    });
}

fn draw_name<C, G>(
    actor: &Actor,
    draw_state: &DrawState,
    base_transform: &math::Matrix2d,
    cache: &mut C,
    g: &mut G,
) -> Result<(), C::Error>
where
    C: CharacterCache,
    G: Graphics<Texture = <C as CharacterCache>::Texture>,
{
    let width = cache.width(NAME_FONT_SIZE, actor.name.as_str())?;
    text::Text::new_color([1.0, 1.0, 1.0, 0.8], NAME_FONT_SIZE).draw(
        &actor.name[..],
        cache,
        draw_state,
        base_transform
            .trans(
                actor.position.x - NAME_SCALE * width / 2.0,
                actor.position.y - actor.body.shape.radius - 0.3,
            )
            .scale(NAME_SCALE, NAME_SCALE),
        g,
    )
}

fn send_or_apply_actor_action(
    sender: Option<&Sender<PlayerUpdate>>,
    actor_action: ActorAction,
    actor_index: usize,
    world: &mut World,
) {
    if let Some(s) = sender {
        s.send(PlayerUpdate::Action(actor_action)).unwrap();
    } else {
        apply_actor_action(&actor_action, actor_index, world);
    }
}
