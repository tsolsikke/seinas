//! ポップアップ(xdg_popup)の置き方・描き方・grab を、ピクセルとテスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「ポップアップ」):
//! - 置き方は positioner に従い、はみ出すときは constraint_adjustment(反転・ずらし・縮め)を、出力の使える範囲の中で
//!   扱う。ポップアップは根元のウィンドウのすぐ手前に描き、飾りも、根元のウィンドウの角丸のクリップも付けない。
//! - grab の間は、grab したクライアントの面にだけポインターを渡し、ほかの所を押したら連なりを閉じる(popup_done)。
//!   キーボードのフォーカスは、いちばん上のポップアップに付く(Esc もクライアントへ渡す)。
//! - Alt+Tab・Alt+ドラッグ・別のウィンドウへのフォーカスの移り変わりでも、連なりを閉じる。
//!
//! テスト用のクライアント(`test-client window`)は、標準入力の `popup …` の行でポップアップを出す。ポップアップは紫。

mod common;

use common::{Compositor, Shot, TestClient, BAR};

/// テスト用のクライアントのポップアップの色(紫)と、ウィンドウの色(橙)。
const PURPLE: [u8; 3] = [0x80, 0x00, 0xff];
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
const KEY_ESC: u32 = 1;
const KEY_TAB: u32 = 15;
const KEY_LEFT_ALT: u32 = 56;

/// 大きさ `size` のウィンドウを出し、描かれるまで待つ。ポインターとキーボードのイベントを書き出す。
fn window(seinas: &Compositor, size: &str) -> TestClient {
    let client = seinas.connect_test_client(&["window", "--pointer", "--keyboard", "--size", size]);
    client.wait_for_line("window: drawn");
    client
}

/// 届いた行のうち、`prefix` で始まる最初の行。ほかの行は読み飛ばす。
fn next_line_starting(client: &TestClient, prefix: &str) -> String {
    loop {
        let line = client.next_line(prefix);
        if line.starts_with(prefix) {
            return line;
        }
    }
}

/// (`x`, `y`)で左のボタンを押して、離す。
fn click(seinas: &mut Compositor, x: i32, y: i32) {
    seinas.pointer(&format!("motion {x} {y}"));
    seinas.pointer("press");
    seinas.pointer("release");
}

/// `client` のウィンドウの(`x`, `y`)を押して離し、クライアントに届くまで待つ(grab に使う押下の serial を作る)。
fn click_client(seinas: &mut Compositor, client: &TestClient, x: i32, y: i32) {
    click(seinas, x, y);
    client.wait_for_line("window: pointer button 0x110 released");
}

/// grab するポップアップを出し、キーボードのフォーカスが移り、絵が出るまで待つ。`n` はポップアップの番号。
fn open_grabbing_popup(client: &mut TestClient, line: &str, n: usize) {
    client.send_line(line);
    client.wait_for_line(&format!("window: keyboard enter popup {n}"));
    let (w, h) = {
        let mut words = line.split_whitespace().skip(3);
        (
            words.next().unwrap().to_owned(),
            words.next().unwrap().to_owned(),
        )
    };
    client.wait_for_line(&format!("window: popup {n} drawn {w}x{h}"));
}

/// 画面の上の矩形(左上 `x`, `y`、大きさ `w` × `h`)の四隅が、色 `color` か。
fn filled(shot: &Shot, (x, y, w, h): (usize, usize, usize, usize), color: [u8; 3]) -> bool {
    [
        (x, y),
        (x + w - 1, y),
        (x, y + h - 1),
        (x + w - 1, y + h - 1),
    ]
    .iter()
    .all(|&(px, py)| shot.pixel(px, py) == color)
}

#[test]
fn a_popup_is_placed_by_its_positioner_in_front_of_its_window() {
    let mut seinas = Compositor::start_with_input("pa");
    let mut client = window(&seinas, "200x150");
    // anchor rect は (20, 30) の 1x1、anchor と gravity は bottom_right なので、ポップアップの左上は (21, 31)。
    client.send_line("popup 20 30 60 40");
    assert_eq!(
        next_line_starting(&client, "window: popup 1 configure"),
        "window: popup 1 configure 21 31 60 40"
    );
    client.wait_for_line("window: popup 1 drawn 60x40");
    // 画面の上では、ウィンドウのコンテンツの左上(0, BAR)からの位置。
    seinas.wait_for_screen("the popup in front of the window", |shot| {
        filled(shot, (21, BAR + 31, 60, 40), PURPLE)
            && shot.pixel(20, BAR + 31) == ORANGE
            && shot.pixel(21, BAR + 30) == ORANGE
    });
    // 右下の角の外へはみ出すポップアップは、ウィンドウの角丸のクリップに切られず、ウィンドウの外にも出る。
    client.send_line("popup 190 140 60 40 color=8000ff");
    client.wait_for_line("window: popup 2 drawn 60x40");
    seinas.wait_for_screen("the popup over the cut corner", |shot| {
        // ウィンドウ(コンテンツは 200x150)の右下の角を丸く欠いた所(x 199、y BAR + 149)と、ウィンドウの外。
        shot.pixel(199, BAR + 149) == PURPLE && filled(shot, (191, BAR + 141, 60, 40), PURPLE)
    });
    // grab の無いポップアップは、外を押しても、Seinas は閉じない(閉じるかは、クライアントが決める)。
    click(&mut seinas, 600, 100);
    let lines = client.lines_within(std::time::Duration::from_millis(500));
    assert!(!lines.iter().any(|l| l.ends_with(" done")), "{lines:?}");
}

#[test]
fn a_popup_at_the_edge_flips_slides_or_shrinks() {
    let seinas = Compositor::start_with_input("pb");
    let mut client = window(&seinas, "600x400");
    // 何もしなければ、はみ出したまま(左上 (551, 381)、画面の右と下の外へ出る)。
    client.send_line("popup 550 380 120 80");
    assert_eq!(
        next_line_starting(&client, "window: popup 1 configure"),
        "window: popup 1 configure 551 381 120 80"
    );
    // 反転(flip_x 4 + flip_y 8):anchor と gravity を逆にして、anchor rect の左上へ。
    client.send_line("popup 550 380 120 80 adjust=12");
    assert_eq!(
        next_line_starting(&client, "window: popup 2 configure"),
        "window: popup 2 configure 430 300 120 80"
    );
    client.wait_for_line("window: popup 2 drawn 120x80");
    seinas.wait_for_screen("the flipped popup", |shot| {
        filled(shot, (430, BAR + 300, 120, 80), PURPLE)
    });
    // ずらし(slide_x 1):右の端に合わせて左へ。縦は、そのまま。
    client.send_line("popup 550 380 120 80 adjust=1");
    assert_eq!(
        next_line_starting(&client, "window: popup 3 configure"),
        "window: popup 3 configure 520 381 120 80"
    );
    // 縮め(resize_x 16 + resize_y 32):使える範囲(640x480。コンテンツの左上は (0, 36))に収まる大きさへ。
    client.send_line("popup 550 380 120 80 adjust=48");
    assert_eq!(
        next_line_starting(&client, "window: popup 4 configure"),
        format!(
            "window: popup 4 configure 551 381 89 {}",
            480 - BAR as i32 - 381
        )
    );
    // 置き方を変える(xdg_popup.reposition)と、repositioned と新しい configure が届く。
    client.send_line("popup-move 2 100 50 7");
    client.wait_for_line("window: popup 2 repositioned 7");
    assert_eq!(
        next_line_starting(&client, "window: popup 2 configure"),
        "window: popup 2 configure 101 51 120 80"
    );
}

#[test]
fn a_press_outside_a_grabbing_popup_closes_the_chain() {
    let mut seinas = Compositor::start_with_input("pc");
    let mut client = window(&seinas, "300x200");
    // ウィンドウを押した serial で grab する。キーボードのフォーカスは、ポップアップへ移る。
    click_client(&mut seinas, &client, 100, 100);
    open_grabbing_popup(&mut client, "popup 20 30 100 60 grab", 1);
    // ポップアップの中を押すと、ポップアップへ渡り、閉じない。
    click(&mut seinas, 60, BAR as i32 + 60);
    client.wait_for_line("window: pointer enter popup 1 39 29");
    client.wait_for_line("window: pointer button 0x110 pressed");
    // 入れ子のポップアップ(ポップアップを押した serial で grab)。
    open_grabbing_popup(&mut client, "popup 10 10 50 30 grab child", 2);
    // Esc は、いちばん上のポップアップへ渡る(閉じるかは、クライアントが決める)。
    seinas.pointer(&format!("key {KEY_ESC} press"));
    seinas.pointer(&format!("key {KEY_ESC} release"));
    client.wait_for_line("window: key 1 pressed");
    // 同じクライアントのウィンドウ(ポップアップの外)を押しても閉じず、そのウィンドウへ渡る(xdg-shell の
    // owner-events。閉じるかは、クライアントが決める)。
    click(&mut seinas, 250, BAR as i32 + 150);
    client.wait_for_line("window: pointer enter 250 150");
    client.wait_for_line("window: pointer button 0x110 released");
    // 何も無い所(ウィンドウの外)を押すと、上のポップアップから順に閉じ、フォーカスはウィンドウへ戻る。
    click(&mut seinas, 600, 450);
    client.wait_for_line("window: popup 2 done");
    client.wait_for_line("window: popup 1 done");
    client.wait_for_line("window: keyboard enter");
    seinas.wait_for_screen("the popups closed", |shot| {
        shot.pixel(30, BAR + 40) == ORANGE
    });
    // ポップアップが閉じた後は、ウィンドウにふつうにキーが届く。
    seinas.pointer("key 30 press");
    seinas.pointer("key 30 release");
    client.wait_for_line("window: key 30 pressed");
}

#[test]
fn the_title_bar_and_seinas_operations_close_the_chain() {
    let mut seinas = Compositor::start_with_input("pd");
    let mut client = window(&seinas, "300x200");
    // タイトルバー(Seinas のもの)を押すと、閉じる。
    click_client(&mut seinas, &client, 100, 100);
    open_grabbing_popup(&mut client, "popup 20 30 60 40 grab", 1);
    click(&mut seinas, 100, 10);
    client.wait_for_line("window: popup 1 done");
    // Alt+Tab(切り替えを始める)で、閉じる。2 つ目のウィンドウ(スロット 1。外枠の左上は (32, 32))を出しておき、
    // 1 つ目のウィンドウの、2 つ目に覆われない所(x 280)を押して手前に戻す。
    let _other = window(&seinas, "200x150");
    click_client(&mut seinas, &client, 280, 100);
    open_grabbing_popup(&mut client, "popup 20 30 60 40 grab", 2);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    seinas.pointer(&format!("key {KEY_TAB} press"));
    seinas.pointer(&format!("key {KEY_TAB} release"));
    client.wait_for_line("window: popup 2 done");
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    // 1 つ目のウィンドウを手前に戻して、ポップアップを出し直し、Alt+ドラッグで閉じる。
    click_client(&mut seinas, &client, 280, 100);
    open_grabbing_popup(&mut client, "popup 20 30 60 40 grab", 3);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    seinas.pointer("motion 280 150");
    seinas.pointer("press");
    client.wait_for_line("window: popup 3 done");
    seinas.pointer("motion 300 200");
    seinas.pointer("release");
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    // Alt+ドラッグは、ふつうに働く(ウィンドウは (20, 50) 動き、コンテンツは x 20〜320、y BAR + 50〜BAR + 250 になる)。
    seinas.wait_for_screen("the window moved by Alt+drag", |shot| {
        shot.pixel(310, BAR + 240) == ORANGE && shot.pixel(10, BAR + 20) != ORANGE
    });
}

#[test]
fn another_window_taking_the_focus_closes_the_chain() {
    let mut seinas = Compositor::start_with_input("pe");
    let mut client = window(&seinas, "300x200");
    click_client(&mut seinas, &client, 100, 100);
    open_grabbing_popup(&mut client, "popup 20 30 60 40 grab", 1);
    // 新しいウィンドウが手前に来て activated になると、閉じる。
    let _other = window(&seinas, "200x150");
    client.wait_for_line("window: popup 1 done");
}

#[test]
fn a_layer_surface_popup_flips_above_a_bottom_panel() {
    let mut seinas = Compositor::start_with_input("pf");
    // 画面の下の端(y 440〜480)の、タスクバーのような top の layer surface。
    let mut panel = seinas.connect_shell_client(&[
        "layer",
        "--anchor",
        "bottom,left,right",
        "--size",
        "0x40",
        "--color",
        "2040c0",
        "--pointer",
        "--keyboard",
    ]);
    panel.wait_for_line("layer: drawn");
    let window = window(&seinas, "300x200");
    // パネルを押した serial で、下へ開くポップアップを grab する。画面の外へ出るので、上へ反転する(flip_y 8)。
    click(&mut seinas, 10, 450);
    panel.wait_for_line("window: pointer button 0x110 released");
    panel.send_line("popup 10 0 100 60 adjust=8 grab");
    let mut lines = Vec::new();
    while lines.last().map(String::as_str) != Some("window: popup 1 drawn 100x60") {
        lines.push(panel.next_line("the popup drawn"));
    }
    assert!(
        lines
            .iter()
            .any(|l| l == "window: popup 1 configure 11 -60 100 60"),
        "{lines:?}"
    );
    // grab で、キーボードのフォーカスはポップアップへ移る。
    assert!(
        lines.iter().any(|l| l == "window: keyboard enter popup 1"),
        "{lines:?}"
    );
    seinas.wait_for_screen("the popup above the panel", |shot| {
        filled(shot, (11, 380, 100, 60), PURPLE) && shot.pixel(10, 450) == [0x20, 0x40, 0xc0]
    });
    // ポップアップの中の押下は、ポップアップへ渡る。
    click(&mut seinas, 60, 400);
    panel.wait_for_line("window: pointer enter popup 1 49 20");
    // ほかのクライアント(ウィンドウ)を押すと閉じ、その押下はウィンドウへ渡らない。
    click(&mut seinas, 100, 100);
    panel.wait_for_line("window: popup 1 done");
    let lines = window.lines_within(std::time::Duration::from_millis(500));
    assert!(
        !lines
            .iter()
            .any(|l| l.starts_with("window: pointer button")),
        "the press reached the window: {lines:?}"
    );
}

#[test]
fn a_menu_opened_by_a_held_press_does_not_leave_the_press_held() {
    let mut seinas = Compositor::start_with_input("pg");
    let mut client = window(&seinas, "200x150");
    let other = window(&seinas, "200x150");
    // 1 つ目(コンテンツは (0, BAR)〜(200, BAR + 150))を、2 つ目((32, 32 + BAR) から)に覆われない所で押して、
    // raise する。2 つ目は、右下の (220, 200) のあたりだけが見える。
    click_client(&mut seinas, &client, 10, 100);
    other.wait_for_line("window: keyboard leave");
    // 右のボタンで押したまま、その押下の serial でメニューを出し、離す(右クリックのメニューと同じ。離す側は、
    // ポップアップの grab の間に届く)。
    seinas.pointer("press right");
    client.wait_for_line("window: pointer button 0x111 pressed");
    open_grabbing_popup(&mut client, "popup 5 60 60 40 grab", 1);
    seinas.pointer("release right");
    client.wait_for_line("window: pointer button 0x111 released");
    // 何も無い所を押して閉じる。
    click(&mut seinas, 600, 450);
    client.wait_for_line("window: popup 1 done");
    // その後も、2 つ目を押すと、ふだんどおり raise され、キーボードのフォーカスが移る。
    click(&mut seinas, 220, 200);
    other.wait_for_line("window: keyboard enter");
}
