//! 最大化しているウィンドウをドラッグで戻すことと、上端へのドラッグで最大化することを、fake screen のピクセルと、
//! テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「最大化・最小化」):
//! - 最大化しているウィンドウのタイトルバーを 8 ピクセル動かすと、元の大きさに戻り、そのまま動く。ポインターは、
//!   元の大きさのタイトルバーの、横の同じ割合の所(ボタンの上は避ける)に来る。8 ピクセルの手前で離したら、何もしない。
//! - 動かしている最中に、ポインターが使える範囲の上端に触れると、最大化の予告(使える範囲の内側に沿った、太さ
//!   4 ピクセルの枠)を出し、そこで離すと最大化する。上端から離れると、予告を消す。大きさの決まったウィンドウでは、
//!   予告も最大化もしない。
//! - どちらの操作でも、foreign-toplevel の maximized の状態が合い、キーボードのフォーカスと activated は動かない。
//!
//! ポインターのイベントは、テスト用の入力インターフェース(`--test-input`)から送る。

mod common;

use common::{bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, BAR, HEIGHT, WIDTH};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// 最大化の予告の枠の色(activated のタイトルバーと同じ)。
const MARK: [u8; 3] = ACTIVE_BAR;

/// 外枠の左上が (`x0`, `y0`)、コンテンツが `w` × `h` のウィンドウが、activated のタイトルバーで描かれているか。
fn shows_window(shot: &Shot, (x0, y0): (usize, usize), (w, h): (usize, usize)) -> bool {
    bar_color(shot, x0, y0, w) == Some(ACTIVE_BAR)
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + w - 1, y0 + BAR + h - 1) == ORANGE
}

/// 最大化しているウィンドウが描かれているか。
fn shows_maximized(shot: &Shot) -> bool {
    shows_window(shot, (0, 0), (WIDTH, HEIGHT - BAR))
}

/// 最大化の予告の枠が出ているか(右の辺と下の辺の、ウィンドウの無い所で見る)。
fn shows_mark(shot: &Shot) -> bool {
    shot.pixel(WIDTH - 2, HEIGHT / 2) == MARK && shot.pixel(WIDTH / 2 + 100, HEIGHT - 2) == MARK
}

/// 次の configure の行(`WxH [状態]`)。状態は名前の順に並べ直す。ほかの行は読み飛ばす。
fn next_configure(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        if let Some(rest) = line.strip_prefix("window: configure ") {
            let (size, states) = rest.split_once(' ').expect("the size and the states");
            let mut states: Vec<&str> = states
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .filter(|s| !s.is_empty())
                .collect();
            states.sort();
            return format!("{size} [{}]", states.join(","));
        }
    }
}

/// 一覧(`test-client toplevels`)で、ウィンドウ 1 の状態が `state`(名前の順にコンマで並べたもの)になるまで
/// 待つ。状態の並ぶ順は問わない。
fn wait_for_state(list: &TestClient, state: &str) {
    loop {
        let line = list.next_line(state);
        let Some(rest) = line.strip_prefix(r#"toplevel 1: title="" app_id="" state=["#) else {
            continue;
        };
        let (states, _) = rest.split_once(']').expect("the states");
        let mut states: Vec<&str> = states.split(',').filter(|s| !s.is_empty()).collect();
        states.sort();
        if states.join(",") == state {
            return;
        }
    }
}

/// キーボードのイベントの、次の行。ほかの行は読み飛ばす。
fn next_keyboard_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        let keyboard = ["window: key ", "window: keyboard ", "window: modifiers "]
            .iter()
            .any(|prefix| line.starts_with(prefix));
        if keyboard {
            return line;
        }
    }
}

/// キーボードのフォーカスが、操作の間ずっと動かなかったこと。キーを 1 つ送って、その前に leave も enter も
/// 届いていないことを見る。
fn expect_the_focus_kept(seinas: &mut Compositor, window: &TestClient) {
    seinas.pointer("key 30 press");
    seinas.pointer("key 30 release");
    assert_eq!(
        next_keyboard_line(window, "the key"),
        "window: key 30 pressed"
    );
}

/// 一覧のシェルの部品をつないだ構成で、キーボードを使うウィンドウを出す(最初のフォーカスまで読む)。
fn start(name: &str, args: &[&str]) -> (Compositor, TestClient, TestClient) {
    let seinas = Compositor::start_with_input(name);
    let list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");
    let window =
        seinas.connect_test_client(&[&["window", "--follow-size", "--keyboard"], args].concat());
    window.wait_for_line("window: drawn");
    assert_eq!(
        next_keyboard_line(&window, "enter"),
        "window: keyboard enter"
    );
    wait_for_state(&list, "activated");
    (seinas, window, list)
}

#[test]
fn dragging_a_maximized_window_restores_it_under_the_pointer() {
    let (mut seinas, mut window, list) = start("da", &[]);
    window.send_line("maximize");
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    wait_for_state(&list, "activated,maximized");
    seinas.wait_for_screen("the maximized window", shows_maximized);

    // 横の 100 / 640 の所を grab する。8 ピクセルの手前では、何も起きない。
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("motion 105 10");
    // 10 ピクセル動かすと、元の大きさ(200x150)に戻る。ポインターは、元のタイトルバーの 100 / 640 の所
    // (31 ピクセル)に来るので、外枠の左上は (79, 0)。そのまま動かすと、ついてくる。
    seinas.pointer("motion 110 10");
    assert_eq!(
        next_configure(&window, "restored"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );
    wait_for_state(&list, "activated");
    seinas.pointer("motion 140 60");
    seinas.pointer("release");
    seinas.wait_for_screen("the restored window under the pointer", |shot| {
        shows_window(shot, (109, 50), (WINDOW_WIDTH, WINDOW_HEIGHT))
            && shot.pixel(105, 100) == BACKGROUND
    });
    expect_the_focus_kept(&mut seinas, &window);
}

#[test]
fn releasing_before_the_slack_keeps_a_maximized_window() {
    let (mut seinas, mut window, list) = start("db", &[]);
    window.send_line("maximize");
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    wait_for_state(&list, "activated,maximized");
    // 7 ピクセルまで動かして離しても、最大化したまま。
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("motion 107 10");
    seinas.pointer("release");
    // 少し時間を空けて、ダブルクリックにしない。その後の、処理が済んだことを sync で確かめる。
    std::thread::sleep(std::time::Duration::from_millis(600));
    window.send_line("sync");
    window.wait_for_line("window: synced");
    seinas.wait_for_screen("still maximized", shows_maximized);
    // ダブルクリックは、今までどおり元に戻す。
    seinas.pointer("press");
    seinas.pointer("release");
    seinas.pointer("press");
    seinas.pointer("release");
    assert_eq!(
        next_configure(&window, "restored by a double click"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );
    wait_for_state(&list, "activated");
    expect_the_focus_kept(&mut seinas, &window);
}

#[test]
fn the_top_of_the_screen_previews_and_maximizes() {
    let (mut seinas, window, list) = start("dc", &[]);
    seinas.wait_for_screen("the window", |shot| {
        shows_window(shot, (0, 0), (WINDOW_WIDTH, WINDOW_HEIGHT))
    });
    // grab して下へ動かし、上の端へ戻すと、予告が出る。
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("motion 100 60");
    seinas.pointer("motion 100 0");
    seinas.wait_for_screen("the mark", shows_mark);
    // 上の端から離れると、予告は消える。
    seinas.pointer("motion 100 30");
    seinas.wait_for_screen("no mark", |shot| {
        shot.pixel(WIDTH - 2, HEIGHT / 2) == BACKGROUND
            && shot.pixel(WIDTH / 2 + 100, HEIGHT - 2) == BACKGROUND
    });
    // 上の端で離すと、最大化する。予告は消える。
    seinas.pointer("motion 100 0");
    seinas.wait_for_screen("the mark again", shows_mark);
    seinas.pointer("release");
    assert_eq!(
        next_configure(&window, "maximized at the top"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    wait_for_state(&list, "activated,maximized");
    seinas.wait_for_screen("the maximized window without the mark", shows_maximized);
    let mut seinas = seinas;
    expect_the_focus_kept(&mut seinas, &window);
}

#[test]
fn a_window_of_a_fixed_size_is_not_maximized_at_the_top() {
    let (mut seinas, mut window, _list) =
        start("dd", &["--min-size", "200x150", "--max-size", "200x150"]);
    seinas.wait_for_screen("the window", |shot| {
        shows_window(shot, (0, 0), (WINDOW_WIDTH, WINDOW_HEIGHT))
    });
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("motion 100 60");
    seinas.wait_for_screen("the window moved", |shot| {
        shows_window(shot, (0, 50), (WINDOW_WIDTH, WINDOW_HEIGHT))
    });
    seinas.pointer("motion 100 0");
    seinas.pointer("release");
    window.send_line("sync");
    window.wait_for_line("window: synced");
    // 上の端へ動いただけで、予告も最大化も無い。
    let shot = seinas.wait_for_shot("the window at the top", |shot| {
        shows_window(shot, (0, 0), (WINDOW_WIDTH, WINDOW_HEIGHT))
    });
    assert_eq!(shot.pixel(WIDTH - 2, HEIGHT / 2), BACKGROUND);
    expect_the_focus_kept(&mut seinas, &window);
}
