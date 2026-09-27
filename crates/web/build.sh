#!/bin/sh
# Build e for the browser into dev/pkg: the wasm module and its bindings.
# Needs the wasm32-unknown-unknown target (rust-toolchain.toml installs it)
# and wasm-bindgen-cli at the crate's version:
#   cargo install wasm-bindgen-cli --version 0.2.128 --locked
set -eu
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir dev/pkg target/wasm32-unknown-unknown/release/e_web.wasm
