# seinas-fbdev の release の切り方

ZeikOS の側で Seinas の描いた絵を画面に出すために、`seinas-fbdev`(musl 向けの静的 PIE)を、GitHub の release として配っています。
ZeikOS の CI は pixman の musl 向けのライブラリを作れないので、Seinas の側でビルドしたものを、release から取ってくる形です。
この文書は、次の release を切るときの手順です。

## 置くもの

| ファイル | 中身 |
| --- | --- |
| `seinas-fbdev` | 実行ファイル(x86-64 の Linux、musl、静的 PIE。共有ライブラリに依存しない) |
| `seinas-fbdev-<タグ>-third-party.tar.gz` | この実行ファイルに入っている第三者のソフトウェアのライセンス文と著作権表示。中の `NOTICES.md` が一覧 |
| `SHA256SUMS` | 上の2つの SHA-256(`sha256sum --check SHA256SUMS` で確かめられる) |
| release の説明文 | 何の実行ファイルか、作り方(コミット、Rust と musl の版)、ライセンス。`NOTES.md` として作られ、`gh release create` に渡す |

実行ファイルは、リポジトリの `main` のソースから、変更を入れずに作ります。ZeikOS のための変更は入れません。

## タグの決め方

- タグは `v<版>` で、版は `Cargo.toml` の `[workspace.package]` の `version` です(例: `v0.1.0`)。
- release は `seinas-fbdev` だけを置きますが、タグはリポジトリ全体に付きます(ワークスペースの版が1つなので、実行ファイルごとに版を分けません)。
- 同じ版で作り直すときは、版を上げてから切ります。切った後のタグは、付け替えません。
- 版を上げるときは、`Cargo.toml` の `version` を変え、`cargo build --locked` で `Cargo.lock` を合わせて、コミットしてから切ります。

## 手順

1. `main` を最新にし、作業ツリーに変更が無いことを確かめます(スクリプトも確かめます)。
2. 一式を作ります。musl 向けの成果物(C のライブラリを含む)を消してから作り直すので、数分かかります。

```bash
tools/release-fbdev.sh v0.1.0
```

   できるものは `target/release-assets/v0.1.0/` の下です。終わりに、SHA-256 と、release を作るコマンドが表示されます。

3. 決定的に作れていることを確かめます。もう一度 2 を実行し、`SHA256SUMS` が同じになることを見ます(下の「決定的に作れること」)。
4. 表示されたコマンドで release を作ります。タグは、そのコミットに付きます。

```bash
gh release create v0.1.0 --target <コミット> --title "seinas-fbdev v0.1.0" --notes-file target/release-assets/v0.1.0/NOTES.md target/release-assets/v0.1.0/seinas-fbdev target/release-assets/v0.1.0/seinas-fbdev-v0.1.0-third-party.tar.gz target/release-assets/v0.1.0/SHA256SUMS
```

5. できた release のページで、置いたファイルと SHA-256 を確かめます。

```bash
gh release view v0.1.0
```

## スクリプトがすること

`tools/release-fbdev.sh` は、次を順に行います。

1. 作業ツリーに変更が無いことを確かめ、コミットのハッシュを控える。
2. `target/x86_64-unknown-linux-musl` と `target/musl-libs` を消し、`tools/build-musl.sh` で作り直す(`docs/musl-static.md`)。
3. `tools/elf-report.sh` で、静的 PIE であることを確かめる。`tools/third-party.py --check` で、ライセンス文の置き場が最新であることを確かめる。
4. `tools/release-notices.py` で、`seinas-fbdev` に入る第三者のライセンス文を集め、アーカイブにする。
5. `SHA256SUMS` と `NOTES.md` を作る。

`tools/release-notices.py` は、次のものを集めます。

- Rust のクレート: `cargo tree -p seinas-fbdev -e normal,no-proc-macro --target x86_64-unknown-linux-musl` で、この実行ファイルに入るものだけを選びます(ワークスペース全体で解決すると、`seinas-fbdev` には入らない Wayland 系のクレートまで数えてしまうので、そうしません)。ライセンス文は `THIRD-PARTY/licenses/` と `licenses-upstream/` から写します。
- C のライブラリ: `THIRD-PARTY/c-libraries/` から写します。リンクの指定があっても、使われないものは実行ファイルに入らないので、実行ファイルのシンボル(`nm`)を見て、実際に入っているものだけを入れます。`seinas-fbdev` には pixman と musl が入り、libxkbcommon は入りません。
- Rust の標準ライブラリと libunwind は、一覧に書きます。
- Seinas 自身の `LICENSE` も入れます。

アーカイブは、中身の並びと日時をそろえて作るので、同じ中身なら同じファイルになります。

## 道の名前を残さないこと

`tools/build-musl.sh` は、Rust のソースの道の名前を `--remap-path-prefix` で置き換えます(リポジトリの置き場所は `/seinas`、Cargo の置き場所(`CARGO_HOME`、無ければ `~/.cargo`)は `/cargo`)。
`tools/build-musl-libs.sh` も、C のライブラリを `-ffile-prefix-map` で同じように作ります。
そのため、実行ファイルに、作った環境の絶対パス(`/home/…` など)は残りません。確かめるには、次のようにします。

```bash
strings -a target/x86_64-unknown-linux-musl/release/seinas-fbdev | grep -c /home/
```

0 であれば、残っていません(残っていてよいのは、`/seinas/…`、`/cargo/…`、Rust 自身が置き換えた `/rustc/<ハッシュ>/…` です)。

release v0.1.0 は、この置き換えを入れる前に作ったものです(実行ファイルに、作った環境の道の名前が入っています)。
v0.1.0 は切り直さず、そのままにしています。次に release を切るときから、置き換えを入れた形で作ります。

## 決定的に作れること

`tools/build-musl.sh` は、同じソース、同じ Rust、同じ手順で作れば、同じ実行ファイルになります(2026-10-06 に、musl 向けの成果物を消してから2回作り、SHA-256 が同じであることを確かめました)。

ただし、次が変わると、実行ファイルも変わります。

- Rust と C の、道の名前の置き換え(上の節)。置き換えているので、リポジトリや Cargo の置き場所が違っても、同じ実行ファイルになります(それ以外の置き場所、たとえば Rust のツールチェーンの置き場所は、Rust 自身が置き換えています)。
- Rust の版(`rust-toolchain.toml`)、Cargo.lock、pixman の版(`tools/build-musl-libs.sh`)。
- ホストの musl-gcc(Ubuntu の musl-tools)の版。pixman のコンパイルに使うヘッダーが変わるためです。

別の環境で同じものを作りたいときは、同じ版をそろえてください。

## 確かめ方(release を取ってきた側)

```bash
sha256sum --check SHA256SUMS
```

```bash
./seinas-fbdev --dry-run
```

`--dry-run` は、装置を開かずに終わります。実際の画面へ出す動かし方は `docs/fbdev.md` にあります。
