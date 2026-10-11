# テストのデータ

## two-faces.ttc

TTC(1 つのファイルに face が 2 つ)を、一覧と役の選び方で確かめるための、小さなフォント(7,696 バイト)。`tests/fonts.rs` が使う。

| face | 元のフォント | 名前(英語、日本語) |
| --- | --- | --- |
| 0 | BIZ UDPゴシック Regular(v1.051) | BIZ UDPGothic、BIZ UDPゴシック |
| 1 | BIZ UDゴシック Regular(v1.051) | BIZ UDGothic、BIZ UDゴシック |

- 入っている文字は `Seinasi タイトル` の文字(と空白)だけ。ヒンティングは外してある。
- 作り方:`tools/fetch-fonts.sh` で元のフォントを取ってから、fontTools 4.60.1 を入れた環境で `tools/make-test-ttc.sh` を動かす。中で行っていることは、次のとおり。

```sh
pyftsubset target/fonts/BIZUDPGothic-Regular.ttf --text='Seinasi タイトル' \
    --name-IDs='*' --name-languages='*' --no-hinting --output-file=a.ttf
pyftsubset target/fonts/BIZUDGothic-Regular.ttf --text='Seinasi タイトル' \
    --name-IDs='*' --name-languages='*' --no-hinting --output-file=b.ttf
# fontTools の TTCollection に a.ttf、b.ttf の順で入れて、表を共有して保存する
```

- 同じ入力からは、同じファイルができる(SHA-256 `f1f4463200f688ac275a2c851d9e222b135952ae705e3041ff8baafe80ac3065`)。

### ライセンス

元のフォントは SIL Open Font License 1.1(Copyright 2022 The BIZ UDGothic Project Authors)。このファイルは、元のフォントから文字を減らした、変えた版で、同じ OFL 1.1 で配る。ライセンス文と著作権表示は、隣の `OFL.txt`(上流のもの)。元のフォントに予約したフォント名(Reserved Font Name)は無いので、名前は元のままにしてある。
