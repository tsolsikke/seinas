#!/usr/bin/env python3
"""アイコンのテストのための小さな画像と .desktop のファイル(crates/seinas-icons/tests/data/share/)を作る。

使い方:
    python3 tools/make-test-icons.py

できるもの(データの置き場の形。テストでは、XDG_DATA_DIRS にこの share を渡す):

- applications/seinas.test.{files,terminal,notes,settings}.desktop:Icon に、下のアイコンの名前を書いたもの
- icons/hicolor/24x24/apps/seinas-test-{folder,terminal,notes,settings}.png:24x24 の RGBA
- icons/hicolor/48x48/apps/seinas.test.large.png:48x48(.desktop は無い。縮めて使う所の確かめ)
- pixmaps/seinas.test.pixmap.png:24x24(テーマに無いときの pixmaps の確かめ)

絵柄は、フォルダー・端末・メモ・歯車・丸・三角の簡単な形で、どれも自作(既製のアイコンの集まりをコピーしていない)。
1 ピクセルを 4x4 に分けて数え、縁をなめらかにする。Python の標準ライブラリ(zlib)だけで作るので、同じ入力から
同じファイルができる。これらは Seinas の一部として、Seinas と同じライセンス(MIT)で配る。
"""

import math
import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                    "crates", "seinas-icons", "tests", "data", "share")
SAMPLES = 4


def rounded(x, y, w, h, r):
    def inside(px, py):
        if px < x or py < y or px >= x + w or py >= y + h:
            return False
        cx = min(max(px, x + r), x + w - r)
        cy = min(max(py, y + r), y + h - r)
        return (px - cx) ** 2 + (py - cy) ** 2 <= r * r
    return inside


def circle(cx, cy, r):
    return lambda px, py: (px - cx) ** 2 + (py - cy) ** 2 <= r * r


def union(*shapes):
    return lambda px, py: any(s(px, py) for s in shapes)


def minus(a, b):
    return lambda px, py: a(px, py) and not b(px, py)


def gear(cx, cy, r_out, r_in, teeth, hole):
    def inside(px, py):
        dx, dy = px - cx, py - cy
        d = math.hypot(dx, dy)
        if d <= hole:
            return False
        angle = math.atan2(dy, dx) % (2 * math.pi / teeth)
        tooth = abs(angle - math.pi / teeth) < math.pi / teeth * 0.5
        return d <= (r_out if tooth else r_in)
    return inside


def segment(x0, y0, x1, y1, width):
    def inside(px, py):
        vx, vy = x1 - x0, y1 - y0
        t = max(0.0, min(1.0, ((px - x0) * vx + (py - y0) * vy) / (vx * vx + vy * vy)))
        qx, qy = x0 + t * vx, y0 + t * vy
        return (px - qx) ** 2 + (py - qy) ** 2 <= (width / 2) ** 2
    return inside


def paint(size, layers):
    """奥から順の (色, 図形) を重ねた、premultiplied でない RGBA の行の並び。"""
    pixels = [[(0.0, 0.0, 0.0, 0.0)] * size for _ in range(size)]
    for y in range(size):
        for x in range(size):
            r = g = b = a = 0.0
            for color, shape in layers:
                count = sum(
                    shape(x + (sx + 0.5) / SAMPLES, y + (sy + 0.5) / SAMPLES)
                    for sy in range(SAMPLES) for sx in range(SAMPLES))
                cover = count / (SAMPLES * SAMPLES)
                if cover == 0:
                    continue
                # premultiplied で重ねる。
                r = color[0] / 255 * cover + r * (1 - cover)
                g = color[1] / 255 * cover + g * (1 - cover)
                b = color[2] / 255 * cover + b * (1 - cover)
                a = cover + a * (1 - cover)
            pixels[y][x] = (r, g, b, a)
    rows = []
    for row in pixels:
        line = bytearray(b"\x00")
        for r, g, b, a in row:
            if a > 0:
                line += bytes(round(v / a * 255) for v in (r, g, b))
            else:
                line += b"\x00\x00\x00"
            line.append(round(a * 255))
        rows.append(bytes(line))
    return size, b"".join(rows)


def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def write_png(path, picture):
    size, rows = picture
    os.makedirs(os.path.dirname(path), exist_ok=True)
    data = (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9))
            + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(data)


def folder(s):
    k = s / 24
    return paint(s, [
        ((0x5a, 0x8e, 0xd6), rounded(2 * k, 4 * k, 9 * k, 6 * k, 1.5 * k)),
        ((0x68, 0xa0, 0xe8), rounded(2 * k, 6.5 * k, 20 * k, 13.5 * k, 2 * k)),
    ])


def terminal(s):
    k = s / 24
    return paint(s, [
        ((0x81, 0x90, 0xab), rounded(2 * k, 4 * k, 20 * k, 16 * k, 2.5 * k)),
        ((0x1c, 0x26, 0x3a), rounded(3.5 * k, 5.5 * k, 17 * k, 13 * k, 1.5 * k)),
        ((0xe8, 0xec, 0xf2), union(segment(6.5 * k, 9 * k, 9.5 * k, 12 * k, 1.6 * k),
                                    segment(9.5 * k, 12 * k, 6.5 * k, 15 * k, 1.6 * k),
                                    segment(11.5 * k, 15.2 * k, 16 * k, 15.2 * k, 1.6 * k))),
    ])


def notes(s):
    k = s / 24
    lines = [segment(8 * k, y * k, 16 * k, y * k, 1.6 * k) for y in (8.5, 12, 15.5)]
    return paint(s, [
        ((0xa8, 0xb8, 0xd8), rounded(4.5 * k, 2.5 * k, 15 * k, 19 * k, 2 * k)),
        ((0x40, 0x48, 0x60), union(*lines)),
    ])


def settings(s):
    k = s / 24
    return paint(s, [
        ((0xa8, 0xb0, 0xd0), gear(12 * k, 12 * k, 10 * k, 7.5 * k, 8, 3.2 * k)),
    ])


def large(s):
    k = s / 24
    return paint(s, [((0x5c, 0xb8, 0x7a), circle(12 * k, 12 * k, 10 * k))])


def pixmap(s):
    k = s / 24
    triangle = lambda px, py: (py >= 4 * k and py <= 20 * k
                              and abs(px - 12 * k) <= (py - 4 * k) * 0.55)
    return paint(s, [((0xd0, 0x7a, 0x4a), triangle)])


DESKTOP = """[Desktop Entry]
Type=Application
Name={name}
Exec=true
Icon={icon}
"""


def main():
    apps = [("files", "seinas-test-folder", folder),
            ("terminal", "seinas-test-terminal", terminal),
            ("notes", "seinas-test-notes", notes),
            ("settings", "seinas-test-settings", settings)]
    for app, icon, draw in apps:
        desktop = os.path.join(ROOT, "applications", f"seinas.test.{app}.desktop")
        os.makedirs(os.path.dirname(desktop), exist_ok=True)
        with open(desktop, "w") as f:
            f.write(DESKTOP.format(name=f"Seinas test {app}", icon=icon))
        write_png(os.path.join(ROOT, "icons", "hicolor", "24x24", "apps", icon + ".png"), draw(24))
    write_png(os.path.join(ROOT, "icons", "hicolor", "48x48", "apps", "seinas.test.large.png"), large(48))
    write_png(os.path.join(ROOT, "pixmaps", "seinas.test.pixmap.png"), pixmap(24))
    print(f"wrote the test icons under {ROOT}")


if __name__ == "__main__":
    main()
