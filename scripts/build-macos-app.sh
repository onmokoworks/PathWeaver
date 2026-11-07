#!/usr/bin/env bash
# Build a local, ad-hoc signed app for this Mac's architecture.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
    echo 'This script requires macOS.' >&2
    exit 1
fi
profile=release
case "${1:-}" in
    '') build_flags=(--release) ;;
    --debug) profile=debug; build_flags=() ;;
    *) echo "Usage: $0 [--debug]" >&2; exit 2 ;;
esac
if ! command -v cargo >/dev/null 2>&1; then
    echo 'Rust (cargo) is required. Install from https://rustup.rs and reopen Terminal.' >&2
    exit 1
fi
# Use an explicit target directory, independent of the caller's Cargo config.
export CARGO_TARGET_DIR="$PWD/threadtype-egui/target"
cargo build --manifest-path threadtype-egui/Cargo.toml --locked --bin pathweaver ${build_flags[@]+"${build_flags[@]}"}
app="$PWD/dist/PathWeaver.app"
mkdir -p "$app/Contents/MacOS"
cp "$CARGO_TARGET_DIR/$profile/pathweaver" "$app/Contents/MacOS/pathweaver"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleName</key><string>PathWeaver</string>
    <key>CFBundleDisplayName</key><string>PathWeaver</string>
    <key>CFBundleIdentifier</key><string>com.onmokoworks.pathweaver</string>
    <key>CFBundleExecutable</key><string>pathweaver</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
printf 'Built %s (%s, %s)\n' "$app" "$(uname -m)" "$profile"
printf 'Open with: open "%s"\n' "$app"
