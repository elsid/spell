#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Clap;

#[cfg(feature = "render")]
use spell::{run_multi_player, run_single_player, MultiPlayerParams, SinglePlayerParams};
use spell::{run_server, ServerParams};

#[derive(Clap)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Clap)]
enum Command {
    #[cfg(feature = "render")]
    SinglePlayer(SinglePlayerParams),
    #[cfg(feature = "render")]
    MultiPlayer(MultiPlayerParams),
    Server(ServerParams),
}

fn main() {
    env_logger::init();
    match Args::parse().command {
        #[cfg(feature = "render")]
        Command::SinglePlayer(params) => run_single_player(params),
        #[cfg(feature = "render")]
        Command::MultiPlayer(params) => run_multi_player(params),
        Command::Server(params) => {
            let stop_server = Arc::new(AtomicBool::new(false));
            {
                let stop = stop_server.clone();
                ctrlc::set_handler(move || {
                    info!("Stopping server...");
                    stop.store(true, Ordering::Release)
                })
                .unwrap();
            }
            run_server(params, stop_server)
        }
    }
}
