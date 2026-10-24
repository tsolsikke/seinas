# release のスクリプト(tools/release-standalone.sh、tools/release-fbdev.sh)が使う手続き。`. tools/release-lib.sh` で読む。
#
# 読む側は `set -eu` で動かす。/bin/sh(dash)には pipefail が無く、パイプの途中の失敗は set -e で止まらないので、
# 止めたい手順はパイプでつながない。

# 作業ツリーに変更が無く、HEAD が origin の main と同じであることを確かめる。違えば止まる(終わりの値 1)。
# release のタグは GitHub にあるコミットに付けるので、push する前のコミットからは作らない。
release_require_pushed() {
    # 作業ツリーに変更が残っていると、どのソースから作ったかが分からなくなる。
    if [ -n "$(git status --porcelain)" ]; then
        echo "作業ツリーに変更があります。コミットするか、元に戻してから作ってください。" >&2
        exit 1
    fi
    head=$(git rev-parse HEAD)
    if ! remote=$(git ls-remote --exit-code origin refs/heads/main); then
        echo "origin の main を読めませんでした。つながるかを確かめてから作ってください。" >&2
        exit 1
    fi
    remote=${remote%%	*}
    if [ "$head" != "$remote" ]; then
        echo "HEAD($head)が origin の main($remote)と違います。push してから作ってください。" >&2
        exit 1
    fi
}

# `dir` の下のディレクトリ `name` を、`dir/name.tar.gz` にまとめる。中身の並びと日時をそろえ、同じ中身なら同じ
# アーカイブになるようにする。tar と gzip をパイプでつながないので、どちらが失敗しても止まる。
release_pack() {
    tar -C "$1" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@0" -cf "$1/$2.tar" "$2"
    gzip -n "$1/$2.tar"
}
