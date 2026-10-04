#!/bin/sh
# seinas、seinas-standalone、seinas-fbdev、zeyes-minを、x86_64-unknown-linux-musl向けに静的にリンクして作る。
#
# 使い方:
#     tools/build-musl.sh
#
# できるもの:
#     target/x86_64-unknown-linux-musl/release/seinas、seinas-standalone、seinas-fbdev、zeyes-min、bad-client
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
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas -p zeyes-min -p seinas-test-clients
# 構成ごとに、別に作る。一緒に作ると、Cargoが機能を1つにまとめて、その構成に要らないもの
# (fbdevだけの構成にWaylandの受け口、など)までビルドに入ってしまう。
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-standalone
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-fbdev

out=target/x86_64-unknown-linux-musl/release

# 静的にリンクできているかを確かめる。
tools/elf-report.sh "$out/seinas" "$out/seinas-standalone" "$out/seinas-fbdev" "$out/zeyes-min" > /dev/null

# seinas-standaloneに、入れ子の裏側(SCTK、wayland-client)が入っていないかを確かめる。
tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client calloop-wayland-source

# seinas-fbdevに、Waylandの受け口が入っていないかを確かめる。
tools/check-no-wayland.sh seinas-fbdev "$out/seinas-fbdev"

# 配布物をまとめる。静的にリンクしたライブラリのライセンス文を、実行ファイルと一緒に置く。
dist=target/dist/seinas-musl
rm -rf "$dist"
mkdir -p "$dist"
cp "$out/seinas" "$out/seinas-standalone" "$out/seinas-fbdev" "$out/zeyes-min" LICENSE "$dist/"
cp -R THIRD-PARTY "$dist/THIRD-PARTY"

ls -l "$dist"
