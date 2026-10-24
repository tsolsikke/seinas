#!/bin/sh
# ZeikOS の上で動かす Seinas のコンポジタ(seinas-standalone)とタスクバー(seinas-taskbar)を、GitHubのreleaseに
# 置くためのアセットを作る。どちらも musl向けの静的PIE。
#
# 使い方:
#     tools/release-standalone.sh <タグ>   例: tools/release-standalone.sh v0.3.0
#
# できるもの(target/release-assets/<タグ>/ の下):
#     seinas-standalone                            コンポジタの実行ファイル(tools/build-musl.sh の出力と同じもの。
#                                                  strip はしない)
#     seinas-standalone-<タグ>-third-party.tar.gz  コンポジタに入っている第三者のライセンス文と著作権表示
#     seinas-taskbar                               タスクバーの実行ファイル(同じく、strip はしない)
#     seinas-taskbar-<タグ>-third-party.tar.gz     タスクバーに入っている第三者のライセンス文と著作権表示
#     SHA256SUMS                                   上の4つのSHA-256
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

# 置く実行ファイル(パッケージの名前と、実行ファイルの名前が同じもの)。名前に空白は無い。
programs="seinas-standalone seinas-taskbar"

# 作業ツリーに変更が無く、HEAD が push 済み(origin の main と同じ)であること。
release_require_pushed
commit=$(git rev-parse HEAD)

# いつも同じ条件で作るため、musl向けの成果物を消してから作り直す(Cのライブラリも作り直す)。
rm -rf target/x86_64-unknown-linux-musl target/musl-libs
tools/build-musl.sh > /dev/null
python3 tools/third-party.py --check > /dev/null

out=target/release-assets/$tag
rm -rf "$out"
mkdir -p "$out"
assets=""
for program in $programs; do
    binary=target/x86_64-unknown-linux-musl/release/$program
    tools/elf-report.sh "$binary" > /dev/null
    cp "$binary" "$out/$program"
    # 第三者のライセンス文を、実行ファイルごとに1つのアーカイブにまとめる(tools/release-lib.sh の release_pack)。
    notices=$program-$tag-third-party
    python3 tools/release-notices.py "$program" "$binary" "$out/$notices" > /dev/null
    release_pack "$out" "$notices"
    rm -r "$out/$notices"
    assets="$assets $program $notices.tar.gz"
done

# assets は、空白で区切った名前の並び(わざと引用しない)。
(cd "$out" && sha256sum $assets > SHA256SUMS)

rust=$(rustc -V)
cat > "$out/NOTES.md" <<NOTES
# Seinas $tag(seinas-standalone、seinas-taskbar)

ZeikOS の上で Seinas のコンポジタとタスクバーを動かすための実行ファイルです。

- \`seinas-standalone\`:親の Wayland を使わない構成の Seinas のコンポジタです。Wayland のクライアントを受け、Linux の fbdev(\`/dev/fb0\`)へ絵を出します。最初の 1 枚を出し終えたときに、ログ(標準エラー)へ \`presented the first frame (<幅>x<高さ>)\` を 1 回だけ出します。
- \`seinas-taskbar\`:Seinas のタスクバー(シェルの部品)です。コンポジタのシェルの部品のためのソケットへつなぎ、画面の下に出ます。コンポジタより先に起こされても、ソケットができるまで待ってつなぎます。

どちらも x86-64 の Linux 向けに、musl で静的にリンクした PIE で、共有ライブラリに依存しません。
ZeikOS 側の仕組みに合わせた変更は入れていません(Linux のプログラムをそのまま動かします)。
起動のしかたと置くファイルは、リポジトリの \`docs/standalone.md\` の「ZeikOS の上で起動する」にあります。

## 置いてあるもの

| ファイル | 中身 |
| --- | --- |
| \`seinas-standalone\` | コンポジタの実行ファイル(x86-64、musl、静的 PIE。strip していない) |
| \`seinas-standalone-$tag-third-party.tar.gz\` | コンポジタに入っている第三者のソフトウェアのライセンス文と著作権表示(\`NOTICES.md\` が一覧) |
| \`seinas-taskbar\` | タスクバーの実行ファイル(x86-64、musl、静的 PIE。strip していない) |
| \`seinas-taskbar-$tag-third-party.tar.gz\` | タスクバーに入っている第三者のソフトウェアのライセンス文と著作権表示(\`NOTICES.md\` が一覧) |
| \`SHA256SUMS\` | 上の4つの SHA-256 |

## 作り方

- ソース: https://github.com/tsolsikke/seinas のコミット \`$commit\`(タグ \`$tag\`)
- 手順: \`tools/release-standalone.sh $tag\`(中で \`tools/build-musl.sh\` を使う。手順の説明は \`docs/release.md\` と \`docs/musl-static.md\`)
- Rust: $rust(ターゲットは x86_64-unknown-linux-musl、CPU の指定は無し。既定の x86-64、SSE2 まで)
- musl: 1.2.5(Rust に同梱のもの。ヘッダーはホストの musl-gcc の 1.2.4)
- pixman: 0.42.2、libxkbcommon: 1.6.0(ソースから musl 向けに静的ビルド。実行ファイルに実際に入っているものは、それぞれのアーカイブの \`NOTICES.md\` にある)

## ライセンス

- Seinas 自身のコードは MIT ライセンスです(アーカイブの中の \`LICENSE\`)。
- 第三者のソフトウェア(pixman、libxkbcommon、musl、Rust のクレート、Rust の標準ライブラリ、libunwind)は、それぞれのライセンスに従います。本文と著作権表示は、実行ファイルごとのアーカイブにあります。
NOTES

echo "できたもの($out):"
cat "$out/SHA256SUMS"
echo
echo "releaseを作るには、次を実行してください(タグは、このコミットに付きます):"
files=""
for asset in $assets; do
    files="$files $out/$asset"
done
echo "  gh release create $tag --target $commit --title \"Seinas $tag\" --notes-file $out/NOTES.md$files $out/SHA256SUMS"
