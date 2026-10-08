# ADR 0001: 外部ライブラリを、後で置き換えられる境界で使う

- 状態: 承認
- 日付: 2026-10-08

## 背景

Seinas は、描画の合成に pixman、テキスト描画に cosmic-text と swash、フォントの一覧に fontdb、Wayland の土台に Smithay、イベントループに calloop を使っている。キーボードの配列のために、Smithay が xkbcommon をリンクする。

運用者の方針として、これらの外部ライブラリは、将来、自前のコードに置き換えられる形で使う。今すぐ置き換えるのではなく、置き換えるときに、影響が狭い範囲で済むようにしておく。

ただし、外部ライブラリには性質の違う 2 種類がある。

- 1 つの機能を受け持つ部品(ピクセルの合成、グリフの描画など)。境界の内側に隠せる。
- Wayland の protocol の処理と、イベントループ。Seinas の作り全体がその上に乗っていて、完全には隠せない。

この 2 つを同じ決まりで扱うと、後者について守れない決まりになる。そこで、種類ごとに決まりを分ける。

## 決定

### 1. 外部ライブラリを 2 種類に分ける

| 種類 | 外部ライブラリ | 決まり |
| --- | --- | --- |
| 部品(置き換えの候補) | pixman、cosmic-text、swash、fontdb、xkbcommon | 境界の内側の 1 か所だけで使い、library crate の公開 API に出さない(決定 2) |
| 土台 | Smithay、calloop | 完全には隠さない。使ってよい範囲を閉じ込める(決定 3) |

ここに無い外部ライブラリを新しく使うときは、どちらの種類か、または対象外(決定 5)かを決めてから使う。

### 2. 部品の決まり

- Seinas の library crate の公開 API(公開の関数の引数・戻り値、公開の型、公開のフィールド、公開の trait の bound、`pub use`)に、部品の型・trait・関数を出さない。
- 部品を使うのは、その層の境界の内側の 1 か所(1 つの crate、またはその中の 1 つの module)だけにする。
- 境界には、Seinas の型を置く。値の受け渡しは、Seinas の型か、標準ライブラリの型(`&[u8]`、`String`、`std::io::Error` など)で行う。

### 3. 土台の決まり

- 土台を使ってよいのは、`seinas-frontend` と、バイナリ(`seinas`、`seinas-standalone` などの `main.rs`。組み立てる場所、composition root)だけにする。
- `seinas-frontend` は、土台の型を公開 API に出してよい(バイナリが使うため)。`pub use smithay` も、その範囲で認める。
- バイナリは、イベントループと Wayland の display を直接持ってよい。ただし、ウィンドウ管理の決まりをバイナリに書かない。
- 次のものは、土台に依存しない:
  - 描画(`seinas-render`)
  - テキスト描画(`seinas-text`)
  - ウィンドウ管理の決まり(`seinas-frontend` の中の、スタック・ポインターの操作・タイトルバーの計算。`stack.rs`、`interaction.rs`、`decoration.rs` の計算の部分)
  - バックエンド(`seinas-fbdev`)
- 土台を置き換えるのは、Wayland の protocol 層の作り直しになる。部品の差し替えとは別の判断として、そのときに改めて決める。この ADR は、土台の置き換えを準備するものではない。範囲を閉じ込めるのは、土台の影響を受ける場所を少なくしておくためである。

### 4. 置き換える前の出力を、正解として保存する

- 部品を使う層(描画の合成、テキスト描画、フォントの選択)は、決まった入力に対する出力(ピクセル、グリフのビットマップ、選ばれたフォントの名前など)を、正解のデータとしてリポジトリに保存する。
- 正解のデータは、テストで読み込み、今の出力と機械で比べる。置き換えた後も、同じテストを通す。
- 正解は、置き換える前の実装で作る。作り直すときは、作り直した理由と差分をコミットに書く。
- 完全一致でなくてよい層(アンチエイリアスの端など)は、許す差の大きさをテストに書く。

### 5. 置き換えの対象外

次のものは、置き換えの対象にしない。決定 2・3 も当てはめない。

- 暗号・乱数(`sha2`、`rand`、`getrandom` など)
- Unicode のテーブル(`unicode-script`、`unicode-bidi`、`unicode-linebreak`、`unicode-segmentation` など)
- ビルドのときだけ使うもの(proc-macro、`cc`、`pkg-config`、`wayland-scanner` など)
- 標準ライブラリに近い基本の部品(`libc`、`bitflags`、`thiserror`、ロギングの `tracing` など)。ただし、公開 API には出さない。

## 層ごとの境界(2026-10-08 の棚卸し)

| 層 | 種類 | 境界 | 外部ライブラリ | 今の状態 | 今ある違反 |
| --- | --- | --- | --- | --- | --- |
| 描画の合成 | 部品 | `seinas-render`(要素を重ねて 1 枚にする) | pixman(Smithay の `PixmanRenderer` 経由) | 合成の手順を、Smithay の `Renderer`・`Frame`・`RenderElement` で書いている | あり(違反 1〜4) |
| テキスト描画 | 部品 | `seinas-text` | cosmic-text、swash | 公開 API は Seinas の型と標準ライブラリの型だけ(`Canvas`、`Line`、`Drawn`、`FontFiles`、`FontProblem`、`TextPainter`) | なし |
| フォントの一覧と選択 | 部品 | (まだ無い。`seinas-text` の中) | fontdb | `seinas-text` の外には出ていない。ただし、読み込み・名前での選択・fallback の順が、シェーピングと描画と同じ `TextPainter` の中に混ざっていて、層としての境界が無い | なし(境界を作るのは、フォントの設計のときに行う) |
| キーボード | 部品 | (まだ無い) | xkbcommon(Smithay がリンクする) | Seinas のコードは使っていない。キーボードの入力を作るときに、境界を作る | なし |
| Wayland の protocol | 土台 | `seinas-frontend` | Smithay | `pub use smithay`、`Frontend` の公開フィールド、`FrontendHost: SeatHandler` などで、バイナリに Smithay の型を見せている(決定 3 で認める範囲) | なし |
| ウィンドウ管理の決まり | 土台に依存しない | `seinas-frontend` の `stack.rs`・`interaction.rs`・`decoration.rs` の計算 | (なし) | Smithay の振る舞いには依存していないが、Smithay の幾何の型を使っている | あり(違反 5) |
| イベントループ | 土台 | バイナリの `main.rs` | calloop | `seinas` と `seinas-standalone` の `main.rs` だけで、Smithay の reexport を通して使っている | なし |
| バックエンド(fbdev) | 土台に依存しない | `seinas-fbdev` | (なし。`libc` は対象外) | Smithay に直接は依存していないが、`seinas-render` を通して Smithay の型を公開 API に出している | あり(違反 6) |

### 今ある違反

| 番号 | 場所 | 内容 | 破っている決まり |
| --- | --- | --- | --- |
| 1 | `seinas-render` の `Painter::renderer()`、`Painter::paint` の bound、`RenderError::Pixman` | 公開 API に `PixmanRenderer`・`RenderElement<PixmanRenderer>`・`PixmanError` が出ている | 決定 2 |
| 2 | `seinas-render` の `BACKGROUND`、`SolidRect`、`solid_rect` | 公開 API に Smithay の `Color32F`・`SolidColorRenderElement` が出ている | 決定 3 |
| 3 | `seinas-render` の全体 | 合成の手順が Smithay の renderer の仕組み(`Renderer`・`Frame`・`RenderElement`)に依存している | 決定 3 |
| 4 | `seinas-frontend` の `WindowElement`、`Frontend::render_elements`、`TitleBar::element` | 公開 API に `PixmanRenderer` が出ている | 決定 2 |
| 5 | `seinas-frontend` の `stack.rs`・`interaction.rs`・`decoration.rs` の計算 | Smithay の幾何の型(`Point`、`Size`、`Rectangle`、`Logical`)を使っている。後で Seinas の型に替える | 決定 3 |
| 6 | `seinas-fbdev` の `test_picture` | 戻り値が `Vec<SolidRect>`(Smithay の型) | 決定 3 |

違反 1〜4 と 6 は、描画の境界を Seinas の型にするときに、まとめて直る見込みである(GPU で描画する構成を足すときにも、同じ所を直す)。

## 依存の重複(様子見)

2026-10-08 の時点で、実行時の依存に、次のバージョン違いの重複がある。課題にはせず、関係する依存を上げるときに、そろうかを確かめる。

- font-types・read-fonts・skrifa の 2 バージョン:cosmic-text 0.19.0 と、その中の swash 0.2.10 が、別のバージョンの skrifa を使うため。
- getrandom 0.3 と 0.4:rand_core 0.9 と、Smithay が使う tempfile 3.27 のため。
- thiserror 1 と 2:pixman 0.2.1 と、Smithay・SCTK のため。

## 結果

- 部品を置き換えるときの作業が、境界の内側に閉じる。
- 土台の影響を受ける場所が、`seinas-frontend` とバイナリに限られる。
- 境界に Seinas の型を置くぶん、変換のコードが少し増える。
- 正解のデータを保存するぶん、リポジトリが少し大きくなる(PPM の小さな画像や、ハッシュ)。
- 今ある違反は、すぐには直さない。順に直す。新しく書くコードは、この決まりに従う。
