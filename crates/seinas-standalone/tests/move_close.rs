//! ウィンドウを、題名の帯のドラッグで動かせること、閉じるボタンで閉じられることを、画素で確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - 帯を左のボタンで掴んで動かすと、ウィンドウが付いてくる。押してから4画素動くまでは、動かない。
//! - 画面の外へは、帯をもう一度掴める範囲までしか出せない。
//! - 動かしたウィンドウは、置き場所の番号を手放す。
//! - 閉じるボタン(帯の右の端の24x24)を押して、その上で離すと、クライアントに閉じるように頼む。
//!   外へ出して離すと、取り消し。奥のウィンドウのボタンも、見えていれば1回で効く。
//! - クライアントから頼まれた移動は、そのクライアントの上でボタンが押されている間だけ受ける。
//!
//! ポインターの知らせは、seinas-standaloneの試験のための入力の口(`--test-input`)から送る。
//! 試験用のクライアント(`test-client window`)のウィンドウは、中身が200x150の橙。

mod common;

use common::{
    bar_color, wait_for, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, BAR, CLOSE_BUTTON,
    HEIGHT, HOT_CLOSE_BUTTON, INACTIVE_BAR, WIDTH,
};

/// 試験用のクライアントのウィンドウの、中身の大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];

/// 外形の左上が(`x0`, `y0`)にある試験用のウィンドウが、帯の色 `bar` で、全体が見える形で描かれているか。
fn shows_window(shot: &Shot, x0: usize, y0: usize, bar: [u8; 3]) -> bool {
    bar_color(shot, x0, y0, WINDOW_WIDTH) == Some(bar)
        && shot.pixel(x0, y0) == bar
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + WINDOW_WIDTH - 1, y0 + BAR + WINDOW_HEIGHT - 1) == ORANGE
}

/// 外形の左上が(`x0`, `y0`)にあるウィンドウの、閉じるボタンの中央。
fn close_button(x0: usize, y0: usize) -> (usize, usize) {
    (x0 + WINDOW_WIDTH - CLOSE_BUTTON / 2, y0 + BAR / 2)
}

/// ポインターを(`x`, `y`)へ動かす。
fn motion(seinas: &mut Compositor, x: i32, y: i32) {
    seinas.pointer(&format!("motion {x} {y}"));
}

/// (`from`)で左のボタンを押し、(`to`)まで動かして、離す。
fn drag(seinas: &mut Compositor, from: (i32, i32), to: (i32, i32)) {
    motion(seinas, from.0, from.1);
    seinas.pointer("press");
    motion(seinas, to.0, to.1);
    seinas.pointer("release");
}

/// (`x`, `y`)で左のボタンを押して、離す。
fn click(seinas: &mut Compositor, x: usize, y: usize) {
    drag(seinas, (x as i32, y as i32), (x as i32, y as i32));
}

/// ウィンドウを1つ出すクライアントをつなぎ、描かれるまで待つ。
fn connect(seinas: &Compositor, args: &[&str]) -> TestClient {
    let client = seinas.connect_test_client(&[&["window"], args].concat());
    client.wait_for_line("window: drawn");
    client
}

#[test]
fn dragging_the_title_bar_moves_the_window() {
    let mut seinas = Compositor::start_with_input("ma");
    let mut client = connect(&seinas, &["--title", "drag", "--pointer"]);
    seinas.wait_for_screen("the window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });

    // 帯を押して、3画素だけ動かして離す: 動かない(4画素の遊び)。
    drag(&mut seinas, (100, 10), (103, 10));
    // 帯を押して、(50, 50)だけ動かす: ウィンドウも、ちょうど同じだけ動く。上の操作で少しでも
    // ずれていれば、ここで位置が合わなくなる。
    drag(&mut seinas, (100, 10), (150, 60));
    seinas.wait_for_screen("the window moved by (50, 50)", |shot| {
        shows_window(shot, 50, 50, ACTIVE_BAR) && shot.pixel(2, 2) == BACKGROUND
    });

    // 動かしている途中も、ポインターに付いてくる。遊びを超えた後は、遊びのぶんも含めて動く。
    motion(&mut seinas, 150, 60);
    seinas.pointer("press");
    motion(&mut seinas, 153, 60);
    motion(&mut seinas, 154, 60);
    seinas.wait_for_screen("the window following the pointer by 4", |shot| {
        shows_window(shot, 54, 50, ACTIVE_BAR) && shot.pixel(53, 60) == BACKGROUND
    });
    motion(&mut seinas, 250, 200);
    seinas.wait_for_screen("the window following the pointer", |shot| {
        shows_window(shot, 150, 190, ACTIVE_BAR)
    });
    seinas.pointer("release");

    // 離した後は、ポインターを動かしても、付いてこない。もう一度掴むと、また動く。
    motion(&mut seinas, 400, 400);
    drag(&mut seinas, (160, 200), (170, 210));
    seinas.wait_for_screen("the window moved again from where it was left", |shot| {
        shows_window(shot, 160, 200, ACTIVE_BAR)
    });

    // 帯の上で押したボタンは、クライアントには届いていない。中身の上で押すと、届く。
    // 位置は、中身の左上を原点にしたもの。
    click(&mut seinas, 160 + 20, 200 + BAR + 30);
    let mut seen = Vec::new();
    loop {
        let line = client.next_line("the button on the content");
        if line == "window: pointer button 0x110 pressed" {
            break;
        }
        seen.push(line);
    }
    assert!(
        !seen.iter().any(|line| line.contains("pointer button")),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"window: pointer motion 20 30".to_owned())
            || seen.contains(&"window: pointer enter 20 30".to_owned()),
        "{seen:?}"
    );
    assert!(client.process.is_running());
    assert!(seinas.process.is_running());
}

#[test]
fn a_dragged_window_cannot_leave_the_screen_entirely() {
    let mut seinas = Compositor::start_with_input("mb");
    let _client = connect(&seinas, &["--title", "edge"]);
    seinas.wait_for_screen("the window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    // 帯の(100, 10)を掴んだまま、あちこちへ引っ張る。
    motion(&mut seinas, 100, 10);
    seinas.pointer("press");

    // 右下へ: 帯の全体が、画面の下の端に残る。横は、帯の左の48画素が残る。
    motion(&mut seinas, 5000, 5000);
    let (left, top) = (WIDTH - 48, HEIGHT - BAR);
    seinas.wait_for_screen("the bar kept at the bottom right", |shot| {
        shot.pixel(left, top) == ACTIVE_BAR
            && shot.pixel(WIDTH - 1, HEIGHT - 1) == ACTIVE_BAR
            && shot.pixel(left - 1, top) == BACKGROUND
            && shot.pixel(left, top - 1) == BACKGROUND
    });

    // 左上へ: 帯の上の端は、画面の上の端で止まる。横は、帯の右の48画素(閉じるボタンを含む)が残る。
    motion(&mut seinas, -5000, -5000);
    seinas.wait_for_screen("the bar kept at the top left", |shot| {
        shot.pixel(47, 0) == ACTIVE_BAR
            && shot.pixel(48, 0) == BACKGROUND
            && shot.pixel(47, BAR - 1) == ACTIVE_BAR
            // 中身も、右の48画素だけが見えている。
            && shot.pixel(47, BAR) == ORANGE
            && shot.pixel(48, BAR) == BACKGROUND
            && shot.pixel(47, BAR + WINDOW_HEIGHT - 1) == ORANGE
    });

    // ポインターを戻すと、限りから離れて、また付いてくる。
    motion(&mut seinas, 300, 110);
    seinas.wait_for_screen("the window following the pointer again", |shot| {
        shows_window(shot, 200, 100, ACTIVE_BAR)
    });
    seinas.pointer("release");

    // 外へ出した後でも、残った帯を掴んで、引き戻せる。
    drag(&mut seinas, (300, 110), (-5000, 110));
    seinas.wait_for_screen("the window pushed to the left edge", |shot| {
        shot.pixel(47, 100) == ACTIVE_BAR && shot.pixel(48, 100) == BACKGROUND
    });
    // 残っているのは、帯の右の48画素。その左の半分(閉じるボタンでない所)を掴む。
    drag(&mut seinas, (10, 110), (310, 210));
    seinas.wait_for_screen("the window pulled back", |shot| {
        shows_window(shot, 148, 200, ACTIVE_BAR)
    });
    assert!(seinas.process.is_running());
}

#[test]
fn a_moved_window_gives_up_its_slot() {
    let mut seinas = Compositor::start_with_input("mc");
    let _first = connect(&seinas, &["--title", "first"]);
    seinas.wait_for_screen("the first window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    drag(&mut seinas, (100, 10), (400, 210));
    seinas.wait_for_screen("the first window moved away", |shot| {
        shows_window(shot, 300, 200, ACTIVE_BAR) && shot.pixel(2, 2) == BACKGROUND
    });

    // 次のウィンドウは、空いた置き場所0(左上)に入る。動かしたウィンドウは、動かした先に残る。
    let second = connect(&seinas, &["--title", "second"]);
    seinas.wait_for_screen("the second window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR) && shows_window(shot, 300, 200, INACTIVE_BAR)
    });
    // その次は、置き場所1。
    let _third = connect(&seinas, &["--title", "third"]);
    seinas.wait_for_screen("the third window at the next slot", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR) && shows_window(shot, 300, 200, INACTIVE_BAR)
    });

    // 動かさずに帯を押しただけのウィンドウ(2つ目。手前に出る)は、番号を持ったまま。去ると、
    // 次のウィンドウが、その番号0に入る。
    click(&mut seinas, 10, 10);
    seinas.wait_for_screen("the second window raised", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    drop(second);
    seinas.wait_for_screen("the second window gone", |shot| {
        shot.pixel(2, 2) == BACKGROUND
    });
    let _fourth = connect(&seinas, &["--title", "fourth"]);
    seinas.wait_for_screen("the fourth window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
}

#[test]
fn the_close_button_asks_the_client_to_close() {
    let mut seinas = Compositor::start_with_input("md");
    let mut back = connect(&seinas, &["--title", "back"]);
    let mut front = connect(&seinas, &["--title", "front"]);
    seinas.wait_for_screen("two windows", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR)
            && bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(INACTIVE_BAR)
    });

    // ポインターを乗せると、そのボタンだけ、地が赤になる。
    let (x, y) = close_button(32, 32);
    motion(&mut seinas, x as i32, y as i32);
    seinas.wait_for_screen("the hot close button", |shot| {
        bar_color(shot, 32, 32, WINDOW_WIDTH) == Some(HOT_CLOSE_BUTTON)
            // ボタンの左の端までが赤で、その手前は帯の色。
            && shot.pixel(32 + WINDOW_WIDTH - CLOSE_BUTTON, 40) == HOT_CLOSE_BUTTON
            && shot.pixel(32 + WINDOW_WIDTH - CLOSE_BUTTON - 1, 40) == ACTIVE_BAR
            && bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(INACTIVE_BAR)
    });

    // 押して、外へ出して離す: 取り消し。外へ出すと、赤でなくなる。
    seinas.pointer("press");
    motion(&mut seinas, 100, 120);
    seinas.wait_for_screen("the close button after the pointer left it", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR)
    });
    seinas.pointer("release");

    // 押して、ボタンの上で離す: 手前のウィンドウに、閉じるように頼む。クライアントが応じて、去る。
    click(&mut seinas, x, y);
    front.wait_for_line("window: close");
    seinas.wait_for_screen("the front window closed", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
            && shot.pixel(32 + WINDOW_WIDTH - 1, 32 + BAR + WINDOW_HEIGHT - 1) == BACKGROUND
    });
    let status = wait_for("the front client to exit", || {
        front.process.0.try_wait().expect("check the client")
    });
    assert!(status.success());
    // 取り消したときの頼みは、届いていない(届いたのは、1回だけ)。奥のウィンドウにも、届いていない。
    assert!(back.process.is_running());

    // 奥のウィンドウの閉じるボタンも、見えていれば1回で効く。
    let mut front = connect(&seinas, &["--title", "front again"]);
    seinas.wait_for_screen("two windows again", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR)
            && bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(INACTIVE_BAR)
    });
    let (x, y) = close_button(0, 0);
    click(&mut seinas, x, y);
    back.wait_for_line("window: close");
    seinas.wait_for_screen("the back window closed", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR) && shot.pixel(2, 2) == BACKGROUND
    });
    assert!(front.process.is_running());
    assert!(seinas.process.is_running());
}

#[test]
fn a_client_that_ignores_the_close_request_stays() {
    let mut seinas = Compositor::start_with_input("me");
    let mut stubborn = connect(&seinas, &["--title", "stubborn", "--ignore-close"]);
    seinas.wait_for_screen("the window", |shot| shows_window(shot, 0, 0, ACTIVE_BAR));

    let (x, y) = close_button(0, 0);
    click(&mut seinas, x, y);
    stubborn.wait_for_line("window: close");
    // もう一度押すと、もう一度頼む。
    click(&mut seinas, x, y);
    stubborn.wait_for_line("window: close");

    // ウィンドウは残り、動かすこともできる。
    drag(&mut seinas, (50, 10), (150, 110));
    seinas.wait_for_screen("the window still there, moved", |shot| {
        shows_window(shot, 100, 100, ACTIVE_BAR)
    });
    assert!(stubborn.process.is_running());
    assert!(seinas.process.is_running());
}

#[test]
fn a_client_may_ask_to_be_moved_while_a_button_is_held_on_it() {
    let mut seinas = Compositor::start_with_input("mf");
    let mut client = connect(&seinas, &["--title", "mover", "--move-on-press"]);
    seinas.wait_for_screen("the window", |shot| shows_window(shot, 0, 0, ACTIVE_BAR));

    // 中身の上で押すと、クライアントが、動かしてほしいと頼んでくる。
    // 頼みが届くのを待たずに動かしても、押した点からのずれだけ、ちょうど動く。
    motion(&mut seinas, 100, 100);
    seinas.pointer("press");
    motion(&mut seinas, 160, 130);
    client.wait_for_line("window: pointer button 0x110 pressed");
    seinas.wait_for_screen("the window following the pointer", |shot| {
        shows_window(shot, 60, 30, ACTIVE_BAR)
    });
    // 画面の外の限りは、帯を掴んだときと同じ。
    motion(&mut seinas, 100, -500);
    seinas.wait_for_screen("the window stopped at the top", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    motion(&mut seinas, 300, 300);
    seinas.wait_for_screen("the window following the pointer", |shot| {
        shows_window(shot, 200, 200, ACTIVE_BAR)
    });
    seinas.pointer("release");
    motion(&mut seinas, 500, 400);

    // 離した後は、付いてこない。ボタンを押していないときに動かしても、動かない。
    drag(&mut seinas, (250, 210), (260, 210));
    seinas.wait_for_screen("the window dragged by its bar afterwards", |shot| {
        shows_window(shot, 210, 200, ACTIVE_BAR)
    });

    // 動かし終えた後も、ポインターの出入りは、いつもどおり知らされる。
    motion(&mut seinas, 220, 300);
    client.wait_for_line("window: pointer enter 10 76");
    motion(&mut seinas, 500, 400);
    client.wait_for_line("window: pointer leave");
    assert!(client.process.is_running());
    assert!(seinas.process.is_running());
}

#[test]
fn without_the_test_input_no_seat_is_offered() {
    let mut seinas = Compositor::start("mg");
    // ポインターを使おうとするクライアントは、席が無いので、準備の段階で失敗して終わる。
    let mut client = seinas.connect_test_client(&["window", "--pointer"]);
    let status = wait_for("the client to give up", || {
        client.process.0.try_wait().expect("check the client")
    });
    assert_eq!(status.code(), Some(2));
    assert!(seinas.process.is_running());
}
