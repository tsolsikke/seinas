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
| pixman 0.2.1、pixman-sys 0.1.0 | MIT | 上流のリポジトリ(<https://github.com/cmeissl/pixman-rs>)にもライセンス文のファイルが無い。`Cargo.toml` にMITと書かれていて、作者はChristian Meisslさん |

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
