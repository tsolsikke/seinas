//! 出力(画面)の大きさが変わったときの動きを、fake screen のピクセルと、クライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「出力の大きさが変わったとき」):
//! - wl_output と xdg-output のモードを送り直し、layer surface(壁紙、タスクバー)を並べ直す(大きさが変われば
//!   configure を送り直す)。
//! - 使える範囲を求め直し、最大化しているウィンドウに新しい大きさを伝え、画面の外に出たウィンドウを、
//!   タイトルバーが届く所へ戻す(動かすときの限りと同じ)。
//!
//! 大きさは、テスト用の入力インターフェースの `output WxH` の行で変える(入れ子の構成で、親の窓の大きさが
//! 変わったときと同じ道を通る)。

mod common;

use std::fs;

use common::{wait_for, Compositor, Shot, TestClient, ACTIVE_BAR, BAR, HEIGHT, WIDTH};

/// テスト用のクライアントのウィンドウの色(橙)と、壁紙の色、タスクバーの高さと地の色。
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
const BLUE: [u8; 3] = [0x33, 0x66, 0x99];
const TASKBAR: usize = 40;
const TASKBAR_BACKGROUND: [u8; 3] = [0x20, 0x24, 0x2c];

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

/// 壁紙が `size` の絵を出したと、ログに書くまで待つ。
fn wait_for_wallpaper(seinas: &Compositor, size: &str) {
    wait_for(&format!("the wallpaper drawn at {size}"), || {
        seinas
            .wallpaper_log()
            .contains(&format!("drew {size}"))
            .then_some(())
    });
}

#[test]
fn the_wallpaper_and_the_taskbar_follow_the_output() {
    let mut seinas = Compositor::start_with_input("oa");
    let dir = seinas.config_dir().join("seinas");
    fs::create_dir_all(&dir).expect("the config directory");
    fs::write(dir.join("wallpaper.conf"), "color = 336699\n").expect("the config");
    let _wallpaper = seinas.connect_wallpaper();
    let _taskbar = seinas.connect_taskbar();
    wait_for_wallpaper(&seinas, &format!("{WIDTH}x{HEIGHT}"));

    // 大きくすると、壁紙は新しい大きさで再描画し、タスクバーは新しい下の端に移る。
    seinas.pointer("output 800x600");
    wait_for_wallpaper(&seinas, "800x600");
    seinas.wait_for_screen("the larger output", |shot: &Shot| {
        shot.width == 800
            && shot.height == 600
            && shot.pixel(799, 0) == BLUE
            && shot.pixel(799, 600 - TASKBAR - 1) == BLUE
            && shot.pixel(799, 599) == TASKBAR_BACKGROUND
    });
    // 小さくしても、同じ。
    seinas.pointer("output 480x360");
    wait_for_wallpaper(&seinas, "480x360");
    seinas.wait_for_screen("the smaller output", |shot: &Shot| {
        shot.width == 480
            && shot.height == 360
            && shot.pixel(479, 360 - TASKBAR - 1) == BLUE
            && shot.pixel(479, 359) == TASKBAR_BACKGROUND
    });
    assert!(seinas.log().contains("output: 480x360"));
    // 同じ大きさを送っても、何も起きない(壁紙は再描画しない)。
    seinas.pointer("output 480x360");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(seinas.wallpaper_log().matches("drew ").count(), 3);
}

#[test]
fn a_maximized_window_gets_the_new_size() {
    let mut seinas = Compositor::start_with_input("ob");
    let mut window = seinas.connect_test_client(&["window", "--follow-size"]);
    window.wait_for_line("window: drawn");
    window.send_line("maximize");
    assert_eq!(
        next_configure(&window, "maximized"),
        format!("{WIDTH}x{} [activated,maximized]", HEIGHT - BAR)
    );
    seinas.pointer("output 800x600");
    assert_eq!(
        next_configure(&window, "maximized on the larger output"),
        format!("800x{} [activated,maximized]", 600 - BAR)
    );
    seinas.wait_for_screen(
        "the maximized window on the larger output",
        |shot: &Shot| {
            shot.width == 800 && shot.pixel(799, 599) == ORANGE && shot.pixel(1, 1) == ACTIVE_BAR
        },
    );
}

#[test]
fn a_window_outside_the_smaller_output_comes_back() {
    let mut seinas = Compositor::start_with_input("oc");
    let window = seinas.connect_test_client(&["window"]);
    window.wait_for_line("window: drawn");
    // タイトルバーを grab して、右下(外枠の左上 (500, 400))へ動かす。
    seinas.pointer("motion 100 10");
    seinas.pointer("press");
    seinas.pointer("motion 600 410");
    seinas.pointer("release");
    seinas.wait_for_screen("the window at the bottom right", |shot: &Shot| {
        shot.pixel(501, 401) == ACTIVE_BAR
    });
    // 400x300 にすると、外枠の左上は、右は 400 - 48 = 352、下は 300 - 24 = 276 まで戻る。
    seinas.pointer("output 400x300");
    seinas.wait_for_screen("the window brought back", |shot: &Shot| {
        shot.width == 400
            && shot.pixel(353, 277) == ACTIVE_BAR
            && shot.pixel(351, 277) != ACTIVE_BAR
            && shot.pixel(353, 275) != ACTIVE_BAR
    });
    // 戻したウィンドウは、また動かせる。
    seinas.pointer("motion 380 285");
    seinas.pointer("press");
    seinas.pointer("motion 128 75");
    seinas.pointer("release");
    seinas.wait_for_screen("the window moved again", |shot: &Shot| {
        shot.pixel(101, 67) == ACTIVE_BAR && shot.pixel(110, 100) == ORANGE
    });
}

#[test]
fn bad_output_lines_are_ignored() {
    let mut seinas = Compositor::start_with_input("od");
    seinas.pointer("output 0x0");
    seinas.pointer("output big");
    seinas.pointer("output 640x480");
    let window = seinas.connect_test_client(&["window"]);
    window.wait_for_line("window: drawn");
    let shot = seinas.shot();
    assert_eq!((shot.width, shot.height), (WIDTH, HEIGHT));
    assert!(seinas.process.is_running());
}
