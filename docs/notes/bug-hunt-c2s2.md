# Seinas のバグ探しの記録(C2S2)

2026-11-05 に行ったバグ探しの記録です。読むのと試すのだけを行い、コードは直していません。

- 調べたコード:90ad7c7(Smithay は fork の 793c042)。
- コンポジタの側のコード(`seinas-frontend`、`seinas-icons`、`seinas-text`、`seinas-standalone` の `src`)は、v0.4.0(79825d8)と差がありません。ここに書いたコンポジタの不具合は、v0.4.0 にも入っています。
- ZeikOS の上では、どれも動かしていません。「ZeikOS でも届く」は、読んだ上での見立てです。
- 番号(1〜20、R1〜R23)は変えません。ほかの記録から、この番号で指します。

## 結果

| 分け方 | 数 |
| --- | --- |
| 動かして確かめた不具合 | 20 件(重いもの 3 件、中くらい 12 件、低いもの 5 件) |
| 読んだだけの候補 | 23 件 |
| 再現しなかった候補 | 1 件 |
| 判定できなかったもの | 1 件 |
| 不具合とは数えない観察 | 1 件 |

## 重いもの(動かして確かめた)

どれも、クライアントの要求だけでコンポジタが止まるか落ちます。

### 1. 自分自身を親にした get_popup で、コンポジタが無限ループする

- 現象:`xdg_surface.get_popup` の parent に自分を渡すと、ほかのクライアントの sync に答えなくなります。CPU 96%、状態 R のままです(debug と release)。
- 影響:クライアント 1 つで画面全体が止まります。入力が要らないので、ZeikOS でも届くと見ています。
- 見立て:`popup.rs:106,134`。`find_popup_root_surface` が親をたどるときに、循環を止めていません。2 つのポップアップで作る循環も同じ形と読みましたが、これは動かしていません。
- 再現:h40、h54。

### 2. とても大きな set_window_geometry で、コンポジタが panic で落ちるか、メモリーを大きく使う

- 現象:次の表のとおりです(release、`ulimit -v` 10 GB の下)。debug では、幅 100000000 と 2147483647 は `decoration.rs:274` の overflow で panic します。

| 幅 | 結果 |
| --- | --- |
| 100000000、2147483647 | `capacity overflow` で panic |
| 120000000 | `seinas-text` の `lib.rs:112` の assert(the canvas size)で panic |
| 1000000 | 落ちないが、RSS が 12 MB から 575 MB に増える |
| 30000、8000 | 問題なし |

- 影響:クライアント 1 つでコンポジタが落ちます。ZeikOS でも届くと見ています。
- 見立て:`decoration.rs:274` の `width * TITLE_BAR_HEIGHT` が i32 の掛け算で、幅に上限がありません。release では折り返すので、値によって壊れ方が変わります。
- 再現:h50a〜d、h57a〜d。

### 3. app_id が FIFO を指すと、コンポジタが open で止まる

- 現象:app_id を絶対パスにして `<パス>.desktop` を FIFO にすると、wchan が `wait_for_partner` のまま止まり、ほかのクライアントに答えなくなります(debug と release)。
- 影響:クライアント 1 つで画面全体が止まります。同じ読み方をするタスクバーも止まると読みました(こちらは動かしていません)。ZeikOS に FIFO があるかは見ていません。
- 見立て:`seinas-icons` の `lib.rs:345,391`。app_id の絶対パスと `..` を拒まず、ファイルの種類も見ずに開いています。
- 再現:h48、h54。

## 中くらい(動かして確かめた)

### 4. トーチのメニューが開いたまま、タスクやタスクバーの空いた所を押しても閉じない

- 現象:ウィンドウは activate されますが、キーボードはポップアップに残ります。隠れたタスクの一覧も同じコードです。
- 影響:見えているメニューが残り、キーがウィンドウへ届きません。
- 見立て:同じクライアントの押下は grab が閉じません。`update_activation` が閉じるのは、ウィンドウが根の連なりだけです。
- 再現:h1。

### 5. 子より先に親のポップアップを消しても、エラーにならない

- 現象:`not_the_topmost_popup` が出ません。子は画面から消えますが popup_done は届かず、外を押すまでキーが見えないポップアップへ届きます。
- 影響:キーが見えない所へ入り続けます。
- 見立て:destroy の順を確かめていません。
- 再現:h4、h17。

### 6. 閉じられたポップアップを親にした子の grab で、クライアントが切られる

- 現象:親が dismiss された後の子の grab が、NotTopmost の protocol error になります。
- 影響:押下と popup_done が行き違うと、ふつうのアプリが落ちます。
- 見立て:`xdg_popup.grab` の仕様は「親が閉じられていたら、すぐに閉じる」です。`popup.rs:213` は、grab していない親と同じエラーへ進みます。
- 再現:h53(`HUNT_KEEP_POPUP=1` で走らせる)。

### 7. ドラッグのアイコンがポインターに付いて来ない

- 現象:GTK でアイコンがドラッグの開始の位置に残り、動かす前後の画面が同じです。
- 影響:ドラッグの見た目が止まります。ドロップは成功します。
- 見立て:`pointer_input` が、ふつうの移動では再描画を頼みません。
- 再現:GTK のアプリで、アイコンつきのドラッグを始めて動かします(控えの hunt.rs には入っていません)。

### 8. on_demand の layer のフォーカスが、Alt+Tab の後も残る

- 現象:ウィンドウは activated になりますが、keyboard enter が届かず、キーはパネルへ届きます。
- 影響:見た目と違う所へキーが入ります。
- 見立て:`layer_focus` が、activate のときに外れません。
- 再現:h43。

### 9. exclusive の layer を null buffer で unmap しても、キーが届き続ける

- 現象:画面からは消えます(ピクセルの変化で確かめた)が、キーはその layer へ届き、ウィンドウへ戻りません。
- 影響:見えない layer にキーを取られます。
- 見立て:`keyboard_focus_target` が map の状態を見ていません。
- 再現:h55(h44 は、unmap の確認が無い形)。

### 10. ウィンドウのポップアップの grab が、exclusive の top layer のキーを取る

- 現象:exclusive の layer があっても、キーはポップアップへ届きます。
- 影響:ロック画面のような layer で問題になります。
- 見立て:`keyboard_focus_target` が、popup grab を先に見ています。
- 再現:h5。

### 11. selection の持ち主が終わっても、selection(null) が届かない

- 現象:フォーカスのあるクライアントが古い offer を持ったままで、貼り付けは空になります。
- 影響:貼り付けの見た目と結果が合いません。
- 見立て:持ち主が消えたときの通知の抜けです。Smithay の側も含めて見る必要があります。
- 再現:h12。

### 12. ドラッグの元が途中で終わっても、相手に drop が届く

- 現象:データは空で、ログは `ended (dropped)` です。
- 影響:受ける側が空のデータを受け取ります。
- 見立て:元が消えたときに取り消していません。
- 再現:h9。

### 13. 押したまま開いたメニューへ、ドラッグと離す操作が届かない

- 現象:右ボタンを押したまま grab のポップアップを開くと、ポップアップに enter も motion も届かず、離す側はウィンドウへ届きます。対照として、何も押していない形では enter と押下が届きました。
- 影響:押して、動かして、離して選ぶメニューが使えません。
- 見立て:ボタンを押している間のグラブ(ClickGrab)が続いています。
- 再現:h46、対照は h52。

### 14. set_mode に答えの configure が来ない

- 現象:すでに server-side の装飾へ、server-side または client-side を求めても、configure が来ません。sync は返ります。対照として、初めて求めるときは configure が届きました。
- 影響:仕様は「configure で答える」です。答えを待つクライアントが止まるおそれがあります。
- 見立て:`lib.rs:1462`。`send_pending_configure` は、変化が無ければ送りません。
- 再現:h58(h45 と h51 は、sync と対照が無い形)。

### 15. とても低い画面で、トーチのメニューを開くとタスクバーが panic で落ちる

- 現象:400x60 の画面で、`torch_menu.rs:224` の `copy_from_slice` の長さが合わず落ちます(36000 と 17280)。
- 影響:タスクバーが落ちます。ふつうの画面では届きません。読んだ範囲では、メニューは高さ 49〜72、一覧は 45〜52 で起きます。
- 見立て:`paint_*` が高さを下限まで広げますが、バッファの大きさはそのままです。
- 再現:h49。

## 低いもの(動かして確かめた)

### 16. swallowed_button が残る

- 現象:外を押した後に離す側が届かないと、次のクリックの離す側が飲み込まれ、押下だけが届きます。
- 影響:起きにくい形です。Interaction の `Grab::Client` も残ると読みました。
- 見立て:飲み込む予定を、leave のときに消していません。
- 再現:h6。

### 17. 離した後の start_drag で、cancelled が来ない

- 現象:黙って拒まれ、元は待ち続けます。
- 影響:クライアントの状態が残ります。
- 見立て:Smithay の側の振る舞いです。
- 再現:h31。

### 18. 起こした子が、次のイベントまでゾンビのまま残る

- 現象:タスクバーにイベントが来るまで片付きません。
- 影響:プロセスの表の項目が一時的に残るだけです。
- 見立て:`reap` がイベントの後にだけ呼ばれます。
- 再現:h18。

### 19. grab のないポップアップが、ウィンドウを動かしても置き直されない

- 現象:画面の外へ出ます。
- 影響:見えなくなります。制限として文書に書く形でもよいと思います。
- 見立て:移動のときに constrain をし直していません。
- 再現:h19。

### 20. 同じ serial の 2 回目の start_drag で、最初のドラッグの ended がログに出る

- 現象:started が 2 回の後に `ended (cancelled)` が 1 回出ます。ドラッグの見た目と `dnd_active` への影響は動かして確かめていません(読んだ範囲では `dnd_active` が偽になり、アイコンも消えます)。
- 影響:落ちはしませんでした。
- 見立て:`data_device.rs:36-53`。古い grab の unset から dropped が届きます。
- 再現:h47。

## 不具合と数えない観察:Esc の離す側

メニューを Esc で閉じた後に、`keyboard enter keys 1` と `key 1 released` がウィンドウへ届きます(12 回中 3 回)。仕様の範囲と見ています。Enter で閉じるときも同じ形です。

## 読んだだけの候補(動かしていない)

読むだけのエージェントの結果で、動かしていません。直す前に確かめが要ります。

コンポジタ:

- R1:ホイールがどこへも届かない。`crates/seinas/src/parent.rs:350` で Axis を捨て、`PointerInput` に軸がありません。`docs/taskbar.md` はホイールを書いています。これだけは grep で経路を確かめました。実機の操作は見ていません。
- R2:ウィンドウと layer の入力を、input region と subsurface が無視する。
- R3:layer が根の連なりが、activate で閉じない(4 と同じ原因)。`lib.rs:527-538`。
- R4:dropped の中でフォーカスが変わると、seat_data の二重の borrow で panic する形(今は通らないとの読み)。`lib.rs:2850`、Smithay の `dnd_grab.rs`。
- R5:`swallowed_button` が 1 つだけで、重なると離す側がずれる。`lib.rs:1915`。
- R6:serial がクライアントに結び付かず、文書より緩い。`popup.rs:177`。
- R7:grab の無い layer のポップアップを押すと、`layer_focus` が外れる。`lib.rs:1860`。
- R8:NoRoot のときに popup_done を送らない。`popup.rs:195`。
- R9:null buffer で unmap した toplevel が activated のまま残り、下のウィンドウへフォーカスが行かない。`lib.rs:817-822`。
- R10:兄弟のポップアップで、古いものが上になる。`popup.rs:295`。

タスクバー:

- R11:OnlyShowIn と NotShowIn を見ていない。
- R12:空の `Icon=` で、`%i` が `--icon ""` になる。
- R13:引用符の中の `%%` と、`--file=%f` を拒む。
- R14:TryExec が実行のビットを見ない。
- R15:一覧の余白の hover で、選択が外れる(`main.rs:775`)。
- R16:ウィンドウの数が変わっても、メニューがトーチを追わない。
- R17:`--app-display` が、docs の引数の表と `main.rs` の先頭に無い。
- R18:消したポップアップの frame を、タスクバー本体のものとして扱う(`main.rs:618`)。
- R19:`select_row` の早い return でスクロールしない(ホイールが届くようになると出る)。`torch_menu.rs:243`、`main.rs:392`。
- R20:`paint_menu` が毎回全部の行を描く(最大 768 行、約 44 MB)。`lib.rs:460`。
- R21:`/tmp/.s-shell` から、アプリのソケットを求められない(`launch.rs:43`)。
- R22:`WAYLAND_DISPLAY=""` やソケットでないパスで 30 秒待つ(`seinas-connect` の `lib.rs:50`)。
- R23:ZeikOS で `CommandLauncher` が無条件に使われる(`main.rs:1267`。今は入力が無いので届かない)。

## 再現しなかったもの・判定できなかったもの

- タイトルバーのドラッグの最中に grab すると Interaction が残る、という候補:再現しませんでした。離した後の移動で、ウィンドウは付いて来ません(h42)。
- 古い serial の grab:判定できていません。h16 はクリックが対象のウィンドウへ届いていて、serial が古くなっていません。作り直した h56 も途中で時間切れになりました。どちらも走らない形(ignore)で残しています。

## テストの待ち方(読むだけ)

入力(コンポジタの標準入力)とクライアントのソケットの順に頼っている所を読みました。

- 偽の失敗の形:1 か所。`taskbar.rs` の `the_list…` の `press_task_small` です。ウィンドウを開いた直後に押すので、タスクバーが全部のウィンドウをまだ知らないと、レイアウトが変わるおそれがあります。
- 偽の成功の形:6 か所。クライアントの sync は、標準入力から送った入力が処理されたことを保証しません。`decoration.rs:68`、`drag_maximize.rs:169`(600 ms の sleep あり)、`drag_maximize.rs:232`、`drag_maximize.rs:267`、`maximize.rs:191`、`maximize.rs:231`。
- 問題なし:`layer_keyboard.rs:92`(500 ms 届かないことを見る)、`ime.rs`(待っている)、`switch_windows.rs:147`(片付け)、`data_device.rs:180`。

## 見たもの・試したもの

- ポップアップ:位置、ウィンドウの余白、grab の連なり、親と子の消す順、外の押下、自分を親にする形。
- layer のキーボード:exclusive、on_demand、unmap、popup grab との順。
- data-device:コピーと貼り付け、持ち主の終了、DnD、Alt+Tab の最中のドラッグ、元の終了、start_drag の形、GTK のドラッグ。
- 装飾の取り決め:set_mode、大きな geometry、GTK の CSD の移動・影でのリサイズ・ダブルクリックの最大化。
- 隠れたタスク:開く、閉じる、キー、押下。
- `.desktop` の読み方、Exec の分け方、子の環境と fd、低い画面、app_id と FIFO。
- つなぐ手順の試し直し:`connect_to_env` からの後退はありませんでした。
- 繰り返し:結合テストの全部を、2 CPU(`taskset -c 0,1`)で 3 回。失敗はありませんでした。golden_layers と調べ用の hunt は除いています。

問題なしと確かめたもの:

- ポップアップの位置(h7)。
- grab を持つクライアントを kill した後のフォーカス(h8)。
- 別の根の 2 つ目の grab で、前の連なりが閉じること(h14)。
- grab していない親への子の grab のエラー(h15。仕様どおり)。
- DnD の最中の Alt+Tab(h23)。
- 子の環境と fd(h2)。fd 9 は `tools/zeikos-lock.sh` の包みから来たもので、タスクバーのものではありません。

## 途中で訂正したこと

- 6 番:最初の試しで出た `Unknown id: 19` は、テスト用のクライアントが popup_done の後に自分で消したためでした。親を残す形で試し直して、NotTopmost を確かめました。
- 2 番:debug の overflow の panic だけでは release の振る舞いが分からないので、release で試し直しました。

## 再現の控え

`docs/notes/bug-hunt-c2s2/` にあります。

- `hunt.rs`:実験のテスト。アサートせず、起きたことを `H…` の行で書き出します。上の h の番号は、このファイルのテストの名前です。
- `test-client.diff`:テスト用のクライアントに足す、調べ用の口です。
  - `color=030201` のポップアップは、自分自身を親にします(1 番)。
  - `geometry X Y W H` の行は、`set_window_geometry` を送ります(2 番)。
  - layer の `unmap` の行は、null buffer を commit します(9 番)。
  - 環境変数 `HUNT_KEEP_POPUP` があると、popup_done の後もポップアップを消しません(6 番)。

控えは 90ad7c7 に対するものです。リポジトリのコードは変えないので、別の worktree で走らせます。`<repo>` は、このリポジトリのパスです。

```bash
git worktree add --detach <dir> 90ad7c7
cd <dir>
git apply <repo>/docs/notes/bug-hunt-c2s2/test-client.diff
cp <repo>/docs/notes/bug-hunt-c2s2/hunt.rs crates/seinas-standalone/tests/hunt.rs
mkdir -p target && cp -r <repo>/target/fonts target/fonts
<repo>/tools/zeikos-lock.sh bash -c 'ulimit -v 10000000; cargo test --release --locked -p seinas-standalone --test hunt -- --nocapture --test-threads=2 2>&1' | grep -aE "H ?[0-9]"
```

- `ulimit -v` は、2 番の大きな確保を止めるための上限です。外さないでください。
- 各テストの最初の行は `test … ...` と同じ行に出るので、行の先頭で絞り込まないでください。
- 6 番は、`HUNT_KEEP_POPUP=1` を付けて h53 だけを走らせます。ほかのテストは、この環境変数の無い形で走らせます。
- 走らない形にしたテストは h16、h41、h56 です。h41 は、コンポジタが落ちると手順の側が失敗するので、h50 と h57 が代わりです。
- 走らせた結果(90ad7c7、release):39 件が通り、3 件が走らない形でした。1〜3 番と 6 番は、この手順で再現しました。
