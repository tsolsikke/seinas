#!/bin/sh
# 重い作業を、ZeikOS の QEMU と重ならないように、ホストのロックで包んで動かす。
#
# 使い方:
#     tools/zeikos-lock.sh <コマンド> [引数...]      例: tools/zeikos-lock.sh cargo test --workspace --locked
#
# ZeikOS は、QEMU を使う間、/tmp/zeikos-qemu.lock を flock の共有(LOCK_SH)で持つ(zeikos のリポジトリの
# ADR-0089 の「重い作業をする側の約束」)。こちらは、重い作業(cargo test の全体、pre-push の code 以上の段、
# musl のビルド)の間、同じファイルを排他(LOCK_EX)で持つ。取れなければ、1 行出してから待つ。
#
# - ロックのファイルには書かない。消さない(flock が、無ければ空のファイルを作るだけ)。
# - 待つのは最長 90 分(ZeikOS の全検査は 1 時間ほどかかる)。過ぎたら、コマンドを動かさずに終わる(終わりの値 1)。
#   コマンドが 75 で終わったときも、時間切れと見分けられないので、同じ扱いになる(Seinas の道具は 75 を使わない)。
# - 取った後は SEINAS_HOLDS_QEMU_LOCK=1 を子に渡し、中から呼ばれたときは取り直さない(同じロックを、自分の親と
#   取り合って止まらないように)。
set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: tools/zeikos-lock.sh <command> [args...]" >&2
    exit 2
fi

if [ "${SEINAS_HOLDS_QEMU_LOCK:-}" = 1 ]; then
    exec "$@"
fi

lock=/tmp/zeikos-qemu.lock
wait_seconds=5400

if ! flock -x -n "$lock" true; then
    echo "waiting for ZeikOS's QEMU ($lock。最長 $((wait_seconds / 60)) 分)" >&2
fi
export SEINAS_HOLDS_QEMU_LOCK=1
status=0
# 時間切れのときだけ 75 にし、コマンドの終わりの値と見分ける。
flock -x -w "$wait_seconds" -E 75 "$lock" "$@" || status=$?
if [ "$status" -eq 75 ]; then
    echo "zeikos-lock.sh: $((wait_seconds / 60)) 分待っても $lock が取れなかったので、動かさずに終わります" >&2
    exit 1
fi
exit "$status"
