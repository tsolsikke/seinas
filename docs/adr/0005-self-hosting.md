# ADR 0005: セルフホスティングを長期のゴールにする(Seinas の側の条件)

- 状態: 承認
- 日付: 2026-10-15

## 背景

運用者の決定(2026-10-15)で、セルフホスティングを、ZeikOS と Seinas のロードマップのゴールの 1 つにした。ゴールは「ZeikOS の上で、ZeikOS と Seinas のソースを編集し、ビルドし、できたものに切り替えて動かせる」こと。長期の目標で、今のスプリントの優先順位は変えない。

正本は Confluence の「長期ゴール — セルフホスティング(ZeikOS・Seinas)」のページ(https://zeikos.atlassian.net/wiki/spaces/SCRUM/pages/5767169)。段階(S0〜S5)、ZeikOS の側の条件、今の計画への影響は、そちらに書いてある。zeikos のリポジトリにも、ZeikOS を再開したときに ADR を作る。できたら、ここにその参照を足す。この ADR には、Seinas の側の条件だけを短く書く。

## 決定

### 1. 段階と、達成とする所

- S4「ZeikOS の中で ZeikOS と Seinas をビルドできる」を、ゴールの達成とする。S5(中でビルドしたものに切り替えて動かし、中で全部の検査も回す)は、発展の目標。
- 段階の一覧と、それぞれの位置づけは、Confluence のページを見る。

### 2. ビルドの依存(proc-macro と build.rs)

- Seinas は Smithay を使うので、proc-macro と build.rs への依存は避けない(選べない)。依存を絞ることはせず、調べて記録する。
- 今の Seinas のビルドが使うもの(2026-10-15 に、Cargo.lock と `cargo metadata --locked --filter-platform` を読んで数えた。依存は変えていない):
  - ワークスペースの依存の数は 136(x86_64-unknown-linux-gnu と x86_64-unknown-linux-musl で同じ。dev-dependencies を入れても変わらない)。Seinas の crate そのものに build.rs は無い。
  - proc-macro のクレート(7):

    | クレート | バージョン | どこから(直に使うクレート) |
    | --- | --- | --- |
    | bytemuck_derive | 1.12.1 | bytemuck |
    | paste | 1.0.15 | pixman |
    | profiling-procmacros | 1.0.18 | profiling |
    | thiserror-impl | 1.0.69 | thiserror 1 |
    | thiserror-impl | 2.0.21 | thiserror 2 |
    | tracing-attributes | 0.1.31 | tracing |
    | wayland-scanner | 0.31.11 | wayland-server、wayland-client、wayland-protocols(-misc・-wlr・-experimental)、smithay-client-toolkit |

  - build.rs を持つクレート(23):cgmath 0.18.0、crc32fast 1.5.2、drm-fourcc 2.2.0、generic-array 0.14.7、getrandom 0.3.4、getrandom 0.4.3、libc 0.2.190、libm 0.2.16、num-traits 0.2.19、paste 1.0.15、pixman-sys 0.1.0、proc-macro2 1.0.107、quote 1.0.47、rustix 1.1.5、slotmap 1.1.1、smithay 0.7.0、smithay-client-toolkit 0.21.1、thiserror 1.0.69、thiserror 2.0.21、wayland-backend 0.3.17、wayland-client 0.31.15、wayland-sys 0.31.11、zerocopy 0.8.59。
  - 中で動かすには、rustc が proc-macro を読み込めること(共有ライブラリと dlopen)と、cargo が build.rs のプログラムを子のプロセスとして動かせることが要る(Confluence のページの S3)。
- 依存を変えたときは、この一覧を数え直す。

### 3. 将来の要件:Seinas を開発する環境として要るもの

中で Seinas を開発するために、Seinas の上に要るもの。どう作るかは、ここでは決めない(設計の論点は、運用者の決まりどおり GPT-6 に相談してから決める)。

- 端末のエミュレータ(Wayland のクライアント)
- コピーと貼り付け
- 等幅のフォント
- Kieli での日本語の入力
- エディタは、端末の中のもの(zi・vim など)で足りる。GUI のエディタは要らない。

### 4. ZeikOS への要求(今の Seinas から見えるもの)

- Seinas のコンポジタは、listen のソケットを 3 つ使う(ふだん・シェルの部品・IME)。ZeikOS の今の上限は 1 つ(`docs/zeikos-requirements.md` の「ソケットの数」の行)。
- ZeikOS には、利用者と権限の概念が無い(ソケットの名前に、所有者と権限の検査が無い)。Seinas の IME の権限は、ソケットを分けた役割の分け方だけに頼る(`docs/ime.md` の「見せる global」、ADR 0004 の 1.6)。

## 結果

- これからの Seinas の設計では、セルフホスティング(とくに S4)を妨げないかを確かめる。
- proc-macro と build.rs の一覧は、依存を変えたときに数え直す。
- 開発する環境として要るもの(3)は、Seinas の機能の候補として残る。
