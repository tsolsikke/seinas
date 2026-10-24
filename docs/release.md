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
| release の説明文 | 何の実行ファイルか、作り方(コミット、Rust と musl のバージョン)、ライセンス。`NOTES.md` として作られ、`gh release create` に渡す |

実行ファイルは、リポジトリの `main` のソースから、変更を入れずに作ります。ZeikOS のための変更は入れません。

## タグの決め方

- タグは `v<バージョン>` で、バージョンは `Cargo.toml` の `[workspace.package]` の `version` です(例: `v0.1.0`)。
- release は `seinas-fbdev` だけを置きますが、タグはリポジトリ全体に付きます(ワークスペースのバージョンが1つなので、実行ファイルごとにバージョンを分けません)。
- 同じバージョンで作り直すときは、バージョンを上げてから切ります。切った後のタグは、付け替えません。
- バージョンを上げるときは、`Cargo.toml` の `version` を変え、`cargo build --locked` で `Cargo.lock` を合わせて、コミットしてから切ります。

## 手順

1. `main` を最新にして push し、作業ツリーに変更が無いことを確かめます(スクリプトも、作業ツリーと、HEAD が origin の `main` と同じことを確かめ、違えば止まります)。
2. アセットを作ります。musl 向けの成果物(C のライブラリを含む)を消してから作り直すので、数分かかります。

```bash
tools/release-fbdev.sh v0.1.0
```

   できるものは `target/release-assets/v0.1.0/` の下です。終わりに、SHA-256 と、release を作るコマンドが表示されます。

3. 再現可能なビルドになっていることを確かめます。もう一度 2 を実行し、`SHA256SUMS` が同じになることを見ます(下の「再現可能なビルド」)。
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

1. 作業ツリーに変更が無く、HEAD が origin の `main` と同じ(push 済み)であることを確かめ、コミットのハッシュを控える。release のタグは GitHub にあるコミットに付くので、push する前のコミットからは作らない(`git ls-remote` で読むだけで、GitHub には書かない)。
2. `target/x86_64-unknown-linux-musl` と `target/musl-libs` を消し、`tools/build-musl.sh` で作り直す(`docs/musl-static.md`)。
3. `tools/elf-report.sh` で、静的 PIE であることを確かめる。`tools/third-party.py --check` で、ライセンス文の置き場が最新であることを確かめる。
4. `tools/release-notices.py` で、`seinas-fbdev` に入る第三者のライセンス文を集め、アーカイブにする。
5. `SHA256SUMS` と `NOTES.md` を作る。

`tools/release-notices.py` は、次のものを集めます。

- Rust のクレート: `cargo tree -p seinas-fbdev -e normal,no-proc-macro --target x86_64-unknown-linux-musl` で、この実行ファイルに入るものだけを選びます(ワークスペース全体で解決すると、`seinas-fbdev` には入らない Wayland 系のクレートまで数えてしまうので、そうしません)。ライセンス文は `THIRD-PARTY/licenses/` と `licenses-upstream/` からコピーします。
- C のライブラリ: `THIRD-PARTY/c-libraries/` からコピーします。リンクの指定があっても、使われないものは実行ファイルに入らないので、実行ファイルのシンボル(`nm`)を見て、実際に入っているものだけを入れます。`seinas-fbdev` には pixman と musl が入り、libxkbcommon は入りません。
- Rust の標準ライブラリと libunwind: `THIRD-PARTY/rust-std/` のライセンス文(標準ライブラリの `COPYRIGHT-library.html` と、libunwind の `LICENSE.TXT`)をコピーします。`THIRD-PARTY/rust-std/RUSTC-VERSION` が今の `rustc -V` と違うか、ファイルが欠けていれば止まります(`THIRD-PARTY/rust-std/README.md`)。
- Seinas 自身の `LICENSE` も入れます。

アーカイブは、中身の並びと日時をそろえて作るので、同じ中身なら同じファイルになります。tar と gzip はパイプでつながず、tar の出力をファイルに書いてから gzip するので、どちらが失敗してもスクリプトは止まります(`/bin/sh` には pipefail が無いため。`tools/release-lib.sh`)。

## パスを残さないこと

`tools/build-musl.sh` は、Rust のソースのパスを `--remap-path-prefix` で置き換えます(リポジトリの置き場所は `/seinas`、Cargo の置き場所(`CARGO_HOME`、無ければ `~/.cargo`)は `/cargo`)。
`tools/build-musl-libs.sh` も、C のライブラリを `-ffile-prefix-map` で同じように作ります。
そのため、実行ファイルに、作った環境の絶対パス(`/home/…` など)は残りません。確かめるには、次のようにします。

```bash
strings -a target/x86_64-unknown-linux-musl/release/seinas-fbdev | grep -c /home/
```

0 であれば、残っていません(残っていてよいのは、`/seinas/…`、`/cargo/…`、Rust 自身が置き換えた `/rustc/<ハッシュ>/…` です)。

release v0.1.0 は、この置き換えを入れる前に作ったものです(実行ファイルに、作った環境のパスが入っています)。
v0.1.0 は切り直さず、そのままにしています。次に release を切るときから、置き換えを入れた形で作ります。

## 再現可能なビルド

`tools/build-musl.sh` は、同じソース、同じ Rust、同じ手順で作れば、同じ実行ファイルになります(2026-10-06 に、musl 向けの成果物を消してから2回作り、SHA-256 が同じであることを確かめました)。

ただし、次が変わると、実行ファイルも変わります。

- Rust と C の、パスの置き換え(上の節)。置き換えているので、リポジトリや Cargo の置き場所が違っても、同じ実行ファイルになります(それ以外の置き場所、たとえば Rust のツールチェーンの置き場所は、Rust 自身が置き換えています)。
- Rust のバージョン(`rust-toolchain.toml`)、Cargo.lock、pixman のバージョン(`tools/build-musl-libs.sh`)。
- ホストの musl-gcc(Ubuntu の musl-tools)のバージョン。pixman のコンパイルに使うヘッダーが変わるためです。

別の環境で同じものを作りたいときは、同じバージョンをそろえてください。

## 確かめ方(release を取ってきた側)

```bash
sha256sum --check SHA256SUMS
```

```bash
./seinas-fbdev --dry-run
```

`--dry-run` は、装置を開かずに終わります。実際の画面へ present する動かし方は `docs/fbdev.md` にあります。

## seinas-standalone の release

ZeikOS の上で Seinas のコンポジタとタスクバーを動かすために、`seinas-standalone` と `seinas-taskbar`(どちらも musl 向けの静的 PIE)も、同じ形の release として配ります(タスクバーは v0.3.0 から。v0.2.0 はコンポジタだけ)。手順は、上の seinas-fbdev と同じで、スクリプトが `tools/release-standalone.sh` に替わります。

| ファイル | 中身 |
| --- | --- |
| `seinas-standalone` | コンポジタの実行ファイル(x86-64 の Linux、musl、静的 PIE)。`tools/build-musl.sh` の出力と同じもので、strip はしない(CI の SHA-256 と同じバイトにするため) |
| `seinas-standalone-<タグ>-third-party.tar.gz` | コンポジタに入っている第三者のソフトウェアのライセンス文と著作権表示。中の `NOTICES.md` が一覧 |
| `seinas-taskbar` | タスクバーの実行ファイル(同じく、strip はしない) |
| `seinas-taskbar-<タグ>-third-party.tar.gz` | タスクバーに入っている第三者のソフトウェアのライセンス文と著作権表示。中の `NOTICES.md` が一覧 |
| `SHA256SUMS` | 上の4つの SHA-256 |

```bash
tools/release-standalone.sh v0.3.0
```

- 第三者のライセンス文は、実行ファイルごとに集めます(`tools/release-notices.py`)。タスクバーには、pixman と libxkbcommon は入りません(Rust のクレートと musl、Rust の標準ライブラリと libunwind)。
- release の Title は `Seinas <タグ>` です(v0.2.0 は `seinas-standalone v0.2.0`)。

- タグはリポジトリ全体に付くので、seinas-fbdev と seinas-standalone を同じバージョンで出すときは、同じタグの release に両方を置くこともできます。別々に出すときは、バージョンを上げてから切ります(上の「タグの決め方」)。
- コンポジタは、最初の 1 枚を画面へ present し終えたときに、標準エラーのログへ `presented the first frame (<幅>x<高さ>)` を 1 回だけ出します(target は `seinas_standalone`。ZeikOS の側の確かめが、この行を探す)。文言を変えるときは、ZeikOS の側に知らせます。
- ZeikOS の上での起動のしかたと、置くファイル(名前・大きさ・SHA-256)は `docs/standalone.md` の「ZeikOS の上で起動する」にあります。
