# ADR 0002: タスクバー・メニュー・デスクトップを、コンポジタの外の別のプログラムにする

- 状態: 承認
- 日付: 2026-10-11

## 背景

モックアップでは、Seinas のデスクトップに、タスクバー(下に置き、上下左右へ動かせる。中央にトーチとアプリのアイコン、右に時計・トレイ・IME の表示)、トーチのメニュー(Windows のスタートメニューに当たる)、デスクトップ(壁紙と左のアイコン)がある。

これらをコンポジタの中に作り込むと、コンポジタが大きくなり、落ちたときにデスクトップ全体が止まる。また、ZeikOS の上へ持っていくときに、コンポジタに入るコードと syscall が増える。

Smithay 0.7.0 で使えるもの(2026-10-11 に、ソースで確かめた):

| プロトコル | Smithay 0.7.0 | 備考 |
| --- | --- | --- |
| wlr-layer-shell-unstable-v1(`zwlr_layer_shell_v1`) | あり(`wayland::shell::wlr_layer`、バージョン 4)。配置の計算も `desktop::LayerMap` にある(`desktop` feature。追加の依存なし) | global の公開先を絞る `new_with_filter` がある |
| ext-foreign-toplevel-list-v1 | あり(`wayland::foreign_toplevel_list`) | 一覧だけ(読むだけ)。操作は無い。`new_with_filter` がある |
| wlr-foreign-toplevel-management-unstable-v1(`zwlr_foreign_toplevel_manager_v1`、バージョン 3) | handler は無い | プロトコルの型は、すでに依存に入っている `wayland-protocols-wlr` 0.3.12 にある。Dispatch を自分で書く |
| クライアントの側(タスクバー) | smithay-client-toolkit 0.21.1 に layer-shell のクライアントがある | foreign-toplevel のクライアントは無い。`wayland-protocols-wlr` の client の型を使う |

## 決定

### 1. 別のプログラムにする

タスクバー、トーチのメニュー、デスクトップは、コンポジタの外の Wayland のクライアント(シェルの部品)として作る。コンポジタは、置き場所と一覧と操作のためのプロトコルを公開するだけにする。

### 2. 置き場所は layer-shell

- `zwlr_layer_shell_v1`(バージョン 4)を、Smithay の `WlrLayerShellState` で公開する。配置(anchor、margin、exclusive zone)の計算は、Smithay の `desktop::LayerMap` を使う。
- 描く順は、奥から background(デスクトップ)、bottom、通常のウィンドウ、top(タスクバー)、overlay(トーチのメニュー)。
- タスクバーは、どれか 1 辺に anchor し、その辺の向きの exclusive zone を持つ。左右に置いたときは、縦に並べる(並べ方はタスクバーのプログラムが決める)。
- キーボードの入力は、layer surface の `keyboard_interactivity` に従う。メニューは on_demand を使う(キーボードの経路の設計で詰める)。

### 3. ウィンドウの一覧と操作は、wlr-foreign-toplevel-management を主にし、ext-foreign-toplevel-list も公開する

- タスクバーが使うのは `zwlr_foreign_toplevel_manager_v1`(バージョン 3)。一覧(title、app_id、state、output)と操作(activate、最小化・最大化とその解除、close)が 1 つのプロトコルにそろっている。Smithay に handler が無いので、`seinas-frontend` の中に Dispatch を書く。
  - `set_fullscreen` は、全画面に対応するまで何もしない。`set_rectangle`(最小化のアニメーションの行き先)は受け取って覚えるだけにする。
- `ext_foreign_toplevel_list_v1` も、Smithay の実装で公開する。読むだけの一覧で、ほかのツール(スクリーンショットなど)が後で使う。ほぼ書く量が無いので、同時に入れる。
- 将来、ext の操作のプロトコルが標準になったら、そちらへ移ることを検討する。

### 4. 一覧と操作を見せるのは、シェルの部品だけにする

- コンポジタは、ふだんのソケット(`seinas-0` など)のほかに、シェルの部品のためのソケット(ふだんのソケットのパスに `-shell` を付けたもの。`--shell-socket PATH` で変えられる)を listen する。
- シェルのソケットからつないだクライアントにだけ、layer-shell と 2 つの foreign-toplevel の global を見せる(Smithay の `new_with_filter` と、クライアントごとのデータに付けた印で絞る)。ふだんのソケットのクライアントには、global そのものが見えない。
- 理由:ほかのウィンドウの一覧を読み、閉じたり最小化したりできるのは強い権限なので、ふつうのアプリには渡さない。印の付け方は、ソケットを分けるのがいちばん簡単で、プロセスを起こす仕組みも要らない(下の 5)。
- 採らなかった案:
  - すべてのクライアントに見せる(sway などの既定)。簡単だが、どのアプリでもほかのウィンドウを閉じられる。
  - コンポジタがシェルの部品を起こし、`WAYLAND_SOCKET`(socketpair の片方)で渡したクライアントだけに印を付ける。安全だが、コンポジタがプロセスを起こすことになる(下の 5)。
  - security-context プロトコル。サンドボックスのアプリを制限するためのもので、向きが逆になる。

### 5. シェルの部品は、起動の仕組みが起こす

- コンポジタは、プロセスを起こさない。Linux(WSLg)では、起動のスクリプト(または手で)がコンポジタを起こし、続けてタスクバーなどをシェルのソケットへつなぐ。ZeikOS では、起動の仕組み(init に当たるもの)が同じことをする。
- 理由:コンポジタに fork・exec(musl の posix_spawn は `clone(CLONE_VM|CLONE_VFORK)`、pipe、wait4、SIGCHLD を使う)を入れずに済み、ZeikOS の上へ持っていくときに要るものが増えない。
- シェルの部品が落ちても、コンポジタは動き続ける(ふだんのクライアントと同じ)。起こし直すのは、起動の仕組みの役目にする(今は起こし直さない)。
- アプリの起動(トーチのメニューから)は、メニューのプログラムが行う。ZeikOS の上でプロセスを起こせるようになってから入れる。

### 6. 使える範囲(usable area)

- 使える範囲は、画面から、layer surface の exclusive zone を除いた矩形(`LayerMap::non_exclusive_zone`)とする。
- 新しいウィンドウの置き場所(スロット)、最大化の大きさ、ドラッグで画面の外へ出せる範囲は、使える範囲を基準にする。タスクバーの下にタイトルバーが隠れないようにするため。
- クライアントに伝える大きさ(configure)も、使える範囲から求める。
- 使える範囲が変わったとき(タスクバーを動かした・消えた)は、最大化しているウィンドウの大きさを直し、そのほかのウィンドウは、タイトルバーが使える範囲に入るように位置を直す。
- 新しく書く決まりのコード(使える範囲の計算など)は、ADR 0001 に従い、Smithay の幾何の型を使わない。今ある違反 5(ウィンドウ管理の決まりが Smithay の幾何の型を使っている)は、layer-shell の受け口を作る最初に直すことを提案する。

### 7. 最小化・最大化

- タイトルバーの最大化・最小化・元に戻すのボタンは、タスクバーができてから入れる。
- 最小化したウィンドウは描かず、ポインターのヒットの対象から外す。戻すのは、タスクバーから(foreign-toplevel の activate か unset_minimized)。
- foreign-toplevel の manager を bind しているシェルの部品が 1 つも無いときは、最小化のボタンを出さない(戻す手段が無いため)。

### 8. タスクバーのプログラム

- 新しいバイナリの crate にする(名前は `seinas-taskbar` を提案)。smithay-client-toolkit(layer-shell)と、`wayland-protocols-wlr` の client の型(foreign-toplevel)を使い、文字は `seinas-text` で描き、wl_shm のバッファへ直接描く。
- 既存の GUI のツールキット(GTK、iced、egui など)は使わない。依存が大きく、musl の静的ビルドと ZeikOS の上へ持っていくのが難しく、GPU を前提にするものもあるため。ボタンや一覧のような部品が増えてきたら、自前の小さな部品の crate を作ることを検討する。
- 時計は、分が変わる時刻に起きる timerfd で描き直す。タイムゾーンのデータベースは読まない(ZeikOS に無いため)。時差の決め方は、タスクバーの課題で決める。

### 9. 今は決めないもの

- トーチのメニューとデスクトップのプログラム(C1S3 では作らない)。
- メニューに並べるアプリの決まり(Linux の .desktop ファイルの一部に当たる、名前・起動のコマンド・アイコンの小さな形式を提案する)。
- Windows キーに当たるキーでメニューを開くこと。コンポジタがキーを受けて、シェルの部品へ知らせる経路が要る。IME を通るキーボードの経路の設計で、一緒に決める。

### 10. ADR 0001 との整合

- layer-shell と foreign-toplevel のプロトコルの処理は、土台(Smithay と wayland-protocols)の範囲なので、`seinas-frontend` の中だけに置く。
- タスクバーの文字は `seinas-text` の公開 API だけで描く。`seinas-text` の公開 API は Seinas の型だけなので、タスクバーに部品(cosmic-text など)の型は出ない。
- smithay-client-toolkit と wayland-client は、タスクバーのバイナリ(composition root)だけで使う。

## ZeikOS の上へ持っていくときの前提(C1S4)

- プロセス:コンポジタ 1(スレッドはクライアントが切断した後に 1 本増える)、タスクバー 1、アプリ(zeyes など)。最小の構成で 3 プロセス。
- コンポジタの fd の増え方:シェルのソケット(listen 1、ロックファイル 1)が増える。ほかは今と同じ(クライアントごとに接続 1 と、マップしている pool の fd)。
- タスクバーの fd の見込み:接続 1、epoll 1、eventfd 1、timerfd 1〜2、pool の memfd 1、で 5〜6。フォントは読み込んだら閉じる。実際の数は、タスクバーを作るときに測って記録する。
- syscall:コンポジタは、今の M3 の記録(`docs/m3-syscalls.md`)と同じ種類で足りる見込み(ソケットが 1 つ増えるだけ)。タスクバーは zeyes-min と同じ種類に、時計のための `clock_gettime`(vDSO が無ければ syscall)が加わる見込み。プロセスを起こさないので、fork・exec の類は要らない。
- ZeikOS では、コンポジタ・タスクバー・アプリの少なくとも 3 プロセスを同時に起こす手段(シェルのバックグラウンド実行か、init に当たるもの)が要る。
- 既定のフォントのファイルは、1 ファイルが約 4.05 MiB を超える(BIZ UDPゴシック 4,669,688 バイト、GNU Unifont JP 5,250,304 バイト)。ZeikOS の ext2 で、2 段目の間接ブロックに対応している必要がある。
- ZeikOS には、まだ壁の時計(実際の時刻)が無い。タスクバーの時計は、時刻を取れないときの表示を持つ(表示の形は、タスクバーの課題で決める)。
- 機能を足すたびに strace の差分を取り、`docs/m3-syscalls.md` に記録する。

## 実装の刻み

| 順 | 内容 | 見込み |
| --- | --- | --- |
| 1 | layer-shell の受け口、シェルのソケットと global の絞り込み、使える範囲(違反 5 の直しを含む) | 約 400 行 |
| 2 | foreign-toplevel の受け口(wlr の Dispatch と ext の公開) | 約 400 行 |
| 3 | 最小のタスクバー(一覧と時計) | 約 600 行 |
| 4 | タイトルバーの最大化・最小化・元に戻す | 約 300 行 |

## 結果

- コンポジタは、置き場所・一覧・操作のプロトコルを公開するだけになり、デスクトップの見た目はシェルの部品で変えられる。
- シェルの部品が落ちても、コンポジタとアプリは動き続ける。
- 起動の仕組みが、コンポジタとシェルの部品を順に起こす必要がある。
- wlr-foreign-toplevel-management の Dispatch を、自分で持つ(保守の量が増える)。
