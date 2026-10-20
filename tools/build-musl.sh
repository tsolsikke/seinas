#!/bin/sh
# seinas、seinas-standalone、seinas-fbdev、seinas-taskbar、seinas-wallpaper、zeyes-minを、x86_64-unknown-linux-musl向けに静的にリンクして作る。
#
# 使い方:
#     tools/build-musl.sh
#
# できるもの:
#     target/x86_64-unknown-linux-musl/release/seinas、seinas-standalone、seinas-fbdev、seinas-taskbar、seinas-wallpaper、zeyes-min、test-client
#     target/dist/seinas-musl/(実行ファイルと、LICENSE、第三者のライセンス文、既定のフォントをまとめたもの)
#
# CPUの指定(target-cpu=nativeなど)は付けない。Rustの既定のx86-64(SSE2まで)で作る。
# Cのライブラリは、tools/build-musl-libs.sh が作った静的ライブラリをリンクする。
# ソースのパスは、--remap-path-prefix で、置き場所によらない名前(/seinas、/cargo)に置き換える。
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# 重い作業なので、ZeikOS の QEMU と重ならないように、ホストのロックの中で動かす(tools/zeikos-lock.sh)。
if [ "${SEINAS_HOLDS_QEMU_LOCK:-}" != 1 ]; then
    exec "$root/tools/zeikos-lock.sh" "$root/tools/build-musl.sh" "$@"
fi
libs="$root/target/musl-libs/lib"

if [ ! -f "$libs/libpixman-1.a" ] || [ ! -f "$libs/libxkbcommon.a" ]; then
    "$root/tools/build-musl-libs.sh"
fi

# ほかで設定されたRUSTFLAGSが混ざると、CPUの指定などが入り込むおそれがあるので外す。
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS

cd "$root"
# 実行ファイルに、作った環境のパス(リポジトリの置き場所、Cargoの置き場所)が入らないようにする。
# 入ると、置き場所が違うだけで別の実行ファイルになり、手元のパスが配布物に残る。
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-L native=$libs --remap-path-prefix=$root=/seinas --remap-path-prefix=$cargo_home=/cargo"
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas -p zeyes-min -p seinas-test-clients
# 構成ごとに、別に作る。一緒に作ると、Cargoが機能を1つにまとめて、その構成に要らないもの
# (fbdevだけの構成にWaylandのフロントエンド、など)までビルドに入ってしまう。
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-standalone
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-fbdev
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-taskbar
cargo build --release --locked --target x86_64-unknown-linux-musl -p seinas-wallpaper

out=target/x86_64-unknown-linux-musl/release

# 静的にリンクできているかを確かめる。
tools/elf-report.sh "$out/seinas" "$out/seinas-standalone" "$out/seinas-fbdev" "$out/seinas-taskbar" "$out/seinas-wallpaper" "$out/zeyes-min" > /dev/null

# seinas-standaloneに、入れ子のバックエンド(SCTK、wayland-client)が入っていないかを確かめる。
tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client calloop-wayland-source

# seinas-fbdevに、Waylandのフロントエンドが入っていないかを確かめる。
tools/check-no-wayland.sh seinas-fbdev "$out/seinas-fbdev"

# seinas-taskbarは、コンポジタとは別のクライアントなので、Waylandのサーバーの側(Smithay)が入っていないことを確かめる。
tools/check-deps.sh seinas-taskbar smithay wayland-server pixman

# seinas-wallpaperも、別のクライアント。Smithayと、文字を描くクレート(使わない)が入っていないことを確かめる。
tools/check-deps.sh seinas-wallpaper smithay wayland-server pixman seinas-text cosmic-text

# 既定のフォントを取得する(置いてあり、SHA-256 が合えば、何もしない)。
# 取得できないとき(ネットワークが無い、SHA-256 が合わない)は、フォントの無い配布物を作らずに止まる。
# 前に作った配布物が残って、新しいものと取り違えないよう、先に消しておく。
dist=target/dist/seinas-musl
rm -rf "$dist"
if ! tools/fetch-fonts.sh > /dev/null; then
    echo "build-musl.sh: 既定のフォントを取得できなかったので、配布物を作らずに止めます(理由は上のとおり)" >&2
    exit 1
fi

# 配布物をまとめる。静的にリンクしたライブラリのライセンス文と、既定のフォントを、実行ファイルと一緒に置く。
# フォントのライセンス文と著作権表示は、THIRD-PARTY/fonts/ に入っている。
mkdir -p "$dist/fonts"
cp "$out/seinas" "$out/seinas-standalone" "$out/seinas-fbdev" "$out/seinas-taskbar" "$out/seinas-wallpaper" "$out/zeyes-min" LICENSE "$dist/"
cp -R THIRD-PARTY "$dist/THIRD-PARTY"
cp target/fonts/*.ttf target/fonts/*.otf "$dist/fonts/"

ls -l "$dist"
