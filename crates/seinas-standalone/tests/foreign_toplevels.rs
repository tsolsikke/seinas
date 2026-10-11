//! ウィンドウの一覧と操作のプロトコル(foreign-toplevel)を、テスト用のクライアントの出力と fake screen のピクセルで
//! 確かめる。
//!
//! 決まり(`docs/window-placement.md`、`docs/adr/0002-shell-components.md`):
//! - 2 つの global(wlr の manager と ext の一覧)は、シェルの部品のためのソケットからつないだクライアントに
//!   だけ見える。
//! - 一覧に載るのは、絵を出したウィンドウだけ。タイトル・app_id・activated かどうかが変わると送り、
//!   去ったウィンドウには closed を送る。
//! - activate はウィンドウを raise して activated にし、close はクライアントに閉じるように頼む。
//! - 最大化・最小化と、その戻しの頼みに応じ、状態(maximized、minimized)を送る。全画面の頼みは受けるが、今は何もしない。

mod common;

use common::{
    bar_color, wait_for, Compositor, TestClient, ACTIVE_BAR, BACKGROUND, HEIGHT, INACTIVE_BAR,
    WIDTH,
};

/// テスト用のクライアントのコンテンツの色(橙)。
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];

/// テスト用のクライアントのウィンドウの、コンテンツの幅。
const WINDOW_WIDTH: usize = 200;
/// 2 つ目のウィンドウのスロット(スロット 1)の位置。
const STEP: usize = 32;

/// 一覧の 1 行。
fn toplevel(number: usize, title: &str, app_id: &str, activated: bool, outputs: usize) -> String {
    let state = if activated { "activated" } else { "" };
    format!(
        "toplevel {number}: title={title:?} app_id={app_id:?} state=[{state}] outputs={outputs}"
    )
}

/// 一覧を受け取るクライアントをシェルのソケットへつなぎ、最初の一覧を受け取り終えるまで待つ。
fn connect_list(seinas: &Compositor, args: &[&str]) -> TestClient {
    let list = seinas.connect_shell_client(&[&["toplevels"], args].concat());
    list.wait_for_line("toplevels: ready");
    list
}

/// ウィンドウを出し、描かれるまで待つ。
fn open_window(seinas: &Compositor, args: &[&str]) -> TestClient {
    let window = seinas.connect_test_client(&[&["window"], args].concat());
    window.wait_for_line("window: drawn");
    window
}

#[test]
fn the_lists_are_offered_only_on_the_shell_socket() {
    let seinas = Compositor::start("fa");
    // ふだんのソケットからは、wlr の manager も ext の一覧も見えない。クライアントは準備の段階で失敗する。
    for args in [&["toplevels"][..], &["toplevels", "--ext-only"][..]] {
        let mut ordinary = seinas.connect_test_client(args);
        let status = wait_for("the ordinary client to give up", || {
            ordinary.process.0.try_wait().expect("check the client")
        });
        assert_eq!(status.code(), Some(2), "{args:?}");
    }
    // シェルの部品のためのソケットからは、両方見える。
    connect_list(&seinas, &["--ext"]);
}

#[test]
fn the_list_follows_the_windows() {
    let seinas = Compositor::start("fb");
    let list = connect_list(&seinas, &[]);

    // 絵を出したときに、タイトルと app_id がそろった形で載る。
    let _first = open_window(&seinas, &["--title", "一つ目", "--app-id", "one"]);
    assert_eq!(
        list.next_line("the first window"),
        toplevel(1, "一つ目", "one", true, 1)
    );

    // 新しいウィンドウが activated になり、前のものは外れる。
    let mut second = open_window(&seinas, &["--title", "二つ目"]);
    assert_eq!(
        list.next_line("the first window, deactivated"),
        toplevel(1, "一つ目", "one", false, 1)
    );
    assert_eq!(
        list.next_line("the second window"),
        toplevel(2, "二つ目", "", true, 1)
    );

    // タイトルを変えると、送る。
    second.send_line("title 新しいタイトル");
    assert_eq!(
        list.next_line("the new title"),
        toplevel(2, "新しいタイトル", "", true, 1)
    );

    // 去ると closed が届き、その下のウィンドウが activated になる。
    drop(second);
    assert_eq!(list.next_line("closed"), "toplevel 2: closed");
    assert_eq!(
        list.next_line("the first window, activated again"),
        toplevel(1, "一つ目", "one", true, 1)
    );
}

#[test]
fn the_ext_list_follows_the_windows() {
    let seinas = Compositor::start("fc");
    let list = connect_list(&seinas, &["--ext-only"]);
    let window = open_window(&seinas, &["--title", "一つ目", "--app-id", "one"]);
    list.wait_for_line(r#"list 1: title="一つ目" app_id="one" identifier=32"#);
    drop(window);
    list.wait_for_line("list 1: closed");
}

#[test]
fn windows_that_already_exist_are_listed_when_bound() {
    let seinas = Compositor::start("fd");
    let _first = open_window(&seinas, &["--title", "一つ目"]);
    let _second = open_window(&seinas, &["--title", "二つ目"]);
    // 順は、絵を出した順。
    let list = seinas.connect_shell_client(&["toplevels"]);
    assert_eq!(list.next_line("1"), toplevel(1, "一つ目", "", false, 1));
    assert_eq!(list.next_line("2"), toplevel(2, "二つ目", "", true, 1));
    assert_eq!(list.next_line("ready"), "toplevels: ready");

    // wl_output を後から bind したクライアントには、そのときに output_enter が届く。
    let late = seinas.connect_shell_client(&["toplevels", "--output-after"]);
    assert_eq!(late.next_line("1"), toplevel(1, "一つ目", "", false, 0));
    assert_eq!(late.next_line("2"), toplevel(2, "二つ目", "", true, 0));
    assert_eq!(
        late.next_line("1 enters"),
        toplevel(1, "一つ目", "", false, 1)
    );
    assert_eq!(
        late.next_line("2 enters"),
        toplevel(2, "二つ目", "", true, 1)
    );
    assert_eq!(late.next_line("ready"), "toplevels: ready");
}

#[test]
fn activate_raises_the_window() {
    let seinas = Compositor::start_with_input("fe");
    let first = open_window(&seinas, &["--title", "一つ目"]);
    let _second = open_window(&seinas, &["--title", "二つ目", "--color", "00a0ff"]);
    assert_eq!(first.next_configure_states("deactivated"), "[]");
    let mut list = connect_list(&seinas, &[]);
    seinas.wait_for_screen("the second window in front", |shot| {
        bar_color(shot, STEP, STEP, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });

    list.send_line("activate 1");
    assert_eq!(list.next_line("1"), toplevel(1, "一つ目", "", true, 1));
    assert_eq!(list.next_line("2"), toplevel(2, "二つ目", "", false, 1));
    assert_eq!(list.next_line("synced"), "toplevels: synced");
    assert_eq!(first.next_configure_states("the configure"), "[activated]");
    seinas.wait_for_screen("the first window in front", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && bar_color(shot, STEP, STEP, WINDOW_WIDTH) == Some(INACTIVE_BAR)
    });

    // すでに手前にあるウィンドウへの activate では、何も変わらない。
    list.send_line("activate 1");
    assert_eq!(list.next_line("synced"), "toplevels: synced");
}

#[test]
fn close_asks_the_client() {
    let seinas = Compositor::start("ff");
    let stubborn = open_window(&seinas, &["--title", "応じない", "--ignore-close"]);
    let polite = open_window(&seinas, &["--title", "応じる"]);
    let mut list = connect_list(&seinas, &[]);

    // 応じないクライアントには頼みが届くだけで、一覧から消えない。
    list.send_line("close 1");
    stubborn.wait_for_line("window: close");
    assert_eq!(list.next_line("synced"), "toplevels: synced");

    // 応じるクライアントは終わり、closed が届く。
    list.send_line("close 2");
    polite.wait_for_line("window: close");
    list.wait_for_line("toplevel 2: closed");
    assert_eq!(
        list.next_line("the first window, activated"),
        toplevel(1, "応じない", "", true, 1)
    );
}

#[test]
fn maximize_and_minimize_requests_change_the_window() {
    let seinas = Compositor::start("fg");
    let _window = open_window(&seinas, &["--title", "一つ目", "--follow-size"]);
    let mut list = connect_list(&seinas, &[]);
    let line = |states: &str| {
        format!("toplevel 1: title=\"一つ目\" app_id=\"\" state=[{states}] outputs=1")
    };

    // 最大化すると、使える範囲いっぱいになる(クライアントが従えば)。状態に maximized が付く。
    list.send_line("maximize 1");
    assert_eq!(list.next_line("maximized"), line("maximized,activated"));
    assert_eq!(list.next_line("synced"), "toplevels: synced");
    seinas.wait_for_screen("the maximized window", |shot| {
        bar_color(shot, 0, 0, WIDTH) == Some(ACTIVE_BAR)
            && shot.pixel(WIDTH - 1, HEIGHT - 1) == ORANGE
    });
    list.send_line("unmaximize 1");
    assert_eq!(list.next_line("restored"), line("activated"));
    assert_eq!(list.next_line("synced"), "toplevels: synced");
    seinas.wait_for_screen("the restored window", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && shot.pixel(WINDOW_WIDTH + 2, 2) == BACKGROUND
    });

    // 最小化すると、描かれなくなり、activated も外れる。戻すと、また activated になる。
    list.send_line("minimize 1");
    assert_eq!(list.next_line("minimized"), line("minimized"));
    assert_eq!(list.next_line("synced"), "toplevels: synced");
    seinas.wait_for_screen("no window", |shot| shot.pixel(2, 2) == BACKGROUND);
    list.send_line("unminimize 1");
    assert_eq!(list.next_line("unminimized"), line("activated"));
    assert_eq!(list.next_line("synced"), "toplevels: synced");
    seinas.wait_for_screen("the window again", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });

    // 全画面は、まだ持っていない。頼みは受けるが、何もしない。
    for command in ["fullscreen", "unfullscreen"] {
        list.send_line(&format!("{command} 1"));
        assert_eq!(list.next_line(command), "toplevels: synced");
    }
    let log = seinas.log();
    assert!(log.contains("SetFullscreen") && log.contains("is not supported yet"));
}

#[test]
fn a_negative_rectangle_is_a_protocol_error() {
    let mut seinas = Compositor::start("fh");
    let _window = open_window(&seinas, &["--title", "一つ目"]);
    let mut list = connect_list(&seinas, &[]);
    // 場所を覚える(0x0 は、消す)。
    list.send_line("rectangle 1 10 20 100 30");
    assert_eq!(list.next_line("a rectangle"), "toplevels: synced");
    list.send_line("rectangle 1 0 0 0 0");
    assert_eq!(list.next_line("no rectangle"), "toplevels: synced");
    // 幅か高さが負なら、プロトコルのエラーで切る。
    list.send_line("rectangle 1 0 0 -1 30");
    let line = list.next_line("the protocol error");
    assert!(
        line.starts_with("toplevels: protocol error: ")
            && line.ends_with("code 0, \"the width and the height must not be negative\""),
        "{line}"
    );
    // コンポジタは動き続け、ウィンドウも残る。
    assert!(seinas.process.is_running());
    let again = seinas.connect_shell_client(&["toplevels"]);
    assert_eq!(again.next_line("1"), toplevel(1, "一つ目", "", true, 1));
}

#[test]
fn stop_ends_the_announcements() {
    let seinas = Compositor::start_with_input("fi");
    let _first = open_window(&seinas, &["--title", "一つ目"]);
    let mut list = seinas.connect_shell_client(&["toplevels"]);
    assert_eq!(list.next_line("1"), toplevel(1, "一つ目", "", true, 1));
    assert_eq!(list.next_line("ready"), "toplevels: ready");
    list.send_line("stop");
    assert_eq!(list.next_line("finished"), "toplevels: finished");
    assert_eq!(list.next_line("synced"), "toplevels: synced");

    // 新しいウィンドウは知らされない。前からある handle には、状態の変化が届き続ける。
    let _second = open_window(&seinas, &["--title", "二つ目"]);
    assert_eq!(
        list.next_line("deactivated"),
        toplevel(1, "一つ目", "", false, 1)
    );
    list.send_line("activate 1");
    assert_eq!(
        list.next_line("activated"),
        toplevel(1, "一つ目", "", true, 1)
    );
    assert_eq!(list.next_line("synced"), "toplevels: synced");
}
