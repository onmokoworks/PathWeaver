#!/usr/bin/env bash
# Build pathweaver-core as wasm32 and place it where the web UI imports it.
set -euo pipefail
cd "$(dirname "$0")/../../pathweaver-wasm"
command -v cargo >/dev/null || { echo "build-wasm: cargo not found (install Rust and: rustup target add wasm32-unknown-unknown)" >&2; exit 1; }
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/pathweaver_wasm.wasm ../pathweaver-web/src/core/pathweaver.wasm
