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

Seinasは、ウィンドウの題名などを描くのに、次のフォントを使います。
フォントは、実行ファイルに埋め込まず、リポジトリにも入れていません。`tools/fetch-fonts.sh` が、版とSHA-256を固定して取得し、Seinasは実行のときにファイルとして読みます。

| フォント | 版 | 使われ方 | ライセンス | ライセンス文と著作権表示 |
| --- | --- | --- | --- | --- |
| BIZ UDPゴシック Regular(`BIZUDPGothic-Regular.ttf`) | 1.051 | 題名などのUIの文字 | SIL Open Font License 1.1 | `fonts/biz-udgothic-1.051/`(`OFL.txt`、`AUTHORS.txt`、`CONTRIBUTORS.txt`) |
| GNU Unifont JP(`unifont_jp-18.0.01.otf`) | 18.0.01 | 控え(上のフォントに無い文字) | SIL Open Font License 1.1(二重ライセンスのうち、こちらを選ぶ) | `fonts/unifont-18.0.01/`(`OFL-1.1.txt`、`COPYING`) |

- BIZ UDPゴシックの取得元は、<https://github.com/googlefonts/morisawa-biz-ud-gothic> のタグ `v1.051` です。著作権表示は「Copyright 2022 The BIZ UDGothic Project Authors」で、予約されたフォント名の宣言はありません。
- GNU Unifont JPの取得元は、GNUの配布元 <https://ftp.gnu.org/gnu/unifont/unifont-18.0.01/> です。フォントのファイルは、「SIL Open Font License 1.1」と「GNU GPL 2以降(フォントの埋め込みの例外つき)」の二重ライセンスです(上流の `COPYING` と、フォントの中のライセンスの表記に書かれています)。SeinasはOFL 1.1を選びます。`fonts/unifont-18.0.01/` の2つのファイルは、上流のソースの配布物(`unifont-18.0.01.tar.gz`)に入っているものです。
  フォントの中の著作権表示は「Copyright © 1998-2026 Roman Czyborra, Paul Hardy, Qianqian Fang, Andrew Miller, Johnnie Weaver, David Corbett, Ælla Chiana Moskopp, Rebecca Bettencourt, Ho-Seok Ee, et al.」です。
- どちらのフォントも、手を加えずに使います。
- `tools/fetch-fonts.sh` が取得する BIZ UDゴシック Regular(`BIZUDGothic-Regular.ttf`。端末・コード用)は、BIZ UDPゴシックと同じ配布物のもので、ライセンスも同じです。今のSeinasは、まだ使っていません。

フォントのファイルをSeinasと一緒に配るときは、`fonts/` の下のライセンス文と著作権表示も一緒に配ってください。

## Cのライブラリ

Rustクレートのほかに、次のCのライブラリをリンクします。

| ライブラリ | 版 | ライセンス | 使われ方 | ライセンス文 |
| --- | --- | --- | --- | --- |
| pixman(libpixman-1) | 0.42.2 | MIT | 画素の合成 | `c-libraries/pixman-0.42.2/COPYING` |
| libxkbcommon | 1.6.0 | MIT(X11系の表示を含む) | キーボードの配列(Smithayが必ずリンクする) | `c-libraries/libxkbcommon-1.6.0/LICENSE` |
| musl | 1.2.5 | MIT | Cの標準ライブラリ(Rustのmusl向けターゲットに同梱のもの) | `c-libraries/musl-1.2.5/COPYRIGHT` |

版は、musl向けの静的ビルド(`tools/build-musl.sh`)で実行ファイルに入るものです。
pixmanとlibxkbcommonのライセンス文は、ビルドに使うソース(`tools/build-musl-libs.sh` が取る版)に入っているファイルと同じものです。
muslのライセンス文は、musl 1.2.5のソースの `COPYRIGHT` です。

ホスト(glibc)向けのビルドでは、OSに入っている共有ライブラリを実行時に使うので、これらは実行ファイルに入りません。

## Rustの標準ライブラリ

Rustでビルドした実行ファイルには、Rustの標準ライブラリ(MIT OR Apache-2.0)が入ります。
musl向けの静的ビルドでは、Rustに同梱のlibunwind(Apache-2.0 WITH LLVM-exception)も入ります。

## 配布物に入れるもの

静的にリンクした実行ファイルを配るときは、リポジトリの先頭の `LICENSE` と、この `THIRD-PARTY/` をまるごと一緒に配ってください。
`tools/build-musl.sh` は、実行ファイルとこれらをまとめたものを `target/dist/seinas-musl/` に作ります。
