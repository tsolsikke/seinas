#!/bin/sh
# 既定のフォントを、バージョンを固定して取得する。
#
# 使い方:
#     tools/fetch-fonts.sh
#
# できるもの(target/fonts/ の下。リポジトリには入れない):
#     BIZUDPGothic-Regular.ttf   BIZ UDPゴシック Regular(UI用。幅が文字ごとに違う)
#     BIZUDGothic-Regular.ttf    BIZ UDゴシック Regular(端末・コード用。半角と全角の幅が1:2)
#     OFL.txt、AUTHORS.txt、CONTRIBUTORS.txt   BIZ UDゴシックのライセンス文と著作権表示(上流のもの)
#     unifont_jp-18.0.01.otf     GNU Unifont JP(控え用。上のフォントに無い文字を描く)
#
# 取得元:
# ・BIZ UDゴシック: googlefonts/morisawa-biz-ud-gothic の v1.051 のタグ(OFL 1.1で公開されているバージョン)。
# ・GNU Unifont JP: GNUの配布元(ftp.gnu.org)の 18.0.01。OFL 1.1と、GPL 2以降(フォントの埋め込みの
#   例外つき)の二重ライセンスで、SeinasはOFL 1.1を選んで使う。ライセンス文と著作権表示は
#   THIRD-PARTY/fonts/ に置いてある。
# バージョンは、ここに書いたものに固定している。SHA-256が合わなければ止まる。
# フォントは実行ファイルに埋め込まず、別のファイルとして読む。
set -eu

BIZ_UD="https://raw.githubusercontent.com/googlefonts/morisawa-biz-ud-gothic/v1.051"
UNIFONT="https://ftp.gnu.org/gnu/unifont/unifont-18.0.01"

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/target/fonts"
mkdir -p "$out"

# 取ってきて、SHA-256を確かめる。取れないとき、合わないときは、置かずに理由を出して止まる。
# ネットワークが無いときに長く待たないよう、つなぐまでと全体の時間に上限を置く。
fetch() {
    url=$1
    sha256=$2
    file="$out/$(basename "$url")"
    if [ -f "$file" ] && echo "$sha256  $file" | sha256sum --check --quiet >/dev/null 2>&1; then
        return
    fi
    if ! curl --fail --location --silent --show-error --retry 3 \
        --connect-timeout 30 --max-time 900 --output "$file.part" "$url"; then
        rm -f "$file.part"
        echo "fetch-fonts.sh: $url を取得できませんでした(ネットワークか、取得元の問題)" >&2
        exit 1
    fi
    actual=$(sha256sum "$file.part" | cut -d ' ' -f 1)
    if [ "$actual" != "$sha256" ]; then
        rm -f "$file.part"
        echo "fetch-fonts.sh: $url の SHA-256 が合いません(期待 $sha256、実際 $actual)" >&2
        exit 1
    fi
    mv "$file.part" "$file"
}

fetch "$BIZ_UD/fonts/ttf/BIZUDPGothic-Regular.ttf" 258d7156c165f2ff774b6efee637c22c3b950de0d8a10e501137061bc8085d01
fetch "$BIZ_UD/fonts/ttf/BIZUDGothic-Regular.ttf" 7d2b48d84ef4e65c9f85bcf65fb1fb41c92b134be641fab6a7141d597c9a92b0
fetch "$BIZ_UD/OFL.txt" e753d7155d53c747d037a445e584c8ecfca6dd79846db610417e282a736b28bc
fetch "$BIZ_UD/AUTHORS.txt" cfd728c227f41117d8c23179963b72d97c7cff01345dc755b6cb819fa1d55ba2
fetch "$BIZ_UD/CONTRIBUTORS.txt" e7511d722f989a8cab2701b39d76e38524a25a62faaac296fc48bf04bab0f2ed
fetch "$UNIFONT/unifont_jp-18.0.01.otf" b411e78488d861f6b41fba514381123abb01ba7adeac8d0df8801a61891a14d3

echo "取得したもの($out):"
ls -l "$out"
