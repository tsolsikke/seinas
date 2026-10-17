# 第三者のライセンス文と著作権表示

Seinasが使う、ほかの人が作ったソフトウェアのライセンス文と著作権表示を、ここにまとめています。
Seinas自身のライセンス(MIT)は、リポジトリの先頭の `LICENSE` にあります。

## 置いてあるもの

| 場所 | 中身 |
| --- | --- |
| `rust-crates.md` | 使っているRustクレートの一覧(名前、版、ライセンス、使われ方) |
| `licenses/` | 各クレートの配布物に入っていたライセンス文を、そのまま写したもの |
| `licenses-upstream/` | 配布物にライセンス文が入っていないクレートについて、上流のリポジトリから取ってきたもの |
| `c-libraries/` | 静的にリンクするCのライブラリのライセンス文と著作権表示 |
| `fonts/` | 既定のフォントのライセンス文と著作権表示(フォントそのものは、リポジトリに入れていない) |

`rust-crates.md` と `licenses/` は、`tools/third-party.py` が Cargo.lock をもとに作ります。
依存を変えたら、次のように作り直してください。CIでも、古くなっていないかを確かめています。

```bash
python3 tools/third-party.py
```

## 配布物にライセンス文が入っていないクレート

次のクレートは、crates.ioの配布物にライセンス文のファイルがありません。

| クレート | ライセンス | 対応 |
| --- | --- | --- |
| drm-fourcc 2.2.0 | MIT | 上流の `main` ブランチの `LICENSE` を `licenses-upstream/` に置いた(2026-10-04に取得) |
| profiling 1.0.18、profiling-procmacros 1.0.18 | MIT OR Apache-2.0 | 上流の `master` ブランチの `LICENSE-MIT` と `LICENSE-APACHE` を `licenses-upstream/` に置いた(2026-10-04に取得) |
| harfrust 0.5.2 | MIT | 上流のリポジトリ(<https://github.com/harfbuzz/harfrust>)の、この版のもとになったコミット(efdae3142ab76a2f1524d72cff9e3dfdc5afd7ca)の `LICENSE` を `licenses-upstream/` に置いた(2026-10-05に取得) |
| pixman 0.2.1、pixman-sys 0.1.0 | MIT | 上流のリポジトリ(<https://github.com/cmeissl/pixman-rs>)にもライセンス文のファイルが無い。`Cargo.toml` にMITと書かれていて、作者はChristian Meisslさん |

## ライセンスを選んで使うクレート

複数のライセンスから選べるクレートは、MIT、Apache-2.0、Zlibのどれかで使います。GPLを選ぶものは、ありません。

| クレート | 書かれているライセンス | 選ぶもの |
| --- | --- | --- |
| self_cell 1.3.0 | Apache-2.0 OR GPL-2.0-only | Apache-2.0 |

## フォント

Seinas が自分で描く文字(タイトルバーなど)のために、次のフォントを使います。
フォントは、バイナリに埋め込まず、リポジトリにも入れていません(テストのための小さな TTC だけは入れています。下の「テストのための TTC」)。`tools/fetch-fonts.sh` がバージョンと SHA-256 を固定して取得し、Seinas は実行のときにファイルとして読みます。
musl の静的ビルド(`tools/build-musl.sh`)の配布物 `target/dist/seinas-musl/` には、`fonts/` の下にフォントのファイルが、`THIRD-PARTY/fonts/` の下にライセンス文と著作権表示が入ります。

この節が、フォントの出どころとライセンスの記録の置き場所です。取得元の URL と SHA-256 は `tools/fetch-fonts.sh` にあり、ほかの文書はここと `tools/fetch-fonts.sh` を指します。

| フォント(上流の名前) | ファイル | バージョン | 使われ方 | 加工 | ライセンス | ライセンス文と著作権表示 |
| --- | --- | --- | --- | --- | --- | --- |
| BIZ UDPGothic Regular | `BIZUDPGothic-Regular.ttf` | 1.051 | UI の文字(タイトルバーなど) | なし | SIL Open Font License 1.1 | `fonts/biz-udgothic-1.051/`(`OFL.txt`、`AUTHORS.txt`、`CONTRIBUTORS.txt`) |
| BIZ UDGothic Regular | `BIZUDGothic-Regular.ttf` | 1.051 | 端末・コードの文字(端末を作るときに使う。今のコードは読まない) | なし | SIL Open Font License 1.1 | 同上 |
| GNU Unifont JP | `unifont_jp-18.0.01.otf` | 18.0.01 | フォールバック(上のフォントに無い文字) | なし | SIL Open Font License 1.1(二重ライセンスのうち、こちらを選ぶ。下の注記) | `fonts/unifont-18.0.01/`(`OFL-1.1.txt`、`COPYING`) |

### BIZ UDPGothic と BIZ UDGothic

- 取得元:<https://github.com/googlefonts/morisawa-biz-ud-gothic> のタグ `v1.051`(コミット 18934af56b9c003ca58c54bffbf226848cb11032)。Windows に付属するファイルではなく、OFL で公開されているものです。
- バージョン:フォントの中の表記は `Version 1.051`。
- 予約されたフォント名(Reserved Font Name)の宣言は、ありません。
- 著作権表示(上流のものを、そのまま写しています):
  - `OFL.txt` の 1 行目:`Copyright 2022 The BIZ UDGothic Project Authors (https://github.com/googlefonts/morisawa-biz-ud-mincho)`
  - フォントの中の表記:`Copyright 2022 The BIZ UDGothic Project Authors (https://github.com/googlefonts/morisawa-biz-ud-gothic)`
  - `OFL.txt` とフォントの中とで、URL の末尾が違います(`mincho` と `gothic`)。上流のファイルが、そうなっています。
  - `AUTHORS.txt` に書かれた著作者:`Morisawa Inc.`、`Type Network`
  - フォントの中の商標の表記:`BIZ UDPGothic is a trademark of Morisawa Inc.`(BIZ UDGothic のほうは、名前の部分が `BIZ UDGothic`)

### テストのための TTC

- `crates/seinas-text/tests/data/two-faces.ttc`(7,696 バイト)は、BIZ UDPGothic と BIZ UDGothic の Regular(1.051)から、fontTools で数文字だけを残し、face が 2 つの TTC にしたものです。テストだけが使い、配布物には入りません。
- OFL 1.1 の変えたバージョン(Modified Version)として、同じ OFL 1.1 で配ります。予約されたフォント名の宣言が無いので、名前は元のままです。ライセンス文と著作権表示は、隣の `OFL.txt`(上流のもの)。作り方は、隣の `README.md` と `tools/make-test-ttc.sh`。

### GNU Unifont JP

- 取得元:GNU の配布元 <https://ftp.gnu.org/gnu/unifont/unifont-18.0.01/>。同じファイルを、もう 1 つの配布元(<https://unifoundry.com/pub/unifont/unifont-18.0.01/font-builds/>)からも取り、SHA-256 が同じであることを確かめました(2026-10-05)。配布元の署名(`.sig`。キー 95D2E9AB8740D8046387FD151A09227B1F435A33)は、キーを取ってこられず、確かめていません。
- バージョン:18.0.01(2026-09-16 に公開。Unicode 18.0.0 に合わせたもの)。フォントの中の名前は `Unifont-JP`。
- 二重ライセンスの注記:上流は、フォントのファイルを「SIL Open Font License 1.1」と「GNU GPL 2 以降(フォントの埋め込みの例外つき)」の二重ライセンスで出しています。上流の `COPYING` と、フォントの中のライセンスの表記(「Dual license: SIL Open Font License version 1.1, and GNU GPL version 2 or later with the GNU Font Embedding Exception.」)の両方に書かれています(13.0.04 から)。Seinas は、OFL 1.1 を選んで使います。`fonts/unifont-18.0.01/` の 2 つのファイルは、上流のソースの配布物(`unifont-18.0.01.tar.gz`)に入っているもので、二重ライセンスの表示をそのまま残しています。
- 予約されたフォント名の宣言は、ありません。
- 著作権表示(フォントの中の表記):`Copyright © 1998-2026 Roman Czyborra, Paul Hardy, Qianqian Fang, Andrew Miller, Johnnie Weaver, David Corbett, Ælla Chiana Moskopp, Rebecca Bettencourt, Ho-Seok Ee, et al.`

フォントのファイルを Seinas と一緒に配るときは、`fonts/` の下のライセンス文と著作権表示も一緒に配ってください。Seinas 自身のコードの MIT ライセンスは、フォントには当てはまりません。

## Cのライブラリ

Rustクレートのほかに、次のCのライブラリをリンクします。

| ライブラリ | 版 | ライセンス | 使われ方 | ライセンス文 |
| --- | --- | --- | --- | --- |
| pixman(libpixman-1) | 0.42.2 | MIT | ピクセルの合成 | `c-libraries/pixman-0.42.2/COPYING` |
| libxkbcommon | 1.6.0 | MIT(X11系の表示を含む) | キーボードの配列。Smithayがリンクの指定を付けるが、使わない実行ファイルには入らない(seinasとseinas-standaloneには入る。seinas-fbdevには入らない) | `c-libraries/libxkbcommon-1.6.0/LICENSE` |
| musl | 1.2.5 | MIT | Cの標準ライブラリ(Rustのmusl向けターゲットに同梱のもの) | `c-libraries/musl-1.2.5/COPYRIGHT` |

版は、musl向けの静的ビルド(`tools/build-musl.sh`)で実行ファイルに入るものです。
リンクの指定があっても、使われないライブラリは、リンカーが落とすので実行ファイルに入りません。実際に入っているかは、実行ファイルのシンボル(`nm` で `xkb_` や `pixman_` で始まる関数があるか)で分かります。
1つの実行ファイルに入っているものだけを集めるには、`tools/release-notices.py` を使います。
pixmanとlibxkbcommonのライセンス文は、ビルドに使うソース(`tools/build-musl-libs.sh` が取る版)に入っているファイルと同じものです。
muslのライセンス文は、musl 1.2.5のソースの `COPYRIGHT` です。

ホスト(glibc)向けのビルドでは、OSに入っている共有ライブラリを実行時に使うので、これらは実行ファイルに入りません。

## Rustの標準ライブラリ

Rustでビルドした実行ファイルには、Rustの標準ライブラリ(MIT OR Apache-2.0)が入ります。
musl向けの静的ビルドでは、Rustに同梱のlibunwind(Apache-2.0 WITH LLVM-exception)も入ります。

## 配布物に入れるもの

静的にリンクした実行ファイルを配るときは、リポジトリの先頭の `LICENSE` と、この `THIRD-PARTY/` をまるごと一緒に配ってください。
`tools/build-musl.sh` は、実行ファイルとこれらをまとめたものを `target/dist/seinas-musl/` に作ります。
