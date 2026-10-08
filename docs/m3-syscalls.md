# M3 で使うシステムコール(strace の記録)

ZeikOS の M3(ZeikOS の上で、Wayland のクライアントがつながったウィンドウを出す)の準備として、M3 で動かす構成を Linux の上で strace した記録です。
ZeikOS に何が要るかを見積もるための材料で、Seinas の振る舞いを決めるものではありません。

## 取った条件

| 項目 | 内容 |
| --- | --- |
| 日付 | 2026-10-07 |
| OS | Linux 6.18(WSL2、x86-64) |
| strace | 6.8(`strace -f -tt -yy -s 96`。`-f` でスレッドも追う) |
| コンポジタ | `seinas-standalone`(Wayland のフロントエンド + fake screen のバックエンド)。musl の静的 PIE |
| クライアント | `zeyes-min`。musl の静的 PIE |
| ソース | commit 8380435 と同じソース(その後のコミットは CI とドキュメントだけ) |

使ったバイナリの SHA-256(`tools/build-musl.sh` で作ったもの。CI で作ったものとも同じ):

| バイナリ | SHA-256 |
| --- | --- |
| seinas-standalone | 1d29536e2c32fbe4f4f3214c287a47beb671f903bd8e51a970fe530bf859c80b |
| zeyes-min | 68ace2053c4e0a6b273e7ed631c7347ce20c054b00e2d66ea329c7afe9db06e2 |
| test-client(shrink の記録だけ) | 09fc617f5205897b1001360d9cbda12584eec396a1f95106fa322bc891f6ca90 |

手順:

1. コンポジタを strace の下で起動する:`seinas-standalone --fake 800x600 --socket <パス> --fonts target/fonts --test-input`
2. クライアントを strace の下で起動する:`WAYLAND_DISPLAY=<パス> zeyes-min`
3. 2 秒後、テスト用の入力インターフェースから、タイトルバーの close ボタンの上で press と release を送る。
4. zeyes-min が `xdg_toplevel.close` を受け取り、自分で終わる。
5. 1.5 秒後に、コンポジタを SIGTERM で止める。

あわせて、shm の縮小と SIGBUS の流れを見るために、もう 1 つ記録を取りました。同じコンポジタ(`--test-input` なし)に `test-client shrink` をつなぎ、pool を縮めさせたものです。

注意:

- close ボタンを押すために `--test-input` を使いました。これは標準入力を読むスレッド(名前は `test input`)を 1 本作ります。M3 の本番の構成には無いので、下の表からは、このスレッドの分を除いてあります(clone の数だけは注記を見てください)。
- fake screen で動かしたので、`/dev/fb0` の open・ioctl・mmap は入っていません。M2 の記録と同じものになるはずです。
- strace の生のログは、リポジトリに入れていません。

## システムコールの種類と回数

3 つの記録を合わせて 44 種類です。数は「コンポジタ / zeyes-min / shrink の記録のコンポジタ」の順です。

| 分類 | システムコール | 回数 |
| --- | --- | --- |
| 起動とメモリー | execve | 1 / 1 / 1 |
| | arch_prctl(ARCH_SET_FS) | 1 / 1 / 1 |
| | set_tid_address | 1 / 1 / 1 |
| | brk | 2 / 2 / 2 |
| | mmap | 53 / 13 / 39 |
| | munmap | 19 / 11 / 11 |
| | mprotect | 4 / 1 / 3 |
| | poll(起動時に fd 0〜2 を確かめる。events=0、timeout 0) | 1 / 1 / 1 |
| | getrandom(16 バイト、GRND_INSECURE) | 1 / 0 / 1 |
| | exit_group | 0 / 1 / 0 |
| シグナル | rt_sigaction | 6 / 5 / 6 |
| | rt_sigprocmask | 9 / 1 / 7 |
| | sigaltstack | 4 / 3 / 4 |
| | rt_sigreturn | 0 / 0 / 1 |
| スレッド | clone | 2 / 0 / 1 |
| | futex | 1 / 152 / 1 |
| | prctl(PR_SET_NAME) | 1 / 0 / 1 |
| | gettid | 1 / 0 / 1 |
| イベントループ | epoll_create1 | 2 / 1 / 2 |
| | epoll_ctl | 26 / 308 / 23 |
| | epoll_pwait | 18 / 151 / 17 |
| | eventfd2 | 2 / 1 / 1 |
| | timerfd_create | 1 / 1 / 1 |
| | timerfd_settime | 10 / 151 / 9 |
| ソケット | socket | 1 / 1 / 1 |
| | bind | 1 / 0 / 1 |
| | listen | 1 / 0 / 1 |
| | accept4 | 2 / 0 / 2 |
| | connect | 0 / 1 / 0 |
| | sendto | 4 / 2 / 4 |
| | sendmsg | 0 / 1 / 0 |
| | recvmsg | 7 / 8 / 9 |
| | ioctl(FIONBIO) | 1 / 0 / 1 |
| ファイル | open | 3 / 0 / 3 |
| | stat | 2 / 0 / 2 |
| | fstat | 3 / 1 / 3 |
| | read | 16 / 151 / 12 |
| | write | 9 / 27 / 9 |
| | close | 4 / 6 / 4 |
| | fcntl | 5 / 3 / 5 |
| | flock | 1 / 0 / 1 |
| 共有メモリー | memfd_create | 0 / 1 / 0 |
| | ftruncate | 0 / 1 / 0 |

- コンポジタの clone の 2 回のうち 1 回は、`test input` のスレッドの分です。本番の構成では 1 回(shm の後始末のスレッド)です。shrink の記録(`--test-input` なし)では 1 回でした。
- zeyes-min の epoll・timerfd・read・futex の多さは、16 ms ごとに起きて待ち直していたためです(下の「Seinas の側で軽くできる所」)。
- musl の静的 PIE なので、`open`・`stat` を使います(`openat`・`newfstatat` ではありません)。

## ZeikOS で効きそうな引数の形

### AF_UNIX と SCM_RIGHTS(Wayland の接続)

- `socket(AF_UNIX, SOCK_STREAM|SOCK_CLOEXEC, 0)`。ほかのソケットの種類は使いません。
- コンポジタ:先に、ソケットのパスに `.lock` を付けたロックファイルを `open(O_RDWR|O_CREAT|O_TRUNC, 0660)` して、`flock(LOCK_EX|LOCK_NB)` をかけます。ソケットのパスを `stat` して、無い(ENOENT)ことを確かめます。
- その後 `bind`(ファイルシステムの上の絶対パス)→ `listen`(backlog -1)→ `ioctl(FIONBIO)` で non-blocking にする → `accept4(SOCK_CLOEXEC)`。2 回目の `accept4` は EAGAIN で返ります(待っている接続がもう無い、という意味で使っています)。
- クライアント:`socket` → `connect`(同じパス)。
- 送受信:コンポジタは `sendto(MSG_DONTWAIT|MSG_NOSIGNAL)` と `recvmsg(MSG_DONTWAIT|MSG_CMSG_CLOEXEC)`、クライアントは `sendmsg`。
- クライアントは、共有メモリーの fd を SCM_RIGHTS で送ります。コンポジタは `recvmsg` で受け取り、受けた fd には close-on-exec が付きます。
- 相手が切断したことは、`recvmsg` が 0 バイトで返ることで知ります。
- SIGPIPE は、起動時に `rt_sigaction` で SIG_IGN にしています。送るときも MSG_NOSIGNAL を付けています。

### memfd と seal

- zeyes-min(SCTK)は、`memfd_create("smithay-client-toolkit", MFD_CLOEXEC|MFD_ALLOW_SEALING)` → `fcntl(F_ADD_SEALS, F_SEAL_SEAL|F_SEAL_SHRINK)` → `ftruncate(614400)` → `mmap(MAP_SHARED, PROT_READ|PROT_WRITE)` の順で pool を作ります。
- F_SEAL_SHRINK を付けているので、SCTK のクライアントは pool を縮められません。縮めるのは、seal を付けない作りのクライアント(`test-client shrink` など)だけです。
- コンポジタは、受け取った fd を `mmap(NULL, 614400, PROT_READ|PROT_WRITE, MAP_SHARED, fd, 0)` でマップします。2 つのプロセスが、同じページを MAP_SHARED で共有します。

### epoll の入れ子と EPOLLONESHOT

- コンポジタは epoll を 2 つ作ります(どちらも EPOLL_CLOEXEC)。1 つは calloop のもの、もう 1 つは wayland-server(wayland-backend)の中のものです。
- wayland-backend の epoll の fd を `fcntl(F_DUPFD_CLOEXEC)` で複製し、calloop の epoll に入れます(epoll の入れ子)。内側の epoll が readable になったことを、外側の epoll が知る必要があります。
- `epoll_ctl` は ADD と MOD。events は EPOLLIN|EPOLLPRI|EPOLLERR|EPOLLHUP です。timerfd と eventfd には EPOLLONESHOT を付け、イベントのたびに MOD で付け直します。listen しているソケットとクライアントの接続には付けません(level-triggered)。EPOLLET は使いません。
- `epoll_pwait` の timeout は -1(無期限)と 0(確かめるだけ)。sigmask は NULL です。

### timerfd と eventfd

- `timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC|TFD_NONBLOCK)`。`timerfd_settime` は相対時間(flags 0)の 1 回きりで、interval はありません。0 を渡して止めることも多いです。
- コンポジタは、描画を 1 秒に 60 回までに抑えるのに使います(残り 12.7 ms などを設定する)。
- `eventfd2(0, EFD_CLOEXEC|EFD_NONBLOCK)` は、calloop の wakeup に使います。read は、たいてい EAGAIN で返ります。

### clone(スレッド)

- flags は `CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|CLONE_SYSVSEM|CLONE_SETTLS|CLONE_PARENT_SETTID|CLONE_CHILD_CLEARTID|CLONE_DETACHED`、つまり 0x7d0f00 です。
- CLONE_DETACHED(0x400000)は、今の Linux では無視される flag です。musl が付けてきます。知らない flag として拒むと、スレッドが作れません。
- 前もって、スタックを `mmap(NULL, 2109440, PROT_NONE)` で取り、`mprotect` で 2101248 バイトを読み書きにします(先頭がガードページ)。
- clone の直前に `rt_sigprocmask(SIG_BLOCK)` で RTMIN・RT_1・RT_2 以外のシグナルを止め、直後に親と子の両方が `SIG_SETMASK` で元に戻します。
- 子のスレッドは、`prctl(PR_SET_NAME, "Shm dropping th…")`、`gettid`、`sigaltstack`(8 KiB とガードページ)を行ってから動きます。
- child_tidptr は、主スレッドが `set_tid_address` に渡したのと同じアドレスです(musl の作り)。
- このスレッドは、最初のクライアントが切断して、その pool を手放すときに作られます(Smithay 0.7.0)。まず `munmap` と `close` をして、次の仕事を futex で待ちます。
- Smithay は、このスレッドを作れないと `unwrap` で panic します。clone が無いと、最初のクライアントが切断した時点で、コンポジタが落ちます。

### futex

- zeyes-min:`FUTEX_WAKE_PRIVATE`(起こす数 2147483647)を 152 回。wayland-client の中の condition variable の notify_all です。スレッドは 1 本なので、いつも 0 人を起こすだけです。
- コンポジタ:shm の後始末のスレッドが、`FUTEX_WAIT_BITSET_PRIVATE`(値 4294967295、timeout なし、FUTEX_BITSET_MATCH_ANY)で待ちます。今回の記録では、待ったままプロセスごと止められました。
- WAKE で待っているスレッドを実際に起こす場面(2 つ目のクライアントが切断したとき)は、記録に入っていません。

### シグナルと SIGBUS の流れ

- Rust の標準ライブラリが、起動時に SIGSEGV と SIGBUS の handler を登録します(SA_ONSTACK|SA_SIGINFO|SA_RESTORER。stack overflow を見つけるため)。`sigaltstack` で 8 KiB の代替スタック(前にガードページ)を置きます。
- Smithay は、最初にクライアントのバッファを読むときに、SIGBUS の handler を自分のものに差し替えます(SA_NODEFER|SA_SIGINFO|SA_RESTORER。SA_ONSTACK は付けません)。前の handler は保存し、自分に関係のない SIGBUS はそちらへ回します。
- shrink の記録では、次の順に進みました。
  1. クライアントが pool を縮める。
  2. コンポジタが、縮められた pool を読む → SIGBUS(si_code は BUS_ADRERR、si_addr は pool の中)。
  3. handler の中で、pool の範囲に `mmap(MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS)` をかぶせる。
  4. `rt_sigreturn` で、読んでいた命令からやり直す(0 のピクセルが読める)。
  5. そのクライアントに protocol error を送って切断する。
  6. shm の後始末のスレッドを clone で作る。
- 止めるときは、外から SIGTERM を送りました。handler は無いので、既定の動き(プロセスごと終了)で、すべてのスレッドが終わりました。

### ファイル(フォント)

- `open(O_RDONLY|O_LARGEFILE|O_CLOEXEC)` → `fcntl(F_SETFD)` → `fstat` → `read` を 2 回(全体と、終わりの確かめ)→ `close`。約 4.7 MB と 5.3 MB を、ヒープに読み込みます。
- フォントのファイルを mmap することは、していません。
- フォントのファイルが無くても、コンポジタは動きます(タイトルバーだけを描き、ログに出します)。

### zeyes-min の終わり方

- `xdg_toplevel.close` を受け取る → 自分のクリーンアップ(`munmap`、`close`)→ `sigaltstack` を外す → `exit_group(0)`。

## M3 で ZeikOS に要るもの

ZeikOS のコードは読んでいません。M1・M2 で入ったもの(syscall 命令、静的 PIE の読み込みと auxv、TLS、mmap・munmap・mprotect、起動に要る小さなシステムコール、/dev/fb0)を前提にした見積もりです。

### M1・M2 で入っているはずのもの

- 読み込み、arch_prctl、set_tid_address、mmap・munmap・mprotect(MAP_FIXED・PROT_NONE を含む)
- rt_sigaction の登録、sigaltstack、getrandom、read・write、/dev/fb0 の open・ioctl・mmap
- futex の WAKE(zeyes-min の FUTEX_WAKE_PRIVATE は、これで足りる見込み)
- 確かめておくとよいもの:brk、poll(events=0、timeout 0)、rt_sigprocmask、fstat、close、exit_group、getrandom の GRND_INSECURE(flag 4)、open の O_LARGEFILE・O_CLOEXEC

### 新しく要るもの

- 2 つのプロセスを同時に動かすこと(コンポジタとクライアント)。クライアントをだれが起動するか(shell や init に当たるもの)も要ります。
- AF_UNIX の SOCK_STREAM:socket・bind(ファイルシステムの上のパス)・listen・accept4・connect、FIONBIO、MSG_DONTWAIT・MSG_NOSIGNAL・MSG_CMSG_CLOEXEC
- SCM_RIGHTS での fd の受け渡し(sendmsg・recvmsg)
- memfd_create、F_ADD_SEALS(F_SEAL_SEAL と F_SEAL_SHRINK)、ftruncate
- 2 つのプロセスのあいだの MAP_SHARED(同じページを両方が見る)
- epoll(epoll_create1・epoll_ctl・epoll_pwait)、EPOLLONESHOT と MOD での付け直し、epoll の入れ子、fcntl の F_DUPFD_CLOEXEC
- eventfd2(non-blocking)、timerfd_create・timerfd_settime(CLOCK_MONOTONIC、相対時間、0 での停止)
- flock(LOCK_EX|LOCK_NB)と、書き込めるファイルシステム(ロックファイルの O_CREAT|O_TRUNC、ソケットのパスの stat)
- スレッド:clone、futex の WAIT_BITSET、prctl(PR_SET_NAME)、gettid、スレッドごとのシグナルマスクと代替スタック、終わり方(exit_group で全スレッドが終わる)
- shm の縮小と SIGBUS:縮められた共有メモリーのページの無効化、フォールトからの SIGBUS の同期的な配送(SA_SIGINFO、si_code・si_addr)、handler の中での MAP_FIXED の差し替え、rt_sigreturn での命令のやり直し
- 外からプロセスを止める手段(SIGTERM の既定の動き、または同じ働きのもの)

### 振る舞いで気をつける所

- `accept4`・`recvmsg`・eventfd の read が、待たずに EAGAIN を返すこと。Seinas は、これを「もう無い」の合図として使います。
- `recvmsg` が 0 バイトを返すことで、切断を知ること。
- epoll の入れ子で、内側の epoll の状態の変化が、外側にも伝わること。
- EPOLLONESHOT で、一度イベントを返したら、MOD で付け直すまで返さないこと。
- clone の CLONE_DETACHED を、知らない flag として拒まないこと。
- Smithay の SIGBUS の handler は SA_ONSTACK を付けず、主スレッドのスタックの上で動くこと。
- プロセスを止めたとき、futex で待っているスレッドも一緒に終わること。

## スレッドなしで、どこまで動くか

- クライアントがつながって、描画されるまでは、スレッドを使いません。clone は、最初のクライアントが切断するときに起きます。
- そのため、スレッドの対応が無くても、「つないで描画する」ところまでは動くはずです。
- ただし、クライアントが切断した時点で、Smithay が shm の後始末のスレッドを作ろうとし、作れないと panic でコンポジタが落ちます。「閉じて終わる」には、スレッドの対応が要ります。
- shm の縮小と SIGBUS の対応が要るのは、seal を付けずに pool を縮めるクライアントが来たときだけです。SCTK のクライアントは F_SEAL_SHRINK を付けるので、縮めません。

## Seinas の側で軽くできる所

「Linux のプログラムをそのまま動かす」方針に合わせて、Linux の上でも良くなるものだけを挙げます。

- zeyes-min の待ち方:16 ms の timeout つきで待つのを繰り返していて、何も起きていなくても 1 秒に約 60 回起きていました(timerfd_settime・epoll_ctl・epoll_pwait・futex・read が、それぞれ約 150 回)。待つ理由が無いときは timeout なしで待つようにすれば、Linux でも無駄が減り、ZeikOS では timerfd と epoll を呼ぶ回数が大きく減ります。
- shm の後始末のスレッド:Smithay 0.7.0 の作りで、Seinas の側からは止められません。スレッドを使わない構成にするには、Smithay を変えるか、別のバージョンを選ぶ必要があり、ZeikOS のための作りになるので勧めません。ZeikOS の側でスレッドに対応するのが筋だと考えます。
- epoll の入れ子、ロックファイルと flock:wayland-server と Smithay の作りです。Seinas の側で避けるには特別な作りが要るので、勧めません。
- フォント:置かなくても動くので、M3 の最初はフォントなしで始められます(変更は要りません)。
- `--test-input`:M3 では使わないでください(標準入力を読むスレッドが 1 本増えます)。
