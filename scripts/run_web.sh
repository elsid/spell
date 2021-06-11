#!/bin/bash -ex

rustup target add wasm32-unknown-unknown
cargo install basic-http-server
cargo build --release --target wasm32-unknown-unknown --features=client
install target/wasm32-unknown-unknown/release/spell.wasm web/
install -d fonts/ web/fonts
install fonts/* web/fonts/
basic-http-server --addr 127.0.0.1:8080 web
