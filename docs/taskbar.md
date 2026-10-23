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
| `--connect-tries N` | コンポジタのソケットがまだ無いときに、最初の接続を試す回数(最初の 1 回を含む。既定は 300。1 なら試し直さない)。下の「起動の順序」。テストで短くするためのもの |

## 起動の順序

タスクバーは、コンポジタより先に起こされても動きます。ZeikOS では、`/dev/fb0` を前景のプロセスしか開けないので、コンポジタを前景(zash の `a | b` の右側)にし、タスクバーを先に切り離して起動する側(左側)にします。タスクバーが先に起きるので、コンポジタのソケットがまだ無いことがあります(運用者の決定。2026-10-19)。

- 最初の接続で、ソケットが無い(`ENOENT`)か、listen する相手がいない(`ECONNREFUSED`)か、接続のテーブルが満杯(`EAGAIN`)のときは、100 ms 眠ってから試し直します。合わせて 300 回(約 30 秒)まで試します。眠るのは `std::thread::sleep`(musl の静的ビルドでは、`clock_nanosleep(CLOCK_MONOTONIC, 0, …)`。230 番。ZeikOS も受ける)で、待つ間に CPU は使いません。
  - `EAGAIN` は ZeikOS の connect が、接続のテーブルに空きが無いときに返すものです。Linux の blocking connect は、相手の backlog が満杯なら待つので、試し直して Linux の「待つ」に近づけます(回数の上限があるので、いつまでも待つことはない)。
  - 1 回目に失敗したときに、ログに `the compositor is not ready at <パス> (<errno の名前>: <理由>); trying again every 100 ms, up to 300 tries` を出します。つなげたら `connected to the compositor after <回数> tries` を出します(1 回目でつなげたときは出さない)。
  - 300 回試してもつなげなければ、`the compositor did not appear at <パス>: gave up after 300 tries (<最後の理由>)` のエラーで終わります(exit status 1)。
  - ほかの理由の失敗(パスが長すぎる、権限が無いなど)と、`WAYLAND_DISPLAY` が無い・相対の名前なのに `XDG_RUNTIME_DIR` が無いときは、試し直さずに、すぐにエラーで終わります。`WAYLAND_SOCKET`(親から渡された接続)のときも、試し直しません。
- 間隔を 100 ms にした理由:コンポジタのソケットができてから、タスクバーが出るまでの遅れを 0.1 秒までにするためです。1 回の試しは `socket` と `connect` の 2 つの syscall で、1 秒に 10 回なら、ほかのプロセスの邪魔になりません。
- 上限を約 30 秒にした理由:コンポジタは、起きてからソケットを作るまでに、実行ファイル(約 9 MB)の読み込みと画面の準備をします。QEMU の上の ZeikOS ではこれに何秒かかるか測っていないので、十分に長くしました。一方、`WAYLAND_DISPLAY` の間違いやコンポジタの起動の失敗のときに、いつまでも待たずにエラーで終わるため、上限を置きます。
- 試し直すのは、最初の接続だけです。一度つないだ後に接続が切れたときは、これまでどおり、`the connection to the compositor was lost` のエラーで終わります(つなぎ直さない)。

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
- syscall の種類は 31 で、zeyes-min の種類に `open`(フォント)と `ppoll`(つないだ直後の往復を待つ)が加わります。コンポジタより先に起きて試し直すときは、`clock_nanosleep` が加わります(上の「起動の順序」。`socket`・`connect`・`close` は、つなぐときにも使うもの)。`clock_gettime` は、Linux では vDSO で済むので strace に出ませんが、ZeikOS で vDSO が無ければ、`CLOCK_REALTIME` と `CLOCK_MONOTONIC` の syscall が要ります。
- コンポジタの側では、タスクバーは、ほかのシェルの部品と同じです(接続と pool の memfd で fd が 2 つ増える)。

## テスト

- 並べ方、押した点からのボタンの選び方、押したときに頼むこと、時計の表示(既定の +09:00 と UTC)と次に起きるまでの時間、時差の読み方(範囲、15 分刻み、読めない値)、設定のファイル、時差を決める順は、`crates/seinas-taskbar` のユニットテストで確かめています。
- 絵は、正解データ(`crates/seinas-taskbar/tests/golden.rs`)と比べます(`docs/golden.md`)。
- 実際に動かして、`crates/seinas-standalone/tests/taskbar.rs` が、ピクセルで確かめます(画面の下にあって、configure の大きさが小さくなること、ウィンドウのボタンが並んで activated のものが分かること、押すと raise されること、閉じたウィンドウのボタンが消えること、ふだんのソケットではつなげないこと、時計の時差が設定のファイルと起動オプションで変わり、読めない値では既定で動くこと、コンポジタより先に起こしても、1 回目の接続に失敗した後に起こしたコンポジタへつなぐこと、上限まで試してもつなげなければ `the compositor did not appear` で終わること)。ソケットのパスの決め方と、試し直す失敗の種類は、`crates/seinas-taskbar` のユニットテストで確かめています。
