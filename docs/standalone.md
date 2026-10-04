# 受け口とfbdevを組み合わせた構成

`crates/seinas-standalone` は、親のWaylandを使わない構成のSeinasです。
Waylandの受け口で受けたクライアントの画面を、共通の描画で合成し、fbdevの画面へ出します。

## 構成

| クレート | 役割 | この構成で使うか |
| --- | --- | --- |
| `seinas-frontend` | Waylandの受け口(Smithay) | 使う |
| `seinas-render` | 共通の描画 | 使う |
| `seinas-fbdev`(ライブラリ) | fbdevの裏側と、偽の画面 | 使う |
| `seinas`(入れ子の裏側。SCTK、wayland-client) | 親のWaylandへ出す | 使わない |

受け口は、もとは `seinas` の中のモジュールでした。入れ子の構成とこの構成の両方から使えるように、`seinas-frontend` として切り出しています。
Smithayのハンドラは、コンポジタの状態の型そのものに実装する決まりなので、`seinas-frontend` は次の2つを用意しています。

- `FrontendHost`: 状態の型が実装する。受け口の置き場所と、描き直しの知らせ方を教える。
- `delegate_frontend!`: Smithayのハンドラと、受け口の作り方を、状態の型に実装するマクロ。

入れ子の裏側が依存に入っていないことは、次で確かめます。CIと `tools/build-musl.sh` にも入れてあります。

```bash
tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client calloop-wayland-source
```

受け口の側のwayland系のクレート(wayland-server、wayland-backend、wayland-protocolsなど)は、入っていて当然なので見ません。

## 動かし方

装置を開かずに、偽の画面(メモリー上)で動かす例です。

```bash
cargo run --locked -p seinas-standalone -- --fake 800x600 --dump /tmp/screen.ppm --socket /tmp/seinas.sock
```

```bash
WAYLAND_DISPLAY=/tmp/seinas.sock cargo run --locked -p zeyes-min
```

| 引数 | 意味 |
| --- | --- |
| `--device PATH` | 画面の装置。無ければ環境変数 `SEINAS_FBDEV`、それも無ければ `/dev/fb0` |
| `--fake WIDTHxHEIGHT` | 装置を開かず、その大きさの偽の画面へ描く |
| `--dump FILE` | 偽の画面の中身を、描くたびにPPM形式の画像としてファイルに書く(`--fake` のときだけ) |
| `--socket PATH` | クライアントを待ち受けるソケットの場所。無ければ環境変数 `SEINAS_SOCKET`、それも無ければ `XDG_RUNTIME_DIR` の下の `seinas-0` |

- ソケットの場所を指定したときは、`XDG_RUNTIME_DIR` は要りません。
- ソケットの隣に、同じ名前に `.lock` を付けた鍵のファイルを作ります。
- クライアントは、`WAYLAND_DISPLAY` にソケットの絶対パスを入れてつなぎます。
- `--dump` は、書きかけを読まれないように、別の名前で書いてから名前を付け替えます。

## 公開するもの

| グローバル | 中身 |
| --- | --- |
| wl_compositor、wl_subcompositor | 画面(サーフェス) |
| wl_shm | 共有メモリーのバッファ |
| xdg_wm_base | ウィンドウ |
| wl_output | 画面の大きさ。表示モードは1つで、幅と高さは画面と同じ |

wl_seatは公開しません(入力は扱いません)。

## 描く時機

fbdevには、垂直同期の知らせがありません。そこで、次のようにしています。

- クライアントがcommitすると、「描き直しが要る」と覚えます。
- 描き直しが要るときにだけ、1枚を合成して画面へ出します。
- 出した直後に、クライアントへframeコールバックを返します。クライアントは、それを合図に次の絵を描きます。
- 描く回数は、1秒に60回までです。前に描いてから1/60秒たっていなければ、その時刻まで待ってから描きます。
- 起動したときは、クライアントがいなくても、背景を1枚出します。
- 何も起きていない間は、何もせずに待ちます(定期的に目を覚ますことはしません)。

## 画面への反映

`seinas-fbdev` の `Presenter` を、そのまま使っています。今はLinux向けの `WriteThrough`(何もしない)だけです。
別のOS向けの反映は、`Presenter` を実装した型を足して、`src/screen.rs` で選ぶものを差し替えれば入ります。

## 今の制限

- 入力は扱いません。
- クライアントのウィンドウは、左上にそのまま重ねて描きます。
- 止めるには、シグナルで終わらせます。終わるときの後片付け(画面を元に戻す、ソケットと鍵のファイルを消す)はしていません。次に起動したときは、残った鍵のファイルを取り直して、そのまま動きます。
- 実際の装置(`/dev/fb0`)では、まだ動かしていません。

## 試験

`crates/seinas-standalone/tests/with_client.rs` が、次を確かめます。親のWaylandも画面の装置も要らないので、CIで動きます。

1. 偽の画面(640x480)で起動すると、背景が1枚出て、ソケットで待ち受ける。
2. zeyes-minをつなぐと、そのウィンドウが画面の左上に描かれる。ポインターは無いままです。
3. zeyes-minを強制終了すると、ウィンドウが消えて背景に戻り、後始末のスレッドが1本できる。
4. もう一度つなぐと、同じように描かれる。スレッドは増えない。

描かれたかどうかは、決まった位置の画素の色で見ます(目のまわりの緑、白目、目の縁、瞳、ウィンドウの外の背景)。
待つ時間の上限は、段階ごとに60秒です。条件がそろい次第、すぐ先へ進みます。

## 使うシステムコール

muslの静的版を `strace -f` の下で動かし、起動、クライアントの接続、描画、強制終了、再接続、もう一度の強制終了までを記録しました(2026-10-04、Linux 6.18、偽の画面800x600)。
全体で230回です。

### コンポジタの側(seinas-standalone)

ソケット:

| 呼び出し | 旗など |
| --- | --- |
| `socket(AF_UNIX, SOCK_STREAM\|SOCK_CLOEXEC, 0)` | 待ち受け用。SOCK_NONBLOCKは付けない |
| `bind` | ソケットのパス |
| `listen(fd, -1)` | 待ち行列の長さは -1(OSの上限まで) |
| `ioctl(fd, FIONBIO, [1])` | 待ち受けのソケットを、待たない設定にする |
| `accept4(fd, ..., SOCK_CLOEXEC)` | SOCK_NONBLOCKは付けない。次の接続が無ければEAGAIN |
| `recvmsg(..., MSG_DONTWAIT\|MSG_CMSG_CLOEXEC)` | クライアントの要求を読む |
| `sendto(..., MSG_DONTWAIT\|MSG_NOSIGNAL, NULL, 0)` | クライアントへイベントを送る |
| `close` | 切れたクライアントのソケットを閉じる |

鍵のファイル(待ち受けの前に1回):
`open(O_RDWR|O_CREAT|O_TRUNC|O_CLOEXEC)`、`fcntl(F_SETFD, FD_CLOEXEC)`、`flock(LOCK_EX|LOCK_NB)`、`fstat`、`stat`(鍵のファイルと、ソケットのパス)。

待ち:

| 呼び出し | 旗など |
| --- | --- |
| `epoll_create1(EPOLL_CLOEXEC)` | 2つ作る(イベントループ用と、クライアントの待ち受け用) |
| `fcntl(F_DUPFD_CLOEXEC)` | 2つ目のepollを複製し、1つ目のepollに登録する(epollの入れ子) |
| `epoll_ctl` | ADD、MOD、DEL。EPOLLIN、EPOLLPRI、EPOLLERR、EPOLLHUP、EPOLLONESHOT |
| `epoll_pwait` | 期限は -1(無期限)か0。シグナルの指定はNULL |
| `eventfd2(0, EFD_CLOEXEC\|EFD_NONBLOCK)` | イベントループを起こすための口。毎周 `read` して、EAGAINが返る |
| `timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC\|TFD_NONBLOCK)` | 期限付きで待つため |
| `timerfd_settime(fd, 0, ...)` | 相対の時間。描く間隔を待つときに値が入る。それ以外は0(止める) |
| `poll` | 起動時に1回だけ。Rustの標準ライブラリが、0〜2番のfdを確かめる |

共有メモリー:

- コンポジタの側は、`memfd_create`、`ftruncate`、封印の `fcntl`、プールへの `fstat` を、一度も呼びません。
- プールのfdは、クライアントから受け取ります。受け取ったら `mmap(NULL, 大きさ, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)` で対応づけます。
- 後始末は、`munmap` と `close` です(後始末のスレッドが呼びます)。

fdの受け渡し:

- 受け取りは `recvmsg` で、`SCM_RIGHTS` の付いた制御メッセージに、fdが1つ入っています(`MSG_CMSG_CLOEXEC` 付き)。
- コンポジタの側からfdを送ることはありません。`sendmsg` は一度も呼ばず、送信はすべて `sendto` です。

スレッド:

- 前回の調査(入れ子の構成)と同じでした。
- `clone` は1回で、最初のクライアントが去ったときです。旗の組も同じです(CLONE_VM、FS、FILES、SIGHAND、THREAD、SYSVSEM、SETTLS、PARENT_SETTID、CHILD_CLEARTID、DETACHED)。
- スレッドのスタックは、`mmap`(2,109,440バイト、PROT_NONE)と `mprotect`(2,101,248バイト)です。
- `futex` は3回だけです。スレッドが待つ `FUTEX_WAIT_BITSET_PRIVATE`(期限なし)が2回、2つ目のプールを捨てるときの `FUTEX_WAKE_PRIVATE`(数は1)が1回です。
- 入れ子の構成で毎周出ていた `FUTEX_WAKE_PRIVATE`(数は2147483647)は、この構成では出ません。出どころのwayland-clientを含まないためです。

同時に開くfdの数:

| 状態 | 数 | 内訳 |
| --- | --- | --- |
| クライアントがいない | 10 | 0〜2番、epollが3つ(うち1つは複製)、eventfd、timerfd、鍵のファイル、待ち受けのソケット |
| クライアントが1つ | 12 | 上に加えて、クライアントのソケットと、プールのfd |

- 最大は12でした。クライアント1つにつき、ソケット1つと、プール1つごとにfdが1つ増えます。
- 装置を使うときは、装置のfdが1つ増えます。`--dump` を使うときは、書き出す間だけ1つ増えます。

そのほか:
`mmap`(匿名。ヒープと、800x600の絵2枚ぶん)、`munmap`、`brk`、`mprotect`、`rt_sigaction`、`rt_sigprocmask`、`sigaltstack`、`getrandom(16, GRND_INSECURE)`、`arch_prctl(ARCH_SET_FS)`、`set_tid_address`、`write`(ログ)。

### クライアントの側(zeyes-min)

M3では、クライアントも別のプロセスとして動きます。muslの静的版の記録です。

| 観点 | 呼び出し |
| --- | --- |
| ソケット | `socket(AF_UNIX, SOCK_STREAM\|SOCK_CLOEXEC, 0)`、`connect` |
| 共有メモリー | `memfd_create("smithay-client-toolkit", MFD_CLOEXEC\|MFD_ALLOW_SEALING)`、`ftruncate(fd, 614400)`、`fcntl(fd, F_ADD_SEALS, F_SEAL_SEAL\|F_SEAL_SHRINK)`、`fstat`、`mmap(..., MAP_SHARED, fd, 0)` |
| fdの受け渡し | `fcntl(F_DUPFD_CLOEXEC)` で複製したfdを、`sendmsg` の `SCM_RIGHTS` で送る |
| 待ち | `epoll_create1`、`epoll_ctl`、`epoll_pwait`、`eventfd2`、`timerfd_create`、`timerfd_settime`、`ppoll`(最初の往復で1回) |
| スレッド | `clone` は無し。`futex(FUTEX_WAKE_PRIVATE, 2147483647)` を、ループの1周ごとに呼ぶ(待つ相手はいないので、戻り値は0) |

- 開くfdは8つでした。
- zeyes-minは、16ミリ秒ごとに目を覚ます作りなので、何も起きていなくても、待ちの呼び出しが続きます。

## muslでの静的ビルドの記録

`tools/build-musl.sh` は、seinas-standaloneも静的PIEとして作ります。`tools/elf-report.sh` の出力です(2026-10-04)。

```
== seinas-standalone
file size: 3930256 bytes
  Type:                              DYN (Position-Independent Executable file)
  Machine:                           Advanced Micro Devices X86-64
PT_INTERP: 0, DT_NEEDED: 0
PT_TLS: FileSiz 0x000098, MemSiz 0x0002f8, Align 0x10
Program Headers:
  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align
  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x021080 0x021080 R   0x1000
  LOAD           0x022000 0x0000000000022000 0x0000000000022000 0x21c356 0x21c356 R E 0x1000
  LOAD           0x23f000 0x000000000023f000 0x000000000023f000 0x083eb4 0x083eb4 R   0x1000
  LOAD           0x2c2f20 0x00000000002c3f20 0x00000000002c3f20 0x01a3c0 0x01c1c8 RW  0x1000
  DYNAMIC        0x2da5f8 0x00000000002db5f8 0x00000000002db5f8 0x000180 0x000180 RW  0x8
  NOTE           0x000270 0x0000000000000270 0x0000000000000270 0x000024 0x000024 R   0x4
  TLS            0x2c2f20 0x00000000002c3f20 0x00000000002c3f20 0x000098 0x0002f8 R   0x10
  GNU_EH_FRAME   0x27b1f0 0x000000000027b1f0 0x000000000027b1f0 0x0082f4 0x0082f4 R   0x4
  GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x10
  GNU_RELRO      0x2c2f20 0x00000000002c3f20 0x00000000002c3f20 0x0190e0 0x0190e0 R   0x1
```

- `ET_DYN` の静的PIEで、`PT_INTERP` も `DT_NEEDED` もありません。
- `PT_TLS` は、スレッドごとに0x2f8バイト(760バイト、16バイト境界)です。
- メモリーに置いたときの像の大きさは0x2e00e8(約2.88 MiB)です。
- 再配置は `R_X86_64_RELATIVE` だけで、5607個です。
- 実行ファイルの大きさは3,930,256バイト、stripすると3,005,104バイトです(glibc版は2,520,160バイト)。
- YMM・ZMMを使う命令と `xgetbv` は、0個です。
- 偽の画面800x600で、zeyes-minを1つつないだときの最大RSSは6,148 kB、主スレッドのスタックで触れた量は20 kBでした。
