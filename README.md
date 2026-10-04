# Seinas

Seinasは、ZeikOS向けのWaylandコンポジタです。

今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中でクライアントを合成して見せる「入れ子」の形で動きます。

## 構成

| 場所 | 中身 |
| --- | --- |
| `crates/seinas` | 入れ子の構成のコンポジタ。Waylandの受け口と、親のWaylandへ出す裏側(SCTK)を組み合わせる |
| `crates/seinas-standalone` | 親のWaylandを使わない構成のコンポジタ。Waylandの受け口と、fbdevの裏側を組み合わせる |
| `crates/seinas-frontend` | Waylandの受け口(Smithay)。クライアントの要求を受け、描く要素を並べる |
| `crates/seinas-render` | 共通の描画。pixmanで1枚の絵に合成する。Waylandの受け口には依存しない |
| `crates/seinas-fbdev` | fbdev(`/dev/fb0`)へ出す裏側。共通の描画だけを使い、Waylandのソケットは開かない |
| `crates/seinas-test-clients` | 試験用のクライアント。わざと不正な要求を送ったり、共有メモリーを縮めたりする |
| `crates/zeyes-min` | 動作確認用の小さなクライアント。xeyesのように2つの目を描き、目玉がポインターを追う |
| `docs/render-boundary.md` | 共通の描画と裏側の境界の決まり |
| `docs/standalone.md` | 受け口とfbdevを組み合わせた構成の、動かし方、描く時機、使うシステムコール |
| `docs/window-placement.md` | ウィンドウの重ね方と置き方の決まり |
| `docs/shrink-test.md` | 共有メモリーを縮めてくるクライアントの試験の、動かし方と期待する結果 |
| `docs/fbdev.md` | fbdevの裏側の構成、動かし方、確かめ方 |
| `docs/musl-static.md` | muslでの静的ビルドの手順と、できた実行ファイルの記録 |
| `tools/` | 静的ビルドや、依存の一覧を作るための道具 |
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

## muslでの静的ビルド

ZeikOS向けに、共有ライブラリを使わない実行ファイルを作れます。

```bash
tools/build-musl.sh
```

実行ファイルは `target/x86_64-unknown-linux-musl/release/` に、ライセンス文と一緒にまとめたものは `target/dist/seinas-musl/` にできます。
手順の中身と、できた実行ファイルの記録は `docs/musl-static.md` にあります。

## 今の制限

- 大きさは800x600に固定で、サイズ変更には応じません。
- クライアントへ渡す入力は、ポインターだけです。
- クライアントのウィンドウは、決まった位置に少しずつずらして重ねます(`docs/window-placement.md`)。動かすことはできません。

## ライセンス

MITライセンスです。`LICENSE` を見てください。第三者のライセンス文は `THIRD-PARTY/` にあります。
