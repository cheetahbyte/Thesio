#!/bin/zsh
# Replace the running hand-test build (dist-preview) with target/release. It
# runs as dist-preview/Thesio.app (so the menu bar says Thesio) on the
# saved preview library, or ~/Documents/Thesio.
# Only this checkout's own binaries are stopped:
# pkill matches this checkout's absolute path. Log: /tmp/thesio-preview.log
set -e
cd "$(dirname "$0")/.."
preview_library="${THESIO_HOME:-$(cat dist-preview/library-path 2>/dev/null || printf '%s' "$HOME/Documents/Thesio")}"
# An empty path would put the library in the current directory.
[ -n "$preview_library" ] || { echo "preview.sh: no preview library" >&2; exit 1; }
export THESIO_HOME="$preview_library"
app="$PWD/dist-preview/Thesio.app"
bin="$app/Contents/MacOS/thesio"
pkill -f "^$bin" || true
sleep 1
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp assets/branding/app-icon.png "$app/Contents/Resources/app-icon.png"
icon_tmp=$(mktemp -d)
iconset="$icon_tmp/app-icon.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/branding/app-icon.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) assets/branding/app-icon.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/app-icon.icns"
rm -rf "$icon_tmp"
cp target/release/thesio "$bin.bin"
# Finder launches must use the same library as this script.
printf '#!/bin/zsh\nexport THESIO_HOME=%q\nexec "${0:A:h}/thesio.bin" "$@"\n' "$preview_library" > "$bin"
chmod +x "$bin"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Thesio</string>
  <key>CFBundleDisplayName</key><string>Thesio</string>
  <key>CFBundleIdentifier</key><string>local.thesio.preview</string>
  <key>CFBundleExecutable</key><string>thesio</string>
  <key>CFBundleIconFile</key><string>app-icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSDocumentsFolderUsageDescription</key><string>Thesio stores its document library in the Thesio folder in Documents.</string>
</dict></plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1 || true
(THESIO_HOME="$THESIO_HOME" nohup "$bin" >/tmp/thesio-preview.log 2>&1 &)
sleep 2; pgrep -fl "^$bin"
