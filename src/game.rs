use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use conrod_core::position::Place;
use conrod_core::{widget, Positionable, Scalar, Widget};
use glfw_window::GlfwWindow;
use piston_window::texture::UpdateTexture;
use piston_window::{
    ellipse, line, math, polygon, rectangle, types, Button, DrawState, EventLoop, EventSettings,
    G2d, G2dTexture, Graphics, Key, MouseButton, MouseCursorEvent, MouseScrollEvent, OpenGL,
    PistonWindow, Polygon, PressEvent, ReleaseEvent, TextureSettings, Transformed, UpdateEvent,
    Window, WindowSettings,
};

use crate::control::apply_player_action;
use crate::engine::Engine;
use crate::meters::{DurationMovingAverage, FpsMovingAverage};
use crate::protocol::{apply_world_update, GameUpdate, PlayerAction, PlayerUpdate};
use crate::vec2::Vec2f;
use crate::world::{Aura, Disk, Element, Material, RingSector, StaticShape, World};

pub struct Server {
    pub address: String,
    pub port: u16,
    pub sender: Sender<PlayerUpdate>,
}

pub fn run_game(mut world: World, server: Option<Server>, receiver: Receiver<GameUpdate>) {
    info!("Run game");
    let mut window: PistonWindow<GlfwWindow> = WindowSettings::new("spell", [640, 480])
        .graphics_api(OpenGL::V3_2)
        .exit_on_esc(true)
        .build()
        .unwrap();
    window.window.window.maximize();
    window.set_event_settings(EventSettings::new().max_fps(60).ups(60));
    let mut ui = conrod_core::UiBuilder::new([window.size().width, window.size().height])
        .theme(make_theme())
        .build();
    ui.fonts.insert_from_file("fonts/UbuntuMono-R.ttf").unwrap();
    let widget_ids = WidgetIds::new(ui.widget_id_generator());
    let mut texture_context = window.create_texture_context();
    let mut text_vertex_data = Vec::new();
    let (mut glyph_cache, mut text_texture_cache) = {
        let cache = conrod_core::text::GlyphCache::builder().build();
        let (width, height) = cache.dimensions();
        let init = vec![128; width as usize * height as usize];
        let settings = TextureSettings::new();
        let texture =
            G2dTexture::from_memory_alpha(&mut texture_context, &init, width, height, &settings)
                .unwrap();
        (cache, texture)
    };
    let image_map = conrod_core::image::Map::new();

    let mut engine = Engine::default();
    let mut scale = window.size().height / 20.0;
    let time_step = 1.0 / 60.0;
    let mut last_mouse_pos = Vec2f::ZERO;
    let mut last_viewport_shift = Vec2f::ZERO;
    let mut last_player_position = Vec2f::ZERO;
    let mut last_player_index = None;
    let mut eps = FpsMovingAverage::new(100, Duration::from_secs(1));
    let mut render_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut update_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut player_id = None;
    let mut local_world_revision = 0;
    let mut local_world_time = 0.0;
    let mut lshift = false;
    let mut show_debug_info = false;
    let sender = server.as_ref().map(|v| &v.sender);

    while let Some(event) = window.next() {
        let size = window.size();

        if let Some(e) = conrod_piston::event::convert(
            event.clone(),
            size.width as Scalar,
            size.height as Scalar,
        ) {
            ui.handle_event(e);
        }

        if let Some(button) = event.press_args() {
            match button {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::Move(true),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        if lshift {
                            send_or_apply_player_action(
                                sender,
                                PlayerAction::StartAreaOfEffectMagick,
                                player_index,
                                &mut world,
                            );
                        } else {
                            send_or_apply_player_action(
                                sender,
                                PlayerAction::StartDirectedMagick,
                                player_index,
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
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::Move(false),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::CompleteDirectedMagick,
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Mouse(MouseButton::Middle) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::SelfMagick,
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::LShift) => lshift = false,
                Button::Keyboard(Key::Q) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Water),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::A) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Lightning),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::W) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Life),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::S) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Arcane),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::E) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Shield),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::D) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Earth),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::R) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Cold),
                            player_index,
                            &mut world,
                        );
                    }
                }
                Button::Keyboard(Key::F) => {
                    if let Some(player_index) = last_player_index {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::AddSpellElement(Element::Fire),
                            player_index,
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
                    GameUpdate::GameOver => player_id = None,
                    GameUpdate::SetPlayerId(v) => player_id = Some(v),
                    GameUpdate::WorldSnapshot(v) => {
                        world = v;
                        if let Some(player_id) = player_id {
                            last_player_index = world.actors.iter().position(|v| v.id == player_id);
                        }
                    }
                    GameUpdate::WorldUpdate(world_update) => {
                        apply_world_update(world_update, &mut world);
                        if let Some(player_id) = player_id {
                            last_player_index = world.actors.iter().position(|v| v.id == player_id);
                        }
                    }
                }
            }
            if let Some(sender) = sender.as_ref() {
                if let Err(..) = sender.send(PlayerUpdate::AckWorldRevision(world.revision)) {
                    break;
                }
            }
            if let Some(player_index) = last_player_index {
                let target_direction = (last_mouse_pos - last_viewport_shift) / scale;
                let norm = target_direction.norm();
                if norm > f64::EPSILON {
                    let direction = target_direction / norm;
                    if direction != world.actors[player_index].target_direction {
                        send_or_apply_player_action(
                            sender,
                            PlayerAction::SetTargetDirection(direction),
                            player_index,
                            &mut world,
                        );
                    }
                }
            }
            if sender.is_some() {
                local_world_revision = (local_world_revision + 1).max(world.revision);
                local_world_time += time_step;
                if local_world_time < world.time {
                    local_world_time = world.time;
                }
                engine.update_visual(&mut world);
            } else {
                engine.update(time_step, &mut world);
            }
            if let Some(player_id) = player_id {
                last_player_index = world.actors.iter().position(|v| v.id == player_id);
            }
            if let Some(player_index) = last_player_index {
                last_player_position = world.actors[player_index].position;
            }
            let mut ui_cell = ui.set_widgets();

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
                let format_world_revision = || {
                    Some(if server.is_some() {
                        format!(
                            "World revision: {} (+{})",
                            world.revision,
                            local_world_revision - world.revision
                        )
                    } else {
                        format!("World revision: {}", world.revision)
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
                    || Some(format!("Player: {:?} {:?}", player_id, last_player_index));
                let format_actors = || Some(format!("Actors: {}", world.actors.len()));
                let format_dynamic_objects =
                    || Some(format!("Dynamic objects: {}", world.dynamic_objects.len()));
                let format_static_objects =
                    || Some(format!("Static objects: {}", world.static_objects.len()));
                let format_beams = || Some(format!("Beams: {}", world.beams.len()));
                let format_static_areas =
                    || Some(format!("Static areas: {}", world.static_areas.len()));
                let format_temp_areas = || Some(format!("Temp areas: {}", world.temp_areas.len()));
                let format_bounded_areas =
                    || Some(format!("Bounded areas: {}", world.bounded_areas.len()));
                let format_fields = || Some(format!("Fields: {}", world.fields.len()));

                type FormatRef<'a> = &'a dyn Fn() -> Option<String>;

                let formats: &[FormatRef] = &[
                    &format_eps as FormatRef,
                    &format_render as FormatRef,
                    &format_update as FormatRef,
                    &format_server as FormatRef,
                    &format_world_revision as FormatRef,
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

                let text = formats.iter().fold(String::new(), |r, f| {
                    if let Some(v) = f() {
                        format!("{}\n{}", r, v)
                    } else {
                        r
                    }
                });
                widget::Canvas::new().set(widget_ids.canvas, &mut ui_cell);
                widget::Text::new(text.as_str())
                    .x_place_on(widget_ids.canvas, Place::Start(Some(10.0)))
                    .y_place_on(widget_ids.canvas, Place::End(Some(4.0)))
                    .line_spacing(4.0)
                    .set(widget_ids.debug_info, &mut ui_cell);
            }

            update_duration.add(Instant::now() - start);
        }

        window.draw_2d(&event, |ctx, g, device| {
            if let Some(viewport) = ctx.viewport.as_ref() {
                let start = Instant::now();

                last_viewport_shift =
                    Vec2f::new(viewport.window_size[0] / 2.0, viewport.window_size[1] / 2.0);

                let base_transform = ctx
                    .transform
                    .trans(last_viewport_shift.x, last_viewport_shift.y)
                    .scale(scale, scale)
                    .trans(-last_player_position.x, -last_player_position.y);

                piston_window::clear([0.0, 0.0, 0.0, 1.0], g);

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

                if let Some(player_index) = last_player_index {
                    let target =
                        last_player_position + (last_mouse_pos - last_viewport_shift) / scale;
                    piston_window::line_from_to(
                        [0.0, 0.0, 0.0, 0.5],
                        1.0 / scale,
                        [last_player_position.x, last_player_position.y],
                        [target.x, target.y],
                        base_transform,
                        g,
                    );

                    let player = &world.actors[player_index];
                    let current_target =
                        player.position + player.current_direction * player.body.shape.radius * 2.0;
                    piston_window::line_from_to(
                        [0.0, 0.0, 0.0, 0.5],
                        1.0 / scale,
                        [last_player_position.x, last_player_position.y],
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
                    for (i, element) in world.actors[player_index].spell_elements.iter().enumerate()
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

                let primitives = ui.draw();
                let cache_queued_glyphs = |_: &mut G2d,
                                           cache: &mut G2dTexture,
                                           rect: conrod_core::text::rt::Rect<u32>,
                                           data: &[u8]| {
                    let offset = [rect.min.x, rect.min.y];
                    let size = [rect.width(), rect.height()];
                    let format = piston_window::texture::Format::Rgba8;
                    text_vertex_data.clear();
                    text_vertex_data.extend(data.iter().flat_map(|&b| vec![255, 255, 255, b]));
                    UpdateTexture::update(
                        cache,
                        &mut texture_context,
                        format,
                        &text_vertex_data[..],
                        offset,
                        size,
                    )
                    .expect("failed to update texture")
                };

                fn texture_from_image<T>(img: &T) -> &T {
                    img
                }

                conrod_piston::draw::primitives(
                    primitives,
                    ctx,
                    g,
                    &mut text_texture_cache,
                    &mut glyph_cache,
                    &image_map,
                    cache_queued_glyphs,
                    texture_from_image,
                );

                texture_context.encoder.flush(device);

                render_duration.add(Instant::now() - start);
            }
        });

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
        use piston_window::triangulation::{tx, ty};
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

fn send_or_apply_player_action(
    sender: Option<&Sender<PlayerUpdate>>,
    player_action: PlayerAction,
    actor_index: usize,
    world: &mut World,
) {
    if let Some(s) = sender {
        s.send(PlayerUpdate::Action(player_action)).unwrap();
    } else {
        apply_player_action(&player_action, actor_index, world);
    }
}

widget_ids! {
    pub struct WidgetIds {
        canvas,
        debug_info,
    }
}

pub fn make_theme() -> conrod_core::Theme {
    use conrod_core::position::{Align, Direction, Padding, Position, Relative};
    conrod_core::Theme {
        name: "Spell".to_string(),
        padding: Padding::none(),
        x_position: Position::Relative(Relative::Align(Align::Start), None),
        y_position: Position::Relative(Relative::Direction(Direction::Backwards, 20.0), None),
        background_color: conrod_core::color::TRANSPARENT,
        shape_color: conrod_core::color::LIGHT_CHARCOAL,
        border_color: conrod_core::color::BLACK,
        border_width: 0.0,
        label_color: conrod_core::color::WHITE,
        font_id: None,
        font_size_large: 26,
        font_size_medium: 22,
        font_size_small: 18,
        widget_styling: conrod_core::theme::StyleMap::default(),
        mouse_drag_threshold: 0.0,
        double_click_threshold: std::time::Duration::from_millis(500),
    }
}
