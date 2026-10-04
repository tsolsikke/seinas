#!/bin/sh
# 実行ファイルのELFの形を書き出す。静的にリンクした静的PIEでなければ、失敗で終わる。
#
# 使い方:
#     tools/elf-report.sh target/x86_64-unknown-linux-musl/release/seinas ...
#
# 書き出すもの: ELFの種類、PT_INTERPと共有ライブラリへの依存の有無、PT_TLSの大きさ、
# プログラムヘッダー(区画の権限と大きさ)、ファイルの大きさ。
set -eu
export LC_ALL=C

for file in "$@"; do
    echo "== $file"
    echo "file size: $(stat -c %s "$file") bytes"
    readelf -hW "$file" | grep -E '^ +(Type|Machine):'
    interp=$(readelf -lW "$file" | grep -c ' INTERP ' || true)
    needed=$(readelf -dW "$file" | grep -c '(NEEDED)' || true)
    echo "PT_INTERP: $interp, DT_NEEDED: $needed"
    readelf -lW "$file" | awk '$1 == "TLS" { printf "PT_TLS: FileSiz %s, MemSiz %s, Align %s\n", $5, $6, $8; found = 1 }
        END { if (!found) print "PT_TLS: none" }'
    readelf -lW "$file" | awk '/^Program Headers:/ { on = 1 } /Section to Segment/ { on = 0 } on && NF'
    echo

    if ! readelf -hW "$file" | grep -q 'Type: *DYN'; then
        echo "$file: ET_DYN(静的PIE)ではありません" >&2
        exit 1
    fi
    if [ "$interp" != 0 ] || [ "$needed" != 0 ]; then
        echo "$file: 静的にリンクされていません(PT_INTERPかDT_NEEDEDがあります)" >&2
        exit 1
    fi
done
