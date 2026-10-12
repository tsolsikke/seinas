//! Alt+Tab でのウィンドウの切り替えを、fake screen のピクセルと、テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「Alt+Tab」):
//! - Alt を押したまま Tab で、activated のウィンドウの次から、最後に activated だった順に候補を進める。
//!   Alt+Shift+Tab は、逆の順。候補は、いちばん手前に見せるだけ(activated とフォーカスは、決めるまで変えない)。
//! - Alt を離したら決める(raise、activated、キーボードのフォーカス)。Esc で取り消す。
//! - 最小化しているウィンドウも候補で、決めたら戻す。
//! - Seinas が使った Tab と Esc は、クライアントへ渡さない。Alt を押したこと・離したことは、フォーカスのある
//!   クライアントへ渡す(どのクライアントにも、Alt が押されたまま残らない)。
//!
//! キーは、テスト用の入力インターフェース(`--test-input`)から送る。
//!
//! ウィンドウ a、b、c を、それぞれの色で、この順に出す。位置は (0, 0)、(32, 32)、(64, 64) で、点
//! (100, 120) には、3 つのコンテンツが重なっている。そこの色で、いちばん手前に見えているウィンドウが分かる。

mod common;

use common::{bar_color, Compositor, TestClient, ACTIVE_BAR, BACKGROUND, BAR};

/// evdev のキーの番号。
const ESC: u32 = 1;
const TAB: u32 = 15;
const A: u32 = 30;
const SHIFT: u32 = 42;
const ALT: u32 = 56;

/// 3 つのウィンドウのコンテンツが重なる点。
const OVERLAP: (usize, usize) = (100, 120);
const RED: [u8; 3] = [0xd0, 0x20, 0x20];
const GREEN: [u8; 3] = [0x20, 0xa0, 0x40];
const BLUE: [u8; 3] = [0x20, 0x40, 0xd0];
/// ウィンドウの、コンテンツの幅。
const WINDOW_WIDTH: usize = 200;

/// キーボードのイベントの、次の行。ほかの行(configure、keymap など)は読み飛ばす。
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

/// キーボードのイベントが、この順に届くこと。
fn expect_keyboard(client: &TestClient, lines: &[&str]) {
    for line in lines {
        assert_eq!(next_keyboard_line(client, line), format!("window: {line}"));
    }
}

/// フォーカスが来たこと。押されているキーは無く、修飾キーも押されていない。
const ENTER: [&str; 2] = ["keyboard enter", "modifiers 0 0 0 0"];
/// Alt を押して、離したこと(Alt は Mod1 で、8)。
const ALT_PRESSED: [&str; 2] = ["key 56 pressed", "modifiers 8 0 0 0"];
const ALT_RELEASED: [&str; 2] = ["key 56 released", "modifiers 0 0 0 0"];

fn open_window(seinas: &Compositor, title: &str, color: &str) -> TestClient {
    let window =
        seinas.connect_test_client(&["window", "--keyboard", "--title", title, "--color", color]);
    window.wait_for_line("window: drawn");
    window
}

/// a(赤)、b(緑)、c(青)を出す。c が activated で、フォーカスもある。それぞれの、最初のフォーカスと、
/// 次のウィンドウが出てフォーカスが外れたことは、読んでおく。
fn three_windows(seinas: &Compositor) -> [TestClient; 3] {
    let a = open_window(seinas, "a", "d02020");
    expect_keyboard(&a, &ENTER);
    let b = open_window(seinas, "b", "20a040");
    expect_keyboard(&a, &["keyboard leave"]);
    expect_keyboard(&b, &ENTER);
    let c = open_window(seinas, "c", "2040d0");
    expect_keyboard(&b, &["keyboard leave"]);
    expect_keyboard(&c, &ENTER);
    seinas.wait_for_screen("c in front", |shot| {
        shot.pixel(OVERLAP.0, OVERLAP.1) == BLUE
    });
    [a, b, c]
}

fn press(seinas: &mut Compositor, code: u32) {
    seinas.pointer(&format!("key {code} press"));
}

fn release(seinas: &mut Compositor, code: u32) {
    seinas.pointer(&format!("key {code} release"));
}

fn tap(seinas: &mut Compositor, code: u32) {
    press(seinas, code);
    release(seinas, code);
}

/// いちばん手前に `color` のウィンドウが見えるまで待つ。
fn wait_for_front(seinas: &Compositor, what: &str, color: [u8; 3]) {
    seinas.wait_for_screen(what, |shot| shot.pixel(OVERLAP.0, OVERLAP.1) == color);
}

#[test]
fn alt_tab_goes_through_the_windows_in_the_order_they_were_activated() {
    let mut seinas = Compositor::start_with_input("sa");
    let [a, b, c] = three_windows(&seinas);

    // Alt を押したことは、フォーカスのある c に届く。Tab は届かず、b を手前に見せる。
    press(&mut seinas, ALT);
    expect_keyboard(&c, &ALT_PRESSED);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "b as the candidate", GREEN);
    // 見せている間も、activated は c のまま(c のタイトルバーは activated の色)。
    assert_eq!(
        bar_color(&seinas.shot(), 64, 64, WINDOW_WIDTH),
        Some(ACTIVE_BAR)
    );
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "a as the candidate", RED);

    // Alt を離すと、決める。c は Alt を離したことを受け取ってから、フォーカスを失う。a には、押されている
    // キーの無い enter が届く。
    release(&mut seinas, ALT);
    expect_keyboard(&c, &ALT_RELEASED);
    expect_keyboard(&c, &["keyboard leave"]);
    expect_keyboard(&a, &ENTER);
    seinas.wait_for_screen("a activated in front", |shot| {
        shot.pixel(OVERLAP.0, OVERLAP.1) == RED
            && bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });
    // キーは a に届く。c に Tab は届いていない(次の行が、a へ移った後のもの)。
    tap(&mut seinas, A);
    expect_keyboard(&a, &["key 30 pressed", "key 30 released"]);

    // 最後に activated だった順は a、c、b。Alt+Tab 1 回で c へ戻る。
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    release(&mut seinas, ALT);
    expect_keyboard(&a, &ALT_PRESSED);
    expect_keyboard(&a, &ALT_RELEASED);
    expect_keyboard(&a, &["keyboard leave"]);
    expect_keyboard(&c, &ENTER);
    wait_for_front(&seinas, "c back in front", BLUE);
    // b には、ずっと何も届いていない。
    tap(&mut seinas, A);
    expect_keyboard(&c, &["key 30 pressed"]);
    drop(b);
}

#[test]
fn alt_shift_tab_goes_backward() {
    let mut seinas = Compositor::start_with_input("sb");
    let [a, b, c] = three_windows(&seinas);
    press(&mut seinas, ALT);
    press(&mut seinas, SHIFT);
    expect_keyboard(&c, &ALT_PRESSED);
    expect_keyboard(&c, &["key 42 pressed", "modifiers 9 0 0 0"]);
    // 逆の順:c の前は、いちばん後ろの a。その前は b。
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "a as the candidate", RED);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "b as the candidate", GREEN);
    // Shift を離して Tab なら、次へ(a)。もう一度 Shift で、前(b)。
    release(&mut seinas, SHIFT);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "a again", RED);
    press(&mut seinas, SHIFT);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "b again", GREEN);
    // Shift を押したまま Alt を離すと、b に決まる。Shift は b へ引き継がれ、離したことも b に届く。
    release(&mut seinas, ALT);
    expect_keyboard(
        &c,
        &[
            "key 42 released",
            "modifiers 8 0 0 0",
            "key 42 pressed",
            "modifiers 9 0 0 0",
            "key 56 released",
            "modifiers 1 0 0 0",
        ],
    );
    expect_keyboard(&c, &["keyboard leave"]);
    expect_keyboard(&b, &["keyboard enter keys 42", "modifiers 1 0 0 0"]);
    release(&mut seinas, SHIFT);
    expect_keyboard(&b, &["key 42 released", "modifiers 0 0 0 0"]);
    seinas.wait_for_screen("b activated in front", |shot| {
        shot.pixel(OVERLAP.0, OVERLAP.1) == GREEN
            && bar_color(shot, 32, 32, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });
    drop(a);
}

#[test]
fn esc_cancels_and_keeps_the_stack_and_the_focus() {
    let mut seinas = Compositor::start_with_input("sc");
    let [a, b, c] = three_windows(&seinas);
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "a as the candidate", RED);
    // 切り替えの最中のほかのキー(A)は、どこへも渡さない。
    tap(&mut seinas, A);
    tap(&mut seinas, ESC);
    wait_for_front(&seinas, "c in front again", BLUE);
    release(&mut seinas, ALT);
    // c には Alt だけが届き、フォーカスは c のまま(leave が無い)。Tab、A、Esc は届いていない。
    expect_keyboard(&c, &ALT_PRESSED);
    expect_keyboard(&c, &ALT_RELEASED);
    tap(&mut seinas, A);
    expect_keyboard(&c, &["key 30 pressed", "key 30 released"]);
    // 重なる順も元のまま(b のタイトルバーが c の上に出ていない)。
    let shot = seinas.shot();
    assert_eq!(bar_color(&shot, 64, 64, WINDOW_WIDTH), Some(ACTIVE_BAR));
    assert_eq!(shot.pixel(40, 32 + BAR + 2), GREEN, "b is above a");
    drop((a, b));
}

#[test]
fn a_minimized_window_is_a_candidate_and_comes_back() {
    let mut seinas = Compositor::start_with_input("sd");
    let [a, b, c] = three_windows(&seinas);
    let mut list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");
    // c を最小化すると、b が activated になる。
    list.send_line("minimize 3");
    expect_keyboard(&c, &["keyboard leave"]);
    expect_keyboard(&b, &ENTER);
    wait_for_front(&seinas, "b in front", GREEN);

    // 最後に activated だった順は b、c、a。Alt+Tab で、最小化している c を見せる。
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "the minimized c as the candidate", BLUE);
    release(&mut seinas, ALT);
    expect_keyboard(&b, &ALT_PRESSED);
    expect_keyboard(&b, &ALT_RELEASED);
    expect_keyboard(&b, &["keyboard leave"]);
    expect_keyboard(&c, &ENTER);
    list.wait_for_line(r#"toplevel 3: title="c" app_id="" state=[activated] outputs=1"#);
    seinas.wait_for_screen("c restored", |shot| {
        shot.pixel(OVERLAP.0, OVERLAP.1) == BLUE
            && bar_color(shot, 64, 64, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });
    drop(a);
}

#[test]
fn alt_tab_with_no_window_or_one_window() {
    let mut seinas = Compositor::start_with_input("se");
    // ウィンドウが無ければ、何も起きない。
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    release(&mut seinas, ALT);
    let window = open_window(&seinas, "a", "d02020");
    expect_keyboard(&window, &ENTER);

    // 1 つなら、そのウィンドウのまま。Tab は届かず、Alt は届く。フォーカスは動かない。
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    tap(&mut seinas, TAB);
    release(&mut seinas, ALT);
    expect_keyboard(&window, &ALT_PRESSED);
    expect_keyboard(&window, &ALT_RELEASED);
    tap(&mut seinas, A);
    expect_keyboard(&window, &["key 30 pressed", "key 30 released"]);
    assert_eq!(seinas.shot().pixel(10, BAR + 10), RED);
    assert!(seinas.process.is_running());
}

#[test]
fn a_candidate_that_closes_is_skipped() {
    let mut seinas = Compositor::start_with_input("sf");
    let [a, b, c] = three_windows(&seinas);
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "b as the candidate", GREEN);
    // 見せている b が閉じると、次の候補の a を見せる。
    drop(b);
    wait_for_front(&seinas, "a as the next candidate", RED);
    release(&mut seinas, ALT);
    expect_keyboard(&c, &ALT_PRESSED);
    expect_keyboard(&c, &ALT_RELEASED);
    expect_keyboard(&c, &["keyboard leave"]);
    expect_keyboard(&a, &ENTER);
    seinas.wait_for_screen("a activated", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
    });

    // 候補が全部閉じると、切り替えは終わる。
    press(&mut seinas, ALT);
    tap(&mut seinas, TAB);
    wait_for_front(&seinas, "c as the candidate", BLUE);
    drop(c);
    drop(a);
    seinas.wait_for_screen("no window", |shot| {
        shot.pixel(OVERLAP.0, OVERLAP.1) == BACKGROUND
    });
    release(&mut seinas, ALT);
    let window = open_window(&seinas, "d", "d02020");
    expect_keyboard(&window, &ENTER);
    assert!(seinas.process.is_running());
}
