# Seinas

Seinasは、ZeikOS向けのWaylandコンポジタです。

今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中でクライアントを合成して見せる「入れ子」の形で動きます。

## 構成

| 場所 | 中身 |
| --- | --- |
| `crates/seinas` | コンポジタの本体。Waylandの受け口(Smithay)と、親のWaylandへ出す裏側(SCTK)を持つ |
| `crates/seinas-render` | 共通の描画。pixmanで1枚の絵に合成する。Waylandの受け口には依存しない |
| `crates/zeyes-min` | 動作確認用の小さなクライアント。xeyesのように2つの目を描き、目玉がポインターを追う |
| `docs/render-boundary.md` | 共通の描画と裏側の境界の決まり |
| `THIRD-PARTY/` | 第三者のライセンス文と著作権表示、依存の一覧 |

## ビルドに要るもの

- Rust(版は `rust-toolchain.toml` で固定)
- pixmanとlibxkbcommonの開発用ファイル。Ubuntuなら次のとおりです。

```bash
sudo apt install libpixman-1-dev libxkbcommon-dev
```

依存するクレートの版は `Cargo.lock` で固定しています。ビルドやテストでは `--locked` を付けてください。

## ビルドとテスト

```bash
cargo build --workspace --locked
```

```bash
cargo test --workspace --locked
```

## 動かし方

Waylandのセッションの中(WSLgなど)で、まずSeinasを起動します。800x600のウィンドウが1つ開きます。

```bash
cargo run --locked -p seinas
```

別の端末から、Seinasの中でクライアントを動かします。

```bash
WAYLAND_DISPLAY=seinas-0 cargo run --locked -p zeyes-min
```

Seinasのウィンドウを閉じると、Seinasは終わります。

## 今の制限

- 大きさは800x600に固定で、サイズ変更には応じません。
- クライアントへ渡す入力は、ポインターだけです。
- クライアントのウィンドウは、左上にそのまま重ねて描きます。

## ライセンス

MITライセンスです。`LICENSE` を見てください。第三者のライセンス文は `THIRD-PARTY/` にあります。
