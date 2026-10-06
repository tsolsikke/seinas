#!/bin/sh
# seinas-fbdev(musl向けの静的PIE)を、GitHubのreleaseに置くための一式を作る。
#
# 使い方:
#     tools/release-fbdev.sh <タグ>        例: tools/release-fbdev.sh v0.1.0
#
# できるもの(target/release-assets/<タグ>/ の下):
#     seinas-fbdev                                 実行ファイル(musl向けの静的PIE)
#     seinas-fbdev-<タグ>-third-party.tar.gz       実行ファイルに入っている第三者のライセンス文と著作権表示
#     SHA256SUMS                                   上の2つのSHA-256
#     NOTES.md                                     releaseの説明文(作り方、版、ライセンス)
#
# 作った後、releaseを作るgh のコマンドを表示する(実行はしない)。手順は docs/release.md にある。
#
# 要るもの: tools/build-musl.sh が要るもの(musl-gcc、meson、ninja、bison、curl)、python3、nm、tar、gzip
set -eu
export LC_ALL=C

tag=${1:?usage: tools/release-fbdev.sh <tag>}
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# 作業ツリーに変更が残っていると、どのソースから作ったかが分からなくなる。
if [ -n "$(git status --porcelain)" ]; then
    echo "作業ツリーに変更があります。コミットするか、元に戻してから作ってください。" >&2
    exit 1
fi
commit=$(git rev-parse HEAD)

# いつも同じ条件で作るため、musl向けの成果物を消してから作り直す(Cのライブラリも作り直す)。
rm -rf target/x86_64-unknown-linux-musl target/musl-libs
tools/build-musl.sh > /dev/null
binary=target/x86_64-unknown-linux-musl/release/seinas-fbdev
tools/elf-report.sh "$binary" > /dev/null
python3 tools/third-party.py --check > /dev/null

out=target/release-assets/$tag
rm -rf "$out"
mkdir -p "$out"
cp "$binary" "$out/seinas-fbdev"

# 第三者のライセンス文を、1つのアーカイブにまとめる。中身の並びと日時をそろえ、同じ中身なら同じアーカイブになるようにする。
notices=seinas-fbdev-$tag-third-party
python3 tools/release-notices.py seinas-fbdev "$binary" "$out/$notices" > /dev/null
tar -C "$out" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@0" -cf - "$notices" | gzip -n > "$out/$notices.tar.gz"
rm -r "$out/$notices"

(cd "$out" && sha256sum seinas-fbdev "$notices.tar.gz" > SHA256SUMS)

rust=$(rustc -V)
cat > "$out/NOTES.md" <<NOTES
# seinas-fbdev $tag

ZeikOS の画面に Seinas の描いた絵を出すための、\`seinas-fbdev\` の実行ファイルです。
Linux の fbdev(\`/dev/fb0\`)へ、共通の描画で作った絵を出します。Wayland の受け口は含みません。
x86-64 の Linux 向けに、musl で静的にリンクした PIE で、共有ライブラリに依存しません。
ZeikOS 側の仕組みに合わせた変更は入れていません(Linux のプログラムをそのまま動かします)。

## 置いてあるもの

| ファイル | 中身 |
| --- | --- |
| \`seinas-fbdev\` | 実行ファイル(x86-64、musl、静的 PIE) |
| \`$notices.tar.gz\` | この実行ファイルに入っている第三者のソフトウェアのライセンス文と著作権表示(\`NOTICES.md\` が一覧) |
| \`SHA256SUMS\` | 上の2つの SHA-256 |

## 作り方

- ソース: https://github.com/tsolsikke/seinas のコミット \`$commit\`(タグ \`$tag\`)
- 手順: \`tools/release-fbdev.sh $tag\`(中で \`tools/build-musl.sh\` を使う。手順の説明は \`docs/release.md\` と \`docs/musl-static.md\`)
- Rust: $rust(ターゲットは x86_64-unknown-linux-musl、CPU の指定は無し。既定の x86-64、SSE2 まで)
- musl: 1.2.5(Rust に同梱のもの。ヘッダーはホストの musl-gcc の 1.2.4)
- pixman: 0.42.2(ソースから musl 向けに静的ビルド)

## ライセンス

- Seinas 自身のコードは MIT ライセンスです(アーカイブの中の \`LICENSE\`)。
- 第三者のソフトウェア(pixman、musl、Rust のクレート、Rust の標準ライブラリ)は、それぞれのライセンスに従います。本文と著作権表示は \`$notices.tar.gz\` にあります。
NOTES

echo "できたもの($out):"
cat "$out/SHA256SUMS"
echo
echo "releaseを作るには、次を実行してください(タグは、このコミットに付きます):"
echo "  gh release create $tag --target $commit --title \"seinas-fbdev $tag\" --notes-file $out/NOTES.md $out/seinas-fbdev $out/$notices.tar.gz $out/SHA256SUMS"
