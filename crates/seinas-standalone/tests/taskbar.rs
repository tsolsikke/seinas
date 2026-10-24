//! タスクバー(seinas-taskbar)を fake screen につなぎ、ピクセルで確かめる。
//!
//! 決まり(`docs/taskbar.md`):
//! - タスクバーは画面の下に固定され、exclusive zone で通常のウィンドウと重ならない。
//! - 開いているウィンドウを、map した順にボタンで並べ、activated のウィンドウのボタンは地の色が違う。
//! - ボタンを押すと、そのウィンドウが raise される。activated のウィンドウのボタンでは最小化し、最小化している
//!   ウィンドウのボタンでは戻す。
//! - シェルの部品のためのソケットでなければ、つなげない。
//! - コンポジタより先に起こされても、コンポジタのソケットができるまで試し直してつなぐ。期限まで試しても
//!   できなければ、`the compositor did not appear` のエラーで終わる。
//! - つないだ後の最初の往復は、期限内に遅れて答えがあれば起動する。答えが無ければ `the initialization timed out`
//!   で終わる。往復の途中の切断とプロトコルエラーは、段階と原因を書いて、試し直さずに終わる。

mod common;

use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixListener,
    time::Duration,
};

use common::{
    bar_color, compositor_dir, start_taskbar, wait_for, Compositor, Shot, TestClient, ACTIVE_BAR,
    BACKGROUND, HEIGHT, WIDTH,
};

/// タスクバーの高さと、ボタンの上の端・高さ・幅(ウィンドウが 2 つのとき、640 の幅で 200)。
const TASKBAR: usize = 40;
const BUTTON_TOP: usize = 4;
const BUTTON_WIDTH: usize = 200;
/// ボタンの左の端。
const BUTTONS: [usize; 2] = [4, 208];
/// タスクバーの地と、activated のウィンドウのボタンの地(seinas-taskbar の色)。
const TASKBAR_BACKGROUND: [u8; 3] = [0x20, 0x24, 0x2c];
const ACTIVE_BUTTON: [u8; 3] = [0x3a, 0x40, 0x4c];
/// テスト用のクライアントのウィンドウの幅と、2 つ目のスロットの位置。
const WINDOW_WIDTH: usize = 200;
const STEP: usize = 32;

/// タスクバーの上の端の y。
const TOP: usize = HEIGHT - TASKBAR;

/// ボタン `index` の地の色(左上の隅。文字も印も来ない所)。
fn button_ground(shot: &Shot, index: usize) -> [u8; 3] {
    shot.pixel(BUTTONS[index] + 1, TOP + BUTTON_TOP + 1)
}

/// ボタン `index` の中に、地の色でないピクセル(タイトルの文字)があるか。
fn button_has_text(shot: &Shot, index: usize) -> bool {
    let ground = button_ground(shot, index);
    (TOP + BUTTON_TOP..TOP + BUTTON_TOP + 24)
        .any(|y| (BUTTONS[index] + 10..BUTTONS[index] + 80).any(|x| shot.pixel(x, y) != ground))
}

fn open_window(seinas: &Compositor, title: &str) -> TestClient {
    let window = seinas.connect_test_client(&["window", "--title", title]);
    window.wait_for_line("window: drawn");
    window
}

#[test]
fn the_taskbar_sits_at_the_bottom_and_shrinks_the_usable_area() {
    let seinas = Compositor::start("ta");
    let _taskbar = seinas.connect_taskbar();
    seinas.wait_for_screen("the taskbar at the bottom", |shot| {
        shot.pixel(0, TOP) == TASKBAR_BACKGROUND
            && shot.pixel(WIDTH - 1, HEIGHT - 1) != [0x19, 0x1e, 0x28]
            && shot.pixel(WIDTH / 2, TOP - 1) == [0x19, 0x1e, 0x28]
    });
    // 新しいウィンドウには、タスクバーの分を除いた大きさを伝える(640x480 - 40 - タイトルバー 24)。
    let window = seinas.connect_test_client(&["window"]);
    // タスクバーがあるので、最小化もできる(順は決まっていない)。
    let capabilities = window.next_line("the capabilities");
    assert!(
        [
            "window: wm_capabilities [maximize,minimize]",
            "window: wm_capabilities [minimize,maximize]"
        ]
        .contains(&capabilities.as_str()),
        "{capabilities}"
    );
    assert_eq!(
        window.next_line("the first configure"),
        format!(
            "window: configure {WIDTH}x{} [activated]",
            HEIGHT - TASKBAR - 24
        )
    );
}

#[test]
fn the_taskbar_lists_the_windows_and_raises_the_one_pressed() {
    let mut seinas = Compositor::start_with_input("tb");
    let _taskbar = seinas.connect_taskbar();
    let _first = open_window(&seinas, "一つ目");
    let _second = open_window(&seinas, "二つ目");
    // 2 つのボタンが、タイトルつきで並ぶ。activated の 2 つ目のボタンだけ、地の色が違う。
    seinas.wait_for_screen("two buttons, the second activated", |shot| {
        button_ground(shot, 0) == TASKBAR_BACKGROUND
            && button_ground(shot, 1) == ACTIVE_BUTTON
            && button_has_text(shot, 0)
            && button_has_text(shot, 1)
    });

    // 1 つ目のボタンを押すと、1 つ目のウィンドウが raise され、ボタンの地も入れ替わる。
    seinas.pointer(&format!(
        "motion {} {}",
        BUTTONS[0] + BUTTON_WIDTH / 2,
        TOP + 20
    ));
    seinas.pointer("press");
    seinas.pointer("release");
    seinas.wait_for_screen("the first window raised", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && bar_color(shot, STEP, STEP, WINDOW_WIDTH) != Some(ACTIVE_BAR)
            && button_ground(shot, 0) == ACTIVE_BUTTON
            && button_ground(shot, 1) != ACTIVE_BUTTON
    });
}

#[test]
fn a_closed_window_leaves_the_taskbar() {
    let seinas = Compositor::start("tc");
    let _taskbar = seinas.connect_taskbar();
    let first = open_window(&seinas, "一つ目");
    seinas.wait_for_screen("one button", |shot| button_ground(shot, 0) == ACTIVE_BUTTON);
    drop(first);
    seinas.wait_for_screen("no buttons", |shot| {
        button_ground(shot, 0) == TASKBAR_BACKGROUND && !button_has_text(shot, 0)
    });
}

#[test]
fn the_taskbar_needs_the_shell_socket() {
    let seinas = Compositor::start("td");
    let mut taskbar = seinas.connect_taskbar_to(&seinas.socket, &[]);
    let status = wait_for("the taskbar to give up", || {
        taskbar.0.try_wait().expect("check the taskbar")
    });
    assert!(!status.success());
    assert!(
        seinas
            .taskbar_log()
            .contains("does not offer zwlr_layer_shell_v1"),
        "{}",
        seinas.taskbar_log()
    );
}

#[test]
fn the_taskbar_minimizes_and_restores_a_window() {
    let mut seinas = Compositor::start_with_input("te");
    let _taskbar = seinas.connect_taskbar();
    let _first = open_window(&seinas, "一つ目");
    let indicator = |shot: &Shot| shot.pixel(BUTTONS[0] + BUTTON_WIDTH / 2, TOP + BUTTON_TOP + 30);
    seinas.wait_for_screen("the window and its button", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && button_ground(shot, 0) == ACTIVE_BUTTON
    });
    let press_button = |seinas: &mut Compositor| {
        seinas.pointer(&format!(
            "motion {} {}",
            BUTTONS[0] + BUTTON_WIDTH / 2,
            TOP + 20
        ));
        seinas.pointer("press");
        seinas.pointer("release");
    };

    // activated のウィンドウのボタンを押すと、最小化する。ウィンドウは描かれず、ボタンの地と印が消える
    // (地の色を見るため、ポインターはボタンの外へ出しておく)。
    press_button(&mut seinas);
    seinas.pointer("motion 400 200");
    seinas.wait_for_screen("the window minimized", |shot| {
        shot.pixel(2, 30) == BACKGROUND
            && button_ground(shot, 0) == TASKBAR_BACKGROUND
            && indicator(shot) == TASKBAR_BACKGROUND
            && button_has_text(shot, 0)
    });
    // もう一度押すと、戻って activated になる。
    press_button(&mut seinas);
    seinas.wait_for_screen("the window restored", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && button_ground(shot, 0) == ACTIVE_BUTTON
            && indicator(shot) != TASKBAR_BACKGROUND
    });
}

#[test]
fn the_clock_offset_comes_from_the_config_file_or_the_argument() {
    let seinas = Compositor::start("tf");
    let wait_for_log =
        |text: &str| wait_for(text, || seinas.taskbar_log().contains(text).then_some(()));
    // 設定が無ければ、既定の +09:00。
    let taskbar = seinas.connect_taskbar_to(&seinas.shell_socket(), &[]);
    wait_for_log("the clock is UTC+09:00 (from the default)");
    drop(taskbar);
    // taskbar.conf の 1 行で、UTC にする。
    let dir = seinas.config_dir().join("seinas");
    std::fs::create_dir_all(&dir).expect("the config directory");
    std::fs::write(dir.join("taskbar.conf"), "utc-offset = UTC\n").expect("the config");
    let taskbar = seinas.connect_taskbar_to(&seinas.shell_socket(), &[]);
    wait_for_log("the clock is UTC+00:00 (from taskbar.conf)");
    drop(taskbar);
    // 起動オプションは、設定より優先する。
    let taskbar = seinas.connect_taskbar_to(&seinas.shell_socket(), &["--utc-offset", "-05:30"]);
    wait_for_log("the clock is UTC-05:30 (from --utc-offset)");
    drop(taskbar);
    // 読めない値は、ログに出して既定で動く。
    std::fs::write(dir.join("taskbar.conf"), "utc-offset = Asia/Tokyo\n").expect("the config");
    let _taskbar = seinas.connect_taskbar_to(&seinas.shell_socket(), &[]);
    wait_for_log("utc-offset must be UTC or from -12:00 to +14:00");
    wait_for_log("the clock is UTC+09:00 (from the default)");
}

#[test]
fn a_taskbar_started_before_the_compositor_connects_when_the_socket_appears() {
    // ZeikOS の起動の順序(`a | b` の左が先に起きる)と同じく、タスクバーを先に起こす。
    let dir = compositor_dir("tg");
    let mut taskbar = start_taskbar(&dir, &dir.join("wl-shell"), &["--utc-offset", "+00:00"]);
    // 1 回目の接続に失敗したことを、ログで確かめてから、コンポジタを起こす(時間には頼らない)。
    let log = || fs::read_to_string(dir.join("taskbar.log")).unwrap_or_default();
    // 試し直さずに終わったら、すぐに落とす。
    wait_for("the first try to fail", || {
        let text = log();
        let failed_once = text.contains("the compositor is not ready at");
        assert!(failed_once || taskbar.is_running(), "{text}");
        failed_once.then_some(())
    });
    assert!(taskbar.is_running(), "{}", log());
    assert!(log().contains("(ENOENT: "), "{}", log());
    let seinas = Compositor::start("tg");
    seinas.wait_for_screen("the taskbar at the bottom", |shot| {
        shot.pixel(0, TOP) == TASKBAR_BACKGROUND
    });
    assert!(
        seinas
            .taskbar_log()
            .contains("connected to the compositor after"),
        "{}",
        seinas.taskbar_log()
    );
    assert!(taskbar.is_running());
}

#[test]
fn a_taskbar_gives_up_when_the_compositor_does_not_appear() {
    // 残ったソケットのファイル(listen する相手がいない。ECONNREFUSED)へつなごうとする。
    let dir = compositor_dir("th");
    let stale = dir.join("stale");
    drop(UnixListener::bind(&stale).expect("bind a socket"));
    let mut taskbar = start_taskbar(&dir, &stale, &["--connect-timeout", "300"]);
    let code = exit_code(&mut taskbar);
    let log = fs::read_to_string(dir.join("taskbar.log")).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(code, Some(1), "{log}");
    assert!(
        log.contains("the compositor is not ready at") && log.contains("(ECONNREFUSED: "),
        "{log}"
    );
    assert!(
        log.contains("the compositor did not appear at") && log.contains(" within 300 ms ("),
        "{log}"
    );
}

/// コンポジタのプロセスを止める(`SIGSTOP`)か、動かし直す(`SIGCONT`)。止めている間、listen のソケットは残るので、
/// connect は通るが、答えは返らない。
fn signal(seinas: &Compositor, signal: libc::c_int) {
    let pid = seinas.process.0.id() as libc::pid_t;
    // SAFETY: 自分が起こした子のプロセスへ、シグナルを送るだけ。
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
}

/// タスクバーが終わるのを待ち、終わりの値とログを返す。
fn taskbar_ends(seinas: &Compositor, taskbar: &mut common::Running) -> (Option<i32>, String) {
    (exit_code(taskbar), seinas.taskbar_log())
}

/// タスクバーが終わるのを待ち、終わりの値を返す。終わらなければ、待つ上限で落ちる(待ち続けない)。
fn exit_code(taskbar: &mut common::Running) -> Option<i32> {
    wait_for("the taskbar to end", || {
        taskbar.0.try_wait().expect("look at the taskbar")
    })
    .code()
}

#[test]
fn a_taskbar_starts_when_the_first_round_trip_is_answered_late() {
    let seinas = Compositor::start("ti");
    signal(&seinas, libc::SIGSTOP);
    let mut taskbar = seinas.connect_taskbar();
    // 答えを待ち始めたことを、ログで確かめてから、コンポジタを動かし直す(時間には頼らない)。
    wait_for("the taskbar to wait for the first round trip", || {
        let log = seinas.taskbar_log();
        assert!(taskbar.is_running(), "{log}");
        log.contains(
            "connected; waiting up to 30000 ms for the compositor to answer the first round trip",
        )
        .then_some(())
    });
    signal(&seinas, libc::SIGCONT);
    seinas.wait_for_screen("the taskbar at the bottom", |shot| {
        shot.pixel(0, TOP) == TASKBAR_BACKGROUND
    });
    let log = seinas.taskbar_log();
    assert!(
        log.contains("the compositor answered the first round trip"),
        "{log}"
    );
    assert!(taskbar.is_running(), "{log}");
}

#[test]
fn a_taskbar_ends_with_a_timeout_when_the_first_round_trip_is_not_answered() {
    let seinas = Compositor::start("tj");
    signal(&seinas, libc::SIGSTOP);
    let mut taskbar =
        seinas.connect_taskbar_to(&seinas.shell_socket(), &["--round-trip-timeout", "500"]);
    let (code, log) = taskbar_ends(&seinas, &mut taskbar);
    signal(&seinas, libc::SIGCONT);
    assert_eq!(code, Some(1), "{log}");
    assert!(
        log.contains(
            "seinas-taskbar: the compositor did not answer the first round trip within 500 ms (the initialization timed out)"
        ),
        "{log}"
    );
}

/// テストの側の listener に 1 つだけつながせ、最初の要求を読んでから `answer` を書いて閉じる。終わった後、2 つ目の
/// 接続が来ていない(試し直していない)ことを確かめて、タスクバーのログを返す。
fn serve_once(name: &str, answer: &[u8]) -> (Option<i32>, String) {
    let dir = compositor_dir(name);
    let path = dir.join("fake");
    let listener = UnixListener::bind(&path).expect("bind a socket");
    let mut taskbar = start_taskbar(&dir, &path, &[]);
    let (mut stream, _) = listener.accept().expect("accept the taskbar");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set a read timeout");
    let mut request = [0u8; 64];
    let read = stream.read(&mut request).expect("read the first request");
    assert!(read >= 8, "a short request: {read}");
    stream.write_all(answer).expect("write the answer");
    drop(stream);
    let code = exit_code(&mut taskbar);
    listener.set_nonblocking(true).expect("set non-blocking");
    let again = listener.accept();
    let log = fs::read_to_string(dir.join("taskbar.log")).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    assert!(again.is_err(), "the taskbar connected again: {log}");
    (code, log)
}

/// `wl_display.error`(object 1、opcode 0)のイベントのバイト列。
fn display_error(code: u32, message: &str) -> Vec<u8> {
    let mut text = message.as_bytes().to_vec();
    text.push(0);
    let length = text.len() as u32;
    while !text.len().is_multiple_of(4) {
        text.push(0);
    }
    let size = (8 + 4 + 4 + 4 + text.len()) as u32;
    let mut bytes = Vec::new();
    for word in [1, size << 16, 1, code, length] {
        bytes.extend_from_slice(&u32::to_le_bytes(word));
    }
    bytes.extend_from_slice(&text);
    bytes
}

#[test]
fn a_disconnection_during_the_first_round_trip_is_named_with_its_stage_and_cause() {
    let (code, log) = serve_once("tk", &[]);
    assert_eq!(code, Some(1), "{log}");
    let line = log
        .lines()
        .find(|line| {
            line.contains("the connection to the compositor failed during the first round trip: ")
        })
        .unwrap_or_else(|| panic!("{log}"));
    // 原因(OS のエラーなど)が、段階の後に続く。
    assert!(
        line.len() > line.find("round trip: ").unwrap() + 12,
        "{line}"
    );
}

#[test]
fn a_protocol_error_during_the_first_round_trip_is_not_hidden_by_a_retry() {
    let (code, log) = serve_once("tl", &display_error(3, "a test protocol error"));
    assert_eq!(code, Some(1), "{log}");
    assert!(
        log.contains(
            "the connection to the compositor failed during the first round trip: protocol error 3 on wl_display@1: a test protocol error"
        ),
        "{log}"
    );
}
