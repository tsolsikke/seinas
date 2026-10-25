# 壁紙(seinas-wallpaper)

Seinas の壁紙です。タスクバーと同じく、コンポジタとは別のプログラム(シェルの部品)で、layer-shell の background の層に、出力いっぱいの面を出し、色か画像(PNG)を描きます(`docs/adr/0002-shell-components.md`)。コンポジタは、このプログラムを起こしません。起こすのは、利用者か、起動の仕組みです。

## 動かし方

シェルの部品のためのソケットへつなぎます。

```bash
WAYLAND_DISPLAY=seinas-0-shell cargo run --locked -p seinas-wallpaper
```

| 引数 | 意味 |
| --- | --- |
| `--config PATH` | 設定のファイル。無ければ `$XDG_CONFIG_HOME/seinas/wallpaper.conf`(`XDG_CONFIG_HOME` が無ければ `~/.config/seinas/wallpaper.conf`) |

| 環境変数 | 意味 |
| --- | --- |
| `SEINAS_WALLPAPERS` | 同梱の壁紙の置き場(絶対パス)。下の「同梱の壁紙」の探す順の、いちばん先 |
| `XDG_DATA_HOME`・`XDG_DATA_DIRS` | 同梱の壁紙を探す、データの置き場(XDG Base Directory の決まり) |

- ふだんのソケット(`seinas-0`)につなぐと、layer-shell が見えないので、理由を出して終わります。
- 設定のファイルが無いときは、同梱の壁紙を出します(下の「同梱の壁紙」)。同梱の壁紙も見つからなければ、コンポジタの背景と同じ色(`191e28`)だけを出します。そのときは、壁紙をつないでも、見た目は変わりません。
- 設定のファイルがあれば、その設定に従います(`image` を書いていなければ、色だけ。同梱の壁紙は使わない)。

## 設定のファイル

形式は、フォントの設定(`docs/adr/0003-font-selection.md` の決定 6)と同じ、1 行に `キー = 値` の小さな形式です。読み方は、`crates/seinas-config` を共有しています。`#` から行の終わりまでは、コメントです。

```text
# Seinas の壁紙の設定
color = 1e3a5f
image = pictures/sky.png
mode = fill
```

| キー | 意味 | 既定 |
| --- | --- | --- |
| `color` | 色。16 進の 6 桁(`RRGGBB`)。画像が無いときは全体を、画像があるときは余白と透けた所を、この色にする | `191e28`(コンポジタの背景の色) |
| `image` | 画像(PNG)のパス。相対パスは、設定のファイルの置き場から見たもの | 無し(色だけ) |
| `mode` | 画像の置き方。`fill`・`fit`・`center` | `fill` |

- 色に `#` は付けません(`#` から後はコメントになるため)。
- 読めない行、知らないキー、読めない値は、ログに出して飛ばします。止まりません。
- 読むのは起動のときだけです。変えたら、壁紙のプログラムを起動し直してください。

### 置き方

Windows の「デスクトップの背景」の置き方に倣いました。

| 値 | Windows の名前 | 置き方 |
| --- | --- | --- |
| `fill` | 塗りつぶし | 縦横の比を保って、出力を全部覆うまで拡大・縮小し、はみ出た所を切る(中央に合わせる) |
| `fit` | ページ幅に合わせる | 縦横の比を保って、画像の全体が入るまで拡大・縮小する。余白は `color` で塗る |
| `center` | 中央に表示 | 大きさを変えずに中央に置く。はみ出た所は切り、余白は `color` で塗る |

- 拡大・縮小は、縦と横に分けた三角形のフィルターで行います。縮めるときは、フィルターの幅を縮める割合だけ広げて、間引きのギザギザを抑えます。
- 画像の透けた所(アルファ)は、`color` の上に重ねます。

## 同梱の壁紙

リポジトリの `assets/wallpapers/` に、既定の壁紙の画像を 2 枚置いています(運用者が生成 AI で作ったもの。CC0 1.0。くわしくは `assets/wallpapers/README.md`)。

| ファイル | 幅x高さ | 比率 | バイト数 |
| --- | --- | --- | --- |
| `torch-16x9.png` | 1920x1080 | 16:9 | 1,729,376 |
| `torch-4x3.png` | 800x600 | 4:3 | 474,583 |

### 設定のファイルが無いとき(既定)

設定のファイルが無いときは、同梱の壁紙の置き場を次の順に探し、読める PNG(名前の終わりが `.png`)が 1 つでもある、最初の置き場を使います。

1. 環境変数 `SEINAS_WALLPAPERS`(その置き場そのもの)
2. `$XDG_DATA_HOME/seinas/wallpapers`(`XDG_DATA_HOME` が無ければ `~/.local/share/seinas/wallpapers`)
3. `$XDG_DATA_DIRS` の各々の下の `seinas/wallpapers`(`XDG_DATA_DIRS` が無ければ `/usr/local/share` と `/usr/share`)

- 相対パスは使いません(XDG Base Directory の決まりのとおり。ZeikOS も、相対のパスの open を断る)。空の値は、無いものとして扱います。実行ファイルからの相対の置き場は、探しません。
- 置き場の中の画像のうち、出力の縦横の比にいちばん近いもの(比の対数の差がいちばん小さいもの。同じなら名前の順で先のもの)を、塗りつぶし(`fill`)で出します。色は既定(`191e28`)です。
  - 例:1920x1080 → 16:9、1280x800(16:10。ZeikOS の画面)→ 16:9(左右が約 10% 切れる)、800x600・1024x768 → 4:3。
  - 出力の大きさが変わったら、選び直します(再描画は、今までどおり大きさが変わったときだけ)。
- どこにも無ければ、色だけです。
- ログ:起動のときに `no config file; the bundled wallpapers in <置き場>: torch-16x9.png (1920x1080), torch-4x3.png (800x600)`(見つからなければ `no config file and no bundled wallpaper`)、描くたびに `the bundled wallpaper torch-4x3.png (800x600) for 800x600` を出します。読めない PNG は、理由をログに出して飛ばします。
- リポジトリから動かすときは、`SEINAS_WALLPAPERS` にリポジトリの `assets/wallpapers` の絶対パスを渡します。

```bash
SEINAS_WALLPAPERS=$PWD/assets/wallpapers WAYLAND_DISPLAY=seinas-0-shell cargo run --locked -p seinas-wallpaper
```

### 設定のファイルで選ぶとき

設定のファイルの `image =` に、画像のパスを書きます(比での選び分けはしない)。相対パスは、設定のファイルの置き場から見たものなので、リポジトリの外に設定のファイルを置くときは、絶対パスで書きます。

```text
# 同梱の 16:9 の壁紙を、画面いっぱいに(はみ出た所を切って)出す
image = /home/<ユーザー>/workspace/hack-labs/seinas/assets/wallpapers/torch-16x9.png
mode = fill
```

- 4:3 の画面(800x600 など)では、`image =` を `torch-4x3.png` にすると、切れる所がありません。
- 1920x1080 の画像は、読み込んでから縮めるまでの間、約 8.3 MB(1 ピクセル 4 バイト)をメモリーに置きます。

### 配り方

- 画像は、実行ファイルに埋め込みません(1.7 MB ほど大きくなるため)。
- release では、壁紙の実行ファイル(`seinas-wallpaper`)を release に入れるときに、別のアセット `seinas-wallpapers-<タグ>.tar.gz`(PNG の 2 枚、`LICENSE`、`README.md`)にして一緒に入れます。置く先は `<データの置き場>/seinas/wallpapers`(例:`/usr/share/seinas/wallpapers`)です。今の release(v0.3.0。seinas-standalone とタスクバー)には、壁紙が入っていないので、release のスクリプトはまだ変えていません。

### ZeikOS の上で

- 2 枚とも 4 MiB より小さい(1,729,376 と 474,583 バイト)ので、今の ZeikOS の ext2 でも読めます。
- ただし、壁紙は出力の大きさの共有メモリー(1280x800 で 4,096,000 バイト)を使い、今の ZeikOS の memfd は 32 KiB まで(下の「ZeikOS の上で動かすときの前提」)なので、画像にかかわらず、ZeikOS の上ではまだ壁紙を出せません。
- 縮めた画像(たとえば、ZeikOS の画面にそのまま合う 1280x800)は、今は作りません。作るのは、壁紙を ZeikOS に載せると決めたときです(読み込みのメモリーと縮める処理が減る)。

## 画像

- 読めるのは PNG だけです(グレースケール・RGB・パレット・アルファつき、8 ビットと 16 ビット)。読むのは、純 Rust の `png` クレートです。
- 大きさの上限は、4096x4096(16,777,216 ピクセル)です。これを越える画像は、画像のデータを読む前に断り、ログに出して色だけで動きます。
  - 理由:4K(3840x2160)の画像まで入れつつ、読み込んだ絵(1 ピクセル 4 バイト。上限なら 64 MiB)を、縮めるまでの間だけメモリーに置いても困らない大きさにしました。これより大きな画像は、どうせ出力に合わせて縮めるので、利用者に縮めてもらいます。
- 読めない画像(無い、PNG でない、壊れている)も、ログに出して(`… ; only the color is shown`)色だけで動きます。

## 再描画

- configure で届いた大きさが前と違うときだけ、画像を読み直して、縮め直し、再描画します。同じ大きさの configure では、再描画しません。
- 縮めた後は、読み込んだ絵を手放します(持っているのは、出力の大きさの共有メモリーだけ)。大きさが変わったときは、画像のファイルを読み直します。
- 入れ子の構成では、親のウィンドウの大きさを変えると、出力の大きさも変わり、コンポジタが壁紙へ新しい大きさの configure を送ります(`docs/window-placement.md` の「出力の大きさが変わったとき」)。壁紙は、その大きさで再描画します。fake screen では、テスト用の入力インターフェースの `output WxH` の行で、同じ道を通せます。
- コンポジタとつないだ再描画(大きくしたとき・小さくしたとき・同じ大きさでは再描画しないこと)は、`crates/seinas-standalone/tests/output_resize.rs` が確かめます。

## layer surface

| 項目 | 値 |
| --- | --- |
| 層 | background |
| anchor | 上・下・左・右 |
| 大きさ | 0x0(コンポジタに任せる。出力の大きさになる) |
| exclusive zone | -1(タスクバーなどの exclusive zone にかかわらず、出力の全体に置いてもらう。タスクバーの下にも描く) |
| キーボードの interactivity | none(フォーカスを取らない) |
| namespace | `seinas-wallpaper` |

ウィンドウとタスクバーは、その上に描かれます。壁紙は seat を使わないので、キーボードとポインターのイベントを受けません。

## メモリーと syscall

- 800x600 の出力で、VmRSS は約 2.9 MB、VmData は 124 kB です(共有メモリーの pool 1,920,000 バイトを含む)。画像を縮めている間だけ、読み込んだ絵と作業の分が増えます(48x32 のテスト画像で、VmHWM 約 5.2 MB)。
- syscall は zeyes-min と同じ形に、設定と画像のファイルの open・fstat・read を足したものです。スレッドは作りません。記録は `docs/m3-syscalls.md` の「変更ごとの差分の記録」にあります。

## ZeikOS の上で動かすときの前提

zeyes-min と同じもの(`docs/zeikos-requirements.md`)に加えて、次のものが要ります。

- 設定と画像のファイルを読むこと(open・fstat・read)。画像が 4 MiB を越えると、ext2 の 2 段目の間接ブロックが要ります。
- 出力の大きさの共有メモリー(800x600 で 1,920,000 バイト。ZeikOS の memfd は、今は 32 KiB まで)。
- 環境変数 `WAYLAND_DISPLAY`(シェルの部品のソケット)と `XDG_RUNTIME_DIR`、設定の置き場の `XDG_CONFIG_HOME` か `HOME`。

## テスト

- 設定の読み方、置き方、透けた所の重ね方、拡大・縮小、大きすぎる画像と壊れた画像は、`crates/seinas-wallpaper` のユニットテスト(`src/lib.rs`)で確かめます。
- 置き方ごとの絵は、正解データ(`crates/seinas-wallpaper/tests/golden.rs`。W1 色だけ、W2 塗りつぶし、W3 ページ幅に合わせる、W4 中央に表示、W5 縦長の出力での塗りつぶし、W6 画像より小さな出力での中央に表示)と比べます(`docs/golden.md`)。
- テストに使う画像は、`crates/seinas-wallpaper/tests/data/test-48x32.png`(48x32、362 バイト)と、テストの中で作る小さな単色の PNG だけで、システムの画像と、同梱の壁紙(`assets/wallpapers/`)には左右されません。コンポジタにつなぐテストは、同梱の壁紙の置き場(`SEINAS_WALLPAPERS`・`XDG_DATA_HOME`・`XDG_DATA_DIRS`)を、どれも作業用の場所の中に向けます。
- 同梱の壁紙の探す順・置き場の選び方・比での選び方は、ユニットテスト(`src/lib.rs`)で、見つかったとき(比の近いものを出し、出力の大きさが変わったら選び直す)・見つからないとき・設定のファイルがあるときは、`crates/seinas-standalone/tests/wallpaper.rs` で確かめます。`tools/make-test-wallpaper.py` が Python の標準ライブラリだけで作る自作のもので、Seinas と同じライセンス(MIT)です。
- コンポジタにつないだ動き(設定が無いときに見た目が変わらないこと、タスクバーの下まで出力いっぱいに描くこと、画像の余白、読めない画像、ふだんのソケットではつながらないこと、同じ大きさでは再描画しないこと)は、`crates/seinas-standalone/tests/wallpaper.rs` が fake screen のピクセルで確かめます。
