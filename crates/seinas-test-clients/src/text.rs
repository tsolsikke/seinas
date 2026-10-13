//! テスト用のクライアントが、自分の絵に文字を描くための小さな部品(`seinas-text` を使う)。
//!
//! フォントは、同梱のフォントだけを読む(置き場は環境変数 `SEINAS_FONTS`、無ければ `target/fonts`)。読めなければ、
//! 文字を描かない(絵は出す)。

use std::path::PathBuf;

use seinas_text::{Canvas, FontFiles, Line, TextPainter};

/// 文字の大きさ(タイトルバーと同じ)。
pub const SIZE: f32 = 16.0;

/// 同梱のフォントを読む。
pub fn painter() -> TextPainter {
    let dir = std::env::var_os("SEINAS_FONTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/fonts"));
    TextPainter::load(&FontFiles::in_dir(&dir)).0
}

/// `width` × `height` の、地が `background`(赤, 緑, 青)の絵(B, G, R, X)。
pub fn canvas(width: i32, height: i32, background: [u8; 3]) -> Vec<u8> {
    [background[2], background[1], background[0], 0xff].repeat((width * height) as usize)
}

/// 絵 `pixels`(幅 `width`)の (x, top) から高さ `height` の行に、`text` を色 `color` で描く。描いた幅を返す。
#[allow(clippy::too_many_arguments)]
pub fn draw(
    painter: &mut TextPainter,
    pixels: &mut [u8],
    width: i32,
    text: &str,
    x: i32,
    top: i32,
    height: i32,
    color: [u8; 3],
) -> i32 {
    let rows = pixels.len() / 4 / width as usize;
    painter
        .draw_line(
            &mut Canvas::new(pixels, width as usize, rows),
            text,
            Line {
                size: SIZE,
                color,
                x,
                top,
                height,
                max_width: width - x,
            },
        )
        .width
}

/// 絵 `pixels`(幅 `width`)の矩形を、色 `color` で塗る。
pub fn fill(pixels: &mut [u8], width: i32, rect: (i32, i32, i32, i32), color: [u8; 3]) {
    let rows = (pixels.len() / 4 / width as usize) as i32;
    let (x, y, w, h) = rect;
    for row in y.max(0)..(y + h).min(rows) {
        for column in x.max(0)..(x + w).min(width) {
            let at = ((row * width + column) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&[color[2], color[1], color[0], 0xff]);
        }
    }
}
