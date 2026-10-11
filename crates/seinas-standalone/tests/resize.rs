//! 縁と角でのサイズ変更を、fake screen のピクセルと、テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - 外枠の外側の 8 ピクセルの境界を左のボタンで grab して動かすと、その辺(角では 2 つの辺)が動く。
//! - ドラッグの間は resizing の状態つきの configure で、離したら resizing の無い configure で、
//!   コンテンツの新しい大きさを伝える。クライアントの最小・最大の大きさを守る。
//! - 左か上の辺を動かすときは、反対の辺(右か下)の位置を保つ。
//! - クライアントからのサイズ変更の頼み(xdg_toplevel の resize)にも応える。
//!
//! ポインターのイベントは、seinas-standalone のテスト用の入力インターフェース(`--test-input`)から送る。
//! テスト用のクライアント(`test-client window --follow-size`)は、届いた大きさで再描画する。

mod common;

use common::{bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, BAR};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// 2 つ目のウィンドウのスロット(スロット 1)の位置。
const STEP: usize = 32;

/// 外枠の左上が (`x0`, `y0`)、コンテンツが `w` × `h` のウィンドウが、activated のタイトルバーで描かれているか。
/// 右と下のすぐ外が、背景であることも見る。
fn shows_window(shot: &Shot, (x0, y0): (usize, usize), (w, h): (usize, usize)) -> bool {
    bar_color(shot, x0, y0, w) == Some(ACTIVE_BAR)
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + w - 1, y0 + BAR + h - 1) == ORANGE
        && shot.pixel(x0 + w, y0 + BAR + h / 2) == BACKGROUND
        && shot.pixel(x0 + w / 2, y0 + BAR + h) == BACKGROUND
}

/// (`from`)で左のボタンを押し、(`to`)まで動かして、離す。
fn drag(seinas: &mut Compositor, from: (i32, i32), to: (i32, i32)) {
    seinas.pointer(&format!("motion {} {}", from.0, from.1));
    seinas.pointer("press");
    seinas.pointer(&format!("motion {} {}", to.0, to.1));
    seinas.pointer("release");
}

/// ウィンドウを 1 つ出すクライアントをつなぎ、描かれるまで待つ。
fn connect(seinas: &Compositor, args: &[&str]) -> TestClient {
    let client = seinas.connect_test_client(&[&["window", "--follow-size"], args].concat());
    client.wait_for_line("window: drawn");
    client
}

/// 次の行。ポインターのイベントの行は、読み飛ばす。
fn next_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        if !line.starts_with("window: pointer ") {
            return line;
        }
    }
}

/// サイズ変更の configure(resizing つき)、再描画、最後の configure(resizing 無し)が、この順に届くこと。
fn expect_resize(client: &TestClient, (w, h): (usize, usize)) {
    assert_eq!(
        next_line(client, "the configure while resizing"),
        format!("window: configure {w}x{h} [activated,resizing]")
    );
    assert_eq!(
        next_line(client, "the new buffer"),
        format!("window: resized {w}x{h}")
    );
    assert_eq!(
        next_line(client, "the last configure"),
        format!("window: configure {w}x{h} [activated]")
    );
}

#[test]
fn dragging_the_bottom_right_corner_resizes_the_window() {
    let mut seinas = Compositor::start_with_input("ra");
    let window = connect(&seinas, &[]);
    // 外枠は (0, 0)〜(200, 174)。右下の角の外側を grab して、右へ 50、下へ 30 動かす。
    drag(&mut seinas, (204, 178), (254, 208));
    expect_resize(&window, (250, 180));
    seinas.wait_for_screen("the larger window", |shot| {
        shows_window(shot, (0, 0), (250, 180))
    });
}

#[test]
fn the_left_and_top_edges_keep_the_right_and_bottom_edges() {
    let mut seinas = Compositor::start_with_input("rb");
    let _first = connect(&seinas, &[]);
    let second = connect(&seinas, &[]);
    // 2 つ目の外枠は (32, 32)〜(232, 206)。左上の角の外側を grab して、左へ 20、上へ 10 動かす。
    drag(&mut seinas, (28, 28), (8, 18));
    expect_resize(&second, (220, 160));
    // 右の端(x 232)と下の端(y 206)は、そのまま。
    seinas.wait_for_screen("the window grown to the top left", |shot| {
        shows_window(shot, (STEP - 20, STEP - 10), (220, 160))
    });
}

#[test]
fn the_size_keeps_the_limits_of_the_client() {
    let mut seinas = Compositor::start_with_input("rc");
    let window = connect(&seinas, &["--min-size", "150x100", "--max-size", "300x200"]);
    // 大きくしても、最大の 300x200 まで。
    drag(&mut seinas, (204, 178), (604, 478));
    expect_resize(&window, (300, 200));
    seinas.wait_for_screen("the largest window", |shot| {
        shows_window(shot, (0, 0), (300, 200))
    });
    // 小さくしても、最小の 150x100 まで。外枠は (0, 0)〜(300, 224)。
    drag(&mut seinas, (304, 228), (10, 10));
    expect_resize(&window, (150, 100));
    seinas.wait_for_screen("the smallest window", |shot| {
        shows_window(shot, (0, 0), (150, 100))
    });
}

#[test]
fn a_client_can_ask_to_be_resized() {
    let mut seinas = Compositor::start_with_input("rd");
    let window = connect(&seinas, &["--resize-on-press", "bottom-right"]);
    // コンテンツを押すと、クライアントがサイズ変更を頼む。そこから右へ 30、下へ 20 動かす。頼みが届くのを
    // 待たずに動かしても、押した点からのずれのぶんだけ変わる。頼みが届いてから、ボタンを離す。
    seinas.pointer("motion 100 100");
    seinas.pointer("press");
    seinas.pointer("motion 130 120");
    let (w, h) = (WINDOW_WIDTH + 30, WINDOW_HEIGHT + 20);
    assert_eq!(
        next_line(&window, "the configure while resizing"),
        format!("window: configure {w}x{h} [activated,resizing]")
    );
    seinas.pointer("release");
    assert_eq!(
        next_line(&window, "the new buffer"),
        format!("window: resized {w}x{h}")
    );
    assert_eq!(
        next_line(&window, "the last configure"),
        format!("window: configure {w}x{h} [activated]")
    );
    seinas.wait_for_screen("the window resized by the client", |shot| {
        shows_window(shot, (0, 0), (WINDOW_WIDTH + 30, WINDOW_HEIGHT + 20))
    });
}
