//! ウィンドウを、タイトルバーのドラッグで動かせること、閉じるボタンで閉じられることを、ピクセルで確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - タイトルバーを左のボタンで grab して動かすと、ウィンドウが付いてくる。押してから4ピクセル動くまでは、動かない。
//! - 画面の外へは、タイトルバーをもう一度 grab できる範囲までしか出せない。
//! - 動かしたウィンドウは、スロット番号を手放す。
//! - 閉じるボタン(タイトルバーの右の端の24x24)を押して、その上で離すと、クライアントに閉じるように頼む。
//!   外へ出して離すと、取り消し。奥のウィンドウのボタンも、見えていれば1回で効く。
//! - クライアントから頼まれた移動は、そのクライアントの上でボタンが押されている間だけ受ける。
//!
//! ポインターのイベントは、seinas-standaloneのテスト用の入力インターフェース(`--test-input`)から送る。
//! テスト用のクライアント(`test-client window`)のウィンドウは、コンテンツが200x150の橙。

mod common;

use common::{
    bar_color, upper_bar_color, wait_for, Compositor, Shot, TestClient, ACTIVE_BAR, ACTIVE_BORDER,
    AROUND, BACKGROUND, BAR, CLOSE_BUTTON, FRAME_RADIUS, HEIGHT, HOT_CLOSE_BUTTON, INACTIVE_BAR,
    WIDTH,
};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];

/// 外枠の左上が(`x0`, `y0`)にあるテスト用のウィンドウが、タイトルバーの色 `bar` で、全体が見える形で描かれているか。
fn shows_window(shot: &Shot, x0: usize, y0: usize, bar: [u8; 3]) -> bool {
    bar_color(shot, x0, y0, WINDOW_WIDTH) == Some(bar)
        // 左の端は、丸く欠いた角の下で見る。下の角も欠いてあるので、右の端は、その上の行で見る。
        && shot.pixel(x0, y0 + FRAME_RADIUS) == bar
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + WINDOW_WIDTH - 1, y0 + BAR + WINDOW_HEIGHT - 1 - FRAME_RADIUS) == ORANGE
}

/// 外枠の左上が(`x0`, `y0`)にあるウィンドウの、閉じるボタンの中央。
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

    // タイトルバーを押して、3ピクセルだけ動かして離す: 動かない(4ピクセルのしきい値)。
    drag(&mut seinas, (100, 10), (103, 10));
    // タイトルバーを押して、(50, 50)だけ動かす: ウィンドウも、ちょうど同じだけ動く。上の操作で少しでも
    // ずれていれば、ここで位置が合わなくなる。押す所は、ダブルクリックにならないよう、前の所から離す。
    drag(&mut seinas, (110, 10), (160, 60));
    seinas.wait_for_screen("the window moved by (50, 50)", |shot| {
        shows_window(shot, 50, 50, ACTIVE_BAR) && shot.pixel(2, 2) == BACKGROUND
    });

    // 動かしている途中も、ポインターに付いてくる。しきい値を超えた後は、しきい値のぶんも含めて動く。
    motion(&mut seinas, 150, 60);
    seinas.pointer("press");
    motion(&mut seinas, 153, 60);
    motion(&mut seinas, 154, 60);
    seinas.wait_for_screen("the window following the pointer by 4", |shot| {
        shows_window(shot, 54, 50, ACTIVE_BAR) && shot.pixel(54 - AROUND, 60) == BACKGROUND
    });
    motion(&mut seinas, 250, 200);
    seinas.wait_for_screen("the window following the pointer", |shot| {
        shows_window(shot, 150, 190, ACTIVE_BAR)
    });
    seinas.pointer("release");

    // 離した後は、ポインターを動かしても、付いてこない。もう一度 grab すると、また動く。
    motion(&mut seinas, 400, 400);
    drag(&mut seinas, (160, 200), (170, 210));
    seinas.wait_for_screen("the window moved again from where it was left", |shot| {
        shows_window(shot, 160, 200, ACTIVE_BAR)
    });

    // タイトルバーの上で押したボタンは、クライアントには届いていない。コンテンツの上で押すと、届く。
    // 位置は、コンテンツの左上を原点にしたもの。
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
    // タイトルバーの(100, 10)を grab したまま、あちこちへ引っ張る。
    motion(&mut seinas, 100, 10);
    seinas.pointer("press");

    // 右下へ: タイトルバーの全体が、画面の下の端に残る。横は、タイトルバーの左の48ピクセルが残る。左と上の外には、
    // 枠(の線)があり、その外は背景(角は丸く欠いてあるので、角の丸みの内側の行と列で見る)。
    motion(&mut seinas, 5000, 5000);
    let (left, top) = (WIDTH - 48, HEIGHT - BAR);
    seinas.wait_for_screen("the bar kept at the bottom right", |shot| {
        shot.pixel(left, top + FRAME_RADIUS) == ACTIVE_BAR
            && shot.pixel(WIDTH - 1, HEIGHT - 1) == ACTIVE_BAR
            && shot.pixel(left - 1, top + FRAME_RADIUS) == ACTIVE_BORDER
            && shot.pixel(left + FRAME_RADIUS, top - 1) == ACTIVE_BORDER
            && shot.pixel(left - AROUND, top + FRAME_RADIUS) == BACKGROUND
            && shot.pixel(left + FRAME_RADIUS, top - AROUND) == BACKGROUND
    });

    // 左上へ: タイトルバーの上の端は、画面の上の端で止まる。横は、タイトルバーの右の 180 ピクセル(48 と、
    // ボタン 3 つ分の 132)が残る。ポインターは、上の端(最大化の予告が出る所)の 1 つ下までにする。
    motion(&mut seinas, -5000, 1);
    let keep = 48 + 3 * CLOSE_BUTTON;
    seinas.wait_for_screen("the bar kept at the top left", |shot| {
        shot.pixel(keep - 1, FRAME_RADIUS) == ACTIVE_BAR
            && shot.pixel(keep, FRAME_RADIUS) == ACTIVE_BORDER
            && shot.pixel(keep + AROUND, FRAME_RADIUS) == BACKGROUND
            && shot.pixel(keep - 1, BAR - 1) == ACTIVE_BAR
            // コンテンツも、右の 180 ピクセルだけが見えている。
            && shot.pixel(keep - 1, BAR) == ORANGE
            && shot.pixel(keep + AROUND, BAR) == BACKGROUND
            && shot.pixel(keep - 1, BAR + WINDOW_HEIGHT - 1 - FRAME_RADIUS) == ORANGE
    });

    // ポインターを戻すと、限りから離れて、また付いてくる。
    motion(&mut seinas, 300, 110);
    seinas.wait_for_screen("the window following the pointer again", |shot| {
        shows_window(shot, 200, 100, ACTIVE_BAR)
    });
    seinas.pointer("release");

    // 外へ出した後でも、残ったタイトルバーを grab して、引き戻せる。
    drag(&mut seinas, (300, 110), (-5000, 110));
    seinas.wait_for_screen("the window pushed to the left edge", |shot| {
        shot.pixel(keep - 1, 110) == ACTIVE_BAR && shot.pixel(keep + AROUND, 110) == BACKGROUND
    });
    // 残っているのは、タイトルバーの右の 180 ピクセル(左は -20)。その左の 92 ピクセル(ボタンでない所)を
    // grab する。
    drag(&mut seinas, (10, 110), (310, 210));
    seinas.wait_for_screen("the window pulled back", |shot| {
        shows_window(shot, 280, 200, ACTIVE_BAR)
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

    // 次のウィンドウは、空いたスロット0(左上)に入る。動かしたウィンドウは、動かした先に残る。
    let second = connect(&seinas, &["--title", "second"]);
    seinas.wait_for_screen("the second window at the top left", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR) && shows_window(shot, 300, 200, INACTIVE_BAR)
    });
    // その次は、スロット1。
    let _third = connect(&seinas, &["--title", "third"]);
    seinas.wait_for_screen("the third window at the next slot", |shot| {
        shows_window(shot, 32, 32, ACTIVE_BAR) && shows_window(shot, 300, 200, INACTIVE_BAR)
    });

    // 動かさずにタイトルバーを押しただけのウィンドウ(2つ目。raise される)は、番号を持ったまま。去ると、
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
            && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
    });

    // ポインターを乗せると、そのボタンだけ、地が赤になる。
    let (x, y) = close_button(32, 32);
    motion(&mut seinas, x as i32, y as i32);
    seinas.wait_for_screen("the hot close button", |shot| {
        bar_color(shot, 32, 32, WINDOW_WIDTH) == Some(HOT_CLOSE_BUTTON)
            // ボタンの左の端までが赤で、その手前はタイトルバーの色。
            && shot.pixel(32 + WINDOW_WIDTH - CLOSE_BUTTON, 40) == HOT_CLOSE_BUTTON
            && shot.pixel(32 + WINDOW_WIDTH - CLOSE_BUTTON - 1, 40) == ACTIVE_BAR
            && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
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
            && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
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

    // コンテンツの上で押すと、クライアントが、動かしてほしいと頼んでくる。
    // 頼みが届くのを待たずに動かしても、押した点からのずれだけ、ちょうど動く。
    motion(&mut seinas, 100, 100);
    seinas.pointer("press");
    motion(&mut seinas, 160, 130);
    client.wait_for_line("window: pointer button 0x110 pressed");
    seinas.wait_for_screen("the window following the pointer", |shot| {
        shows_window(shot, 60, 30, ACTIVE_BAR)
    });
    // 画面の外の限りは、タイトルバーを grab したときと同じ(ポインターは、最大化の予告が出る上の端の 1 つ下まで)。
    motion(&mut seinas, 100, 1);
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
    client.wait_for_line(&format!("window: pointer enter 10 {}", 100 - BAR));
    motion(&mut seinas, 500, 400);
    client.wait_for_line("window: pointer leave");
    assert!(client.process.is_running());
    assert!(seinas.process.is_running());
}

#[test]
fn without_the_test_input_no_seat_is_offered() {
    let mut seinas = Compositor::start("mg");
    // ポインターを使おうとするクライアントは、seat が無いので、準備の段階で失敗して終わる。
    let mut client = seinas.connect_test_client(&["window", "--pointer"]);
    let status = wait_for("the client to give up", || {
        client.process.0.try_wait().expect("check the client")
    });
    assert_eq!(status.code(), Some(2));
    assert!(seinas.process.is_running());
}
