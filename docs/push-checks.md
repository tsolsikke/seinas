# push の前の確かめ

push の前に行う確かめを、変更の種類に応じた段に分けます(運用者の決定。2026-10-12)。手順は `tools/pre-push.sh` にまとめてあり、push の後の CI の確かめは `tools/ci-wait.sh` で行います。

## 守るもの

- テストが落ちたまま push しない。
- 正解データ(golden)の決まり(`docs/golden.md`):作り直しとコードの変更を同じコミットに混ぜない、見た目を変えたら直後のコミットで作り直して一緒に push する、作り直しのコミットの本文に差分の要約を書く。
- push の後は、CI の成功を必ず確かめる。落ちたらすぐ直し、直せないときは revert してから報告する。
- `CLAUDE.md` の決まり。

## これまでの手順と、所要時間の目安

2026-10-12 に手元(WSL2)で測った値です。ビルドの成果物が残っている状態で、`crates/seinas-frontend/src/lib.rs` を 1 つ触ったときの時間です。

| 手順 | どこに書いてあったか | 時間の目安 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | CI | 0.4 秒 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | CI | 2 秒 |
| `cargo test --workspace --locked`(golden を含む) | CI | 29 秒(golden だけなら 1 秒) |
| 古い言い方の grep(`docs/terminology.md`) | 運用者の指示 | 1 秒 |
| `python3 tools/third-party.py --check` | CI | 0.4 秒 |
| `tools/check-deps.sh` と `tools/check-no-wayland.sh` | CI、`tools/build-musl.sh` | 数秒 |
| `tools/build-musl.sh`(musl の静的ビルドと配布物) | CI、`docs/musl-static.md` | 13 秒(依存を変えたときは数分) |
| musl のテスト(`cargo test --target x86_64-unknown-linux-musl`) | CI、`docs/musl-static.md` | 26 秒(依存を変えたときは数分) |
| 途中のコミットを別の worktree で確かめる | 作業の習慣 | 別の target で作り直すので 5〜10 分 |
| strace での syscall・fd・メモリーの記録 | 運用者の指示(新しい機能のとき) | 5〜15 分 |
| push の後の CI を待つ(ホストの検査と musl のビルドが並んで動く) | 運用者の指示 | 4〜5 分 |

これまでは、どの変更でも、上の多くを毎回行っていました(コードの変更 1 回で、CI を待つ時間を除いて、およそ 1〜2 分。途中のコミットを確かめると 10 分を越える)。

## 段

| 段 | 選ぶ変更 | 行う確かめ |
| --- | --- | --- |
| docs | 文書だけ(`*.md`、`docs/`、`LICENSE`) | 差分の空白(`git diff --check`)、新しく書いた行の課題のキー、古い言い方(「使わない語」の表の語は落とす。対応表の語は表示するだけ)、コミットメッセージの形 |
| code | コード(上と下のどれにも当たらないもの。`crates/` の多く、`tools/` のほとんど) | 上に加えて、fmt・clippy・ホストのテスト(golden を含む)・テストの後に残ったプロセスの確かめ(`tools/check-leftovers.py`) |
| visual | 見た目に関わるもの(`seinas-render`・`seinas-text`・`seinas-fbdev` の全体、`seinas-frontend` の `decoration.rs`・`title_bar.rs`・`lib.rs`、`seinas-taskbar` と `seinas-wallpaper` の `lib.rs`、`tests/golden/`・`tests/golden.rs`) | 上に加えて、golden の決まり(既存の正解データの作り直しと `src/` の変更が、同じコミットに無いか) |
| full | 依存・ビルド(`Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml`、`.github/`、`THIRD-PARTY/`、`tools/build-musl.sh` などのビルドと依存の道具) | 上に加えて、third-party の確かめ、依存の検査、musl の静的ビルドとテスト、SHA-256 |

- 段は、`origin/main` との差のファイルごとに決め、いちばん上の段を選びます。
- 案から変えたところ:code の段でも golden を動かします。golden は `cargo test` の中で 1 秒ほどで終わり、動かすかどうかを分けるほうが手間だからです。visual の段で増えるのは、golden の決まり(コミットの分け方)の確かめです。
- musl の静的ビルドと SHA-256 は、CI を正とします。手元で回すのは full の段だけです。
- 途中のコミットを別の worktree で確かめることは、決まりにはしません(テストが通っているべきなのは、push する先頭のコミット。golden の決まりで、途中のコミットで golden が落ちることも認められている)。意味のまとまりで分けたコミットのうち、途中のものが単独で通ることを確かめたいときだけ行います。

## 使い方

```bash
tools/pre-push.sh
```

- 変更をすべてコミットしてから動かします(コミットしていない変更があると止まります。テストが、push するものと別のものを見ないようにするため)。
- `--tier docs|code|visual|full` で、段を手で上げられます(下げることはできません)。たとえば、新しい syscall を使いうる変更で、musl のビルドでも確かめたいときは `--tier full`。
- `--base <ref>` で、比べる相手を変えられます(既定は `origin/main`)。
- `--dry-run` で、選んだ段と、ファイルごとの段だけを出します。
- 終わりに、段と、かかった秒数と、落ちた確かめを出します。落ちたものがあれば、終わりの値は 1 です。

push の後は、CI を待って確かめます。

```bash
tools/ci-wait.sh
```

- HEAD の run を探して、終わるまで待ちます。成功したらジョブごとの結果を出し、落ちたら終わりの値 1 と、記録の見方(`gh run view <run> --log-failed`)を出します。
- 落ちたらすぐ直します。直せないときは revert して push し、CI が戻ったことを確かめてから報告します。

## git blame と言い換えのコミット

IT 用語を言い換えただけのコミット(行の意味は変えていないもの)は、`.git-blame-ignore-revs` に載せてあります。手元の `git blame` で飛ばすには、次の設定を入れます(入れるのは運用者)。

```bash
git config blame.ignoreRevsFile .git-blame-ignore-revs
```

- 載せたコミットより前のコミットは、作り直しません(SHA が変わり、載せた行が効かなくなるため)。
- 古い言い方の確かめ(`tools/old-terms.py`)は、このファイルを見ません(言い換える前の件名を、そのまま載せているため)。

## テストの後に残るプロセス

テスト(seinas-standalone の結合テストと golden)は、コンポジタとクライアントを子のプロセスとして起こします。同じホストで ZeikOS の確かめ(QEMU)も動くので、テストの後に子を残しません。

- 子は `tests/common/mod.rs` の `child_command` で起こします。ふつうの終わり方(成功、失敗、panic、timeout)では `Running` と `Compositor` の Drop が kill と wait をし、作業用の場所(`/tmp/seinas-<名前>-<PID>`)を消します。
- Drop が走らない終わり方(`std::process::exit`、Ctrl+C、SIGKILL)に備え、子には `PR_SET_PDEATHSIG`(SIGKILL)を付けます。テストのプロセスが終わると、カーネルが子を止めます。プロセスグループで止める形は、テストのプロセスが SIGKILL で終わったときに、止める側が残らないので選びませんでした。
- golden の `Runner::finish` は、失敗があると `std::process::exit` で終わります。子は、呼ぶ前に止めます。
- 片付けられなかった作業用の場所(印のファイル `.seinas-test` があるもの)は、次にテストが動くときに消します。
- `tools/pre-push.sh` は、テストを動かした段(code 以上)の終わりに `tools/check-leftovers.py` で、起こしたテストがもういない子のプロセスと作業用の場所が 0 であることを確かめます。手で確かめるときも同じものを使い、`--clean` で片付けられます。

## 報告に載せるもの

- SHA-256・バイナリの大きさ:変わったときだけ載せます(CI の「ELF の記録」の値を正とする)。
- syscall の差分:新しい syscall や fd を使いうる変更(新しいプログラム、依存の追加、ファイル・ソケット・共有メモリー・スレッド・タイマーを使う処理の追加)のときだけ、strace で取って `docs/m3-syscalls.md` に記録し、報告に載せます。
- CI の結果と、push した SHA は、いつも載せます。

## 所要時間の比べ

この手順を入れたコミットの push で比べました(`tools/` と `docs/` と `CLAUDE.md` の変更なので、code の段)。

| 手順 | 時間 |
| --- | --- |
| これまで(fmt・clippy・ホストのテスト・third-party・musl のビルドとテスト) | 約 49 秒(コードを 1 つ触った直後なら約 70 秒) |
| 新しい手順(`tools/pre-push.sh` の code の段) | 約 19 秒 |

どちらも、この後に CI を待つ時間(4〜5 分)がかかります。
