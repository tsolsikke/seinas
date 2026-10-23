# ZeikOS で Seinas を動かすのに要るもの

Seinas の構成(seinas・seinas-standalone・seinas-taskbar・zeyes-min)が ZeikOS の上で動くのに要るものを 1 つにまとめ、ZeikOS の今の対応と突き合わせた文書です(C1S4 の準備)。

- Seinas の側の値は、2026-10-11 に Linux(WSL2)の上で、musl の静的ビルドを `strace -f` で取ったものです(4 つを同時に動かし、seinas は seinas-standalone を親にして入れ子で動かした)。取り方と、変更ごとの差分の記録は `docs/m3-syscalls.md` にあります。こちらは、その記録から「要るもの」を引いた表です。
- 2026-10-21 に、下の「2026-10-21 に合わせ直したこと」の節と、表のうち時計・epoll・eventfd・timerfd・ppoll・ソケット・ロックファイル・memfd の封・スレッドの行を、zeikos の 253c8b39(2026-10-20)のソースと ADR に合わせました。ほかの行と「数の要約」は、10/11 のままです。
- 2026-10-23 に、「fd の種類」の節と、「新しい課題の候補」の 13 を、zeikos の 6cfe9e8a(2026-10-23)の ADR-0091(fd の上限を 32 にした)と、seinas-standalone の `--no-ime-socket` に合わせました。
- ZeikOS の側は、隣の zeikos のリポジトリの main(9666a4cce94c6e886d3c83b3b46ed99bd3f727fb)のソースを読んで判断しました。ビルドも実行もしていません。根拠は、ファイルと関数(と行)で書きます。断りの無いファイルは `kernel/src/syscall.rs` です。syscall の番号から関数への表は `dispatch()`(771〜1128 行)で、表に無い番号は `-ENOSYS` を返します(1123 行)。
- 下の「まとまり」の欄は、ZeikOS の側の機能のまとまりの名前です。どの課題に当たるかは、Jira を正とします(下の「まとまりごとの一覧」)。

状態の書き方:**有る**(要る形で動く)、**一部**(要るフラグや場合の一部が無い。足りないものを備考に書く)、**無い**(番号が無いか、要る形がすべて断られる)、**不明**。

## 構成と、ZeikOS の上での役目

| 構成 | 役目 | ZeikOS の上で |
| --- | --- | --- |
| seinas-standalone | コンポジタ(親の Wayland を持たない構成。fbdev か fake screen へ描く) | M3 で動かす本体 |
| seinas | コンポジタ(入れ子の構成。親の Wayland のクライアントとしてウィンドウを出す) | 親の Wayland が無いので、ZeikOS では動かさない見込み。比べるために載せる |
| seinas-taskbar | タスクバー(シェルの部品。layer-shell と foreign-toplevel のクライアント) | コンポジタと同時に動かす |
| seinas-wallpaper | 壁紙(シェルの部品。layer-shell のクライアント) | コンポジタと同時に動かす(下の「壁紙」) |
| zeyes-min | 試しのクライアント(xdg_toplevel のウィンドウを 1 つ出す) | コンポジタと同時に動かす |

## 要るものと、ZeikOS の対応

「使う構成」の欄:S は seinas-standalone、N は seinas(入れ子)、T は seinas-taskbar、Z は zeyes-min。

### 起動とメモリー

| 要るもの(syscall とフラグ) | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| 静的 PIE の読み込み(execve に当たるもの)、arch_prctl(ARCH_SET_FS)、brk、exit_group | S N T Z | 有る | `SYS_SPAWN`(`spawn_from_ring3` 3624 行)、`sys_arch_prctl_body` 5891 行、`sys_brk` 4469 行 | execve の番号は無く、ZeikOS だけの spawn で読み込む。実行ファイルは 16 MiB まで、ユーザーのスタックは 16 KiB | — |
| set_tid_address | S N T Z | 一部 | `sys_set_tid_address` 5534 行 | アドレスを覚えるだけで、終わるときの消去と futex の WAKE はしない(スレッドを使うときに要る) | スレッド |
| mmap(PROT_NONE・PROT_READ\|PROT_WRITE、MAP_PRIVATE\|MAP_ANONYMOUS、MAP_STACK、MAP_FIXED)、munmap・mprotect(無名のもの) | S N T Z | 有る | `mmap_from_ring3` 2939 行 → `mmap_anonymous_from_ring3` 2312 行、`munmap_from_ring3` 2464 行、`mprotect_from_ring3` 2528 行 | 1 回 16 MiB まで、マップは 64 個まで。ページは先に割り当てる。知らないフラグは断らない | — |
| ファイルの mmap(PROT_READ、MAP_SHARED。フォントと xkb のファイル) | S N T | 無い | 2993 行の `shm_of(fd)` が、memfd と画面のほかは EBADF | フォントは、mmap できなければヒープへ読む(Seinas の側で対応済み。プロセスごとに約 14 MB 増える)。xkbcommon も、mmap できないときは読み込みに切り替わる作り(ZeikOS の上では未確認)。zeikos の `docs/deferred-decisions.md` 323 行で後回し | 新しい課題の候補 |
| poll(events=0、timeout 0。Rust の起動のとき) | S N T Z | 有る | `poll_from_ring3` 1857 行(1901 行) | — | — |
| getrandom(GRND_INSECURE と、フラグ 0) | S N T | 有る | `sys_getrandom` 5659 行 | 1 回 256 バイトまで | getrandom のフラグ 0(フラグ 0 は動く形になっている) |
| clock_gettime(CLOCK_MONOTONIC) | S N T Z | 有る | `sys_clock_gettime` 5126 行 | 分解能 10 ms。vDSO は無い(AT_SYSINFO_EHDR を置かない。`kernel/src/abi/linux/initial_stack.rs` 75 行)ので、musl は syscall で呼ぶ | — |
| clock_gettime(CLOCK_REALTIME。タスクバーの時計と、ログの時刻) | S N T | 有る | `sys_clock_gettime`、zeikos の ADR-0090 | 起動時に CMOS の RTC を 1 回読み、単調な時計の経過を足す(読めなければ EINVAL のまま)。コンポジタも、tracing のログの時刻(UTC)で使う。2026-10-21 に、ZeikOS の上でログに UTC の時刻が出ることを確かめた(ZeikOS の側) | — |

### 共有メモリー(Wayland の shm)

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| memfd_create(MFD_CLOEXEC\|MFD_ALLOW_SEALING) | S N T Z | 有る | `memfd_create_from_ring3` 2893 行 | 名前とフラグは読まない | — |
| ftruncate(memfd。pool の大きさ。SCTK は伸ばす) | N T Z | 一部 | `ftruncate_from_ring3` 2910 行、`kernel/src/shm.rs` の `set_size` | 1 回だけ(2 回目は EINVAL)、32 KiB まで。zeyes-min の pool は 614,400 バイト、タスクバーは 256,000 バイト、800x600 のウィンドウは 1.9 MB で、どれも越える | 新しい課題の候補 |
| 2 つのプロセスの間の MAP_SHARED(PROT_READ\|PROT_WRITE、memfd) | S N T Z | 一部 | 2993〜3067 行、`kernel/src/shm.rs`(ADR-0065) | 共有は動く。memfd はシステム全体で 2 つまで(`MAX_SHM`、`shm.rs` 41 行)、1 つ 8 ページ(`MAX_SHM_PAGES`、45 行)。クライアントごとに 1 つ以上と、コンポジタの keymap の分が要る | 新しい課題の候補 |
| munmap・mprotect(shm のマップ) | S N T Z | 一部 | `munmap_from_ring3` 2464 行(shm・画面は EINVAL) | コンポジタは、クライアントが pool を手放すと munmap する | 新しい課題の候補 |
| fcntl(F_ADD_SEALS。F_SEAL_SEAL\|F_SEAL_SHRINK と、keymap の F_SEAL_SEAL\|F_SEAL_SHRINK\|F_SEAL_GROW\|F_SEAL_WRITE) | S N T Z | 有る | zeikos の ADR-0065 の Addendum(memfd のフラグと封と FD_CLOEXEC) | memfd_create のフラグと名前を確かめ、封を持って効かせる | — |
| write と lseek(SEEK_SET。memfd。コンポジタが keymap を書く) | S N | 無い | `sys_write` 6520〜6601 行(memfd は EBADF)、`sys_lseek` 4677 行(何もしない) | keymap を書けないと、キーボードの keymap をクライアントへ渡せない | 新しい課題の候補 |
| pread64(親の keymap を読む) | N | 無い | 17 番が無い | 入れ子の構成だけ | 新しい課題の候補(低い) |

### イベントループ

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| epoll_create1(EPOLL_CLOEXEC)、epoll_ctl(ADD・MOD・DEL、EPOLLIN\|EPOLLPRI\|EPOLLERR\|EPOLLHUP、EPOLLONESHOT)、epoll_pwait(timeout -1・0・正の値)、epoll の入れ子 | S N T Z | 有る | `kernel/src/epoll.rs`、zeikos の ADR-0088 | レベルトリガーと EPOLLONESHOT(EPOLLET は断る)。epoll はシステム全体で 8 個、1 つに項目 16 個、入れ子の鎖は 5 個まで(`MAX_EPOLLS`・`MAX_ENTRIES`・`MAX_CHAIN`)。コンポジタは 2 個(うち 1 つを複製して 3 つの fd)、クライアントは 1 個ずつ使う | — |
| eventfd2(0、EFD_CLOEXEC\|EFD_NONBLOCK) | S N T Z | 有る | zeikos の ADR-0088 | システム全体で 8 個。プロセスごとに 1 個使う | — |
| timerfd_create(CLOCK_MONOTONIC、TFD_CLOEXEC\|TFD_NONBLOCK)、timerfd_settime(相対、1 回きり、0 で止める) | S N T Z | 有る | `kernel/src/timerfd.rs`、zeikos の ADR-0088 | システム全体で 8 個(`MAX_TIMERFDS`)。締切は 10 ms のティックに切り上げる。プロセスごとに 1 個使う | — |
| ppoll(fd 1 つ、POLLIN\|POLLERR、timeout と sigmask は NULL) | N T Z | 無い | 271 番が無い(2026-10-21 の zeikos でも、番号の表に無い) | wayland-client が使う(タスクバーは、つないだ直後の往復で使う)。クライアントを ZeikOS で動かすときに要る | 新しい課題の候補 |
| fcntl(F_SETFD の FD_CLOEXEC、F_DUPFD_CLOEXEC) | S N T Z | 有る | `sys_fcntl` 5789 行 | CLOEXEC は覚えない(exec が無いので困らない) | — |

### AF_UNIX ソケット(Wayland の接続)

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| socket(AF_UNIX、SOCK_STREAM\|SOCK_CLOEXEC) | S N T Z | 有る | `socket_from_ring3`、zeikos の ADR-0087 | SOCK_CLOEXEC と SOCK_NONBLOCK を受ける | — |
| bind(ファイルシステムの上のパス)、ソケットのパスの stat | S N | 一部 | `bind_from_ring3`、zeikos の ADR-0087 の決定 2 | 名前はカーネルの表に置き、inode は作らない(stat と unlink は ENOENT)。wayland-server の形(ロックファイルを取り、残ったソケットのファイルを消してから bind)は、これで通る。名前は 107 バイトまで | — |
| listen(backlog -1) | S N | 有る | `listen_from_ring3` 2043 行 | backlog は見ない | — |
| accept4(SOCK_CLOEXEC)と、待っている接続が無いときの EAGAIN、ioctl(FIONBIO) | S N | 有る | `accept_from_ring3`、`SYS_IOCTL` の FIONBIO、zeikos の ADR-0087 の Addendum | 待たない印の立った listener の accept は、待っている接続が無ければ EAGAIN を返す。FIONBIO は fd の種類によらず受ける | — |
| connect | N T Z | 有る | `connect_from_ring3` 2122 行 | — | — |
| sendto・sendmsg(MSG_DONTWAIT\|MSG_NOSIGNAL)、recvmsg(MSG_DONTWAIT\|MSG_CMSG_CLOEXEC)、SCM_RIGHTS での memfd の受け渡し | S N T Z | 一部 | `sendmsg_from_ring3`・`recvmsg_from_ring3`(2026-10-19・20) | MSG_DONTWAIT・MSG_NOSIGNAL・MSG_CMSG_CLOEXEC を受け、ほかのフラグは断る。待たない形なら、空きや読めるものが無いとき EAGAIN。SCM_RIGHTS は、1 通に 1 つ、memfd だけで、向きごとに 1 つまでしか待たせられない(下の「SCM_RIGHTS を続けて送る場面」) | AF_UNIX ソケットのフラグ・accept4・SCM_RIGHTS |
| 相手が切ったとき recvmsg が 0 を返すこと | S N T Z | 有る | `read_from_socket` 2168 行 | — | — |
| ソケットの数:コンポジタは listen を 3 つ(ふだんのソケットと、シェルの部品のソケットと、IME のソケット)、接続はクライアントの数だけ | S N | 有る | `kernel/src/socket.rs`(`MAX_LISTENERS`=4、`MAX_CONNECTIONS`=8。システム全体で、受け付け待ちも含む) | — | — |

### ファイル

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| open(O_RDONLY\|O_LARGEFILE\|O_CLOEXEC)、stat、fstat、read、close | S N T | 有る | `sys_open` 4869 行、`sys_stat` 6164 行、`sys_fstat` 5828 行、`sys_read` 4038 行 | — | — |
| ロックファイル:open(O_RDWR\|O_CREAT\|O_TRUNC)と flock(LOCK_EX\|LOCK_NB) | S N | 有る | `sys_flock`(`kernel/src/flock.rs`)、`sys_open`、zeikos の ADR-0087 | flock の表はシステム全体で 8 本。O_TRUNC の無い O_RDWR は EROFS のまま | — |
| faccessat2(AT_FDCWD、xkb の置き場、R_OK\|X_OK、AT_EACCESS) | S N | 無い | 439 番が無い | xkbcommon が、xkb のファイルの置き場を確かめる | 新しい課題の候補 |
| 一時ファイル:open(`$XDG_RUNTIME_DIR` のディレクトリ、O_RDWR\|O_TMPFILE\|O_CLOEXEC)、write、fcntl(F_DUPFD_CLOEXEC)、close(IME の keyboard の grab と、バージョン 7 より古い wl_keyboard へ、封をしない keymap を渡すとき) | S N | 無い | `sys_open`(O_RDWR は EROFS。2026-10-12 に、HEAD が 809e5de の zeikos のソースで確かめた) | Smithay 0.7.0 は、IME の grab へは、いつもこの形で渡す。tempfile は、O_TMPFILE が使えなければ、名前を付けて作って unlink する形に切り替える(そのときは O_CREAT\|O_EXCL と unlink が要る) | 新しい課題の候補 |
| クライアントから受け取った keymap の fd の mmap(PROT_READ、MAP_PRIVATE。IME の virtual-keyboard) | S N | 一部 | `mmap_from_ring3` と `shm_of`(memfd のほかは EBADF。MAP_PRIVATE と MAP_SHARED を分けない。2026-10-12 に、HEAD が 809e5de のソースで確かめた) | IME が memfd で渡せば通る(読むだけなので、分けなくても困らない)。Smithay が grab で渡した一時ファイルを、IME がそのまま戻すと EBADF | ファイルの mmap(新しい課題の候補) |
| 4 MiB を越えるファイルを読む(ext2 の 2 段目の間接ブロック) | S N T | 無い | `common/src/ext2.rs` 364 行、`uses_unsupported_indirection` 487 行 | 同梱のフォントは 3 本とも越える(4,667,380・4,669,688・5,250,304 バイト)。フォントが無くても、コンポジタとタスクバーは文字を出さずに動く | 4 MiB を越えるファイル |
| getdents64 と open(O_DIRECTORY)(フォントの置き場を調べるとき。設定でフォントを指定したときだけ) | S N T | 有る | `sys_getdents64` 5002 行 | — | — |
| 書けるファイルシステム(ロックファイル、ソケットの置き場) | S N | 一部 | ext2 の書き込み、`/tmp` は `kernel/build.rs` 1447 行で作る | 書けるのは O_WRONLY の形だけ。ソケットの「パス」はファイルにならない | flock とパスへの bind |

### シグナル

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| rt_sigaction(SA_SIGINFO\|SA_ONSTACK\|SA_RESTORER、SA_NODEFER、SIGPIPE の SIG_IGN)、rt_sigprocmask、sigaltstack | S N T Z | 一部 | `sys_rt_sigaction` 5246 行、`kernel/src/process_state.rs` の `set_action` 175 行、5290・5332 行 | 登録は受けるが、配送しない。ふだんの動きには困らない | 新しい課題の候補(シグナルの配送) |
| shm の縮小のときの SIGBUS の同期的な配送(BUS_ADRERR、si_addr)、handler の中の MAP_FIXED、rt_sigreturn | S N | 無い | 15 番(rt_sigreturn)が無い。zeikos の `docs/deferred-decisions.md` 329 行 | seal を付けずに pool を縮めるクライアントが来たときだけ要る。ZeikOS の ftruncate は 1 回きりなので、今は縮める場面が起きない | 新しい課題の候補(シグナルの配送) |
| 外からプロセスを止める(kill と SIGTERM の既定の動き) | S N T Z | 無い | 62 番(kill)が無い。`sys_tkill` 5379 行は自分だけ | 今は、前面の子を Ctrl+C で止める形だけ(`kernel/src/input.rs`) | 新しい課題の候補 |

### スレッド

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| clone(0x7d0f00。CLONE_DETACHED を含む)、prctl(PR_SET_NAME)、gettid、スレッドごとのシグナルマスクと代替スタック | S N | 一部 | `clone_from_ring3`・`classify_clone_flags`(M3a) | musl の pthread_create の形の clone と gettid は有る。スレッドはシステム全体で 4 本まで。prctl は無い(ENOSYS。Rust はスレッドの名前の失敗を無視する見込み) | スレッド |
| futex(FUTEX_WAIT_BITSET_PRIVATE。上のスレッドが待つ) | S N | 一部 | `sys_futex`(M3a) | 本当に待つようになった。期限つきの待ちは ENOSYS | スレッド |
| futex(FUTEX_WAKE_PRIVATE。wayland-client などの notify) | S N T Z | 一部 | `sys_futex` 5559 行 | いつも 0 を返す。スレッドが 1 本なら、これで足りる | futex の WAKE |

### 入出力の装置

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| /dev/fb0 の open・ioctl(FBIOGET_VSCREENINFO・FSCREENINFO)・mmap | S(fbdev の構成) | 有る | `open_screen_from_ring3` 1531 行、`screen_ioctl_from_ring3` 1605 行、`mmap_screen_from_ring3` 1698 行 | 前面のプロセスだけが開ける | — |
| キーボードとポインター | S | 一部 | ZeikOS だけの `SYS_OPEN_INPUT`(`open_input_from_ring3` 1437 行) | PS/2 のキーボードだけ(Linux の input_event)。/dev/input/event* のパスとマウスは無い。Seinas の側も、seinas-standalone はまだ装置から入力を読まない | 新しい課題の候補(両方の側) |

### 数の要約

使う構成ごとの、「無い」と「一部」の行の数(上の表の 44 行を、使う構成ごとに数える。fbdev と入力の装置の行は、seinas-standalone に入れる):

| 構成 | 無い | 一部 |
| --- | --- | --- |
| seinas-standalone | 13 | 14 |
| seinas(入れ子) | 15 | 14 |
| seinas-taskbar | 8 | 9 |
| zeyes-min | 5 | 9 |

## fd の種類

ふだんの動き(コンポジタに、タスクバーと zeyes-min と入れ子の seinas がつながったとき)の、プロセスごとの fd。標準入出力は除く。

| fd の種類 | seinas-standalone | seinas(入れ子) | seinas-taskbar | zeyes-min |
| --- | --- | --- | --- | --- |
| AF_UNIX の listen(ふだん・シェルの部品・IME) | 3(`--no-ime-socket` では 2) | 3 | — | — |
| AF_UNIX の接続 | クライアントの数(この例で 3) | 親へ 1 + 子の数 | 1 | 1 |
| ロックファイル(`<ソケット>.lock`。開いたまま) | 3(`--no-ime-socket` では 2) | 3 | — | — |
| memfd(keymap と、クライアントから受け取った pool) | 1 + pool の数(IME がつながると、virtual-keyboard の keymap の 1 と、候補ウィンドウの pool が増える) | keymap 2 + 自分の pool 1 | pool 1 | pool 1 |
| epoll | 3(calloop、wayland-backend、その複製) | 3 | 1 | 1 |
| eventfd | 1 | 1 | 1 | 1 |
| timerfd | 1 | 1 | 1 | 1 |
| 普通のファイル(フォント、xkb) | 読んだら閉じる(フォントはマップしたまま) | 同じ | 同じ(フォント) | — |
| /dev/fb0 | fbdev の構成で 1 | — | — | — |

ZeikOS の 1 プロセスの fd は 32 まで(標準入出力を含む。zeikos の ADR-0091。2026-10-23 に 16 から上げた)です。2026-10-21 に ZeikOS の上で、クライアント 0 のコンポジタ(seinas-standalone、/dev/fb0)が開いた fd は 15 本でした(標準入出力 3、epoll 3、eventfd 1、timerfd 1、/dev/fb0 1、ロックファイル 3、listen 3。ZeikOS の側が測った値)。クライアント 1 つにつき、ソケットと pool の memfd で 2 本増えるので、タスクバーが 1 つつながると 17 本です。

`--no-ime-socket`(IME のためのソケットを開かない。`docs/standalone.md`)を付けると、IME の listen とロックファイルの 2 本が減ります。同じ数え方(/dev/fb0 を開く構成、標準入出力を含む)で、次のとおりです。

| コンポジタにつながるもの | 既定(ソケット 3 つ) | `--no-ime-socket`(ソケット 2 つ) |
| --- | --- | --- |
| クライアント 0 | 15 | 13 |
| タスクバー 1 つ | 17 | 15 |

- 2026-10-23 に Linux(WSL2)の上で、fake screen の seinas-standalone(/dev/fb0 を開かないので、1 本少ない)の `/proc/<pid>/fd` を数えて確かめました。既定はクライアント 0 で 14、タスクバー 1 つで 16、`--no-ime-socket` は 12 と 14 でした。2 つの違いは、IME の listen とロックファイルの 2 本だけです。タスクバーがつながって増えたのは、接続のソケット 1 本と pool の memfd 1 本でした。
- 32 までの残りは、`--no-ime-socket` でタスクバー 1 つのとき 17 本です。pool を 1 つ使うクライアントなら、あと 8 つつなげます。

## 2026-10-21 に合わせ直したこと

ZeikOS の側に、epoll・eventfd・timerfd、待たない形のソケット、memfd の封、CLOCK_REALTIME が入り、Seinas の origin/main(1ba9fd5)の seinas-standalone が、変更なしで ZeikOS の上で起動しました(クライアント 0。ZeikOS の側の確かめ)。上の表の行のほかに、次のことが分かっています。

- 相対のパスの open は、ZeikOS が断ります。コンポジタの既定のフォントの置き場は、作業中のディレクトリから見た `target/fonts` なので、ZeikOS の上では、起動のときにフォントを読めないという WARN が出ます(文字を描かないだけで動き続ける)。フォントを置くときは、`--fonts` か `SEINAS_FONTS` に絶対のパスを渡します。
- fd の複製:ZeikOS は、ソケットとパイプの端の複製で参照を数えていません(複製を閉じると端が閉じる)。2026-10-21 に、Linux の上で、musl の静的ビルドのコンポジタ(fake screen)とタスクバー(シェルの部品のソケットへつなぐ)を `strace -f` で取って調べました。
  - コンポジタ:複製するのは epoll の fd だけです(calloop が `fcntl(F_DUPFD_CLOEXEC)` で 1 回。ZeikOS の側で対応済み)。ソケットの fd は、dup・dup2・dup3・F_DUPFD・F_DUPFD_CLOEXEC のどれでも複製しません。パイプは作りません。
  - タスクバー:複製するのは、pool の memfd だけです(smithay-client-toolkit が `fcntl(F_DUPFD_CLOEXEC)` で 1 回)。ソケットは複製せず、パイプも作りません。memfd の複製で参照を数えるかは、ZeikOS の側に確かめてもらう点です。
- SCM_RIGHTS を続けて送る場面:ZeikOS は、向きごとに 1 つまでしか fd を待たせられません。
  - 上の記録(キーボードの無い構成。クライアントはタスクバー 1 つ)では、タスクバーからコンポジタへ、pool の memfd を 1 つ、1 回だけ送りました。コンポジタからクライアントへは、fd を送りません(fd を付けない送信は `sendto(MSG_DONTWAIT|MSG_NOSIGNAL)`)。今の C1S4 の構成では、続けて 2 つ送る場面はありません。
  - 続けて送りうる場面(コードを読んで分かったもの。どれも今の ZeikOS の構成では起きない):キーボードを持つ構成(`--test-input`)で、コンポジタが wl_keyboard を作るたびに keymap の fd を送るとき(1 つのクライアントが wl_keyboard を続けて 2 つ作ると、同じ向きに 2 つ)。IME の virtual-keyboard のキーで、Smithay がフォーカスのあるクライアントへ keymap を送り直すとき。クライアントが pool を続けて 2 つ作るとき(今のタスクバー・壁紙・zeyes-min は、それぞれ 1 つ)。

## 置くファイル

| ファイル | 大きさ | 使う構成 | 無いとき |
| --- | --- | --- | --- |
| フォント 3 本(`BIZUDPGothic-Regular.ttf`、`BIZUDGothic-Regular.ttf`、`unifont_jp-18.0.01.otf`。置き場は `/usr/share/seinas/fonts` の案) | 4,669,688・4,667,380・5,250,304 バイト(合わせて 14,587,372) | S N T(BIZUDGothic は端末を作るまで読まない) | 文字を描かずに動く(ログに出す) |
| xkb のファイル 29 個(`/usr/share/X11/xkb` の rules・keycodes・types・compat・symbols の jp106 に要るもの) | 合わせて 213,688 バイト | S(N は、親の keymap が届く前に読む) | キーボードの無い構成として動く(ログに出す) |
| 設定のファイル(`$XDG_CONFIG_HOME/seinas/fonts.conf`) | 数百バイト | S N T | 既定の設定で動く |
| ロックファイルとソケット(`$XDG_RUNTIME_DIR/<名前>`、`<名前>.lock`、`<名前>-shell`、`<名前>-shell.lock`、`<名前>-ime`、`<名前>-ime.lock`) | ロックファイルは 0 バイト | S N が作る | 作れなければ、コンポジタが起動しない |

## スレッドを使う所

| 所 | 構成 | いつ | 無いとき |
| --- | --- | --- | --- |
| Smithay の shm のクリーンアップのスレッド(「Shm dropping th…」) | S N | 最初のクライアントが切れて、その pool を手放すとき | panic でコンポジタが落ちる(つないで描くまでは動く) |
| テスト用の入力インターフェースのスレッド(`--test-input`) | S | 起動のとき | ZeikOS では使わない |

ほかの構成(タスクバー、zeyes-min)は、スレッドを作りません。

## 3〜4 本のプロセスを同時に動かすのに要るもの

コンポジタ(seinas-standalone)、タスクバー、クライアント(zeyes-min)と、これから作るシェルの部品(壁紙など)を同時に動かす形です。

- 起こし方:コンポジタを先に起こし、ソケットができてから、シェルの部品とクライアントを起こします。コンポジタは、ほかのプロセスを起こしません(ADR 0002)。起こすのは、init か shell に当たるものです。
  - ZeikOS の今:fork・execve・wait4・kill が無く、ZeikOS だけの `SYS_SPAWN`(親は子が終わるまで待つ)と `SYS_SPAWN_DETACHED`(1 つだけ並べて動かす)で起こします。Ring 3 のタスクは 2 つまで(`kernel/src/arch/x86_64/ring3.rs` 235 行の `USER_TASK_SLOTS`=2)で、並べて動かせる子は 1 つです(`kernel/src/userland.rs` の `start_detached` 3335 行)。shell(`kernel/userland/zash.rs`)は、`a | b` の左だけを並べて動かし、`&` はありません。今は、コンポジタとクライアントの 2 本(`compd`・`compc` の形)までです。→ 新しい課題の候補
- 環境変数:
  - コンポジタ:`XDG_RUNTIME_DIR`(ソケットとロックファイルの置き場)。`--socket` でパスを直に渡してもよい。設定とフォントの置き場のために `XDG_CONFIG_HOME` か `HOME`、`SEINAS_FONTS`(か `--fonts`)。
  - シェルの部品(タスクバーなど):`WAYLAND_DISPLAY=<名前>-shell`(シェルの部品のためのソケット)と `XDG_RUNTIME_DIR`。
  - IME:`WAYLAND_DISPLAY=<名前>-ime`(IME のためのソケット)と `XDG_RUNTIME_DIR`。
  - クライアント:`WAYLAND_DISPLAY=<名前>` と `XDG_RUNTIME_DIR`。
  - ZeikOS の今:spawn に envp を渡せます(8 個・1,024 バイトまで。`common/src/env.rs` 22 行)。既定は `/etc/environment`(`TERM`、`PATH`、`HOME=/root`)で、zash に `export` があります。4 つの変数なら収まります。
- ソケットの置き場:`XDG_RUNTIME_DIR` は、書けるディレクトリが要ります。ZeikOS には `/run` が無く、`/tmp` は書けます。ただし、bind はファイルを作らないので、wayland-server の「ソケットのパスが無いことを stat で確かめる」「ロックファイルを O_RDWR\|O_CREAT\|O_TRUNC で作る」が通るかは、「flock とパスへの bind」のまとまりしだいです。
- ソケットと共有メモリーの数:listen 2、接続 3、memfd はクライアントごとに 1 つ以上と keymap の 1 つ(上の表の「新しい課題の候補」)。
- 止め方:外からプロセスを止める手段(kill)が無いので、今は前面の子を Ctrl+C で止めるだけです。

## まとまりごとの一覧

「無い」「一部」を、ZeikOS の側の機能のまとまりで分けたものです。それぞれが ZeikOS のどの課題に当たるかは、この文書には書かず、Jira(この文書を作った課題のコメント)を正とします(課題は閉じたり分かれたりするので、文書に書いたキーは古くなるため)。

| まとまり | 当てはまる「無い」「一部」 |
| --- | --- |
| epoll・eventfd・timerfd・ppoll | ppoll(と poll の POLLIN\|POLLERR)。epoll(入れ子と EPOLLONESHOT を含む)・eventfd2・timerfd は、2026-10-21 の時点で入った |
| AF_UNIX ソケットのフラグ・accept4・SCM_RIGHTS | SCM_RIGHTS の制限(1 通に 1 つ、memfd だけ、向きごとに 1 つ)。SOCK_CLOEXEC・SOCK_NONBLOCK、accept4 と EAGAIN、FIONBIO、MSG_DONTWAIT・MSG_NOSIGNAL・MSG_CMSG_CLOEXEC は、2026-10-21 の時点で入った |
| futex の WAKE | futex の WAKE(いまは 0 を返すだけ。スレッドを使うまでは足りる) |
| flock とパスへの bind | 2026-10-21 の時点で入った(flock、ロックファイルの O_RDWR\|O_CREAT\|O_TRUNC。bind は inode を作らない形で、wayland-server の形が通る。zeikos の ADR-0087) |
| 4 MiB を越えるファイル | 4 MiB を越えるファイル(フォント 3 本) |
| getrandom のフラグ 0 | getrandom のフラグ 0(読んだ限りでは、もう動く形。確かめて閉じられるかを ZeikOS の側で見てもらう) |
| スレッド | clone(スレッド)、futex の WAIT、prctl(PR_SET_NAME)、gettid、set_tid_address の終わるときの消去 |

### 新しい課題の候補(上のまとまりに入らないもの)

1. 共有メモリーの量:memfd の数(システムで 2 つ)と大きさ(32 KiB)、ftruncate を 2 回以上(伸ばす)、shm のマップの munmap・mprotect。
2. memfd への write と lseek(コンポジタが keymap を書く)。
3. ファイルの mmap(PROT_READ、MAP_SHARED。フォントと xkb。無くてもヒープへ読んで動くが、メモリーが増える)。
4. AF_UNIX の数(listen 1・接続 2 を、listen 2・接続 4 以上に)。2026-10-21 の時点で、listen 4・接続 8 になった。
5. 3〜4 本のプロセスを同時に動かすこと(Ring 3 のタスクの数、並べて動かす子の数、shell からの起こし方)。
6. 外からプロセスを止めること(kill と SIGTERM の既定の動き)。
7. シグナルの配送(SIGBUS の同期的な配送、rt_sigreturn。shm の縮小のときだけ要る)。
8. clock_gettime の CLOCK_REALTIME(タスクバーの時計)。2026-10-21 の時点で入った(zeikos の ADR-0090)。
9. faccessat2(xkbcommon)。
10. 入力の装置(マウスと、Linux の /dev/input/event* の形)。Seinas の側の、seinas-standalone が装置から入力を読む作りと合わせて決める。
11. pread64(入れ子の構成だけ。低い)。
12. IME の道:`$XDG_RUNTIME_DIR` の O_TMPFILE の一時ファイル(Smithay が IME の grab へ keymap を渡す形)と、クライアントから受け取った memfd でない fd の mmap。プロセスは IME の 1 本が増える(上の 5)。
13. 1 プロセスの fd の数。2026-10-23 の時点で、16 から 32 になった(zeikos の ADR-0091)。コンポジタは、タスクバーが 1 つつながって 17 本(`--no-ime-socket` で 15 本。上の「fd の種類」)。

## 壁紙

seinas-wallpaper(`docs/wallpaper.md`)が要るものは、zeyes-min と同じ形に、設定と画像のファイルの open(O_RDONLY\|O_LARGEFILE\|O_CLOEXEC)・fstat・read を足したもの(syscall は 33 種類)です。スレッドは作らず、fd は AF_UNIX の接続・memfd・epoll・eventfd・timerfd が 1 つずつです。違いは、画像が 4 MiB を越えると「4 MiB を越えるファイル」のまとまりが要ることと、出力の大きさの pool(800x600 で 1,920,000 バイト。上の「共有メモリーの量」の候補)が要ることです。syscall・fd・メモリーの記録は、`docs/m3-syscalls.md` の「変更ごとの差分の記録」にあります。

## IME

IME の道(`docs/ime.md`)で、コンポジタに増えるものは、上の表の「一時ファイル」と「クライアントから受け取った keymap の fd の mmap」の 2 行です。syscall の種類は増えません(41 種類のまま)。fd は、IME がつながると 3 つ増えます(IME の接続、候補ウィンドウの pool、virtual-keyboard の keymap の memfd)。IME は、シェルの部品と同じく、コンポジタの外のプログラムで、IME のためのソケット(`WAYLAND_DISPLAY=<名前>-ime`)へつなぎます。IME のソケットを足したので、コンポジタの fd は、IME がつながる前から 2 つ多くなります(listen のソケットとロックファイル)。ZeikOS のソケットの名前には、所有者と権限の検査が無いので、どのプロセスも IME のソケットにつなげます(`docs/ime.md` の「見せる global」)。プロセスは 1 本増えます。記録は `docs/m3-syscalls.md` の「変更ごとの差分の記録」にあります。
