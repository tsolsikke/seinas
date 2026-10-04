#!/bin/sh
# seinas、zeyes-min、seinas-fbdevを、x86_64-unknown-linux-musl向けに静的にリンクして作る。
#
# 使い方:
#     tools/build-musl.sh
#
# できるもの:
#     target/x86_64-unknown-linux-musl/release/seinas、zeyes-min、seinas-fbdev
#     target/dist/seinas-musl/(実行ファイルと、LICENSE、第三者のライセンス文をまとめたもの)
#
# CPUの指定(target-cpu=nativeなど)は付けない。Rustの既定のx86-64(SSE2まで)で作る。
# Cのライブラリは、tools/build-musl-libs.sh が作った静的ライブラリをリンクする。
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
libs="$root/target/musl-libs/lib"

if [ ! -f "$libs/libpixman-1.a" ] || [ ! -f "$libs/libxkbcommon.a" ]; then
    "$root/tools/build-musl-libs.sh"
fi

# ほかで設定されたRUSTFLAGSが混ざると、CPUの指定などが入り込むおそれがあるので外す。
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS

cd "$root"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-L native=$libs"
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas -p zeyes-min
# seinas-fbdevは、別に作る。一緒に作ると、Cargoが機能を1つにまとめて、Waylandの受け口まで
# ビルドに入ってしまう。
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-fbdev

out=target/x86_64-unknown-linux-musl/release

# 静的にリンクできているかを確かめる。
tools/elf-report.sh "$out/seinas" "$out/zeyes-min" "$out/seinas-fbdev" > /dev/null

# seinas-fbdevに、Waylandの受け口が入っていないかを確かめる。
tools/check-no-wayland.sh seinas-fbdev "$out/seinas-fbdev"

# 配布物をまとめる。静的にリンクしたライブラリのライセンス文を、実行ファイルと一緒に置く。
dist=target/dist/seinas-musl
rm -rf "$dist"
mkdir -p "$dist"
cp "$out/seinas" "$out/zeyes-min" "$out/seinas-fbdev" LICENSE "$dist/"
cp -R THIRD-PARTY "$dist/THIRD-PARTY"

ls -l "$dist"
