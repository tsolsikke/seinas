#!/usr/bin/env python3
"""第三者のRustクレートの一覧とライセンス文を、THIRD-PARTY/ の下に作る。

使い方:
    python3 tools/third-party.py           一覧とライセンス文を作り直す
    python3 tools/third-party.py --check   作り直した結果が、置いてあるものと同じかを確かめる

Cargo.lock のとおりに依存を調べる(cargo metadata --locked)。対象は、Linuxのx86-64(glibcとmusl)で
実際に使うクレートだけである。ライセンス文は、各クレートの配布物に入っているファイルをそのまま写す。
"""

import filecmp
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "THIRD-PARTY")
TARGETS = ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"]
LICENSE_PREFIXES = ("license", "licence", "copying", "copyright", "notice", "unlicense")


def metadata(target):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def classify(meta):
    """クレートを、実行ファイルに入るもの(runtime)と、ビルドのときだけ使うもの(build)に分ける。"""
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])

    def is_proc_macro(pkg_id):
        return any("proc-macro" in t["kind"] for t in packages[pkg_id]["targets"])

    runtime, build = set(), set()
    # (クレート, ビルドのときだけか) をたどる。テストだけの依存(dev)は、配布物に入らないので追わない。
    stack = [(m, False) for m in members]
    seen = set()
    while stack:
        pkg_id, build_only = stack.pop()
        if (pkg_id, build_only) in seen:
            continue
        seen.add((pkg_id, build_only))
        build_only = build_only or is_proc_macro(pkg_id)
        if pkg_id not in members:
            (build if build_only else runtime).add(pkg_id)
        for dep in nodes[pkg_id]["deps"]:
            kinds = {k["kind"] for k in dep["dep_kinds"]}
            if None in kinds:
                stack.append((dep["pkg"], build_only))
            elif "build" in kinds:
                stack.append((dep["pkg"], True))
    build -= runtime
    return packages, runtime, build


def license_files(package):
    directory = os.path.dirname(package["manifest_path"])
    names = [
        name
        for name in sorted(os.listdir(directory))
        if name.lower().startswith(LICENSE_PREFIXES) and os.path.isfile(os.path.join(directory, name))
    ]
    return directory, names


def generate(out):
    packages, runtime, build = {}, set(), set()
    for target in TARGETS:
        p, r, b = classify(metadata(target))
        packages.update(p)
        runtime |= r
        build |= b
    build -= runtime

    licenses_dir = os.path.join(out, "licenses")
    os.makedirs(licenses_dir)
    rows = []
    for pkg_id in sorted(runtime | build, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        package = packages[pkg_id]
        label = f'{package["name"]}-{package["version"]}'
        directory, names = license_files(package)
        if names:
            os.makedirs(os.path.join(licenses_dir, label))
            for name in names:
                shutil.copyfile(os.path.join(directory, name), os.path.join(licenses_dir, label, name))
            texts = f"[licenses/{label}/](licenses/{label}/)"
        else:
            texts = "配布物に入っていない"
        rows.append(
            "| {} | {} | {} | {} | {} |".format(
                package["name"],
                package["version"],
                package.get("license") or "(記載なし)",
                "実行ファイルに入る" if pkg_id in runtime else "ビルドのときだけ",
                texts,
            )
        )

    with open(os.path.join(out, "rust-crates.md"), "w", encoding="utf-8") as f:
        f.write("# Rustクレートの一覧\n\n")
        f.write("`tools/third-party.py` が作るファイルです。手では直さないでください。\n\n")
        f.write(
            "Cargo.lock で固定した版のうち、Linuxのx86-64(glibcとmusl)で使うものを載せています。"
            f"実行ファイルに入るものが{len(runtime)}個、ビルドのときだけ使うものが{len(build)}個です。\n\n"
        )
        f.write("| クレート | 版 | ライセンス | 使われ方 | ライセンス文 |\n")
        f.write("| --- | --- | --- | --- | --- |\n")
        f.write("\n".join(rows) + "\n")


def same_tree(a, b):
    cmp = filecmp.dircmp(a, b)
    if cmp.left_only or cmp.right_only or cmp.funny_files:
        return False
    _, mismatch, errors = filecmp.cmpfiles(a, b, cmp.common_files, shallow=False)
    if mismatch or errors:
        return False
    return all(same_tree(os.path.join(a, d), os.path.join(b, d)) for d in cmp.common_dirs)


def main():
    check = "--check" in sys.argv[1:]
    with tempfile.TemporaryDirectory() as tmp:
        generate(tmp)
        current_list = os.path.join(OUT, "rust-crates.md")
        current_licenses = os.path.join(OUT, "licenses")
        if check:
            ok = (
                os.path.isfile(current_list)
                and filecmp.cmp(os.path.join(tmp, "rust-crates.md"), current_list, shallow=False)
                and os.path.isdir(current_licenses)
                and same_tree(os.path.join(tmp, "licenses"), current_licenses)
            )
            if not ok:
                print("THIRD-PARTY/ が古くなっています。tools/third-party.py を実行して作り直してください。")
                return 1
            print("THIRD-PARTY/ は最新です。")
            return 0
        os.makedirs(OUT, exist_ok=True)
        shutil.rmtree(current_licenses, ignore_errors=True)
        shutil.copytree(os.path.join(tmp, "licenses"), current_licenses)
        shutil.copyfile(os.path.join(tmp, "rust-crates.md"), current_list)
    return 0


if __name__ == "__main__":
    sys.exit(main())
