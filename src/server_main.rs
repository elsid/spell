#[macro_use]
extern crate log;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Clap;

use spell::{run_server, ServerParams};

fn main() {
    env_logger::init();
    let params = ServerParams::parse();
    let stop_server = Arc::new(AtomicBool::new(false));
    setup_ctrlc_handler(stop_server.clone());
    run_server(params, stop_server)
}

fn setup_ctrlc_handler(stop: Arc<AtomicBool>) {
    ctrlc::set_handler(move || {
        info!("Stopping server...");
        stop.store(true, Ordering::Release)
    })
    .unwrap();
}
