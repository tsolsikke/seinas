//! 壁紙(seinas-wallpaper)を fake screen につなぎ、ピクセルで確かめる。
//!
//! 決まり(`docs/wallpaper.md`):
//! - 壁紙は、シェルの部品のソケットにつなぎ、layer-shell の background の層に、出力いっぱい(タスクバーの下も)の
//!   面を出す。ウィンドウとタスクバーは、その上に描かれる。
//! - 設定が無いときは、コンポジタの背景と同じ色だけを出す(見た目が変わらない)。
//! - 色だけの指定と、PNG の画像の指定ができる。読めない画像は、ログに出して色だけで動く。
//! - 再描画するのは、大きさが変わったときだけ。

mod common;

use std::{fs, path::Path};

use common::{wait_for, Compositor, Shot, BACKGROUND, BAR, HEIGHT, WIDTH};

/// テスト用のクライアントのウィンドウの色(橙)。
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// タスクバーの高さと地の色。
const TASKBAR: usize = 40;
const TASKBAR_BACKGROUND: [u8; 3] = [0x20, 0x24, 0x2c];
/// 設定で使う色。
const BLUE: [u8; 3] = [0x33, 0x66, 0x99];

/// `seinas/wallpaper.conf` に `text` を書く。
fn write_config(seinas: &Compositor, text: &str) {
    let dir = seinas.config_dir().join("seinas");
    fs::create_dir_all(&dir).expect("the config directory");
    fs::write(dir.join("wallpaper.conf"), text).expect("write the wallpaper config");
}

/// 壁紙が `width`x`height` の絵を出したと、ログに書くまで待つ。
fn wait_for_drawing(seinas: &Compositor, size: &str) {
    wait_for("the wallpaper drawn", || {
        seinas
            .wallpaper_log()
            .contains(&format!("drew {size}"))
            .then_some(())
    });
}

fn test_image() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../seinas-wallpaper/tests/data/test-48x32.png")
        .display()
        .to_string()
}

#[test]
fn without_a_config_the_screen_looks_the_same() {
    let seinas = Compositor::start("pa");
    let _wallpaper = seinas.connect_wallpaper();
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    let _window = seinas.connect_test_client(&["window"]);
    seinas.wait_for_screen("the window over the wallpaper", |shot| {
        shot.pixel(10, BAR + 10) == ORANGE
    });
    // 窓の外は、コンポジタの背景と同じ色。
    let shot = seinas.shot();
    for (x, y) in [
        (0, HEIGHT - 1),
        (WIDTH - 1, 0),
        (WIDTH / 2, HEIGHT / 2),
        (WIDTH - 1, HEIGHT - 1),
    ] {
        assert_eq!(shot.pixel(x, y), BACKGROUND, "({x}, {y})");
    }
    assert!(seinas.wallpaper_log().contains("color 191e28"));
}

#[test]
fn a_color_fills_the_whole_output_under_the_taskbar_and_windows() {
    let seinas = Compositor::start("pb");
    write_config(&seinas, "color = 336699\n");
    let _taskbar = seinas.connect_taskbar();
    let _wallpaper = seinas.connect_wallpaper();
    // タスクバーの exclusive zone にかかわらず、出力いっぱいの大きさをもらう。
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    let _window = seinas.connect_test_client(&["window"]);
    seinas.wait_for_screen(
        "the wallpaper, the window and the taskbar",
        |shot: &Shot| {
            shot.pixel(WIDTH - 1, 0) == BLUE
                && shot.pixel(WIDTH / 2, HEIGHT - TASKBAR - 1) == BLUE
                && shot.pixel(10, BAR + 10) == ORANGE
                && shot.pixel(WIDTH - 1, HEIGHT - 1) == TASKBAR_BACKGROUND
        },
    );
    // 後からタスクバーがつながっても、大きさは変わらないので、再描画しない。
    let _second_taskbar = seinas.connect_taskbar();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(seinas.wallpaper_log().matches("drew ").count(), 1);
}

#[test]
fn an_image_is_placed_and_its_margins_get_the_color() {
    let seinas = Compositor::start("pc");
    write_config(
        &seinas,
        &format!("color = 336699\nimage = {}\nmode = fit\n", test_image()),
    );
    let _wallpaper = seinas.connect_wallpaper();
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    // 3:2 の画像を 640x480 に合わせると、640x427 になり、上下に 26 と 27 ピクセルの余白(色)ができる。
    seinas.wait_for_screen("the picture with margins", |shot: &Shot| {
        let [r, _, b] = shot.pixel(40, 120);
        shot.pixel(WIDTH / 2, 10) == BLUE
            && shot.pixel(WIDTH / 2, HEIGHT - 10) == BLUE
            && shot.pixel(WIDTH / 2, HEIGHT / 2)[0] < 0x20
            && r > 0xc0
            && b < 0x40
    });
}

#[test]
fn an_unreadable_image_leaves_only_the_color() {
    let seinas = Compositor::start("pd");
    write_config(&seinas, "color = 336699\nimage = missing.png\n");
    let _wallpaper = seinas.connect_wallpaper();
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    seinas.wait_for_screen("only the color", |shot: &Shot| {
        shot.pixel(WIDTH / 2, HEIGHT / 2) == BLUE && shot.pixel(0, 0) == BLUE
    });
    assert!(seinas.wallpaper_log().contains("only the color is shown"));
}

#[test]
fn the_wallpaper_needs_the_shell_socket() {
    let seinas = Compositor::start("pe");
    let log = seinas.config_dir().join("plain.log");
    let status = std::process::Command::new(common::workspace_bin(
        "seinas-wallpaper",
        "seinas-wallpaper",
    ))
    .env("WAYLAND_DISPLAY", seinas.socket.as_os_str())
    .env("XDG_CONFIG_HOME", seinas.config_dir())
    .stderr(fs::File::create(&log).expect("the log"))
    .status()
    .expect("run seinas-wallpaper");
    assert!(!status.success());
    assert!(fs::read_to_string(&log)
        .expect("the log")
        .contains("zwlr_layer_shell_v1"));
}
