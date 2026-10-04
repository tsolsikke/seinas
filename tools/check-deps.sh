#!/bin/sh
# パッケージの依存に、入っていてはいけないクレートが無いことを確かめる。
#
# 使い方:
#     tools/check-deps.sh <パッケージ名> <入っていてはいけないクレート>...
#
# 例(受け口とfbdevを組み合わせた構成に、入れ子の裏側が入っていないこと):
#     tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client
#
# 見るのは、実行ファイルに入る依存だけ(cargo tree -e normal)。テストだけの依存は見ない。
set -eu
export LC_ALL=C

package=$1
shift

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

tree=$(cargo tree --locked -p "$package" -e normal --prefix none | sort -u)
status=0
for crate in "$@"; do
    if echo "$tree" | grep -q "^$crate v"; then
        echo "$package: 依存に $crate が入っています" >&2
        status=1
    else
        echo "$package: 依存に $crate はありません"
    fi
done
exit $status
