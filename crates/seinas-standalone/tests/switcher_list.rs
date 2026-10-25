//! 切り替えの一覧(切り替えの最中に、出力の中央に出す絵)を、fake screen のピクセルで確かめる。
//!
//! 決まり(`docs/window-placement.md` の「Alt+Tab」):
//! - 切り替えを始めたら、出力の中央に一覧を出し、決めたとき・取り消したときに消す。
//! - 候補を切り替えの順に 1 行ずつ並べ、今の候補の行を強調する。
//! - 一覧は出力からはみ出さない(幅は 480 まで、行は入るだけ)。
//! - 途中で候補が閉じたら、一覧から外す。
//!
//! 一覧の置き方:幅 480(出力が狭ければ、出力の幅 - 32)、高さ 16 + 行の数 × 28 を、出力の中央に置く。行 i の
//! 強調は、一覧の左上から (4, 8 + 28 i + 1) の所から始まる。

mod common;

use common::{Compositor, Shot, TestClient};

const ESC: u32 = 1;
const TAB: u32 = 15;
const ALT: u32 = 56;

// 色は、テーマのトークン(`seinas-theme` の `switcher`)から読む。今の候補の行は、左の端の 2 列が枠(選んだものの色)。
const BORDER: [u8; 3] = seinas_theme::switcher::PANEL_BORDER;
const HIGHLIGHT: [u8; 3] = seinas_theme::color::SELECTION;
const PANEL: [u8; 3] = seinas_theme::switcher::PANEL_BACKGROUND;

/// 出力 (w, h) に、`rows` 行の一覧を置いたときの左上と大きさ。
fn panel(output: (usize, usize), rows: usize) -> (usize, usize, usize, usize) {
    let width = 480.min(output.0 - 32);
    let height = 16 + rows * 28;
    (
        (output.0 - width) / 2,
        (output.1 - height) / 2,
        width,
        height,
    )
}

/// 一覧が `rows` 行で出ていて、`current` 行目が強調されているか(ほかの行は強調されていない)。
fn shows_list(shot: &Shot, output: (usize, usize), rows: usize, current: usize) -> bool {
    let (x, y, w, h) = panel(output, rows);
    let row_top = |i: usize| y + 8 + i * 28 + 1;
    shot.width == output.0
        && shot.pixel(x, y) == BORDER
        && shot.pixel(x + w - 1, y + h - 1) == BORDER
        && shot.pixel(x + 2, y + 2) == PANEL
        // 一覧のすぐ外は、一覧ではない。
        && shot.pixel(x, y.saturating_sub(1)) != BORDER
        && shot.pixel(x, y + h) != BORDER
        && (0..rows).all(|i| (shot.pixel(x + 5, row_top(i) + 2) == HIGHLIGHT) == (i == current))
}

/// 一覧が無いか(出していたときの左上の枠の所に、枠の色が無い)。
fn no_list(shot: &Shot, output: (usize, usize), rows: usize) -> bool {
    let (x, y, w, _) = panel(output, rows);
    shot.pixel(x, y) != BORDER && shot.pixel(x + w - 1, y) != BORDER
}

fn open_windows(seinas: &Compositor, count: usize) -> Vec<TestClient> {
    (0..count)
        .map(|i| {
            let title = format!("ウィンドウ {i}");
            let window = seinas.connect_test_client(&["window", "--keyboard", "--title", &title]);
            window.wait_for_line("window: drawn");
            window
        })
        .collect()
}

const OUTPUT: (usize, usize) = (640, 480);

#[test]
fn the_list_shows_moves_and_hides_when_decided() {
    let mut seinas = Compositor::start_with_input("sl");
    let _windows = open_windows(&seinas, 3);
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    // 候補は、手前から c(いま)、b、a。最初の Tab で b(1 行目)。
    seinas.wait_for_screen("the list on b", |shot| shows_list(shot, OUTPUT, 3, 1));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    seinas.wait_for_screen("the list on a", |shot| shows_list(shot, OUTPUT, 3, 2));
    seinas.pointer(&format!("key {ALT} release"));
    seinas.wait_for_screen("the list hidden", |shot| no_list(shot, OUTPUT, 3));
}

#[test]
fn the_list_hides_when_cancelled() {
    let mut seinas = Compositor::start_with_input("sm");
    let _windows = open_windows(&seinas, 3);
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    seinas.wait_for_screen("the list", |shot| shows_list(shot, OUTPUT, 3, 1));
    seinas.pointer(&format!("key {ESC} press"));
    seinas.pointer(&format!("key {ESC} release"));
    seinas.wait_for_screen("the list hidden", |shot| no_list(shot, OUTPUT, 3));
    seinas.pointer(&format!("key {ALT} release"));
}

#[test]
fn the_list_stays_inside_a_small_output_with_many_windows() {
    let mut seinas = Compositor::start_with_input("sn");
    seinas.pointer("output 320x240");
    seinas.wait_for_screen("the small output", |shot| shot.width == 320);
    let _windows = open_windows(&seinas, 8);
    // 240 の高さには 6 行。8 個の候補の 2 つ目(1)が今の候補で、上から 0、1、2、3、4 の候補と「▼ ほか 3 件」。
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    seinas.wait_for_screen("the list inside 320x240", |shot| {
        shows_list(shot, (320, 240), 6, 1)
    });
    let (x, y, w, h) = panel((320, 240), 6);
    assert!(x + w <= 320 && y + h <= 240);
    seinas.pointer(&format!("key {ALT} release"));
    seinas.wait_for_screen("the list hidden", |shot| no_list(shot, (320, 240), 6));
}

#[test]
fn a_candidate_closed_during_the_switch_leaves_the_list() {
    let mut seinas = Compositor::start_with_input("so");
    let mut windows = open_windows(&seinas, 3);
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    seinas.wait_for_screen("the list on b", |shot| shows_list(shot, OUTPUT, 3, 1));
    // 今の候補 b(2 つ目に開いたウィンドウ)が閉じると、一覧は 2 行になり、次の候補 a(1 行目)を強調する。
    drop(windows.remove(1));
    seinas.wait_for_screen("the list without b", |shot| shows_list(shot, OUTPUT, 2, 1));
    seinas.pointer(&format!("key {ALT} release"));
    seinas.wait_for_screen("the list hidden", |shot| no_list(shot, OUTPUT, 2));
}
