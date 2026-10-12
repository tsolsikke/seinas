#!/usr/bin/env python3
"""新しく書いた行に、古い言い方(docs/terminology.md の対応表の「これまで」)が無いかを探す。

使い方:
    git diff <base>...HEAD -U0 | python3 tools/old-terms.py

対応表の「これまで」の欄から言葉を作る(括弧の中の断り書きは除き、「、」で分ける)。「これから」の欄に同じ言葉が
入っているもの(「実行ファイル」→「バイナリ(実行ファイルでもよい)」など)は、古い言い方として扱わない。動詞は、
活用しても当たるよう、語幹で探す(「掴む」は「掴」、「待ち受ける」は「待ち受け」)。

見つけた行を「ファイル:行の番号: 言葉: 行」の形で書く。読み方の問題なので、終わりの値はいつも 0(知らせるだけ)。
"""

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TABLE = os.path.join(ROOT, "docs", "terminology.md")

# 対応表のこのファイルと、この道具そのものは、古い言い方を書くのが役目なので見ない。依存の一覧
# (tools/third-party.py が作るもの)も見ない。
SKIPPED = {"docs/terminology.md", "tools/old-terms.py", "THIRD-PARTY/rust-crates.md"}

# 同じ字を含むが、古い言い方ではない言葉。言葉の前後に、この字が続くときは当たりにしない。
ALLOWED = {
    "像": ["画像", "想像", "像を結"],
    "版": ["出版", "版権"],
    "帯": ["帯域", "時間帯", "所帯"],
    "要素": [],
    # 「当たり、外れ」(ヒット、ミス)のほかの「外れる」(取れる、離れる)は、古い言い方ではない。
    "外れ": ["外れる", "外れた", "外れて", "外れな", "外れま"],
    "席": ["出席", "欠席"],
}

VERB_ENDINGS = "るすむくぐぶつう"


def terms():
    text = open(TABLE, encoding="utf-8").read()
    table = text[text.index("## 対応表"):]
    found = []
    for line in table.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 2 or cells[0] in ("これまで", "---"):
            continue
        old, new = cells
        old = re.sub(r"[(（][^)）]*[)）]", "", old)
        for word in old.split("、"):
            word = word.strip()
            if not word or word in new:
                continue
            if word.endswith("する"):
                word = word[: -len("する")]
            elif len(word) >= 2 and word[-1] in VERB_ENDINGS and word not in ("試験",):
                word = word[:-1]
            found.append(word)
    # 長い言葉を先に(「題名の帯」を「帯」より先に見る)。
    return sorted(set(found), key=len, reverse=True)


def allowed_at(line, word, at):
    for compound in ALLOWED.get(word, []):
        start = compound.find(word)
        begin = at - start
        if begin >= 0 and line[begin : begin + len(compound)] == compound:
            return True
    return False


def main():
    words = terms()
    path = None
    number = 0
    for raw in sys.stdin:
        raw = raw.rstrip("\n")
        if raw.startswith("+++ "):
            path = raw[6:] if raw.startswith("+++ b/") else None
            continue
        match = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)", raw)
        if match:
            number = int(match.group(1))
            continue
        if not raw.startswith("+") or path is None:
            continue
        line = raw[1:]
        if path not in SKIPPED:
            hits = []
            for word in words:
                for m in re.finditer(re.escape(word), line):
                    if not allowed_at(line, word, m.start()):
                        hits.append(word)
                        break
            if hits:
                print(f"{path}:{number}: {'・'.join(hits)}: {line.strip()[:160]}")
        number += 1


if __name__ == "__main__":
    main()
