//! fbdev の blit(描画結果を、画面の形式に直してバッファへ書く)の出力を、正解データ(tests/golden/)と比べる。
//! 決まりは docs/golden.md にある。
//!
//! 入力は、色の成分が全部ちがう 16x8 の絵。出力は、バッファのバイト列そのもの(raw)。
//! バッファは 0xaa で埋めてから書くので、書かれない所(行の余白、見えている範囲の外)も確かめられる。

use seinas_fbdev::layout::{Bitfield, Layout, ScreenInfo};
use seinas_golden::Runner;
use seinas_render::{solid_rect, Painter};

const WIDTH: u32 = 16;
const HEIGHT: u32 = 8;

fn field(offset: u32) -> Bitfield {
    Bitfield { offset, length: 8 }
}

/// XRGB(メモリー上の並びは B, G, R, X)の画面。
fn xrgb() -> ScreenInfo {
    ScreenInfo {
        width: WIDTH,
        height: HEIGHT,
        x_offset: 0,
        y_offset: 0,
        bits_per_pixel: 32,
        line_length: WIDTH * 4,
        red: field(16),
        green: field(8),
        blue: field(0),
        transp: Bitfield::default(),
    }
}

/// 色の成分が全部ちがう 4 つの縦の帯と、その上の横の帯の絵を描き、`info` の形式のバッファへ書く。
fn blit(info: &ScreenInfo) -> (Vec<u8>, usize) {
    let layout = Layout::new(info).expect("a supported layout");
    let mut painter = Painter::new(WIDTH as i32, HEIGHT as i32).expect("a painter");
    let rgb = |r: u8, g: u8, b: u8| [r, g, b, 0xff].map(|c| c as f32 / 255.0);
    // 先頭がいちばん手前。
    let elements = [
        solid_rect(0, 0, 16, 2, rgb(0xf0, 0xe1, 0xd2)),
        solid_rect(0, 0, 4, 8, rgb(0x11, 0x22, 0x33)),
        solid_rect(4, 0, 4, 8, rgb(0x44, 0x55, 0x66)),
        solid_rect(8, 0, 4, 8, rgb(0x77, 0x88, 0x99)),
        solid_rect(12, 0, 4, 8, rgb(0xaa, 0xbb, 0xcc)),
    ];
    let view = painter.paint(&elements).expect("paint");
    let mut screen = vec![0xaau8; layout.required_len()];
    layout.blit(&view, &mut screen).expect("blit");
    (screen, info.line_length as usize)
}

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-fbdev");

    let cases = [
        ("d1-xrgb", xrgb()),
        (
            // ARGB: 透明度のバイトには 0xff が書かれる。
            "d2-argb",
            ScreenInfo {
                transp: field(24),
                ..xrgb()
            },
        ),
        (
            // XBGR(メモリー上の並びは R, G, B, X)。
            "d3-xbgr",
            ScreenInfo {
                red: field(0),
                blue: field(16),
                ..xrgb()
            },
        ),
        (
            // RGBX(メモリー上の並びは X, B, G, R)。
            "d4-rgbx",
            ScreenInfo {
                red: field(24),
                green: field(16),
                blue: field(8),
                ..xrgb()
            },
        ),
        (
            // 見えている範囲が (3, 2) から始まり、1 行の終わりに 20 バイトの余白がある。
            "d5-offset-and-padding",
            ScreenInfo {
                x_offset: 3,
                y_offset: 2,
                line_length: (WIDTH + 3) * 4 + 20,
                ..xrgb()
            },
        ),
    ];
    for (name, info) in cases {
        let (screen, row) = blit(&info);
        run.raw(name, row, &screen);
    }

    run.finish();
}
