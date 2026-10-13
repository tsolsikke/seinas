#!/usr/bin/env python3
"""テストの後に残った、Seinas のテストの子のプロセスと作業用の場所を探す。

使い方:
    python3 tools/check-leftovers.py [--clean]

残ったものとして数えるもの(どれも、起こしたテストのプロセスがもういないもの):
  1. 環境変数 SEINAS_TEST_CHILD(値はテストのプロセスの PID)を持つプロセス。テストの子は、これを付けて起こす
     (crates/seinas-standalone/tests/common/mod.rs の child_command)。
  2. 印を付ける前のテストが起こした seinas-standalone(引数が `--dump <一時ディレクトリ>/seinas-<名前>-<PID>/...`)。
  3. 一時ディレクトリの `seinas-<名前>-<PID>` で、印のファイル `.seinas-test` があるもの。

テストのプロセスが生きているもの(並べて動かしているテスト)と、印も形も合わないもの(運用者が起こした
seinas など)は数えない。自分のユーザーのプロセスだけを見る。

見つけたら一覧を書き、exit status 1 で終わる。`--clean` を付けると、見つけたプロセスを SIGTERM で止め、
作業用の場所を消してから、もう一度数える。PR_SET_PDEATHSIG で止まる子は少し遅れて消えるので、数える前に
最大 3 秒待つ。
"""

import os
import re
import shutil
import signal
import sys
import tempfile
import time

MARK_ENV = b"SEINAS_TEST_CHILD="
DIR_MARK = ".seinas-test"
DUMP = re.compile(r"--dump\s+(\S*/seinas-[A-Za-z0-9]+-(\d+))/")
DIR_NAME = re.compile(r"^seinas-[A-Za-z0-9]+-(\d+)$")


def alive(pid):
    return os.path.exists(f"/proc/{pid}")


def read(path):
    try:
        with open(path, "rb") as f:
            return f.read()
    except OSError:
        return None


def leftover_processes():
    found = []
    me = os.getuid()
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            if os.stat(f"/proc/{entry}").st_uid != me:
                continue
        except OSError:
            continue
        environ = read(f"/proc/{entry}/environ")
        cmdline = read(f"/proc/{entry}/cmdline")
        if environ is None or cmdline is None:
            continue
        command = cmdline.replace(b"\0", b" ").decode(errors="replace").strip()
        owner = None
        for item in environ.split(b"\0"):
            if item.startswith(MARK_ENV):
                owner = item[len(MARK_ENV):].decode(errors="replace")
        if owner is None:
            match = DUMP.search(command)
            if match and "seinas-standalone" in command.split(" ")[0]:
                owner = match.group(2)
        if owner is None or not owner.isdigit() or alive(owner):
            continue
        stat = read(f"/proc/{entry}/stat") or b""
        ppid = stat.rsplit(b")", 1)[-1].split()[1].decode() if b")" in stat else "?"
        found.append((int(entry), ppid, owner, command))
    return found


def leftover_dirs():
    found = []
    base = tempfile.gettempdir()
    for name in os.listdir(base):
        match = DIR_NAME.match(name)
        path = os.path.join(base, name)
        if match and os.path.exists(os.path.join(path, DIR_MARK)) and not alive(match.group(1)):
            found.append(path)
    return found


def survey():
    # PR_SET_PDEATHSIG で止まる子は、少し遅れて消える。
    deadline = time.monotonic() + 3
    while True:
        processes, dirs = leftover_processes(), leftover_dirs()
        if not processes or time.monotonic() >= deadline:
            return processes, dirs
        time.sleep(0.2)


def main():
    clean = "--clean" in sys.argv[1:]
    processes, dirs = survey()
    for pid, ppid, owner, command in processes:
        print(f"残ったプロセス:PID {pid}、親 {ppid}、起こしたテスト {owner}(もういない):{command[:200]}")
    for path in dirs:
        print(f"残った作業用の場所:{path}")
    if clean and (processes or dirs):
        for pid, _, _, _ in processes:
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
        for path in dirs:
            shutil.rmtree(path, ignore_errors=True)
        processes, dirs = survey()
        print(f"片付けた後:プロセス {len(processes)}、作業用の場所 {len(dirs)}")
    if processes or dirs:
        if not clean:
            print("片付けるには:python3 tools/check-leftovers.py --clean")
        return 1
    print("残ったテストのプロセスと作業用の場所は、ありません。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
