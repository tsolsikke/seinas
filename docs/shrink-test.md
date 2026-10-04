# 共有メモリーを縮めてくるクライアントの試験

Waylandのクライアントは、画面の絵を共有メモリー(wl_shmのプール)に置いて、コンポジタに渡します。
行儀の悪いクライアントは、渡した後でそのプールを縮めることができます。コンポジタが、縮められたプールを元の大きさのつもりで読むと、範囲の外を読むことになります。

この試験は、そういうクライアントがいても、コンポジタが落ちず、そのクライアントだけが切られることを確かめます。

## 何が起きるか(Linux)

1. クライアントが、封印していないmemfdでプールを作り、バッファを1枚出す。コンポジタが描く。
2. クライアントが、`ftruncate` でプールを0バイトに縮める。コンポジタに伝えた大きさは、元のままです。
3. クライアントが、もう一度commitする。コンポジタは、描くためにそのバッファを読む。
4. 読んだ所にはもうファイルの中身が無いので、カーネルがコンポジタへ SIGBUS を送る。
5. Smithayの受け口が、SIGBUSを受け止める。その範囲を、0で埋めた領域に差し替えて(`mmap` の `MAP_FIXED`)、読み出しを続けさせる。読み終えた後で、「読み出しは失敗した」として扱う。
6. Smithayが、そのクライアントへプロトコルのエラーを送って切る。
7. Seinasは、そのウィンドウだけを「描けなかった」として扱い、ほかのウィンドウはいつもどおり描く。そのあとすぐ、切られたクライアントのソケットとウィンドウを片付ける。

ふつうのクライアント(SCTKを使うzeyes-minなど)は、プールを「縮められない」ように封印してから渡すので、こうはなりません。

## 試験用のクライアント

`crates/seinas-test-clients` の `bad-client` に、`shrink` という種類があります。Waylandのライブラリは使わず、ソケットへ直接書きます。

```bash
WAYLAND_DISPLAY=<ソケットの名前か絶対パス> bad-client shrink [--step]
```

- 400x300の、紫(赤と青が最大)のウィンドウを1つ出します。
- frameコールバックが届いたら(コンポジタが描いた合図)、標準出力に `shrink: drawn` と書きます。
- `--step` を付けると、ここで、標準入力から1行届くまで待ちます。呼ぶ側が、画面を確かめてから先へ進めるためです。
- プールを縮めて `shrink: shrunk` と書き、もう一度commitします。
- コンポジタが切るまで読み、結果を書きます。
- 終了コードは、切られたら0、切られなければ1、準備の段階で失敗したら2です。

## 期待する結果

クライアントの標準出力は、次の4行になります。

```
shrink: drawn
shrink: shrunk
shrink: protocol error: object 11, code 2, "Bad pool size."
shrink: the compositor closed the connection
```

- プロトコルのエラーは、縮めたプールから作ったバッファ(object 11、wl_buffer)に対して届きます。codeの2は、wl_shmのエラーの `invalid_fd` です。
- クライアントの終了コードは0です。

コンポジタの側は、次のようになります。

| 確かめること | 期待する結果 |
| --- | --- |
| コンポジタのプロセス | 動き続ける |
| 縮めたクライアント | 切られる。ログに `a client disconnected: ProtocolError(... "Bad pool size.")` と出る |
| 縮めたクライアントのウィンドウ | 画面から消える |
| 同時につないでいるほかのクライアント | 切られず、描かれ続ける |
| その後につなぐ新しいクライアント | 受け付けられ、描かれる |
| ログ | `1 element(s) could not be drawn` という警告が1回出る |

## 手で動かす

偽の画面で動かす例です。3つの端末を使います。

```bash
cargo run --locked -p seinas-standalone -- --fake 800x600 --dump /tmp/screen.ppm --socket /tmp/seinas.sock
```

```bash
WAYLAND_DISPLAY=/tmp/seinas.sock cargo run --locked -p zeyes-min
```

```bash
WAYLAND_DISPLAY=/tmp/seinas.sock cargo run --locked -p seinas-test-clients -- shrink
```

実際の画面(fbdev)で動かすときは、`--fake` と `--dump` の代わりに `--device` を使います。画面では、紫のウィンドウが一度出て、すぐ消えます。zeyes-minのウィンドウは残ります。

## 自動の試験

`crates/seinas-standalone/tests/shrink.rs` が、上の流れを偽の画面で確かめます。`cargo test` で動き、CIでも動きます。

1. seinas-standaloneを偽の画面で起動し、zeyes-minをつなぐ。
2. `bad-client shrink --step` をつなぎ、`shrink: drawn` が届くのを待つ。紫のウィンドウが画面に描かれていることを、画素で確かめる。
3. 1行送って、縮めさせる。
4. 標準出力の残りの3行と、終了コードが0であることを確かめる。
5. 紫のウィンドウ(後からつなぐので、zeyes-minの手前に、右下へずれて出る)が消え、zeyes-minのウィンドウの全体が見えていることを、画素で確かめる。seinas-standaloneとzeyes-minが動いていることを確かめる。
6. zeyes-minを止めて、新しいzeyes-minをつなぎ、描かれることを確かめる。

順序は、クライアントの出力と画面の画素を待って進めるので、時間のゆらぎでは落ちません。待つ時間の上限は、段階ごとに60秒です。

## 別のOSで動かすときに見る点

この試験は、コンポジタの側の次の働きに頼っています。

| 働き | 使われる所 |
| --- | --- |
| 縮められた共有メモリーを読んだときに、シグナル(SIGBUS)で知らせる | 手順4 |
| `sigaction` で `SA_SIGINFO` の受け口を置き、失敗した番地(`si_addr`)を受け取る | 手順5 |
| 受け口の中から、`mmap`(`MAP_FIXED`、`MAP_PRIVATE`、`MAP_ANONYMOUS`)で、その範囲を差し替える | 手順5 |
| 受け口から戻った後、読み出しを続ける | 手順5 |

クライアントの側は、`memfd_create`(封印なし)、`ftruncate` での縮小、fdの受け渡し(`sendmsg` の `SCM_RIGHTS`)を使います。

縮められた範囲を読んでもシグナルが来ないOS(たとえば、0が読めるだけのOS)では、コンポジタは縮められたことに気づきません。
その場合、クライアントの出力の3行目が `shrink: the second frame was drawn (the compositor did not notice)` になり、切られないまま待ち時間の上限で終わります(終了コード1)。コンポジタは落ちませんが、期待する結果とは違うので、OSの側の扱いを決める材料になります。
