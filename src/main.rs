#[cfg(feature = "desktop")]
use clap::Clap;

use spell::game::{run_game, GameSettings};

#[cfg(feature = "desktop")]
#[derive(Clap)]
struct Args {
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

#[macroquad::main(window_conf)]
async fn main() {
    #[cfg(feature = "desktop")]
    env_logger::init();
    #[cfg(feature = "desktop")]
    let settings = {
        let args = Args::parse();
        GameSettings {
            random_seed: args.random_seed,
            default_server_address: args.default_server_address,
            default_server_port: args.default_server_port,
            default_player_name: args.default_player_name,
            connect_timeout: args.connect_timeout,
            read_timeout: args.read_timeout,
            retry_period: args.retry_period,
            max_world_frame_delay: args.max_world_frame_delay,
            world_updates_delay: args.world_updates_delay,
        }
    };
    #[cfg(not(feature = "desktop"))]
    let settings = GameSettings::default();
    run_game(settings).await;
}

fn window_conf() -> macroquad::prelude::Conf {
    macroquad::prelude::Conf {
        window_title: String::from("Spell"),
        high_dpi: true,
        sample_count: 2,
        ..Default::default()
    }
}
