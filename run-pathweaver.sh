#!/usr/bin/env bash
# macOS defaults to GPUI; the egui frontend remains available for compatibility.
set -euo pipefail
cd "$(dirname "$0")"
if ! command -v cargo >/dev/null 2>&1; then
    echo 'Rust (cargo) is required. Install from https://rustup.rs and reopen Terminal.' >&2
    exit 1
fi
manifest=pathweaver-gpui/Cargo.toml
if [[ "$(uname -s)" != Darwin ]]; then manifest=threadtype-egui/Cargo.toml; fi
# The generated HTML is checked in so cargo also works without Node. Rebuild
# source edits for interactive runs; tests/checks use the committed artifact.
if [[ "$manifest" == pathweaver-gpui/Cargo.toml && ( "${1:-}" == "" || "${1:-}" == --release ) ]]; then
    ./scripts/build-tweeq-ui.sh
fi
case "${1:-}" in
    '') exec cargo run --manifest-path "$manifest" --locked --bin pathweaver ;;
    --release) exec cargo run --manifest-path "$manifest" --locked --release --bin pathweaver ;;
    --legacy) exec cargo run --manifest-path threadtype-egui/Cargo.toml --locked --bin pathweaver ;;
    --test)
        cargo test --manifest-path pathweaver-core/Cargo.toml --locked
        cargo test --manifest-path "$manifest" --locked --bin pathweaver
        ;;
    --check)
        cargo clippy --manifest-path pathweaver-core/Cargo.toml --locked --all-targets -- -D warnings
        cargo clippy --manifest-path "$manifest" --locked --all-targets -- -D warnings
        ;;
    *) echo "Usage: $0 [--release|--test|--check|--legacy]" >&2; exit 2 ;;
esac
