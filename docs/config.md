# 設定のファイル

Seinas の設定のファイルは、どれも `$XDG_CONFIG_HOME/seinas/` の下に置きます(`XDG_CONFIG_HOME` が無ければ `~/.config/seinas/`)。書き方は共通で、1 行に `キー = 値`、`#` から行の終わりまではコメントです(読み方は `crates/seinas-config`)。読めない行は、ログに出して飛ばし、既定の値で動きます。

| ファイル | 読むもの | 書くこと | くわしく |
| --- | --- | --- | --- |
| `seinas.conf` | コンポジタ(`seinas`、`seinas-standalone`) | ウィンドウの切り替えのキー | 下の「seinas.conf」 |
| `fonts.conf` | コンポジタ、タスクバー | フォントの選び方 | `docs/text-rendering.md`、ADR 0003 |
| `wallpaper.conf` | 壁紙 | 色、画像、置き方 | `docs/wallpaper.md` |
| `taskbar.conf` | タスクバー | 時計の時差 | `docs/taskbar.md` |

どのプログラムも、`--config PATH`(フォントは `--font-config PATH`)で、別の場所のファイルを読めます。

## seinas.conf

```text
# ウィンドウの切り替え(docs/window-placement.md の「Alt+Tab」)
switch-next = Alt+Tab, Alt+F6
switch-prev = Alt+Shift+Tab, Alt+Shift+F6
```

| キー | 意味 | 既定 |
| --- | --- | --- |
| `switch-next` | 次のウィンドウへ切り替えるキー | `Alt+Tab`。入れ子の構成(`seinas`)では `Alt+Tab, Alt+F6` |
| `switch-prev` | 前のウィンドウへ切り替えるキー | `Alt+Shift+Tab`。入れ子の構成では `Alt+Shift+Tab, Alt+Shift+F6` |

- キーの書き方:修飾キー(`Alt`・`Shift`・`Ctrl`・`Super`)と、キーの名前を `+` でつなぎます。キーの名前は、xkb の keysym の名前(`Tab`、`F6`、`Escape`、`a` など。大文字と小文字は区別しない)で、終わりに 1 つだけ書きます。
- `,` で区切って、いくつでも書けます。書いたキーは、既定のキーと置き換わります(足すのではありません)。入れ子で Alt+F6 を残したまま別のキーを足すときは、`Alt+F6` も書きます。
- Shift を含むキーは、Shift を押したときの keysym(Shift+Tab なら `ISO_Left_Tab`)でも、Shift を外した keysym(`Tab`)でも書けます。
- 読めない値(知らない修飾キー、知らないキーの名前、修飾キーだけ)は、ログに出して、そのキー(次へ・前へ)を既定のままにします。知らないキーの行も、ログに出して飛ばします。
- 決まったキーは、起動のときに `switch windows: next Alt+Tab, Alt+F6; previous Alt+Shift+Tab, Alt+Shift+F6` の形でログに出します。

### どのキーが届くかを確かめる

入れ子の構成(WSLg)では、親の側(Windows)が先に使うキーがあり、Seinas まで届きません(Alt+Tab がそう)。どの組み合わせが届くかは、起動のオプション `--log-keys` で確かめられます。Seinas に届いたキーを、設定と同じ書き方でログに出します。

```bash
cargo run --locked -p seinas -- --log-keys
```

```text
INFO seinas_frontend: key: Alt_L pressed
INFO seinas_frontend: key: Alt+F6 pressed
INFO seinas_frontend: key: Alt+F6 released
```

ログに出た組み合わせ(`Alt+F6` など)を、そのまま `seinas.conf` に書けます。ログに出ないキーは、親の側が使っています。`--log-keys` は、`seinas-standalone` にもあります。既定では出しません。打ったキーがそのままログに残るので、確かめるときだけ明示して付けるテスト用のものです(ふだんのログには、キー・keysym・変換中の文字・確定した文字を出しません。ADR 0004 の 1.6)。
