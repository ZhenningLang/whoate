#!/usr/bin/env bash
# Build dist/whoate.app: release Rust static library, release SwiftPM app,
# Info.plist, icons, ad-hoc signature (not notarized, U-DEC-1).
#   scripts/bundle-app.sh          build the bundle from committed icons
#   scripts/bundle-app.sh icons    regenerate assets/logo/{AppIcon.icns,menubar-template.pdf}
set -euo pipefail
exec </dev/null
root=$(cd "$(dirname "$0")/.." && pwd)
logo="$root/assets/logo"

icons() {
    command -v rsvg-convert >/dev/null || { echo "rsvg-convert required (brew install librsvg)" >&2; exit 1; }
    local set
    set=$(mktemp -d)/AppIcon.iconset
    mkdir -p "$set"
    for size in 16 32 128 256 512; do
        rsvg-convert -w "$size" -h "$size" "$logo/app-icon.svg" -o "$set/icon_${size}x${size}.png"
        rsvg-convert -w $((size * 2)) -h $((size * 2)) "$logo/app-icon.svg" -o "$set/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns "$set" -o "$logo/AppIcon.icns"
    rsvg-convert -f pdf -w 36 -h 27 "$logo/logo-template.svg" -o "$logo/menubar-template.pdf"
    rm -rf "$(dirname "$set")"
}

if [[ "${1:-}" == icons ]]; then
    icons
    exit 0
fi

# Match the app's minimum macOS; otherwise bundled C objects (SQLite) target the build host.
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build -p whoate-ffi --release --locked --manifest-path "$root/Cargo.toml"
WHOATE_FFI_PROFILE=release swift build -c release --package-path "$root/app" --product Whoate
binary="$(WHOATE_FFI_PROFILE=release swift build -c release --package-path "$root/app" --show-bin-path)/Whoate"

# The debug Rust library would leak the fixture entry point into a release app.
# Read nm once: under pipefail, `nm | grep -q` can SIGPIPE nm and skip this guard.
symbols=$(nm "$binary")
if grep -q '_whoate_engine_new_from_fixture' <<<"$symbols"; then
    echo "release binary links the debug Rust library" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="$root/dist/whoate.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Whoate"
cp "$logo/AppIcon.icns" "$logo/menubar-template.pdf" "$app/Contents/Resources/"
cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>io.github.zhenninglang.whoate</string>
    <key>CFBundleName</key><string>whoate</string>
    <key>CFBundleDisplayName</key><string>whoate</string>
    <key>CFBundleExecutable</key><string>Whoate</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${version}</string>
    <key>CFBundleVersion</key><string>${version}</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist" >/dev/null
codesign --force --sign - "$app"
codesign --verify --strict "$app"
echo "BUNDLE_OK $app"
