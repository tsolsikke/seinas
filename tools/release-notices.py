#!/usr/bin/env python3
"""1つの実行ファイルに入る、第三者のライセンス文と著作権表示を、1つのディレクトリにまとめる。

使い方:
    python3 tools/release-notices.py <パッケージ名> <実行ファイル> <出力先>

例(musl向けのseinas-fbdevに入るもの):
    python3 tools/release-notices.py seinas-fbdev target/x86_64-unknown-linux-musl/release/seinas-fbdev target/release-assets/third-party

対象は、x86_64-unknown-linux-musl 向けの静的ビルドで、そのパッケージの実行ファイルに入るものである。
Rustのクレートは Cargo.lock のとおりに調べ(cargo metadata --locked)、ライセンス文は THIRD-PARTY/ に
置いてあるものを写す(THIRD-PARTY/ が最新であること。tools/third-party.py --check で確かめられる)。
Cのライブラリ(pixman、libxkbcommon、musl)とRustの標準ライブラリの分も、あわせて入れる。
Cのライブラリは、リンクの指定があっても、使われなければ実行ファイルに入らない(リンカーが、参照されない
部分を落とす)。そこで、実行ファイルのシンボルを見て、実際に入っているものだけを入れる。
"""

import importlib.util
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
THIRD_PARTY = os.path.join(ROOT, "THIRD-PARTY")
TARGET = "x86_64-unknown-linux-musl"

# 静的にリンクするCのライブラリ。THIRD-PARTY/README.md の「Cのライブラリ」と同じもの。
# 最後の要素は、そのライブラリの関数のシンボルの頭(実行ファイルに入っているかを見るのに使う)。
# Noneなら、いつも入っているものとして扱う。
C_LIBRARIES = [
    ("pixman (libpixman-1)", "0.42.2", "MIT", "pixman-0.42.2", "画素の合成", "pixman_"),
    ("libxkbcommon", "1.6.0", "MIT(X11系の表示を含む)", "libxkbcommon-1.6.0", "キーボードの配列", "xkb_"),
    ("musl", "1.2.5", "MIT", "musl-1.2.5", "Cの標準ライブラリ(Rustのmusl向けターゲットに同梱のもの)", None),
]

# 配布物にライセンス文が無く、上流からも取れないクレート。THIRD-PARTY/README.md と同じ説明。
NO_LICENSE_FILE = {
    "pixman": "上流のリポジトリ(https://github.com/cmeissl/pixman-rs)にもライセンス文のファイルが無い。Cargo.toml にMITと書かれていて、作者はChristian Meisslさん",
    "pixman-sys": "上流のリポジトリ(https://github.com/cmeissl/pixman-rs)にもライセンス文のファイルが無い。Cargo.toml にMITと書かれていて、作者はChristian Meisslさん",
}


def load_third_party_module():
    path = os.path.join(ROOT, "tools", "third-party.py")
    spec = importlib.util.spec_from_file_location("third_party", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def runtime_crates(third_party, package_name):
    """`package_name` の実行ファイルに入る、第三者のクレート(名前と版の順)。

    cargo tree で、そのパッケージだけを起点に依存を解決する(cargo metadata は、ワークスペース全体で
    機能をまとめて解決するので、このパッケージには入らないものまで数えてしまう)。
    ビルドのときだけ使うもの(build、proc-macro)は入れない。
    """
    out = subprocess.run(
        [
            "cargo", "tree", "--locked", "-p", package_name, "--target", TARGET,
            "-e", "normal,no-proc-macro", "--prefix", "none", "--format", "{p}",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    wanted = set()
    for line in out.splitlines():
        words = line.split()
        if len(words) >= 2 and words[1].startswith("v"):
            wanted.add((words[0], words[1][1:]))
    meta = third_party.metadata(TARGET)
    members = {p["id"] for p in meta["packages"] if p["id"] in meta["workspace_members"]}
    crates = [
        p for p in meta["packages"]
        if (p["name"], p["version"]) in wanted and p["id"] not in members
    ]
    return sorted(crates, key=lambda p: (p["name"], p["version"]))


def copy_license_dir(label, out):
    """THIRD-PARTY/licenses/ か licenses-upstream/ にある `label` のライセンス文を写す。写した場所を返す。"""
    for kind in ("licenses", "licenses-upstream"):
        src = os.path.join(THIRD_PARTY, kind, label)
        if os.path.isdir(src):
            dst = os.path.join(out, "rust-crates", label)
            shutil.copytree(src, dst)
            return f"rust-crates/{label}/"
    return None


def linked_c_libraries(binary):
    """実行ファイルに実際に入っているCのライブラリ。シンボルの表(nm)で確かめる。"""
    symbols = subprocess.run(["nm", binary], check=True, capture_output=True, text=True).stdout
    defined = {
        line.split()[2]
        for line in symbols.splitlines()
        if len(line.split()) == 3 and line.split()[1] in ("T", "t")
    }
    libraries = []
    for library in C_LIBRARIES:
        prefix = library[5]
        if prefix is None or any(name.startswith(prefix) for name in defined):
            libraries.append(library)
    return libraries


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    package_name, binary, out = sys.argv[1], sys.argv[2], os.path.abspath(sys.argv[3])
    third_party = load_third_party_module()
    crates = runtime_crates(third_party, package_name)
    c_libraries = linked_c_libraries(binary)

    if os.path.exists(out):
        shutil.rmtree(out)
    os.makedirs(os.path.join(out, "rust-crates"))
    shutil.copyfile(os.path.join(ROOT, "LICENSE"), os.path.join(out, "LICENSE"))

    rows = []
    for package in crates:
        label = f'{package["name"]}-{package["version"]}'
        where = copy_license_dir(label, out)
        if where is None:
            note = NO_LICENSE_FILE.get(package["name"])
            if note is None:
                sys.exit(f"{label} のライセンス文が THIRD-PARTY/ にありません。tools/third-party.py を実行してください")
            where = note
        rows.append((package["name"], package["version"], package.get("license") or "(記載なし)", where))

    for _, _, _, directory, _, _ in c_libraries:
        shutil.copytree(os.path.join(THIRD_PARTY, "c-libraries", directory), os.path.join(out, "c-libraries", directory))

    with open(os.path.join(out, "NOTICES.md"), "w", encoding="utf-8") as f:
        f.write(f"# {package_name} に入っているソフトウェアのライセンス文と著作権表示\n\n")
        f.write(f"`tools/release-notices.py` が作るファイルです。対象は、`{TARGET}` 向けに静的にリンクした `{package_name}` です。\n\n")
        f.write("## Seinas 自身\n\nMITライセンスです。`LICENSE` を見てください。\n\n")
        f.write(f"## Rustのクレート({len(rows)}個)\n\n")
        f.write("| クレート | 版 | ライセンス | ライセンス文 |\n| --- | --- | --- | --- |\n")
        for name, version, license_, where in rows:
            f.write(f"| {name} | {version} | {license_} | {where} |\n")
        f.write("\n複数のライセンスから選べるクレートは、MIT、Apache-2.0、Zlibのどれかで使います。\n\n")
        f.write("## Cのライブラリ(静的にリンクしたもの)\n\n")
        f.write("| ライブラリ | 版 | ライセンス | 使われ方 | ライセンス文 |\n| --- | --- | --- | --- | --- |\n")
        for name, version, license_, directory, use, _ in c_libraries:
            files = ", ".join(sorted(os.listdir(os.path.join(out, "c-libraries", directory))))
            f.write(f"| {name} | {version} | {license_} | {use} | c-libraries/{directory}/({files}) |\n")
        skipped = [name for name, *_ in C_LIBRARIES if name not in {l[0] for l in c_libraries}]
        if skipped:
            f.write(f"\nリンクの指定はあるが、使われないので実行ファイルに入っていないもの: {', '.join(skipped)}\n")
        f.write("\n## Rustの標準ライブラリ\n\n")
        f.write("Rustの標準ライブラリ(MIT OR Apache-2.0)と、Rustに同梱のlibunwind(Apache-2.0 WITH LLVM-exception)が入っています。\n")
    print(f"{out}: Rustのクレート{len(rows)}個、Cのライブラリ{len(c_libraries)}個")


if __name__ == "__main__":
    main()
