#!/bin/sh
# 正解データ(golden)を、今の出力で作り直す。
#
# 使い方:
#     tools/update-golden.sh
#
# 4 つの crate(seinas-render、seinas-text、seinas-frontend、seinas-fbdev)の golden テストを、
# SEINAS_UPDATE_GOLDEN=1 を付けてホストで動かす。テストは、比べるかわりに tests/golden/ のファイルを上書きし、
# 古いファイルとの差を 1 ケース 1 行で出す。最後に git diff --stat を出す。
#
# フォントが要るので、先に tools/fetch-fonts.sh を実行しておくこと(フォントが無いか、版がちがうと止まる)。
# 作り直したときの決まり(コミットに理由と差分の要約を書く、など)は docs/golden.md にある。
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

SEINAS_UPDATE_GOLDEN=1 cargo test --locked \
    -p seinas-render -p seinas-text -p seinas-frontend -p seinas-fbdev \
    --test golden

echo
git status --short -- 'crates/*/tests/golden'
git diff --stat -- 'crates/*/tests/golden'
