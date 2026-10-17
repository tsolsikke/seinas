#!/bin/sh
# 正解データ(golden)を、今の出力で作り直す。
#
# 使い方:
#     tools/update-golden.sh
#
# 6 つの crate(seinas-render、seinas-text、seinas-frontend、seinas-fbdev、seinas-taskbar、seinas-wallpaper)の golden テストと、
# seinas-standalone の golden_layers を、
# SEINAS_UPDATE_GOLDEN=1 を付けてホストで動かす。テストは、比べるかわりに tests/golden/ のファイルを上書きし、
# 古いファイルとの差を 1 ケース 1 行で出す。最後に git diff --stat を出す。
#
# フォントが要るので、先に tools/fetch-fonts.sh を実行しておくこと(フォントが無いか、バージョンがちがうと止まる)。
# 作り直したときの決まり(コミットに理由と差分の要約を書く、など)は docs/golden.md にある。
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

SEINAS_UPDATE_GOLDEN=1 cargo test --locked \
    -p seinas-render -p seinas-text -p seinas-frontend -p seinas-fbdev -p seinas-taskbar -p seinas-wallpaper \
    --test golden
# layer surface とウィンドウの重なり(seinas-standalone を fake screen で動かす)。
SEINAS_UPDATE_GOLDEN=1 cargo test --locked -p seinas-standalone --test golden_layers

echo
git status --short -- 'crates/*/tests/golden'
git diff --stat -- 'crates/*/tests/golden'
