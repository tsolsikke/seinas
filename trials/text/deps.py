#!/usr/bin/env python3
"""候補ごとに、実行ファイルに入る依存のクレートの数と、ライセンスの内訳を出す。

使い方(このディレクトリで):
    python3 deps.py

見るのは、Linuxのx86-64で、実行ファイルに入る依存だけ(ビルドのときだけ使うものと、手続きマクロは除く)。
土台(try-baseline)と、試しの共通の部分は、数に入れない。
"""

import collections
import json
import subprocess

TARGET = "x86_64-unknown-linux-gnu"
OWN = {"text-trial-common", "try-baseline", "try-cosmic", "try-swash", "try-fontdue"}


def main():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    for name in ["try-cosmic", "try-swash", "try-fontdue"]:
        root = next(i for i, p in packages.items() if p["name"] == name)
        seen, stack = set(), [root]
        while stack:
            pkg = stack.pop()
            if pkg in seen:
                continue
            seen.add(pkg)
            if any("proc-macro" in t["kind"] for t in packages[pkg]["targets"]):
                continue
            for dep in nodes[pkg]["deps"]:
                if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                    stack.append(dep["pkg"])
        deps = sorted(
            (packages[i] for i in seen if packages[i]["name"] not in OWN and not any(
                "proc-macro" in t["kind"] for t in packages[i]["targets"]
            )),
            key=lambda p: p["name"],
        )
        licenses = collections.Counter(p.get("license") or "(none)" for p in deps)
        links = [p["name"] for p in deps if p.get("links")]
        build_scripts = [p["name"] for p in deps if any("custom-build" in t["kind"] for t in p["targets"])]
        print(f"== {name}: {len(deps)} crates")
        for license, count in licenses.most_common():
            print(f"   {count:3}  {license}")
        print("   crates: " + ", ".join(f'{p["name"]} {p["version"]}' for p in deps))
        print("   links to a native library: " + (", ".join(links) or "none"))
        print("   with a build script: " + (", ".join(build_scripts) or "none"))


if __name__ == "__main__":
    main()
