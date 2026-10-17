# ADR 0004: キーボードの入力は、IME(Kieli)を通る経路にする(text-input-v3・input-method-v2)

- 状態: 承認(2026-10-15 に書き直した。下の「取り消した決定」も参照)
- 日付: 2026-10-11(書き直し: 2026-10-15)

## 背景

日本語の入力のために、オリジナルの IME(Kieli)が、Seinas の上のアプリと連携できるようにしたい(運用者の最低要件。2026-10-07)。Seinas は、まだキーボードの入力を扱っていない(seat にはポインターだけがある。2026-10-11 の時点)。キーボードを入れる前に、IME を通る経路を決めておく。

Kieli について決まっていること:運用者の決定 K1〜K9(2026-10-14)に従う。Seinas の側では、これに足さない。Kieli の作り方への Seinas の希望は、下の「3. Seinas から Kieli への申し送り」に分けて書く。

この表はコピーである。正本は運用者の決定で、ずれたときは正本に従う。Kieli の設計文書ができたら、そちらへの参照に置き換える。

| | 運用者の決定(2026-10-14) |
| --- | --- |
| K1 | Rust で一から作る。独立したリポジトリ、MIT。外のコードはコピーしない |
| K2 | ふつうのローマ字入力とかな漢字変換を目指す(SKK 方式を中心にはしない) |
| K3 | 構成案は kieli-core(no_std + alloc)と、最初の利用者の kieli-cli(案の段階で、決定ではない) |
| K4 | Linux・ZeikOS・Windows 11 で使える作りを保つ。OS の IME として登録することは、最初のバージョンの必須にしない |
| K5 | 最初のバージョンはローマ字からひらがなへの変換だけ。辞書とかな漢字変換は入れない |
| K6 | 作る順は Seinas の最初のバージョン → Kieli の最初のバージョン → 統合。Kieli の最初のバージョンができたことと、統合ができたことは別々に判定する。Kieli を CUI に先に組み込む案は採らない |
| K7 | ZeikOS の `external/` に submodule として置く |
| K8 | Seinas と完璧に連携する。タスクバーの右に IME の表示を出す。依存ライブラリは、将来自前に置き換える前提で設計する |
| K9 | Kieli の本体は Smithay に依存させない |

方針は「Seinas は Kieli に準拠する」。Kieli の変換の決まりの正本は Kieli の設計、Seinas の安全の決まり(キーの配り方、権限、復旧)の正本はこの ADR とする。

### この ADR の分け方

| 部分 | 書くもの | 決めるのは |
| --- | --- | --- |
| 1. Seinas の決定 | コンポジタの決まり。fcitx5 など、どの IME がつないでも同じ | Seinas(運用者の承認) |
| 2. 標準の仕様が求めること | input-method-v2・text-input-v3・virtual-keyboard の仕様の文(出典つき)。Seinas も Kieli も守る | 仕様 |
| 3. Seinas から Kieli への申し送り | Kieli の作り方について、Seinas の側で見えていること。決まりではなく、Kieli の設計の材料 | Kieli の設計、または運用者(項目ごとに書く) |

### Smithay 0.7.0 で使えるもの

2026-10-11 に、ソースで確かめた。Cargo.lock のバージョンは、smithay 0.7.0、wayland-protocols 0.32.13(text-input-v3)、wayland-protocols-misc 0.3.12(input-method-v2、virtual-keyboard)。Smithay が作る global のバージョンは、3 つとも 1(text-input-v3 の XML は、バージョン 2 まである)。

| プロトコル | Smithay 0.7.0 | 備考 |
| --- | --- | --- |
| text-input-unstable-v3(`zwp_text_input_v3`) | あり(`wayland::text_input`) | キーボードのフォーカスと一緒に、text-input のフォーカスも動く(`wayland/seat/keyboard.rs`)。global の公開先は絞れない |
| input-method-unstable-v2(`zwp_input_method_v2`) | あり(`wayland::input_method`) | commit_string、preedit、delete_surrounding、commit の serial、キーボードの grab、候補ウィンドウ(`zwp_input_popup_surface_v2`)。公開先を絞れる |
| virtual-keyboard-unstable-v1(`zwp_virtual_keyboard_v1`) | あり(`wayland::virtual_keyboard`) | IME が使わなかったキーを、アプリへ戻すのに使う。公開先を絞れる |
| キーボード(`wl_keyboard`) | あり(`Seat::add_keyboard`) | keymap は、xkb の名前(rules・model・layout)から作る。文字列から作れるのは、作った後の `set_keymap_from_string` だけ |

## 1. Seinas の決定(コンポジタの決まり)

### 1.1 標準のプロトコルで取り次ぐ

- アプリは text-input-v3、IME は input-method-v2 と virtual-keyboard を使い、Seinas が両者のあいだを取り次ぐ。取り次ぎには、Smithay の 3 つの部品(text-input、input-method、virtual-keyboard)を使う。
- Kieli は、コンポジタの外の別のプロセスの Wayland のクライアントとしてつなぐ(運用者の決定 Q1。2026-10-14)。コンポジタに変換の本体を入れない。理由:障害を分けられる(Kieli が落ちても Seinas は動く)、ほかの IME と交換できる。費用:プロセスの管理と、1 往復の遅れ。
- IME が使わなかったキー(矢印、IME が切れているときの英字など)は、IME が virtual-keyboard でアプリへ戻す。

```text
                         ┌──────────────── Seinas(コンポジタ) ────────────────┐
キーボードの装置 ──key──→│ (a) Seinas のショートカット? ──はい──→ Seinas が使う   │
(入れ子では親の          │        │いいえ                                         │
 wl_keyboard)            │        ▼                                               │
                         │ (b) IME が keyboard を grab している? ──いいえ──┐      │
                         │        │はい                                    │      │
                         └────────┼───────────────────────────────────────┼──────┘
                                  ▼                                        ▼
                   IME(input-method-v2 のクライアント)            アプリ(wl_keyboard)
                     │ commit_string・preedit・delete_surrounding ─→ Seinas ─→ アプリ(text-input-v3)
                     │ 使わなかったキー(virtual-keyboard) ────────→ Seinas ─→ アプリ(wl_keyboard)
                     │ 候補ウィンドウ(input popup surface) ─────────────→ Seinas が、文字を打っている所の近くに置く
                     ▼
アプリ(text-input-v3)── 有効・無効、周りの文字、カーソルの矩形、入力の種類 ─→ Seinas ─→ IME
```

### 1.2 IME が grab していれば、いつも IME を通る

- IME が keyboard を grab している間は、Seinas のショートカット以外のキーを、全部 IME へ渡す。アプリが text-input を使っていないときも同じ(IME が virtual-keyboard で戻す)。
  - 理由:IME の切り替えのキー(半角/全角など)が、どのアプリの上でも同じに働くようにするため。どのキーをアプリへ戻すかを、IME の 1 か所で決められるため。
  - 費用:IME を通るぶん、1 往復(Seinas → IME → Seinas)遅れる。統合のときに、キーを押してからアプリに届くまでの時間を測る。
- Smithay の grab は、text-input が有効でないときも、キーを IME へ渡す形になっている(ソースで確かめた)。

### 1.3 IME へ渡したキーは、アプリへ送り直さない

- grab で IME へ渡したキーを、Seinas がアプリへ送ることはしない。IME が何も返さなくても、時間が過ぎても、IME が去っても同じ。仕様の求めること(2.3)で、同じキーが 2 度届くのも避けるため。
- IME は、キーを受けて、文字を出さずに使ってよい(切り替えのキーなど)。Seinas は、キーごとの返事を IME に求めない(2.2)。

### 1.4 IME が無いときは、直接の経路

- IME がつないでいない、去った、keyboard の grab を手放したときは、次のキーから、Seinas がアプリへ直接渡す経路(`wl_keyboard`)にする。IME が落ちても、キーボードが使えなくならないようにするため。

### 1.5 IME の復旧(2026-10-15 に決めた)

時間切れで「応答しない」と見なして直接の経路へ逃がす決まりは、2026-10-15 に取り消した(下の「取り消した決定」)。代わりに、次の 2 つで直接の経路に戻す。どちらも、IME にキーごとの返事を求めない。

- 接続断:IME のクライアントが去った、または keyboard の grab を手放したら、次のキーから直接の経路にする(1.4)。
- input method の destroy(2026-10-16 に決めた):IME が `zwp_input_method_v2` だけを destroy して、grab のオブジェクトを残していても、その世代の grab・virtual-keyboard の認可・候補ウィンドウを片付けて、直接の経路にする(IME が去ったときと同じ片付け)。仕様の根拠は input-method-unstable-v2.xml(wayland-protocols-misc 0.3.12)357〜363 行の `zwp_input_method_v2.destroy`:「Destroys the zwp_text_input_v2 object and any associated child objects, i.e. zwp_input_popup_surface_v2 and zwp_input_method_keyboard_grab_v2.」(対象を zwp_text_input_v2 と書いているのは XML の書き損じで、input method の destroy の説明)。
  - 片付けるのは、その接続・seat・世代のものだけ。keyboard の grab は、それが IME の grab のときだけ外す。物理のキーボードの状態には触れない。
  - 片付けは 1 回だけ行う。input method と grab の destroy の順が違っても、接続断と重なっても、`wl_keyboard` の leave と enter の送り直しは 1 回。
  - 古い世代の grab のオブジェクトが残っている間は、新しい `get_input_method` を断る(ログに `the destroyed input method still has a keyboard grab`)。古い grab の destroy(Smithay 0.7.0 の部品は、seat の grab を外す)が、新しい IME の grab を外さないようにするため。
    - これは暫定の守り(2026-10-16 に確かめた)。Smithay 0.7.0 の `src/wayland/input_method/input_method_keyboard_grab.rs` 106〜114 行の `destroyed` は、持ち主も世代も確かめずに、共有の grab を `None` にし(112 行 `data.handle.inner.lock().unwrap().grab = None`)、seat の keyboard の `unset_grab` を呼ぶ(113 行)。共有の grab は seat に 1 つで(`input_method_handle.rs` 30〜35 行の `InputMethod` の `keyboard_grab`)、どの grab のオブジェクトの user data も、その同じものを持つ(同じファイルの 277〜292 行の `GrabKeyboard`)。`unset_grab`(`src/input/keyboard/mod.rs` 892〜898 行)は、そのとき付いている grab を、何のものかを問わずに外す。そのため、古い grab を残したまま新しい IME を受け入れると、古い grab の destroy で、新しい IME の grab が外れる。
    - 本来の形は、古い grab を効力の無いものとして扱い、新しい IME を受け入れること。それには、grab の破棄の処理で、持ち主と世代を確かめることが要る。Smithay を変えずにできるか(Seinas の側で destroyed を包むなど)は、C1S4 で調べる。
    - 次の 2 つは、別の条件として扱う。1 つは「古い grab のリソースが残っているだけ」で、本来は新しい IME を受け入れられる(今断っているのは、上の Smithay の動きのため)。もう 1 つは「古い virtual-keyboard の世代を安全に見分けられない」(1.6 の限界)で、これは同じ接続のままでは安全にできず、再接続が要る。
- 緊急解除:Seinas が IME より先に見るショートカット(1.8)で、IME のクライアントの接続を切る(R-b)。
  - 既定のキーは Alt+Shift+F12。入れ子(WSLg)では、運用者が、届くこと・働くことを確かめた(2026-10-15)。ZeikOS では未確認(Seinas のキーボードの入力の装置が、まだ無い)。`seinas.conf` の `ime-release` で変えられる。
  - 発動するのは、物理のキーボード(入れ子では親の `wl_keyboard`)から来たキーだけ。IME が virtual-keyboard で送ったキーでは発動しない(Smithay 0.7.0 の virtual-keyboard のキーは、ショートカットの判定を通らず、フォーカスのあるクライアントへ直に届く)。
  - 押し続けても、1 回だけ発動する(同じキーを離すまで、繰り返しの押下は使って捨てる)。

緊急解除の順番:

1. 先に、その接続の IME の認可と世代を消す(1.6)。この後、その接続の input-method と virtual-keyboard の要求は捨てる。
2. その後で片付ける:Seinas が keyboard の grab を外し(次のキーから直接の経路)、候補ウィンドウを隠し、接続を切る。切った接続のクリーンアップ(Smithay の部品の destroy)は、Seinas がその場で行わせる。IME の側からは、ただの切断に見える。Seinas は wayland-server の `Client::kill` で接続を閉じるだけで、`wl_display.error` は送らない(wayland-backend 0.3.17 の `src/rs/server_impl/client.rs` 325〜328 行の `kill` は、切断の印を付けて `ClientData::disconnected` に理由を渡すだけ。`wl_display.error` を送るのは 274〜303 行の `post_error` の道だけ)。理由の文は、Seinas のログ(`a client disconnected: ProtocolError { … message: "the input method was released with the emergency key" }`)にだけ出る。IME は、読むと接続の終わり(EOF)、送ると EPIPE になる(2026-10-15 にテスト用の IME で確かめた。何も送っていなければ EOF で終わり、virtual-keyboard でキーを戻そうとしていれば「Broken pipe」で終わる)。
3. IME へ渡したキーは、アプリへ送り直さない(1.3)。それより後の入力は、アプリへ直接届ける。
4. IME が去った後(接続断のときも同じ)、フォーカスのあるアプリへ `wl_keyboard` の leave と enter を送り直す。IME が virtual-keyboard で押したキー(アプリには押されたまま)は enter に入れない。enter に入るのは、物理のキーボードで今押されていて、IME かアプリへ渡したキーだけ(Seinas が使ったショートカットのキーは入らない)。

text-input と `wl_keyboard` の leave は、別のもの:

| leave | 送るもの | 対象 | いつ |
| --- | --- | --- | --- |
| text-input-v3 の leave | Smithay の input-method の部品(IME の `zwp_input_method_v2` が destroy されたとき) | フォーカスのあるアプリの text-input | 接続を切った後のクリーンアップの中で、先に |
| `wl_keyboard` の leave と enter | Seinas | フォーカスのあるアプリの `wl_keyboard` | IME が去ったと分かった後に(上の 4) |

- 変換中の文字(preedit):Seinas から、空の preedit_string と done を送らない。done を送ると、先に届いていた commit_string まで、アプリに適用させるおそれがあるため。text-input の leave で、アプリが preedit を消すこと(text-input-v3 の leave の「The client should reset any preedit string previously set」)に任せる。
- 戻すのは、IME が新しい接続でつなぎ直したときだけ。古い接続のクリーンアップが終わるまで(Smithay の部品で、古い input method と grab の destroy が済むまで)は、新しい `get_input_method` を断る。古い接続の destroy が、新しい IME の grab や instance を消さないようにするため(Smithay 0.7.0 の destroy は、どの IME のものでも、seat の instance と grab を空にする)。
- Seinas には、キーの繰り返しのタイマーが無い(繰り返しは、アプリと IME が `repeat_info` で行う)。そのため、Seinas が止める繰り返しは無い。アプリの繰り返しは `wl_keyboard` の leave で止まり、IME の繰り返しは接続を切ると届かなくなる。
- 未解決(保証していない範囲):serial の合わない古い要求が、新しいアプリに入りうる。Smithay 0.7.0 は、IME の commit_string・preedit・delete_surrounding を、届いたその場で、そのときアクティブな text-input へ送るため(フォーカスを変えた直後に届いた古い要求も入る)。また、commit の前に切れた IME が送っていた commit_string は、アプリの pending の状態に残り、後の done で適用されることがある(アプリの作りによる)。次の課題で扱う。
- 自動で IME の止まりを見つけることが本当に要るようになったら、そのときに、明示の probe を持つ別の拡張などを設計する。標準の IME に、独自の返事の義務を課さない。

### 1.6 見せる相手と、IME の権限(2026-10-15 に決めた)

- text-input-v3 の global は、全部のクライアントに見せる(アプリが使うもの。Smithay の部品は、公開先を絞れない)。
- IME の権限を、シェルの部品の権限と分ける。これは「権限を分けること」で、相手が誰かを確かめる「認証」ではない。
- IME 専用のソケット:ふだんのソケットのパスの名前に `-ime` を入れたもの(名前に拡張子があれば、その前に入れる。`docs/standalone.md`)(入れ子は `seinas-0-ime`。seinas-standalone は `--ime-socket PATH` で変えられる)を listen する。
  - input-method-v2 と virtual-keyboard の global は、IME のソケットからつないだクライアントにだけ見せる。シェルのソケットとふだんのソケットには見せない。
  - IME のソケットには、layer-shell と foreign-toplevel を見せない。候補ウィンドウ(input popup surface)に要る `wl_compositor`・`wl_shm` などのふつうの global は見せる。
  - ソケットは、ほかのソケットと同じく wayland-server の `ListeningSocket` で作る(ロックファイルで、生きている別のインスタンスのソケットを消さない)。
- virtual-keyboard(A'):作れるのは、`get_input_method` を受け入れたクライアントだけ。ほかは、virtual-keyboard の `unauthorized` のエラーにする(2.5)。
  - 認可は、接続と、受け入れた IME の世代に結び付ける。seat は見ていない(Seinas の seat は 1 つだけなので、接続と世代で足りる。`create_virtual_keyboard` の seat の引数も見ずに Smithay へ渡す。2026-10-17 にコードに合わせて直した)。seat を増やすときは、seat も認可のキーに入れる必要がある(IME の世代と virtual-keyboard の待ち行列を seat ごとに持つ)。受け入れた input method が destroy されたとき、緊急解除で消したときに、認可も消え、それまでに作った virtual-keyboard からの要求も捨てる。新しく `get_input_method` を受け入れたら、世代を 1 つ進める。
  - 同じ接続からの 2 つ目の `get_input_method` を断っても(1.7)、1 つ目の認可は消さない。
  - 限界(2026-10-15 に受け入れ、2026-10-16 に小さく直した):Smithay が作る virtual-keyboard のオブジェクトの番号は、作る要求を処理する時点では分からない(wayland-server 0.31.14 の `New<I>` には中のオブジェクトを取り出す口が無く、Smithay 0.7.0 の `VirtualKeyboardUserData` は外から作れないので、作るときにマッピングできない。2026-10-16 に確かめた)。そのため、作ったときの世代はクライアントごとの待ち行列に置き、そのオブジェクトの最初の要求(destroy の要求を含む)で決める。待ち行列の決まり:今の世代で作ったものはどれも同じ世代なので、どの順で要求が来ても正しく決まる。IME が去ったときに待っていたものは「前の世代のもの」として数だけを残し、前の世代のものと今の世代のものが両方待っているときに要求が来たら、見分けられないとして、待っているものを全部認めない(安全側。今の世代のものも捨てられる)。要求を送らずに待てるのは 1 つのクライアントで 8 個までで、越えて作ると `unauthorized` のエラーにする。要求を送らずに destroy されたもの(接続断など)は待ち行列から外し、待つものが無くなったクライアントの記録は消す。作るときのマッピングは、C1S4 で比べ直す。
- 守れることと、守れないこと:
  - 守れる:タスクバーや壁紙の部品、ふつうのアプリは、input-method と virtual-keyboard の global が見えないので、キーの横取り(grab)と注入ができない。
  - 守れない:IME のソケットにつなげるプロセスは、どれも IME になれる。先にソケットにつないで、IME の枠(同時に 1 つ)を取ることも防げない。
  - Linux:ソケットは `XDG_RUNTIME_DIR`(0700)の下に作るので、つなげるのは同じ利用者のプロセスだけ(ファイルの所有者と権限の検査は、カーネルが行う)。同じ利用者のプロセスどうしは、分けられない。
  - ZeikOS(2026-10-15 に zeikos を読んで確かめた):unix ソケットの名前はカーネルの表で、ファイルシステムに inode を作らない(`kernel/src/socket.rs` の doc)。利用者の概念が無く(`kernel/src/syscall.rs` の `sys_stat` の doc:「st_uid / st_gid——利用者の概念が無い」)、`mkdir` も mode を見ない。そのため、0700 は効かず、どのプロセスも、どのソケットの名前にもつなげる。ZeikOS で効くのは、ソケットを分けることによる役割の分け方だけ。
- ログ:ふだんのログに、生のキー・keysym・preedit・確定した文字・周りの文字・ウィンドウのタイトルを出さない(タイトルは 2026-10-16 に足した。文書の名前などが入るため。応答しないクライアントを切るときのログは、接続とオブジェクトの id と、切る理由を書く)。キーを名前でログに出すのは、明示して付けるテスト用の `--log-keys` だけ(緊急解除のキーが届くかの確かめに使う)。script がログのファイルを作るときは、0700 のディレクトリと 0600 のファイルにする。パスワードの文脈(2.6)で、文字のキーを伏せる形(P1)は入れない(`--log-keys` は、ふだんは付けないため)。

### 1.7 IME は同時に 1 つ(seat ごと)

- IME は、seat ごとに同時に 1 つだけにする(Seinas の seat は 1 つ)。2 つ目の IME には、input-method-v2 の `unavailable` を送る(2.4)。
- Smithay 0.7.0 は、これを逆に送るので(下の「足りないもの」)、Seinas の側で避ける:2 つ目からの `get_input_method` を Smithay へ渡さずに Seinas が受けて断る(2026-10-12 に決めた。`docs/ime.md`)。

### 1.8 Seinas のショートカット

- コンポジタが使うキーは、IME へ渡す前に Seinas が見て、使ったら IME にもアプリにも渡さない(Smithay の `KeyboardHandle::input` の filter で、grab の前に止められる)。仕様は、コンポジタが一部のキーを grab へ渡さないことを認めている(2.3)。
- 理由:IME が止まっていても、ショートカット(と、作り直した後の緊急解除の操作)が効くようにするため。
- 今の一覧:ウィンドウの切り替え(既定は Alt+Tab と Alt+Shift+Tab。入れ子の構成では Alt+F6 と Alt+Shift+F6 も。`seinas.conf` で変えられる。`docs/config.md`)と、切り替えの最中の Esc(取り消し)。Seinas が使ったキーは、離したことも、どこへも渡さない。
- 緊急解除(1.5):既定は Alt+Shift+F12(入れ子では確かめた。ZeikOS では未確認)。`seinas.conf` の `ime-release` で変えられる。
- Seinas が予約するキーと、IME の切り替えのキーの分け方は、統合の前に決める(3 の申し送り)。

### 1.9 virtual-keyboard で戻ったキー

- IME が virtual-keyboard で戻したキーは、キーボードのフォーカスのあるクライアントへ届ける。
- 戻ったキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーのときだけ渡す(`docs/ime.md` の「キーの道」)。フォーカスを動かしたら、記録を空にする。どのクライアントにも、キーが押されたまま残らないようにするため。

### 1.10 変換中の文字と、text-input を使わないアプリ

- 変換中の文字(preedit)は、アプリが、文字を打っている所に描く(text-input-v3 の preedit_string。カーソルの位置も届く)。IME は、アプリの上に描かない(運用者の決定 Q5。2026-10-14)。
- text-input-v3 を使わないアプリ(zeyes など)では、変換しない。キーは、IME が virtual-keyboard でそのまま戻す(preedit を出す場所が無いため)。

### 1.11 候補ウィンドウを置く

- IME の候補ウィンドウ(input popup surface)は、Seinas が置く。アプリが伝えたカーソルの矩形(`set_cursor_rectangle`)の下に置き、画面の下に入らなければ上に置く。重ね方は、全部のウィンドウより手前(layer surface の top と同じ所)で、ポインターも受け取る(候補を押して選べるように)。
- 候補ウィンドウを誰が描くかは、Kieli の設計で決める(3 の申し送り)。Seinas は、input popup surface で届いたものを置くだけ。

### 1.12 キーボードと keymap(Seinas の側)

- seat にキーボードを足す。keymap の既定は、日本語の配列(xkb の layout `jp`、model `jp106`)にし、設定で `us` などに変えられるようにする。keymap は Seinas が決めて配る(アプリにも、grab で IME にも)。
- 入れ子の構成では、親の `wl_keyboard` から届くキー(evdev のキーの番号)を、そのまま Seinas の seat へ入れる。fbdev の構成(ZeikOS)では、ZeikOS のキーボードの装置から読む。
- キーボードのフォーカスは、activated のウィンドウ(いちばん手前のウィンドウ)にする。layer surface の `keyboard_interactivity`(メニュー)は、メニューを作るときに足す。

### 1.13 IME の状態の表示

- Kieli の状態(「あ」「A」など)は、タスクバーの右に出す(K8)。届け方は、まだ決めていない(3 の申し送り)。決まるまで、Seinas は出さない。

### 1.14 対応する範囲

- Seinas の IME の経路が対応するのは、当面「Linux 上の Seinas」と「ZeikOS 上の Seinas」。
- Linux のほかのデスクトップは、IME の側のプロトコルが違う(Sway は input-method-v2、KDE Plasma は input-method-v1、GNOME は IBus の D-Bus。出典:Fcitx の「Using Fcitx 5 on Wayland」、2025-09-20 更新)。input-method-v2 を、Linux の全部のデスクトップで同じ IME の接続の決まりとは説明しない。
- Windows 11(K4)は、Seinas が動かないので、この ADR の範囲の外。

### 1.15 分担

| 役目 | Seinas | IME(Kieli) | アプリ |
| --- | --- | --- | --- |
| キーボードの装置からキーを読む | ○ | | |
| keymap(日本語の配列)を決めて配る | ○ | 受け取る(使い方は申し送り) | 受け取る |
| ショートカット(と、緊急解除の操作) | ○ | | |
| キーを IME へ渡す、IME が無ければアプリへ渡す | ○ | | |
| IME の権限(誰を IME と認めるか) | ○ | | |
| キーの意味(ローマ字からかな、変換、切り替え) | | ○ | |
| 変換中の文字を描く | | | ○(text-input-v3) |
| 候補ウィンドウを描く | | (申し送り) | |
| 候補ウィンドウを置く | ○ | | |
| 文字を打っている所(カーソルの矩形、周りの文字)を伝える | 取り次ぐ | 受け取る | ○ |
| IME の状態の表示 | (申し送り) | (申し送り) | |

## 2. 標準の仕様が求めること

出典は、Cargo.lock のバージョンに入っている XML(行の番号も、その XML のもの)。

- input-method-v2・virtual-keyboard:wayland-protocols-misc 0.3.12 の `protocols/input-method-unstable-v2.xml`、`protocols/virtual-keyboard-unstable-v1.xml`
- text-input-v3:wayland-protocols 0.32.13 の `protocols/unstable/text-input/text-input-unstable-v3.xml`

ほかのバージョン(プロトコルの master や、Smithay を上げたとき)では、XML と照らし直す。

### 2.1 input-method-v2 は、実験的なプロトコル

- 「experimental and backward incompatible changes may be made」(input-method-unstable-v2.xml 43〜44 行)。使う XML・crate・interface のバージョン(今は 1)を固定して書く。

### 2.2 commit は、状態を適用する要求で、キーごとの返事ではない

- `zwp_input_method_v2.commit` は、commit_string・set_preedit_string・delete_surrounding_text で変えた pending の状態を、今の状態にする要求(279〜306 行。「Apply state changes from commit_string, set_preedit_string and delete_surrounding_text requests」)。
- serial は、その input-method のオブジェクトが送った done の数。違う serial の commit も受け付けるが、状態は変えない(298〜303 行)。
- 文字を出さずにキーを使うのは、ふつうの動き。文字の要求が返らないことだけでは、IME が止まったとは言えない。空の commit も、preedit を含む状態の更新として働きうるので、害の無い返事として扱えない。

### 2.3 grab へ渡したキーは、コンポジタがそれ以上扱わない

- 「The compositor should send all keyboard events on the seat to the grab holder」「the compositor may decide not to forward any particular event」「The compositor must not further process any event after it has been forwarded to the grab holder」(321〜337 行)。
- そのため、渡したキーをアプリへ送り直さない(1.3)。ショートカットを grab の前に止めるのは、「may decide not to forward」に当たる(1.8)。

### 2.4 input-method は、seat ごとに 1 つ

- 「There must be no more than one input method object per seat」(70 行)、「No more than one input method must be associated with any seat at any given time」(469〜470 行)。
- 2 つ目には、`unavailable` を、そのオブジェクトのただ 1 つのイベントとして送る。`unavailable` を受けた IME のオブジェクトは inert になり、destroy のほかの要求とイベントは無視される(340〜355 行)。
- inactive の状態で届いた要求も受け付けるが、serial の決まりと activate での状態のリセットで、次の text input には効かない(66〜68 行)。

### 2.5 virtual-keyboard は、信頼しないクライアントに使わせない

- 「If the compositor enables a keyboard to perform arbitrary actions, it should present an error when an untrusted client requests a new keyboard」(virtual-keyboard-unstable-v1.xml 101〜108 行。エラーは `unauthorized`、97〜99 行)。
- key の要求の前に、keymap が要る(54〜68 行の「Keymap must be set before issuing this request」)。keymap を渡す fd の種類(memfd など)は、仕様では決まっていない。

### 2.6 パスワードの文脈

- content_purpose の `password`・`pin` は、content_hint の `sensitive_data` と組み合わせる(text-input-unstable-v3.xml 240〜241 行)。`sensitive_data` は「typed text should not be stored」(210 行)、`hidden_text` は「characters should be hidden」(209 行)。
- text-input-v3 の XML はバージョン 2 まであり、バージョン 2 で preedit_hint などが増えた(555 行の `since="2"`)。名前だけで機能の範囲を決めない(Smithay 0.7.0 が作るのはバージョン 1)。

## 3. Seinas から Kieli への申し送り

Kieli の作り方について、Seinas の側で見えていることを、Kieli の設計の材料として渡す。Seinas の決まりではない。Kieli の設計で別の形に決まったら、Seinas はそれに合わせる(1 の決まりと、2 の仕様の範囲で)。

「GPT-6 の推奨」は、2026-10-14 のレビュー(Notion の話題レポート「ZeikOS・Seinas・Kieliの設計方針」)の推奨で、運用者が採ったものではない。

| 項目 | 申し送り | 決めるのは | GPT-6 の推奨 |
| --- | --- | --- | --- |
| つなぎ方 | Seinas への入口は、input-method-v2 と virtual-keyboard を使う、別のプロセスの Wayland のクライアント | 運用者(決定 Q1。2026-10-14) | 維持。変換の決まりは Kieli、フォーカス・権限・キーの配り方は Seinas |
| 入口の置き場(Q2) | Wayland の入口を、Kieli のリポジトリの中に置くか、kieli-core・kieli-cli との関係 | Kieli の設計 | Kieli のリポジトリの kieli-wayland。kieli-core は no_std + alloc、CLI と入口は std を使ってよい |
| 起動と、IME として認められること | Kieli は、起動の仕組みが起こす(コンポジタは起こさない。ADR 0002 の 5)。IME として認められる方法は、Seinas の IME の権限の設計(1.6)で決める | 運用者(ZeikOS の起動の仕組みと一緒に)と Seinas の設計 | IME の権限を、シェルの部品の権限と分ける |
| キーへの返事 | Seinas は、キーごとの返事を求めない(1.3、2.2)。文字を出さずにキーを使ってよい。空の commit を返事の代わりに送らない | Seinas の決定(Kieli に義務は無い) | 標準の IME に独自の返事の義務を課さない |
| 緊急解除のときの見え方 | 緊急解除(1.5)では、Seinas が IME の接続を閉じる。IME には `wl_display.error` は届かず、ただの切断に見える(読むと EOF、送ると EPIPE)。理由の文は Seinas のログにだけ出る。つなぎ直すのは、IME(または起動の仕組み)の役目で、新しい接続でつなげば戻る | 事実(Seinas の決定。2026-10-15 に確かめた) | — |
| 使わなかったキー | virtual-keyboard でアプリへ戻す。押したキーは、離したことも戻す | Kieli の設計 | 押下と解放の組、修飾キーの状態、繰り返しを 1 つの配り方の決まりで扱う |
| unavailable と、activate・deactivate | `unavailable` を受けたら、そのオブジェクトを捨てる(2.4)。deactivate・activate で、入力文脈の状態を作り直す | Kieli の設計(仕様の動き) | 入力欄ごとに 1 つのセッション。モードと未確定の文字を、プロセス全体の状態に置かない |
| 変換中の文字 | アプリが text-input-v3 の preedit で描く。text-input-v3 を使わないアプリでは変換しない | 運用者(決定 Q5。2026-10-14) | 維持。統合のテストには、専用の小さな文字入力のアプリを使う |
| 候補ウィンドウ(Q4) | 誰が描くか。最初のバージョン(K5)では要らない | Kieli の設計(かな漢字変換を入れる段階で) | Kieli の入口が popup を描き、Seinas が置く。描画のライブラリは、かな漢字変換に着手するときに決める |
| IME の状態の表示(Q6) | 状態(あ/A)をタスクバーの右へ届ける方法(K8) | Kieli の設計 | Kieli → Seinas → シェルの部品の経路。input-method-v2 だけでは足りないので、別に合意する拡張が要る。状態の正本は Kieli、表示はシェル。通信の実装は、タスクバーとの統合のときまで決めない |
| 切り替えのキー(Q7) | 半角/全角などと、英字配列での組み合わせ。Seinas が使うキー(1.8)は Kieli に届かない | Kieli の設計 | 統合の前に、システムの予約キーと IME の切り替えのキーの担当を決め、設定のぶつかりを検出する。virtual-keyboard で戻ったキーを、また IME へ回さない |
| keymap(Q8) | grab で届いた keymap(既定は jp106)の使い方。ZeikOS では、grab で届いた fd をそのまま戻すと使えないので、memfd に書き直して送る(`docs/zeikos-requirements.md`) | Kieli の設計 | 入口が keymap と修飾キーを解釈し、core には論理的な入力を渡す。ローマ字の表は core、物理的な配列の解釈は入口。memfd は ZeikOS での作り方で、Wayland の必須ではない |
| キーの繰り返し | IME が使ったキーの繰り返しは IME が行う(grab に repeat_info が届く)。戻したキーは、アプリが行う | Kieli の設計 | 時刻と繰り返しのタイマーは入口に置き、core に入れない |
| 依存(Q9) | Wayland の入口で使う crate | Kieli の設計 | wayland-client と、生成したプロトコルの定義を使い、ソケットへ直に書かない。依存は入口に閉じる。使うバージョン・ライセンス・XML の出どころを記録する |
| パスワードの文脈 | `sensitive_data` の入力を、ログや将来の学習に残さない(2.6) | Kieli の設計 | 同じ |
| 対応する範囲 | 当面は「Linux 上の Seinas」と「ZeikOS 上の Seinas」(1.14) | 運用者 | 次に Sway などを 1 つ選んで試す。GNOME・KDE は、要るようになったときに比べる |
| Windows 11(K4) | Seinas の決まりは、Wayland の入口にだけかかる。Windows の入口(TSF など)には、何もかからない | Kieli の設計 | TSF は後回し。core では、入力文脈ごとの状態、UTF-8 と UTF-16 の位置の変換の境目、描画を分けておく |

## 4. 判定:Kieli の最初のバージョンと、Seinas との統合

K6 のとおり、2 つを別々に判定する。

| 判定 | できたと言える条件 | 決めるのは |
| --- | --- | --- |
| Kieli の最初のバージョン | Kieli の設計が決める(K5:ローマ字からひらがな)。Seinas の側の条件は付けない。Seinas につながなくても判定できる(kieli-cli など) | Kieli の設計 |
| Seinas との統合 | テスト用の IME(`test-client ime`)の代わりに Kieli をつなぎ、`crates/seinas-standalone/tests/ime.rs` と同じ流れと、ふつうの入力、文字を出さないキー、フォーカスの切り替え、押しっぱなし、IME の接続断、遅れて届く応答、keymap の変更、text-input を使わないアプリが通ること。入れ子(WSLg)では、親の IME との二重変換が起きないことも見る | 運用者 |

## 取り消した決定

消さずに残す。

### 時間切れでの退避(2026-10-12 に入れた。2026-10-15 に取り消した)

元の文(決定の 2):

> - IME が動いていない(つないでいない、切断した)ときと、応答しないときは、Seinas がアプリへ直接渡す経路(`wl_keyboard`)に戻す。IME が落ちたり固まったりしても、キーボードが使えなくならないようにするため。
>   - 応答しない(1,000 ms は仮の値。2026-10-12 に入れた):キーの押下を IME へ送ってから 1,000 ms の間に、その IME のクライアントから要求が 1 つも届かなければ、応答しないと見なし、次のキーから直接の経路にする。IME へ送ったまま返ってこなかったキーは、アプリへ送り直さない(同じキーが 2 度届くのを避けるため)。数え方の決まりは次の 2 つ。
>     - 修飾キー(Shift、Ctrl、Alt など)だけの押下は数えない。IME は、修飾キーの状態を覚えるだけで、何も返さないことがあるため。
>     - 同じクライアントからの virtual-keyboard の要求も、返事として数える。使わなかったキーを virtual-keyboard で戻すのが、その IME の返事だから。
>   - Kieli は、キーの押下(修飾キーだけのものを除く)を受けるたびに、必ず何かを返す決まりにする(使ったキーは preedit などと input-method の commit、使わなかったキーは virtual-keyboard のキー)。そのため、正しく動いている Kieli は、上の条件に当たらない。ほかの IME では、当たることがある(値は、Kieli と試して決め直す)。
>   - 応答しないと見なした後に、その IME から要求が届いたら、次のキーから IME を通る経路に戻す。直接の経路にした・戻したことは、ログに出す。

(決定の 4 の「IME が固まっていても(上の、応答しないと見なす前の 1,000 ms の間も)」も、この決まりを前に置いた書き方だった。)

### Q3 の暫定の決定(2026-10-14。2026-10-15 に取り消した)

上の決まりと、文字を出さないキーとのぶつかりを避けるための、運用者の暫定の決定:

- C-1:文字を出さないキー(切り替えのキーなど)を受けたときも、IME は input-method の commit(空のもの)を 1 つ返す決まりを、Kieli の設計に入れる。
- C-3:応答しないと見なすまでの時間を、設定で変えられるようにする。

### 取り消した理由(2026-10-15、運用者の決定)

- input-method-v2 の commit は、キーごとの返事ではなく、状態を適用する要求(2.2)。空の commit は、preedit を含む状態の更新として働きうる。
- 文字を出さずにキーを使うのは、ふつうの動き。時間切れで「応答しない」と見なす判定は、正しく動いている IME を止まったと見なすことを含む。時間を設定で長くしても、この原因は無くならない。
- 標準の IME(fcitx5 など)に、Seinas の独自の返事の作法を求めることになる。
- 代わりの形は 1.5(復旧の作り直し)。IME の権限は 1.6。

## Smithay 0.7.0 で足りるか

足りるもの:

- text-input-v3 と input-method-v2 の取り次ぎ(commit_string、preedit、delete_surrounding、done の serial)。
- キーボードのフォーカスに text-input のフォーカスが付いてくること、IME の有効・無効(activate・deactivate)。
- IME のキーボードの grab(grab している間のキーと修飾キーを IME へ送り、keymap と繰り返しの設定も送る)。
- 候補ウィンドウの role と、カーソルの矩形を IME へ伝えること(`InputMethodHandler::parent_geometry` で、アプリのウィンドウの位置を Seinas が教える)。
- virtual-keyboard(公開先を絞れる)。

足りないもの・Seinas が書くもの:

- `Seat::add_keyboard` は、xkb の名前から keymap を作るので、xkeyboard-config のファイル(Linux では `/usr/share/X11/xkb`)が要る。ZeikOS には無い。案:(a)ZeikOS に、要る分だけのファイル(evdev の rules、`jp` と `us` の symbols など)を置く、(b)Smithay に、keymap の文字列から keyboard を作る口を足す(上流に提案するか、小さなパッチ)。実装の最初に(a)の大きさを測って決める。
- 候補ウィンドウの描画(ウィンドウより手前の層)、置く位置の計算(画面の端での上下の入れ替え)、ポインターのヒット。
- 注意:Smithay 0.7.0 の `add_instance` の不具合(下)は、Seinas の側で IME を 1 つに限る形で避ける(2 つ目の IME に、Smithay の取り次ぎを使わせない)。
- IME を 1 つに限ること。Smithay 0.7.0 では、2 つ目の IME が `get_input_method` をすると、前からある IME の方へ `unavailable` を送り、新しい方には何も送らない(新しい方は覚えられず、宙に浮く。`input_method_handle.rs` の `add_instance`。ソースで確かめた)。プロトコルでは、新しい方へ送るもの。また、2 つ目の IME も 1 つ目と同じ keyboard の grab の入れ物を指すので、2 つ目が grab を取って手放すと、1 つ目の grab まで外れる。決定(2026-10-12):2 つ目からの `get_input_method` を、Smithay の Dispatch の前に、Seinas が書いた Dispatch で受けて断る(Seinas の側の入れ物でオブジェクトを作り、`unavailable` を送り、その後の要求は捨てる)。2 つ目の要求が Smithay の部品に届かないので、どちらの不具合も起きない。Smithay は直さない(このリポジトリの中で Smithay のバージョンを上げない)。
- ショートカットの filter と、キーボードのフォーカスの決まり。
- keymap を配るとき、Smithay は memfd に keymap を書いて封をする(`memfd_create` の `MFD_ALLOW_SEALING`、`fcntl` の `F_ADD_SEALS`)。

## Linux(WSLg)の上で試す手順

WSL には、text-input-v3 や input-method-v2 を使うアプリと IME(foot、GTK4 のアプリ、fcitx5 など)が入っていない。入れるには、運用者の判断が要る。そこで、まず Seinas の中のテスト用のクライアントで、経路の全体を確かめる。

1. 入れ子の Seinas に、キーボードを入れる(親の `wl_keyboard` から)。`test-client window --keyboard` が、届いたキーを書き出す。
2. テスト用の IME(`test-client ime`):シェルのソケットへつなぎ、input-method-v2 で keyboard を grab し、小さな表(`a` → 「あ」など)で preedit と commit_string を送り、ほかのキーは virtual-keyboard で戻す。
3. テスト用のアプリ(`test-client window --text-input`):text-input-v3 を有効にし、届いた preedit・commit_string・enter・leave を書き出す。
4. 同じ組み合わせを、seinas-standalone の fake screen とテスト用の入力インターフェース(`key` の行を足す)で動かし、CI のテストにする(キー → IME → アプリの文字、IME が無いときの直接の道、IME が去ったときに直接の道へ戻ること、候補ウィンドウの位置)。
5. 運用者が良ければ、foot(text-input-v3)と fcitx5(input-method-v2)を入れて、ほかの実装とも話せることを確かめる(任意)。
6. Kieli の最初のバージョンができた後に、統合として、テスト用の IME の代わりに Kieli をつなぐ(4 の「Seinas との統合」の判定)。

## ZeikOS の上で動かすまでに要るもの

- キーボードの入力の装置:キーを押した・離したを、evdev と同じキーの番号で、コンポジタが読めるもの(今の端末へのバイト列とは別に。PS/2 のスキャンコードから evdev の番号への変換を含む)。コンポジタが読んでいる間は、端末へは渡さない。
- xkb:libxkbcommon(musl の静的ビルドに、もう入っている)と、上の(a)か(b)。
- `memfd_create`(`MFD_ALLOW_SEALING`)、`fcntl`(`F_ADD_SEALS`)、`write`、`lseek`:keymap を配るため。クライアントから届く keymap(virtual-keyboard)の fd の `mmap`。
- プロセスの本数:コンポジタ・タスクバー・Kieli・アプリで、最小で 4 つ(Kieli を別のプロセスにする決定 Q1 による)。
- ZeikOS の上で動くアプリ(端末など)が、text-input-v3 に対応すること。
- 機能を入れたら、strace の差分を `docs/m3-syscalls.md` に記録する。

## 実装の刻み

| 順 | 内容 | 見込み |
| --- | --- | --- |
| 1 | seat のキーボード(keymap は `jp`)、キーボードのフォーカス(activated のウィンドウ)、入れ子の親からのキー、テスト用の入力インターフェースの `key`、テスト用のクライアントの `--keyboard` | 約 400 行 |
| 2 | text-input-v3・input-method-v2・virtual-keyboard の global(公開先の絞り込み)、IME を 1 つに限ること、ショートカットの filter、候補ウィンドウの描画・置く位置・ポインター | 約 400 行 |
| 3 | テスト用の IME とテスト用のアプリ(ソケットへ直接書くもの)と、経路の全体のテスト | 約 500 行 |
| 4 | ZeikOS のキーボードの装置から読むバックエンド(装置ができてから) | 約 200 行 |
| 5 | IME の権限(1.6:IME のソケット、virtual-keyboard の認可)と、復旧(1.5:接続断、緊急解除) | 3 SP と 4 SP の見込み(2026-10-15) |
| 5b | 遅れて届く古い要求の無効化(1.5 の未解決) | 次の課題で見積もる |
| 6 | Kieli の Wayland の入口(置き場は Kieli の設計で決める) | Kieli の側で見積もる |

- Seinas の側は、1〜4 で約 1,500 行。1 は、IME が無くても要るもの(キーボードそのもの)。

## 今は決めないもの

- IME の状態の表示の届け方(3 の申し送り。タスクバーとの統合のときに決める)。
- 英字配列のときの、IME の切り替えのキー(Kieli の設計で決める)。
- 画面に出すキーボード(ソフトウェアキーボード)。
- text-input-v3 を使わないアプリでの変換(ウィンドウの外に変換中の文字を出す形など)。
- Linux の、Seinas のほかのデスクトップ(GNOME・KDE など)での Kieli の動かし方(1.14)。

## 結果

- Kieli は、Wayland の標準のプロトコル(input-method-v2・virtual-keyboard)で Seinas とつながる。input-method-v2 を使う、Linux のほかの IME(fcitx5 など)やコンポジタとも、同じ形で話せる(KDE・GNOME は別のプロトコル。1.14)。
- IME があれば、キーはいつも IME を通るので、切り替えのキーがどこでも同じに働く。代わりに、キーが届くまでに 1 往復ぶん遅れる。
- IME が無いか去ったときは、直接の道でキーボードが使える。IME が止まったときの戻り方は、作り直す(1.5)。
- Seinas は、IME にキーごとの返事を求めない。Kieli の作り方への Seinas の希望は、決まりではなく申し送り(3)として渡す。
- ZeikOS には、キーボードの装置と xkb のファイル(またはパッチ)が要る。
