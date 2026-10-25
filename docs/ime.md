# IME の道

Seinas は、アプリの text-input-v3 と、IME の input-method-v2 を取り次ぎます。決めたことと理由は `docs/adr/0004-ime-keyboard-path.md` にあります。ここには、今の実装の決まりと、試し方を書きます。

## 見せる global

| global | 見せる相手 | 作る条件 |
| --- | --- | --- |
| `zwp_text_input_manager_v3` | 全部のクライアント | いつも |
| `zwp_input_method_manager_v2` | IME のためのソケット(`seinas-0-ime`)からつないだクライアントだけ | seat にキーボードがあり、IME のためのソケットを開くとき(seinas-standalone の `--no-ime-socket` では作らない) |
| `zwp_virtual_keyboard_manager_v1` | 同じ | 同じ |

IME の権限は、シェルの部品の権限と分けています(ADR 0004 の 1.6)。これは権限を分けることで、相手が誰かを確かめる認証ではありません。

- IME のためのソケットは、ふだんのソケットのパスの名前に `-ime` を入れたもの(名前に拡張子があれば、その前に入れる。`docs/standalone.md`)です(入れ子は `seinas-0-ime`。seinas-standalone は `--ime-socket PATH` で変えられる)。シェルの部品のソケットとふだんのソケットには、input-method と virtual-keyboard を見せません。IME のソケットには、layer-shell と foreign-toplevel を見せません(候補ウィンドウに要る `wl_compositor`・`wl_shm` などは見せる)。
- virtual-keyboard を作れるのは、`get_input_method` を受け入れたクライアントだけです。ほかは、virtual-keyboard の `unauthorized` のエラーで接続を切ります(ログに `a virtual keyboard was refused`)。認可は、接続と、受け入れた IME の世代に結び付けます。受け入れた input method が destroy されたら認可も消え、それまでに作った virtual-keyboard から届いたキーは捨てます。同じ接続からの 2 つ目の `get_input_method` を断っても、1 つ目の認可は消しません。
  - 限界:virtual-keyboard を作ったときの世代は、そのオブジェクトの最初の要求で決めます(Smithay が作るオブジェクトの番号は、作る要求の時点では分からないため)。前の世代で作って要求を送っていないものと、今の世代で作ったものが両方待っているときは、見分けられないので、待っているものを全部捨て、IME の接続を `unauthorized` のエラーで切ります(つなぎ直しを求める。ログに `a virtual keyboard was refused (it cannot be told whether it was made for the past or the current input method); …`)。要求を送らずに待てるのは 1 つのクライアントで 8 個まで(越えるとログに `a virtual keyboard was refused (too many wait for their first request)`)、生きている virtual-keyboard は 1 つのクライアントで 16 個まで(越えるとログに `a virtual keyboard was refused (too many are alive on the connection)`)で、越えると `unauthorized` のエラーになります(ADR 0004 の 1.6)。
- 守れないこと:IME のソケットにつなげるプロセスは、どれも IME になれます。先につないで、IME の枠(同時に 1 つ)を取ることも防げません。
- Linux:ソケットは `XDG_RUNTIME_DIR`(0700)の下にあるので、つなげるのは同じ利用者のプロセスだけです。
- ZeikOS:ソケットの名前はカーネルの表で、利用者の概念も、ファイルの権限の検査もありません。0700 は効かず、どのプロセスも IME のソケットにつなげます。効くのは、ソケットを分けた役割の分け方だけです。
- ソケットとロックファイルが 1 つずつ増えるので、コンポジタの fd は 2 つ増えます(2026-10-15 に測った。seinas-standalone `--fake --test-input` で 13 から 15、入れ子の seinas で 15 から 17)。
- IME を使わない構成では、seinas-standalone に `--no-ime-socket` を付けると、IME のためのソケットとロックファイルを作らず、input-method と virtual-keyboard の global も作りません(ADR 0004 の 1.6)。入れ子の seinas は、いつも開きます。

キーボードの無い seat では、input-method と virtual-keyboard の global を作りません。Smithay 0.7.0 は、キーボードの無い seat への `get_input_method` と、virtual-keyboard の `key`・`modifiers` で、`unwrap` の panic で止まるためです。Seinas は、次のことで避けています。

- seat にキーボードを足した後に、足せたとき(keymap を作れたとき)だけ、2 つの global を作ります。キーボードを外す道はありません。
- キーボードの無い seat への `get_input_method` は、Smithay へ渡す前に断ります(下の「IME は同時に 1 つ」と同じ断り方)。
- `the_input_method_globals_need_a_keyboard` のテストで、keymap を作れなかった構成では 2 つの global が見えないことを確かめています。

Smithay の側を直した patch は、`docs/notes/smithay-patches/` に在ります(上流には出していません。Smithay のバージョンは上げません)。

ZeikOS でキーボードを有効にするときは、xkb のファイル(ADR 0004 の「足りないもの」の (a))か、keymap を文字列から作る口((b))を先に用意してください。用意していないと、keymap を作れず、キーボードの無い構成として動きます(IME の global も作りません)。

## キーの道

1. Seinas のショートカット(Alt+Tab、IME の緊急解除など)を、いつも先に見ます。使ったキーは、IME にもアプリにも渡しません。
2. IME が keyboard を grab していれば、ほかのキーと修飾キーの状態を IME へ渡します。
3. IME が virtual-keyboard で戻したキーは、キーボードのフォーカスのあるクライアントへ届きます。
4. IME が去ったか、grab を手放したら、次のキーから直接の道(フォーカスのあるクライアントの `wl_keyboard`)にします。ログに `the input method left`、`the input method released the keyboard` を出します。IME へ渡したキーは、アプリへ送り直しません。

Alt+Tab の最中も、Alt は IME へ渡ります。Alt を離すと、Seinas はその場でフォーカスを移しますが、IME が virtual-keyboard で戻す「Alt を離した」は、その後で届きます。

- virtual-keyboard で戻ってきたキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーのときだけ渡します。Seinas は、virtual-keyboard のオブジェクトごとに、「押した」を渡したキーと、渡した先とフォーカスの世代を覚えます。キーボードのフォーカスを動かしたら(同じクライアントへの leave と enter のし直しも含む)、世代を進め、前の世代で渡したものは、leave で消えたものとして扱います。Smithay の `forwarded_pressed_keys` は、Smithay の外から読めず、virtual-keyboard のキーも入らないので使いません。
- そのため、新しいフォーカスのクライアントには、押したことを知らない Alt の「離した」が届きません。前のクライアントには leave が届いているので、Alt が押されたまま残るクライアントもありません。

## IME の復旧(接続断と緊急解除)

時間切れで「応答しない」と見なす決まりは、2026-10-15 に取り消し、実装からも外しました(ADR 0004 の 1.5 と「取り消した決定」)。文字を出さずにキーを使うのは、ふつうの IME の動きです。IME にキーごとの返事は求めません。直接の道に戻すのは、次の 2 つのときだけです。

- 接続断:IME のクライアントが去った(接続を切った、input method と grab を destroy した)とき。
- input method の destroy:IME が `zwp_input_method_v2` だけを destroy し、grab を残したときも、その世代の grab・virtual-keyboard の認可・候補ウィンドウを片付けて、直接の道に戻します(仕様では、input method の destroy は子の grab と候補ウィンドウも破棄する。ADR 0004 の 1.5)。ログに `the input method was destroyed; its keyboard grab is ignored and keys go straight to the applications` を出します。古い grab のオブジェクトが残っていても、新しい IME を受け入れます(同じ接続でも、別の接続でも)。古い grab の破棄は、新しい IME の grab を外しません(Smithay の fork の直し。`docs/smithay-patches.md`)。Seinas は、IME ごとに今の grab のオブジェクトを覚え、それが破棄されたときだけ、grab が無くなったとして扱います(ログに `the input method released the keyboard`)。片付けと、`wl_keyboard` の leave と enter の送り直しは、順が違っても 1 回だけです。
- 緊急解除:Seinas のショートカット(既定は Alt+Shift+F12。入れ子(WSLg)では、運用者が届くこと・働くことを確かめた(2026-10-15)。ZeikOS では未確認。`seinas.conf` の `ime-release` で変えられる。`docs/config.md`)を押したとき。IME のクライアントの接続を切ります。IME のためのソケットを開かない構成(`--no-ime-socket`)では、このキーを予約せず、アプリへ渡します(ADR 0004 の 1.8)。ログに `the input method was released with the emergency key; its connection is closed and keys go straight to the applications` を出します。
  - IME の側からは、ただの切断に見えます。Seinas は接続を閉じるだけで、`wl_display.error` は送りません(ADR 0004 の 1.5)。IME は、読むと接続の終わり(EOF)、送ると EPIPE になります。切断の理由の文(`a client disconnected: ProtocolError { … }`)は、Seinas のログにだけ出ます。
  - 物理のキーボード(入れ子では親の `wl_keyboard`)から来たキーだけで発動します。IME が virtual-keyboard で送ったキーは、ショートカットの判定を通らないので、発動しません。
  - 押し続けても 1 回だけです(同じキーを離すまで、繰り返しの押下は使って捨てる)。
  - IME がつないでいないときに押すと、ログに `the emergency release key was pressed, but no input method is running` を出すだけです。
  - 緊急解除の後、切った接続のクリーンアップが済む前にもう一度押したときは、ログに `the emergency release key was pressed, but the input method is already released and its connection is being cleaned up` を出すだけです。

緊急解除の順番:

1. その接続の認可を消します。この後、その接続の input-method と virtual-keyboard の要求は捨てます。
2. keyboard の grab を外し(次のキーから直接の道)、候補ウィンドウを隠し、接続を切ります。切った接続のクリーンアップは、その場で行わせます(バックエンドの `reap_dead_clients`)。
3. IME へ渡したキーは、アプリへ送り直しません。

IME が去った後(接続断でも緊急解除でも)は、次の順に届きます。text-input の leave と `wl_keyboard` の leave は、別のものです。

| 順 | 届くもの | 相手 | 送るもの |
| --- | --- | --- | --- |
| 1 | text-input-v3 の leave | フォーカスのあるアプリの text-input | Smithay(IME の `zwp_input_method_v2` の destroy の中で) |
| 2 | `wl_keyboard` の leave と enter | フォーカスのあるアプリの `wl_keyboard` | Seinas(IME の input method と grab が両方とも無くなったとき) |

- 変換中の文字:Seinas から、空の preedit_string と done は送りません(先に届いていた commit_string まで、アプリに適用させるおそれがあるため)。text-input の leave で、アプリが変換中の文字を消します(text-input-v3 の leave の「The client should reset any preedit string previously set」)。テスト用のアプリは、そのとおりに消し、`window: reset preedit` を書きます。
- 押されたままのキー:`wl_keyboard` の enter に入るのは、物理のキーボードで今押されていて、IME かアプリへ渡したキーだけです(Smithay の `forwarded_pressed_keys`)。IME が virtual-keyboard で押したキーは入らないので、leave でアプリから消えます。Seinas が使ったショートカットのキー(緊急解除の F12 など)も入りません。物理のキーボードで押しているキーは、離したことが直接アプリへ届きます。
- virtual-keyboard が壊れたとき(destroy、IME の接続断、input method の destroy での認可の取り消し、緊急解除):その virtual-keyboard が押したままのキーの「離した」を、渡した先のアプリへキーごとに送り、modifiers を物理のキーボードの状態へ戻します(ADR 0004 の 1.9)。ログには `released N key(s) … left pressed by a virtual keyboard` を出します(キーは出しません)。物理のキーボードやほかの virtual-keyboard が同じキーを押しているときと、フォーカスが動いた後は、送りません。2 回続けて壊れても(destroy の後の接続断)、2 回目は送りません。
- grab の release だけでは、virtual-keyboard の押下を消しません(仕様は、grab の release が押下を消すとは書いていない)。IME が「離した」を遅れて送れば、そのとおりアプリに届きます。IME が「離した」を送らずに接続を保ったままなら、アプリにはキーが押されたまま残るので、緊急解除で復旧します。
- 繰り返し:Seinas には、キーの繰り返しのタイマーがありません(繰り返しは、アプリと IME が `repeat_info` で行う)。アプリの繰り返しは `wl_keyboard` の leave で止まり、IME の繰り返しは、接続を切ると届かなくなります。
- 戻すのは、IME が新しい接続でつなぎ直したときだけです。古い接続のクリーンアップ(Smithay の部品で、古い input method と grab の destroy)が済むまでは、新しい `get_input_method` を断ります(ログに `the released input method is not cleaned up yet`)。Smithay 0.7.0 の destroy は、どの IME のものでも seat の instance と grab を空にするため、新しい IME の grab を消さないようにするためです。新しい IME がつながると、フォーカスのあるアプリに text-input の enter が届き、同じ欄で続けて打てます(クリックし直さなくてよい)。
- 保証していない範囲:serial の合わない古い要求が、新しいアプリに入りえます(Smithay 0.7.0 は、IME の文字の要求を、届いたその場で、そのときアクティブな text-input へ送るため)。commit の前に切れた IME の commit_string は、アプリの pending の状態に残り、アプリの作りによっては、後の done で適用されます。次の課題で扱います(ADR 0004 の 1.5)。

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

## 候補ウィンドウ

IME の候補ウィンドウ(input popup surface)は、Seinas が置きます(ADR 0004 の 1.11)。

- 見せるのは、IME が有効で、親(text-input のフォーカスのあるウィンドウ)があるときだけです。フォーカスが移って IME が deactivate になると隠し、新しいウィンドウで activate になると、そのウィンドウを親にして見せます。
- 置く位置は、親のウィンドウのコンテンツの左上に、アプリが伝えたカーソルの矩形(`set_cursor_rectangle`)を足した所の下で、左の端をカーソルにそろえます。画面の下の端からはみ出すなら、カーソルの矩形の上に置きます。それでもはみ出す分は、画面の中へ寄せます(右の端、左の端、上の端)。
- 位置は、描くたびに求め直します。ウィンドウを動かしたとき、カーソルの矩形が変わったとき、出力の大きさが変わったときも、画面の中に収まります。
- 重ね方は、overlay の layer surface の下、top の layer surface とウィンドウの上です。ポインターのイベントも、同じ順で候補ウィンドウが受けます(候補を押して選べるように)。
- 親のウィンドウが最小化しているときは、描きません。

## IME は同時に 1 つ

IME が 1 つつないでいる間は、2 つ目からの `get_input_method` を断ります。

- 断り方:2 つ目の要求は、Smithay の部品へ渡さずに Seinas が受け、Seinas の側の入れ物でオブジェクトを作って、すぐ `unavailable` を送ります。その後の要求(grab_keyboard、候補ウィンドウなど)も、Seinas の側で受けて捨てます。ログには `an input method was refused (another input method is running)` を出します。
- 理由:Smithay 0.7.0 の 2 つの不具合を避けるためです。
  - `InputMethodHandle::add_instance` は、2 つ目が来ると、前からある IME の方へ `unavailable` を送り、新しい方には何も送りません。
  - 2 つ目の IME も、1 つ目と同じ keyboard の grab の入れ物を指します。2 つ目が grab を取ってから手放すと、1 つ目の grab まで外れます。1 つ目の IME は、知らされないまま grab を失います(キーが届かなくなる)。コンポジタは落ちません。Smithay だけでは、入れ物の grab のオブジェクトと seat の keyboard の grab は、いつも一緒に付き、一緒に外れるためです。

  2 つ目の要求が Smithay の部品に一度も届かなければ、どちらも起きません。
- 1 つ目の IME の `zwp_input_method_v2` と、keyboard の grab が両方とも無くなったら(ふつうは、接続を切ったとき)、次の IME を受け入れます。

## 試し方

### CI のテスト

`cargo test -p seinas-standalone --test ime` で、fake screen とテスト用の入力インターフェースを使って確かめます。IME には、テスト用の IME(`test-client ime`)を使います。fcitx5 などの IME を入れる必要はありません。

| テスト | 確かめること |
| --- | --- |
| `keys_go_through_the_ime_and_come_back_to_the_window` | キーが IME を通り、IME が virtual-keyboard で戻したキーがウィンドウに届く |
| `the_input_method_globals_are_only_on_the_ime_socket` | input-method と virtual-keyboard の global は IME のソケットにだけあり、シェルの部品のソケットとふだんのソケットには無い。IME のソケットには layer-shell と foreign-toplevel が無い。シェルの部品のソケットからつないだ IME は、準備の段階で失敗する |
| `the_input_method_globals_need_a_keyboard` | xkb のファイルが無く、seat にキーボードを足せなかった構成では、IME のソケットにも input-method と virtual-keyboard の global が無い(キーボードのある構成では、両方ある) |
| `the_compositor_starts_without_an_xkb_include_path` | xkb の置き場が無く、xkbcommon の context そのものを作れない構成でも、コンポジタが落ちずに、キーボードの無い構成として動く(IME の global も無い) |
| `alt_tab_works_while_the_ime_has_the_keyboard` | IME が grab している間も、Alt+Tab が効き、Tab は IME に渡らない。切り替えた先には、IME が戻した「Alt を離した」が届かない |
| `alt_through_the_ime_during_alt_tab_reaches_only_the_window_that_got_the_press` | Alt+Tab で行き来しても、IME が戻した Alt の押す・離すは、押したことを受けたウィンドウにだけ届き、切り替えた先で押して離した Alt は両方とも届く |
| `keys_go_straight_to_the_window_after_the_ime_leaves` | IME が去ったら、直接の道になる |
| `keys_go_straight_to_the_window_after_the_ime_releases_the_keyboard` | IME が grab を手放したら、直接の道になる |
| `a_virtual_keyboard_needs_the_accepted_input_method` | `get_input_method` を受け入れていないクライアントの virtual-keyboard は unauthorized で断られ、受け入れた IME が input method を destroy した後は、その virtual-keyboard のキーがウィンドウに届かない |
| `a_second_input_method_on_the_same_connection_keeps_the_first` | 同じ接続からの 2 つ目の `get_input_method` は断られ、1 つ目の grab と virtual-keyboard は使える |
| `virtual_keyboards_of_one_generation_work_in_any_order` | 同じ世代で A・B の順に作った virtual-keyboard を、B から先に使っても、両方とも使える |
| `virtual_keyboards_of_two_generations_are_not_mixed_up` | 前の世代で作った A と今の世代で作った B が両方待っているとき、B が最初の要求を送ると、どちらも認めず、IME の接続を `unauthorized` のエラーで切って、ログに理由を出す。つなぎ直した IME の virtual-keyboard は使える |
| `a_virtual_keyboard_destroyed_before_its_first_request_does_not_shift_the_queue` | 作ってすぐ destroy するのを 10 回くり返しても、上限にかからず、次に作ったものが使える |
| `too_many_waiting_virtual_keyboards_are_refused` | 要求を送らずに 9 個目を作ると unauthorized で切られ、その後につないだ IME はふつうに使える |
| `too_many_live_virtual_keyboards_are_refused` | 最初の要求を送ったものも数えて、生きている virtual-keyboard が 16 個ある所で次を作ると unauthorized で切られ、ログに理由が出る |
| `a_virtual_keyboard_key_released_after_the_grab_release_is_not_left_pressed` | virtual-keyboard で押し、grab を release した後に virtual-keyboard で離すと、離したことがウィンドウに届く |
| `a_virtual_keyboard_key_left_pressed_after_the_grab_release_is_released_with_the_virtual_keyboard` | grab を release しただけでは、virtual-keyboard で押したキーは押されたまま(離したことも leave も届かない)。その virtual-keyboard を destroy すると、離したことが届く |
| `a_virtual_keyboard_destroyed_with_a_key_pressed_releases_it` | virtual-keyboard で押したまま destroy すると、離したことがウィンドウに届き、ログに出る |
| `a_disconnected_ime_releases_the_keys_of_its_virtual_keyboard` | IME の接続が切れると、leave の送り直しより先に、押したままのキーの離したことが届く |
| `a_destroyed_input_method_releases_the_keys_of_its_virtual_keyboard` | input method の destroy(認可の取り消し)でも同じ |
| `the_release_key_releases_the_keys_of_the_virtual_keyboards` | 緊急解除でも同じ。物理のキーボードで押している Alt と Shift は離さない |
| `a_virtual_keyboard_key_is_released_once_when_the_destroy_and_the_disconnection_follow` | destroy の後に接続が切れても、離したことは 1 回だけ |
| `a_key_held_on_the_physical_keyboard_is_not_released_with_a_virtual_keyboard` | 同じキーを物理のキーボードも押しているときは、virtual-keyboard が壊れても離さない |
| `only_the_keys_of_the_broken_virtual_keyboard_are_released` | 2 つの virtual-keyboard が押しているとき、壊れた方だけが押しているキーだけを離す |
| `keys_are_not_released_after_the_focus_moved` | フォーカスが別のウィンドウへ動いた後に壊れても、どちらのウィンドウにも離したことを送らない |
| `modifiers_set_by_a_virtual_keyboard_are_restored_when_it_breaks` | キーを押さずに modifiers だけを渡した virtual-keyboard が壊れると、modifiers が物理のキーボードの状態に戻る |
| `the_release_key_goes_to_the_window_without_the_ime_socket` | `--no-ime-socket` の構成では、Alt+Shift+F12 の押下と解放が、全部ウィンドウへ届く(緊急解除のキーを予約しない) |
| `the_release_key_is_reserved_while_no_ime_is_connected` | IME のためのソケットを開く構成では、IME がつながっていなくても F12 を渡さず(Alt と Shift は渡る)、続けて F12 だけを押せば届く |
| `a_virtual_keyboard_request_refused_before_the_first_keymap_is_not_cleaned_up_later` | seat で最初の keymap が届く前に、Smithay が断った virtual-keyboard の modifiers は記録せず、接続が切れた後に、渡していない modifiers を送り直さない(ログに `left pressed by a virtual keyboard` を出さない) |
| `a_second_ime_is_refused_and_the_first_keeps_working` | 2 つ目の IME に unavailable が届き、2 つ目が grab を取って去っても、1 つ目は使える |
| `preedit_and_commit_reach_the_window` | `a`・`i` が変換中の文字「あ」「あい」になり、Enter で「あい」が確定してウィンドウに届く。IME が使ったキーは、ウィンドウの wl_keyboard に届かない |
| `a_window_opened_after_the_ime_gets_the_text_input` | IME の後に開いたウィンドウにも、text-input の enter が届き、変換中の文字が届く |
| `a_disconnected_ime_returns_the_keys_and_the_window_resets_its_preedit` | IME が commit_string を送り、commit の前に接続を切ると、アプリには text-input の leave だけが届き(done は届かない)、アプリが変換中の文字を消す。押されたキーの無い enter が送り直され、次のキーは直接届く |
| `the_release_key_closes_the_ime_and_keys_go_straight_to_the_window` | delete_surrounding を送った IME を緊急解除すると、IME の接続が切れ、アプリに text-input の leave が届く(done は届かない)。送り直した enter には、押している Alt と Shift だけが入り、F12 は入らない。次のキーは直接届く |
| `an_ime_that_swallows_a_key_is_not_bypassed` | 文字を出さずにキーを使う IME(`--swallow`)は、間を空けて何度使っても、直接の道にならない |
| `a_key_held_on_the_keyboard_and_through_the_ime_is_not_left_pressed` | 物理のキーボードで押したままのキーを、IME も virtual-keyboard で押していたとき、緊急解除の後の enter にそのキーが入り、離したことが直接届く |
| `a_key_pressed_only_through_the_ime_is_not_left_pressed` | IME が virtual-keyboard で押したまま(離したことをためている)のキーは、緊急解除の後の enter に入らない |
| `the_release_key_works_once_while_it_is_held` | 緊急解除のキーを押し続けた繰り返しの押下では、新しくつないだ IME を切らない。離した後に押せば、また解除する |
| `a_virtual_keyboard_key_does_not_release_the_ime` | IME が virtual-keyboard で送った Alt+Shift+F12 では、緊急解除にならない |
| `keys_reach_the_window_after_the_ime_destroys_only_its_input_method` | IME が input method だけを destroy し、grab を残しても、次のキーがウィンドウに届き、IME には届かない |
| `the_cleanup_runs_once_whatever_the_order` | input method が先・grab が先のどちらの順で消えても、leave の送り直しは 1 回で、次のキーは直接届く |
| `the_old_cleanup_does_not_remove_the_grab_of_a_new_ime` | 古い世代の grab が残っていても、別の接続の新しい IME を受け入れ、古い grab の release と古い接続の切断で、新しい IME の grab は外れない(手放したログも出ない) |
| `the_old_grab_on_the_same_connection_does_not_remove_the_new_grab` | 同じ接続で、input method だけを destroy した後に次の input method で grab を取ると、古い grab を release しても、キーは新しい grab に届く(手放したログも出ない) |
| `a_second_release_before_the_cleanup_says_so` | 緊急解除の後、クリーンアップの前にもう一度押すと、「no input method is running」ではなく、クリーンアップの途中であることをログに書く |
| `a_new_ime_connection_continues_in_the_same_field` | 古い IME が去った後、新しい接続の IME がつながると、同じウィンドウの同じ欄に enter が届き、クリックし直さずに続けて打てる |
| `the_candidate_window_follows_the_cursor_inside_the_output` | 候補ウィンドウ(40x30)が、カーソルの矩形の下に出て、カーソルの矩形を右の端へ動かすと、画面の中へ寄る |
| `the_candidate_window_goes_above_the_cursor_at_the_bottom_edge` | 下に入らないときは、カーソルの矩形の上に出る |
| `the_candidate_window_stays_inside_a_resized_output` | 出力の高さを縮めると、下から上へ移る |
| `the_candidate_window_hides_when_the_focus_leaves` | フォーカスが text-input を使わないウィンドウへ移ると、候補ウィンドウが消える |
| `focus_changes_move_the_text_input_and_the_activation` | Alt+Tab・最小化・閉じるで、leave・enter と deactivate・activate が届き、変換中の文字が新しいフォーカスのウィンドウに届く |

### テスト用の IME

`test-client ime` は、IME のためのソケットへつなぎます。`WAYLAND_DISPLAY` に、そのソケットの名前(`XDG_RUNTIME_DIR` の下なら `seinas-0-ime`)か、絶対パスを入れます。

```bash
WAYLAND_DISPLAY=seinas-0-ime target/debug/test-client ime
```

- keyboard を grab し、grab で届いた keymap を、そのまま virtual-keyboard の keymap にします。
- IME が有効(activate を受けた)なら、`a`・`i`・`u` を変換中の文字の「あ」「い」「う」にし、変換中の文字があるときの Enter で確定します。
- ほかのキーは、virtual-keyboard でアプリへ戻します。
- 標準入力の `release` で grab を手放し、`quit` で接続を切って終わります。
- `--popup WxH` を付けると、その大きさの候補ウィンドウを、紫(8000ff)の単色で出します。届いたカーソルの矩形を `ime: popup rectangle X Y W H` の行で書きます。
- `--swallow KEY` を付けると、evdev の番号 KEY のキーを、文字を出さずに使います(押下も解放も、何も返さない。半角/全角のように、IME の状態だけを変えるキーのまね)。
- 標準入力の `hold` で、キーを受けて送る要求(変換中の文字、確定、virtual-keyboard のキーと修飾キー)をためはじめ、`send` で、ためた順に送ります(遅れて届く返事のまね)。
- 標準入力の `commit-string TEXT`・`delete-surrounding BEFORE AFTER` は、その要求だけを送り、commit は送りません(commit の前に切れる IME のまね)。
- 標準入力の `vk-press CODE`・`vk-release CODE`・`vk-mods DEPRESSED` は、受けたキーと関係なく、virtual-keyboard でキーと修飾キーの状態を送ります。`again` は、同じ接続から 2 つ目の `get_input_method` を頼み、`destroy-input-method` は、input method だけを destroy します。`--virtual-keyboard-only` は、`get_input_method` を頼まずに virtual-keyboard を作ります(認可の無いクライアントのまね)。
- くわしくは `crates/seinas-test-clients/src/ime.rs` にあります。

### テスト用のアプリ

`test-client window --text-input` は、ふだんのソケットへつなぎ、text-input-v3 を使います。enter が届くたびに有効にして、カーソルの矩形(`--cursor X,Y,W,H`。既定は 10,20,2,16)を送ります。届いた enter・leave・preedit・commit・done を、`window: preedit あい` のような行で標準出力に書きます。text-input-v3 のとおり、届いた状態は done で効かせ(確定した文字を効かせたら `window: applied commit あい`)、leave では変換中の文字を消します(消すものがあれば `window: reset preedit`)。

```bash
WAYLAND_DISPLAY=seinas-0 target/debug/test-client window --text-input
```

位置の決まり(`popup_location`)のユニットテストは、`cargo test -p seinas-frontend --lib ime_popup` です。

### 入れ子の構成で試す手順(運用者の方へ)

入れ子の構成(WSLg の中の `seinas`)では、キーを親のウィンドウから打つので、CI ではなく手で確かめます。IME には、テスト用の IME を使います(fcitx5 などを入れる必要はありません)。

1. ビルドする:`cargo build --workspace --locked` と `tools/fetch-fonts.sh`。
2. 端末 1 で、入れ子の Seinas を起動する:`target/debug/seinas`。ふだんのソケットは `seinas-0`、シェルの部品のためのソケットは `seinas-0-shell`、IME のためのソケットは `seinas-0-ime` です(どれも `XDG_RUNTIME_DIR` の下)。
3. 端末 2 で、テスト用のアプリを出す:`WAYLAND_DISPLAY=seinas-0 target/debug/test-client window --text-input --title あ`。
4. 端末 3 で、テスト用の IME をつなぐ:`WAYLAND_DISPLAY=seinas-0-ime target/debug/test-client ime --popup 40x30`。端末 3 に `ime: ready` と `ime: activate` が出ます。
5. Seinas のウィンドウをクリックしてキーボードのフォーカスを渡し、`a`、`i`、Enter を打つ。端末 2 に `window: preedit あ`、`window: preedit あい`、`window: commit あい` が出て、Seinas のウィンドウの中、アプリのウィンドウのカーソルの矩形(コンテンツの左上から 10, 20)の下に、紫の候補ウィンドウが出ていることを見る。
6. `x` を打つ。端末 3 に `ime: key 45 pressed`、端末 2 に `window: key 45 pressed` が出る(IME が戻したキー)。
7. Alt+Tab を試すなら、端末 2 と同じものをもう 1 つ出してから、Seinas のウィンドウの中で Alt+Tab を打つ。切り替えたウィンドウに `window: text-input enter` が出て、候補ウィンドウがそのウィンドウへ移る。
8. 端末 3 で `quit` を打ち、IME を終える。端末 1 のログに `the input method left` が出て、`x` が端末 2 に直接届く(端末 3 には出ない)。
9. Seinas のウィンドウの大きさを、親の側で小さくする。候補ウィンドウが、Seinas のウィンドウの中に収まったままであることを見る(手順 4 と 5 をもう一度行ってから)。

うまくいかないとき:端末 3 で `the compositor does not offer zwp_input_method_manager_v2` と出たら、`WAYLAND_DISPLAY` が `seinas-0-ime` になっているかを確かめてください(ふだんのソケットとシェルの部品のソケットからは見えません)。
