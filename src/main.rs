use clap::Clap;

use spell::{run_multi_player, run_single_player, MultiPlayerParams, SinglePlayerParams};

#[derive(Clap)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Clap)]
enum Command {
    SinglePlayer(SinglePlayerParams),
    MultiPlayer(MultiPlayerParams),
}

fn main() {
    env_logger::init();
    match Args::parse().command {
        Command::SinglePlayer(params) => run_single_player(params),
        Command::MultiPlayer(params) => run_multi_player(params),
    }
}
