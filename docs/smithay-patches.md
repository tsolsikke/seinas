# Smithay の局所パッチ

Seinas は Smithay 0.7.0 を使います。見つけた不具合は Seinas の側で直し、GitHub の fork(`tsolsikke/smithay`)のブランチ `seinas/0.7.0` に積んで、`Cargo.toml` の `[patch.crates-io]` から rev を固定して参照します(ADR 0006)。上流との折衝はしません。

- ブランチ:`seinas/0.7.0`(タグ `v0.7.0` = `a166cf4c` から。crates.io の smithay 0.7.0 と同じソースであることを、`src/` の比べで確かめた。2026-10-25)
- 参照の形:`Cargo.toml` の `[patch.crates-io]` に `smithay = { git = "https://github.com/tsolsikke/smithay.git", rev = "793c042e5583dcc75e2e92ce072087be38cb7419" }`(2026-10-25。path の形ではコミットしない)
- ブランチは force push しない(rev を固定して参照するため。直しは上に積む)。

## パッチの一覧

| コミット | 中身 | 理由(Seinas で起きること) | 上流の状態(2026-10-25) | 回帰のテスト | 次のバージョンで外せるか |
| --- | --- | --- | --- | --- | --- |
| `01d2b90d` input_method: only release the keyboard grab when the current grab object is destroyed | `zwp_input_method_keyboard_grab_v2` の破棄で、それが seat の今の grab のオブジェクトのときだけ、keyboard の grab を外す | input method だけを destroy して grab を残した IME の後に、新しい IME が grab を取ると、古い grab の破棄が新しい grab を外し、新しい IME にキーが届かなくなる(ADR 0004 の 1.5)。Seinas は、古い grab が残る間は新しい IME を断る守りで避けていた | 直っていない(master `24647fc8` の `destroyed` も、無条件に外す)。関係する PR #2147(input-method の取り次ぎの作り直し)は未マージ | Smithay:`releasing_the_grab_of_a_destroyed_input_method_keeps_the_new_grab`(`bbce097c`)。Seinas:`the_old_cleanup_does_not_remove_the_grab_of_a_new_ime`(別の接続)、`the_old_grab_on_the_same_connection_does_not_remove_the_new_grab`(同じ接続。ADR 0004 の 1.5) | 上流で直ったら外せる。外す前に、上の Seinas の 2 本が通ることを確かめる |
| `c7a79dc1` shm: don't panic or remap the pool on a wl_shm_pool.resize with a size of 0 or less | `wl_shm_pool.resize` の大きさが 0 以下のとき、プロトコルエラーの後に return する | `resize(0)` で、`NonZeroUsize::try_from(0).unwrap()` の panic でコンポジタが止まる(どのクライアントでも、要求 1 つで止められる。2026-10-25 に Seinas の seinas-standalone で確かめた)。負の値は、大きな値として pool を外してから mmap に失敗する | 直っていない(master も同じ形) | Smithay:`resizing_a_pool_to_zero_is_a_protocol_error`、`resizing_a_pool_to_a_negative_size_is_a_protocol_error`、`a_pool_can_grow`(`891533e1`)。Seinas:`tests/shm_pool_resize.rs` の 3 本(`test-client pool-resize SIZE`。resize(0) と resize(-1) でそのクライアントだけが切られ、コンポジタは動き続ける) | 上流で直ったら外せる |
| `e132b1c8` input_method: apply the input method state on commit, and discard commits with an old serial | IME の `commit_string`・`set_preedit_string`・`delete_surrounding_text` を、input method の保留の状態に置き、`commit` の serial が送った done の数と同じときだけ、アクティブな text-input へ送る。違えば捨てる | IME の要求は、届いたその場でアクティブな text-input へ送られ、serial の違う commit には `done(0)` だけが送られていた。text-input-v3 のアプリは変更を適用するので、フォーカスを変える前の応答(古い入力欄への変換中の文字)が、新しいアプリに入った(ADR 0004 の 1.5 と 4 の項目 3) | 直っていない(master `24647fc8` も、届いたその場で送り、serial の違う commit には `done(0)` を送るだけ。2026-10-25 に gh で読んで確かめた)。上流の PR #2215(未マージ。2026-10-13)は、serial の違う commit も「いつも適用する」向き(text-input へ `done(false)` を送る)で、この直しと逆向き。それが入ったバージョンへ替えるときは、同じ `Commit` の処理がぶつかり、当て直しが難しくなりうる(どちらの向きを採るかを決め直す) | Smithay:`a_commit_with_an_old_serial_does_not_reach_the_text_input`(`2811a28c`)。Seinas:`an_answer_sent_before_a_focus_change_does_not_reach_the_new_window`、`an_answer_for_a_window_does_not_reach_it_after_the_focus_comes_back`、`an_answer_sent_before_the_text_input_is_enabled_again_does_not_reach_it`、`a_late_answer_without_a_focus_change_still_reaches_the_window` | 上流で同じ向きに直ったら外せる。PR #2215 の向きで入ったら外せない(外すと、フォーカスを変える前の応答が新しいアプリに入る) |
| `a4dc9e68` input_method: reset the pending input method state on activate | activate を送るときに、input method の保留の状態(`e132b1c8` で置いたもの)を空にする | `e132b1c8` だけでは、activate より前に置かれた要求(前の入力欄への変換中の文字、inactive の間に送られた要求)が保留に残り、activate の後の正しい serial の commit で、新しい入力欄に入った。仕様の activate は、これらの要求の状態をリセットする(ADR 0004 の 1.16) | 上流には保留の状態が無い(届いたその場で送る)ので、当たらない | Smithay:`a_preedit_set_before_the_activate_is_not_applied_by_a_later_commit`(`793c042e`)。Seinas:`a_preedit_committed_only_after_a_focus_change_does_not_reach_the_new_window`、`requests_sent_while_the_ime_is_inactive_reach_no_window` | `e132b1c8` と一緒に外す |

積まなかったもの:

- キーボードの無い seat での `get_input_method` の panic の直し(手元の `f6913407`・`8c197939`)。Seinas は、キーボードの無い seat では input-method と virtual-keyboard の global を作らず、`get_input_method` も Smithay へ渡す前に断るので、この道に入らない(`the_input_method_globals_need_a_keyboard` が確かめる)。fork を小さく保つため、Seinas が当たらないものは積まない。Seinas の側のこの守り(キーボードの無い seat では global を作らず、`get_input_method` を断る)を外すときに積む(運用者の決定、2026-10-25)。

## 取り込む候補

まだ積まないが、時期が来たら積むもの。

| 上流のコミット | 中身 | 当たる場面 | 取り込む時期 |
| --- | --- | --- | --- |
| `8a270232` input: send keymap updates to grabs(2026-10-13。0.7.0 の後) | keyboard の keymap が替わったとき(と、virtual-keyboard が別の keymap でキーを送るとき)に、IME の keyboard の grab へも新しい keymap を送る。0.7.0 の `update_xkb_state` は、新しい keymap を `wl_keyboard` へだけ送り、grab へは送らない(`src/input/keyboard/mod.rs` 770〜797 行) | 入れ子の構成で、親の keymap に置き換えるとき(IME が grab している間に keymap が替わると、IME は古い keymap のままキーを読む)。seinas-standalone には keymap を変える道が無いので、今は当たらない | Kieli の第 2 段階(Kieli を Seinas につなぐ段階)に入る前(運用者の決定、2026-10-25)。master(`Dispatch2` の形)の上に書かれ、`input_method_keyboard_grab.rs` も変えるので、`01d2b90d` と合わせて 0.7.0 の形に移して積む。ADR 0004 の 4 の項目 4 の keymap の変更の確かめと一緒に行う |

## 残る限り

- 今の IME の grab のオブジェクトが破棄されると、Smithay は keyboard の grab を、それが IME の grab かどうかを見ずに外す(0.7.0 と上流の master と同じ)。IME でない grab が置かれている間に今の IME の grab が破棄されると、その grab も外れる。Seinas は、IME のほかに keyboard の grab を使わないので、今は当たらない(2026-10-25 に、Smithay の側の一時的なテストで確かめた)。fork には直しを足さない。IME 以外の keyboard の grab を足すときに見直す(運用者の決定、2026-10-25)。

## 次のバージョンへ替えるときの手順(下書き)

1. 新しいバージョンのタグから、新しいブランチ `seinas/<バージョン>` を作る(前のブランチは書き換えない。force push しない)。
2. 上の一覧のパッチを、1 つずつ確かめる。上流で直っていれば積まない。直っていなければ、新しいバージョンの形に移して積む(回帰のテストも一緒に)。
3. Smithay の側のテスト(直した所にかかわるもの)を通す。
4. Seinas の `[patch.crates-io]` を、一時的に path で新しいブランチへ向け、Seinas のテストの全体(ホストと musl)を通す。パッチを外すと、上の回帰のテストが落ちることを確かめる。
5. 運用者が fork へ push した後に、`[patch.crates-io]` を git と rev の形にし、`Cargo.toml` の `smithay` のバージョンと一緒にコミットする。
6. この文書の一覧を、新しいブランチの hash と、上流の状態に合わせて書き直す。

input-method の `Commit` の処理は、上流の PR #2215(serial の違う commit も適用する向き)が入ると、`e132b1c8` と逆向きにぶつかる。そのバージョンへ替えるときは、2 の前に、どちらの向きを採るかを運用者と決め直す(ADR 0004 の 1.16)。

wayland-backend(と wayland-server)を上げるときは、Seinas の側の作ったばかりの grab のオブジェクトの見分け方(`crates/seinas-frontend/src/ime.rs` の `new_grab_object`)が頼っている順(要求の新しいオブジェクトを data の無い形で先に作り、要求の処理の後で data を入れる。wayland-backend 0.3.17)が変わっていないかを確かめる(ADR 0004 の 1.5)。変わると、同じ接続の古い grab と新しい grab を見分けられなくなり、`the_old_grab_on_the_same_connection_does_not_remove_the_new_grab`(jq)が落ちるので、気づける。
