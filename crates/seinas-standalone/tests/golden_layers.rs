//! layer surface とウィンドウの重なりの描画を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! 160x120 の fake screen に、次のものを出して、画面の全体を比べる。
//! - background:全面(exclusive zone は無視する)。緑
//! - top:上の辺、高さ 12、exclusive zone 12。青
//! - bottom:左の辺、幅 20。茶
//! - top:右の辺、幅 24、exclusive zone を無視する。灰
//! - overlay:anchor なし、40x20(使える範囲の中央)。赤
//! - ウィンドウ:140x40、タイトルなし。橙。使える範囲の左上(上のバーの下)に置かれる。下の茶と緑が、ウィンドウの
//!   影の外にも見えるよう、低くしてある
//!
//! 手前から、overlay、top、ウィンドウ、bottom、background の順に重なる。

mod common;

use common::{Compositor, Shot, BAR};
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
    run.finish();
}
