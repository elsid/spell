#[cfg(any(all(feature = "client", feature = "desktop"), feature = "server"))]
#[macro_use]
extern crate log;

#[cfg(all(feature = "client", feature = "desktop"))]
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
