//! layer-shell(wlr-layer-shell)のフロントエンドを、fake screen のピクセルと、テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md`、`docs/adr/0002-shell-components.md`):
//! - layer-shell の global は、シェルの部品のためのソケット(ふだんのパスに `-shell` を付けたもの)から
//!   つないだクライアントにだけ見える。
//! - exclusive zone を持つ layer surface がある間、使える範囲は、その分だけ狭まる。新しいウィンドウの
//!   スロットと configure の大きさ、ドラッグで外へ出せる範囲は、使える範囲を基準にする。
//! - top と overlay の layer surface の上でのボタンは、その下のウィンドウを raise しない。

mod common;

use common::{
    bar_color, upper_bar_color, wait_for, Compositor, ACTIVE_BAR, BAR, HEIGHT, INACTIVE_BAR, WIDTH,
};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// タスクバーの代わりの layer surface の色(青)と、高さ。
const PANEL: [u8; 3] = [0x20, 0x40, 0xc0];
const PANEL_SIZE: usize = 40;

#[test]
fn the_layer_shell_is_offered_only_on_the_shell_socket() {
    let seinas = Compositor::start("la");
    // ふだんのソケットからは、layer-shell の global が見えない。クライアントは準備の段階で失敗する。
    let mut ordinary =
        seinas.connect_test_client(&["layer", "--anchor", "top,left,right", "--size", "0x40"]);
    let status = wait_for("the ordinary client to give up", || {
        ordinary.process.0.try_wait().expect("check the client")
    });
    assert_eq!(status.code(), Some(2));
    // シェルの部品のためのソケットからは、見える。
    let shell =
        seinas.connect_shell_client(&["layer", "--anchor", "top,left,right", "--size", "0x40"]);
    assert_eq!(
        shell.next_line("the configure"),
        format!("layer: configure {WIDTH}x40")
    );
    shell.wait_for_line("layer: drawn");
}

#[test]
fn an_exclusive_zone_shrinks_the_usable_area() {
    let seinas = Compositor::start("lb");
    let panel = seinas.connect_shell_client(&[
        "layer",
        "--anchor",
        "top,left,right",
        "--size",
        "0x40",
        "--exclusive",
        "40",
        "--color",
        "2040c0",
    ]);
    panel.wait_for_line("layer: drawn");

    // 新しいウィンドウは、使える範囲の左上(パネルの下)に置かれ、configure の大きさもその分だけ小さい。
    let window = seinas.connect_test_client(&["window"]);
    assert_eq!(
        window.next_configure_states("the first configure"),
        "[activated]"
    );
    seinas.wait_for_screen("the window below the panel", |shot| {
        shot.pixel(2, 2) == PANEL
            && shot.pixel(2, PANEL_SIZE - 1) == PANEL
            && bar_color(shot, 0, PANEL_SIZE, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && shot.pixel(2, PANEL_SIZE + BAR) == ORANGE
    });

    // パネルが去ると、使える範囲が戻り、スロットにあるウィンドウは画面の左上へ戻る。
    drop(panel.process);
    seinas.wait_for_screen("the window back at the top left", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR) && shot.pixel(2, BAR) == ORANGE
    });
}

#[test]
fn the_first_configure_tells_the_usable_size() {
    let seinas = Compositor::start("lc");
    let panel = seinas.connect_shell_client(&[
        "layer",
        "--anchor",
        "bottom,left,right",
        "--size",
        "0x40",
        "--exclusive",
        "40",
    ]);
    panel.wait_for_line("layer: drawn");
    // 最初の configure の大きさは 0x0(クライアントに任せる)。使える範囲より大きく開いたら、画面(640x480)から
    // パネル(40)とタイトルバー(36)を引いた大きさに収めるよう、configure し直す。
    let window = seinas.connect_test_client(&["window", "--size", "1000x1000"]);
    assert_eq!(
        window.next_line("the first configure"),
        "window: wm_capabilities [maximize]"
    );
    assert_eq!(
        window.next_line("the first configure"),
        "window: configure 0x0 [activated]"
    );
    assert_eq!(
        window.next_line("the configure to fit"),
        format!(
            "window: configure {WIDTH}x{} [activated]",
            HEIGHT - PANEL_SIZE - BAR
        )
    );
}

#[test]
fn a_dragged_window_stays_inside_the_usable_area() {
    let mut seinas = Compositor::start_with_input("ld");
    let top = seinas.connect_shell_client(&[
        "layer",
        "--anchor",
        "top,left,right",
        "--size",
        "0x40",
        "--exclusive",
        "40",
        "--color",
        "2040c0",
    ]);
    top.wait_for_line("layer: drawn");
    let bottom = seinas.connect_shell_client(&[
        "layer",
        "--anchor",
        "bottom,left,right",
        "--size",
        "0x40",
        "--exclusive",
        "40",
        "--color",
        "2040c0",
    ]);
    bottom.wait_for_line("layer: drawn");
    let window = seinas.connect_test_client(&["window"]);
    window.wait_for_line("window: drawn");
    seinas.wait_for_screen("the window below the top panel", |shot| {
        bar_color(shot, 0, PANEL_SIZE, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });

    // 上へ引っ張っても、タイトルバーは上のパネルの下で止まる。
    seinas.pointer("motion 100 50");
    seinas.pointer("press");
    seinas.pointer("motion 100 -500");
    seinas.wait_for_screen("the window stopped below the top panel", |shot| {
        shot.pixel(100, PANEL_SIZE - 1) == PANEL && shot.pixel(100, PANEL_SIZE) == ACTIVE_BAR
    });
    // 下へ引っ張ると、タイトルバーの全体が、下のパネルの上に残る。
    seinas.pointer("motion 100 5000");
    let bar_top = HEIGHT - PANEL_SIZE - BAR;
    seinas.wait_for_screen("the window stopped above the bottom panel", |shot| {
        shot.pixel(100, bar_top) == ACTIVE_BAR
            && shot.pixel(100, bar_top + BAR - 1) == ACTIVE_BAR
            && shot.pixel(100, HEIGHT - PANEL_SIZE) == PANEL
    });
    seinas.pointer("release");
}

#[test]
fn a_press_on_a_top_layer_does_not_raise_the_window_below() {
    let mut seinas = Compositor::start_with_input("le");
    let back = seinas.connect_test_client(&["window"]);
    back.wait_for_line("window: drawn");
    let front = seinas.connect_test_client(&["window"]);
    front.wait_for_line("window: drawn");
    // 左の端に、幅 20 の top の layer surface。exclusive zone は持たない(-1)ので、ウィンドウは動かない。
    let panel = seinas.connect_shell_client(&[
        "layer",
        "--layer",
        "top",
        "--anchor",
        "left,top,bottom",
        "--size",
        "20x0",
        "--exclusive",
        "-1",
        "--color",
        "2040c0",
    ]);
    panel.wait_for_line("layer: drawn");
    seinas.wait_for_screen("the panel over the back window", |shot| {
        shot.pixel(10, 10) == PANEL && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
    });

    // パネルの上(奥のウィンドウのタイトルバーの上でもある)を押しても、奥のウィンドウは raise されない。
    seinas.pointer("motion 10 10");
    seinas.pointer("press");
    seinas.pointer("release");
    // パネルの外のタイトルバーを押すと、raise される。それが見えるまで待つ間に、上の押した結果も描かれている。
    let shot = seinas.shot();
    assert_eq!(upper_bar_color(&shot, 0, 0, WINDOW_WIDTH), INACTIVE_BAR);
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("release");
    seinas.wait_for_screen("the back window raised", |shot| {
        upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == ACTIVE_BAR && shot.pixel(10, 10) == PANEL
    });
    assert!(seinas.process.is_running());
}
