#!/usr/bin/env bash
# Build an ad-hoc signed native GPUI application for this Mac.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then echo 'This script requires macOS.' >&2; exit 1; fi
profile=release
build_flags=(--release)
frontend=pathweaver-gpui
bundle=PathWeaver-GPUI
identifier=com.onmokoworks.pathweaver.gpui
for arg in "$@"; do
    case "$arg" in
        --debug) profile=debug; build_flags=() ;;
        --legacy) frontend=threadtype-egui; bundle=PathWeaver; identifier=com.onmokoworks.pathweaver ;;
        *) echo "Usage: $0 [--debug] [--legacy]" >&2; exit 2 ;;
    esac
done
if ! command -v cargo >/dev/null 2>&1; then echo 'Install Rust from https://rustup.rs first.' >&2; exit 1; fi
app="$PWD/dist/$bundle.app"
# Do not replace a binary while a user is editing in that application.
if ps -axo command= | grep -F -x "$app/Contents/MacOS/pathweaver" >/dev/null; then
    echo "Save your work and close $app before rebuilding it." >&2
    exit 1
fi
export CARGO_TARGET_DIR="$PWD/$frontend/target"
cargo build --manifest-path "$frontend/Cargo.toml" --locked --bin pathweaver ${build_flags[@]+"${build_flags[@]}"}
mkdir -p "$app/Contents/MacOS"
cp "$CARGO_TARGET_DIR/$profile/pathweaver" "$app/Contents/MacOS/pathweaver"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleName</key><string>$bundle</string>
    <key>CFBundleDisplayName</key><string>$bundle</string>
    <key>CFBundleIdentifier</key><string>$identifier</string>
    <key>CFBundleExecutable</key><string>pathweaver</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.2.0</string>
    <key>CFBundleVersion</key><string>2</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
printf 'Built %s (%s, %s)\n' "$app" "$(uname -m)" "$profile"
