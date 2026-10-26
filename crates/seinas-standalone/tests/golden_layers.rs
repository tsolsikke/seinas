//! layer surface とウィンドウの重なりの描画を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! layers-stacking:160x120 の fake screen に、次のものを出して、画面の全体を比べる。
//! - background:全面(exclusive zone は無視する)。緑
//! - top:上の辺、高さ 12、exclusive zone 12。青
//! - bottom:左の辺、幅 20。茶
//! - top:右の辺、幅 24、exclusive zone を無視する。灰
//! - overlay:anchor なし、40x20(使える範囲の中央)。赤
//! - ウィンドウ:140x40、タイトルなし。橙。使える範囲の左上(上のバーの下)に置かれる。下の茶と緑が、ウィンドウの
//!   影の外にも見えるよう、低くしてある
//!
//! 手前から、overlay、top、ウィンドウ、bottom、background の順に重なる。
//!
//! popup-flipped:160x120 の fake screen に、ウィンドウ(120x60、タイトルなし。橙)を 1 つ出し、コンテンツの右下に近い
//! 所(110, 55)を anchor rect にして、右下へ開く 50x30 のポップアップ(紫)を頼む。画面の右と下の端からはみ出すので、
//! constraint_adjustment の flip_x と flip_y で、anchor rect の左上へ反転する。ポップアップには枠も影も角丸も付かず、
//! ウィンドウの手前に描かれる。
//!
//! desktop-1920x1080(フォントの要るもの):見本の画像(`docs/ui-spec.md`)と同じ配置の画面。1920x1080 の fake screen に、
//! タスクバー(時刻は 2026-10-26 20:45 +09:00 に止める)と、テスト用のアイコンの置き場の app_id のウィンドウ 4 つ
//! (前面の「ファイル」と背面の「端末」。「メモ」と「設定」は最小化)を出す。ウィンドウは、タイトルバーをポインターで
//! ドラッグして置く。壁紙は出さない(地は Seinas の背景の単色)。壁紙のピクセルが正解データの大きさの大半になり、
//! 作り直すたびに履歴に積まれるため。壁紙の出し方は、seinas-wallpaper の golden と `tests/wallpaper.rs` で確かめる。

mod common;

use std::path::Path;
use std::time::Duration;

use common::{
    bar_color, upper_bar_color, Compositor, Shot, TestClient, ACTIVE_BAR, BACKGROUND, BAR,
    INACTIVE_BAR,
};
use seinas_golden::{Image, Runner, Tolerance};

const WIDTH: usize = 160;
const HEIGHT: usize = 120;

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-standalone");
    let seinas = Compositor::start_sized("gl", WIDTH, HEIGHT);
    // 上のバーを先につなぐ(layer surface は、つないだ順に並べられる)。
    let layers: [&[&str]; 5] = [
        &[
            "layer",
            "--layer",
            "top",
            "--anchor",
            "top,left,right",
            "--size",
            "0x12",
            "--exclusive",
            "12",
            "--color",
            "2040c0",
        ],
        &[
            "layer",
            "--layer",
            "background",
            "--anchor",
            "top,bottom,left,right",
            "--exclusive",
            "-1",
            "--color",
            "204020",
        ],
        &[
            "layer",
            "--layer",
            "bottom",
            "--anchor",
            "left,top,bottom",
            "--size",
            "20x0",
            "--color",
            "806020",
        ],
        &[
            "layer",
            "--layer",
            "top",
            "--anchor",
            "right,top,bottom",
            "--size",
            "24x0",
            "--exclusive",
            "-1",
            "--color",
            "c0c0c0",
        ],
        &[
            "layer", "--layer", "overlay", "--size", "40x20", "--color", "c02020",
        ],
    ];
    let mut clients = Vec::new();
    for args in layers {
        let client = seinas.connect_shell_client(args);
        client.wait_for_line("layer: drawn");
        clients.push(client);
    }
    let window = seinas.connect_test_client(&["window", "--size", "140x40"]);
    window.wait_for_line("window: drawn");
    clients.push(window);

    // 全部が描かれるまで待つ(どの面も、決まった所の色で見る)。
    let shot: Shot = seinas.wait_for_shot("every surface drawn", |shot| {
        shot.pixel(2, 2) == [0x20, 0x40, 0xc0]
            && shot.pixel(150, 60) == [0xc0, 0xc0, 0xc0]
            && shot.pixel(80, 66) == [0xc0, 0x20, 0x20]
            && shot.pixel(10, 110) == [0x80, 0x60, 0x20]
            && shot.pixel(100, 110) == [0x20, 0x40, 0x20]
            && shot.pixel(30, 12 + BAR + 4) == [0xff, 0x80, 0x00]
    });
    run.image(
        "layers-stacking",
        Tolerance::EXACT,
        Image::rgb(shot.width, shot.height, shot.pixels.clone()),
    );
    drop(clients);
    // finish は、失敗があると std::process::exit で終わり、Drop が走らない。コンポジタは先に止める。
    drop(seinas);

    let shot = popup_flipped();
    run.image(
        "popup-flipped",
        Tolerance::EXACT,
        Image::rgb(shot.width, shot.height, shot.pixels),
    );

    run.with_fonts("desktop-1920x1080", |run, fonts: &Path| {
        let shot = desktop(fonts);
        run.image(
            "desktop-1920x1080",
            Tolerance::EXACT,
            Image::rgb(shot.width, shot.height, shot.pixels),
        );
    });
    run.finish();
}

/// 画面の端で反転したポップアップの画面(popup-flipped)。
fn popup_flipped() -> Shot {
    let seinas = Compositor::start_sized("gp", WIDTH, HEIGHT);
    let mut window = seinas.connect_test_client(&["window", "--size", "120x60"]);
    window.wait_for_line("window: drawn");
    // flip_x 4 + flip_y 8。反転して、anchor rect (110, 55) の左上へ:ウィンドウの幾何から (60, 25)。
    window.send_line("popup 110 55 50 30 adjust=12");
    window.wait_for_line("window: popup 1 configure 60 25 50 30");
    window.wait_for_line("window: popup 1 drawn 50x30");
    let purple = [0x80, 0x00, 0xff];
    seinas.wait_for_shot("the flipped popup", |shot| {
        shot.pixel(60, BAR + 25) == purple && shot.pixel(109, BAR + 54) == purple
    })
}

/// 見本の画像と同じ配置の画面の大きさ。
const DESKTOP: (usize, usize) = (1920, 1080);
/// タスクバーの時計を止める時刻(2026-10-26 20:45 +09:00 の、UNIX 時刻の秒)。
const FIXED_TIME: &str = "1793015100";
/// 前面の「ファイル」と背面の「端末」の、外枠の左上と、コンテンツの大きさ。
const FILES: ((usize, usize), (usize, usize)) = ((69, 118), (866, 437));
const TERMINAL: ((usize, usize), (usize, usize)) = ((904, 61), (786, 286));

/// テスト用のウィンドウを出し、描かれるまで待つ。
fn window(
    seinas: &Compositor,
    title: &str,
    app_id: &str,
    size: (usize, usize),
    color: &str,
) -> TestClient {
    let size = format!("{}x{}", size.0, size.1);
    let client = seinas.connect_test_client(&[
        "window", "--title", title, "--app-id", app_id, "--size", &size, "--color", color,
    ]);
    client.wait_for_line("window: drawn");
    client
}

/// (`from`)で左のボタンを押し、5 歩で(`to`)まで動かして、離す。
fn drag(seinas: &mut Compositor, from: (usize, usize), to: (usize, usize)) {
    let (from, to) = ((from.0 as i64, from.1 as i64), (to.0 as i64, to.1 as i64));
    seinas.pointer(&format!("motion {} {}", from.0, from.1));
    seinas.pointer("press");
    for step in 1..=5 {
        let x = from.0 + (to.0 - from.0) * step / 5;
        let y = from.1 + (to.1 - from.1) * step / 5;
        seinas.pointer(&format!("motion {x} {y}"));
    }
    seinas.pointer("release");
}

/// 見本の画像と同じ配置の画面を作り、変わらなくなった所の画面を返す。
fn desktop(fonts: &Path) -> Shot {
    let (width, height) = DESKTOP;
    let mut seinas = Compositor::start_sized_with_input("gd", fonts, width, height);
    let _taskbar = seinas.connect_taskbar_to(
        &seinas.shell_socket(),
        &["--utc-offset", "+09:00", "--fixed-time", FIXED_TIME],
    );
    let taskbar_top = height - seinas_theme::taskbar::HEIGHT as usize;
    seinas.wait_for_screen("the taskbar", |shot| {
        shot.pixel(0, taskbar_top + 1) == seinas_theme::taskbar::BACKGROUND
    });

    // カスケードの位置は、つないだ順に (0, 0)、(32, 32)、(64, 64)、(96, 96)。後の 2 つは最小化する。
    let _files = window(&seinas, "ファイル", "seinas.test.files", FILES.1, "0e192b");
    let _terminal = window(
        &seinas,
        "端末",
        "seinas.test.terminal",
        TERMINAL.1,
        "0c1422",
    );
    let mut notes = window(&seinas, "メモ", "seinas.test.notes", (400, 300), "202c3b");
    let mut settings = window(
        &seinas,
        "設定",
        "seinas.test.settings",
        (400, 300),
        "202c3b",
    );
    for client in [&mut settings, &mut notes] {
        client.send_line("minimize");
        client.send_line("sync");
        client.wait_for_line("window: synced");
    }

    // 端末(2 つ目。(32, 32))を右上へ、ファイル(1 つ目。(0, 0))を左上へ動かす(後に動かしたファイルが前面になる)。
    let (terminal_at, files_at) = (TERMINAL.0, FILES.0);
    drag(
        &mut seinas,
        (152, 42),
        (152 + terminal_at.0 - 32, 42 + terminal_at.1 - 32),
    );
    drag(&mut seinas, (120, 10), (120 + files_at.0, 10 + files_at.1));
    // ポインターは、ウィンドウの無い所に置く。
    seinas.pointer(&format!("motion {} {}", width / 2, height / 2 + 200));

    let ready = |shot: &Shot| {
        bar_color(shot, files_at.0, files_at.1, FILES.1 .0) == Some(ACTIVE_BAR)
            && bar_color(shot, terminal_at.0, terminal_at.1, TERMINAL.1 .0) == Some(INACTIVE_BAR)
            && upper_bar_color(shot, files_at.0, files_at.1, FILES.1 .0) == ACTIVE_BAR
            // 最小化した「設定」((96, 96))のタイトルバーがあった所は、背景。
            && shot.pixel(300, 100) == BACKGROUND
            && shot.pixel(files_at.0, files_at.1 + BAR) == [0x0e, 0x19, 0x2b]
    };
    // 条件を満たし、少し待っても変わらない画面(タスクバーのアイコンと、最小化の印まで描き終えた画面)を取る。
    loop {
        let first = seinas.wait_for_shot("the desktop like the reference image", ready);
        std::thread::sleep(Duration::from_millis(500));
        let second = seinas.shot();
        if second.pixels == first.pixels {
            return second;
        }
    }
}
