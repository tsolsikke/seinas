#!/bin/sh
# 既定のフォントを、版を固定して取得する。
#
# 使い方:
#     tools/fetch-fonts.sh
#
# できるもの(target/fonts/ の下。リポジトリには入れない):
#     BIZUDPGothic-Regular.ttf   BIZ UDPゴシック Regular(UI用。幅が文字ごとに違う)
#     BIZUDGothic-Regular.ttf    BIZ UDゴシック Regular(端末・コード用。半角と全角の幅が1:2)
#     OFL.txt、AUTHORS.txt、CONTRIBUTORS.txt   ライセンス文と著作権表示(上流のもの)
#
# 取得元は、googlefonts/morisawa-biz-ud-gothic の v1.051 のタグ(OFL 1.1で公開されている版)。
# SHA-256が合わなければ止まる。フォントは実行ファイルに埋め込まず、別のファイルとして読む。
set -eu

VERSION=v1.051
BASE="https://raw.githubusercontent.com/googlefonts/morisawa-biz-ud-gothic/$VERSION"

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/target/fonts"
mkdir -p "$out"

# 取ってきて、SHA-256を確かめる。合わなければ、置かずに止まる。
fetch() {
    path=$1
    sha256=$2
    file="$out/$(basename "$path")"
    if [ ! -f "$file" ] || ! echo "$sha256  $file" | sha256sum --check --quiet 2>/dev/null; then
        curl --fail --location --silent --show-error --output "$file.part" "$BASE/$path"
        echo "$sha256  $file.part" | sha256sum --check --quiet
        mv "$file.part" "$file"
    fi
}

fetch fonts/ttf/BIZUDPGothic-Regular.ttf 258d7156c165f2ff774b6efee637c22c3b950de0d8a10e501137061bc8085d01
fetch fonts/ttf/BIZUDGothic-Regular.ttf 7d2b48d84ef4e65c9f85bcf65fb1fb41c92b134be641fab6a7141d597c9a92b0
fetch OFL.txt e753d7155d53c747d037a445e584c8ecfca6dd79846db610417e282a736b28bc
fetch AUTHORS.txt cfd728c227f41117d8c23179963b72d97c7cff01345dc755b6cb819fa1d55ba2
fetch CONTRIBUTORS.txt e7511d722f989a8cab2701b39d76e38524a25a62faaac296fc48bf04bab0f2ed

echo "取得したもの($out):"
ls -l "$out"
