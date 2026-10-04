#!/bin/sh
# musl向けの静的ライブラリ(libpixman-1.a と libxkbcommon.a)を、版を固定してソースから作る。
#
# 使い方:
#     tools/build-musl-libs.sh
#
# 要るもの: musl-gcc(musl-tools)、meson、ninja、bison、curl、sha256sum、tar、xz
# できるもの: target/musl-libs/lib/libpixman-1.a、target/musl-libs/lib/libxkbcommon.a
#
# ソースは決まった版を取り、SHA-256が合わなければ止まる。CPU向けの最適化の指定(-march=nativeなど)は
# 付けないので、できるコードはx86-64の基本の命令(SSE2まで)が前提になる。
# pixmanは、実行時にCPUを調べてMMX・SSE2・SSSE3の速い処理を選ぶ。0.42.2にはAVX2の処理は無い。
set -eu

PIXMAN_VERSION=0.42.2
PIXMAN_URL="https://www.cairographics.org/releases/pixman-${PIXMAN_VERSION}.tar.gz"
PIXMAN_SHA256=ea1480efada2fd948bc75366f7c349e1c96d3297d09a3fe62626e38e234a625e

XKBCOMMON_VERSION=1.6.0
XKBCOMMON_URL="https://xkbcommon.org/download/libxkbcommon-${XKBCOMMON_VERSION}.tar.xz"
XKBCOMMON_SHA256=0edc14eccdd391514458bc5f5a4b99863ed2d651e4dd761a90abf4f46ef99c2b

root=$(cd "$(dirname "$0")/.." && pwd)
prefix="$root/target/musl-libs"
work="$prefix/build"
mkdir -p "$work"

# 取ってきて、SHA-256を確かめてから展開する。
fetch() {
    url=$1
    sha256=$2
    file="$work/$(basename "$url")"
    if [ ! -f "$file" ]; then
        curl --fail --location --silent --show-error --output "$file.part" "$url"
        mv "$file.part" "$file"
    fi
    echo "$sha256  $file" | sha256sum --check --quiet
    tar -xf "$file" -C "$work"
}

# mesonに、musl-gccでx86-64のLinux向けに作ることを伝える。
cat > "$work/musl-cross.ini" <<CROSS
[binaries]
c = 'musl-gcc'
ar = 'ar'
strip = 'strip'

[host_machine]
system = 'linux'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
CROSS

# 共通の設定。静的ライブラリだけを作る。静的PIEにリンクできるよう、位置独立のコードにする。
configure() {
    src=$1
    shift
    rm -rf "$src/build-musl"
    meson setup "$src/build-musl" "$src" \
        --cross-file "$work/musl-cross.ini" \
        --prefix "$prefix" \
        --libdir lib \
        --buildtype release \
        --default-library static \
        -Db_staticpic=true \
        "$@"
    ninja -C "$src/build-musl" install
}

rm -rf "$work/pixman-$PIXMAN_VERSION" "$work/libxkbcommon-$XKBCOMMON_VERSION"

fetch "$PIXMAN_URL" "$PIXMAN_SHA256"
configure "$work/pixman-$PIXMAN_VERSION" \
    -Dtests=disabled \
    -Dgtk=disabled \
    -Dlibpng=disabled \
    -Dopenmp=disabled \
    -Dgnuplot=false \
    -Dtimers=false

fetch "$XKBCOMMON_URL" "$XKBCOMMON_SHA256"
configure "$work/libxkbcommon-$XKBCOMMON_VERSION" \
    -Denable-tools=false \
    -Denable-x11=false \
    -Denable-docs=false \
    -Denable-wayland=false \
    -Denable-xkbregistry=false \
    -Denable-bash-completion=false \
    -Dxkb-config-root=/usr/share/X11/xkb \
    -Dxkb-config-extra-path=/etc/xkb \
    -Dx-locale-root=/usr/share/X11/locale

# ライセンス文を、ライブラリの隣に置いておく(配布物に同梱するときの元になる)。
mkdir -p "$prefix/licenses"
cp "$work/pixman-$PIXMAN_VERSION/COPYING" "$prefix/licenses/pixman-$PIXMAN_VERSION.COPYING"
cp "$work/libxkbcommon-$XKBCOMMON_VERSION/LICENSE" "$prefix/licenses/libxkbcommon-$XKBCOMMON_VERSION.LICENSE"

echo "できたもの:"
ls -l "$prefix/lib/libpixman-1.a" "$prefix/lib/libxkbcommon.a"
