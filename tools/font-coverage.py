#!/usr/bin/env python3
"""既定のフォントが、日本語の UI と端末に要る文字を持っているかと、端末用のフォントの幅を確かめる。

使い方:
    python3 tools/font-coverage.py [フォントの置き場]      (既定は target/fonts)

確かめること:
・JIS X 0208 の文字(非漢字、第 1 水準、第 2 水準)と、半角カナ、いくつかの記号が、
  UI のフォント(BIZ UDPゴシック)、端末のフォント(BIZ UDゴシック)、フォールバックのフォント(GNU Unifont JP)の
  それぞれの cmap にあるか。UI のフォントに無い文字が、フォールバックのフォントにあるか。
・端末のフォント(BIZ UDゴシック)で、半角の文字と全角の文字の送り幅が 1:2 になっているか。

JIS X 0208 の文字の一覧は、Python の euc_jp のコーデックから作る(外のデータは読まない)。
フォントのファイルを読むだけで、何も書かない。依存は Python の標準ライブラリだけ。
"""

import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UI = "BIZUDPGothic-Regular.ttf"
MONO = "BIZUDGothic-Regular.ttf"
FALLBACK = "unifont_jp-18.0.01.otf"


def tables(data):
    count = struct.unpack(">H", data[4:6])[0]
    found = {}
    for i in range(count):
        tag, _, offset, length = struct.unpack(">4sIII", data[12 + 16 * i : 28 + 16 * i])
        found[tag.decode("latin-1")] = data[offset : offset + length]
    return found


def cmap(table):
    """Unicode から glyph の番号への対応。format 12 があれば使い、無ければ format 4。"""
    count = struct.unpack(">H", table[2:4])[0]
    subtables = {}
    for i in range(count):
        platform, encoding, offset = struct.unpack(">HHI", table[4 + 8 * i : 12 + 8 * i])
        subtables[(platform, encoding)] = offset
    mapping = {}
    if (3, 10) in subtables:
        at = subtables[(3, 10)]
        groups = struct.unpack(">I", table[at + 12 : at + 16])[0]
        for g in range(groups):
            start, end, glyph = struct.unpack(">III", table[at + 16 + 12 * g : at + 28 + 12 * g])
            for code in range(start, end + 1):
                mapping[code] = glyph + code - start
        return mapping
    at = subtables[(3, 1)]
    segments = struct.unpack(">H", table[at + 6 : at + 8])[0] // 2
    ends = struct.unpack(f">{segments}H", table[at + 14 : at + 14 + 2 * segments])
    base = at + 16 + 2 * segments
    starts = struct.unpack(f">{segments}H", table[base : base + 2 * segments])
    deltas = struct.unpack(f">{segments}h", table[base + 2 * segments : base + 4 * segments])
    offsets_at = base + 4 * segments
    offsets = struct.unpack(f">{segments}H", table[offsets_at : offsets_at + 2 * segments])
    for s in range(segments):
        for code in range(starts[s], ends[s] + 1):
            if code == 0xFFFF:
                continue
            if offsets[s] == 0:
                glyph = (code + deltas[s]) & 0xFFFF
            else:
                where = offsets_at + 2 * s + offsets[s] + 2 * (code - starts[s])
                glyph = struct.unpack(">H", table[where : where + 2])[0]
                if glyph:
                    glyph = (glyph + deltas[s]) & 0xFFFF
            if glyph:
                mapping[code] = glyph
    return mapping


def advances(found):
    metrics = struct.unpack(">H", found["hhea"][34:36])[0]
    hmtx = found["hmtx"]
    widths = [struct.unpack(">H", hmtx[4 * i : 4 * i + 2])[0] for i in range(metrics)]
    return lambda glyph: widths[min(glyph, metrics - 1)]


def load(path):
    data = open(path, "rb").read()
    found = tables(data)
    units = struct.unpack(">H", found["head"][18:20])[0]
    return {"cmap": cmap(found["cmap"]), "advance": advances(found), "units": units}


def jis_x_0208():
    """JIS X 0208 の文字を、区ごとの分類と一緒に返す。"""
    groups = {"非漢字(1〜8区)": [], "第1水準(16〜47区)": [], "第2水準(48〜84区)": []}
    for row in range(1, 85):
        for cell in range(1, 95):
            try:
                char = bytes([0xA0 + row, 0xA0 + cell]).decode("euc_jp")
            except UnicodeDecodeError:
                continue
            if row <= 8:
                groups["非漢字(1〜8区)"].append(char)
            elif 16 <= row <= 47:
                groups["第1水準(16〜47区)"].append(char)
            elif row >= 48:
                groups["第2水準(48〜84区)"].append(char)
    return groups


def main():
    fonts_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "fonts")
    fonts = {name: load(os.path.join(fonts_dir, name)) for name in (UI, MONO, FALLBACK)}
    groups = jis_x_0208()
    groups["半角カナ(U+FF61〜FF9F)"] = [chr(c) for c in range(0xFF61, 0xFFA0)]
    groups["記号(¥ \\ ‾ ～ 〜 ・ 結合濁点・半濁点)"] = list("¥\\‾～〜・゙゚")
    groups["罫線(U+2500〜257F)"] = [chr(c) for c in range(0x2500, 0x2580)]

    status = 0
    print("収録(フォントの cmap にある文字の数 / 対象の数)")
    for group, chars in groups.items():
        line = [f"{group}: {len(chars)}字"]
        for name in (UI, MONO, FALLBACK):
            have = sum(1 for c in chars if ord(c) in fonts[name]["cmap"])
            line.append(f"{name.split('-')[0]} {have}")
        ui_missing = [c for c in chars if ord(c) not in fonts[UI]["cmap"]]
        neither = [c for c in ui_missing if ord(c) not in fonts[FALLBACK]["cmap"]]
        line.append(f"UIに無い {len(ui_missing)}(うちフォールバックにも無い {len(neither)})")
        print("  " + "、".join(line))
        if ui_missing:
            print("    UIに無い字: " + " ".join(f"U+{ord(c):04X}" for c in ui_missing[:40]) + (" …" if len(ui_missing) > 40 else ""))
        if neither:
            status = 1
            print("    どちらにも無い字: " + " ".join(f"U+{ord(c):04X}" for c in neither))

    mono = fonts[MONO]
    advance = lambda c: mono["advance"](mono["cmap"][ord(c)])
    half = {advance(chr(c)) for c in range(0x21, 0x7F)} | {advance(chr(c)) for c in range(0xFF61, 0xFFA0)}
    full = {advance(c) for c in groups["第1水準(16〜47区)"] + list("あア　、。")}
    print(f"端末のフォントの送り幅(units_per_em {mono['units']}): 半角 {sorted(half)}、全角 {sorted(full)}")
    if len(half) == 1 and len(full) == 1 and 2 * half.pop() == full.pop():
        print("  半角と全角は 1:2")
    else:
        status = 1
        print("  半角と全角が 1:2 になっていない")
    return status


if __name__ == "__main__":
    sys.exit(main())
