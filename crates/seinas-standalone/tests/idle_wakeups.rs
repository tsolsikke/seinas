//! 何もしていないときに、コンポジタが決まった間隔で起きないことを確かめる。
//!
//! 決まり:standalone も入れ子も、何か起きるまで(クライアント・親の Wayland・テスト用の入力インターフェースの
//! fd)か、期限(pong、IME の返事)まで待つ。何もしていない間に起きる回数は、1 秒に数回より少ない。
//!
//! 起きる回数は、/proc/<pid>/task/*/status の voluntary_ctxt_switches と nonvoluntary_ctxt_switches の差を、
//! 全部のスレッドで足したもの(眠っていたスレッドが起きるたびに 1 つ増える)。混んだ CI でも、眠っている
//! プロセスの回数は増えない。測り方は `tools/idle-cost.py` と同じ(`docs/standalone.md` の「何もしていないとき」)。

mod common;

use std::{fs, time::Duration};

use common::Compositor;

/// 測る時間と、その間に許す起きる回数(1 秒に 1 回より少ない)。
const MEASURE: Duration = Duration::from_secs(3);
const ALLOWED: u64 = 3;

fn switches(pid: u32) -> u64 {
    let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) else {
        return 0;
    };
    tasks
        .flatten()
        .filter_map(|task| fs::read_to_string(task.path().join("status")).ok())
        .flat_map(|status| {
            status
                .lines()
                .filter(|line| line.contains("ctxt_switches"))
                .filter_map(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok())
                .collect::<Vec<_>>()
        })
        .sum()
}

/// `pid` が、`MEASURE` の間に起きた回数。
fn wakeups(pid: u32) -> u64 {
    let before = switches(pid);
    std::thread::sleep(MEASURE);
    switches(pid) - before
}

#[test]
fn the_standalone_compositor_sleeps_while_idle() {
    let seinas = Compositor::start_with_input("iw");
    let window = seinas.connect_test_client(&["window", "--title", "idle"]);
    window.wait_for_line("window: drawn");
    std::thread::sleep(Duration::from_millis(500));
    let count = wakeups(seinas.process.0.id());
    assert!(
        count <= ALLOWED,
        "the compositor woke up {count} times in {MEASURE:?}"
    );
}

#[test]
fn the_nested_compositor_sleeps_while_idle() {
    let seinas = Compositor::start_with_input("in");
    let nested = seinas.start_nested(&[]);
    let window = seinas.connect_test_client_at(&nested.socket, &["window", "--title", "idle"]);
    window.wait_for_line("window: drawn");
    std::thread::sleep(Duration::from_millis(500));
    let count = wakeups(nested.process.0.id());
    // 入れ子の Seinas は、前は 16 ms ごとに起きていた(3 秒で約 190 回)。
    assert!(
        count <= ALLOWED,
        "the nested compositor woke up {count} times in {MEASURE:?}"
    );
}
