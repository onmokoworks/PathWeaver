#!/usr/bin/env bash
# macOS / Linux: ./run-pathweaver.sh [--release|--test|--check]
set -euo pipefail
cd "$(dirname "$0")/threadtype-egui"
if ! command -v cargo >/dev/null 2>&1; then
    echo 'Rust (cargo) is required. Install from https://rustup.rs and reopen Terminal.' >&2
    exit 1
fi
case "${1:-}" in
    '') exec cargo run --locked --bin pathweaver ;;
    --release) exec cargo run --locked --release --bin pathweaver ;;
    --test) exec cargo test --locked --bin pathweaver ;;
    --check) exec cargo clippy --locked --bin pathweaver -- -D warnings ;;
    *) echo "Usage: $0 [--release|--test|--check]" >&2; exit 2 ;;
esac
