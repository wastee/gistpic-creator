#!/usr/bin/env bash
# 在 macOS 上打包成 .app 和便携压缩包：dist/gistpic-creator-macos.zip
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release

app="dist/省流图创造机.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "target/release/gistpic-creator" "$app/Contents/MacOS/"

# 应用图标：从 assets/icon.png 生成 AppIcon.icns。sips 和 iconutil 是 macOS 自带的，
# 不需要额外装工具。iconset 要求的尺寸和命名是固定的，少一张 iconutil 会报错。
iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "assets/icon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" "assets/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>省流图创造机</string>
    <key>CFBundleDisplayName</key>
    <string>省流图创造机</string>
    <key>CFBundleIdentifier</key>
    <string>com.gistpic.creator</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>gistpic-creator</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

# 本机自签名，避免“应用已损坏”的提示；分发给别人时对方仍需在“系统设置 → 隐私与安全性”里允许打开。
codesign --force --deep --sign - "$app" 2>/dev/null || true

cd dist
rm -f "gistpic-creator-macos.zip"
zip -q -r "gistpic-creator-macos.zip" "省流图创造机.app"
cd ..
echo "已生成 dist/省流图创造机.app 和 dist/gistpic-creator-macos.zip"
