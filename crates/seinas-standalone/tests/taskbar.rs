//! タスクバー(seinas-taskbar)を fake screen につなぎ、ピクセルで確かめる。
//!
//! 決まり(`docs/taskbar.md`):
//! - タスクバーは画面の下に固定され、exclusive zone で通常のウィンドウと重ならない。
//! - 開いているウィンドウを、map した順にボタンで並べ、activated のウィンドウのボタンは地の色が違う。
//! - ボタンを押すと、そのウィンドウが raise される。activated のウィンドウのボタンでは最小化し、最小化している
//!   ウィンドウのボタンでは戻す。
//! - シェルの部品のためのソケットでなければ、つなげない。

mod common;

use common::{
    bar_color, wait_for, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, HEIGHT, WIDTH,
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
