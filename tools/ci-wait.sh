#!/bin/sh
# push の後に、HEAD の CI が終わるまで待ち、成功したかを確かめる(docs/push-checks.md)。
#
# 使い方:
#     tools/ci-wait.sh
#
# gh(GitHub CLI)を使う。CI が落ちたら、終わりの値 1 で、落ちたジョブと記録の見方を出す。落ちたらすぐ直し、
# 直せないときは revert してから報告する。
set -eu

sha=$(git rev-parse HEAD)
run=""
# push の直後は、まだ run が作られていないことがある。
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    run=$(gh run list --commit "$sha" --limit 1 --json databaseId --jq '.[0].databaseId // empty')
    [ -n "$run" ] && break
    sleep 5
done
if [ -z "$run" ]; then
    echo "ci-wait.sh: $sha の CI が見つかりません(push したか確かめてください)" >&2
    exit 1
fi

started=$(date +%s)
if gh run watch "$run" --exit-status > /dev/null; then
    echo "CI(run $run、${sha%"${sha#???????}"})は成功しました($(( $(date +%s) - started )) 秒待った)。"
    gh run view "$run" --json jobs --jq '.jobs[] | "  " + .name + ": " + .conclusion'
else
    echo "CI(run $run)が落ちました。すぐ直すか、直せなければ revert してください。" >&2
    gh run view "$run" --json jobs --jq '.jobs[] | "  " + .name + ": " + .conclusion' >&2
    echo "記録:gh run view $run --log-failed" >&2
    exit 1
fi
