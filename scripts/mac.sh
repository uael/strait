#!/bin/zsh
# Builds strait.app and installs it in /Applications.
#
#   scripts/mac.sh build    build target/strait.app
#   scripts/mac.sh install  build, then replace /Applications/strait.app and launch it
#
# STRAIT_SIGN names the signing identity (default: "strait local signing"
# when the keychain has it, else ad hoc). macOS ties Accessibility and Input
# Monitoring grants to the signature, so an ad hoc signature loses them on
# every rebuild.
set -euo pipefail

root=${0:A:h:h}
app=$root/target/strait.app
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)

build() {
  cargo build --release --manifest-path "$root/Cargo.toml" -p strait-mac
  swift build -c release --package-path "$root/mac"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$root/mac/.build/release/Strait" "$app/Contents/MacOS/strait"
  local iconset=$root/target/AppIcon.iconset
  rm -rf "$iconset"
  swift "$root/scripts/icon.swift" "$iconset"
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
  cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.uael.strait</string>
  <key>CFBundleName</key><string>strait</string>
  <key>CFBundleDisplayName</key><string>strait</string>
  <key>CFBundleExecutable</key><string>strait</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>strait finds your Windows PC and sends it your keyboard and mouse over the local network.</string>
  <key>NSBonjourServices</key><array><string>_strait._tcp</string></array>
  <key>NSHumanReadableCopyright</key><string>Shares this Mac's keyboard, mouse and clipboard with a Windows PC.</string>
</dict>
</plist>
EOF
  local identity=${STRAIT_SIGN:-}
  if [[ -z $identity ]]; then
    if security find-identity -p codesigning | grep -q '"strait local signing"'; then
      identity="strait local signing"
    else
      identity=-
    fi
  fi
  codesign --force --options runtime --sign "$identity" "$app"
  echo "built $app (signed by $identity)"
}

case ${1:-} in
  build) build ;;
  install)
    build
    osascript -e 'quit app "strait"' 2>/dev/null || true
    pkill -x strait 2>/dev/null || true
    rm -rf /Applications/strait.app
    cp -R "$app" /Applications/strait.app
    open /Applications/strait.app
    ;;
  *) echo "usage: $0 build|install" >&2; exit 2 ;;
esac
