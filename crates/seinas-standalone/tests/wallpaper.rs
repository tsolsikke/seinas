//! 壁紙(seinas-wallpaper)を fake screen につなぎ、ピクセルで確かめる。
//!
//! 決まり(`docs/wallpaper.md`):
//! - 壁紙は、シェルの部品のソケットにつなぎ、layer-shell の background の層に、出力いっぱい(タスクバーの下も)の
//!   面を出す。ウィンドウとタスクバーは、その上に描かれる。
//! - 設定が無いときは、コンポジタの背景と同じ色だけを出す(見た目が変わらない)。
//! - 色だけの指定と、PNG の画像の指定ができる。読めない画像は、ログに出して色だけで動く。
//! - 設定のファイルが無いときは、同梱の壁紙の置き場で見つかった画像のうち、出力の比にいちばん近いものを、塗りつぶし
//!   で出す。見つからなければ色だけ。設定のファイルがあれば、同梱の壁紙は使わない。
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
    // ウィンドウの外は、コンポジタの背景と同じ色。
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
    // 同梱の壁紙の置き場は、どれも無い場所に向けてある(`connect_wallpaper`)。
    assert!(seinas
        .wallpaper_log()
        .contains("no config file and no bundled wallpaper"));
}

const RED: [u8; 3] = [0xc0, 0x20, 0x20];
const GREEN: [u8; 3] = [0x20, 0xa0, 0x40];

/// `path` に、幅 `width`・高さ `height` の単色 `rgb` の PNG を書く(圧縮しない deflate の、いちばん小さな形)。
fn write_png(path: &Path, width: u32, height: u32, rgb: [u8; 3]) {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let crc = crc32(&[kind, data].concat());
        [
            &(data.len() as u32).to_be_bytes()[..],
            kind,
            data,
            &crc.to_be_bytes(),
        ]
        .concat()
    }
    // 行ごとに、フィルターの種類 0 と、RGB の並び。
    let row: Vec<u8> = std::iter::once(0)
        .chain(rgb.repeat(width as usize))
        .collect();
    let raw = row.repeat(height as usize);
    assert!(raw.len() <= 0xffff, "one stored block");
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let len = raw.len() as u16;
    let zlib = [
        &[0x78, 0x01, 0x01][..],
        &len.to_le_bytes(),
        &(!len).to_le_bytes(),
        &raw,
        &((b << 16) | a).to_be_bytes(),
    ]
    .concat();
    let ihdr = [
        &width.to_be_bytes()[..],
        &height.to_be_bytes(),
        &[8, 2, 0, 0, 0],
    ]
    .concat();
    let png = [
        &b"\x89PNG\r\n\x1a\n"[..],
        &chunk(b"IHDR", &ihdr),
        &chunk(b"IDAT", &zlib),
        &chunk(b"IEND", &[]),
    ]
    .concat();
    fs::write(path, png).expect("write the PNG");
}

/// 同梱の壁紙の置き場のまね:16:9 の赤(`wide.png`)と、4:3 の緑(`four-three.png`)。
fn bundled_dir(seinas: &Compositor) -> std::path::PathBuf {
    let dir = seinas.config_dir().join("bundled");
    fs::create_dir_all(&dir).expect("the bundled directory");
    write_png(&dir.join("wide.png"), 32, 18, RED);
    write_png(&dir.join("four-three.png"), 32, 24, GREEN);
    dir
}

#[test]
fn without_a_config_the_bundled_wallpaper_of_the_closest_ratio_is_shown() {
    let mut seinas = Compositor::start_with_input("pf");
    let bundled = bundled_dir(&seinas);
    let _wallpaper = seinas.connect_wallpaper_with_bundled(Some(&bundled));
    // 640x480(4:3)には、4:3 の緑を、塗りつぶしで出す(余白が無い)。
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    seinas.wait_for_screen("the 4:3 wallpaper", |shot| {
        [(0, 0), (WIDTH / 2, HEIGHT / 2), (WIDTH - 1, HEIGHT - 1)]
            .iter()
            .all(|&(x, y)| shot.pixel(x, y) == GREEN)
    });
    let log = seinas.wallpaper_log();
    assert!(
        log.contains("the bundled wallpapers in")
            && log.contains("four-three.png (32x24), wide.png (32x18)"),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "the bundled wallpaper four-three.png (32x24) for {WIDTH}x{HEIGHT}"
        )),
        "{log}"
    );
    // 出力を 16:9 にすると、16:9 の赤を選び直す。
    seinas.pointer("output 1280x720");
    wait_for_drawing(&seinas, "1280x720");
    seinas.wait_for_screen("the 16:9 wallpaper", |shot| {
        shot.width == 1280
            && [(0, 0), (640, 360), (1279, 719)]
                .iter()
                .all(|&(x, y)| shot.pixel(x, y) == RED)
    });
    assert!(seinas
        .wallpaper_log()
        .contains("the bundled wallpaper wide.png (32x18) for 1280x720"));
}

#[test]
fn a_config_file_wins_over_the_bundled_wallpaper() {
    let seinas = Compositor::start("pg");
    let bundled = bundled_dir(&seinas);
    write_config(&seinas, "color = 336699\n");
    let _wallpaper = seinas.connect_wallpaper_with_bundled(Some(&bundled));
    wait_for_drawing(&seinas, &format!("{WIDTH}x{HEIGHT}"));
    seinas.wait_for_screen("only the color of the config", |shot| {
        shot.pixel(WIDTH / 2, HEIGHT / 2) == BLUE && shot.pixel(0, 0) == BLUE
    });
    let log = seinas.wallpaper_log();
    assert!(log.contains("color 336699"), "{log}");
    assert!(!log.contains("bundled"), "{log}");
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
    let status = common::child_command(common::workspace_bin(
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
