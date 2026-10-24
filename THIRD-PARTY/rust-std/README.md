# Rustの標準ライブラリとlibunwindのライセンス文

Rustでビルドした実行ファイルに入る、Rustの標準ライブラリと、musl向けの静的ビルドに入るlibunwindの、ライセンス文と著作権表示です。
`tools/release-notices.py` が、release のアーカイブに、このディレクトリをそのまま入れます。

| ファイル | 中身 | 取った所 |
| --- | --- | --- |
| `COPYRIGHT-library.html` | Rustの標準ライブラリの著作権表示とライセンス文(MIT OR Apache-2.0。標準ライブラリが使うクレートの分を含む) | Rustのツールチェーンの `share/doc/rust/COPYRIGHT-library.html`(`RUSTC-VERSION` のもの) |
| `libunwind-LICENSE.TXT` | libunwindのライセンス文(Apache-2.0 WITH LLVM-exception と、古いコードの University of Illinois/NCSA) | rust-lang/llvm-project のコミット `7738295178045041669876bf32b0543ec8319a5c` の `libunwind/LICENSE.TXT` |
| `RUSTC-VERSION` | 上の 2 つを取った Rust のバージョン(`rustc -V`) | — |

- 2 つの本文は、上流のファイルを、中身を変えずに置いています(行末の空白も残す。`.gitattributes` で、空白の確かめから外している)。
- libunwindは、Rustのmusl向けターゲットに同梱のもの(`lib/rustlib/x86_64-unknown-linux-musl/lib/self-contained/libunwind.a`)です。Rustのツールチェーンの `share/doc/rust/COPYRIGHT.html` は、これを作った `src/llvm-project` を次のように書いています。
  - License: Apache-2.0 WITH LLVM-exception AND NCSA
  - Copyright: 2003-2019 University of Illinois at Urbana-Champaign
  - Copyright: 2003-2019 by the contributors listed in CREDITS.TXT(https://github.com/rust-lang/llvm-project/blob/7738295178045041669876bf32b0543ec8319a5c/llvm/CREDITS.TXT)
  - Copyright: 2010 Apple Inc
- Rustのバージョン(`rust-toolchain.toml`)を上げたときは、このディレクトリも作り直してください。`tools/release-notices.py` は、`RUSTC-VERSION` が今の `rustc -V` と違えば止まります。
  - `COPYRIGHT-library.html` は、新しいツールチェーンの `share/doc/rust/` からコピーします。
  - `libunwind-LICENSE.TXT` は、新しいツールチェーンの `COPYRIGHT.html` の `src/llvm-project` の行にあるコミットから取り直します。
