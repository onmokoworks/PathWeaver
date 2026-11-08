#!/usr/bin/env bash
# Build an ad-hoc signed native GPUI application for this Mac.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then echo 'This script requires macOS.' >&2; exit 1; fi
profile=release
build_flags=(--release)
frontend=pathweaver-gpui
bundle=PathWeaver
identifier=com.onmokoworks.pathweaver
app_dir="$PWD/dist"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) profile=debug; build_flags=(); shift ;;
        --legacy) frontend=threadtype-egui; bundle=PathWeaver-Legacy; identifier=com.onmokoworks.pathweaver.legacy; shift ;;
        --output-dir)
            if [[ $# -lt 2 || -z "$2" ]]; then echo '--output-dir requires a directory.' >&2; exit 2; fi
            app_dir="$2"; shift 2 ;;
        *) echo "Usage: $0 [--debug] [--legacy] [--output-dir directory]" >&2; exit 2 ;;
    esac
done
mkdir -p "$app_dir"
app_dir="$(cd "$app_dir" && pwd -P)"
if ! command -v cargo >/dev/null 2>&1; then echo 'Install Rust from https://rustup.rs first.' >&2; exit 1; fi
app="$app_dir/$bundle.app"
# Do not replace a binary while a user is editing in that application.
ensure_closed() {
    if ps -axo command= | grep -F -x "$app/Contents/MacOS/pathweaver" >/dev/null; then
        echo "Save your work and close $app before rebuilding it." >&2
        exit 1
    fi
}
ensure_closed
if [[ "$frontend" == pathweaver-gpui ]]; then ./scripts/build-tweeq-ui.sh; fi
export CARGO_TARGET_DIR="$PWD/$frontend/target"
cargo build --manifest-path "$frontend/Cargo.toml" --locked --bin pathweaver ${build_flags[@]+"${build_flags[@]}"}
ensure_closed
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
if [[ "$frontend" == pathweaver-gpui ]]; then
    mkdir -p "$app/Contents/Resources/Licenses"
    cp tweeq-ui/vendor/tweeq/LICENSE "$app/Contents/Resources/Licenses/Tweeq-MIT.txt"
    cp tweeq-ui/vendor/icon-licenses/* "$app/Contents/Resources/Licenses/"
    cp pathweaver-gpui/THIRD_PARTY_NOTICES.md "$app/Contents/Resources/Licenses/THIRD_PARTY_NOTICES.md"
fi
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
printf 'Built %s (%s, %s)\n' "$app" "$(uname -m)" "$profile"
