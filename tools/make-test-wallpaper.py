#!/usr/bin/env python3
"""壁紙のテストのための小さな画像(crates/seinas-wallpaper/tests/data/test-48x32.png)を作る。

使い方:
    python3 tools/make-test-wallpaper.py

48x32 の RGBA。上半分は赤から青、下半分は緑から黄の横のグラデーション。外周の 1 ピクセルは白の枠、中央に
黒の十字。右の端の 8 列は、半ば透けている(アルファ 128)。置き方ごとの切り方・余白・縮め方・透けた所の
重ね方が、絵で分かるようにしてある。Python の標準ライブラリ(zlib)だけで作るので、同じ入力から同じファイルが
できる。この画像は Seinas の一部として、Seinas と同じライセンス(MIT)で配る。
"""

import os
import struct
import zlib

WIDTH, HEIGHT = 48, 32


def pixel(x, y):
    t = x / (WIDTH - 1)
    if y < HEIGHT // 2:
        r, g, b = round(255 * (1 - t)), 0, round(255 * t)
    else:
        r, g, b = round(255 * t), 255, 0
    if x in (0, WIDTH - 1) or y in (0, HEIGHT - 1):
        r, g, b = 255, 255, 255
    if x in (WIDTH // 2 - 1, WIDTH // 2) or y in (HEIGHT // 2 - 1, HEIGHT // 2):
        r, g, b = 0, 0, 0
    a = 128 if x >= WIDTH - 8 else 255
    return bytes((r, g, b, a))


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def main():
    rows = b"".join(b"\x00" + b"".join(pixel(x, y) for x in range(WIDTH)) for y in range(HEIGHT))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    path = os.path.join(root, "crates", "seinas-wallpaper", "tests", "data", "test-48x32.png")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)
    print(f"{path}: {len(png)} bytes")


if __name__ == "__main__":
    main()
