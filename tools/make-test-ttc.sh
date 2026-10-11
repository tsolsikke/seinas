#!/bin/sh
# テストのための小さな TTC(face が 2 つ)を作る。
#
# 使い方:
#     tools/make-test-ttc.sh
#
# 要るもの: fontTools 4.60.1(pyftsubset と、python3 の fontTools)。tools/fetch-fonts.sh で取った
# target/fonts の BIZ UDPゴシックと BIZ UDゴシック。
#
# できるもの: crates/seinas-text/tests/data/two-faces.ttc
#     face 0  BIZ UDPゴシック Regular の、TEXT の文字だけにしたもの
#     face 1  BIZ UDゴシック Regular の、TEXT の文字だけにしたもの
# 名前の表(英語と日本語の名前)は残す。ヒンティングは外す。時刻は元のフォントのものを残すので、同じ
# 入力からは同じファイルができる。
set -eu

TEXT='Seinasi タイトル'

root=$(cd "$(dirname "$0")/.." && pwd)
fonts="$root/target/fonts"
out="$root/crates/seinas-text/tests/data/two-faces.ttc"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for name in BIZUDPGothic-Regular BIZUDGothic-Regular; do
    if [ ! -f "$fonts/$name.ttf" ]; then
        echo "make-test-ttc.sh: $fonts/$name.ttf がありません(先に tools/fetch-fonts.sh)" >&2
        exit 1
    fi
    pyftsubset "$fonts/$name.ttf" --text="$TEXT" --name-IDs='*' --name-languages='*' \
        --no-hinting --output-file="$work/$name.ttf"
done

python3 -I - "$out" "$work/BIZUDPGothic-Regular.ttf" "$work/BIZUDGothic-Regular.ttf" <<'PY'
import sys
from fontTools.ttLib import TTFont
from fontTools.ttLib.ttCollection import TTCollection

collection = TTCollection()
collection.fonts = [TTFont(path, recalcTimestamp=False) for path in sys.argv[2:]]
collection.save(sys.argv[1], shareTables=True)
PY

ls -l "$out"
