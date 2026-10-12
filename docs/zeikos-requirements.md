# ZeikOS で Seinas を動かすのに要るもの

Seinas の構成(seinas・seinas-standalone・seinas-taskbar・zeyes-min)が ZeikOS の上で動くのに要るものを 1 つにまとめ、ZeikOS の今の対応と突き合わせた文書です(C1S4 の準備)。

- Seinas の側の値は、2026-10-11 に Linux(WSL2)の上で、musl の静的ビルドを `strace -f` で取ったものです(4 つを同時に動かし、seinas は seinas-standalone を親にして入れ子で動かした)。取り方と、変更ごとの差分の記録は `docs/m3-syscalls.md` にあります。こちらは、その記録から「要るもの」を引いた表です。
- ZeikOS の側は、隣の zeikos のリポジトリの main(9666a4cce94c6e886d3c83b3b46ed99bd3f727fb)のソースを読んで判断しました。ビルドも実行もしていません。根拠は、ファイルと関数(と行)で書きます。断りの無いファイルは `kernel/src/syscall.rs` です。syscall の番号から関数への表は `dispatch()`(771〜1128 行)で、表に無い番号は `-ENOSYS` を返します(1123 行)。
- 下の「まとまり」の欄は、ZeikOS の側の機能のまとまりの名前です。どの課題に当たるかは、Jira を正とします(下の「まとまりごとの一覧」)。

状態の書き方:**有る**(要る形で動く)、**一部**(要る旗や場合の一部が無い。足りないものを備考に書く)、**無い**(番号が無いか、要る形がすべて断られる)、**不明**。

## 構成と、ZeikOS の上での役目

| 構成 | 役目 | ZeikOS の上で |
| --- | --- | --- |
| seinas-standalone | コンポジタ(親の Wayland を持たない構成。fbdev か fake screen へ描く) | M3 で動かす本体 |
| seinas | コンポジタ(入れ子の構成。親の Wayland のクライアントとして窓を出す) | 親の Wayland が無いので、ZeikOS では動かさない見込み。比べるために載せる |
| seinas-taskbar | タスクバー(シェルの部品。layer-shell と foreign-toplevel のクライアント) | コンポジタと同時に動かす |
| seinas-wallpaper | 壁紙(シェルの部品。layer-shell のクライアント) | コンポジタと同時に動かす(下の「壁紙」) |
| zeyes-min | 試しのクライアント(xdg_toplevel の窓を 1 つ出す) | コンポジタと同時に動かす |

## 要るものと、ZeikOS の対応

「使う構成」の欄:S は seinas-standalone、N は seinas(入れ子)、T は seinas-taskbar、Z は zeyes-min。

### 起動とメモリー

| 要るもの(syscall と旗) | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| 静的 PIE の読み込み(execve に当たるもの)、arch_prctl(ARCH_SET_FS)、brk、exit_group | S N T Z | 有る | `SYS_SPAWN`(`spawn_from_ring3` 3624 行)、`sys_arch_prctl_body` 5891 行、`sys_brk` 4469 行 | execve の番号は無く、ZeikOS だけの spawn で読み込む。実行ファイルは 16 MiB まで、ユーザーのスタックは 16 KiB | — |
| set_tid_address | S N T Z | 一部 | `sys_set_tid_address` 5534 行 | アドレスを覚えるだけで、終わるときの消去と futex の WAKE はしない(スレッドを使うときに要る) | スレッド |
| mmap(PROT_NONE・PROT_READ\|PROT_WRITE、MAP_PRIVATE\|MAP_ANONYMOUS、MAP_STACK、MAP_FIXED)、munmap・mprotect(無名のもの) | S N T Z | 有る | `mmap_from_ring3` 2939 行 → `mmap_anonymous_from_ring3` 2312 行、`munmap_from_ring3` 2464 行、`mprotect_from_ring3` 2528 行 | 1 回 16 MiB まで、マップは 64 個まで。ページは先に割り当てる。知らない旗は断らない | — |
| ファイルの mmap(PROT_READ、MAP_SHARED。フォントと xkb のファイル) | S N T | 無い | 2993 行の `shm_of(fd)` が、memfd と画面のほかは EBADF | フォントは、mmap できなければヒープへ読む(Seinas の側で対応済み。プロセスごとに約 14 MB 増える)。xkbcommon も、mmap できないときは読み込みに切り替わる作り(ZeikOS の上では未確認)。zeikos の `docs/deferred-decisions.md` 323 行で後回し | 新しい課題の候補 |
| poll(events=0、timeout 0。Rust の起動のとき) | S N T Z | 有る | `poll_from_ring3` 1857 行(1901 行) | — | — |
| getrandom(GRND_INSECURE と、旗 0) | S N T | 有る | `sys_getrandom` 5659 行 | 1 回 256 バイトまで | getrandom の旗 0(旗 0 は動く形になっている) |
| clock_gettime(CLOCK_MONOTONIC) | S N T Z | 有る | `sys_clock_gettime` 5126 行 | 分解能 10 ms。vDSO は無い(AT_SYSINFO_EHDR を置かない。`kernel/src/abi/linux/initial_stack.rs` 75 行)ので、musl は syscall で呼ぶ | — |
| clock_gettime(CLOCK_REALTIME。タスクバーの時計) | T | 無い | `sys_clock_gettime` 5132 行(MONOTONIC のほかは EINVAL) | 取れないとき、タスクバーは「--:--」を出して動き続ける | 新しい課題の候補 |

### 共有メモリー(Wayland の shm)

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| memfd_create(MFD_CLOEXEC\|MFD_ALLOW_SEALING) | S N T Z | 有る | `memfd_create_from_ring3` 2893 行 | 名前と旗は読まない | — |
| ftruncate(memfd。pool の大きさ。SCTK は伸ばす) | N T Z | 一部 | `ftruncate_from_ring3` 2910 行、`kernel/src/shm.rs` の `set_size` | 1 回だけ(2 回目は EINVAL)、32 KiB まで。zeyes-min の pool は 614,400 バイト、タスクバーは 256,000 バイト、800x600 の窓は 1.9 MB で、どれも越える | 新しい課題の候補 |
| 2 つのプロセスの間の MAP_SHARED(PROT_READ\|PROT_WRITE、memfd) | S N T Z | 一部 | 2993〜3067 行、`kernel/src/shm.rs`(ADR-0065) | 共有は動く。memfd はシステム全体で 2 つまで(`MAX_SHM`、`shm.rs` 41 行)、1 つ 8 ページ(`MAX_SHM_PAGES`、45 行)。クライアントごとに 1 つ以上と、コンポジタの keymap の分が要る | 新しい課題の候補 |
| munmap・mprotect(shm のマップ) | S N T Z | 一部 | `munmap_from_ring3` 2464 行(shm・画面は EINVAL) | コンポジタは、クライアントが pool を手放すと munmap する | 新しい課題の候補 |
| fcntl(F_ADD_SEALS。F_SEAL_SEAL\|F_SEAL_SHRINK と、keymap の F_SEAL_SEAL\|F_SEAL_SHRINK\|F_SEAL_GROW\|F_SEAL_WRITE) | S N T Z | 一部 | `sys_fcntl` 5789 行 | 0 を返すが、封は覚えず、効かない。Seinas が動くのには困らない | — |
| write と lseek(SEEK_SET。memfd。コンポジタが keymap を書く) | S N | 無い | `sys_write` 6520〜6601 行(memfd は EBADF)、`sys_lseek` 4677 行(何もしない) | keymap を書けないと、キーボードの keymap をクライアントへ渡せない | 新しい課題の候補 |
| pread64(親の keymap を読む) | N | 無い | 17 番が無い | 入れ子の構成だけ | 新しい課題の候補(低い) |

### イベントループ

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| epoll_create1(EPOLL_CLOEXEC)、epoll_ctl(ADD・MOD・DEL、EPOLLIN\|EPOLLPRI\|EPOLLERR\|EPOLLHUP、EPOLLONESHOT)、epoll_pwait(timeout -1・0・正の値)、epoll の入れ子 | S N T Z | 無い | 291・233・281 番が無い | calloop と wayland-backend が使う。入れ子は wayland-backend の epoll を calloop の epoll に入れる形 | epoll・eventfd・timerfd・ppoll |
| eventfd2(0、EFD_CLOEXEC\|EFD_NONBLOCK) | S N T Z | 無い | 290 番が無い | calloop の起こし方 | epoll・eventfd・timerfd・ppoll |
| timerfd_create(CLOCK_MONOTONIC、TFD_CLOEXEC\|TFD_NONBLOCK)、timerfd_settime(相対、1 回きり、0 で止める) | S N T Z | 無い | 283・286 番が無い | — | epoll・eventfd・timerfd・ppoll |
| ppoll(fd 1 つ、POLLIN\|POLLERR、timeout と sigmask は NULL) | N T Z | 無い | 271 番が無い。poll でも POLLIN\|POLLERR は EINVAL | wayland-client が使う | epoll・eventfd・timerfd・ppoll |
| fcntl(F_SETFD の FD_CLOEXEC、F_DUPFD_CLOEXEC) | S N T Z | 有る | `sys_fcntl` 5789 行 | CLOEXEC は覚えない(exec が無いので困らない) | — |

### AF_UNIX ソケット(Wayland の接続)

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| socket(AF_UNIX、SOCK_STREAM\|SOCK_CLOEXEC) | S N T Z | 一部 | `socket_from_ring3` 1399 行(1403 行) | SOCK_STREAM だけを受け、SOCK_CLOEXEC・SOCK_NONBLOCK が付くと EINVAL。wayland-rs はいつも SOCK_CLOEXEC を付けるので、今は失敗する | AF_UNIX ソケットの旗・accept4・SCM_RIGHTS |
| bind(ファイルシステムの上のパス)、ソケットのパスの stat | S N | 一部 | `bind_from_ring3` 2004 行、`read_socket_name` 1349 行 | 名前はカーネルの表に入るだけで、ファイルシステムに inode を作らない(stat で見えない)。名前は 31 バイトまで | flock とパスへの bind |
| listen(backlog -1) | S N | 有る | `listen_from_ring3` 2043 行 | backlog は見ない | — |
| accept4(SOCK_CLOEXEC)と、待っている接続が無いときの EAGAIN、ioctl(FIONBIO) | S N | 無い | 288 番が無い(accept は `accept_from_ring3` 2065 行で、いつも待つ)、`sys_ioctl` 6253 行(端末のほかは ENOTTY) | wayland-server は non-blocking の accept4 を使い、EAGAIN を「もう無い」という意味に使う | AF_UNIX ソケットの旗・accept4・SCM_RIGHTS |
| connect | N T Z | 有る | `connect_from_ring3` 2122 行 | — | — |
| sendto・sendmsg(MSG_DONTWAIT\|MSG_NOSIGNAL)、recvmsg(MSG_DONTWAIT\|MSG_CMSG_CLOEXEC)、SCM_RIGHTS での memfd の受け渡し | S N T Z | 一部 | `sendmsg_from_ring3` 3201 行、`recvmsg_from_ring3` 3251 行、`read_msghdr` 3112 行、`write_to_socket` 2235 行 | 旗を読まず、いつも待つ(recvmsg は旗の引数を読まない。送る側も、いっぱいなら待つ)。iovlen は 1、fd は 1 通に 1 つで memfd だけ | AF_UNIX ソケットの旗・accept4・SCM_RIGHTS |
| 相手が切ったとき recvmsg が 0 を返すこと | S N T Z | 有る | `read_from_socket` 2168 行 | — | — |
| ソケットの数:コンポジタは listen を 2 つ(ふだんのソケットと、シェルの部品のソケット)、接続はクライアントの数だけ | S N | 一部 | `kernel/src/socket.rs` 45・48 行(`MAX_LISTENERS`=1、`MAX_CONNECTIONS`=2。システム全体で、受け付け待ちも含む) | コンポジタ・タスクバー・クライアントの 3 本でも、listen 2、接続 2 が要る(シェルの部品が増えれば、その数だけ増える) | 新しい課題の候補 |

### ファイル

| 要るもの | 使う構成 | ZeikOS | 根拠 | 備考 | まとまり |
| --- | --- | --- | --- | --- | --- |
| open(O_RDONLY\|O_LARGEFILE\|O_CLOEXEC)、stat、fstat、read、close | S N T | 有る | `sys_open` 4869 行、`sys_stat` 6164 行、`sys_fstat` 5828 行、`sys_read` 4038 行 | — | — |
| ロックファイル:open(O_RDWR\|O_CREAT\|O_TRUNC)と flock(LOCK_EX\|LOCK_NB) | S N | 無い | `sys_open` 4892〜4897 行(O_RDWR は EROFS)、73 番(flock)が無い | wayland-server が、ソケットのパスに `.lock` を付けたファイルで、同じ名前のコンポジタが 2 つ動かないようにする | flock とパスへの bind |
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
| clone(0x7d0f00。CLONE_DETACHED を含む)、prctl(PR_SET_NAME)、gettid、スレッドごとのシグナルマスクと代替スタック | S N | 無い | 56 番(clone)・157 番(prctl)が無い。gettid は 881 行でいつも 1 | Smithay が、最初のクライアントが切れたときに shm のクリーンアップのスレッド(名前は「Shm dropping th…」)を作る。作れないと panic する | スレッド |
| futex(FUTEX_WAIT_BITSET_PRIVATE。上のスレッドが待つ) | S N | 一部 | `sys_futex` 5559 行 | 値が合うと、待たずにプロセスを止める(終了 137) | スレッド |
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
| AF_UNIX の listen(ふだん・シェルの部品) | 2 | 2 | — | — |
| AF_UNIX の接続 | クライアントの数(この例で 3) | 親へ 1 + 子の数 | 1 | 1 |
| ロックファイル(`<ソケット>.lock`。開いたまま) | 2 | 2 | — | — |
| memfd(keymap と、クライアントから受け取った pool) | 1 + pool の数(IME がつながると、virtual-keyboard の keymap の 1 と、候補の窓の pool が増える) | keymap 2 + 自分の pool 1 | pool 1 | pool 1 |
| epoll | 3(calloop、wayland-backend、その複製) | 3 | 1 | 1 |
| eventfd | 1 | 1 | 1 | 1 |
| timerfd | 1 | 1 | 1 | 1 |
| 普通のファイル(フォント、xkb) | 読んだら閉じる(フォントはマップしたまま) | 同じ | 同じ(フォント) | — |
| /dev/fb0 | fbdev の構成で 1 | — | — | — |

ZeikOS の 1 プロセスの fd は 16 までで、上の数は収まります。

## 置くファイル

| ファイル | 大きさ | 使う構成 | 無いとき |
| --- | --- | --- | --- |
| フォント 3 本(`BIZUDPGothic-Regular.ttf`、`BIZUDGothic-Regular.ttf`、`unifont_jp-18.0.01.otf`。置き場は `/usr/share/seinas/fonts` の案) | 4,669,688・4,667,380・5,250,304 バイト(合わせて 14,587,372) | S N T(BIZUDGothic は端末を作るまで読まない) | 文字を描かずに動く(ログに出す) |
| xkb のファイル 29 個(`/usr/share/X11/xkb` の rules・keycodes・types・compat・symbols の jp106 に要るもの) | 合わせて 213,688 バイト | S(N は、親の keymap が届く前に読む) | キーボードの無い構成として動く(ログに出す) |
| 設定のファイル(`$XDG_CONFIG_HOME/seinas/fonts.conf`) | 数百バイト | S N T | 既定の設定で動く |
| ロックファイルとソケット(`$XDG_RUNTIME_DIR/<名前>`、`<名前>.lock`、`<名前>-shell`、`<名前>-shell.lock`) | ロックファイルは 0 バイト | S N が作る | 作れなければ、コンポジタが起動しない |

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
  - クライアント:`WAYLAND_DISPLAY=<名前>` と `XDG_RUNTIME_DIR`。
  - ZeikOS の今:spawn に envp を渡せます(8 個・1,024 バイトまで。`common/src/env.rs` 22 行)。既定は `/etc/environment`(`TERM`、`PATH`、`HOME=/root`)で、zash に `export` があります。4 つの変数なら収まります。
- ソケットの置き場:`XDG_RUNTIME_DIR` は、書けるディレクトリが要ります。ZeikOS には `/run` が無く、`/tmp` は書けます。ただし、bind はファイルを作らないので、wayland-server の「ソケットのパスが無いことを stat で確かめる」「ロックファイルを O_RDWR\|O_CREAT\|O_TRUNC で作る」が通るかは、「flock とパスへの bind」のまとまりしだいです。
- ソケットと共有メモリーの数:listen 2、接続 3、memfd はクライアントごとに 1 つ以上と keymap の 1 つ(上の表の「新しい課題の候補」)。
- 止め方:外からプロセスを止める手段(kill)が無いので、今は前面の子を Ctrl+C で止めるだけです。

## まとまりごとの一覧

「無い」「一部」を、ZeikOS の側の機能のまとまりで分けたものです。それぞれが ZeikOS のどの課題に当たるかは、この文書には書かず、Jira(この文書を作った課題のコメント)を正とします(課題は閉じたり分かれたりするので、文書に書いたキーは古くなるため)。

| まとまり | 当てはまる「無い」「一部」 |
| --- | --- |
| epoll・eventfd・timerfd・ppoll | epoll(入れ子と EPOLLONESHOT を含む)、eventfd2、timerfd、ppoll(と poll の POLLIN\|POLLERR) |
| AF_UNIX ソケットの旗・accept4・SCM_RIGHTS | socket の SOCK_CLOEXEC、accept4 と EAGAIN、FIONBIO、MSG_DONTWAIT・MSG_NOSIGNAL・MSG_CMSG_CLOEXEC、SCM_RIGHTS の制限(1 通に 1 つ、memfd だけ) |
| futex の WAKE | futex の WAKE(いまは 0 を返すだけ。スレッドを使うまでは足りる) |
| flock とパスへの bind | flock、ロックファイルの O_RDWR\|O_CREAT\|O_TRUNC、ファイルシステムの上のパスへの bind(inode を作り、stat で見えるようにする) |
| 4 MiB を越えるファイル | 4 MiB を越えるファイル(フォント 3 本) |
| getrandom の旗 0 | getrandom の旗 0(読んだ限りでは、もう動く形。確かめて閉じられるかを ZeikOS の側で見てもらう) |
| スレッド | clone(スレッド)、futex の WAIT、prctl(PR_SET_NAME)、gettid、set_tid_address の終わるときの消去 |

### 新しい課題の候補(上のまとまりに入らないもの)

1. 共有メモリーの量:memfd の数(システムで 2 つ)と大きさ(32 KiB)、ftruncate を 2 回以上(伸ばす)、shm のマップの munmap・mprotect。
2. memfd への write と lseek(コンポジタが keymap を書く)。
3. ファイルの mmap(PROT_READ、MAP_SHARED。フォントと xkb。無くてもヒープへ読んで動くが、メモリーが増える)。
4. AF_UNIX の数(listen 1・接続 2 を、listen 2・接続 4 以上に)。
5. 3〜4 本のプロセスを同時に動かすこと(Ring 3 のタスクの数、並べて動かす子の数、shell からの起こし方)。
6. 外からプロセスを止めること(kill と SIGTERM の既定の動き)。
7. シグナルの配送(SIGBUS の同期的な配送、rt_sigreturn。shm の縮小のときだけ要る)。
8. clock_gettime の CLOCK_REALTIME(タスクバーの時計)。
9. faccessat2(xkbcommon)。
10. 入力の装置(マウスと、Linux の /dev/input/event* の形)。Seinas の側の、seinas-standalone が装置から入力を読む作りと合わせて決める。
11. pread64(入れ子の構成だけ。低い)。
12. IME の道:`$XDG_RUNTIME_DIR` の O_TMPFILE の一時ファイル(Smithay が IME の grab へ keymap を渡す形)と、クライアントから受け取った memfd でない fd の mmap。プロセスは IME の 1 本が増える(上の 5)。

## 壁紙

seinas-wallpaper(`docs/wallpaper.md`)が要るものは、zeyes-min と同じ形に、設定と画像のファイルの open(O_RDONLY\|O_LARGEFILE\|O_CLOEXEC)・fstat・read を足したもの(syscall は 33 種類)です。スレッドは作らず、fd は AF_UNIX の接続・memfd・epoll・eventfd・timerfd が 1 つずつです。違いは、画像が 4 MiB を越えると「4 MiB を越えるファイル」のまとまりが要ることと、出力の大きさの pool(800x600 で 1,920,000 バイト。上の「共有メモリーの量」の候補)が要ることです。syscall・fd・メモリーの記録は、`docs/m3-syscalls.md` の「変更ごとの差分の記録」にあります。

## IME

IME の道(`docs/ime.md`)で、コンポジタに増えるものは、上の表の「一時ファイル」と「クライアントから受け取った keymap の fd の mmap」の 2 行です。syscall の種類は増えません(41 種類のまま)。fd は、IME がつながると 3 つ増えます(IME の接続、候補の窓の pool、virtual-keyboard の keymap の memfd)。IME は、シェルの部品と同じく、コンポジタの外のプログラムで、シェルの部品のソケット(`WAYLAND_DISPLAY=<名前>-shell`)へつなぎます。プロセスは 1 本増えます。記録は `docs/m3-syscalls.md` の「変更ごとの差分の記録」にあります。
