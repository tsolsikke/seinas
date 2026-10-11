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
| `--utc-offset ±HH:MM` | 時計の時差。無ければ環境変数 `TZ` の簡単な形、それも無ければ `+09:00`(下の「時計」) |

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

最小化と、その戻しは、コンポジタがまだ扱っていません(頼みは受けますが、何もしません)。ウィンドウの最大化・最小化を入れるときに働くようになります。

## 時計

- 時:分を 24 時間で出します(`12:34`)。
- 時刻は、壁の時計(`clock_gettime(CLOCK_REALTIME)`)から読みます。std の `SystemTime::now` は、失敗すると panic するので使いません。
- 時刻が取れないときは、`--:--` を出します。取れないときとは、`clock_gettime` が失敗したときと、2020-01-01 より前を指しているとき(合わせられていない壁の時計)です。ZeikOS には、まだ壁の時計が無いので、今はこの表示になる見込みです。
- 時差は、`--utc-offset` があればそれ、無ければ環境変数 `TZ` の、夏時間の無い簡単な形(`JST-9`、`UTC0`、`<+09>-9` など)、それも無ければ `+09:00` です。タイムゾーンのデータベース(`/usr/share/zoneinfo`)は読みません(ZeikOS に無いため)。`Asia/Tokyo` のような名前は読めないので、`+09:00` になります。
- 分が変わった少し後(5 ミリ秒後)に起きて、再描画します。時刻が取れないときは、60 秒ごとに読み直します。起きるのには、イベントループ(calloop)のタイマーを使います(timerfd が 1 つ)。

## ZeikOS の上で動かすときの前提

musl で静的にリンクした `seinas-taskbar` を、`seinas-standalone` と zeyes-min と一緒に動かして、strace で測りました(`docs/m3-syscalls.md` の「変更ごとの差分の記録」)。

- プロセスは 1 つ、スレッドも 1 つです。
- fd は、標準入出力の 3 つのほかに 5 つです:接続(Unix のソケット)、epoll、eventfd、timerfd、pool の memfd。ウィンドウの数が増えても変わりません。フォントのファイルは、読み込んだら閉じます(使うのは、UI のフォントとフォールバックのフォントの 2 つ)。
- syscall の種類は 31 で、zeyes-min の種類に `open`(フォント)と `ppoll`(つないだ直後の往復を待つ)が加わります。`clock_gettime` は、Linux では vDSO で済むので strace に出ませんが、ZeikOS で vDSO が無ければ、`CLOCK_REALTIME` と `CLOCK_MONOTONIC` の syscall が要ります。
- コンポジタの側では、タスクバーは、ほかのシェルの部品と同じです(接続と pool の memfd で fd が 2 つ増える)。

## テスト

- 並べ方、押した点からのボタンの選び方、押したときに頼むこと、時計の表示と次に起きるまでの時間、時差の読み方は、`crates/seinas-taskbar` のユニットテストで確かめています。
- 絵は、正解データ(`crates/seinas-taskbar/tests/golden.rs`)と比べます(`docs/golden.md`)。
- 実際に動かして、`crates/seinas-standalone/tests/taskbar.rs` が、ピクセルで確かめます(画面の下にあって、configure の大きさが小さくなること、ウィンドウのボタンが並んで activated のものが分かること、押すと raise されること、閉じたウィンドウのボタンが消えること、ふだんのソケットではつなげないこと)。
