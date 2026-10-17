# Seinas

Seinasは、ZeikOS向けのWaylandコンポジタです。

今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中でクライアントを合成して見せる「入れ子」の形で動きます。

## 構成

| 場所 | 中身 |
| --- | --- |
| `crates/seinas` | 入れ子の構成のコンポジタ。Waylandの受け口と、親のWaylandへ出す裏側(SCTK)を組み合わせる |
| `crates/seinas-standalone` | 親のWaylandを使わない構成のコンポジタ。Waylandの受け口と、fbdevの裏側を組み合わせる |
| `crates/seinas-frontend` | Waylandの受け口(Smithay)。クライアントの要求を受け、描く要素を並べる |
| `crates/seinas-render` | 共通の描画。pixmanで1枚の絵に合成する。Waylandの受け口には依存しない |
| `crates/seinas-text` | Seinasが自分で描く文字(ウィンドウの題名など)。フォントをファイルから読み、1行の文字をピクセルにする |
| `crates/seinas-fbdev` | fbdev(`/dev/fb0`)へ出す裏側。共通の描画だけを使い、Waylandのソケットは開かない |
| `crates/seinas-test-clients` | テスト用のクライアント。わざと不正な要求を送ったり、共有メモリーを縮めたり、届いたイベントを書き出したりする。テスト用の IME と、text-input-v3 を使うアプリもある |
| `crates/seinas-taskbar` | タスクバー。コンポジタとは別のプログラム(layer-shell と foreign-toplevel のクライアント)で、ウィンドウの一覧と時計を出す |
| `crates/seinas-wallpaper` | 壁紙。コンポジタとは別のプログラム(layer-shell のクライアント)で、background の層に色か画像(PNG)を出す |
| `crates/seinas-config` | 設定のファイル(1 行に「キー = 値」)の読み方。フォントと壁紙の設定が使う |
| `crates/zeyes-min` | 動作確認用の小さなクライアント。xeyesのように2つの目を描き、目玉がポインターを追う |
| `docs/render-boundary.md` | 共通の描画と裏側の境界の決まり |
| `docs/standalone.md` | 受け口とfbdevを組み合わせた構成の、動かし方、描く時機、使うシステムコール |
| `docs/window-placement.md` | ウィンドウの重ね方と置き方の決まり |
| `docs/demo-c1s3.md` | C1S3 の機能を、入れ子の構成で 1 回の起動でまとめて確かめる手順(`tools/demo-c1s3.sh` とチェックリスト) |
| `docs/config.md` | 設定のファイルの一覧と、`seinas.conf`(ウィンドウの切り替えのキー)の書き方、届いたキーの確かめ方(`--log-keys`) |
| `docs/ime.md` | IME の道(見せる global、キーの道、IME の復旧(接続断と緊急解除)、文字の受け渡し、フォーカス、候補ウィンドウ、IME を 1 つに限ること)と、テスト用の IME での試し方 |
| `docs/taskbar.md` | タスクバーの見た目、ボタンの働き、時計の決まりと、ZeikOS の上で動かすときの前提 |
| `docs/wallpaper.md` | 壁紙の使い方(設定のファイル、置き方)、画像の大きさの上限と、ZeikOS の上で動かすときの前提 |
| `docs/shrink-test.md` | 共有メモリーを縮めてくるクライアントのテストの、動かし方と期待する結果 |
| `docs/fbdev.md` | fbdevの裏側の構成、動かし方、確かめ方 |
| `docs/musl-static.md` | muslでの静的ビルドの手順と、できた実行ファイルの記録 |
| `docs/release.md` | seinas-fbdevをGitHubのreleaseとして配る手順 |
| `docs/m3-syscalls.md` | ZeikOS の M3 の準備として、seinas-standalone と zeyes-min を strace した記録 |
| `docs/zeikos-requirements.md` | ZeikOS で Seinas を動かすのに要るもの(syscall とフラグ、fd、置くファイル、スレッド、プロセスの起こし方)と、ZeikOS の今の対応の突き合わせ |
| `docs/adr/0001-replaceable-boundaries.md` | ADR: 外部ライブラリを、後で置き換えられる境界で使う決まりと、今の境界の棚卸し |
| `docs/adr/0002-shell-components.md` | ADR: タスクバー・メニュー・デスクトップを、コンポジタの外の別のプログラムにする(layer-shell、foreign-toplevel、シェルのソケット) |
| `docs/adr/0003-font-selection.md` | ADR: 利用者が選んだフォントで描けるようにする(置き場、一覧、役ごとの選び方、設定のファイル、mmap での読み込み) |
| `docs/adr/0004-ime-keyboard-path.md` | ADR: キーボードの入力は、IME(Kieli)を通る経路にする(Seinas の決定、標準の仕様が求めること、Kieli への申し送り、取り消した決定) |
| `docs/adr/0005-self-hosting.md` | ADR: セルフホスティングを長期のゴールにする(Seinas の側の条件、proc-macro と build.rs の一覧、開発する環境として要るもの) |
| `docs/golden.md` | 正解データ(golden)のテスト:置き換える前の出力を保存して比べる決まりと、作り直しの手順 |
| `docs/terminology.md` | 用語と書き方の決まり(IT 用語の使い方と、これまでの言い方との対応表) |
| `docs/text-rendering.md` | 文字の描き方の決まり(手段、フォント、大きさ)と、手段を比べた記録 |
| `tools/` | 静的ビルドや、依存の一覧を作るための道具、フォントを取得する手順、releaseの一式を作る手順 |
| `trials/` | 採るかどうかを決める前の試し。本体のワークスペースには入れていない |
| `THIRD-PARTY/` | 第三者のライセンス文と著作権表示、依存の一覧 |

## ビルドに要るもの

- Rust(版は `rust-toolchain.toml` で固定)
- pixmanとlibxkbcommonの開発用ファイル。Ubuntuなら次のとおりです。

```bash
sudo apt install libpixman-1-dev libxkbcommon-dev
```

依存するクレートの版は `Cargo.lock` で固定しています。ビルドやテストでは `--locked` を付けてください。

## フォント

ウィンドウの題名を描くのに、フォントのファイルを使います。フォントは、リポジトリにも実行ファイルにも入れていません。
次のスクリプトで、決まった版を `target/fonts/` に取得します(SHA-256を確かめます)。テストも、このフォントを使います。

```bash
tools/fetch-fonts.sh
```

フォントの置き場は、引数 `--fonts DIR` か、環境変数 `SEINAS_FONTS` で変えられます。指定が無ければ、`target/fonts`(作業中のディレクトリから見た位置)を見ます。
フォントが読めなくても、Seinasは動きます(題名の文字が出ないだけです)。くわしくは `docs/text-rendering.md` にあります。

## ビルドとテスト

```bash
cargo build --workspace --locked
```

テストの前に、フォントを取得しておいてください(上の「フォント」)。

```bash
cargo test --workspace --locked
```

## 動かし方

Waylandのセッションの中(WSLgなど)で、リポジトリの先頭のディレクトリから、まずSeinasを起動します。800x600のウィンドウが1つ開きます。ウィンドウの大きさは、親の側で変えられます(下限は320x240)。

```bash
cargo run --locked -p seinas
```

別の端末から、Seinasの中でクライアントを動かします。

```bash
WAYLAND_DISPLAY=seinas-0 cargo run --locked -p zeyes-min
```

タスクバーなどのシェルの部品は、`seinas-0-shell` のソケットへつなぎます(`docs/adr/0002-shell-components.md`)。そこからつないだクライアントにだけ、layer-shell とウィンドウの一覧(foreign-toplevel)が見えます。

```bash
WAYLAND_DISPLAY=seinas-0-shell cargo run --locked -p seinas-taskbar
```

タスクバーは画面の下に出て、開いているウィンドウのボタンと時計を並べます(`docs/taskbar.md`)。

```bash
WAYLAND_DISPLAY=seinas-0-shell cargo run --locked -p seinas-wallpaper
```

壁紙は、色か画像(PNG)を画面いっぱいに出します。設定は `~/.config/seinas/wallpaper.conf` です(`docs/wallpaper.md`)。

IME の道は、テスト用の IME とテスト用のアプリで試せます(`docs/ime.md` の「試し方」)。IME は、IME のためのソケット(`seinas-0-ime`)へつなぎます。

```bash
WAYLAND_DISPLAY=seinas-0 cargo run --locked -p seinas-test-clients --bin test-client -- window --text-input
WAYLAND_DISPLAY=seinas-0-ime cargo run --locked -p seinas-test-clients --bin test-client -- ime --popup 40x30
```

クライアントのウィンドウの上には、Seinasが題名の帯を描きます。zeyes-minの題名は「zeyes」で、`--title TEXT` で変えられます。
帯を掴んで動かすと、ウィンドウが動きます。帯の右の端の「×」を押すと、閉じます。

ウィンドウの切り替えは、Alt+Tab です。入れ子では Windows が Alt+Tab を先に使うので、Alt+F6(前へは Alt+Shift+F6)も使えます。キーは `~/.config/seinas/seinas.conf` で変えられ、届いたキーは `--log-keys` で確かめられます(`docs/config.md`)。

Seinasのウィンドウを閉じると、Seinasは終わります。

## muslでの静的ビルド

ZeikOS向けに、共有ライブラリを使わない実行ファイルを作れます。

```bash
tools/build-musl.sh
```

実行ファイルは `target/x86_64-unknown-linux-musl/release/` に、ライセンス文と一緒にまとめたものは `target/dist/seinas-musl/` にできます。
手順の中身と、できた実行ファイルの記録は `docs/musl-static.md` にあります。

## 今の制限

- 入れ子の構成では、出力(画面)の大きさを、親のウィンドウの大きさに合わせます。始めは800x600で、下限は320x240、上限はありません(`docs/window-placement.md` の「出力の大きさが変わったとき」)。
- クライアントへ渡す入力は、ポインターとキーボードです(入れ子では、親の keymap をそのまま使います)。
- IME の道(アプリの text-input-v3 と、IME の input-method-v2・virtual-keyboard の取り次ぎ、候補ウィンドウを置く位置、IME が去ったときと緊急解除(既定は Alt+Shift+F12)のときの直接の道)はあります(`docs/ime.md`)。時間切れで IME が応答しないと見なす形は、2026-10-15 に取り消して外しました(ADR 0004)。IME そのもの(Kieli)は、まだありません。今つなげるのは、テスト用の IME(`test-client ime`)です。IME は同時に 1 つだけで、IME のためのソケットからつなぎます。
- クライアントのウィンドウは、決まった位置に少しずつずらして重ねます(`docs/window-placement.md`)。クリックで raise し、タイトルバーをドラッグして動かし、閉じるボタンで閉じられます。縁と角のドラッグで大きさを変えられます。タイトルバーのボタンとダブルクリックで最大化でき、タスクバーがあれば最小化もできます(`docs/window-placement.md`)。全画面はできません。

## ライセンス

自作のコードは MIT ライセンスです(`LICENSE`)。
フォントと外部のライブラリには、それぞれのライセンスがあります(`THIRD-PARTY/README.md`)。フォントは MIT ではなく、それぞれのフォントのライセンス(SIL Open Font License 1.1)で配ります。
