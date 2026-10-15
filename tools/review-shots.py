#!/usr/bin/env python3
"""Sprint Review 用のスクリーンショット(PNG)を、ヘッドレス(fake screen)で作る。

使い方(リポジトリの先頭から。フォントは tools/fetch-fonts.sh で取っておく):
    python3 tools/review-shots.py <出力先のディレクトリ>

seinas-standalone を fake screen(1280x720)で動かし、次の場面を順に作って、場面ごとに 1 枚の PNG を書く。
  1-desktop.png       壁紙・タスクバー(時計)・日本語のタイトルのウィンドウ 3 つ
  2-maximized.png     最大化したウィンドウ
  3-switcher.png      ウィンドウの切り替えの一覧(Alt+Tab の最中)
  4-ime.png           IME の変換中の文字(下線つき)と、候補の窓
  5-not-responding.png  応答なしのタイトルと閉じるボタン

設定(壁紙、時計)は一時的な XDG_CONFIG_HOME に置き、~/.config は読まない。子には、印の環境変数
SEINAS_TEST_CHILD と PR_SET_PDEATHSIG を付け、作業用の場所は終わるときに消す。最後に
tools/check-leftovers.py で、残りが 0 であることを確かめる(残っていれば、終わりの値は 1)。
"""

import ctypes
import os
import queue
import re
import shutil
import signal
import struct
import subprocess
import sys
import threading
import time
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WIDTH, HEIGHT = 1280, 720
TIMEOUT = 30

# evdev のキーの番号。
KEY_ESC, KEY_TAB, KEY_A, KEY_I, KEY_ALT = 1, 15, 30, 23, 56


def pdeathsig():
    ctypes.CDLL(None).prctl(1, signal.SIGKILL)  # PR_SET_PDEATHSIG


class Client:
    """起こしたプロセスと、標準出力の行。"""

    def __init__(self, args, env, log, stdin=True):
        self.lines = queue.Queue()
        self.proc = subprocess.Popen(args, env=env, stdin=subprocess.PIPE if stdin else subprocess.DEVNULL,
                                     stdout=subprocess.PIPE, stderr=log, text=True, preexec_fn=pdeathsig)
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            self.lines.put(line.rstrip("\n"))

    def wait_for(self, wanted, what=None):
        deadline = time.monotonic() + TIMEOUT
        while True:
            try:
                line = self.lines.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                raise SystemExit(f"review-shots: timed out waiting for {what or wanted}")
            if (wanted(line) if callable(wanted) else line == wanted):
                return line

    def send(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()


def ppm_to_png(source, target):
    data = open(source, "rb").read()
    match = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
    width, height = int(match.group(1)), int(match.group(2))
    pixels = data[match.end():]
    raw = b"".join(b"\0" + pixels[y * width * 3:(y + 1) * width * 3] for y in range(height))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    open(target, "wb").write(png)


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = os.path.abspath(sys.argv[1])
    os.makedirs(out, exist_ok=True)
    subprocess.run(["cargo", "build", "--locked", "--quiet", "-p", "seinas-standalone", "-p", "seinas-taskbar",
                    "-p", "seinas-wallpaper", "-p", "seinas-test-clients"], cwd=ROOT, check=True)
    bin_dir = os.path.join(ROOT, "target", "debug")
    fonts = os.path.join(ROOT, "target", "fonts")
    work = f"/tmp/seinas-review-{os.getpid()}"
    os.makedirs(os.path.join(work, "config", "seinas"))
    open(os.path.join(work, ".seinas-test"), "w").close()
    with open(os.path.join(work, "config", "seinas", "wallpaper.conf"), "w") as f:
        f.write("color = 2f5f7f\n")
    clients = []
    log = open(os.path.join(work, "log"), "w")
    env = dict(os.environ, SEINAS_TEST_CHILD=str(os.getpid()), XDG_CONFIG_HOME=os.path.join(work, "config"),
               XDG_RUNTIME_DIR=work, SEINAS_FONTS=fonts)
    dump = os.path.join(work, "screen.ppm")
    sock = os.path.join(work, "wl")

    def start(args, socket, stdin=True):
        client = Client([os.path.join(bin_dir, args[0])] + args[1:], dict(env, WAYLAND_DISPLAY=socket), log, stdin)
        clients.append(client)
        return client

    def shot(name, settle=0.8):
        # 描き終わるまで、少し待つ(fake screen は、描くたびに画面のコピーを書く)。
        time.sleep(settle)
        ppm_to_png(dump, os.path.join(out, name))
        print(f"review-shots: {os.path.join(out, name)}")

    try:
        seinas = start(["seinas-standalone", "--fake", f"{WIDTH}x{HEIGHT}", "--dump", dump, "--socket", sock,
                        "--fonts", fonts, "--test-input", "--ping-timeout", "1000"], sock)
        deadline = time.monotonic() + TIMEOUT
        while not os.path.exists(sock):
            if time.monotonic() > deadline:
                raise SystemExit("review-shots: the compositor did not start")
            time.sleep(0.05)

        def key(code, pressed):
            seinas.send(f"key {code} {'press' if pressed else 'release'}")

        def tap(code):
            key(code, True)
            key(code, False)

        start(["seinas-wallpaper"], sock + "-shell", stdin=False)
        start(["seinas-taskbar", "--fonts", fonts, "--utc-offset", "+09:00"], sock + "-shell", stdin=False)
        list_client = start(["test-client", "toplevels"], sock + "-shell")
        list_client.wait_for("toplevels: ready")
        windows = [
            start(["test-client", "window", "--ignore-ping", "--follow-size", "--size", "360x200",
                   "--title", "応答しないアプリ", "--color", "c04848"], sock),
            start(["test-client", "window", "--keyboard", "--follow-size", "--size", "360x200",
                   "--title", "メモ(キーボード)", "--color", "3c6cc0"], sock),
            start(["test-client", "window", "--show-text", "--follow-size", "--size", "420x200",
                   "--title", "文字入力(text-input)", "--color", "f0a040"], sock),
        ]
        for window in windows:
            window.wait_for("window: drawn")
        shot("1-desktop.png", settle=1.5)

        def click(x, y):
            seinas.send(f"motion {x} {y}")
            seinas.send("press")
            seinas.send("release")
            time.sleep(0.3)

        # 2. 最大化(メモのウィンドウ。タイトルバーを押して raise してから)。
        memo = windows[1]
        click(150, 44)
        memo.send("maximize")
        memo.wait_for(lambda line: line.startswith("window: resized"), "the maximized size")
        shot("2-maximized.png")
        memo.send("unmaximize")
        memo.wait_for(lambda line: line.startswith("window: resized"), "the restored size")

        # 3. 切り替えの一覧(Alt を押したまま Tab を 1 回)。Esc で取り消してから Alt を離す(スタックを変えない)。
        key(KEY_ALT, True)
        tap(KEY_TAB)
        shot("3-switcher.png")
        tap(KEY_ESC)
        key(KEY_ALT, False)

        # 4. IME:文字入力のウィンドウを raise し(タイトルバーの、メモに隠れていない所を押す)、a と i を打つ。
        text = windows[2]
        click(400, 76)
        ime = start(["test-client", "ime", "--popup", "150x56", "--popup-text"], sock + "-ime")
        ime.wait_for("ime: activate")
        ime.wait_for("ime: done")
        tap(KEY_A)
        tap(KEY_I)
        text.wait_for("window: preedit あい")
        shot("4-ime.png")
        # 確定して、IME を終える(候補の窓を消す)。
        seinas.send("key 28 press")
        seinas.send("key 28 release")
        text.wait_for("window: commit あい")
        ime.send("quit")

        # 5. 応答なし:応答しないアプリ(左上、スロット 0)の閉じるボタンを押す。ping に答えないので、1 秒で印が付く。
        seinas.send(f"motion {360 - 12} 12")
        seinas.send("press")
        seinas.send("release")
        windows[0].wait_for("window: ping")
        # ポインターを外す(乗せたままだと、閉じるボタンがふつうの赤になる)。
        seinas.send("motion 800 400")
        shot("5-not-responding.png", settle=1.8)
    finally:
        for client in reversed(clients):
            client.proc.terminate()
        for client in clients:
            try:
                client.proc.wait(5)
            except subprocess.TimeoutExpired:
                client.proc.kill()
                client.proc.wait()
        shutil.rmtree(work, ignore_errors=True)
    return subprocess.run([sys.executable, os.path.join(ROOT, "tools", "check-leftovers.py")]).returncode


if __name__ == "__main__":
    sys.exit(main())
