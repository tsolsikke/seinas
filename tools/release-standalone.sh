#!/bin/sh
# seinas-standalone(musl向けの静的PIE)を、GitHubのreleaseに置くためのアセットを作る。
#
# 使い方:
#     tools/release-standalone.sh <タグ>   例: tools/release-standalone.sh v0.2.0
#
# できるもの(target/release-assets/<タグ>/ の下):
#     seinas-standalone                            実行ファイル(musl向けの静的PIE。tools/build-musl.sh の出力と同じもの。
#                                                  strip はしない)
#     seinas-standalone-<タグ>-third-party.tar.gz  実行ファイルに入っている第三者のライセンス文と著作権表示
#     SHA256SUMS                                   上の2つのSHA-256
#     NOTES.md                                     releaseの説明文(作り方、バージョン、ライセンス)
#
# 作った後、releaseを作るgh のコマンドを表示する(実行はしない)。手順は docs/release.md にある。
#
# 要るもの: tools/build-musl.sh が要るもの(musl-gcc、meson、ninja、bison、curl)、python3、nm、tar、gzip
set -eu
export LC_ALL=C

tag=${1:?usage: tools/release-standalone.sh <tag>}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
. tools/release-lib.sh

# 作業ツリーに変更が無く、HEAD が push 済み(origin の main と同じ)であること。
release_require_pushed
commit=$(git rev-parse HEAD)

# いつも同じ条件で作るため、musl向けの成果物を消してから作り直す(Cのライブラリも作り直す)。
rm -rf target/x86_64-unknown-linux-musl target/musl-libs
tools/build-musl.sh > /dev/null
binary=target/x86_64-unknown-linux-musl/release/seinas-standalone
tools/elf-report.sh "$binary" > /dev/null
python3 tools/third-party.py --check > /dev/null

out=target/release-assets/$tag
rm -rf "$out"
mkdir -p "$out"
cp "$binary" "$out/seinas-standalone"

# 第三者のライセンス文を、1つのアーカイブにまとめる(tools/release-lib.sh の release_pack)。
notices=seinas-standalone-$tag-third-party
python3 tools/release-notices.py seinas-standalone "$binary" "$out/$notices" > /dev/null
release_pack "$out" "$notices"
rm -r "$out/$notices"

(cd "$out" && sha256sum seinas-standalone "$notices.tar.gz" > SHA256SUMS)

rust=$(rustc -V)
cat > "$out/NOTES.md" <<NOTES
# seinas-standalone $tag

ZeikOS の上で Seinas のコンポジタを動かすための、\`seinas-standalone\` の実行ファイルです。
親の Wayland を使わない構成の Seinas で、Wayland のクライアントを受け、Linux の fbdev(\`/dev/fb0\`)へ絵を出します。
最初の 1 枚を出し終えたときに、ログ(標準エラー)へ \`presented the first frame (<幅>x<高さ>)\` を 1 回だけ出します。
x86-64 の Linux 向けに、musl で静的にリンクした PIE で、共有ライブラリに依存しません。
ZeikOS 側の仕組みに合わせた変更は入れていません(Linux のプログラムをそのまま動かします)。

## 置いてあるもの

| ファイル | 中身 |
| --- | --- |
| \`seinas-standalone\` | 実行ファイル(x86-64、musl、静的 PIE。strip していない) |
| \`$notices.tar.gz\` | この実行ファイルに入っている第三者のソフトウェアのライセンス文と著作権表示(\`NOTICES.md\` が一覧) |
| \`SHA256SUMS\` | 上の2つの SHA-256 |

## 作り方

- ソース: https://github.com/tsolsikke/seinas のコミット \`$commit\`(タグ \`$tag\`)
- 手順: \`tools/release-standalone.sh $tag\`(中で \`tools/build-musl.sh\` を使う。手順の説明は \`docs/release.md\` と \`docs/musl-static.md\`)
- Rust: $rust(ターゲットは x86_64-unknown-linux-musl、CPU の指定は無し。既定の x86-64、SSE2 まで)
- musl: 1.2.5(Rust に同梱のもの。ヘッダーはホストの musl-gcc の 1.2.4)
- pixman: 0.42.2、libxkbcommon: 1.6.0(ソースから musl 向けに静的ビルド)

## ライセンス

- Seinas 自身のコードは MIT ライセンスです(アーカイブの中の \`LICENSE\`)。
- 第三者のソフトウェア(pixman、libxkbcommon、musl、Rust のクレート、Rust の標準ライブラリ、libunwind)は、それぞれのライセンスに従います。本文と著作権表示は \`$notices.tar.gz\` にあります。
NOTES

echo "できたもの($out):"
cat "$out/SHA256SUMS"
echo
echo "releaseを作るには、次を実行してください(タグは、このコミットに付きます):"
echo "  gh release create $tag --target $commit --title \"seinas-standalone $tag\" --notes-file $out/NOTES.md $out/seinas-standalone $out/$notices.tar.gz $out/SHA256SUMS"
