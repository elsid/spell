#[cfg(any(feature = "client", feature = "server"))]
#[macro_use]
extern crate log;

#[cfg(feature = "client")]
pub mod client;
#[cfg(any(feature = "client", feature = "server"))]
mod control;
#[cfg(any(feature = "client", feature = "server"))]
mod engine;
#[cfg(feature = "client")]
pub mod game;
#[cfg(any(feature = "client", feature = "server"))]
mod generators;
#[cfg(any(feature = "client", feature = "server"))]
mod meters;
#[cfg(any(feature = "client", feature = "server"))]
pub mod protocol;
#[cfg(any(feature = "client", feature = "server"))]
mod rect;
#[cfg(feature = "server")]
pub mod server;
#[cfg(any(feature = "client", feature = "server"))]
pub mod vec2;
#[cfg(any(feature = "client", feature = "server"))]
pub mod world;
