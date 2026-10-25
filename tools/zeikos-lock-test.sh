#!/bin/sh
# tools/zeikos-lock.sh の振る舞いを確かめる。本物のロック(/tmp/zeikos-qemu.lock)には触らない。
#
# 使い方:
#     tools/zeikos-lock-test.sh
#
# 一時的な場所に、ロックのファイルの場所と待つ上限(1 秒)だけを替えたコピーを作り、そのコピーで次を確かめる。ZeikOS の
# QEMU の役は、コピーのロックを共有で持つ sleep が務める。
#
# 1. ふつうに包むと、コマンドの中では --held が通り、終わればロックは外れる。
# 2. 中から呼ぶと、取り直さずに動く(自分の親と取り合って止まらない)。
# 3. SEINAS_HOLDS_QEMU_LOCK を偽って渡しても(fd が無い、ロックのファイルを開いただけでロックを持っていない)、ロックを
#    取らずには走らない(ZeikOS の役が持っている間は、待って、上限で動かさずに終わる)。
# 4. 上限まで待っても取れなければ、コマンドを動かさずに、終わりの値 1 で終わる。
# 5. コマンドが終わっても子孫が残れば、ロックは子孫が終わるまで外れず、その pid とコマンド名を出す。
#
# 終わりの値:0 は全部通った、1 は落ちた確かめがある。
set -eu

# pre-push の中(本物のロックを持つ zeikos-lock.sh の子)で動くときも、本物のロックを受け継がない。コピーの入れ子の
# 確かめが、受け継いだ値で揺れないようにし、ここで起こす sleep などが本物のロックを持ち続けないようにする。
unset SEINAS_HOLDS_QEMU_LOCK SEINAS_QEMU_LOCK_OWNER
exec 9<&-

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
holder=""
cleanup() {
    if [ -n "$holder" ]; then
        kill "$holder" 2>/dev/null || true
    fi
    rm -rf -- "${work:?}"
}
trap cleanup EXIT

lock=$work/qemu.lock
sed -e "s#^lock=/tmp/zeikos-qemu.lock\$#lock=$lock#" -e 's/^wait_seconds=5400$/wait_seconds=1/' \
    "$root/tools/zeikos-lock.sh" > "$work/zl.sh"
chmod +x "$work/zl.sh"
grep -q "^lock=$lock\$" "$work/zl.sh"
grep -q '^wait_seconds=1$' "$work/zl.sh"
zl=$work/zl.sh
failed=0

check() {
    if [ "$2" = "$3" ]; then
        echo "ok: $1"
    else
        echo "FAILED: $1 (expected $3, got $2)"
        failed=1
    fi
}

# ロックが空いているか(新しく開いて、排他で取れるか)。
free() {
    if flock -x -n "$lock" true; then echo free; else echo held; fi
}

# ZeikOS の QEMU の役:コピーのロックを共有で持つ(sleep の pid を holder に置く。flock は fd を sleep へ渡す)。
hold_shared() {
    exec 8< "$lock"
    flock -s 8
    sleep 30 &
    holder=$!
    exec 8<&-
}

release_shared() {
    kill "$holder"
    wait "$holder" 2>/dev/null || true
    holder=""
}

# 1. ふつうに包む。
got=$("$zl" sh -c '"$0" --held && echo held-inside' "$zl") || true
check "the command runs with the lock held" "$got" held-inside
check "the lock is released after the command" "$(free)" free

# 2. 中から呼ぶ。
got=$("$zl" sh -c '"$0" sh -c "\"\$0\" --held && echo nested" "$0"' "$zl") || true
check "a nested call runs without taking the lock again" "$got" nested

# 3. 偽の SEINAS_HOLDS_QEMU_LOCK。ZeikOS の役が持つ間は、動かずに上限で終わる。
hold_shared
status=0
got=$(SEINAS_HOLDS_QEMU_LOCK=1 "$zl" echo ran 2>/dev/null) || status=$?
check "a forged number without an open fd does not run" "$got:$status" ":1"
status=0
got=$(sh -c 'exec 9< "$1"; SEINAS_HOLDS_QEMU_LOCK=9 "$2" echo ran' sh "$lock" "$zl" 2>/dev/null) || status=$?
check "an open fd without the lock does not run" "$got:$status" ":1"

# 4. 上限で終わる(上の 2 つと同じ形を、偽りなしで)。
status=0
got=$("$zl" echo ran 2>/dev/null) || status=$?
check "the command does not run when the lock is not taken in time" "$got:$status" ":1"
release_shared

# 5. 子孫が残る。
"$zl" sh -c 'sleep 30 & echo $! > "$0"' "$work/left.pid" 2> "$work/left.log"
left=$(cat "$work/left.pid")
check "the lock stays while a descendant is left" "$(free)" held
if grep -q "still hold $lock; the lock stays until they end: $left(sleep)" "$work/left.log"; then
    echo "ok: the left descendant is named with its pid and command"
else
    echo "FAILED: the left descendant is not named: $(cat "$work/left.log")"
    failed=1
fi
kill "$left"
while kill -0 "$left" 2>/dev/null; do
    sleep 0.1
done
check "the lock is released after the descendant ends" "$(free)" free

exit "$failed"
