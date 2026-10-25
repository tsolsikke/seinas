//! タスクバー(seinas-taskbar)を fake screen につなぎ、ピクセルで確かめる。
//!
//! 決まり(`docs/taskbar.md`):
//! - タスクバーは画面の下に固定され、exclusive zone で通常のウィンドウと重ならない。
//! - トーチと、開いているウィンドウ(タスク)のアイコンを、map した順に中央に並べる。activated のウィンドウの
//!   アイコンの下にはオレンジの線、そのほか(最小化しているものを含む)には点を付ける(`docs/ui-spec.md`)。
//! - タスクを押すと、そのウィンドウが raise される。activated のウィンドウのタスクでは最小化し、最小化している
//!   ウィンドウのタスクでは戻す。トーチを押すと、ログに 1 行出す(メニューはまだ無い)。
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

use seinas_theme::taskbar::{
    ACTIVE_MARK, BACKGROUND as TASKBAR_BACKGROUND, DOT, HEIGHT as TASKBAR_HEIGHT, MARK_GAP,
    MARK_HEIGHT, PITCH, TOP_LINE,
};

/// タスクバーの高さ。
const TASKBAR: usize = TASKBAR_HEIGHT as usize;
/// テスト用のクライアントのウィンドウの幅と、2 つ目のスロットの位置。
const WINDOW_WIDTH: usize = 200;
const STEP: usize = 32;

/// タスクバーの上の端の y。
const TOP: usize = HEIGHT - TASKBAR;

/// タスクが `count` 個のとき、`index` 番目(トーチが 0、タスク `i` が `i + 1`)のアイコンの中心の x。640 の幅では、
/// まとまりは時計の場所にかからず、ちょうど中央に並ぶ。
fn center(count: usize, index: usize) -> usize {
    let pitch = PITCH as usize;
    (WIDTH - (count + 1) * pitch) / 2 + index * pitch + pitch / 2
}

/// タスク `index` の印の色(線か点の中ほど。印が無ければタスクバーの地)。
fn mark(shot: &Shot, count: usize, index: usize) -> [u8; 3] {
    let y = TASKBAR_HEIGHT - MARK_GAP - MARK_HEIGHT + 1;
    shot.pixel(center(count, index + 1), TOP + y as usize)
}

/// タスク `index` のアイコンが描かれているか(アイコンの範囲に、地の色でないピクセルがあるか)。
fn has_icon(shot: &Shot, count: usize, index: usize) -> bool {
    let x = center(count, index + 1);
    (TOP + 10..TOP + 30).any(|y| (x - 10..x + 10).any(|x| shot.pixel(x, y) != TASKBAR_BACKGROUND))
}

fn open_window(seinas: &Compositor, title: &str) -> TestClient {
    let window = seinas.connect_test_client(&["window", "--title", title]);
    window.wait_for_line("window: drawn");
    window
}

/// タスク `index` を押して離す。
fn press_task(seinas: &mut Compositor, count: usize, index: usize) {
    seinas.pointer(&format!(
        "motion {} {}",
        center(count, index + 1),
        TOP + TASKBAR / 2
    ));
    seinas.pointer("press");
    seinas.pointer("release");
}

#[test]
fn the_taskbar_sits_at_the_bottom_and_shrinks_the_usable_area() {
    let seinas = Compositor::start("ta");
    let _taskbar = seinas.connect_taskbar();
    seinas.wait_for_screen("the taskbar at the bottom", |shot| {
        shot.pixel(0, TOP) == TOP_LINE
            && shot.pixel(0, TOP + 1) == TASKBAR_BACKGROUND
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
    // 2 つのタスクが、アイコンつきで並ぶ。activated の 2 つ目にはオレンジの線、1 つ目には点。
    seinas.wait_for_screen("two tasks, the second activated", |shot| {
        mark(shot, 2, 0) == DOT
            && mark(shot, 2, 1) == ACTIVE_MARK
            && has_icon(shot, 2, 0)
            && has_icon(shot, 2, 1)
    });

    // 1 つ目のタスクを押すと、1 つ目のウィンドウが raise され、印も入れ替わる。
    press_task(&mut seinas, 2, 0);
    seinas.wait_for_screen("the first window raised", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && bar_color(shot, STEP, STEP, WINDOW_WIDTH) != Some(ACTIVE_BAR)
            && mark(shot, 2, 0) == ACTIVE_MARK
            && mark(shot, 2, 1) == DOT
    });
}

#[test]
fn a_closed_window_leaves_the_taskbar() {
    let seinas = Compositor::start("tc");
    let _taskbar = seinas.connect_taskbar();
    let first = open_window(&seinas, "一つ目");
    seinas.wait_for_screen("one task", |shot| mark(shot, 1, 0) == ACTIVE_MARK);
    drop(first);
    // タスクが無くなると、トーチだけが中央に残り、タスクの場所は空く。
    seinas.wait_for_screen("no tasks", |shot| {
        mark(shot, 1, 0) == TASKBAR_BACKGROUND
            && !has_icon(shot, 1, 0)
            && shot.pixel(center(0, 0), TOP + TASKBAR / 2) != TASKBAR_BACKGROUND
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
    seinas.wait_for_screen("the window and its task", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR) && mark(shot, 1, 0) == ACTIVE_MARK
    });

    // activated のウィンドウのタスクを押すと、最小化する。ウィンドウは描かれず、印は点になる(最小化していても、
    // 開いているウィンドウなので点を付ける)。印を見るため、ポインターはタスクの外へ出しておく。
    press_task(&mut seinas, 1, 0);
    seinas.pointer("motion 400 200");
    seinas.wait_for_screen("the window minimized", |shot| {
        shot.pixel(2, 30) == BACKGROUND && mark(shot, 1, 0) == DOT && has_icon(shot, 1, 0)
    });
    // もう一度押すと、戻って activated になる。
    press_task(&mut seinas, 1, 0);
    seinas.wait_for_screen("the window restored", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR) && mark(shot, 1, 0) == ACTIVE_MARK
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
        shot.pixel(0, TOP) == TOP_LINE && shot.pixel(0, TOP + 1) == TASKBAR_BACKGROUND
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
        shot.pixel(0, TOP) == TOP_LINE && shot.pixel(0, TOP + 1) == TASKBAR_BACKGROUND
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

/// 既定のアイコンの中の色(`seinas-icons` の `DEFAULT_ICON_BODY`)。
fn seinas_icons_default_body() -> [u8; 3] {
    [0x22, 0x30, 0x4a]
}

/// タスクバーのログに `text` が出るまで待つ。
fn wait_for_taskbar_log(seinas: &Compositor, text: &str) {
    wait_for(text, || seinas.taskbar_log().contains(text).then_some(()));
}

#[test]
fn the_taskbar_shows_app_icons_a_fixed_clock_and_logs_the_torch() {
    let mut seinas = Compositor::start_with_input("tg");
    // 見本の画像と同じ時刻(2026-10-26 20:45 +09:00)に止める。
    let _taskbar = seinas.connect_taskbar_to(
        &seinas.shell_socket(),
        &["--utc-offset", "+09:00", "--fixed-time", "1793015100"],
    );
    wait_for_taskbar_log(&seinas, "the clock is fixed at 1793015100 seconds");
    // .desktop の Icon から、テスト用のアイコンの置き場のフォルダーのアイコンを見つける。
    let _files = seinas.connect_test_client(&["window", "--app-id", "seinas.test.files"]);
    wait_for_taskbar_log(&seinas, "the icon of seinas.test.files: ");
    assert!(seinas.taskbar_log().contains("seinas-test-folder.png"));
    // アイコンの無い app_id は、既定のアイコン。
    let _missing = seinas.connect_test_client(&["window", "--app-id", "seinas.test.missing"]);
    wait_for_taskbar_log(
        &seinas,
        "no icon for seinas.test.missing; the default icon is used",
    );
    // PNG は、まだ読まないので、どちらも既定のアイコン(中ほど、アイコンの左上から (12, 14) は、中の色)。
    let body = seinas_icons_default_body();
    seinas.wait_for_screen("the default icons", |shot| {
        shot.pixel(center(2, 1), TOP + 8 + 14) == body
            && shot.pixel(center(2, 2), TOP + 8 + 14) == body
    });
    // トーチを押すと、ログに 1 行出る(メニューはまだ無い)。ポインターが乗った地と、押した地が付く。
    seinas.pointer(&format!("motion {} {}", center(2, 0), TOP + 3));
    seinas.pointer("press");
    wait_for_taskbar_log(&seinas, "the torch was pressed (no menu yet)");
    seinas.wait_for_screen("the pressed torch", |shot| {
        shot.pixel(center(2, 0) - 16, TOP + 4) == seinas_theme::taskbar::PRESSED
    });
    seinas.pointer("release");
    seinas.wait_for_screen("the hot torch", |shot| {
        shot.pixel(center(2, 0) - 16, TOP + 4) == seinas_theme::taskbar::HOT
    });
}
