#!/usr/bin/env python3
"""新しく書いた行に、古い言い方(docs/terminology.md の対応表の「これまで」)が無いかを探す。

使い方:
    git diff <base>...HEAD -U0 | python3 tools/old-terms.py

対応表の「これまで」の欄から言葉を作る(括弧の中の断り書きは除き、「、」で分ける)。「これから」の欄に同じ言葉が
入っているもの(「実行ファイル」→「バイナリ(実行ファイルでもよい)」など)は、古い言い方として扱わない。動詞は、
活用しても当たるよう、語幹で探す(「掴む」は「掴」、「待ち受ける」は「待ち受け」)。

見つけた行を「ファイル:行の番号: 言葉: 行」の形で書く。対応表の言葉は読み方の問題なので、知らせるだけにする。

docs/terminology.md の「使わない語」の表の「当たる形」は、新しく書いた行に入ったら止める。その行は、頭に
「使わない語:」を付けて書き、終わりの値を 1 にする。「除く形」の中に収まる当たりと、すぐ後に「→」が続く当たり
(言い換えの例)は数えない。

終わりの値:0 は止める当たりが無い、1 は「使わない語」に当たった、2 は使い方の誤り(標準入力が端末のときも)。
"""

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TABLE = os.path.join(ROOT, "docs", "terminology.md")

# 対応表のこのファイルと、この道具そのものは、古い言い方を書くのが役目なので見ない。依存の一覧
# (tools/third-party.py が作るもの)と、言い換えのコミットの件名をそのまま載せる .git-blame-ignore-revs も見ない。
SKIPPED = {"docs/terminology.md", "tools/old-terms.py", "THIRD-PARTY/rust-crates.md", ".git-blame-ignore-revs"}

# 同じ字を含むが、古い言い方ではない言葉。言葉の前後に、この字が続くときは当たりにしない。
ALLOWED = {
    "像": ["画像", "想像", "像を結", "写像", "映像", "解像", "実像", "虚像", "肖像", "偶像", "残像", "群像", "鏡像", "全体像"],
    "写": ["写真", "描写"],
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
    # すぐ後に「→」が続くのは、言い換えの例。
    if line[at + len(word) : at + len(word) + 1] == "→":
        return True
    for compound in ALLOWED.get(word, []):
        start = compound.find(word)
        begin = at - start
        if begin >= 0 and line[begin : begin + len(compound)] == compound:
            return True
    return False


def strict_terms():
    """「使わない語」の表の、(当たる形, 除く形の一覧) の並び。"""
    text = open(TABLE, encoding="utf-8").read()
    start = text.index("## 使わない語")
    end = text.index("\n## ", start + 1)
    found = []
    for line in text[start:end].splitlines():
        # セルの中の「\|」は、区切りではなく文字の「|」。
        cells = [c.strip().replace("\\|", "|") for c in re.split(r"(?<!\\)\|", line.strip().strip("|"))]
        if len(cells) != 5 or cells[0] in ("使わない語", "---"):
            continue
        excluded = re.findall(r"`([^`]+)`", cells[3])
        for form in re.findall(r"`([^`]+)`", cells[2]):
            found.append((form, excluded))
    return found


def strict_hits(line, rules):
    hits = []
    for form, excluded in rules:
        for m in re.finditer(re.escape(form), line):
            if line[m.end() : m.end() + 1] == "→":
                continue
            inside = False
            for word in excluded:
                start = word.find(form)
                begin = m.start() - start
                if start >= 0 and begin >= 0 and line[begin : begin + len(word)] == word:
                    inside = True
            if not inside:
                hits.append(form)
                break
    return hits


def main():
    if any(arg in ("-h", "--help") for arg in sys.argv[1:]):
        print(__doc__.strip())
        return 0
    if sys.argv[1:]:
        print(f"old-terms.py: 引数は取りません: {' '.join(sys.argv[1:])}(--help で使い方)", file=sys.stderr)
        return 2
    if sys.stdin.isatty():
        print("old-terms.py: 差分を標準入力に渡してください(--help で使い方)", file=sys.stderr)
        return 2
    # 差分には、UTF-8 でないファイルの行も入りうる。読めない字は置き換えて、止まらないようにする。
    sys.stdin.reconfigure(encoding="utf-8", errors="replace")
    words = terms()
    rules = strict_terms()
    status = 0
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
            strict = strict_hits(line, rules)
            if strict:
                print(f"使わない語:{path}:{number}: {'・'.join(strict)}: {line.strip()[:160]}")
                status = 1
            hits = []
            for word in words:
                for m in re.finditer(re.escape(word), line):
                    if not allowed_at(line, word, m.start()):
                        hits.append(word)
                        break
            if hits:
                print(f"{path}:{number}: {'・'.join(hits)}: {line.strip()[:160]}")
        number += 1
    return status


if __name__ == "__main__":
    sys.exit(main())
