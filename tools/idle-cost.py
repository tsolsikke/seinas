#!/usr/bin/env python3
"""何もしていないときの CPU 使用と、起きる回数を測る(ヘッドレス)。

使い方(リポジトリの先頭から):
    python3 tools/idle-cost.py [--seconds 60] [--profile release|debug]

2 つの構成を、それぞれ起こして測る。
  standalone:seinas-standalone(fake screen 1280x720)に、タスクバーと壁紙(シェルの部品のソケット)と、
              テスト用のウィンドウ 1 つ。
  nested:    seinas-standalone を親にして、入れ子の seinas を動かし、そこにタスクバーと壁紙とテスト用の
              ウィンドウ 1 つ。親の standalone も測る(入れ子の Seinas が親に描かせる分が入る)。

測るもの(起こしてから 3 秒待ち、その後の --seconds 秒の差):
  CPU:/proc/<pid>/stat の utime + stime(clock tick)の差を、ミリ秒と、使った割合(%)にする。
  起きる回数:/proc/<pid>/task/*/status の voluntary_ctxt_switches + nonvoluntary_ctxt_switches の差を、
              全部のスレッドで足し、1 秒あたりにする。眠っていたスレッドが起きると 1 回ずつ数わる(待ちの
              syscall(epoll_wait など)から戻った回数に、ほぼ同じ)。
テスト用のウィンドウは、測らない(テスト用のクライアントは、200 ms ごとに読み込みの待ちを切り上げる作り)。

子には、印の環境変数 SEINAS_TEST_CHILD と PR_SET_PDEATHSIG を付け、作業用の場所(印のファイルつき)は
終わるときに消す(残りは tools/check-leftovers.py で確かめられる)。
"""

import argparse
import ctypes
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TICK = os.sysconf("SC_CLK_TCK")
PACKAGES = ["seinas", "seinas-standalone", "seinas-taskbar", "seinas-wallpaper", "seinas-test-clients"]


def pdeathsig():
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl(1, signal.SIGKILL)  # PR_SET_PDEATHSIG


def cpu_ticks(pid):
    fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return int(fields[11]) + int(fields[12])  # utime, stime


def switches(pid):
    total = 0
    for task in os.listdir(f"/proc/{pid}/task"):
        try:
            for line in open(f"/proc/{pid}/task/{task}/status"):
                if line.startswith(("voluntary_ctxt_switches", "nonvoluntary_ctxt_switches")):
                    total += int(line.split()[1])
        except OSError:
            pass
    return total


class Run:
    def __init__(self, bin_dir, work):
        self.bin = bin_dir
        self.work = work
        self.procs = []

    def start(self, name, args, env):
        full = dict(os.environ, SEINAS_TEST_CHILD=str(os.getpid()), **env)
        full.pop("SEINAS_FONTS", None)
        log = open(os.path.join(self.work, f"{name}.log"), "w")
        proc = subprocess.Popen([os.path.join(self.bin, args[0])] + args[1:], env=full,
                                stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                preexec_fn=pdeathsig)
        self.procs.append((name, proc))
        return proc

    def stop(self):
        for _, proc in self.procs:
            proc.terminate()
        for _, proc in self.procs:
            try:
                proc.wait(5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()


def wait_for(path, seconds=20):
    deadline = time.monotonic() + seconds
    while not os.path.exists(path):
        if time.monotonic() > deadline:
            raise SystemExit(f"timed out waiting for {path}")
        time.sleep(0.05)


def measure(targets, seconds):
    before = {name: (cpu_ticks(p.pid), switches(p.pid)) for name, p in targets}
    start = time.monotonic()
    time.sleep(seconds)
    elapsed = time.monotonic() - start
    rows = []
    for name, p in targets:
        cpu0, sw0 = before[name]
        cpu_ms = (cpu_ticks(p.pid) - cpu0) * 1000 / TICK
        rows.append((name, cpu_ms, cpu_ms / (elapsed * 1000) * 100, (switches(p.pid) - sw0) / elapsed))
    return rows


def setup(kind, run, fonts):
    work = run.work
    config = os.path.join(work, "config")
    os.makedirs(os.path.join(config, "seinas"), exist_ok=True)
    base = {"XDG_CONFIG_HOME": config, "XDG_RUNTIME_DIR": work, "TZ": "UTC"}
    parent = run.start("standalone", ["seinas-standalone", "--fake", "1280x720", "--socket",
                                      os.path.join(work, "wl"), "--fonts", fonts], base)
    wait_for(os.path.join(work, "wl"))
    targets = []
    if kind == "standalone":
        apps, shell = os.path.join(work, "wl"), os.path.join(work, "wl-shell")
        targets.append(("compositor (standalone)", parent))
    else:
        nested = run.start("seinas", ["seinas", "--fonts", fonts],
                           dict(base, WAYLAND_DISPLAY=os.path.join(work, "wl")))
        apps, shell = os.path.join(work, "seinas-0"), os.path.join(work, "seinas-0-shell")
        wait_for(apps)
        targets.append(("compositor (nested seinas)", nested))
        targets.append(("parent (standalone)", parent))
    targets.append(("taskbar", run.start("taskbar", ["seinas-taskbar", "--fonts", fonts],
                                          dict(base, WAYLAND_DISPLAY=shell))))
    targets.append(("wallpaper", run.start("wallpaper", ["seinas-wallpaper"],
                                            dict(base, WAYLAND_DISPLAY=shell))))
    run.start("window", ["test-client", "window", "--title", "idle"], dict(base, WAYLAND_DISPLAY=apps))
    return targets


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--seconds", type=float, default=60)
    parser.add_argument("--profile", choices=["release", "debug"], default="release")
    parser.add_argument("--only", choices=["standalone", "nested"])
    args = parser.parse_args()
    build = ["cargo", "build", "--locked", "--quiet"] + sum((["-p", p] for p in PACKAGES), [])
    if args.profile == "release":
        build.append("--release")
    subprocess.run(build, cwd=ROOT, check=True)
    bin_dir = os.path.join(ROOT, "target", args.profile)
    fonts = os.path.join(ROOT, "target", "fonts")
    print(f"| 構成 | プロセス | CPU(ms / {args.seconds:g} 秒) | CPU(%) | 起きる回数(1 秒あたり) |")
    print("| --- | --- | --- | --- | --- |")
    for kind in [args.only] if args.only else ["standalone", "nested"]:
        work = os.path.join(tempfile.gettempdir(), f"seinas-idle-{os.getpid()}")
        os.makedirs(work)
        open(os.path.join(work, ".seinas-test"), "w").close()
        run = Run(bin_dir, work)
        try:
            targets = setup(kind, run, fonts)
            time.sleep(3)
            for name, cpu_ms, percent, wakeups in measure(targets, args.seconds):
                print(f"| {kind} | {name} | {cpu_ms:.0f} | {percent:.3f} | {wakeups:.2f} |", flush=True)
        finally:
            run.stop()
            shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
