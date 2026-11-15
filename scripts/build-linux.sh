#!/usr/bin/env bash
# 在 Linux 上打包成便携压缩包：dist/gistpic-creator-linux-x86_64.tar.gz
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release

name="gistpic-creator-linux-x86_64"
dest="dist/$name"

rm -rf "$dest"
mkdir -p "$dest"

cp "target/release/gistpic-creator" "$dest/"
cp "README.md" "$dest/"
cp "THIRD_PARTY_NOTICES.md" "$dest/"

# 窗口和任务栏的图标由桌面环境按 app_id 去匹配 .desktop 文件里的 Icon，
# 程序自己设不了。把图标和 .desktop 一起打进包，用户按 README 里的步骤
# 复制到 ~/.local 下就会生效。桌面文件和 Arch 包共用仓库里那一份，不各写一份。
cp "assets/icon-256.png" "$dest/gistpic-creator.png"
cp "packaging/linux/gistpic-creator.desktop" "$dest/gistpic-creator.desktop"

tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "已生成 dist/$name.tar.gz"
