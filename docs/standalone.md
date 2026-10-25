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

ZeikOS の上では、release の `seinas-standalone`(コンポジタ)と `seinas-taskbar`(タスクバー)を、変更を入れずに、そのまま動かします(`docs/release.md` の「seinas-standalone の release」。v0.3.0 から、タスクバーも入る)。

### 置くファイル(v0.3.0)

release `v0.3.0` の成果物です。大きさと SHA-256 は、release の `SHA256SUMS` と同じです(2026-10-25 に、`31f907d` で `tools/release-standalone.sh v0.3.0` を動かして作った。CI の musl のビルドと同じバイト)。

| release の上の名前 | 置き場(ZeikOS のイメージ) | 大きさ(バイト) | SHA-256 |
| --- | --- | --- | --- |
| `seinas-standalone` | `/bin/linux/seinas-standalone` | 9,095,904 | `a4df394e1fcbd5fb2c2a490666e8de7434149582dda9e38b92e30beaccd55044` |
| `seinas-standalone-v0.3.0-third-party.tar.gz` | 実行ファイルと一緒に(第三者のライセンス文と著作権表示) | 67,997 | `1fff0478bc19d285839eff17f61b08981bc0e40e88ae530aeff7fba49b44ea2a` |
| `seinas-taskbar` | `/bin/linux/seinas-taskbar` | 4,966,232 | `4328035d9d04a9e1acbdda0471d54771b522f77c3cf67ee131bb8073b13c37a2` |
| `seinas-taskbar-v0.3.0-third-party.tar.gz` | 実行ファイルと一緒に(第三者のライセンス文と著作権表示) | 57,048 | `2db60ce438fd4b7dffca5846f8f36e798ae73979e64b47166086553fa042704b` |

- 置き場の名前は、v0.2.0 と同じく、ZeikOS の側が決めます(上の表の置き場は、v0.2.0 の `/bin/linux/seinas-standalone` に合わせた例)。取り込みは ZeikOS の側が行います。
- ほかに置くものはありません。フォントと xkb のファイルは置きません(下の「フォントと xkb」)。
- ZeikOS は、ソケットとロックファイルの置き場の `/tmp/seinas` を、イメージに作ってあります。

### 起動のしかた

zash で、タスクバーを `a | b` の左に、コンポジタを右に置いて起動します。ZeikOS では `/dev/fb0` を前景のプロセスしか開けないので、コンポジタを前景(右側)にします。左側のタスクバーが先に起き、コンポジタのソケットができるまで待ってつなぎます(`docs/taskbar.md` の「起動の順序」)。

```text
export WAYLAND_DISPLAY=/tmp/seinas/s-shell.sock
/bin/linux/seinas-taskbar | /bin/linux/seinas-standalone --socket /tmp/seinas/s.sock --no-ime-socket
```

- `WAYLAND_DISPLAY` は、タスクバーがつなぐ、シェルの部品のためのソケットです。絶対パスにしているので、`XDG_RUNTIME_DIR` は使いません。相対の名前(`s-shell.sock`)でも、ZeikOS の `/etc/environment` の `XDG_RUNTIME_DIR=/tmp/seinas` の下になるので同じです。
- コンポジタは、`--socket` で場所を決めるので、`WAYLAND_DISPLAY` と `XDG_RUNTIME_DIR` を読みません。シェルの部品のためのソケットは、`--socket` のパスから `/tmp/seinas/s-shell.sock` になります。
- `--no-ime-socket` で、IME のためのソケットを開きません(IME を使わない構成。fd が 2 本減る)。コンポジタの fd は、タスクバーがつながって 15 本です(`docs/zeikos-requirements.md` の「fd の種類」)。
- パイプは、起動の順序を決めるためだけに使います。タスクバーは標準出力に何も書かず、コンポジタは標準入力を読みません(`--test-input` を付けないため)。ログはどちらも標準エラーです。
- どちらも `--fonts` と `--config` を付けません。タスクバーの時計は、既定の +09:00 です(`TZ` も設定のファイルも無いため)。

### フォントと xkb

- フォントは置きません。同梱のフォントは 3 本とも 4 MiB を越え、今の ZeikOS の ext2 では読めないためです(`docs/zeikos-requirements.md` の「4 MiB を越えるファイル」)。
- フォントが無くても、どちらも動きます(2026-10-24 に、Linux で、フォントの置き場が無く、作業中のディレクトリが `/` の形で確かめた)。
  - コンポジタ:ウィンドウのタイトルの文字を描きません。起動のときに、フォントを読めないという WARN の行を出します(タスクバーと同じく、既定の置き場が相対のパスのため)。
  - タスクバー:地の色とボタンの地は描き、ボタンの文字と時計の文字を描きません。起動のときに、`cannot read the font target/fonts/BIZUDPGothic-Regular.ttf: …` と `cannot read the font target/fonts/unifont_jp-18.0.01.otf: …` の 2 行を出します(既定の置き場は、作業中のディレクトリから見た `target/fonts` で、ZeikOS は相対のパスの open を断る)。
- xkb のファイルは置きません。コンポジタは、ふだんの起動(`--test-input` なし)ではキーボードを作らず、タスクバーはキーボードを使わないので、どちらも xkb のファイルを読みません。

### 起動が済んだことの見分け方

- コンポジタ(標準エラー):
  - `listening on /tmp/seinas/s.sock`、`listening on /tmp/seinas/s-shell.sock`、`the socket for the input method is not opened (--no-ime-socket)`
  - `screen: 1280x800 (…)`(大きさは、ZeikOS の画面のもの)
  - `presented the first frame (1280x800)`(最初の 1 枚を画面へ present し終えたときに、1 回だけ。target は `seinas_standalone`)
- タスクバー(標準エラー。`seinas-taskbar:` で始まる):
  - `the clock is UTC+09:00 (from the default)` と、上のフォントの 2 行
  - コンポジタより先に起きたとき:`the compositor is not ready at /tmp/seinas/s-shell.sock (ECONNREFUSED: …); trying again every 100 ms for up to 30000 ms`、つないだら `connected to the compositor after <回数> tries`
  - 最初の往復の答えを待ったとき:`connected; waiting up to 30000 ms …` と `the compositor answered the first round trip`
- 画面:背景の色(`191e28`)の下の端に、高さ 40 ピクセルのタスクバー(地の色 `20242c`)が出ます。
- タスクバーがつなげないときは、30 秒(接続)か 30 秒(最初の往復)で、名前つきのエラー(`the compositor did not appear …`、`the compositor did not answer the first round trip …`)で終わります。

### ZeikOS の側に要るもの(タスクバー)

コンポジタだけ(v0.2.0)のときに無かったもので、タスクバーをつなぐと要るものです。詳しくは `docs/zeikos-requirements.md` と `docs/taskbar.md` の「ZeikOS の上で動かすときの前提」にあります。

- `ppoll`(271 番。timeout の timespec と sigmask は NULL):wayland-client の registry の往復で 1 回使います。
- `poll`(7 番):最初の往復を、期限つきで待つのに使います(timeout は 0 と、期限の残りのミリ秒)。
- 共有メモリー:タスクバーの pool は、`ftruncate(fd, 256000)` で始まり、2 枚目のバッファを取ると `ftruncate(fd, 512000)` に伸びます(1280 の幅で、1 枚 204,800 バイト)。コンポジタは、伸ばしたときに、前のマッピングを `munmap` してから `mmap` し直します(Smithay 0.7.0 は mremap を使わない)。
- `clock_gettime(CLOCK_REALTIME)`(時計)と `clock_nanosleep(CLOCK_MONOTONIC)`(つなぐのを待つ間)。

### 止め方

前面のコンポジタを、ZeikOS の手段(Ctrl+C)で止めます。タスクバーは、接続が切れて `the connection to the compositor was lost` で終わります。終わるときのクリーンアップ(ソケットのファイルとロックファイルを消す、など)はしていません(下の「今の制限」)。

コンポジタだけを起動するときは、今までどおり `seinas-standalone --socket /tmp/seinas/s.sock`(v0.2.0 の形)でも動きます。

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
