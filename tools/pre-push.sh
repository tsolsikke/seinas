#!/bin/sh
# push の前の確かめを、変更の種類に応じた段で行う(docs/push-checks.md)。
#
# 使い方:
#     tools/pre-push.sh                 # origin/main との差から段を選ぶ
#     tools/pre-push.sh --tier full     # 段を手で上げる(下げることはできない)
#     tools/pre-push.sh --base <ref>    # 比べる相手を変える(既定は origin/main)
#     tools/pre-push.sh --dry-run       # 選んだ段と、行う確かめを出すだけ
#
# 段(下の段の確かめは、上の段にも入る):
#     docs    文書だけ:差分の空白、古い言い方、課題のキー、コミットメッセージの形
#     code    コードの変更:上に加えて fmt・clippy・ホストのテスト(golden を含む)
#     visual  見た目の変更:上に加えて golden の決まり(正解データとコードを同じコミットに混ぜていないか)
#     full    依存・ビルドの変更:上に加えて third-party・依存の検査・musl の静的ビルドとテスト
#
# 終わりの値:0 は全部通った、1 は落ちた確かめがある、2 は使い方の誤り。
# push の後は、tools/ci-wait.sh で CI の成功を確かめること。
set -eu
export LC_ALL=C.UTF-8

root=$(git rev-parse --show-toplevel)
cd "$root"

base=origin/main
asked=""
dry_run=false
while [ $# -gt 0 ]; do
    case $1 in
        --tier) asked=${2:?--tier needs docs, code, visual or full}; shift 2 ;;
        --base) base=${2:?--base needs a ref}; shift 2 ;;
        --dry-run) dry_run=true; shift ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "pre-push.sh: 知らない引数です: $1" >&2; exit 2 ;;
    esac
done

rank() {
    case $1 in
        none) echo 0 ;; docs) echo 1 ;; code) echo 2 ;; visual) echo 3 ;; full) echo 4 ;;
        *) echo "pre-push.sh: 段は docs、code、visual、full のどれかです: $1" >&2; exit 2 ;;
    esac
}

# 確かめるのは、コミットした内容。作業中の変更があると、テストが別のものを見ることになるので止める
# (除外したもの、たとえば docs/notes/ は数えない)。
if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
    echo "pre-push.sh: コミットしていない変更があります。コミットするか、片付けてから動かしてください:" >&2
    git status --short >&2
    exit 1
fi

range="$base..HEAD"
files=$(git diff --name-only "$base"...HEAD)
commits=$(git rev-list --reverse "$range")
if [ -z "$commits" ]; then
    echo "pre-push.sh: $base より先のコミットがありません。"
    exit 0
fi

# ファイルごとに段を決め、いちばん上の段を選ぶ。
tier_of() {
    case $1 in
        Cargo.toml|*/Cargo.toml|Cargo.lock|rust-toolchain.toml|.github/*|THIRD-PARTY/*|\
        tools/build-musl.sh|tools/build-musl-libs.sh|tools/third-party.py|tools/check-deps.sh|\
        tools/check-no-wayland.sh|tools/elf-report.sh|tools/fetch-fonts.sh|tools/release-*)
            echo full ;;
        crates/*/tests/golden/*|crates/*/tests/golden.rs|crates/seinas-standalone/tests/golden_*.rs|\
        crates/seinas-render/*|crates/seinas-text/*|crates/seinas-fbdev/*|\
        crates/seinas-frontend/src/decoration.rs|crates/seinas-frontend/src/title_bar.rs|\
        crates/seinas-frontend/src/lib.rs|crates/seinas-taskbar/src/lib.rs|crates/seinas-wallpaper/src/lib.rs)
            echo visual ;;
        *.md|LICENSE|docs/*)
            echo docs ;;
        *)
            echo code ;;
    esac
}

tier=none
reasons=""
for file in $files; do
    t=$(tier_of "$file")
    if [ "$(rank "$t")" -gt "$(rank "$tier")" ]; then
        tier=$t
    fi
    reasons="$reasons
  $t	$file"
done
[ "$tier" = none ] && tier=docs
if [ -n "$asked" ]; then
    if [ "$(rank "$asked")" -lt "$(rank "$tier")" ]; then
        echo "pre-push.sh: 段は下げられません(変更から選んだ段は $tier、指定は $asked)。" >&2
        exit 2
    fi
    tier=$asked
fi

echo "== $range の $(echo "$commits" | wc -l) コミット、$(echo "$files" | grep -c .) ファイル。段:$tier"
echo "ファイルごとの段:$reasons"
if $dry_run; then
    exit 0
fi

failed=""
started=$(date +%s)
log=$(mktemp)
trap 'rm -f "$log"' EXIT
# 1 つの確かめを行う。出力は、落ちたときと、伝えることがあるときだけ見せる(落ちたときは終わりの 40 行)。
step() {
    name=$1
    shift
    s=$(date +%s)
    if "$@" > "$log" 2>&1; then
        printf -- '-- %s:通った(%s 秒)\n' "$name" "$(( $(date +%s) - s ))"
        grep '^   ・' "$log" || true
    else
        printf -- '-- %s:落ちた(%s 秒)\n' "$name" "$(( $(date +%s) - s ))"
        tail -n 40 "$log" | sed 's/^/   /'
        failed="$failed
  $name"
    fi
}

# ---- docs の段(どの段でも行う)
added_lines() {
    git diff "$base"...HEAD -U0 -- "$@" | grep '^+' | grep -v '^+++' || true
}

check_whitespace() {
    git diff --check "$base"...HEAD
}

# 課題のキーは、コミットメッセージの 3 行目にだけ書く(コードと文書には書かない)。
# 例外:docs/zeikos-requirements.md は、ZeikOS の課題への当てはめを書く文書で、キーを書くように運用者から
# 頼まれたもの。
check_issue_keys() {
    hits=$(added_lines . ':!docs/zeikos-requirements.md' | grep -nE 'ZEIKOS-[0-9]+' || true)
    if [ -n "$hits" ]; then
        echo "   コードか文書に、課題のキーがあります:"
        echo "$hits" | sed 's/^/     /'
        return 1
    fi
}

# 古い言い方(docs/terminology.md)は、新しく書いた行に使わない。読み方の問題なので、表示するだけにする
# (同じ字を含む別の言葉に当たることがあるため)。
check_terms() {
    pattern='題名|帯|試験|画素|知らせ|席|手前に出す|選ばれている|受け口|裏側|描き直|控え|写す|写し|並び|掴|置き場所|中身|外形|後片付け|後始末|作業ツリー|道の名前|鍵のファイル|待ち受け|焦点'
    hits=$(added_lines . ':!docs/terminology.md' ':!tools/pre-push.sh' | grep -nE "$pattern" || true)
    if [ -n "$hits" ]; then
        echo "   ・新しく書いた行に、古い言い方かもしれない字があります(docs/terminology.md を見て、直すか、そのままでよいかを決める):"
        echo "$hits" | cut -c1-200 | sed 's/^/   ・  /'
    fi
    return 0
}

# コミットメッセージの形:1 行目が要約、2 行目が空、3 行目が課題のキー、4 行目が空、本文が 2〜5 行。
check_messages() {
    status=0
    for c in $commits; do
        message=$(git log -1 --format=%B "$c")
        line2=$(echo "$message" | sed -n 2p)
        line3=$(echo "$message" | sed -n 3p)
        line4=$(echo "$message" | sed -n 4p)
        body=$(echo "$message" | sed -n '5,$p' | grep -c . || true)
        if [ -n "$line2" ] || ! echo "$line3" | grep -qE '^ZEIKOS-[0-9]+$' || [ -n "$line4" ] \
            || [ "$body" -lt 2 ] || [ "$body" -gt 5 ]; then
            echo "   $(git log -1 --format='%h %s' "$c"):形が決まりと違います(空行、3 行目のキー、本文 2〜5 行)"
            status=1
        fi
    done
    return $status
}

step "差分の空白" check_whitespace
step "課題のキー(コードと文書)" check_issue_keys
step "古い言い方" check_terms
step "コミットメッセージの形" check_messages

# ---- code の段
if [ "$(rank "$tier")" -ge 2 ]; then
    step "fmt" cargo fmt --all -- --check
    step "clippy" cargo clippy --workspace --all-targets --locked -- -D warnings
    # golden も、ふだんの cargo test で動く(1 秒ほど)。
    step "ホストのテスト(golden を含む)" cargo test --workspace --locked
fi

# ---- visual の段
# 正解データの作り直しとコードの変更は、同じコミットに混ぜない(新しいケースを足すときは、混ぜてよい)。
check_golden_commits() {
    status=0
    for c in $commits; do
        changed=$(git diff-tree --no-commit-id --name-status -r "$c")
        modified_golden=$(echo "$changed" | grep -E '^M[[:space:]]+crates/[^/]+/tests/golden/' || true)
        code=$(echo "$changed" | grep -E '^[AMD][[:space:]]+crates/[^/]+/src/' || true)
        if [ -n "$modified_golden" ] && [ -n "$code" ]; then
            echo "   $(git log -1 --format='%h %s' "$c"):既存の正解データの作り直しと、コードの変更が同じコミットにあります(docs/golden.md)"
            status=1
        fi
    done
    return $status
}
if [ "$(rank "$tier")" -ge 3 ]; then
    step "golden の決まり(作り直しを別のコミットに)" check_golden_commits
fi

# ---- full の段
if [ "$(rank "$tier")" -ge 4 ]; then
    step "依存の一覧とライセンス文" python3 tools/third-party.py --check
    step "依存の検査" sh -c '
        tools/check-deps.sh seinas-standalone smithay-client-toolkit wayland-client calloop-wayland-source &&
        tools/check-deps.sh seinas-fbdev seinas-text cosmic-text swash fontdb &&
        tools/check-deps.sh seinas-render seinas-text cosmic-text swash fontdb &&
        tools/check-deps.sh seinas-standalone png && tools/check-deps.sh seinas png &&
        tools/check-deps.sh seinas-taskbar png' >/dev/null
    step "musl の静的ビルド" tools/build-musl.sh
    step "musl のテスト" env CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-L native=$root/target/musl-libs/lib" \
        cargo test --workspace --locked --target x86_64-unknown-linux-musl
    echo "SHA-256(CI の値と比べる):"
    (cd target/x86_64-unknown-linux-musl/release && sha256sum seinas seinas-standalone seinas-fbdev seinas-taskbar seinas-wallpaper zeyes-min)
fi

echo
echo "== 段 $tier、$(( $(date +%s) - started )) 秒"
if [ -n "$failed" ]; then
    echo "落ちた確かめ:$failed"
    echo "テストが落ちたまま push しないでください。"
    exit 1
fi
echo "全部通りました。push の後は tools/ci-wait.sh で CI の成功を確かめてください。"
