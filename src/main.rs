use std::collections::VecDeque;
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
use rand::Rng;

use crate::rect::Rectf;
use crate::vec2::Vec2f;
use crate::world::{
    Body,
    Element,
    get_body_mass,
    get_circle_volume,
    get_material_restitution,
    GRAVITY_CONST,
    HEALTH_FACTOR,
    Id,
    Material,
    SHIFT_FACTOR,
    World,
};
use graphics::ellipse::Border;

mod vec2;
mod world;
mod rect;
mod circle;
mod segment;

fn main() {
    let opengl = OpenGL::V4_5;
    let mut window: GlfwWindow = WindowSettings::new("spell", [1920, 1080])
        .graphics_api(opengl)
        .exit_on_esc(true)
        .build()
        .unwrap();
    let mut gl = GlGraphics::new(opengl);
    let mut rng = rand::thread_rng();
    let bounds = Rectf::new(Vec2f::both(-1e2), Vec2f::both(1e2));
    let mut world = World::new(bounds, GRAVITY_CONST);
    let (player_id, initial_player_index) = world.add_body(
        Body {
            mass: get_body_mass(get_circle_volume(1.0), &Material::Flesh),
            radius: 1.0,
            restitution: get_material_restitution(&Material::Flesh),
            material: Material::Flesh,
        }
    );
    world.set_position(initial_player_index, Vec2f::ZERO);
    for material in &[Material::Flesh, Material::Stone] {
        for _ in 0..20 {
            let radius = rng.gen_range(0.8..1.2);
            let (_, index) = world.add_body(
                Body {
                    mass: get_body_mass(get_circle_volume(1.0), material),
                    radius,
                    restitution: get_material_restitution(material),
                    material: material.clone(),
                }
            );
            world.set_position(index, Vec2f::new(rng.gen_range(-100.0..100.0), rng.gen_range(-100.0..100.0)));
        }
    }
    let mut events = Events::new(EventSettings::new());
    let mut scale = window.size().height / (world.bodies()[0].radius * 20.0);
    let time_step = 1.0 / 60.0;
    let mut last_mouse_pos = Vec2f::ZERO;
    let mut last_viewport_shift = Vec2f::ZERO;
    let mut last_player_position = Vec2f::ZERO;
    let mut last_player_index = Some(initial_player_index);
    let mut pause = false;
    let texture_settings = TextureSettings::new().filter(Filter::Linear);
    let mut glyphs = GlyphCache::new("fonts/UbuntuMono-R.ttf", (), texture_settings)
        .expect("Could not load font");
    let mut fps = FpsMovingAverage::new(100, Duration::from_secs(1));
    let mut render_duration = DurationMovingAverage::new(100, Duration::from_secs(1));
    let mut update_duration = DurationMovingAverage::new(100, Duration::from_secs(1));

    while let Some(e) = events.next(&mut window) {
        if let Some(v) = e.press_args() {
            match v {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(player_index) = last_player_index {
                        world.set_const_force(player_index, ((last_mouse_pos - last_viewport_shift) / scale).norm());
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        world.start_directed_magick(player_index);
                    }
                }
                _ => (),
            }
        }

        if let Some(v) = e.release_args() {
            match v {
                Button::Mouse(MouseButton::Left) => {
                    if let Some(player_index) = last_player_index {
                        world.set_const_force(player_index, 0.0);
                    }
                }
                Button::Mouse(MouseButton::Right) => {
                    if let Some(player_index) = last_player_index {
                        world.complete_directed_magick(player_index);
                    }
                }
                Button::Mouse(MouseButton::Middle) => {
                    if let Some(player_index) = last_player_index {
                        world.self_magick(player_index);
                    }
                }
                Button::Keyboard(Key::Q) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Water);
                    }
                },
                Button::Keyboard(Key::A) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Lightning);
                    }
                },
                Button::Keyboard(Key::W) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Life);
                    }
                },
                Button::Keyboard(Key::S) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Arcane);
                    }
                },
                Button::Keyboard(Key::E) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Shield);
                    }
                },
                Button::Keyboard(Key::D) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Earth);
                    }
                },
                Button::Keyboard(Key::R) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Cold);
                    }
                },
                Button::Keyboard(Key::F) => {
                    if let Some(player_index) = last_player_index {
                        world.add_spell_element(player_index, Element::Fire);
                    }
                },
                Button::Keyboard(Key::P) => pause = !pause,
                _ => (),
            }
        }

        if let Some(scroll) = e.mouse_scroll_args() {
            scale *= 1.0 + scroll[1] * 0.1;
        }

        if let Some(args) = e.mouse_cursor_args() {
            last_mouse_pos = Vec2f::new(args[0], args[1]);
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

                    let current_target = world.get_position(player_index)
                        + world.get_current_direction(player_index) * world.get_body(player_index).radius * 2.0;
                    line_from_to(
                        [0.0, 0.0, 0.0, 0.5], 1.0 / scale,
                        [last_player_position.x, last_player_position.y],
                        [current_target.x, current_target.y],
                        base_transform,
                        g,
                    );
                }

                for (index, beam) in world.beams().iter().enumerate() {
                    let (begin, direction) = match beam.source {
                        Id::Body(body_id) => {
                            let body_index = world.get_index(body_id);
                            let direction = world.get_current_direction(body_index);
                            let begin = world.get_position(body_index) + direction * world.get_body(body_index).radius * SHIFT_FACTOR;
                            (begin, direction)
                        }
                        Id::Beam(..) => {
                            let temp_beam = world.get_temp_beam(index);
                            (temp_beam.begin, temp_beam.direction)
                        }
                    };
                    let end = begin + direction * world.get_beam_length(index);
                    let line = [begin.x, begin.y, end.x, end.y];
                    let color = get_magick_power_color(&beam.magick.power);
                    let sum_power = beam.magick.power.iter().sum::<f64>() / 20.0;
                    line::Line::new_round(color, sum_power).draw(line, &Default::default(), base_transform, g);
                }

                for (index, body) in world.bodies().iter().enumerate() {
                    let position = world.get_position(index);
                    let transform = base_transform.trans(position.x, position.y);
                    let square = rectangle::centered_square(0.0, 0.0, body.radius);
                    let border_radius = body.radius * 0.1;
                    let effect = world.get_effect(index);
                    ellipse::Ellipse::new(get_material_color(&body.material, if world.is_active(index) { 1.0 } else { 0.5 }))
                        .border(ellipse::Border { color: get_magick_power_color(&effect.power), radius: border_radius })
                        .draw(square, &ctx.draw_state, transform, g);
                }

                for (index, body) in world.bodies().iter().enumerate() {
                    let position = world.get_position(index);
                    let health_position = position + Vec2f::only_y(body.radius + 0.5);
                    let health_transform = base_transform.trans(health_position.x, health_position.y);
                    let half_width = body.radius * 0.66;
                    let bar_right = -half_width + 2.0 * half_width * world.get_health(index) / (body.mass * HEALTH_FACTOR);
                    rectangle::Rectangle::new([0.0, 0.0, 0.0, 0.8]).draw(
                        rectangle::rectangle_by_corners(-half_width, -body.radius * 0.1, half_width, body.radius * 0.1),
                        &ctx.draw_state,
                        health_transform,
                        g,
                    );
                    rectangle::Rectangle::new([1.0, 0.0, 0.0, 1.0]).draw(
                        rectangle::rectangle_by_corners(-half_width, -body.radius * 0.1, bar_right, body.radius * 0.1),
                        &ctx.draw_state,
                        health_transform,
                        g,
                    );
                    let aura = world.get_aura(index);
                    let square = rectangle::centered_square(0.0, 0.0, body.radius * aura.power);
                    ellipse::Ellipse::new(get_magick_power_color(&aura.elements))
                        .draw(square, &ctx.draw_state, base_transform.trans(position.x, position.y), g);
                    let spell_position = position + Vec2f::new(-half_width, body.radius + 1.0);
                    let spell_transform = base_transform.trans(spell_position.x, spell_position.y);
                    let square = rectangle::centered_square(0.0, 0.0, body.radius * 0.1);
                    let element_width = (2.0 * half_width) / 5.0;
                    for (i, element) in world.get_spell(index).elements().iter().enumerate() {
                        let element_position = Vec2f::new((i as f64 + 0.5) * element_width, -body.radius * 0.1);
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(Border { color: [0.0, 0.0, 0.0, 1.0], radius: body.radius * 0.01 })
                            .draw(square, &ctx.draw_state, spell_transform.trans(element_position.x, element_position.y), g);
                    }
                }

                let bounds = world.bounds();
                rectangle::Rectangle::new_border([1.0, 0.0, 0.0, 0.5], 1.0).draw(
                    rectangle::rectangle_by_corners(
                        bounds.min.x - 1.0,
                        bounds.min.y - 1.0,
                        bounds.max.x + 1.0,
                        bounds.max.y + 1.0,
                    ),
                    &ctx.draw_state,
                    base_transform,
                    g,
                );

                if let Some(player_index) = last_player_index {
                    let radius = 20.0;
                    let square = rectangle::centered_square(0.0, 0.0, radius);
                    for (i, element) in world.get_spell(player_index).elements().iter().enumerate() {
                        let position = last_viewport_shift + Vec2f::new(-5.0 * 2.0 * (radius + 10.0) * 0.5 + (i as f64 + 0.5) * 2.0 * (radius + 10.0), last_viewport_shift.y - 100.0);
                        ellipse::Ellipse::new(get_element_color(*element))
                            .border(Border { color: [0.0, 0.0, 0.0, 1.0], radius: radius * 0.1 })
                            .draw(square, &ctx.draw_state, ctx.transform.trans(position.x, position.y), g);
                    }
                }

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("FPS: {}", fps.get())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 20.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("Render: {}", render_duration.get())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 44.0), g)
                    .unwrap();

                text::Text::new_color([0.5, 0.5, 0.5, 1.0], 20)
                    .draw(&format!("Update: {}", update_duration.get())[..], &mut glyphs, &ctx.draw_state, ctx.transform.trans(10.0, 68.0), g)
                    .unwrap();
            });

            render_duration.add(Instant::now() - start);
        }

        if let Some(_) = e.update_args() {
            let start = Instant::now();
            if !pause {
                if let Some(player_index) = last_player_index {
                    let target_direction = (last_mouse_pos - last_viewport_shift) / scale;
                    let norm = target_direction.norm();
                    if norm <= f64::EPSILON {
                        world.set_target_direction(player_index, world.get_current_direction(player_index));
                    } else {
                        world.set_target_direction(player_index, target_direction / norm);
                    }
                    for i in 0..world.bodies().len() {
                        if i != player_index && matches!(world.get_body(i).material, Material::Flesh) {
                            world.add_spell_element(i, Element::Shield);
                            world.self_magick(i);
                        }
                    }
                }
                world.update(time_step);
                last_player_index = world.find_index(player_id);
                if let Some(player_index) = last_player_index {
                    last_player_position = world.get_position(player_index);
                }
            }
            update_duration.add(Instant::now() - start);
        }

        fps.add(Instant::now());
    }
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

struct FpsMovingAverage {
    max_frames: usize,
    max_interval: Duration,
    times: VecDeque<Instant>,
    sum_duration: Duration,
}

impl FpsMovingAverage {
    pub fn new(max_frames: usize, max_interval: Duration) -> Self {
        assert!(max_frames >= 3);
        Self {
            max_frames,
            max_interval,
            times: VecDeque::new(),
            sum_duration: Duration::new(0, 0),
        }
    }

    pub fn add(&mut self, time: Instant) {
        if self.times.len() >= self.max_frames
            || (self.times.len() >= 3 && self.sum_duration >= self.max_interval) {
            if let Some(removed) = self.times.pop_front() {
                if let Some(first) = self.times.front() {
                    self.sum_duration -= *first - removed;
                }
            }
        }
        if let Some(last) = self.times.back() {
            self.sum_duration += time - *last;
        }
        self.times.push_back(time);
    }

    pub fn get(&self) -> f64 {
        if self.times.len() >= 2 {
            (self.times.len() - 1) as f64 / self.sum_duration.as_secs_f64()
        } else {
            0.0
        }
    }
}

struct DurationMovingAverage {
    max_frames: usize,
    max_interval: Duration,
    durations: VecDeque<Duration>,
    sum_duration: Duration,
}

impl DurationMovingAverage {
    pub fn new(max_frames: usize, max_interval: Duration) -> Self {
        assert!(max_frames >= 2);
        Self {
            max_frames,
            max_interval,
            durations: VecDeque::new(),
            sum_duration: Duration::new(0, 0),
        }
    }

    pub fn add(&mut self, duration: Duration) {
        if self.durations.len() >= self.max_frames
            || (self.durations.len() >= 2 && self.sum_duration >= self.max_interval) {
            if let Some(removed) = self.durations.pop_front() {
                self.sum_duration -= removed;
            }
        }
        self.durations.push_back(duration);
        self.sum_duration += duration;
    }

    pub fn get(&self) -> f64 {
        if self.durations.len() >= 1 {
            self.sum_duration.as_secs_f64() / self.durations.len() as f64
        } else {
            0.0
        }
    }
}
