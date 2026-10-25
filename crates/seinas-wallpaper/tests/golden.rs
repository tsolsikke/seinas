//! 壁紙の絵を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! `paint` は Wayland を使わない関数なので、それだけを呼ぶ。画像は、リポジトリに入れた自作の小さな画像
//! (`tests/data/test-48x32.png`。`tools/make-test-wallpaper.py` が作る)だけを使い、システムの画像には
//! 左右されない。

use std::path::Path;

use seinas_golden::{Image, Runner, Tolerance};
use seinas_wallpaper::{load_png, paint, Mode, DEFAULT_COLOR};

/// 余白の色(既定の色とは別の、見分けやすい色)。
const MARGIN: [u8; 3] = [0x30, 0x30, 0x40];

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-wallpaper");
    let picture =
        load_png(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/test-48x32.png"))
            .expect("the test image");
    let image = |pixels: Vec<u8>, width: u32, height: u32| {
        Image::from_bgrx(width as usize, height as usize, &pixels)
    };

    // W1: 設定が無いとき(色だけ。コンポジタの背景と同じ色)。
    run.image(
        "w1-color-only",
        Tolerance::EXACT,
        image(paint(None, Mode::Fill, DEFAULT_COLOR, 40, 24), 40, 24),
    );
    // W2〜W4: 横長の出力(96x40)に、3:2 の画像を置く。
    for (name, mode) in [
        ("w2-fill", Mode::Fill),
        ("w3-fit", Mode::Fit),
        ("w4-center", Mode::Center),
    ] {
        run.image(
            name,
            Tolerance::EXACT,
            image(paint(Some(&picture), mode, MARGIN, 96, 40), 96, 40),
        );
    }
    // W5〜W6: 画像より小さく、縦長の出力(20x30)。塗りつぶしは縮めて左右を切り、中央に表示は中央を切り出す。
    for (name, mode) in [
        ("w5-fill-shrink", Mode::Fill),
        ("w6-center-crop", Mode::Center),
    ] {
        run.image(
            name,
            Tolerance::EXACT,
            image(paint(Some(&picture), mode, MARGIN, 20, 30), 20, 30),
        );
    }
    // W7: 16:9 の出力(64x36。1920x1080 などを縮めた大きさ)に、3:2 の画像を塗りつぶしで出す。横いっぱいに広げ、
    // 上下を切る。
    run.image(
        "w7-fill-16x9",
        Tolerance::EXACT,
        image(paint(Some(&picture), Mode::Fill, MARGIN, 64, 36), 64, 36),
    );

    run.finish();
}
