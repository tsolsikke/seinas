#!/bin/sh
# 重い作業を、ZeikOS の QEMU と重ならないように、ホストのロックで包んで動かす。
#
# 使い方:
#     tools/zeikos-lock.sh <コマンド> [引数...]      例: tools/zeikos-lock.sh cargo test --workspace --locked
#     tools/zeikos-lock.sh --held                    このプロセスが、もうロックを持っているか(持っていれば終わりの値 0)
#
# ZeikOS は、QEMU を使う間、/tmp/zeikos-qemu.lock を flock の共有(LOCK_SH)で持つ(zeikos のリポジトリの
# ADR-0089 の「重い作業をする側の約束」)。こちらは、重い作業(cargo test の全体、pre-push の code 以上の段、
# musl のビルド)の間、同じファイルを排他(LOCK_EX)で持つ。取れなければ、1 行出してから待つ。
#
# - ロックのファイルには書かない。消さない(無ければ空のファイルを作るだけ)。
# - 待つのは最長 90 分(ZeikOS の全検査は 1 時間ほどかかる)。過ぎたら、コマンドを動かさずに終わる(終わりの値 1)。
# - ロックは、fd 9 に開いたロックのファイルで持つ。fd 9 はコマンドとその子孫に受け継がれるので、コマンドが終わっても
#   子孫が残っていれば、その子孫が終わるまでロックは外れない(作業が続いたままロックが外れて、ZeikOS の QEMU と
#   重なる形を避ける)。コマンドが終わった後に、ロックを持ったまま残った子孫の pid とコマンド名を出す。
# - 取った後は SEINAS_HOLDS_QEMU_LOCK=9(ロックの fd の番号)を子に渡す。中から呼ばれたときは、その fd がロックの
#   ファイルを指し、排他のロックを持っていることを確かめてから、取り直さずに動かす(同じロックを、自分の親と取り
#   合って止まらないように)。確かめられなければ、ふつうに取る(環境変数だけでは、ロックを持っていると見なさない)。
set -eu

lock=/tmp/zeikos-qemu.lock
wait_seconds=5400

if [ "$#" -eq 0 ]; then
    echo "usage: tools/zeikos-lock.sh <command> [args...] | --held" >&2
    exit 2
fi

# SEINAS_HOLDS_QEMU_LOCK の fd が、ロックのファイル(同じ inode)を指し、排他のロックを持っているか。同じ open file
# description が持つロックは、flock -x -n で取り直しても通る。持っていなければ、ほかの人(ZeikOS の共有など)が
# 持っていると断られ、誰も持っていなければ、ここで取る(どちらでも、通ったときはロックを持っている)。
holds_lock() {
    held=${SEINAS_HOLDS_QEMU_LOCK:-}
    case $held in
        '' | *[!0-9]*) return 1 ;;
    esac
    [ -e "/proc/self/fd/$held" ] && [ "/proc/self/fd/$held" -ef "$lock" ] && flock -x -n "$held"
}

if [ "$1" = --held ]; then
    holds_lock
    exit
fi

if [ -n "${SEINAS_HOLDS_QEMU_LOCK:-}" ]; then
    if holds_lock; then
        exec "$@"
    fi
    echo "zeikos-lock.sh: SEINAS_HOLDS_QEMU_LOCK=$SEINAS_HOLDS_QEMU_LOCK does not hold $lock; taking the lock" >&2
fi

[ -e "$lock" ] || : >> "$lock"
exec 9< "$lock"
if ! flock -x -n 9; then
    echo "waiting for ZeikOS's QEMU ($lock。最長 $((wait_seconds / 60)) 分)" >&2
    if ! flock -x -w "$wait_seconds" 9; then
        echo "zeikos-lock.sh: $((wait_seconds / 60)) 分待っても $lock が取れなかったので、動かさずに終わります" >&2
        exit 1
    fi
fi

# 残った子孫を見分けるため、このロックを持たせたプロセスに、印(このスクリプトの pid)を付ける。
export SEINAS_HOLDS_QEMU_LOCK=9 SEINAS_QEMU_LOCK_OWNER=$$
status=0
"$@" || status=$?

# コマンドが終わった後も、ロックの fd を持ったまま残っているプロセス(印の付いたもの)を出す。止めはしない。
left=""
for path in /proc/[0-9]*; do
    pid=${path#/proc/}
    [ "$pid" != $$ ] || continue
    grep -qz "^SEINAS_QEMU_LOCK_OWNER=$$\$" "$path/environ" 2>/dev/null || continue
    for open in "$path"/fd/*; do
        if [ "$open" -ef "$lock" ] 2>/dev/null; then
            left="$left $pid($(cat "$path/comm" 2>/dev/null || echo '?'))"
            break
        fi
    done
done
if [ -n "$left" ]; then
    echo "zeikos-lock.sh: the command ended, but these processes still hold $lock; the lock stays until they end:$left" >&2
fi
exit "$status"
