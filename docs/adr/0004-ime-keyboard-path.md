# ADR 0004: キーボードの入力は、IME(Kieli)を通る経路にする(text-input-v3・input-method-v2)

- 状態: 承認
- 日付: 2026-10-11

## 背景

日本語の入力のために、オリジナルの IME(Kieli)が、Seinas の上のアプリと連携できるようにしたい(運用者の最低要件。2026-10-07)。Seinas は、まだキーボードの入力を扱っていない(seat にはポインターだけがある)。キーボードを入れる前に、IME を通る経路を決めておく。

Kieli について決まっていること:Rust で書き、かな漢字変換を自作する。別のリポジトリにする。Kieli の本体は Smithay に依存させず、Seinas との連携の部分で text-input・input-method を扱う。初版は、ローマ字からひらがなを打てるだけでもよい。

Smithay 0.7.0 で使えるもの(2026-10-11 に、ソースで確かめた。まだ動かしてはいない):

| プロトコル | Smithay 0.7.0 | 備考 |
| --- | --- | --- |
| text-input-unstable-v3(`zwp_text_input_v3`) | あり(`wayland::text_input`) | キーボードのフォーカスと一緒に、text-input のフォーカスも動く(`wayland/seat/keyboard.rs`)。global の公開先は絞れない |
| input-method-unstable-v2(`zwp_input_method_v2`) | あり(`wayland::input_method`) | commit_string、preedit、delete_surrounding、commit の serial、キーボードの grab、候補の窓(`zwp_input_popup_surface_v2`)。公開先を絞れる |
| virtual-keyboard-unstable-v1(`zwp_virtual_keyboard_v1`) | あり(`wayland::virtual_keyboard`) | IME が使わなかったキーを、アプリへ戻すのに使う。公開先を絞れる |
| キーボード(`wl_keyboard`) | あり(`Seat::add_keyboard`) | keymap は、xkb の名前(rules・model・layout)から作る。文字列から作れるのは、作った後の `set_keymap_from_string` だけ |

## 決定

### 1. 標準のプロトコルでつなぐ

- アプリは text-input-v3、Kieli は input-method-v2 を使い、Seinas が両者のあいだを取り次ぐ。取り次ぎには、Smithay の 3 つの部品(text-input、input-method、virtual-keyboard)を使う。
- Kieli が使わなかったキー(ショートカット、矢印、IME が切れているときの英字など)は、Kieli が virtual-keyboard でアプリへ戻す。

```text
                         ┌──────────────── Seinas(コンポジタ) ────────────────┐
キーボードの装置 ──key──→│ (a) Seinas のショートカット? ──はい──→ Seinas が使う   │
(入れ子では親の          │        │いいえ                                         │
 wl_keyboard)            │        ▼                                               │
                         │ (b) IME が keyboard を grab している? ──いいえ──┐      │
                         │        │はい                                    │      │
                         └────────┼───────────────────────────────────────┼──────┘
                                  ▼                                        ▼
                   Kieli(input-method-v2 のクライアント)          アプリ(wl_keyboard)
                     │ commit_string・preedit・delete_surrounding ─→ Seinas ─→ アプリ(text-input-v3)
                     │ 使わなかったキー(virtual-keyboard) ────────→ Seinas ─→ アプリ(wl_keyboard)
                     │ 候補の窓(input popup surface) ─────────────→ Seinas が、文字を打っている所の近くに置く
                     ▼
アプリ(text-input-v3)── 有効・無効、周りの文字、カーソルの矩形、入力の種類 ─→ Seinas ─→ Kieli
```

### 2. IME が動いていれば、いつも IME を通る

- キーボードの入力を、コンポジタがアプリへ直接渡す形を、IME があるときの道にしない。Kieli が動いていて keyboard を grab している間は、Seinas のショートカット以外のキーを、全部 Kieli へ渡す。アプリが text-input を使っていないときも同じ(Kieli がそのまま virtual-keyboard で戻す)。
  - 理由:IME の切り替えのキー(半角/全角など)が、どのアプリの上でも同じに働くようにするため。どのキーをアプリへ戻すかを、Kieli の 1 か所で決められるため。
  - 費用:IME を通るぶん、1 往復(Seinas → Kieli → Seinas)遅れる。入れたら、キーを押してからアプリに届くまでの時間を測る。
- IME が動いていない(つないでいない、切断した)ときと、応答しないときは、Seinas がアプリへ直接渡す経路(`wl_keyboard`)に戻す。IME が落ちたり固まったりしても、キーボードが使えなくならないようにするため。
  - 切断:IME のクライアントが去った(または keyboard の grab を手放した)ら、次のキーから直接の経路にする。
  - 応答しない(案。仮の値):キーの押下を IME へ送ってから 1,000 ms の間に、その IME から要求が 1 つも届かなければ、応答しないと見なし、次のキーから直接の経路にする。IME へ送ったまま返ってこなかったキーは、アプリへ送り直さない(同じキーが 2 度届くのを避けるため)。
  - Kieli は、キーの押下を受けるたびに、必ず何かを返す決まりにする(使ったキーは preedit などと input-method の commit、使わなかったキーは virtual-keyboard のキー)。そのため、正しく動いている Kieli は、上の条件に当たらない。ほかの IME では、当たることがある(値は、Kieli と試して決め直す)。
  - 応答しないと見なした後に、その IME から要求が届いたら、次のキーから IME を通る経路に戻す。直接の経路にした・戻したことは、ログに出す。
- Smithay の grab は、text-input が有効でないときも、キーを IME へ渡す形になっている(ソースで確かめた)。

### 3. 見せる相手

- input-method-v2 と virtual-keyboard の global は、シェルの部品のためのソケットからつないだクライアントにだけ見せる(ADR 0002 と同じ印で絞る)。IME は、全部のキーを読み、どのアプリへもキーを送れるので、強い権限だから。
- text-input-v3 の global は、全部のクライアントに見せる(アプリが使うもの)。Smithay の部品は、公開先を絞れない。
- IME は同時に 1 つだけにする。2 つ目の IME には、input-method-v2 の `unavailable` を送る。Smithay 0.7.0 は、これを逆に送るので(下の「足りないもの」)、Seinas の側で避ける:2 つ目からの `get_input_method` を Smithay へ渡さずに Seinas が受けて断る(2026-10-12 に決めた。`docs/ime.md`)。
- Kieli は、タスクバーと同じく、起動の仕組みが起こしてシェルのソケットへつなぐ。コンポジタはプロセスを起こさない(ADR 0002 の 5)。

### 4. Seinas のショートカット

- コンポジタが使うキー(将来の、メニューを開くキー、ウィンドウの切り替えなど)は、IME へ渡す前に Seinas が見て、使ったら IME にもアプリにも渡さない(Smithay の `KeyboardHandle::input` の filter で、grab の前に止められる)。
- ショートカットの判定は、いつも IME へ送る前に行う。IME が固まっていても(上の、応答しないと見なす前の 1,000 ms の間も)、Alt+Tab などのショートカットが効くようにするため。
- ショートカットの一覧は、キーボードを入れる刻みで決める。最初は無し。

### 5. 候補の窓と、変換中の文字

- 変換中の文字(preedit)は、アプリが、文字を打っている所に描く(text-input-v3 の preedit_string。カーソルの位置も届く)。Kieli は、アプリの上に描かない。
- 変換の候補の窓は、Kieli が input popup surface に描く。Seinas は、アプリが伝えたカーソルの矩形(`set_cursor_rectangle`)の下に置き、画面の下に入らなければ上に置く。候補の窓は、全部のウィンドウより手前(layer surface の top と同じ所)に描き、ポインターも受け取る(候補を押して選べるように)。
- text-input を使わないアプリ(zeyes など)では、Kieli は変換をせず、キーをそのまま戻す(preedit を出す場所が無いため)。

### 6. 切り替えのキーと、確定の流れ

- 半角/全角、変換、無変換、カタカナ/ひらがななどのキーは、Seinas は意味を持たせず、Kieli へ渡す。どのキーで何をするか(英字配列でのキーの組み合わせを含む)は、Kieli が決める。
- 確定(Enter など)と取り消し(Esc など)の流れも、Kieli が決める。Seinas は、Kieli の commit_string・preedit を、text-input の done までまとめてアプリへ渡すだけにする。
- キーを押し続けたときの繰り返しは、アプリへ戻したキーはアプリが行い(押したイベントだけが届くので、アプリの繰り返しが働く)、Kieli が使ったキーは Kieli が自分で行う。

### 7. IME の状態の表示

- Kieli の状態(「あ」「A」など)をタスクバーに出す方法は、トレイと IME の表示の仕組みを決めるときに、一緒に決める。それまでは、出さない。

### 8. 分担

| 役目 | Seinas | Kieli | アプリ |
| --- | --- | --- | --- |
| キーボードの装置からキーを読む | ○ | | |
| keymap(日本語の配列)を決めて配る | ○ | 受け取って、virtual-keyboard で同じものを使う | 受け取る |
| ショートカット | ○ | | |
| キーを IME へ渡す、IME が無ければアプリへ渡す | ○ | | |
| キーの意味(ローマ字からかな、変換、切り替え) | | ○ | |
| 変換中の文字を描く | | | ○(text-input-v3) |
| 候補の窓を描く | | ○(input popup surface) | |
| 候補の窓を置く | ○ | | |
| 文字を打っている所(カーソルの矩形、周りの文字)を伝える | 取り次ぐ | 受け取る | ○ |
| IME の状態の表示 | (後で決める) | (後で決める) | |

- Kieli は、2 つに分ける案:Wayland に依存しない変換の本体と、input-method-v2・virtual-keyboard のクライアントの部分(Linux の Wayland と、ZeikOS の上の Seinas で同じもの)。Smithay には依存しない。Kieli の構成は、Kieli の側で決める。

### 9. キーボードと keymap(Seinas の側)

- seat にキーボードを足す。keymap の既定は、日本語の配列(xkb の layout `jp`、model `jp106`)にし、設定で `us` などに変えられるようにする。
- 入れ子の構成では、親の `wl_keyboard` から届くキー(evdev のキーの番号)を、そのまま Seinas の seat へ入れる。fbdev の構成(ZeikOS)では、ZeikOS のキーボードの装置から読む。
- キーボードのフォーカスは、activated のウィンドウ(いちばん手前のウィンドウ)にする。layer surface の `keyboard_interactivity`(メニュー)は、メニューを作るときに足す。

## Smithay 0.7.0 で足りるか

足りるもの:

- text-input-v3 と input-method-v2 の取り次ぎ(commit_string、preedit、delete_surrounding、done の serial)。
- キーボードのフォーカスに text-input のフォーカスが付いてくること、IME の有効・無効(activate・deactivate)。
- IME のキーボードの grab(grab している間のキーと修飾キーを IME へ送り、keymap と繰り返しの設定も送る)。
- 候補の窓の role と、カーソルの矩形を IME へ伝えること(`InputMethodHandler::parent_geometry` で、アプリのウィンドウの位置を Seinas が教える)。
- virtual-keyboard(公開先を絞れる)。

足りないもの・Seinas が書くもの:

- `Seat::add_keyboard` は、xkb の名前から keymap を作るので、xkeyboard-config のファイル(Linux では `/usr/share/X11/xkb`)が要る。ZeikOS には無い。案:(a)ZeikOS に、要る分だけのファイル(evdev の rules、`jp` と `us` の symbols など)を置く、(b)Smithay に、keymap の文字列から keyboard を作る口を足す(上流に提案するか、小さなパッチ)。実装の最初に(a)の大きさを測って決める。
- 候補の窓の描画(ウィンドウより手前の層)、置く位置の計算(画面の端での上下の入れ替え)、ポインターのヒット。
- 注意:Smithay 0.7.0 の `add_instance` の不具合(下)は、Seinas の側で IME を 1 つに限る形で避ける(2 つ目の IME に、Smithay の取り次ぎを使わせない)。
- IME を 1 つに限ること。Smithay 0.7.0 では、2 つ目の IME が `get_input_method` をすると、前からある IME の方へ `unavailable` を送り、新しい方には何も送らない(新しい方は覚えられず、宙に浮く。`input_method_handle.rs` の `add_instance`。ソースで確かめた)。プロトコルでは、新しい方へ送るもの。また、2 つ目の IME も 1 つ目と同じ keyboard の grab の入れ物を指すので、2 つ目が grab を取って手放すと、1 つ目の grab まで外れる。決定(2026-10-12):2 つ目からの `get_input_method` を、Smithay の Dispatch の前に、Seinas が書いた Dispatch で受けて断る(Seinas の側の入れ物でオブジェクトを作り、`unavailable` を送り、その後の要求は捨てる)。2 つ目の要求が Smithay の部品に届かないので、どちらの不具合も起きない。Smithay は直さない(このリポジトリの中で Smithay のバージョンを上げない)。
- ショートカットの filter と、キーボードのフォーカスの決まり。
- keymap を配るとき、Smithay は memfd に keymap を書いて封をする(`memfd_create` の `MFD_ALLOW_SEALING`、`fcntl` の `F_ADD_SEALS`)。

## Linux(WSLg)の上で試す手順

WSL には、text-input-v3 や input-method-v2 を使うアプリと IME(foot、GTK4 のアプリ、fcitx5 など)が入っていない。入れるには、運用者の判断が要る。そこで、まず Seinas の中のテスト用のクライアントで、経路の全体を確かめる。

1. 入れ子の Seinas に、キーボードを入れる(親の `wl_keyboard` から)。`test-client window --keyboard` が、届いたキーを書き出す。
2. テスト用の IME(`test-client ime`):シェルのソケットへつなぎ、input-method-v2 で keyboard を grab し、小さな表(`a` → 「あ」など)で preedit と commit_string を送り、ほかのキーは virtual-keyboard で戻す。
3. テスト用のアプリ(`test-client text-input`):text-input-v3 を有効にし、届いた preedit・commit_string・enter・leave を書き出す。
4. 同じ組み合わせを、seinas-standalone の fake screen とテスト用の入力インターフェース(`key` の行を足す)で動かし、CI のテストにする(キー → IME → アプリの文字、IME が無いときの直接の道、IME が落ちたときに直接の道へ戻ること、候補の窓の位置)。
5. 運用者が良ければ、foot(text-input-v3)と fcitx5(input-method-v2)を入れて、ほかの実装とも話せることを確かめる(任意)。
6. Kieli の初版ができたら、テスト用の IME の代わりに Kieli をつなぐ。

## ZeikOS の上で動かすまでに要るもの

- キーボードの入力の装置:キーを押した・離したを、evdev と同じキーの番号で、コンポジタが読めるもの(今の端末へのバイト列とは別に。PS/2 のスキャンコードから evdev の番号への変換を含む)。コンポジタが読んでいる間は、端末へは渡さない。
- xkb:libxkbcommon(musl の静的ビルドに、もう入っている)と、上の(a)か(b)。
- `memfd_create`(`MFD_ALLOW_SEALING`)、`fcntl`(`F_ADD_SEALS`)、`write`、`lseek`:keymap を配るため。クライアントから届く keymap(virtual-keyboard)の fd の `mmap`。
- プロセスの本数:コンポジタ・タスクバー・Kieli・アプリで、最小で 4 つ。
- ZeikOS の上で動くアプリ(端末など)が、text-input-v3 に対応すること。
- 機能を入れたら、strace の差分を `docs/m3-syscalls.md` に記録する。

## 実装の刻み

| 順 | 内容 | 見込み |
| --- | --- | --- |
| 1 | seat のキーボード(keymap は `jp`)、キーボードのフォーカス(activated のウィンドウ)、入れ子の親からのキー、テスト用の入力インターフェースの `key`、テスト用のクライアントの `--keyboard` | 約 400 行 |
| 2 | text-input-v3・input-method-v2・virtual-keyboard の global(公開先の絞り込み)、IME を 1 つに限ること、ショートカットの filter、候補の窓の描画・置く位置・ポインター | 約 400 行 |
| 3 | テスト用の IME とテスト用のアプリ(ソケットへ直接書くもの)と、経路の全体のテスト | 約 500 行 |
| 4 | ZeikOS のキーボードの装置から読むバックエンド(装置ができてから) | 約 200 行 |
| 5 | Kieli の input-method-v2 のクライアント(Kieli のリポジトリ) | Kieli の側で見積もる |

- Seinas の側は、1〜4 で約 1,500 行。1 は、IME が無くても要るもの(キーボードそのもの)。

## 今は決めないもの

- IME の状態の表示(トレイと一緒に決める)。
- ショートカットの一覧。
- 英字配列のときの、IME の切り替えのキー(Kieli が決める)。
- 画面に出すキーボード(ソフトウェアキーボード)。
- text-input-v3 を使わないアプリでの変換(ウィンドウの外に変換中の文字を出す形など)。

## 結果

- Kieli は、Wayland の標準のプロトコルだけで Seinas とつながる。Linux の別の IME やアプリとも、同じ形で話せる。
- IME があれば、キーはいつも IME を通るので、切り替えのキーがどこでも同じに働く。代わりに、キーが届くまでに 1 往復ぶん遅れる。
- IME が無いか落ちたときは、直接の道でキーボードが使える。
- ZeikOS には、キーボードの装置と xkb のファイル(またはパッチ)が要る。
