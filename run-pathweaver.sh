#!/usr/bin/env bash
# PathWeaver: Vue + Tweeq front end, pathweaver-core (Rust → wasm), Swift host.
set -euo pipefail
cd "$(dirname "$0")"
need() { command -v "$1" >/dev/null 2>&1 || { echo "$1 is required. $2" >&2; exit 1; }; }
need cargo 'Install Rust from https://rustup.rs, then: rustup target add wasm32-unknown-unknown'
need npm 'Install Node.js 20 or later.'
web() { (cd pathweaver-web && { [[ -d node_modules ]] || npm ci --no-audit --no-fund; } && "$@"); }
case "${1:-}" in
    ''|--app) ./scripts/build-macos-app.sh && open dist/PathWeaver.app ;;
    --dev) web npm run dev ;;                      # browser at http://127.0.0.1:5174
    --test)
        cargo test --manifest-path pathweaver-core/Cargo.toml --locked
        cargo test --manifest-path pathweaver-wasm/Cargo.toml
        web npm run prepare-assets
        web npx vitest run src
        web npx playwright test                     # WebKit, the WKWebView engine
        ;;
    --legacy) exec cargo run --manifest-path threadtype-egui/Cargo.toml --locked --bin pathweaver ;;
    *) echo "Usage: $0 [--app|--dev|--test|--legacy]" >&2; exit 2 ;;
esac
