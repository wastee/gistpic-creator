#!/usr/bin/env bash
# 生成 Arch Linux 可安装包。
#
# 做三件事：
# 1. cargo build --release（已经构建过可以加 --skip-build 跳过）
# 2. 把可执行文件、桌面文件、图标和文档放进 dist/archlinux/，按版本号填好 PKGBUILD
# 3. 机器上有 makepkg 就直接打包成 .pkg.tar.zst；打完用 pacman -U 安装
#
# 用法：
#   ./scripts/build-arch.sh                 # 构建并打包
#   ./scripts/build-arch.sh --skip-build    # 用现成的 target/release 重新打包
#   PKGREL=2 ./scripts/build-arch.sh        # 改包版本号里的 pkgrel（小版本修订）
set -euo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)

skip_build=0
for argument in "$@"; do
    case "$argument" in
        --skip-build)
            skip_build=1
            ;;
        -h|--help)
            sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "不认识的参数：$argument" >&2
            echo "支持的参数：--skip-build、--help" >&2
            exit 2
            ;;
    esac
done

pkgver=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [[ -z "$pkgver" ]]; then
    echo "没能从 Cargo.toml 读到版本号" >&2
    exit 1
fi
pkgrel=${PKGREL:-1}
pkgname=gistpic-creator-bin
out="$root/dist/archlinux"
binary="target/release/gistpic-creator"

for file in packaging/linux/gistpic-creator.desktop "assets/icon-256.png" README.md THIRD_PARTY_NOTICES.md; do
    if [[ ! -f "$file" ]]; then
        echo "缺少打包要用的文件：$file" >&2
        exit 1
    fi
done

if [[ "$skip_build" -eq 0 ]]; then
    cargo build --release --locked
fi
if [[ ! -x "$binary" ]]; then
    echo "找不到 $binary，先跑 cargo build --release，或去掉 --skip-build" >&2
    exit 1
fi

rm -rf "$out"
mkdir -p "$out"

# makepkg 的 source 是相对 PKGBUILD 所在目录找的，所以这些都放在它旁边。
cp "$binary" "$out/gistpic-creator"
cp packaging/linux/gistpic-creator.desktop "$out/gistpic-creator.desktop"
cp assets/icon-256.png "$out/gistpic-creator.png"
cp README.md THIRD_PARTY_NOTICES.md "$out/"
sed -e "s/@PKGVER@/$pkgver/" -e "s/@PKGREL@/$pkgrel/" \
    packaging/arch/PKGBUILD.in > "$out/PKGBUILD"

echo "已生成 $out/PKGBUILD（$pkgname $pkgver-$pkgrel，x86_64）"

if ! command -v makepkg >/dev/null 2>&1; then
    echo "这台机器没有 makepkg，只生成了 PKGBUILD。"
    echo "到 Arch Linux 上进入 $out 运行 makepkg -si，或把整个目录复制过去再打包。"
    exit 0
fi

# --nodeps 表示打包阶段不检查依赖：依赖是安装之后运行才需要的，等 pacman 装包时再校验。
# -f 覆盖上一次的同名包，脚本可以反复跑。
(cd "$out" && makepkg --force --nodeps --noconfirm --clean)

package_file=$(find "$out" -maxdepth 1 -name "$pkgname-$pkgver-$pkgrel-*.pkg.tar.*" -print -quit)
if [[ -z "$package_file" ]]; then
    echo "makepkg 没有产出安装包" >&2
    exit 1
fi

echo
echo "已生成安装包：$package_file"
echo "安装：sudo pacman -U \"$package_file\""
