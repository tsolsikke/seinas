# muslでの静的ビルド

seinasとzeyes-minを、`x86_64-unknown-linux-musl` 向けに静的にリンクして作る手順と、できた実行ファイルの記録です。
ZeikOS側で、像の上限やスタックの大きさを決めるときの材料にします。
同じ手順でseinas-fbdevも作ります。そちらの記録は `docs/fbdev.md` にあります。

## 作り方

```bash
tools/build-musl.sh
```

この1つで、次の順に進みます。

1. `tools/build-musl-libs.sh` が、pixmanとlibxkbcommonのソースを取り、musl向けの静的ライブラリを `target/musl-libs/lib/` に作る(すでにあれば飛ばす)。
2. `cargo build --release --locked --target x86_64-unknown-linux-musl` で、seinasとzeyes-minを作る。
3. `tools/elf-report.sh` で、静的PIEになっていることを確かめる。
4. 実行ファイルと `LICENSE`、`THIRD-PARTY/` を `target/dist/seinas-musl/` にまとめる。

要るものは、Rust(`rust-toolchain.toml` の版とmuslのターゲット)、musl-tools、meson、ninja、bison、curlです。

## 版の固定

| もの | 版 | 固定のしかた |
| --- | --- | --- |
| Rust | 1.97.1 | `rust-toolchain.toml` |
| クレート | — | `Cargo.lock`(`--locked`) |
| pixman | 0.42.2 | `tools/build-musl-libs.sh` の中のURLとSHA-256 |
| libxkbcommon | 1.6.0 | 同上 |
| musl | 1.2.5 | Rustのmusl向けターゲットに同梱のもの(Rustの版で決まる) |

pixmanとlibxkbcommonの版は、Ubuntu 24.04に入っている共有ライブラリ(glibc版が使うもの)と同じにしています。
ソースのSHA-256が合わなければ、ビルドは止まります。

## muslの版の混在

今の手順では、muslの版が2つ混ざっています。

| 場面 | 使うmusl | 版 |
| --- | --- | --- |
| pixmanとlibxkbcommonのコンパイル | ホストのmusl-gcc(Ubuntu 24.04のmusl-tools)のヘッダー | 1.2.4 |
| 最後のリンク(実行ファイルに入るlibc) | Rustのmusl向けターゲットに同梱のもの | 1.2.5 |

今のところ、問題は出ていません。

- musl向けのテスト一式が通り、WSLgでの表示もglibc版と同じです。
- muslは、1.2系の中で、関数の呼び出し方や構造体の並びを変えない方針で作られています。1.2.4のヘッダーでコンパイルしたコードを、1.2.5のlibcにリンクしても、食い違いは起きにくい組み合わせです。
- Cのライブラリが使うのは、文字列、メモリーの確保、ファイルの読み書きなどの基本の関数です。

ただし、版をそろえたほうが、後で原因を追うときに迷いません。そろえる案は2つあります。

1. **リンクもホストのmuslに寄せる。** リンカーに `musl-gcc` を指定し、Rustに同梱のlibcを使わない設定(`-C link-self-contained=no`)にします。すべてが1.2.4になります。手順は少し変わるだけですが、Rustの標準ライブラリは1.2.5に合わせて作られているので、今度はそちらとの組み合わせを確かめる必要があります。
2. **Cのライブラリを、1.2.5のヘッダーでコンパイルする。** musl 1.2.5のソースを取って自分で作り、そのmusl-gccでpixmanとlibxkbcommonを作ります。すべてが1.2.5になります。作るものが1つ増えますが、ホストに入っているmusl-toolsの版に左右されなくなります。

## CPUの命令

- Rustは、既定のx86-64(SSE2まで)で作ります。`target-cpu` などの指定は付けません。`tools/build-musl.sh` は、外から入った `RUSTFLAGS` を外してからビルドします。
- Cのライブラリにも、`-march=native` などの指定は付けません。
- pixman 0.42.2には、AVX2の処理がありません。ソースに `avx` という語が無く、meson の選択肢にも `avx2` がありません。そのため、ビルドの設定でAVX2を切る必要はありません。
- pixmanが実行時に選ぶのは、MMX、SSE2、SSSE3の3つです。CPUIDの機能の旗だけを見て決め、OSXSAVEやXGETBVは見ません。どれもXMMとMMXのレジスタだけを使うので、OSXSAVEが0でも動きます。
- できた実行ファイルを逆アセンブルして、YMM・ZMMのレジスタを使う命令と `xgetbv` が1つも無いことを確かめています。

pixmanの版を上げるときは、注意が要ります。上げた先の版にAVX2の処理が入っていないかを、同じやり方(ソースと選択肢、できたライブラリの逆アセンブル)で確かめ直してください。入っている場合は、実行時の判定がOSXSAVEとXGETBVを見ているかを確かめ、確かでなければビルドの設定でAVX2を切ってください。

実行時にpixmanの速い処理を止めたいときは、環境変数 `PIXMAN_DISABLE` が使えます(例: `PIXMAN_DISABLE="ssse3"`)。

## ELFの記録

`tools/elf-report.sh` の出力です(2026-10-04、上の版で作ったもの)。

```
== seinas
file size: 5013824 bytes
  Type:                              DYN (Position-Independent Executable file)
  Machine:                           Advanced Micro Devices X86-64
PT_INTERP: 0, DT_NEEDED: 0
PT_TLS: FileSiz 0x000098, MemSiz 0x000308, Align 0x10
Program Headers:
  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align
  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x02d7e8 0x02d7e8 R   0x1000
  LOAD           0x02e000 0x000000000002e000 0x000000000002e000 0x277516 0x277516 R E 0x1000
  LOAD           0x2a6000 0x00000000002a6000 0x00000000002a6000 0x09ece4 0x09ece4 R   0x1000
  LOAD           0x345bc0 0x0000000000345bc0 0x0000000000345bc0 0x022760 0x0245a8 RW  0x1000
  DYNAMIC        0x365068 0x0000000000365068 0x0000000000365068 0x000180 0x000180 RW  0x8
  NOTE           0x000270 0x0000000000000270 0x0000000000000270 0x000024 0x000024 R   0x4
  TLS            0x345bc0 0x0000000000345bc0 0x0000000000345bc0 0x000098 0x000308 R   0x10
  GNU_EH_FRAME   0x2e6f88 0x00000000002e6f88 0x00000000002e6f88 0x00a4fc 0x00a4fc R   0x4
  GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x10
  GNU_RELRO      0x345bc0 0x0000000000345bc0 0x0000000000345bc0 0x021440 0x021440 R   0x1

== zeyes-min
file size: 1722952 bytes
  Type:                              DYN (Position-Independent Executable file)
  Machine:                           Advanced Micro Devices X86-64
PT_INTERP: 0, DT_NEEDED: 0
PT_TLS: FileSiz 0x000050, MemSiz 0x000080, Align 0x8
Program Headers:
  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align
  LOAD           0x000000 0x0000000000000000 0x0000000000000000 0x011d50 0x011d50 R   0x1000
  LOAD           0x012000 0x0000000000012000 0x0000000000012000 0x0ae126 0x0ae126 R E 0x1000
  LOAD           0x0c1000 0x00000000000c1000 0x00000000000c1000 0x032020 0x032020 R   0x1000
  LOAD           0x0f3bd8 0x00000000000f4bd8 0x00000000000f4bd8 0x00d03c 0x00e8f0 RW  0x1000
  DYNAMIC        0x0fee40 0x00000000000ffe40 0x00000000000ffe40 0x000180 0x000180 RW  0x8
  NOTE           0x000270 0x0000000000000270 0x0000000000000270 0x000024 0x000024 R   0x4
  TLS            0x0f3bd8 0x00000000000f4bd8 0x00000000000f4bd8 0x000050 0x000080 R   0x8
  GNU_EH_FRAME   0x0d4e48 0x00000000000d4e48 0x00000000000d4e48 0x003884 0x003884 R   0x4
  GNU_STACK      0x000000 0x0000000000000000 0x0000000000000000 0x000000 0x000000 RW  0x10
  GNU_RELRO      0x0f3bd8 0x00000000000f4bd8 0x00000000000f4bd8 0x00c428 0x00c428 R   0x1
```

読み取れること:

- どちらも `ET_DYN` の静的PIEです。`PT_INTERP` も `DT_NEEDED` もありません。
- 置く番地は読み込む側が決めます。起動時に、実行ファイル自身が再配置を行います(`R_X86_64_RELATIVE` だけで、seinasが7734個、zeyes-minが3013個)。そのため、書き込める区画(RW)への書き込みが起動直後に起きます。
- `PT_TLS` があります。スレッドごとに、seinasは0x308バイト(776バイト、16バイト境界)、zeyes-minは0x80バイト(128バイト、8バイト境界)が要ります。
- `GNU_STACK` はRWで、スタックの実行は求めません。
- `GNU_RELRO` があります。再配置の後、その範囲を読み取り専用に変えます(`mprotect`)。

メモリーに置いたときの像の大きさ(最後のLOADの終わりまで)は次のとおりです。

| 実行ファイル | 像の大きさ | 読み取り専用 | 実行(R E) | 書き込み(RW) |
| --- | --- | --- | --- | --- |
| seinas | 0x36a168(約3.42 MiB) | 0x02d7e8 + 0x09ece4 | 0x277516 | 0x0245a8(うちファイルに無い分 0x1e48) |
| zeyes-min | 0x1034c8(約1.01 MiB) | 0x011d50 + 0x032020 | 0x0ae126 | 0x00e8f0(うちファイルに無い分 0x18b4) |

## 実測した量

WSLg(Linux 6.18、x86-64)で、seinasの中でzeyes-minを1つ動かし、5秒ほど置いて測りました(2026-10-07)。
ポインターは動かしていないので、入力を渡す経路のぶんは入っていません。
測ったのは、文字を描く部分(ウィンドウの題名)と、動かす・閉じるの操作を組み込み、道の名前の置き換え(`--remap-path-prefix` など)を入れた後の形です。
seinasは、`target/fonts` のフォント(BIZ UDPゴシックとGNU Unifont JP)を読み込んだ状態です。

### 実行ファイルの大きさ

| 実行ファイル | musl(静的) | muslをstripしたとき | glibc(動的) |
| --- | --- | --- | --- |
| seinas | 8,462,376 バイト | 6,339,824 バイト | 7,112,272 バイト |
| zeyes-min | 1,748,112 バイト | 1,070,008 バイト | 1,807,896 バイト |

2026-10-04(文字を描く部分を組み込む前)は、seinasがmuslで5,013,824バイト(stripして3,574,512バイト)、glibcで3,661,672バイトでした。
増えたぶんのほとんどは、文字を描く部分です(`text-rendering.md` の「実測した量」)。

### メモリー

| 量 | seinas(musl) | zeyes-min(musl) | seinas(glibc) | zeyes-min(glibc) |
| --- | --- | --- | --- | --- |
| 最大RSS(`/usr/bin/time -v`) | 21,544 kB | 1,408 kB | 22,480 kB | 3,020 kB |
| 最大RSS(`VmHWM`) | 21,732 kB | 1,588 kB | 22,948 kB | 3,240 kB |
| 仮想メモリーの最大(`VmPeak`) | 22,584 kB | 1,904 kB | 25,760 kB | 4,552 kB |
| 自分だけの書き込める領域(`VmData`) | 12,172 kB | 132 kB | 11,980 kB | 240 kB |
| スタックで触れた量(`[stack]` のRss) | 44 kB | 20 kB | 44 kB | 16 kB |
| スレッドの数(クライアントがいる間) | 1 | 1 | 1 | 1 |

- 2回測って、ほぼ同じ値でした(違いは、最大RSSで200 kBほど)。
- zeyes-min の大きさは、イベントを timeout なしで待つ形に変えた後(2026-10-08)の値です。メモリーは、変えた後に測り直しても、この表の値との差が 300 kB 以内だったので、表は変えていません。
- seinasの最大RSSには、共有メモリーの対応づけが入っています(下の「描画バッファ」の、親へ出すプールと、クライアントのプール)。
- seinasの `VmData` の大半は、読み込んだフォントの中身(約9.5 MiB)と、合成先のキャンバス(約1.83 MiB)です。フォントを読まないときの値は、`text-rendering.md` の「実測した量」にあります。
- 2026-10-04(文字を描く部分を組み込む前)は、seinas(musl)の最大RSS(`VmHWM`)が8,692 kB、`VmData` が2,204 kBでした。
- クライアントが去ると、seinasのスレッドは2本になります(Smithayが、共有メモリーの後始末のスレッドを作るため)。Rustのスレッドは、既定で2 MiBのスタックを予約します。

### スタックの目安

- 主スレッドで実際に触れたのは、seinasで44 kB、zeyes-minで16〜20 kBでした(2026-10-04は、seinasで20〜24 kBでした。題名の文字を組んで描くぶん、増えています)。
- これは、起動して1枚を描き、待っている状態の値です。ポインターの入力を渡す経路や、クライアントが増えた場合は測れていません。
- Linuxの既定(8 MiB)は、この値に対して十分に大きいので、上限に当たった様子はありません。ZeikOSで小さく決める場合は、測った値に余裕を持たせてください。

### 描画バッファ

画素の形式は、どれも `Xrgb8888`(1画素4バイト)です。

| バッファ | 大きさ | 持ち主 | 置き場所 |
| --- | --- | --- | --- |
| 合成先のキャンバス | 800 × 600 × 4 = 1,920,000 バイト | seinas(共通の描画) | ヒープ |
| 親へ出すプール(バッファ2枚ぶん) | 1,920,000 × 2 = 3,840,000 バイト | seinas(入れ子の裏側) | 共有メモリー |
| zeyes-minのプール(バッファ2枚ぶん) | 320 × 240 × 4 × 2 = 614,400 バイト | zeyes-min(seinasも対応づける) | 共有メモリー |

- fbdevの裏側では、親へ出すプールの代わりに、フレームバッファが表示先になります。
- 親やクライアントがバッファを離すのが遅いと、プールは3枚ぶん以上に伸びることがあります。

## glibc版との違い

- 同じソース、同じ版のpixmanとlibxkbcommonを使っています。
- musl向けでも、テスト一式(合成結果の画素を比べるテストを含む)が通ります。

```bash
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-L native=$PWD/target/musl-libs/lib" cargo test --workspace --locked --target x86_64-unknown-linux-musl
```
