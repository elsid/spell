use clap::Clap;

use spell::game::{run_game, GameSettings};

#[macroquad::main(window_conf)]
async fn main() {
    env_logger::init();
    run_game(GameSettings::parse()).await;
}

fn window_conf() -> macroquad::prelude::Conf {
    macroquad::prelude::Conf {
        window_title: String::from("Spell"),
        high_dpi: true,
        sample_count: 2,
        ..Default::default()
    }
}
