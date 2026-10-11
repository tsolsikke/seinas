//! 閉じるのに応じないクライアントのうち、ping に答えないもの(応答なし)を見分けることを、fake screen のピクセルと、
//! テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「閉じる」):
//! - 閉じるボタンを押すと、close と一緒に ping を送る。決まった時間(テストでは `--ping-timeout 300` で 300 ミリ秒)の
//!   中に pong が返らなければ、応答なしの印(タイトルの後の「(応答なし)」と、暗い赤の閉じるボタン)を付ける。
//!   後から pong が返れば、外す。
//! - close に応じなくても pong を返すクライアントには、印を付けず、切らない。
//!
//! テスト用のクライアントの `--ignore-ping` は、ping に答えない(固まった)クライアントのまね、`--ignore-close` は、
//! close に応じないが pong は返すクライアントのまね。

mod common;

use std::time::Duration;

use common::{bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, BAR};

/// テスト用のクライアントのウィンドウの、コンテンツの幅と色(橙)。
const WINDOW_WIDTH: usize = 200;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// 応答なしのウィンドウの、閉じるボタンの地の色。
const UNRESPONSIVE_CLOSE_BUTTON: [u8; 3] = [0x8a, 0x1f, 0x14];
/// テストでの、pong を待つ時間(ミリ秒)。
const TIMEOUT_MS: u64 = 300;

/// 外枠の左上が (`x0`, `y0`) のウィンドウの、閉じるボタンの左上の角のすぐ内側のピクセル(印の無い所)。
fn close_ground(shot: &Shot, (x0, y0): (usize, usize)) -> [u8; 3] {
    shot.pixel(x0 + WINDOW_WIDTH - 23, y0 + 1)
}

/// 外枠の左上が (`x0`, `y0`) のウィンドウの閉じるボタンを、押して離す。その後、ポインターを外へ動かす
/// (閉じるボタンの色を、ポインターの乗っていない形で見るため)。
fn click_close(seinas: &mut Compositor, (x0, y0): (usize, usize)) {
    let (x, y) = (x0 + WINDOW_WIDTH - 12, y0 + BAR / 2);
    seinas.pointer(&format!("motion {x} {y}"));
    seinas.pointer("press");
    seinas.pointer("release");
    seinas.pointer("motion 600 450");
}

fn open_window(seinas: &Compositor, args: &[&str]) -> TestClient {
    let window = seinas.connect_test_client(&[&["window", "--keyboard"], args].concat());
    window.wait_for_line("window: drawn");
    window
}

fn start(name: &str) -> (Compositor, TestClient) {
    let seinas = Compositor::start_with_input_and(name, &["--ping-timeout", "300"]);
    let list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");
    (seinas, list)
}

#[test]
fn a_client_that_answers_the_ping_is_not_marked_or_cut_off() {
    let (mut seinas, _list) = start("nb");
    let mut stubborn = open_window(&seinas, &["--title", "stubborn", "--ignore-close"]);
    click_close(&mut seinas, (0, 0));
    stubborn.wait_for_line("window: close");
    // 待つ時間を過ぎても、印は付かない(pong が返っている)。
    std::thread::sleep(Duration::from_millis(3 * TIMEOUT_MS));
    stubborn.send_line("sync");
    stubborn.wait_for_line("window: synced");
    let shot = seinas.shot();
    assert_eq!(close_ground(&shot, (0, 0)), ACTIVE_BAR);
    // もう一度押しても、切らずに、もう一度頼むだけ。
    click_close(&mut seinas, (0, 0));
    stubborn.wait_for_line("window: close");
    std::thread::sleep(Duration::from_millis(3 * TIMEOUT_MS));
    stubborn.send_line("sync");
    stubborn.wait_for_line("window: synced");
    assert!(stubborn.process.is_running());
    assert_eq!(seinas.shot().pixel(10, BAR + 10), ORANGE);
    assert!(!seinas.log().contains("did not answer a ping"));
}

#[test]
fn a_late_pong_removes_the_mark() {
    let (mut seinas, _list) = start("nc");
    let mut slow = open_window(&seinas, &["--title", "slow", "--ignore-ping"]);
    click_close(&mut seinas, (0, 0));
    slow.wait_for_line("window: ping");
    seinas.wait_for_screen("the mark of no response", |shot| {
        close_ground(shot, (0, 0)) == UNRESPONSIVE_CLOSE_BUTTON
    });
    // 遅れて pong を返すと、印が外れる。
    slow.send_line("pong");
    slow.wait_for_line("window: ponged");
    seinas.wait_for_screen("the mark removed", |shot| {
        close_ground(shot, (0, 0)) == ACTIVE_BAR
    });
    assert!(seinas
        .log()
        .contains("answered the ping; the mark is removed"));
    // 印の無いウィンドウなので、閉じるボタンは、もう一度頼むだけ(ping も、もう一度送る)。
    click_close(&mut seinas, (0, 0));
    slow.wait_for_line("window: close");
    slow.wait_for_line("window: ping");
    assert!(slow.process.is_running());
    assert!(!seinas.log().contains("disconnecting a client"));
}

#[test]
fn an_ordinary_client_still_closes() {
    let (mut seinas, list) = start("nd");
    let mut back = open_window(&seinas, &["--title", "back"]);
    let front = open_window(&seinas, &["--title", "front"]);
    click_close(&mut seinas, (32, 32));
    front.wait_for_line("window: close");
    list.wait_for_line("toplevel 2: closed");
    // 奥のウィンドウが activated になる。印は、どこにも付かない。
    seinas.wait_for_screen("the back window activated", |shot| {
        bar_color(shot, 0, 0, WINDOW_WIDTH) == Some(ACTIVE_BAR)
            && close_ground(shot, (0, 0)) == ACTIVE_BAR
    });
    std::thread::sleep(Duration::from_millis(3 * TIMEOUT_MS));
    back.send_line("sync");
    back.wait_for_line("window: synced");
    assert_eq!(close_ground(&seinas.shot(), (0, 0)), ACTIVE_BAR);
    assert!(back.process.is_running());
}
