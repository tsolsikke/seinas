#!/bin/bash
# C1S3 で入った機能を、入れ子の構成(WSLg)で 1 回の起動でまとめて確かめるための script(docs/demo-c1s3.md)。
#
# 使い方(リポジトリの先頭から。WSLg などの Wayland のセッションの中で):
#     tools/demo-c1s3.sh
#
# 起こすもの:入れ子の Seinas(--log-keys)、壁紙、タスクバー、テスト用のウィンドウ 3 つ(text-input と IME、
# キーボード、ping に答えない)、テスト用の IME。どれも同じ親の Wayland(WAYLAND_DISPLAY)の中の Seinas に
# つなぐ。
#
# - 設定(フォント、壁紙、時計、切り替えのキー)は、一時的な XDG_CONFIG_HOME に置く。~/.config は読まず、書き換えない。
# - Seinas のソケットは、一時的な XDG_RUNTIME_DIR に置く(いつもの seinas-0 と重ならない)。
# - 各プロセスのログは、別の場所(終わった後も残す)のファイルに分けて書き、終わりにその場所を出す。
# - Seinas のウィンドウを閉じるか、Ctrl+C で終わる。終わるときは、起こしたものを全部止め、一時的な場所を消し、
#   tools/check-leftovers.py で、残りが 0 であることを確かめる。
# - 子には PR_SET_PDEATHSIG(setpriv があれば)と、印の環境変数 SEINAS_TEST_CHILD を付ける。この script が
#   SIGKILL で終わっても、子は止まり、残った一時的な場所は tools/check-leftovers.py --clean で消せる。
#
# Windows の側の操作(入力の送り込み、ウィンドウの操作)は、何もしない(CLAUDE.md の決まり 1)。
#
# 環境変数:
#   DEMO_LOG_DIR       ログの置き場(無ければ /tmp/seinas-demo-logs-<日時>)
#   DEMO_SEINAS_CONF   seinas.conf の代わりに使うファイル(切り替えのキーを変えて確かめるとき)
set -u

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root" || exit 2

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "demo-c1s3.sh: WAYLAND_DISPLAY がありません。Wayland のセッション(WSLg など)の中で動かしてください。" >&2
    exit 2
fi
parent=$WAYLAND_DISPLAY
case "$parent" in
    /*) ;;
    *) parent="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR がありません}/$parent" ;;
esac

echo "== ビルド(変わっていなければ、すぐ終わる)"
cargo build --locked --quiet -p seinas -p seinas-taskbar -p seinas-wallpaper -p seinas-test-clients || exit 2
[ -d target/fonts ] || echo "注意:target/fonts がありません(tools/fetch-fonts.sh)。文字は出ません。"
bin=$root/target/debug

# 一時的な場所(終わるときに消す。印を付け、残ったときは check-leftovers.py が見つける)と、ログの置き場(残す)。
work="${TMPDIR:-/tmp}/seinas-demo-$$"
mkdir "$work" || exit 2
: > "$work/.seinas-test"
logs=${DEMO_LOG_DIR:-/tmp/seinas-demo-logs-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$logs" "$work/run" "$work/config/seinas"
chmod 700 "$work/run"

# 確かめ用の設定。
cat > "$work/config/seinas/fonts.conf" <<'EOF'
# タイトルとタスクバーの文字を、既定(BIZ UDPゴシック)と違う BIZ UDゴシック にする(ログで確かめる)。
ui = BIZ UDGothic
default-dirs = no
EOF
cat > "$work/config/seinas/wallpaper.conf" <<EOF
# テスト用の画像(48x32)を、中央に置き、余白を緑にする。
image = $root/crates/seinas-wallpaper/tests/data/test-48x32.png
mode = center
color = 2f6f4f
EOF
cat > "$work/config/seinas/taskbar.conf" <<'EOF'
# 時計は日本の時刻。
utc-offset = +09:00
EOF
if [ -n "${DEMO_SEINAS_CONF:-}" ]; then
    cp "$DEMO_SEINAS_CONF" "$work/config/seinas/seinas.conf" || exit 2
else
    cat > "$work/config/seinas/seinas.conf" <<'EOF'
# 入れ子の既定と同じ(Alt+Tab と Alt+F6)。届かないときは、DEMO_SEINAS_CONF で別のファイルを渡して起こし直す。
switch-next = Alt+Tab, Alt+F6
switch-prev = Alt+Shift+Tab, Alt+Shift+F6
EOF
fi

pids=()
pdeathsig=()
if command -v setpriv > /dev/null && setpriv --pdeathsig KILL true 2> /dev/null; then
    pdeathsig=(setpriv --pdeathsig KILL)
fi
# 1 つのプロセスを起こす。名前 $1 のログに、標準出力と標準エラーを書く。
start() {
    local name=$1
    shift
    env SEINAS_TEST_CHILD=$$ XDG_CONFIG_HOME="$work/config" XDG_RUNTIME_DIR="$work/run" \
        SEINAS_FONTS="$root/target/fonts" "${pdeathsig[@]}" "$@" \
        < /dev/null > "$logs/$name.log" 2>&1 &
    pids+=($!)
    echo "   $name(PID $!):ログ $logs/$name.log"
}

finished=false
cleanup() {
    $finished && return
    finished=true
    trap - INT TERM EXIT
    echo
    echo "== 止める"
    for pid in "${pids[@]}"; do
        kill "$pid" 2> /dev/null
    done
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        alive=false
        for pid in "${pids[@]}"; do
            kill -0 "$pid" 2> /dev/null && alive=true
        done
        $alive || break
        sleep 0.3
    done
    for pid in "${pids[@]}"; do
        kill -9 "$pid" 2> /dev/null
    done
    wait 2> /dev/null
    rm -rf "$work"
    python3 "$root/tools/check-leftovers.py"
    status=$?
    echo "ログ:$logs"
    ls "$logs"
    exit $status
}
trap cleanup INT TERM EXIT

echo "== 設定:$work/config/seinas(終わるときに消す)"
echo "== 起こす(親:$parent)"
start seinas env WAYLAND_DISPLAY="$parent" "$bin/seinas" --log-keys --fonts "$root/target/fonts"
seinas_pid=${pids[0]}
for _ in $(seq 1 100); do
    [ -S "$work/run/seinas-0" ] && [ -S "$work/run/seinas-0-shell" ] && [ -S "$work/run/seinas-0-ime" ] && break
    kill -0 "$seinas_pid" 2> /dev/null || { echo "Seinas が起動しませんでした($logs/seinas.log)"; exit 1; }
    sleep 0.1
done
shell="$work/run/seinas-0-shell"
ime_socket="$work/run/seinas-0-ime"
apps="$work/run/seinas-0"
start wallpaper env WAYLAND_DISPLAY="$shell" "$bin/seinas-wallpaper"
start taskbar env WAYLAND_DISPLAY="$shell" "$bin/seinas-taskbar" --fonts "$root/target/fonts"
sleep 0.5
start window-keyboard env WAYLAND_DISPLAY="$apps" "$bin/test-client" window --keyboard --pointer \
    --follow-size --title "キーボード" --color 3060c0
sleep 0.3
start window-unresponsive env WAYLAND_DISPLAY="$apps" "$bin/test-client" window --ignore-ping \
    --follow-size --title "応答なし(ping に答えない)" --color c04040
sleep 0.3
start window-text env WAYLAND_DISPLAY="$apps" "$bin/test-client" window --text-input --follow-size \
    --title "文字入力(text-input)" --color ff8000
sleep 0.3
start ime env WAYLAND_DISPLAY="$ime_socket" "$bin/test-client" ime --popup 40x30

echo
echo "準備ができました。docs/demo-c1s3.md の順に確かめてください。"
echo "Seinas のウィンドウを閉じるか、この端末で Ctrl+C を押すと終わります。"
wait "$seinas_pid"
echo "Seinas が終わりました。"
