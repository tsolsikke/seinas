# C1S3 の入れ子での確かめ(チェックリスト)

C1S3 で入った機能を、入れ子の構成(WSLg)で、1 回の起動でまとめて確かめるための手順です。上から順に行えば、全部を確かめられます。操作は、どれも物理的なキーボードとマウスで行います。Windows の側への入力の送り込みや、ウィンドウの操作の自動化は使いません(`CLAUDE.md` の決まり 1)。

## 起こし方

WSLg の端末で、リポジトリの先頭から次を動かします(ビルドと、フォントの取得(`tools/fetch-fonts.sh`)を先に済ませておく)。

```bash
tools/demo-c1s3.sh
```

- 起こすもの:入れ子の Seinas、壁紙、タスクバー、テスト用のウィンドウ 3 つ、テスト用の IME。
  - 青「キーボード」:キーボードとポインターのイベントを書く(`window-keyboard.log`)。
  - 赤「応答なし(ping に答えない)」:ping に答えず、閉じるように頼まれても終わらない(`window-unresponsive.log`)。
  - 橙「文字入力(text-input)」:text-input-v3 を使い、届いた変換中の文字と確定した文字を書く(`window-text.log`)。
  - テスト用の IME:`a`・`i`・`u` を「あ」「い」「う」に、Enter で確定する。候補ウィンドウは紫の四角(`ime.log`)。
- 設定は一時的な `XDG_CONFIG_HOME` に置きます。`~/.config` は読まず、書き換えません。
  - フォント:タイトルとタスクバーの文字を BIZ UDゴシック にする(既定は BIZ UDPゴシック)。
  - 壁紙:テスト用の小さな画像(48x32)を中央に置き、余白を緑(`2f6f4f`)にする。
  - 時計:`utc-offset = +09:00`。
  - 切り替えのキー:入れ子の既定と同じ(Alt+Tab と Alt+F6)。変えるときは、`DEMO_SEINAS_CONF=<ファイル> tools/demo-c1s3.sh` で、`seinas.conf` の代わりのファイルを渡します(書き方は `docs/config.md`)。
- `DEMO_LOG_KEYS=1 tools/demo-c1s3.sh` で起こすと、Seinas に `--log-keys` を付け、届いたキーを `seinas.log` に書きます(どのキーが届くかを確かめるときだけ。ふだんは付けません)。下の表の `key:` の行は、このときだけ出ます。
- ログは、起動のときに表示する場所(`/tmp/seinas-demo-logs-<日時>-XXXXXX/`。XXXXXX は実行ごとに変わる)に、プロセスごとのファイルで書きます。場所は `mktemp -d` の 0700、ファイルは 0600 で作ります(自分だけが読める)。`DEMO_LOG_DIR=<ディレクトリ>` で、その場所を作る親を選べます(既にある、シンボリックリンクでないディレクトリ。親の権限は変えません。条件に合わないか作れないときは、何も起こさずに終わる)。終わった後も残ります。下の手順で「ログ」とあるのは、この場所のファイルです。別の端末で `tail -f <場所>/window-text.log` のように見ながら進めると楽です。
- 終わり方:Seinas のウィンドウを閉じるか、script の端末で Ctrl+C を押します。起こしたものを全部止め、一時的な場所を消し、`tools/check-leftovers.py` で残りが 0 であることを確かめて、ログの場所を表示します。

## チェックリスト

### 1. タイトルバーとタスクバー

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 起動したまま見る | Seinas のウィンドウの中に、3 つのウィンドウが少しずつずれて重なる。それぞれにタイトルバー(タイトル、最小化・最大化・閉じるのボタン)。いちばん手前(橙)のタイトルバーだけが青く、ほかは灰色 |
| 画面の下を見る | タスクバーに、3 つのウィンドウのボタンと時計。手前のウィンドウのボタンが明るい |
| 灰色のウィンドウ(青)のタイトルバーをクリック | そのウィンドウが手前に来て、タイトルバーが青くなる。タスクバーの明るいボタンも移る |
| タスクバーのボタンをクリック | activated でないウィンドウなら手前に来る。activated のウィンドウなら最小化する(もう一度押すと戻る) |

### 2. 最大化・最小化・サイズ変更

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 最大化のボタン(□) | ウィンドウが、タスクバーの上までの全体に広がる。ボタンが「元に戻す」(重なった四角)に変わる |
| もう一度押す | 元の大きさと位置に戻る |
| タイトルバーをダブルクリック | 最大化と元に戻すが切り替わる |
| 最小化のボタン(−) | ウィンドウが消え、タスクバーのボタンは残る。次のウィンドウが選ばれる |
| タスクバーのそのボタンをクリック | ウィンドウが戻り、手前に来る |
| ウィンドウの縁・角をドラッグ | 縁の上でカーソルの形が変わり、ドラッグで大きさが変わる(左や上の縁では、反対の縁が動かない) |

### 3. ドラッグで元に戻す・上端で最大化

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 最大化したウィンドウのタイトルバーを押し、少し(8 ピクセル以上)下へドラッグ | 元の大きさに戻り、ポインターがタイトルバーの同じ割合の所にあるまま動く |
| ウィンドウのタイトルバーをドラッグして、ポインターを Seinas のウィンドウの上端に触れさせる | 使える範囲の内側に、青い枠(最大化の予告)が出る。上端から離すと消える |
| 枠が出たまま離す | ウィンドウが最大化する |

### 4. キーボードとフォーカス

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 青のウィンドウ(キーボード)をクリックして、`a` を打つ | `window-keyboard.log` に `window: keyboard enter`、`window: key 30 pressed`、`window: key 30 released` |
| 別のウィンドウをクリック | `window-keyboard.log` に `window: keyboard leave` |
| もう一度、青のウィンドウをクリックして、Caps Lock を押してから `a` を打つ | `window-keyboard.log` の `window: modifiers` の行の 3 つ目(ロック)が 0 でなくなる。もう一度 Caps Lock を押して戻す |

### 5. ウィンドウの切り替え

| 操作 | 期待する見え方・ログ |
| --- | --- |
| Alt を押したまま F6 を 1 回押す | 次のウィンドウが手前に見える(タイトルバーは灰色のまま)。`DEMO_LOG_KEYS=1` なら、`seinas.log` に `key: Alt_L pressed`、`key: Alt+F6 pressed` |
| Alt を押したまま、F6 を続けて押す | 押すたびに、次のウィンドウへ進む(端では反対の端へ回る) |
| Alt を離す | 見えていたウィンドウが選ばれる(タイトルバーが青くなり、キーボードのフォーカスが移る) |
| Alt+Shift+F6 | 逆の順に進む |
| 切り替えの最中に Esc | 取り消し。始める前のウィンドウのまま |
| ウィンドウを 1 つ最小化してから、Alt+F6 で進める | 最小化したウィンドウも候補に出る。Alt を離して決めると、戻る |
| Alt+Tab を押す | Windows が先に使うときは、Seinas では何も起きず、`DEMO_LOG_KEYS=1` なら、`seinas.log` に `key: Alt+Tab` の行が出ない(Windows の側の切り替えが出たら、Esc で閉じる) |
| 届かないキーがあったとき | `DEMO_LOG_KEYS=1` で起こし直し、`seinas.log` の `key:` の行で、どの組み合わせが届くかを見る。届く組み合わせを `seinas.conf` の形のファイルに書き、`DEMO_SEINAS_CONF=<ファイル>` で起こし直す(`docs/config.md`)。起動のときの `switch windows: next …` の行で、使うキーを確かめられる |

### 6. 応答なし

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 赤のウィンドウ(応答なし)の閉じるボタン(×)を押す | ウィンドウは閉じない。`window-unresponsive.log` に `window: close` と `window: ping` |
| 5 秒待つ | タイトルの後に「(応答なし)」が付き、閉じるボタンの地が暗い赤になる。`seinas.log` に `did not answer a ping` |
| もう一度、閉じるボタンを押す | ウィンドウが消える(接続が切れる)。`seinas.log` に `disconnecting a client that does not respond`。ほかのウィンドウと Seinas は動き続ける |

### 7. 壁紙

| 操作 | 期待する見え方・ログ |
| --- | --- |
| ウィンドウの無い所を見る | 背景が緑で、中央に小さな画像(48x32。色の違う 4 つの区切りの模様)。`wallpaper.log` に `test-48x32.png (Center), color 2f6f4f` と `drew 幅x高さ` |

### 8. 時計

| 操作 | 期待する見え方・ログ |
| --- | --- |
| タスクバーの右の端を見る | 時計が日本の時刻(UTC+09:00)。`taskbar.log` に `the clock is UTC+09:00 (from taskbar.conf)` |

### 9. フォント

| 操作 | 期待する見え方・ログ |
| --- | --- |
| タイトルバーとタスクバーの日本語を見る | 「キーボード」「応答なし」「文字入力」が、欠けずに出る。等幅の BIZ UDゴシック なので、ふだんの BIZ UDPゴシック より文字の間がそろって見える |
| ログを見る | `seinas.log` に `fonts: ui "BIZ UDGothic"`(タスクバーも同じ設定のファイルを読むので、タスクバーの文字も BIZ UDゴシック になる) |

### 10. 出力の大きさ

| 操作 | 期待する見え方・ログ |
| --- | --- |
| Seinas のウィンドウの縁をマウスでドラッグして、大きくする | 壁紙が新しい大きさで描き直され(`wallpaper.log` に `drew` の行が増える)、タスクバーが新しい下の端に移る。最大化しているウィンドウは、新しい大きさに合う。`seinas.log` に `output: 幅x高さ` |
| 小さくする | 画面の外に出たウィンドウは、タイトルバーが届く所へ戻る。320x240 より小さくはならない |

### 11. IME

| 操作 | 期待する見え方・ログ |
| --- | --- |
| 橙のウィンドウ(文字入力)をクリック | `window-text.log` に `window: text-input enter`。`ime.log` に `ime: activate` と `ime: done`。橙のウィンドウのコンテンツの左上から (10, 36) の所に、紫の四角(候補ウィンドウ)が出る |
| `a`、`i` を打つ | `window-text.log` に `window: preedit あ`、`window: preedit あい` |
| Enter を打つ | `window-text.log` に `window: commit あい` |
| `x` を打つ | `ime.log` に `ime: key 45 pressed`。`window-text.log` に `window: key 45 pressed`(IME が戻したキー) |
| 橙のウィンドウを最大化して、Seinas のウィンドウを小さくする | 紫の四角が、Seinas のウィンドウの中に収まったまま |
| 青のウィンドウをクリック | 紫の四角が消える。`ime.log` に `ime: deactivate`。`window-text.log` に `window: text-input leave` |
| Alt+F6 で橙のウィンドウへ戻す | `window-text.log` に `window: text-input enter`。紫の四角がまた出る。橙のウィンドウには、IME が戻した「Alt を離した」は届かない(`window-text.log` の、enter の後に `window: key 56 released` が無い) |
| `a` を打ってから、Alt+Shift+F12(緊急解除)を押す | テスト用の IME が終わる(`ime.log` の終わり)。`seinas.log` に `the input method was released with the emergency key` と `the input method left`。`window-text.log` に `window: text-input leave` と `window: reset preedit`。紫の四角が消える。この後の `x` は、`window-text.log` に `window: key 45 pressed` が直接出る(`ime.log` には出ない)。届かないときは、Windows の側が先に使っている(`DEMO_LOG_KEYS=1` で起こし直し、`seinas.log` の `key:` の行で確かめる) |

## 終わり方

Seinas のウィンドウを閉じるか、script の端末で Ctrl+C を押します。script の端末に、次のように出れば、片付けまで済んでいます。

```text
== 止める
残ったテストのプロセスと作業用の場所は、ありません。
ログ:/tmp/seinas-demo-logs-<日時>-XXXXXX
```

script が SIGKILL などで途中で終わったときも、子のプロセスは止まります(PR_SET_PDEATHSIG)。残った一時的な場所は、`python3 tools/check-leftovers.py --clean` で消せます。

## スクリーンショット(ヘッドレス)

入れ子で動かさずに、Sprint Review に使うスクリーンショット(PNG、1280x720、場面ごとに 1 枚)を作れます。fake screen で動かすので、Windows の側には何も出ません。

```bash
python3 tools/review-shots.py <出力先のディレクトリ>
```

| ファイル | 場面 |
| --- | --- |
| `1-desktop.png` | 壁紙・タスクバー(時計は UTC+09:00)・日本語のタイトルのウィンドウ 3 つ |
| `2-maximized.png` | 最大化したウィンドウ |
| `3-switcher.png` | ウィンドウの切り替えの一覧(Alt+Tab の最中) |
| `4-ime.png` | IME の変換中の文字(下線つき)と、候補ウィンドウ(候補の文字つき) |
| `5-not-responding.png` | 応答なしのタイトルと、暗い赤の閉じるボタン |

- 文字入力のウィンドウは `test-client window --show-text`(確定した文字と変換中の文字を描く)、IME は `test-client ime --popup 150x56 --popup-text`(候補ウィンドウに候補を書く)です。
- 終わるときに、起こしたものを全部止め、作業用の場所を消し、`tools/check-leftovers.py` で残りが 0 であることを確かめます。画像は、出力先にだけ書きます(リポジトリには入れません)。
