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

tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "已生成 dist/$name.tar.gz"
