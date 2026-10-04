# 第三者のライセンス文と著作権表示

Seinasが使う、ほかの人が作ったソフトウェアのライセンス文と著作権表示を、ここにまとめています。
Seinas自身のライセンス(MIT)は、リポジトリの先頭の `LICENSE` にあります。

## 置いてあるもの

| 場所 | 中身 |
| --- | --- |
| `rust-crates.md` | 使っているRustクレートの一覧(名前、版、ライセンス、使われ方) |
| `licenses/` | 各クレートの配布物に入っていたライセンス文を、そのまま写したもの |
| `licenses-upstream/` | 配布物にライセンス文が入っていないクレートについて、上流のリポジトリから取ってきたもの |

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

| ライブラリ | ライセンス | 使われ方 |
| --- | --- | --- |
| pixman(libpixman-1) | MIT | 画素の合成 |
| libxkbcommon | MIT(X11系の表示を含む) | キーボードの配列(Smithayが必ずリンクする) |

ホスト(glibc)向けのビルドでは、OSに入っている共有ライブラリを実行時に使うので、Seinasの配布物には含まれません。
静的にリンクして配る場合は、それぞれのライセンス文を配布物に同梱する必要があります。

## Rustの標準ライブラリ

Rustでビルドした実行ファイルには、Rustの標準ライブラリ(MIT OR Apache-2.0)が入ります。
