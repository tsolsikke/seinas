# 正解データ(golden)

このディレクトリと、`crates/seinas-render/tests/golden/`・`crates/seinas-frontend/tests/golden/`・`crates/seinas-fbdev/tests/golden/` にあるのは、置き換える前の出力を保存した正解データです。各 crate の `tests/golden.rs` が、今の出力と比べます。

- 作り直しは `tools/update-golden.sh` で行います。手で編集しないでください。
- 決まり(比べ方、作り直すときのコミットの書き方、バージョン上げの判断)は `docs/golden.md` にあります。
- 形式:`*.ppm` は PPM(P6、RGB)、`*.pgm` は PGM(P5、グレーのカバレッジ)、`*.bin` は fbdev のバッファのバイト列、`cases.txt` はテキスト描画の戻り値の一覧です。

テキストを描いた正解データ(`crates/seinas-text/tests/golden/` と、`crates/seinas-frontend/tests/golden/` の c1〜c5)は、次のフォントで描いたものです。

| フォント | バージョン | ライセンス |
| --- | --- | --- |
| BIZ UDPゴシック Regular | 1.051 | SIL Open Font License 1.1 |
| GNU Unifont JP | 18.0.01 | SIL Open Font License 1.1(二重ライセンスのうち、こちらを選ぶ) |

描いた結果のピクセルは、フォントそのもの(Font Software)ではなく、フォントを使って作ったものに当たると判断しています(`docs/golden.md` の「ライセンス」)。保存しているのは短い文字列だけです。
