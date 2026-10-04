#!/bin/sh
# 候補を一通り動かして、試し描きの画像と記録を out/ に作る。
#
# 使い方(このディレクトリで):
#     ../../tools/fetch-fonts.sh   # フォントを取得する(初回だけ)
#     ./run.sh
#
# できるもの(out/ の下。リポジトリには入れない):
#     compare-<フォント>-<配色>-<大きさ>px.png   候補を上から順に並べた、見比べ用の画像
#     compare-sizes-<フォント>-<配色>.png        10〜20pxを1pxずつ変えて、見分けにくい字を並べた画像
#     <候補>-<フォント>-<配色>.png               1つの候補を、大きさの順に並べた画像
#     <候補>-report.txt                          速さ、メモリー、文字の幅の記録
# フォントは udp(BIZ UDPゴシック)と ud(BIZ UDゴシック)、配色は light と dark。
set -eu
cd "$(dirname "$0")"

fonts=../../target/fonts
cargo build --release --locked
rm -rf out
for engine in baseline cosmic swash fontdue; do
    ./target/release/try-$engine "$fonts" out
done
# 字形のヒンティングを切った場合も、あわせて出す。
./target/release/try-cosmic "$fonts" out nohint
./target/release/try-swash "$fonts" out nohint
python3 sheets.py out
# もとのPPMは大きいので、PNGにまとめた後は消す。
rm -f out/*.ppm
