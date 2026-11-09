#!/usr/bin/env bash
# 在 macOS 上打包成 .app 和便携压缩包：dist/video-screenshot-macos.zip
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release

app="dist/视频截图.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "target/release/video-screenshot" "$app/Contents/MacOS/"

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>视频截图</string>
    <key>CFBundleDisplayName</key>
    <string>视频截图</string>
    <key>CFBundleIdentifier</key>
    <string>com.videoscreenshot.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>video-screenshot</string>
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
rm -f "video-screenshot-macos.zip"
zip -q -r "video-screenshot-macos.zip" "视频截图.app"
cd ..
echo "已生成 dist/视频截图.app 和 dist/video-screenshot-macos.zip"
