#!/usr/bin/env python3
"""試し描きの画像(PPM)を、見比べやすいPNGにまとめる。

使い方:
    python3 sheets.py [out]

out/ の下の <候補>-<フォント>-<配色>-<大きさ>px.ppm を読み、次のPNGを out/ に書く。

    compare-<フォント>-<配色>-<大きさ>px.png   同じ条件で、候補を上から順に並べたもの(見比べ用)
    compare-sizes-<フォント>-<配色>.png        10〜20pxを1pxずつ変えて、見分けにくい字を並べたもの
    <候補>-<フォント>-<配色>.png               1つの候補で、大きさを上から順に並べたもの

PNGは、Pythonの標準のライブラリ(zlib)だけで書く。
"""

import struct
import sys
import zlib
from pathlib import Path

# 上から順に並べる。-nohint は、字形のヒンティングを切ったもの。fontdueは、もともとヒンティングをしない。
ENGINES = ["cosmic-text", "cosmic-text-nohint", "swash", "swash-nohint", "fontdue"]
FONTS = ["udp", "ud"]
THEMES = ["light", "dark"]
SIZES = [12, 14, 16, 18]
# 画像と画像の間に入れる、仕切りの色と太さ。
GAP = 6
SEPARATOR = {"light": (200, 60, 60), "dark": (240, 120, 120)}


def read_ppm(path):
    data = path.read_bytes()
    magic, width, height, _maxval = data.split(maxsplit=4)[:4]
    assert magic == b"P6"
    width, height = int(width), int(height)
    pixels = data[-width * height * 3 :]
    return width, height, pixels


def write_png(path, width, height, pixels):
    rows = b"".join(b"\x00" + pixels[y * width * 3 : (y + 1) * width * 3] for y in range(height))

    def chunk(tag, body):
        return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    )


def stack(images, separator):
    """画像を上から順に並べる。間に仕切りを入れる。幅は、いちばん広いものに合わせる。"""
    width = max(w for w, _, _ in images)
    out = bytearray()
    height = 0
    for index, (w, h, pixels) in enumerate(images):
        if index:
            out += bytes(separator) * width * GAP
            height += GAP
        for y in range(h):
            row = pixels[y * w * 3 : (y + 1) * w * 3]
            out += row + row[-3:] * (width - w)
        height += h
    return width, height, bytes(out)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "out")
    for font in FONTS:
        for theme in THEMES:
            for size in SIZES:
                images = [read_ppm(out / f"{engine}-{font}-{theme}-{size}px.ppm") for engine in ENGINES]
                write_png(out / f"compare-{font}-{theme}-{size}px.png", *stack(images, SEPARATOR[theme]))
            for engine in ENGINES:
                images = [read_ppm(out / f"{engine}-{font}-{theme}-{size}px.ppm") for size in SIZES]
                write_png(out / f"{engine}-{font}-{theme}.png", *stack(images, SEPARATOR[theme]))
            images = [read_ppm(out / f"{engine}-sizes-{font}-{theme}.ppm") for engine in ENGINES]
            write_png(out / f"compare-sizes-{font}-{theme}.png", *stack(images, SEPARATOR[theme]))
    print(f"wrote the sheets to {out}/ (compare-*.png: {', '.join(ENGINES)} from top to bottom)")


if __name__ == "__main__":
    main()
