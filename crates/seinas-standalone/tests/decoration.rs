//! 装飾(タイトルバー・枠・角・影)を、ウィンドウごとに xdg-decoration の取り決めで決めることを、ピクセルとテスト用の
//! クライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「飾りを描く側の取り決め」):
//! - 装飾のオブジェクトを作っていないウィンドウには、Seinas は何も付けない(クライアントが自分で描く)。
//! - 作ったウィンドウには、いつも server_side と答え(client_side を求めても)、Seinas が描く。
//! - 描き始めるのは、クライアントがその configure に応じた後の commit から。
//! - クライアントが決めたウィンドウの幾何(set_window_geometry)の外(自分で描く影など)は、ウィンドウの大きさに
//!   数えない。
//! - ウィンドウを動かす修飾キー(既定は Alt)を押しながら、ウィンドウのどこでもドラッグすると、左のボタンで動き、
//!   右のボタンで大きさが変わる。
//!
//! テスト用のクライアント(`test-client window`)のウィンドウは、コンテンツが 200x150 の橙。

mod common;

use common::{
    bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, AROUND, BACKGROUND, BAR, FRAME_RADIUS,
};

const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// テスト用のクライアントの `--margin` の縁の色(灰色)。
const MARGIN: [u8; 3] = [0x50, 0x50, 0x50];
/// 左の Alt キー(evdev の番号)。
const KEY_LEFT_ALT: u32 = 56;

fn connect(seinas: &Compositor, args: &[&str]) -> TestClient {
    let client = seinas.connect_test_client(&[&["window"], args].concat());
    client.wait_for_line("window: drawn");
    client
}

/// 外枠の左上が (`x0`, `y0`) のウィンドウが、Seinas の飾りなしで描かれているか。コンテンツが外枠の左上から始まり、
/// まわり(タイトルバー・枠・影の来る所)は背景のまま。
fn shows_bare_window(shot: &Shot, x0: usize, y0: usize) -> bool {
    shot.pixel(x0, y0) == ORANGE
        && shot.pixel(x0 + WINDOW_WIDTH - 1, y0 + WINDOW_HEIGHT - 1) == ORANGE
        && shot.pixel(x0 + WINDOW_WIDTH, y0 + 10) == BACKGROUND
        && shot.pixel(x0 + WINDOW_WIDTH + 4, y0 + 10) == BACKGROUND
        && shot.pixel(x0 + 10, y0 + WINDOW_HEIGHT) == BACKGROUND
        && shot.pixel(x0 + 10, y0 + WINDOW_HEIGHT + 4) == BACKGROUND
}

/// 外枠の左上が (`x0`, `y0`) のウィンドウが、Seinas のタイトルバーつきで描かれているか。
fn shows_decorated_window(shot: &Shot, x0: usize, y0: usize) -> bool {
    bar_color(shot, x0, y0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
        && shot.pixel(x0 + 2, y0 + BAR) == ORANGE
        && shot.pixel(
            x0 + WINDOW_WIDTH - 1,
            y0 + BAR + WINDOW_HEIGHT - 1 - FRAME_RADIUS,
        ) == ORANGE
}

#[test]
fn a_window_without_a_decoration_object_gets_no_decoration() {
    let mut seinas = Compositor::start_with_input("dn");
    let mut client = connect(&seinas, &["--no-decoration", "--pointer"]);
    // 最初の configure では、タイトルバーのぶんを引かない大きさを伝える。
    seinas.wait_for_screen("the bare window", |shot| shows_bare_window(shot, 0, 0));
    // 上の端もコンテンツなので、押すとクライアントへ渡る(Seinas のタイトルバーではない)。
    seinas.pointer("motion 100 5");
    client.wait_for_line("window: pointer enter 100 5");
    seinas.pointer("press");
    client.wait_for_line("window: pointer button 0x110 pressed");
    seinas.pointer("release");
    client.send_line("sync");
    client.wait_for_line("window: synced");
    // 角も欠かない(左上の点も、このウィンドウ)。
    seinas.pointer("motion 0 0");
    client.wait_for_line("window: pointer motion 0 0");
}

#[test]
fn a_window_that_asks_for_server_side_is_decorated() {
    let seinas = Compositor::start("ds");
    let client = seinas.connect_test_client(&["window", "--decoration"]);
    client.wait_for_line("window: decoration server-side");
    client.wait_for_line("window: drawn");
    seinas.wait_for_screen("the decorated window", |shot| {
        shows_decorated_window(shot, 0, 0)
    });
}

#[test]
fn a_window_that_asks_for_client_side_is_still_decorated_by_seinas() {
    let seinas = Compositor::start("dc");
    // client_side を求めても、Seinas は server_side と答え(xdg-decoration の決まりで、コンポジタは違うモードを
    // 選んでよい)、自分で描く。クライアントは、その答えに従って飾りを描かない。
    let client = seinas.connect_test_client(&["window", "--client-side", "--decoration"]);
    client.wait_for_line("window: decoration server-side");
    client.wait_for_line("window: drawn");
    seinas.wait_for_screen("the decorated window", |shot| {
        shows_decorated_window(shot, 0, 0)
    });
}

#[test]
fn a_decoration_asked_later_starts_after_the_client_answers() {
    let seinas = Compositor::start("dl");
    let mut client = connect(&seinas, &["--no-decoration", "--decoration"]);
    seinas.wait_for_screen("the bare window", |shot| shows_bare_window(shot, 0, 0));
    // 後から装飾のオブジェクトを作り、server_side を求める。答えの configure に応じて commit したら、タイトルバーが
    // 付き、コンテンツはその下へ移る。
    client.send_line("decoration server-side");
    client.wait_for_line("window: decoration server-side");
    client.wait_for_line("window: decoration committed");
    seinas.wait_for_screen("the decorated window", |shot| {
        shows_decorated_window(shot, 0, 0)
    });
}

#[test]
fn the_shadow_of_a_client_is_not_counted_in_the_window() {
    let mut seinas = Compositor::start_with_input("dg");
    // 幅 10 の灰色の縁(自分で描く影のまね)を描き、縁を除いた所をウィンドウの幾何として伝える。
    let client = connect(&seinas, &["--no-decoration", "--margin", "10", "--pointer"]);
    // ウィンドウ(橙)は外枠の左上 (0, 0) から。縁は、その外(右と下)に見えるが、ウィンドウには数えない。
    seinas.wait_for_screen("the window with its own shadow", |shot| {
        shot.pixel(0, 0) == ORANGE
            && shot.pixel(WINDOW_WIDTH - 1, WINDOW_HEIGHT - 1) == ORANGE
            && shot.pixel(WINDOW_WIDTH + 5, 50) == MARGIN
            && shot.pixel(50, WINDOW_HEIGHT + 5) == MARGIN
            && shot.pixel(WINDOW_WIDTH + 10, 50) == BACKGROUND
    });
    // ポインターの位置は、画面(surface)の左上からなので、縁のぶん(10)だけずれる。
    seinas.pointer("motion 5 5");
    client.wait_for_line("window: pointer enter 15 15");
    // 2 つ目のウィンドウは、スロット 1((32, 32))に置かれる。1 つ目の大きさは 200x150 として数える。
    let _second = connect(&seinas, &["--color", "0000ff"]);
    seinas.wait_for_screen("the second window", |shot| {
        bar_color(shot, 32, 32, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });
}

#[test]
fn a_client_without_decoration_can_ask_to_be_moved() {
    let mut seinas = Compositor::start_with_input("dm");
    let client = connect(&seinas, &["--no-decoration", "--move-on-press"]);
    seinas.wait_for_screen("the bare window", |shot| shows_bare_window(shot, 0, 0));
    // 自分で描いたタイトルバーのつもりの所(上の端)を押すと、クライアントが move を頼み、Seinas が動かす。
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    client.wait_for_line("window: pointer button 0x110 pressed");
    seinas.pointer("motion 150 60");
    seinas.pointer("release");
    seinas.wait_for_screen("the moved window", |shot| shows_bare_window(shot, 50, 50));
}

#[test]
fn the_drag_modifier_moves_and_resizes_any_window() {
    let mut seinas = Compositor::start_with_input("da");
    let mut client = connect(&seinas, &["--no-decoration", "--follow-size", "--pointer"]);
    seinas.wait_for_screen("the bare window", |shot| shows_bare_window(shot, 0, 0));
    // Alt を押しながら、コンテンツの真ん中を左のボタンでドラッグすると、動く。クライアントには押したことを渡さない。
    seinas.pointer("key 56 press");
    seinas.pointer("motion 100 75");
    seinas.pointer("press");
    seinas.pointer("motion 150 125");
    seinas.pointer("release");
    seinas.wait_for_screen("the window moved by Alt and drag", |shot| {
        shows_bare_window(shot, 50, 50)
    });
    // 右のボタンで、右下の 3 分の 1 をドラッグすると、右下の角で大きさが変わる。
    seinas.pointer("motion 230 180");
    seinas.pointer("press right");
    seinas.pointer("motion 290 220");
    seinas.pointer("release right");
    client.wait_for_line("window: resized 260x190");
    seinas.wait_for_screen("the larger window", |shot| {
        shot.pixel(50 + 259, 50 + 189) == ORANGE
            && shot.pixel(50 + 260 + AROUND, 50 + 100) == BACKGROUND
    });
    // 右のボタンを離したら、サイズ変更は終わっている。続けて左のボタンでドラッグすると、また動く。
    seinas.pointer("motion 100 100");
    seinas.pointer("press");
    seinas.pointer("motion 120 110");
    seinas.pointer("release");
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    seinas.wait_for_screen("the window moved again", |shot| {
        shot.pixel(70, 60) == ORANGE
            && shot.pixel(70 + 259, 60 + 189) == ORANGE
            && shot.pixel(69, 100) == BACKGROUND
    });
    // クライアントには、どのボタンも届いていない。
    client.send_line("sync");
    loop {
        let line = client.next_line("the sync");
        assert!(!line.contains("pointer button"), "{line}");
        if line == "window: synced" {
            break;
        }
    }
}
