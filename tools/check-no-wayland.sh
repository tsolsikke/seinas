#!/bin/sh
# Waylandの受け口を含まない実行ファイルであることを確かめる。
#
# 使い方:
#     tools/check-no-wayland.sh <パッケージ名> <実行ファイル>...
#
# 確かめること:
# ・パッケージの依存に、wayland系のクレートが入っていない(cargo tree)。
# ・実行ファイルに、Smithayの共有メモリーの実装が入っていない。
#   (「Shm dropping thread」の文字列と、sigbus_handlerのシンボルが無い。)
set -eu
export LC_ALL=C

package=$1
shift

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if cargo tree --locked -p "$package" -e normal | grep -i wayland; then
    echo "$package: 依存にwayland系のクレートが入っています" >&2
    exit 1
fi
echo "$package: 依存にwayland系のクレートはありません"

for file in "$@"; do
    if strings -a "$file" | grep -q 'Shm dropping thread'; then
        echo "$file: 「Shm dropping thread」の文字列があります" >&2
        exit 1
    fi
    if nm "$file" 2>/dev/null | grep -q 'sigbus_handler'; then
        echo "$file: sigbus_handlerのシンボルがあります" >&2
        exit 1
    fi
    echo "$file: 共有メモリーの実装は入っていません"
done
