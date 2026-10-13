# IME の道

Seinas は、アプリの text-input-v3 と、IME の input-method-v2 を取り次ぎます。決めたことと理由は `docs/adr/0004-ime-keyboard-path.md` にあります。ここには、今の実装の決まりと、試し方を書きます。

## 見せる global

| global | 見せる相手 | 作る条件 |
| --- | --- | --- |
| `zwp_text_input_manager_v3` | 全部のクライアント | いつも |
| `zwp_input_method_manager_v2` | シェルの部品のためのソケット(`seinas-0-shell`)からつないだクライアントだけ | seat にキーボードがあるとき |
| `zwp_virtual_keyboard_manager_v1` | 同じ | 同じ |

キーボードの無い seat では、input-method と virtual-keyboard の global を作りません。Smithay 0.7.0 は、キーボードの無い seat で `get_input_method` や virtual-keyboard のキーを受けると、`unwrap` で止まるためです。

## キーの道

1. Seinas のショートカット(Alt+Tab など)を、いつも先に見ます。使ったキーは、IME にもアプリにも渡しません。
2. IME が keyboard を grab していれば、ほかのキーと修飾キーの状態を IME へ渡します。
3. IME が virtual-keyboard で戻したキーは、キーボードのフォーカスのあるクライアントへ届きます。
4. IME が去ったか、grab を手放したら、次のキーから直接の道(フォーカスのあるクライアントの `wl_keyboard`)にします。ログに `the input method left`、`the input method released the keyboard` を出します。

Alt+Tab の最中も、Alt は IME へ渡ります。Alt を離すと、Seinas はその場でフォーカスを移しますが、IME が virtual-keyboard で戻す「Alt を離した」は、その後で届きます。

- virtual-keyboard で戻ってきたキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーのときだけ渡します。Seinas は、virtual-keyboard で「押した」を渡したキーを覚え、キーボードのフォーカスを動かしたら(同じクライアントへの leave と enter のし直しも含む)、記録を空にします。Smithay の `forwarded_pressed_keys` は、Smithay の外から読めず、virtual-keyboard のキーも入らないので使いません。
- そのため、新しいフォーカスのクライアントには、押したことを知らない Alt の「離した」が届きません。前のクライアントには leave が届いているので、Alt が押されたまま残るクライアントもありません。
- 渡さなかった「離した」も、IME の返事としては数えます(下の「応答しない IME」)。

## 応答しない IME

IME が固まっても、キーボードが使えなくならないようにします(ADR 0004 の 2)。

- キーの押下を IME へ送ってから 1,000 ms(仮の値)の間に、その IME のクライアントから要求が 1 つも届かなければ、応答しないと見なします。keyboard の grab を外し、次のキーから直接の道にします。ログに `the input method did not answer a key in 1000 ms; keys go straight to the applications` を出します。
- 修飾キーだけの押下は、数えません(返事を待ち始めません)。
- input-method の要求のほか、同じクライアントからの virtual-keyboard の要求も、返事として数えます。
- 返事を待ち始めるのは、待っていないときの最初の押下です。続けて押したキーで、期限は延びません。
- 応答しないと見なした後に、その IME から要求が届いたら、grab を戻し、次のキーから IME を通る道に戻します。ログに `the input method answered again; keys go through it again` を出します。外している間に IME が grab を手放していたら、戻しません。
- 返事の無かったキーは、アプリへ送り直しません(同じキーが 2 度届かないように)。
- 待つ時間は、seinas-standalone の `--ime-timeout MS` で変えられます(テストのため)。期限は、イベントループの待ち時間で待ち、タイマーの fd は作りません。

## 文字の受け渡し

- IME の commit_string・preedit・delete_surrounding_text は、IME の commit で、フォーカスのあるクライアントの、有効な text-input へまとめて渡します(text-input の done)。Smithay 0.7.0 の部品のままです。
- アプリが送る、有効・無効、周りの文字、カーソルの矩形、入力の種類は、アプリの commit で IME へ渡します(IME の done)。

## フォーカスが変わったとき

キーボードのフォーカスが変わると(新しいウィンドウ、クリック、Alt+Tab、最小化、閉じる)、次のものが届きます。

| 相手 | 届くもの |
| --- | --- |
| 前のフォーカスのクライアント | text-input の leave |
| IME | deactivate と done |
| 新しいフォーカスのクライアント | text-input の enter(IME がつないでいるときだけ) |
| IME | 新しいクライアントが text-input を有効にして commit したら、activate と done |

IME がつながったときも、フォーカスのあるクライアントに text-input の enter が届きます。フォーカスのあるウィンドウが無くなったとき(全部を最小化した、最後のウィンドウを閉じた)は、IME に deactivate と done が届きます。

## 候補の窓

IME の候補の窓(input popup surface)は、Seinas が置きます(ADR 0004 の 5)。

- 見せるのは、IME が有効で、親(text-input のフォーカスのあるウィンドウ)があるときだけです。フォーカスが移って IME が deactivate になると隠し、新しいウィンドウで activate になると、そのウィンドウを親にして見せます。
- 置く位置は、親のウィンドウのコンテンツの左上に、アプリが伝えたカーソルの矩形(`set_cursor_rectangle`)を足した所の下で、左の端をカーソルにそろえます。画面の下の端からはみ出すなら、カーソルの矩形の上に置きます。それでもはみ出す分は、画面の中へ寄せます(右の端、左の端、上の端)。
- 位置は、描くたびに求め直します。ウィンドウを動かしたとき、カーソルの矩形が変わったとき、出力の大きさが変わったときも、画面の中に収まります。
- 重ね方は、overlay の layer surface の下、top の layer surface とウィンドウの上です。ポインターのイベントも、同じ順で候補の窓が受けます(候補を押して選べるように)。
- 親のウィンドウが最小化しているときは、描きません。

## IME は同時に 1 つ

IME が 1 つつないでいる間は、2 つ目からの `get_input_method` を断ります。

- 断り方:2 つ目の要求は、Smithay の部品へ渡さずに Seinas が受け、Seinas の側の入れ物でオブジェクトを作って、すぐ `unavailable` を送ります。その後の要求(grab_keyboard、候補の窓など)も、Seinas の側で受けて捨てます。ログには `an input method was refused (another input method is running)` を出します。
- 理由:Smithay 0.7.0 の 2 つの不具合を避けるためです。
  - `InputMethodHandle::add_instance` は、2 つ目が来ると、前からある IME の方へ `unavailable` を送り、新しい方には何も送りません。
  - 2 つ目の IME も、1 つ目と同じ keyboard の grab の入れ物を指します。2 つ目が grab を取ってから手放すと、1 つ目の grab まで外れます。場合によっては、keyboard の grab が残ったまま、Smithay が覚えている grab のオブジェクトだけが無くなり、次のキーで Smithay が `unwrap` で止まります。

  2 つ目の要求が Smithay の部品に一度も届かなければ、どちらも起きません。
- 1 つ目の IME の `zwp_input_method_v2` と、keyboard の grab が両方とも無くなったら(ふつうは、接続を切ったとき)、次の IME を受け入れます。

## 試し方

### CI のテスト

`cargo test -p seinas-standalone --test ime` で、fake screen とテスト用の入力インターフェースを使って確かめます。IME には、テスト用の IME(`test-client ime`)を使います。fcitx5 などの IME を入れる必要はありません。

| テスト | 確かめること |
| --- | --- |
| `keys_go_through_the_ime_and_come_back_to_the_window` | キーが IME を通り、IME が virtual-keyboard で戻したキーがウィンドウに届く |
| `the_input_method_globals_are_only_on_the_shell_socket` | ふだんのソケットには、input-method の global が無い |
| `alt_tab_works_while_the_ime_has_the_keyboard` | IME が grab している間も、Alt+Tab が効き、Tab は IME に渡らない。切り替えた先には、IME が戻した「Alt を離した」が届かない |
| `alt_through_the_ime_during_alt_tab_reaches_only_the_window_that_got_the_press` | Alt+Tab で行き来しても、IME が戻した Alt の押す・離すは、押したことを受けたウィンドウにだけ届き、切り替えた先で押して離した Alt は両方とも届く |
| `keys_go_straight_to_the_window_after_the_ime_leaves` | IME が去ったら、直接の道になる |
| `keys_go_straight_to_the_window_after_the_ime_releases_the_keyboard` | IME が grab を手放したら、直接の道になる |
| `a_second_ime_is_refused_and_the_first_keeps_working` | 2 つ目の IME に unavailable が届き、2 つ目が grab を取って去っても、1 つ目は使える |
| `preedit_and_commit_reach_the_window` | `a`・`i` が変換中の文字「あ」「あい」になり、Enter で「あい」が確定してウィンドウに届く。IME が使ったキーは、ウィンドウの wl_keyboard に届かない |
| `a_window_opened_after_the_ime_gets_the_text_input` | IME の後に開いたウィンドウにも、text-input の enter が届き、変換中の文字が届く |
| `an_unresponsive_ime_is_bypassed_and_comes_back` | `--ime-timeout 300` で、Shift だけの押下を 3 回(合わせて 300 ms を超える)しても応答しないと見なさず、返事の無い x の後に直接の道になり、IME が要求を送ったら IME を通る道に戻る |
| `an_ime_that_answers_is_not_bypassed` | 変換中の文字(input-method の要求)と、戻したキー(virtual-keyboard の要求)で答える IME は、間を空けて打っても応答しないと見なされない |
| `the_candidate_window_follows_the_cursor_inside_the_output` | 候補の窓(40x30)が、カーソルの矩形の下に出て、カーソルの矩形を右の端へ動かすと、画面の中へ寄る |
| `the_candidate_window_goes_above_the_cursor_at_the_bottom_edge` | 下に入らないときは、カーソルの矩形の上に出る |
| `the_candidate_window_stays_inside_a_resized_output` | 出力の高さを縮めると、下から上へ移る |
| `the_candidate_window_hides_when_the_focus_leaves` | フォーカスが text-input を使わないウィンドウへ移ると、候補の窓が消える |
| `focus_changes_move_the_text_input_and_the_activation` | Alt+Tab・最小化・閉じるで、leave・enter と deactivate・activate が届き、変換中の文字が新しいフォーカスのウィンドウに届く |

### テスト用の IME

`test-client ime` は、シェルの部品のためのソケットへつなぎます。`WAYLAND_DISPLAY` に、そのソケットの名前(`XDG_RUNTIME_DIR` の下なら `seinas-0-shell`)か、絶対パスを入れます。

```bash
WAYLAND_DISPLAY=seinas-0-shell target/debug/test-client ime
```

- keyboard を grab し、grab で届いた keymap を、そのまま virtual-keyboard の keymap にします。
- IME が有効(activate を受けた)なら、`a`・`i`・`u` を変換中の文字の「あ」「い」「う」にし、変換中の文字があるときの Enter で確定します。
- ほかのキーは、virtual-keyboard でアプリへ戻します。
- 標準入力の `release` で grab を手放し、`quit` で接続を切って終わります。
- `--popup WxH` を付けると、その大きさの候補の窓を、紫(8000ff)の単色で出します。届いたカーソルの矩形を `ime: popup rectangle X Y W H` の行で書きます。
- `--ignore-keys` を付けると、キーを受けても何も返しません(応答しない IME のまね)。標準入力の `answer` で、ふつうの動きに戻り、input-method の commit を 1 つ送ります。
- くわしくは `crates/seinas-test-clients/src/ime.rs` にあります。

### テスト用のアプリ

`test-client window --text-input` は、ふだんのソケットへつなぎ、text-input-v3 を使います。enter が届くたびに有効にして、カーソルの矩形(`--cursor X,Y,W,H`。既定は 10,20,2,16)を送ります。届いた enter・leave・preedit・commit・done を、`window: preedit あい` のような行で標準出力に書きます。

```bash
WAYLAND_DISPLAY=seinas-0 target/debug/test-client window --text-input
```

位置の決まり(`popup_location`)のユニットテストは、`cargo test -p seinas-frontend --lib ime_popup` です。

### 入れ子の構成で試す手順(運用者の方へ)

入れ子の構成(WSLg の中の `seinas`)では、キーを親の窓から打つので、CI ではなく手で確かめます。IME には、テスト用の IME を使います(fcitx5 などを入れる必要はありません)。

1. ビルドする:`cargo build --workspace --locked` と `tools/fetch-fonts.sh`。
2. 端末 1 で、入れ子の Seinas を起動する:`target/debug/seinas`。ふだんのソケットは `seinas-0`、シェルの部品のためのソケットは `seinas-0-shell` です(どちらも `XDG_RUNTIME_DIR` の下)。
3. 端末 2 で、テスト用のアプリを出す:`WAYLAND_DISPLAY=seinas-0 target/debug/test-client window --text-input --title あ`。
4. 端末 3 で、テスト用の IME をつなぐ:`WAYLAND_DISPLAY=seinas-0-shell target/debug/test-client ime --popup 40x30`。端末 3 に `ime: ready` と `ime: activate` が出ます。
5. Seinas の窓をクリックしてキーボードのフォーカスを渡し、`a`、`i`、Enter を打つ。端末 2 に `window: preedit あ`、`window: preedit あい`、`window: commit あい` が出て、Seinas の窓の中、アプリのウィンドウのカーソルの矩形(コンテンツの左上から 10, 20)の下に、紫の候補の窓が出ていることを見る。
6. `x` を打つ。端末 3 に `ime: key 45 pressed`、端末 2 に `window: key 45 pressed` が出る(IME が戻したキー)。
7. Alt+Tab を試すなら、端末 2 と同じものをもう 1 つ出してから、Seinas の窓の中で Alt+Tab を打つ。切り替えたウィンドウに `window: text-input enter` が出て、候補の窓がそのウィンドウへ移る。
8. 端末 3 で `quit` を打ち、IME を終える。端末 1 のログに `the input method left` が出て、`x` が端末 2 に直接届く(端末 3 には出ない)。
9. Seinas の窓の大きさを、親の側で小さくする。候補の窓が、Seinas の窓の中に収まったままであることを見る(手順 4 と 5 をもう一度行ってから)。

うまくいかないとき:端末 3 で `the compositor does not offer zwp_input_method_manager_v2` と出たら、`WAYLAND_DISPLAY` が `seinas-0-shell` になっているかを確かめてください(ふだんのソケットからは見えません)。
