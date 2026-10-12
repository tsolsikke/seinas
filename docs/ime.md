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

Alt+Tab の最中も、Alt は IME へ渡ります。IME が戻した、Alt を離したことは、フォーカスが移った後で、新しいフォーカスのクライアントへ届くことがあります。前のクライアントには leave が届いているので、Alt が押されたまま残るクライアントはありません。

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
| `alt_tab_works_while_the_ime_has_the_keyboard` | IME が grab している間も、Alt+Tab が効き、Tab は IME に渡らない |
| `keys_go_straight_to_the_window_after_the_ime_leaves` | IME が去ったら、直接の道になる |
| `keys_go_straight_to_the_window_after_the_ime_releases_the_keyboard` | IME が grab を手放したら、直接の道になる |
| `a_second_ime_is_refused_and_the_first_keeps_working` | 2 つ目の IME に unavailable が届き、2 つ目が grab を取って去っても、1 つ目は使える |

### テスト用の IME

`test-client ime` は、シェルの部品のためのソケットへつなぎます。`WAYLAND_DISPLAY` に、そのソケットの名前(`XDG_RUNTIME_DIR` の下なら `seinas-0-shell`)か、絶対パスを入れます。

```bash
WAYLAND_DISPLAY=seinas-0-shell target/debug/test-client ime
```

- keyboard を grab し、grab で届いた keymap を、そのまま virtual-keyboard の keymap にします。
- ほかのキーは、virtual-keyboard でアプリへ戻します。
- 標準入力の `release` で grab を手放し、`quit` で接続を切って終わります。
- くわしくは `crates/seinas-test-clients/src/ime.rs` にあります。
