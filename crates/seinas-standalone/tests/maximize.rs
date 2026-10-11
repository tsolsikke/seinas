//! 最大化・最小化・元に戻すを、fake screen のピクセルと、テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - タイトルバーの最大化のボタンとダブルクリックで、最大化と元に戻すを切り替える。最大化は、使える範囲いっぱい。
//! - 最大化しているウィンドウには、サイズ変更の境界が無い(タイトルバーのドラッグで戻すことは、`tests/drag_maximize.rs`)。使える範囲が変わったら、
//!   新しい範囲の大きさを伝え直す。
//! - 最小化のボタンは、foreign-toplevel の manager を bind しているシェルの部品があるときだけ出る。最小化した
//!   ウィンドウは描かれず、シェルの部品から戻す。シェルの部品が無くなったら、最小化しているウィンドウは戻る。
//! - クライアントからの最大化・元に戻す・最小化の頼み(xdg_toplevel)にも応える。
//!
//! ポインターのイベントは、テスト用の入力インターフェース(`--test-input`)から送る。テスト用のクライアント
//! (`test-client window --follow-size`)は、届いた大きさで再描画する。

mod common;

use common::{bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, BAR, HEIGHT, WIDTH};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// タイトルの色(ボタンの印も、この色)。
const ACTIVE_TITLE: [u8; 3] = [0xff, 0xff, 0xff];

/// 外枠の左上が (`x0`, `y0`)、コンテンツが `w` × `h` のウィンドウが、activated のタイトルバーで描かれているか。
fn shows_window(shot: &Shot, (x0, y0): (usize, usize), (w, h): (usize, usize)) -> bool {
    bar_color(shot, x0, y0, w) == Some(ACTIVE_BAR)
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + w - 1, y0 + BAR + h - 1) == ORANGE
}

/// 左上のウィンドウが、元の大きさ(200x150)で描かれているか。
fn shows_normal_window(shot: &Shot) -> bool {
    shows_window(shot, (0, 0), (WINDOW_WIDTH, WINDOW_HEIGHT))
        && shot.pixel(WINDOW_WIDTH + 2, BAR + 2) == BACKGROUND
}

/// 左上のウィンドウの、最小化のボタン(128〜152)に「−」の印があるか。
fn has_minimize_button(shot: &Shot) -> bool {
    shot.pixel(WINDOW_WIDTH - 72 + 7, 11) == ACTIVE_TITLE
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

/// (`x`, `y`)で左のボタンを押して、離す。
fn click(seinas: &mut Compositor, x: usize, y: usize) {
    seinas.pointer(&format!("motion {x} {y}"));
    seinas.pointer("press");
    seinas.pointer("release");
}

fn open_window(seinas: &Compositor, args: &[&str]) -> TestClient {
    let window = seinas.connect_test_client(&[&["window", "--follow-size"], args].concat());
    window.wait_for_line("window: drawn");
    window
}

#[test]
fn the_maximize_button_and_a_double_click_toggle_maximize() {
    let mut seinas = Compositor::start_with_input("xa");
    let window = open_window(&seinas, &[]);
    seinas.wait_for_screen("the window", shows_normal_window);

    // 最大化のボタン(152〜176)を押すと、使える範囲(画面の全体)いっぱいになる。
    click(&mut seinas, 164, 12);
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    seinas.wait_for_screen("the maximized window", |shot| {
        shows_window(shot, (0, 0), (WIDTH, HEIGHT - BAR))
    });
    // 最大化している間は、元に戻すのボタン(右の端から 2 つ目)。押すと、元の位置と大きさに戻る。
    click(&mut seinas, WIDTH - 36, 12);
    assert_eq!(
        next_configure(&window, "restored"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );
    seinas.wait_for_screen("the restored window", shows_normal_window);

    // タイトルバーのダブルクリックでも、切り替わる。
    click(&mut seinas, 60, 12);
    click(&mut seinas, 60, 12);
    assert_eq!(
        next_configure(&window, "maximized by a double click"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    seinas.wait_for_screen("the maximized window", |shot| {
        shows_window(shot, (0, 0), (WIDTH, HEIGHT - BAR))
    });
    // ダブルクリックで、元に戻る(最大化しているウィンドウのドラッグは、`tests/drag_maximize.rs`)。
    click(&mut seinas, 60, 12);
    click(&mut seinas, 60, 12);
    assert_eq!(
        next_configure(&window, "restored by a double click"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );
    seinas.wait_for_screen("the restored window", shows_normal_window);
}

#[test]
fn a_maximized_window_follows_the_usable_area_and_has_no_resize_border() {
    let mut seinas = Compositor::start_with_input("xb");
    let window = open_window(&seinas, &[]);
    click(&mut seinas, 164, 12);
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );

    // 上に exclusive zone 40 の bottom の layer surface が出ると、使える範囲が狭まり、大きさを伝え直す。
    // bottom の層はウィンドウより奥なので、パネルの上の点は、ウィンドウに当たるならウィンドウが受け取る。
    let panel = seinas.connect_shell_client(&[
        "layer",
        "--layer",
        "bottom",
        "--anchor",
        "top,left,right",
        "--size",
        "0x40",
        "--exclusive",
        "40",
    ]);
    panel.wait_for_line("layer: drawn");
    assert_eq!(
        next_configure(&window, "the new usable area"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - 40 - BAR)
    );
    seinas.wait_for_screen("the maximized window below the panel", |shot| {
        shows_window(shot, (0, 40), (WIDTH, HEIGHT - 40 - BAR))
    });

    // 外枠のすぐ上(y 36)は、最大化していなければサイズ変更の境界だが、最大化しているので何も起きない。
    seinas.pointer("motion 300 36");
    seinas.pointer("press");
    seinas.pointer("motion 300 10");
    seinas.pointer("release");
    // 次に届く configure は、元に戻したときのもの(サイズ変更の configure は届かない)。
    click(&mut seinas, 60, 52);
    click(&mut seinas, 60, 52);
    assert_eq!(
        next_configure(&window, "restored"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );
    seinas.wait_for_screen("the restored window below the panel", |shot| {
        shows_window(shot, (0, 40), (WINDOW_WIDTH, WINDOW_HEIGHT))
    });
}

#[test]
fn a_client_can_ask_to_be_maximized_and_minimized() {
    let mut seinas = Compositor::start_with_input("xc");
    let mut window = open_window(&seinas, &["--resize-on-press", "bottom-right"]);
    window.send_line("maximize");
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    // 最大化している間は、クライアントからのサイズ変更の頼みも受けない。
    seinas.wait_for_screen("the maximized window", |shot| {
        shows_window(shot, (0, 0), (WIDTH, HEIGHT - BAR))
    });
    seinas.pointer("motion 300 300");
    seinas.pointer("press");
    seinas.pointer("motion 350 350");
    seinas.pointer("release");
    window.send_line("unmaximize");
    assert_eq!(
        next_configure(&window, "restored"),
        format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT} [activated]")
    );

    // 戻す手段(シェルの部品)が無いので、最小化の頼みには応じない。
    window.send_line("minimize");
    window.send_line("sync");
    window.wait_for_line("window: synced");
    let list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");
    assert!(seinas.shot().pixel(2, BAR + 2) == ORANGE);
    // シェルの部品がつながった後は、応じる。
    window.send_line("minimize");
    list.wait_for_line(r#"toplevel 1: title="" app_id="" state=[minimized] outputs=1"#);
    seinas.wait_for_screen("no window", |shot| shot.pixel(2, BAR + 2) == BACKGROUND);
    // シェルの部品が去ると、戻す手段が無くなるので、最小化しているウィンドウは戻る。
    drop(list);
    seinas.wait_for_screen("the window back", shows_normal_window);
}

#[test]
fn the_minimize_button_appears_with_a_shell_component() {
    let mut seinas = Compositor::start_with_input("xd");
    let _window = open_window(&seinas, &[]);
    seinas.wait_for_screen("the window", shows_normal_window);
    assert!(!has_minimize_button(&seinas.shot()));

    // 一覧を受け取るシェルの部品がつながると、最小化のボタン(128〜152)が出る。
    let mut list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");
    seinas.wait_for_screen("the minimize button", has_minimize_button);
    // 押すと、描かれなくなり、一覧の状態が minimized になる。
    click(&mut seinas, 140, 12);
    list.wait_for_line(r#"toplevel 1: title="" app_id="" state=[minimized] outputs=1"#);
    seinas.wait_for_screen("no window", |shot| shot.pixel(2, BAR + 2) == BACKGROUND);
    // 最小化している間は、その場所を押しても、何も起きない(背景)。
    click(&mut seinas, 100, 100);
    // シェルの部品から activate すると、戻る。
    list.send_line("activate 1");
    list.wait_for_line(r#"toplevel 1: title="" app_id="" state=[activated] outputs=1"#);
    seinas.wait_for_screen("the window back", |shot| {
        shows_normal_window(shot) && has_minimize_button(shot)
    });
}
