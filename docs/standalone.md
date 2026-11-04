# フロントエンドとfbdevを組み合わせた構成

`crates/seinas-standalone` は、親のWaylandを使わない構成のSeinasです。
Waylandのフロントエンドで受けたクライアントの画面を、共通のレンダラーで合成し、fbdevの画面へ present します。

## 構成

| クレート | 役割 | この構成で使うか |
| --- | --- | --- |
| `seinas-frontend` | Waylandのフロントエンド(Smithay) | 使う |
| `seinas-render` | 共通のレンダラー | 使う |
| `seinas-text` | 文字を描く(ウィンドウのタイトル)。フロントエンドが使う | 使う |
| `seinas-fbdev`(ライブラリ) | fbdevのバックエンドと、fake screen | 使う |
| `seinas`(入れ子のバックエンド。SCTK、wayland-client) | 親のWaylandへ出す | 使わない |

フロントエンドは、もとは `seinas` の中のモジュールでした。入れ子の構成とこの構成の両方から使えるように、`seinas-frontend` として切り出しています。
Smithayのハンドラは、コンポジタの状態の型そのものに実装する決まりなので、`seinas-frontend` は次の2つを用意しています。

- `FrontendHost`: 状態の型が実装する。フロントエンドの置き場所と、再描画の知らせ方を教える。
- `delegate_frontend!`: Smithayのハンドラと、フロントエンドの作り方を、状態の型に実装するマクロ。

入れ子のバックエンドが依存に入っていないことは、次で確かめます。CIと `tools/build-musl.sh` にも入れてあります。

```bash
tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client calloop-wayland-source
```

フロントエンドの側のwayland系のクレート(wayland-server、wayland-backend、wayland-protocolsなど)は、入っていて当然なので見ません。

## 動かし方

装置を開かずに、fake screen(メモリー上)で動かす例です。

```bash
cargo run --locked -p seinas-standalone -- --fake 800x600 --dump /tmp/screen.ppm --socket /tmp/seinas.sock
```

```bash
WAYLAND_DISPLAY=/tmp/seinas.sock cargo run --locked -p zeyes-min
```

この例では、シェルの部品のためのソケットは `/tmp/seinas-shell.sock`、IME のためのソケットは `/tmp/seinas-ime.sock` になり、ロックファイルは `/tmp/seinas.lock`・`/tmp/seinas-shell.lock`・`/tmp/seinas-ime.lock` です(下の `--shell-socket`・`--ime-socket`)。

| 引数 | 意味 |
| --- | --- |
| `--device PATH` | 画面の装置。無ければ環境変数 `SEINAS_FBDEV`、それも無ければ `/dev/fb0` |
| `--fake WIDTHxHEIGHT` | 装置を開かず、その大きさの fake screen へ描く |
| `--dump FILE` | fake screen の中身を、描くたびにPPM形式の画像としてファイルに書く(`--fake` のときだけ) |
| `--socket PATH` | クライアントのために listen するソケットの場所。無ければ環境変数 `SEINAS_SOCKET`、それも無ければ `XDG_RUNTIME_DIR` の下の `seinas-0`。ロックファイルは、パスの拡張子を `.lock` に替えたもの(`/tmp/seinas.sock` なら `/tmp/seinas.lock`。wayland-server の決まり) |
| `--font-config PATH` | フォントの設定のファイル。無ければ `$XDG_CONFIG_HOME/seinas/fonts.conf`(`text-rendering.md` の「利用者が選んだフォント」) |
| `--shell-socket PATH` | シェルの部品(タスクバーなど)のためのソケットの場所。無ければ、ふだんのソケットのパスの名前に `-shell` を入れたもの(名前に拡張子があれば、その前に入れる。`/tmp/seinas.sock` なら `/tmp/seinas-shell.sock`、`/tmp/wl` なら `/tmp/wl-shell`。`--socket` が無ければ `XDG_RUNTIME_DIR` の下の `seinas-0-shell`)。ここからつないだクライアントにだけ、layer-shell とウィンドウの一覧(foreign-toplevel)を見せる |
| `--ime-socket PATH` | IME のためのソケットの場所。無ければ、ふだんのソケットのパスの名前に `-ime` を入れたもの(`--shell-socket` と同じ形。`/tmp/seinas.sock` なら `/tmp/seinas-ime.sock`。`--socket` が無ければ `XDG_RUNTIME_DIR` の下の `seinas-0-ime`)。ここからつないだクライアントにだけ、input-method と virtual-keyboard を見せる(`ime.md`)。3 つのソケットのロックファイルがぶつかる指定(`--socket /tmp/a.sock --shell-socket /tmp/a.other` など、拡張子の前が同じもの)は、`would share the lock file` のエラーで終わる。パスを指定しないソケット(`XDG_RUNTIME_DIR` の下の `seinas-0` など)も比べる |
| `--no-ime-socket` | IME のためのソケットを開かない。ソケットもロックファイルも作らず、input-method と virtual-keyboard の global も作らない(`ime.md`)。IME の緊急解除のキー(既定は Alt+Shift+F12)も予約せず、アプリへ渡す。IME を使わない構成で、fd を 2 本減らすためのもの。既定は開く。`--ime-socket` と一緒に付けると、`cannot be used together` のエラーで終わる |
| `--fonts DIR` | フォントの置き場(ウィンドウのタイトルを描くのに使う)。無ければ環境変数 `SEINAS_FONTS`、それも無ければ `target/fonts`(作業中のディレクトリから見た位置)。フォントが読めなくても動く(タイトルの文字が出ないだけ) |
| `--test-input` | テスト用の入力インターフェースを開く(下の「テスト用の入力インターフェース」)。ふだんの起動では、付けない |
| `--ping-timeout MS` | 閉じるように頼んだクライアントの pong を待つ時間(ミリ秒。既定は 5000)。過ぎたら応答なしにする(`window-placement.md` の「応答なし」)。テストで短くするためのもの |
| `--config PATH` | Seinas の設定のファイル(ウィンドウの切り替えのキー)。無ければ `$XDG_CONFIG_HOME/seinas/seinas.conf`(`config.md`) |
| `--log-keys` | 届いたキーを、修飾キーと keysym の名前でログに出す(`key: Alt+F6 pressed`)。既定では出さない |

- ソケットの場所を指定したときは、`XDG_RUNTIME_DIR` は要りません。
- ソケットの隣に、ソケットのパスの拡張子を `.lock` に替えた名前のロックファイルを作ります(`/tmp/seinas.sock` なら `/tmp/seinas.lock`。拡張子が無ければ `.lock` を付けた名前)。
- クライアントは、`WAYLAND_DISPLAY` にソケットの絶対パスを入れてつなぎます。
- `--dump` は、書きかけを読まれないように、別の名前で書いてから名前を付け替えます。

## ZeikOS の上で起動する

ZeikOS の上では、release の `seinas-standalone`(コンポジタ)・`seinas-taskbar`(タスクバー)・`seinas-wallpaper`(壁紙)・`zeyes-min`(目玉のウィンドウ)を、変更を入れずに、そのまま動かします(`docs/release.md` の「seinas-standalone の release」。タスクバーは v0.3.0 から、壁紙と zeyes-min と同梱の壁紙は v0.4.0 から)。

### v0.3.0 からの変更(v0.4.0)

- 見た目:ウィンドウに枠・角の丸み・影が付き、タイトルバーは高さ 36、ボタンは幅 44 になり、タイトルの左にアプリのアイコンを出します。Alt+Tab の一覧の強調と、最大化の予告は、オレンジの枠と控えめな地です(`docs/ui-spec.md`)。
- タスクバー:薄いタスクバー(中央のトーチとタスクのアイコン、前面の線と背面の点、2 段の時計)。アイコンの間隔は 48 で、入りきらないタスクは末尾の「…」にまとめ、押すと隠れたタスクの一覧を出します(`docs/taskbar.md`)。
- 壁紙:設定のファイルが無いとき、同梱の壁紙(16:9 と 4:3 の 2 枚。CC0 1.0)から、出力の比にいちばん近いものを出します(`docs/wallpaper.md`)。
- 装飾:ウィンドウごとに xdg-decoration で決め、取り決めないウィンドウ(GTK など)には飾りを付けません。最初の configure の大きさは 0x0 で、アプリが自分の大きさで開きます(`docs/window-placement.md`)。
- ポップアップ(xdg_popup)、layer surface のキーボード(keyboard_interactivity)、クリップボードとドラッグ&ドロップ(wl_data_device_manager)を扱います(`docs/window-placement.md`)。ZeikOS の上では、まだ入力が無いので、これらは使われません(下の「ZeikOS の制限に当たる所」)。
- 起動の順序:壁紙と zeyes-min も、タスクバーと同じく、コンポジタより先に起こされたら、ソケットができるまで 100 ms ごとに 30 秒まで試し直してつなぎます(`crates/seinas-connect`)。
- 直したもの:修飾キーと右のボタンで始めたサイズ変更が、右のボタンを離しても終わらなかった。入れ子の Seinas が、応答なしの印を付けた後に再描画しなかった。IME の古い応答を新しい入力先へ適用していた。ポップアップに frame コールバックを返していなかった。ポップアップの grab の間に離したボタンを、Seinas の操作に伝えていなかった。

### 置くファイル(v0.4.0)

release `v0.4.0` の成果物です。大きさと SHA-256 は、release の `SHA256SUMS` と同じになるはずのものです(2026-11-04 に、`8314851` で、push の確かめを外した `tools/release-standalone.sh v0.4.0` を手元で動かして作った。実行ファイルは、ソースと Rust と Cargo.lock が同じなら同じバイトになる(`docs/release.md` の「再現可能なビルド」)。その後のコミットは文書だけ)。release を作ったら、`SHA256SUMS` と見比べてください。

| release の上の名前 | 置き場(ZeikOS のイメージ。例) | 大きさ(バイト) | SHA-256 |
| --- | --- | --- | --- |
| `seinas-standalone` | `/bin/linux/seinas-standalone` | 9,809,392 | `07f0d53735a5774d1b4c870bd76b43d936b02f742d2b654a0fbc9c825757226d` |
| `seinas-standalone-v0.4.0-third-party.tar.gz` | 実行ファイルと一緒に(第三者のライセンス文と著作権表示) | 71,568 | `6f77a26519db9dee8acf920efdfeffcd632fe16d45106fb93fae1c34d504b897` |
| `seinas-taskbar` | `/bin/linux/seinas-taskbar` | 5,370,440 | `836a8facf2ebe9ca69e8d4a734d38581a771065973bf127a5c857c6a9ce89b94` |
| `seinas-taskbar-v0.4.0-third-party.tar.gz` | 実行ファイルと一緒に(同上) | 63,118 | `93ef559bc2075e840fc09d01b3786371d6637ef5293ba9d5fc30f09a4c467cde` |
| `seinas-wallpaper` | `/bin/linux/seinas-wallpaper` | 1,724,680 | `2a56bd91aa924d5055b7afaca388f44e48bd0a585708e8fcad73c8b0c61e6223` |
| `seinas-wallpaper-v0.4.0-third-party.tar.gz` | 実行ファイルと一緒に(同上) | 36,138 | `d6062c2d08691647c89fd97550a6a42cefed725c936743d66bcbfcfd6b8b522e` |
| `zeyes-min` | `/bin/linux/zeyes-min` | 1,753,736 | `e80821a2c0409a07f21d8258844a6a9692e622a2374f80fb97b7995db17194d4` |
| `zeyes-min-v0.4.0-third-party.tar.gz` | 実行ファイルと一緒に(同上) | 32,044 | `72824cb8644d8d683a1d15644ff6a0f475a0116732cc86a444780c6c8c98e61c` |
| `seinas-wallpapers-v0.4.0.tar.gz` | 開いて、中の 4 つを `/usr/share/seinas/wallpapers/` に(下の「同梱の壁紙」) | 2,208,866 | `eeb0911d2b4cab582ece17a1aa34b2f3ea5d68dc2fb8338819f036a4c61e4596` |

- 置き場の名前は、v0.2.0・v0.3.0 と同じく、ZeikOS の側が決めます(上の表の置き場は、v0.3.0 の `/bin/linux/` に合わせた例)。取り込みは ZeikOS の側が行います。
- 実行ファイルのうち、コンポジタとタスクバーは 4 MiB(4,194,304 バイト)を越えます。v0.3.0 のときも越えていて(9,095,904 と 4,966,232 バイト)、ZeikOS の上で動いたので、実行ファイルは 4 MiB の制限の外にある置き方と見ています(ZeikOS の側の確かめを待つ)。
- 第三者のライセンス文と著作権表示のアーカイブ(`*-third-party.tar.gz`)は、実行ファイルと一緒に置くか、配り方に合わせて置きます(実行には要らない)。

#### 同梱の壁紙

- `seinas-wallpapers-v0.4.0.tar.gz` を開くと、`seinas-wallpapers-v0.4.0/` の下に `torch-16x9.png`(1,729,376 バイト)、`torch-4x3.png`(474,583 バイト)、`LICENSE`、`README.md` があります。この 4 つを `/usr/share/seinas/wallpapers/` に置きます。
- 壁紙は、既定で `XDG_DATA_DIRS` の各々の下の `seinas/wallpapers` を探し、`XDG_DATA_DIRS` が無ければ `/usr/local/share` と `/usr/share` の下を探します(どれも絶対パス)。なので、`/usr/share/seinas/wallpapers/` に置けば、環境変数は要りません。`XDG_DATA_DIRS` を別の値にしているときは、その下に置くか、`SEINAS_WALLPAPERS=/usr/share/seinas/wallpapers` を渡します(`SEINAS_WALLPAPERS` が、いちばん先に探す置き場)。
- 2 枚とも 4 MiB より小さいので、ZeikOS 向けに縮めた画像は要りません。1280x800 の画面では、比のいちばん近い `torch-16x9.png`(1920x1080)を読み、塗りつぶし(左右が約 10% 切れる)で出します。読んでから縮めるまでの間、約 8.3 MB(1920x1080 × 4 バイト)をメモリーに置きます。
- 壁紙の共有メモリーは、出力の大きさ(1280x800 で 4,096,000 バイト)です。今の ZeikOS の memfd(1 つ 4096 ページ = 16 MiB、全体で 16,384 ページ、16 個まで。zeikos の `kernel/src/shm.rs`)に収まります。
- 置かなければ、壁紙は色(`191e28`)だけを出し、`no config file and no bundled wallpaper` を 1 行出します。

#### アイコンと .desktop

- 置きません。タスクバーとタイトルバーは、アイコンが見つからないアプリに既定のアイコンを出し、`no icon for zeyes-min; the default icon is used` を 1 回出します。トーチのアイコンは、プログラムが描くので、ファイルは要りません。
- 置くときは、`/usr/share/applications/<app_id>.desktop`(`Icon=` の名前)と、`/usr/share/icons/hicolor/24x24/apps/<名前>.png` です(`docs/taskbar.md` の「アイコン」。PNG だけ。1 MiB まで、幅と高さは 512 まで)。zeyes-min の app_id は `zeyes-min` です。

#### フォント

- 置きません。同梱のフォントは 3 本とも 4 MiB を越え(`BIZUDPGothic-Regular.ttf` 4,669,688、`BIZUDGothic-Regular.ttf` 4,667,380、`unifont_jp-18.0.01.otf` 5,250,304 バイト)、今の ZeikOS では 1 つのファイルが 4 MiB 未満でなければならないためです。
- フォントが無くても、4 つとも動きます。文字を描かないだけです。
  - コンポジタ:ウィンドウのタイトルの文字を描きません。起動のときに、フォントを読めない WARN の行を 3 行出します(既定の置き場が `target/fonts` の相対のパスのため)。
  - タスクバー:時計の文字と、隠れたタスクの一覧のタイトルを描きません(アイコンと印と地は描く)。起動のときに、`cannot read the font target/fonts/BIZUDPGothic-Regular.ttf: …` と `cannot read the font target/fonts/unifont_jp-18.0.01.otf: …` の 2 行を出します。
  - 壁紙と zeyes-min:文字を使いません。
- 文字を出すには、4 MiB 未満のフォント(たとえば、使う文字だけに減らしたもの)を作るか、ZeikOS が 4 MiB 以上のファイルを読めるようになるのを待ちます。どちらにするかは未決です。

#### xkb

- xkb のファイルは置きません。コンポジタは、ふだんの起動(`--test-input` なし)ではキーボードを作らず、タスクバーはキーボードが無ければ使わないので、どれも xkb のファイルを読みません。

### 起動のしかた

zash で、zeyes-min・壁紙・タスクバーを `|` の左に、コンポジタをいちばん右に置いて起動します。ZeikOS では `/dev/fb0` を前景のプロセスしか開けないので、コンポジタを前景(いちばん右)にします。左の 3 つは先に起き、コンポジタのソケットができるまで待ってつなぎます(`docs/taskbar.md` の「起動の順序」。v0.4.0 から、壁紙と zeyes-min も待つ)。

```text
export WAYLAND_DISPLAY=/tmp/seinas/s-shell.sock
WAYLAND_DISPLAY=/tmp/seinas/s.sock /bin/linux/zeyes-min | /bin/linux/seinas-wallpaper | /bin/linux/seinas-taskbar | /bin/linux/seinas-standalone --socket /tmp/seinas/s.sock --no-ime-socket
```

- zeyes-min だけは、ふだんのソケット(`/tmp/seinas/s.sock`)へつなぐので、コマンドの前の `WAYLAND_DISPLAY=…` で変えます。壁紙とタスクバーは、`export` したシェルの部品のためのソケット(`/tmp/seinas/s-shell.sock`)へつなぎます。
- zash が 4 段のパイプと、コマンドの前の `名前=値` を受けるかは、ZeikOS の側で確かめてください。受けないときは、まず zeyes-min を外した `/bin/linux/seinas-wallpaper | /bin/linux/seinas-taskbar | /bin/linux/seinas-standalone …`(`export` は同じ)で、壁紙とタスクバーまでを確かめられます。zeyes-min を同じ 1 行に入れる別の手段(zeyes-min に、つなぐソケットを選ぶ引数を足すなど)は、要るときに決めます。
- `WAYLAND_DISPLAY` は、壁紙とタスクバーがつなぐシェルの部品のためのソケット(`/tmp/seinas/s-shell.sock`)と、zeyes-min がつなぐふだんのソケット(`/tmp/seinas/s.sock`)です。絶対パスなら `XDG_RUNTIME_DIR` は使いません。
- コンポジタは、`--socket` で場所を決めるので、`WAYLAND_DISPLAY` と `XDG_RUNTIME_DIR` を読みません。シェルの部品のためのソケットは、`--socket` のパスから `/tmp/seinas/s-shell.sock` になります。
- `--no-ime-socket` で、IME のためのソケットを開きません(IME を使わない構成)。
- パイプは、起動の順序を決めるためだけに使います。左の 3 つは標準出力に何も書かず、コンポジタは標準入力を読みません(`--test-input` を付けないため)。ログはどれも標準エラーです。
- どれも `--fonts` と `--config` を付けません。タスクバーの時計は、既定の +09:00 です(`TZ` も設定のファイルも無いため)。

### 起動が済んだことの見分け方

- コンポジタ(標準エラー):
  - `listening on /tmp/seinas/s.sock`、`listening on /tmp/seinas/s-shell.sock`、`the socket for the input method is not opened (--no-ime-socket)`
  - `screen: 1280x800 (…)`(大きさは、ZeikOS の画面のもの)
  - `presented the first frame (1280x800)`(最初の 1 枚を画面へ present し終えたときに、1 回だけ。target は `seinas_standalone`)
  - `a client connected`(3 回)、`usable area: 1280x760 at (0, 0)`(タスクバーの exclusive zone)、`new toplevel`(zeyes-min)
- タスクバー(`seinas-taskbar:` で始まる):`the clock is UTC+09:00 (from the default)` と、上のフォントの 2 行。先に起きたときは `the compositor is not ready at /tmp/seinas/s-shell.sock (…); trying again every 100 ms for up to 30000 ms` と `connected to the compositor after <回数> tries`。
- 壁紙(`seinas-wallpaper:` で始まる):`no config file; the bundled wallpapers in /usr/share/seinas/wallpapers: torch-16x9.png (1920x1080), torch-4x3.png (800x600)`、`the bundled wallpaper torch-16x9.png (1920x1080) for 1280x800`、`drew 1280x800`。先に起きたときは、タスクバーと同じ 2 行。
- zeyes-min(`zeyes:` で始まる):先に起きたときは、タスクバーと同じ 2 行。`configured to (None, None)`。
- 画面:壁紙の上に、左上に zeyes-min のウィンドウ(タイトルバーつき。フォントが無いのでタイトルの文字は無い)、下の端に高さ 40 のタスクバー(中央にトーチと zeyes-min のタスク。前面の印のオレンジの線)が出ます。
- つなげないときは、30 秒で `the compositor did not appear …` で終わります(タスクバーは、最初の往復の期限 30 秒も持つ)。

### ZeikOS の制限に当たる所(v0.4.0。2026-11-04 に確かめた)

ZeikOS のバグ探しで分かった制限に、4 つのプログラムが当たるかを、コードを読み、Linux で上の起動のしかたと同じ形(`--test-input` なし、`--no-ime-socket`、フォントなし、作業中のディレクトリは `/`、1280x800)で strace を取って確かめました。今の起動のしかたでは、どれにも当たりません。当たりうる所は、どれも ZeikOS の上ではまだ通らない道(キーボードやポインターが要るもの)です。

| 制限 | 起動のしかたのとおりに動かしたとき | 当たりうる所(今は通らない) |
| --- | --- | --- |
| memfd へ `write()` すると EBADF | 当たらない。3 つのクライアントは、共有メモリーを `memfd_create`・`ftruncate`・`mmap` で作り、mmap で書く(SCTK の `SlotPool`)。コンポジタは memfd を作らない | コンポジタが keymap を送るとき:Smithay の `SealedFile::with_content`(`src/utils/sealed_file.rs` の 37 行目)が `memfd_create` の後に `write_all` する。キーボードがあるときだけ(`--test-input`、入れ子)。ZeikOS の上でキーボードを作るようになると、最初に `wl_keyboard` を取るクライアント(タスクバー)のときに EBADF になり、Smithay は keymap を作れない(コンポジタが止まるかは、その形で確かめる)。IME の virtual-keyboard の keymap の書き直しも同じ(`--no-ime-socket` で通らない)。wayland-cursor(SCTK の依存として、3 つのクライアントに入る)は、カーソルのテーマの絵を memfd へ `write_all` する(`wayland-cursor` 0.31.14 の `src/lib.rs` の 164・374 行目)が、Seinas のクライアントはカーソルのテーマを使わない(`ThemedPointer` などを呼ばない) |
| 1 回の `sendmsg` で fd を 2 つ以上送ると、1 つ目しか運ばない | 当たらない。3 つのクライアントは、`SCM_RIGHTS` で fd を 1 つ(pool の memfd)だけ送る。コンポジタは fd を送らない | wayland-backend は、送る前に溜まったメッセージの fd を、1 回の `sendmsg` にまとめる(28 個まで。`src/rs/socket.rs` の 164 行目)。fd を持つメッセージが、1 回の送りの前に 2 つ溜まると当たる。コンポジタからは keymap(キーボードがあるとき)と、クリップボードの `wl_data_source.send`(キーボードのフォーカスがあるとき)。今は通らない |
| ソケットを POLLOUT で待つと EINVAL | 当たらない。どれも POLLOUT で待たない | 送りが詰まったとき:コンポジタ(wayland-backend のサーバー)は溜めて次に送り直し、epoll は IN だけ(`src/rs/server_impl/handle.rs` の 371 行目)。クライアント(calloop-wayland-source)も、次に送り直すだけ(`src/lib.rs` の 224 行目)。wayland-client の `roundtrip` の中で詰まると、エラーで終わる(`src/conn.rs` の 161 行目。待たない) |
| poll・ppoll に 5 本以上の fd、POLLIN と POLLOUT を一緒に | 当たらない。poll は、Rust の標準ライブラリの起動の 3 本(fd 0〜2、events 0)、タスクバーの最初の往復の 1 本(POLLIN)、wayland-client の読み待ちの 1 本(POLLIN\|POLLERR)だけ。イベントループは epoll(calloop) | — |
| `pread64`・`pwrite64`・`fsync`、memfd 以外の `ftruncate` | 当たらない。`ftruncate` は memfd だけ(タスクバー 256,000 → 512,000、壁紙 1,920,000 → 4,096,000、zeyes-min 614,400 バイト)。ファイルは `read` で読む | — |

- strace の記録と道具は、リポジトリには入れていません(運用者の手元の `docs/notes/zeikos-v040/`)。
- 前提:ZeikOS の上で入力(キーボード・ポインター)をコンポジタへ渡すようになったら、上の「当たりうる所」を、その形で確かめ直します。

### ZeikOS の側に要るもの(v0.4.0)

v0.3.0(コンポジタとタスクバー)で要ったもの(`ppoll`・`poll`・`clock_gettime(CLOCK_REALTIME)`・`clock_nanosleep(CLOCK_MONOTONIC)`。`docs/zeikos-requirements.md`、`docs/taskbar.md` の「ZeikOS の上で動かすときの前提」)に加えて、壁紙と zeyes-min をつなぐと、次のものが要ります。数は、上の strace(1280x800)で確かめた値です。

- 共有メモリー:クライアントごとに pool の memfd が 1 つ。タスクバー 256,000 → 512,000 バイト(`ftruncate` 2 回)、壁紙 1,920,000 → 4,096,000 バイト(`ftruncate` 2 回)、zeyes-min 614,400 バイト(1 回)。コンポジタは memfd を作りません(キーボードが無いので keymap も無い)。zeikos の 5d3b9f39 の `kernel/src/shm.rs` の上限(1 つ 4096 ページ = 16 MiB、全体で 16,384 ページ = 64 MiB、16 個まで)に収まります。`docs/zeikos-requirements.md` の memfd の行(2 つまで、8 ページ)は 10/21 の値で、古くなっています。
- fd:コンポジタは、`--no-ime-socket` でクライアント 0 のとき 13 本(ZeikOS の上で /dev/fb0 を含む)、クライアント 1 つにつき 2 本(接続のソケットと pool の memfd)増えるので、3 つで 19 本です(上限 32)。
- 壁紙は、同梱の壁紙のファイルを `open`・`fstat`・`read` で読みます(1,729,376 バイト)。
- zeyes-min は、ふだんのソケット(`/tmp/seinas/s.sock`)へつなぎます。壁紙とタスクバーは、シェルの部品のためのソケットです。

### 止め方

前面のコンポジタを、ZeikOS の手段(Ctrl+C)で止めます。左の 3 つは、接続が切れて `the connection to the compositor was lost` で終わります。終わるときのクリーンアップ(ソケットのファイルとロックファイルを消す、など)はしていません(下の「今の制限」)。

コンポジタだけを起動するときは、今までどおり `seinas-standalone --socket /tmp/seinas/s.sock`(v0.2.0 の形)でも動きます。v0.3.0 の形(`seinas-taskbar | seinas-standalone …`)も、そのまま動きます。

## 公開するもの

| グローバル | 中身 |
| --- | --- |
| wl_compositor、wl_subcompositor | 画面(サーフェス) |
| wl_shm | 共有メモリーのバッファ |
| xdg_wm_base | ウィンドウ |
| zxdg_decoration_manager_v1 | 飾り(タイトルバー)を描く側のイベント。いつも、サーバーの側で描くと答える |
| zwlr_layer_shell_v1 | layer surface(タスクバーなどのシェルの部品)。シェルの部品のためのソケットからつないだクライアントにだけ見せる(`window-placement.md`) |
| zwlr_foreign_toplevel_manager_v1、ext_foreign_toplevel_list_v1 | ウィンドウの一覧と操作(タスクバーなどが使う)。シェルの部品のためのソケットからつないだクライアントにだけ見せる(`window-placement.md`) |
| wl_output | 画面の大きさ。表示モードは1つで、幅と高さは画面と同じ |

wl_seatは、ふだんは公開しません(入力の装置を扱わないためです)。テスト用の入力インターフェース(下の「テスト用の入力インターフェース」)を開いたときだけ、wl_seat とポインターとキーボードを公開します。キーボードの keymap は、日本語の配列(jp106)で、xkb のファイル(`/usr/share/X11/xkb`)から作ります(`window-placement.md` の「キーボード」)。

## テスト用の入力インターフェース

ウィンドウを動かす・閉じるの操作を、ピクセルのテストで確かめるための口です。`--test-input` を付けて起動したときだけ開きます。
ふだんの起動では開かず、標準入力も読みません。開いたときは、起動のときにログへ警告を出します。

開くと、標準入力の1行を、1つのポインターかキーのイベントとして扱います。

| 行 | 意味 |
| --- | --- |
| `motion X Y` | ポインターが、画面の上の(X, Y)へ動いた |
| `press [BUTTON]` | いまの位置で、ボタンが押された。`BUTTON` は `left` か `right`(無ければ `left`) |
| `release [BUTTON]` | いまの位置で、ボタンが離された |
| `leave` | ポインターが、画面の外へ出た |
| `key CODE press` | キー `CODE`(evdev のキーの番号。30 は A、42 は左の Shift)が押された |
| `key CODE release` | キー `CODE` が離された |
| `locked MASK` | 固定された修飾キー(Caps Lock、Num Lock)の状態が `MASK` になった。入れ子の構成で、親から届く wl_keyboard の modifiers の mods_locked と同じもの(jp106 では、Caps Lock が 2、Num Lock が 16) |
| `output WxH` | 出力(画面)の大きさが `W` × `H` になった。入れ子の構成で、親のウィンドウの大きさが変わったときと同じ道(フロントエンドの `resize_output`)を通る。fake screen のときだけ働く(fbdev の装置の大きさは変えない) |

- イベントは、入れ子の構成で親のWaylandから届くものと同じ道(フロントエンドの `pointer_input`、`keyboard_input`、`set_locked_modifiers`、`resize_output`)を通ります。raise、動かす、閉じる、サイズ変更、最大化・最小化、キーボードのフォーカス、クライアントへ渡す、の決まりは、同じものが働きます。
- 読めない行は、ログに出して捨てます。
- 標準入力は、別のスレッドで読みます(この口を開いたときだけ、スレッドが1本増えます)。標準入力が閉じられると、そのスレッドは終わり、Seinasは動き続けます。

引数にしたのは、テストする実行ファイルと、配る実行ファイルを、同じものにするためです(ビルドのときの切り替えにすると、テストしたものと別の実行ファイルを配ることになります)。

## 描く時機

fbdevには、垂直同期のイベントがありません。そこで、次のようにしています。

- クライアントがcommitすると、「再描画が要る」と覚えます。
- 再描画が要るときにだけ、1枚を合成して画面へ present します。
- 出した直後に、クライアントへframeコールバックを返します。クライアントは、その通知を受けて次の絵を描きます。
- 描く回数は、1秒に60回までです。前に描いてから1/60秒たっていなければ、その時刻まで待ってから描きます。
- 起動したときは、クライアントがいなくても、背景を1枚出します。
- 何も起きていない間は、何もせずに待ちます(定期的に目を覚ますことはしません)。

## 何もしていないとき

何もしていない間、コンポジタ・タスクバー・壁紙は、決まった間隔では起きません。何か起きるまで(クライアントの接続と要求、入れ子では親の Wayland の接続、テスト用の入力インターフェース)か、期限(閉じるように頼んだクライアントの pong)まで眠ります。IME の返事の期限は、2026-10-15 に無くしました(`ime.md` の「IME の復旧」)。入れ子の構成(`seinas`)も同じで、描く歩調は、親の frame コールバックで決まります。

測り方:`python3 tools/idle-cost.py [--seconds 60]`。ヘッドレスで、2 つの構成(standalone と、standalone を親にした入れ子)を起こし、タスクバー・壁紙・何もしないテスト用のウィンドウを 1 つつないで、3 秒待ってから測ります。

- CPU:`/proc/<pid>/stat` の utime + stime の差(release のビルド)。
- 起きる回数:`/proc/<pid>/task/*/status` の `voluntary_ctxt_switches` と `nonvoluntary_ctxt_switches` の差を、全部のスレッドで足したもの。眠っていたスレッドが起きるたびに 1 つ増えます(待ちの syscall(epoll_wait など)から戻った回数に、ほぼ同じ)。

2026-10-14 に、60 秒で測った値(WSL2、release のビルド):

| 構成 | プロセス | 前:CPU(ms / 60 秒) | 前:起きる回数(1 秒あたり) | 後:CPU(ms / 60 秒) | 後:起きる回数(1 秒あたり) |
| --- | --- | --- | --- | --- | --- |
| standalone | コンポジタ | 0 | 0.0 | 0 | 0.03 |
| standalone | タスクバー | 0 | 0.0 | 0 | 0.05 |
| standalone | 壁紙 | 0 | 0.0 | 0 | 0.00 |
| 入れ子 | コンポジタ(seinas) | 100 | 62.4 | 0 | 0.05 |
| 入れ子 | 親(seinas-standalone) | 10 | 0.0 | 0 | 0.03 |
| 入れ子 | タスクバー | 0 | 0.0 | 0 | 0.05 |
| 入れ子 | 壁紙 | 0 | 0.0 | 0 | 0.00 |

- 前の入れ子のコンポジタは、16 ミリ秒ごとに起きていました(pong と IME の期限を確かめるため)。今は、期限があるときだけ、その時刻に起きます。
- 残る回数は、時計の分の境のものです。タスクバーは、分の境でだけタイマーで起き(1 分に 1 回)、時計を再描画します。その絵を受けて、コンポジタが起きて composite し、タスクバーには frame コールバックと、前の絵のバッファーを返したことが届きます。合わせて、タスクバーが 1 分に約 3 回、コンポジタが約 2 回です。standalone の前の値(0.0)は、小数点以下 1 桁で丸めたものです。
- 壁紙は、出力の大きさが変わったときだけ描き直すので、何もしていない間は起きません。
- 何もしていないときに起きないことは、`tests/idle_wakeups.rs` で確かめています(standalone と入れ子のコンポジタを 3 秒見て、3 回以下)。

## 画面への反映

`seinas-fbdev` の `Presenter` を、そのまま使っています。今はLinux向けの `WriteThrough`(何もしない)だけです。
別のOS向けの反映は、`Presenter` を実装した型を足して、`src/screen.rs` で選ぶものを差し替えれば入ります。

## 今の制限

- 入力の装置は扱いません。そのため、ふだんの起動では、ウィンドウを raise する・動かす・閉じる操作ができません(テスト用の入力インターフェースからは、できます)。
- クライアントのウィンドウは、上にタイトルバーを付け、決まった位置に少しずつずらして重ねます(`window-placement.md`)。
- 止めるには、シグナルで終わらせます。終わるときのクリーンアップ(画面を元に戻す、ソケットとロックファイルを消す)はしていません。次に起動したときは、残ったロックファイルを取り直して、そのまま動きます。
- 実際の装置(`/dev/fb0`)では、まだ動かしていません。

## テスト

`crates/seinas-standalone/tests/with_client.rs` が、次を確かめます。親のWaylandも画面の装置も要らないので、CIで動きます。

1. fake screen(640x480)で起動すると、背景が1枚出て、ソケットで listen する。
2. zeyes-minをつなぐと、そのウィンドウが画面の左上に描かれる(タイトルバーの下に、コンテンツが来る)。ポインターは無いままです。
3. zeyes-minを強制終了すると、ウィンドウが消えて背景に戻り、クリーンアップのスレッドが1本できる。
4. もう一度つなぐと、同じように描かれる。スレッドは増えない。

描かれたかどうかは、決まった位置のピクセルの色で見ます(目のまわりの緑、白目、目の縁、瞳、ウィンドウの外の背景)。
待つ時間の上限は、段階ごとに60秒です。条件がそろい次第、すぐ先へ進みます。

タイトルバーは `crates/seinas-standalone/tests/title.rs` が、動かす・閉じるの操作は `crates/seinas-standalone/tests/move_close.rs` が、同じやり方で確かめます(中身は `window-placement.md` の「テスト」)。
テストは、`tools/fetch-fonts.sh` が置いたフォントを使います。

## 使うシステムコール

muslの静的ビルドを `strace -f` の下で動かし、起動、クライアントの接続、描画、強制終了、再接続、もう一度の強制終了までを記録しました(2026-10-04、Linux 6.18、fake screen 800x600)。
全体で230回です。

### コンポジタの側(seinas-standalone)

ソケット:

| 呼び出し | フラグなど |
| --- | --- |
| `socket(AF_UNIX, SOCK_STREAM\|SOCK_CLOEXEC, 0)` | listen 用。SOCK_NONBLOCKは付けない |
| `bind` | ソケットのパス |
| `listen(fd, -1)` | 待ち行列の長さは -1(OSの上限まで) |
| `ioctl(fd, FIONBIO, [1])` | listen のソケットを、待たない設定にする |
| `accept4(fd, ..., SOCK_CLOEXEC)` | SOCK_NONBLOCKは付けない。次の接続が無ければEAGAIN |
| `recvmsg(..., MSG_DONTWAIT\|MSG_CMSG_CLOEXEC)` | クライアントの要求を読む |
| `sendto(..., MSG_DONTWAIT\|MSG_NOSIGNAL, NULL, 0)` | クライアントへイベントを送る |
| `close` | 切れたクライアントのソケットを閉じる |

ロックファイル(listen の前に1回):
`open(O_RDWR|O_CREAT|O_TRUNC|O_CLOEXEC)`、`fcntl(F_SETFD, FD_CLOEXEC)`、`flock(LOCK_EX|LOCK_NB)`、`fstat`、`stat`(ロックファイルと、ソケットのパス)。

待ち:

| 呼び出し | フラグなど |
| --- | --- |
| `epoll_create1(EPOLL_CLOEXEC)` | 2つ作る(イベントループ用と、クライアントの listen 用) |
| `fcntl(F_DUPFD_CLOEXEC)` | 2つ目のepollを複製し、1つ目のepollに登録する(epollの入れ子) |
| `epoll_ctl` | ADD、MOD、DEL。EPOLLIN、EPOLLPRI、EPOLLERR、EPOLLHUP、EPOLLONESHOT |
| `epoll_pwait` | 期限は -1(無期限)か0。シグナルの指定はNULL |
| `eventfd2(0, EFD_CLOEXEC\|EFD_NONBLOCK)` | イベントループを起こすための口。毎周 `read` して、EAGAINが返る |
| `timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC\|TFD_NONBLOCK)` | 期限付きで待つため |
| `timerfd_settime(fd, 0, ...)` | 相対の時間。描く間隔を待つときに値が入る。それ以外は0(止める) |
| `poll` | 起動時に1回だけ。Rustの標準ライブラリが、0〜2番のfdを確かめる |

共有メモリー:

- コンポジタの側は、`memfd_create`、`ftruncate`、封印の `fcntl`、プールへの `fstat` を、一度も呼びません。
- プールのfdは、クライアントから受け取ります。受け取ったら `mmap(NULL, 大きさ, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)` でマップします。
- クリーンアップは、`munmap` と `close` です(クリーンアップのスレッドが呼びます)。

fdの受け渡し:

- 受け取りは `recvmsg` で、`SCM_RIGHTS` の付いた制御メッセージに、fdが1つ入っています(`MSG_CMSG_CLOEXEC` 付き)。
- コンポジタの側からfdを送ることはありません。`sendmsg` は一度も呼ばず、送信はすべて `sendto` です。

スレッド:

- 前回の調査(入れ子の構成)と同じでした。
- `clone` は1回で、最初のクライアントが去ったときです。フラグの組も同じです(CLONE_VM、FS、FILES、SIGHAND、THREAD、SYSVSEM、SETTLS、PARENT_SETTID、CHILD_CLEARTID、DETACHED)。
- スレッドのスタックは、`mmap`(2,109,440バイト、PROT_NONE)と `mprotect`(2,101,248バイト)です。
- `futex` は3回だけです。スレッドが待つ `FUTEX_WAIT_BITSET_PRIVATE`(期限なし)が2回、2つ目のプールを捨てるときの `FUTEX_WAKE_PRIVATE`(数は1)が1回です。
- 入れ子の構成で毎周出ていた `FUTEX_WAKE_PRIVATE`(数は2147483647)は、この構成では出ません。出どころのwayland-clientを含まないためです。

同時に開くfdの数:

| 状態 | 数 | 内訳 |
| --- | --- | --- |
| クライアントがいない | 10 | 0〜2番、epollが3つ(うち1つは複製)、eventfd、timerfd、ロックファイル、listen のソケット |
| クライアントが1つ | 12 | 上に加えて、クライアントのソケットと、プールのfd |

- 最大は12でした。クライアント1つにつき、ソケット1つと、プール1つごとにfdが1つ増えます。
- 装置を使うときは、装置のfdが1つ増えます。`--dump` を使うときは、書き出す間だけ1つ増えます。

そのほか:
`mmap`(匿名。ヒープと、800x600の絵2枚ぶん)、`munmap`、`brk`、`mprotect`、`rt_sigaction`、`rt_sigprocmask`、`sigaltstack`、`getrandom(16, GRND_INSECURE)`、`arch_prctl(ARCH_SET_FS)`、`set_tid_address`、`write`(ログ)。

### クライアントの側(zeyes-min)

M3では、クライアントも別のプロセスとして動きます。muslの静的ビルドの記録です。

| 観点 | 呼び出し |
| --- | --- |
| ソケット | `socket(AF_UNIX, SOCK_STREAM\|SOCK_CLOEXEC, 0)`、`connect` |
| 共有メモリー | `memfd_create("smithay-client-toolkit", MFD_CLOEXEC\|MFD_ALLOW_SEALING)`、`ftruncate(fd, 614400)`、`fcntl(fd, F_ADD_SEALS, F_SEAL_SEAL\|F_SEAL_SHRINK)`、`fstat`、`mmap(..., MAP_SHARED, fd, 0)` |
| fdの受け渡し | `fcntl(F_DUPFD_CLOEXEC)` で複製したfdを、`sendmsg` の `SCM_RIGHTS` で送る |
| 待ち | `epoll_create1`、`epoll_ctl`、`epoll_pwait`、`eventfd2`、`timerfd_create`、`timerfd_settime`、`ppoll`(最初の往復で1回) |
| スレッド | `clone` は無し。`futex(FUTEX_WAKE_PRIVATE, 2147483647)` を、ループの1周ごとに呼ぶ(待つ相手はいないので、戻り値は0) |

- 開くfdは8つでした。
- zeyes-minは、16ミリ秒ごとに目を覚ます作りなので、何も起きていなくても、待ちの呼び出しが続きます。

## muslでの静的ビルドの記録

`tools/build-musl.sh` は、seinas-standaloneも静的PIEとして作ります。`tools/elf-report.sh` の出力です(2026-10-05。タイトルバーと、動かす・閉じるの操作を組み込んだ後)。

```
== seinas-standalone
file size: 7610096 bytes
  Type:                              DYN (Position-Independent Executable file)
  Machine:                           Advanced Micro Devices X86-64
PT_INTERP: 0, DT_NEEDED: 0
PT_TLS: FileSiz 0x000098, MemSiz 0x000308, Align 0x10
Program Headers:
  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align
  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x054458 0x054458 R   0x1000
  LOAD           0x055000 0x0000000000055000 0x0000000000055000 0x40d1b6 0x40d1b6 R E 0x1000
  LOAD           0x463000 0x0000000000463000 0x0000000000463000 0x102364 0x102364 R   0x1000
  LOAD           0x565440 0x0000000000566440 0x0000000000566440 0x045120 0x046f60 RW  0x1000
  DYNAMIC        0x5a5260 0x00000000005a6260 0x00000000005a6260 0x000180 0x000180 RW  0x8
  NOTE           0x000270 0x0000000000000270 0x0000000000000270 0x000024 0x000024 R   0x4
  TLS            0x565440 0x0000000000566440 0x0000000000566440 0x000098 0x000308 R   0x10
  GNU_EH_FRAME   0x4e92b8 0x00000000004e92b8 0x00000000004e92b8 0x00dd64 0x00dd64 R   0x4
  GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x10
  GNU_RELRO      0x565440 0x0000000000566440 0x0000000000566440 0x043bc0 0x043bc0 R   0x1
```

- `ET_DYN` の静的PIEで、`PT_INTERP` も `DT_NEEDED` もありません。
- `PT_TLS` は、スレッドごとに0x308バイト(776バイト、16バイト境界)です。動かす・閉じるの操作と、テスト用の入力インターフェースを組み込む前は、0x2f8バイト(760バイト)でした。
- メモリーに置いたときのイメージの大きさは0x5ad3a0(約5.68 MiB)です。
- 再配置は `R_X86_64_RELATIVE` だけで、14352個です。
- 実行ファイルの大きさは7,610,096バイト、stripすると5,942,576バイトです。

大きさの移り変わり(バイト):

| 時点 | 実行ファイル | stripしたとき |
| --- | --- | --- |
| 文字を描く部分を組み込む前 | 3,952,864 | 3,017,424 |
| タイトルバーを組み込んだ後 | 7,227,488 | 5,675,920 |
| 動かす・閉じるの操作と、テスト用の入力インターフェースを組み込んだ後 | 7,610,096 | 5,942,576 |

文字を描く部分で増えたぶんの中身は、`text-rendering.md` にあります。

- YMM・ZMMを使う命令と `xgetbv` は、0個です。
- fake screen 800x600で、zeyes-minを1つつないだときの最大RSSは6,148 kB、主スレッドのスタックで触れた量は20 kBでした。
