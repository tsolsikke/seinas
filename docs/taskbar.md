# タスクバー(seinas-taskbar)

Seinas のタスクバーは、コンポジタとは別のプログラム(シェルの部品)です(`docs/adr/0002-shell-components.md`)。
画面の下に layer surface(wlr-layer-shell)を出し、wlr-foreign-toplevel-management でウィンドウの一覧を受け取って、ボタンを並べます。右の端に時計を出します。

## 動かし方

シェルの部品のためのソケットへつなぎます。ふだんのソケットには layer-shell と一覧の global が無いので、つなげずに止まります。

```bash
WAYLAND_DISPLAY=seinas-0-shell cargo run --locked -p seinas-taskbar
```

| 引数 | 中身 |
| --- | --- |
| `--fonts DIR` | フォントの置き場。無ければ環境変数 `SEINAS_FONTS`、それも無ければ `target/fonts`(コンポジタと同じ) |
| `--font-config PATH` | フォントの設定のファイル。無ければ `$XDG_CONFIG_HOME/seinas/fonts.conf`(`docs/text-rendering.md` の「利用者が選んだフォント」)。コンポジタと同じファイルを読む |
| `--utc-offset ±HH:MM` か `UTC` | 時計の時差。設定のファイルより優先する(下の「時計」) |
| `--config PATH` | タスクバーの設定のファイル。無ければ `$XDG_CONFIG_HOME/seinas/taskbar.conf`(`XDG_CONFIG_HOME` が無ければ `~/.config/seinas/taskbar.conf`) |
| `--connect-timeout MS` | コンポジタのソケットがまだ無いときに、最初の接続を試し直す期限(ミリ秒。既定は 30000)。下の「起動の順序」。テストで短くするためのもの |
| `--round-trip-timeout MS` | つないだ後の最初の往復の答えを待つ期限(ミリ秒。既定は 30000)。下の「起動の順序」。テストで短くするためのもの |

## 起動の順序

タスクバーは、コンポジタより先に起こされても動きます。ZeikOS では、`/dev/fb0` を前景のプロセスしか開けないので、コンポジタを前景(zash の `a | b` の右側)にし、タスクバーを先に切り離して起動する側(左側)にします。タスクバーが先に起きるので、コンポジタのソケットがまだ無いことがあります(運用者の決定。2026-10-19)。

起動は、次の 3 つの段階で進みます。待つ期限は、どれも単調増加の時計(`std::time::Instant`。Linux と ZeikOS では `CLOCK_MONOTONIC`)で判定します。

| 段階 | 待つもの | 期限(既定) | 期限を越えたとき |
| --- | --- | --- | --- |
| 1. 接続 | `connect` が通ること | 最初の試しから 30 秒(`--connect-timeout`) | `the compositor did not appear at <パス> within 30000 ms (<errno の名前>: <理由>; tried <回数> times)` |
| 2. 最初の往復 | 同じ接続での `wl_display.sync` の答え | つないだ時から 30 秒(`--round-trip-timeout`) | `the compositor did not answer the first round trip within 30000 ms (the initialization timed out)` |
| 3. registry の往復 | wayland-client の `registry_queue_init` の往復 | 無い(下の「限り」) | — |

全体の最大の時間は、1 と 2 を合わせた約 60 秒です(2 の答えがあれば、3 はすぐに済む)。どの段階のエラーでも、exit status 1 で終わります。

- 1. 接続:ソケットが無い(`ENOENT`)か、listen する相手がいない(`ECONNREFUSED`)か、接続のテーブルが満杯(`EAGAIN`)のときだけ、100 ms 眠ってから試し直します。眠るのは `std::thread::sleep`(musl の静的ビルドでは、`clock_nanosleep(CLOCK_MONOTONIC, 0, …)`。230 番。ZeikOS も受ける)で、待つ間に CPU は使いません。期限の近くでは、残りの時間だけ眠ります。
  - `EAGAIN` は ZeikOS の connect が、接続のテーブルに空きが無いときに返すものです。Linux の blocking connect は、相手の backlog が満杯なら待つので、試し直して Linux の「待つ」に近づけます(期限があるので、いつまでも待つことはない)。ZeikOS では、コンポジタが起きる前のソケットの名前は `ECONNREFUSED` になります(ソケットの名前はカーネルの表にあり、ファイルを作らないため)。
  - 1 回目に失敗したときに、ログに `the compositor is not ready at <パス> (<errno の名前>: <理由>); trying again every 100 ms for up to 30000 ms` を出します。つなげたら `connected to the compositor after <回数> tries` を出します(1 回目でつなげたときは出さない)。
  - ほかの理由の失敗(パスが長すぎる、権限が無いなど)と、`WAYLAND_DISPLAY` が無い・相対の名前なのに `XDG_RUNTIME_DIR` が無いときは、試し直さずに、すぐにエラーで終わります。`WAYLAND_SOCKET`(親から渡された接続)のときも、試し直しません。
- 2. 最初の往復:つないだ後、同じ接続で `wl_display.sync` を送り、答え(`done`)を待ちます。待つのは、wayland-client の `prepare_read` と、期限つきの `poll`(7 番。ZeikOS も受ける)です。コンポジタが listen していても、まだ accept していないか、止まっているときは、ここで待ちます。
  - すぐに答えが無いときに、ログに `connected; waiting up to 30000 ms for the compositor to answer the first round trip` を出し、答えが届いたら `the compositor answered the first round trip` を出します。
- 切断・プロトコルエラー:2 と 3 の途中で接続が切れるか、プロトコルエラーが届いたときは、段階と元の原因を書いて、すぐに終わります。試し直しません。
  - 例:`the connection to the compositor failed during the first round trip: Io error: Broken pipe (os error 32)`、`the connection to the compositor failed during the first round trip: protocol error 3 on wl_display@1: <メッセージ>`。3 の段階では `during the registry round trip` になります。
  - この行の前に、wayland-backend が同じ原因を 1 行(`Io error: …` など)出します(log の機能を使わないビルドで、エラーを標準エラーへ書くため)。
- 試し直すのは、1 の接続だけです。一度つないだ後に接続が切れたときは、つなぎ直しません(コンポジタの起動し直しへの対応は、別に決める)。動いている間に切れたときは、これまでどおり `the connection to the compositor was lost` のエラーで終わります。
- 間隔を 100 ms にした理由:コンポジタのソケットができてから、タスクバーが出るまでの遅れを 0.1 秒までにするためです。1 回の試しは `socket` と `connect` の 2 つの syscall で、1 秒に 10 回なら、ほかのプロセスの邪魔になりません。
- 期限を、どちらも 30 秒にした理由:コンポジタは、起きてからソケットを作るまでに、実行ファイル(約 9 MB)の読み込みと画面の準備をし、その後に最初の 1 枚を描いてから、クライアントの要求を処理します。QEMU の上の ZeikOS ではこれに何秒かかるか測っていないので、仮に十分に長くしました。ZeikOS の上で、タスクバーをつなぐときに測って見直します。一方、`WAYLAND_DISPLAY` の間違いや、コンポジタの起動の失敗・止まったときに、いつまでも待たずにエラーで終わるため、期限を置きます。
- 限り:3 の `registry_queue_init` は、中で `Connection::roundtrip` をブロッキングで呼ぶので、Seinas の側から期限をかける口がありません(wayland-client 0.31.15。smithay-client-toolkit 0.21.1 の初期化の手続きには、ほかに往復はない)。2 で答えたコンポジタが、3 の前に止まったときだけは、待ち続けます。また、3 の往復は `ppoll`(271 番)を使うので、ZeikOS には `ppoll` が要ります(`docs/zeikos-requirements.md`)。

## 見た目と並べ方

- 高さは 40 ピクセルで、画面の下の辺と左右の辺に anchor し、exclusive zone も 40 にします。通常のウィンドウは、その上の使える範囲に置かれます(`docs/window-placement.md`)。層は top で、キーボードの入力は受けません(`keyboard_interactivity` は none)。
- ボタンは、左の端から、一覧の順(ウィンドウを map した順)に、同じ幅で並べます。幅は 200 ピクセルまでで、入りきらないときは、そろって狭くなります。ボタンの高さは 32 で、上下に 4 の空きがあります。
- ボタンには、ウィンドウのタイトル(無ければ app_id)を 14 ピクセルの文字で出します。収まらなければ、末尾を切って「…」を付けます。
- activated のウィンドウのボタンは、地の色を明るくし、下にアクセントの色の長い印を付けます。ほかの開いているウィンドウのボタンには、短い灰色の印を付けます。最小化しているウィンドウのボタンには印を付けず、タイトルを薄くします。
- ポインターが乗っているボタンは、地の色を少し明るくします。
- 文字は、コンポジタと同じ既定のフォントと `seinas-text` で描きます。フォントが読めなくても止まらず、文字の無いボタンを並べます。

## ボタンを押したとき

左のボタンで押したときに、コンポジタへ頼むことは、Windows のタスクバーと同じです。

| ウィンドウ | 頼むこと |
| --- | --- |
| 最小化している | 最小化を戻して(unset_minimized)、activate する |
| activated でない | activate する(コンポジタが raise して、activated にする) |
| activated | 最小化する(set_minimized) |

最小化したウィンドウは、コンポジタが描かなくなり、ボタンは印が消えてタイトルが薄くなります。戻すと、そのウィンドウが raise されて activated になります(`docs/window-placement.md` の「最大化・最小化」)。タスクバーがつながっている間は、ウィンドウのタイトルバーにも最小化のボタンが出ます。

## 時計

- 時:分を 24 時間で出します(`12:34`)。
- 時刻は、壁の時計(`clock_gettime(CLOCK_REALTIME)`)から読みます。std の `SystemTime::now` は、失敗すると panic するので使いません。
- 時刻が取れないときは、`--:--` を出します。取れないときとは、`clock_gettime` が失敗したときと、2020-01-01 より前を指しているとき(合わせられていない壁の時計)です。ZeikOS には、まだ壁の時計が無いので、今はこの表示になる見込みです。
- 時差は、次の順に決めます。起動のときに、決めた時差と、どこから決めたかをログに出します(`the clock is UTC+09:00 (from the default)` など)。
  1. 起動オプション `--utc-offset`
  2. 設定のファイル `taskbar.conf` の `utc-offset` の行
  3. 環境変数 `TZ` の、夏時間の無い簡単な形(`JST-9`、`UTC0`、`<+09>-9` など)
  4. 既定の `+09:00`(日本時間。今までの見た目を変えない)
- 設定のファイルは、フォントの設定と同じ 1 行に `キー = 値` の形式です(読み方は `crates/seinas-config`)。日本時間から UTC へ切り替えるには、次の 1 行を書いて、タスクバーを起動し直します。

  ```text
  utc-offset = UTC
  ```

- 書ける値は `UTC`(`+00:00` と同じ)か、`+HH:MM`・`-HH:MM`(`+9` のように時だけでもよい)です。範囲は `-12:00` から `+14:00` まで、分は 0・15・30・45 だけです(例:`+00:00`、`-05:30`、`+05:45`)。
- 読めない値(範囲の外、15 分刻みでない、形が違う)は、ログに出して、無いものとして扱います(次の順のものか、既定の `+09:00` で動く)。起動オプションの読めない値も同じです。知らないキーも、ログに出して飛ばします。
- 地域の名前(`Asia/Tokyo` など)と夏時間は扱いません。タイムゾーンのデータベース(tzdata、`/usr/share/zoneinfo`)が ZeikOS に無いためです。夏時間のある地域では、夏と冬で `utc-offset` を書き換えてください。
- 既定の時差は +09:00 で、`crates/seinas-taskbar/src/lib.rs` の定数 `DEFAULT_UTC_OFFSET` の 1 か所で決めています。WSL には TZ が無いことが多く、UTC にすると、入れ子で見たときに 9 時間ずれるためです。
- 分が変わった少し後(5 ミリ秒後)に起きて、再描画します。時刻が取れないときは、60 秒ごとに読み直します。起きるのには、イベントループ(calloop)のタイマーを使います(timerfd が 1 つ)。

## ZeikOS の上で動かすときの前提

musl で静的にリンクした `seinas-taskbar` を、`seinas-standalone` と zeyes-min と一緒に動かして、strace で測りました(`docs/m3-syscalls.md` の「変更ごとの差分の記録」)。

- プロセスは 1 つ、スレッドも 1 つです。
- fd は、標準入出力の 3 つのほかに 5 つです:接続(Unix のソケット)、epoll、eventfd、timerfd、pool の memfd。ウィンドウの数が増えても変わりません。フォントのファイルは、読み込んだら閉じます(使うのは、UI のフォントとフォールバックのフォントの 2 つ)。
- syscall の種類は 31 で、zeyes-min の種類に `open`(フォント)と `ppoll`(つないだ直後の往復を待つ)が加わります。コンポジタより先に起きて試し直すときは、`clock_nanosleep` が加わります(上の「起動の順序」。`socket`・`connect`・`close` は、つなぐときにも使うもの)。最初の往復を待つ `poll`(7 番。`poll(fd, 1, 0)` と、待つときは期限のミリ秒)も加わります(2026-10-24 に、musl のビルドを strace で確かめた。registry の往復の `ppoll` は 1 回のまま)。`clock_gettime` は、Linux では vDSO で済むので strace に出ませんが、ZeikOS で vDSO が無ければ、`CLOCK_REALTIME` と `CLOCK_MONOTONIC` の syscall が要ります。
- コンポジタの側では、タスクバーは、ほかのシェルの部品と同じです(接続と pool の memfd で fd が 2 つ増える)。

## テスト

- 並べ方、押した点からのボタンの選び方、押したときに頼むこと、時計の表示(既定の +09:00 と UTC)と次に起きるまでの時間、時差の読み方(範囲、15 分刻み、読めない値)、設定のファイル、時差を決める順は、`crates/seinas-taskbar` のユニットテストで確かめています。
- 絵は、正解データ(`crates/seinas-taskbar/tests/golden.rs`)と比べます(`docs/golden.md`)。
- 実際に動かして、`crates/seinas-standalone/tests/taskbar.rs` が、ピクセルで確かめます(画面の下にあって、configure の大きさが小さくなること、ウィンドウのボタンが並んで activated のものが分かること、押すと raise されること、閉じたウィンドウのボタンが消えること、ふだんのソケットではつなげないこと、時計の時差が設定のファイルと起動オプションで変わり、読めない値では既定で動くこと、コンポジタより先に起こしても、1 回目の接続に失敗した後に起こしたコンポジタへつなぐこと、期限まで試してもつなげなければ `the compositor did not appear` で終わること、最初の往復の答えが期限内に遅れて届けば起動し、届かなければ `the initialization timed out` で終わること、往復の途中の切断とプロトコルエラーは段階と原因を書いて試し直さずに終わること)。答えを遅らせるのは、コンポジタを `SIGSTOP` で止める形で、タスクバーが待ち始めたことをログで確かめてから `SIGCONT` で動かし直します(時間には頼らない)。ソケットのパスの決め方と、試し直す失敗の種類は、`crates/seinas-taskbar` のユニットテストで確かめています。
