#!/bin/sh
# パッケージの依存に、入っていてはいけないクレートが無いことを確かめる。
#
# 使い方:
#     tools/check-deps.sh <パッケージ名> <入っていてはいけないクレート>...
#     tools/check-deps.sh --direct-users <クレート> <直に使ってよいパッケージ>...
#
# 例(フロントエンドとfbdevを組み合わせた構成に、入れ子のバックエンドが入っていないこと):
#     tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client
#
# --direct-users は、ワークスペースの中で、そのクレートを直に依存に書いているパッケージが、一覧の中のものだけである
# ことを確かめる(一覧の外のパッケージが直に使ったら落ちる。一覧のパッケージを通して入るのはよい)。例(PNG を読む
# クレートを直に使ってよいのは、壁紙とアイコンのクレートだけ。ADR 0007):
#     tools/check-deps.sh --direct-users png seinas-wallpaper seinas-icons
#
# 見るのは、実行ファイルに入る依存だけ(cargo tree -e normal)。テストだけの依存は見ない。
set -eu
export LC_ALL=C

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if [ "$1" = "--direct-users" ]; then
    crate=$2
    shift 2
    # 逆向きの木の深さ 1 が、そのクレートを直に使うパッケージ。cargo tree が失敗したら(Cargo.lock が古いなど)、
    # 落ちる(空の結果を「直に使うものは無い」と取り違えない)。
    tree=$(cargo tree --locked --workspace -e normal -i "$crate" --prefix depth) || {
        echo "$crate: 依存の木を読めませんでした" >&2
        exit 1
    }
    users=$(echo "$tree" | sed -n 's/^1\([^ ]*\) .*/\1/p' | sort -u)
    status=0
    for user in $users; do
        allowed=false
        for name in "$@"; do
            [ "$user" = "$name" ] && allowed=true
        done
        if $allowed; then
            echo "$crate: $user が直に使っています(一覧の中)"
        else
            echo "$crate: $user が直に使っています。直に使ってよいのは $* だけです" >&2
            status=1
        fi
    done
    exit $status
fi

package=$1
shift

# cargo tree が失敗したら(Cargo.lock が古いなど)、落ちる(空の結果を「入っていない」と取り違えない)。
tree=$(cargo tree --locked -p "$package" -e normal --prefix none) || {
    echo "$package: 依存の木を読めませんでした" >&2
    exit 1
}
tree=$(echo "$tree" | sort -u)
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
