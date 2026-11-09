#!/usr/bin/env bash
# Build PathWeaver.app: a Swift WKWebView host around the Vue + Tweeq UI,
# with pathweaver-core compiled to wasm and inlined into one index.html.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then echo 'This script requires macOS.' >&2; exit 1; fi
bundle=PathWeaver
identifier=com.onmokoworks.pathweaver
app_dir="$PWD/dist"
legacy=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) shift ;; # kept for compatibility; the web build is always optimized
        --legacy) legacy=1; shift ;;
        --output-dir)
            if [[ $# -lt 2 || -z "$2" ]]; then echo '--output-dir requires a directory.' >&2; exit 2; fi
            app_dir="$2"; shift 2 ;;
        *) echo "Usage: $0 [--legacy] [--output-dir directory]" >&2; exit 2 ;;
    esac
done
mkdir -p "$app_dir"
app_dir="$(cd "$app_dir" && pwd -P)"

if [[ $legacy == 1 ]]; then
    # egui compatibility build, unchanged.
    export CARGO_TARGET_DIR="$PWD/threadtype-egui/target"
    cargo build --manifest-path threadtype-egui/Cargo.toml --locked --release --bin pathweaver
    app="$app_dir/PathWeaver-Legacy.app"
    mkdir -p "$app/Contents/MacOS"
    cp "$CARGO_TARGET_DIR/release/pathweaver" "$app/Contents/MacOS/pathweaver"
    exec_name=pathweaver; identifier=com.onmokoworks.pathweaver.legacy; bundle=PathWeaver-Legacy
else
    app="$app_dir/$bundle.app"
    if pgrep -f "$app/Contents/MacOS/" >/dev/null; then
        echo "Save your work and close $app before rebuilding it." >&2
        exit 1
    fi
    (cd pathweaver-web && { [[ -d node_modules ]] || npm ci --no-audit --no-fund; } && npm run build)
    work="$(mktemp -d)"
    swiftc -O -o "$work/PathWeaver" pathweaver-mac/main.swift -framework AppKit -framework WebKit
    rm -rf "$app/Contents"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/web" "$app/Contents/Resources/Licenses"
    cp "$work/PathWeaver" "$app/Contents/MacOS/PathWeaver"
    cp pathweaver-web/dist/index.html "$app/Contents/Resources/web/index.html"
    cp pathweaver-web/vendor/tweeq/LICENSE "$app/Contents/Resources/Licenses/Tweeq-MIT.txt"
    cp pathweaver-web/vendor/icon-licenses/* "$app/Contents/Resources/Licenses/"
    rm -rf "$work"
    exec_name=PathWeaver
fi
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleName</key><string>$bundle</string>
    <key>CFBundleDisplayName</key><string>$bundle</string>
    <key>CFBundleIdentifier</key><string>$identifier</string>
    <key>CFBundleExecutable</key><string>$exec_name</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.3.0</string>
    <key>CFBundleVersion</key><string>3</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key><array><dict>
        <key>CFBundleTypeName</key><string>PathWeaver Document</string>
        <key>CFBundleTypeRole</key><string>Editor</string>
        <key>CFBundleTypeExtensions</key><array><string>pathweaver</string></array>
    </dict></array>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
printf 'Built %s (%s)\n' "$app" "$(uname -m)"
