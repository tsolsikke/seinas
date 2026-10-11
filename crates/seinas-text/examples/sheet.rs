//! 既定のフォントで、確かめたい文字を大きさと背景を変えて並べた画像を作る(目で見て比べるため)。
//!
//! 使い方:
//!
//! ```text
//! cargo run -p seinas-text --example sheet [フォントの置き場] [出力のファイル]
//! ```
//!
//! 既定は `target/fonts` と `target/text-sheet.ppm`。12・14・16・18 px を、明るい背景と暗い背景で並べる。
//! 13 px 以下はヒンティングなし、14 px 以上はあり(`seinas-text` の決まりのとおり)。

use std::path::PathBuf;

use seinas_text::{Canvas, FontFiles, Line, TextPainter};

/// 確かめたい文字の並び。
const SAMPLES: [&str; 7] = [
    "Il1| O0o 8B 5S rn m",
    "¥ \\ ＼ ￥ ~ ～ 〜",
    "が か\u{3099} ぱ は\u{309a} ヴ ウ\u{3099}",
    "─│┌┐└┘├┤┬┴┼ ━┃ ═║╔╗",
    "ｱｲｳｴｵ ｶﾞｷﾞ ﾊﾟﾋﾟ ｰ",
    "ひらがな カタカナ 漢字 弌丐丕 凜熙(第2水準)",
    "Seinas のタイトル 0123456789",
];
const SIZES: [u32; 4] = [12, 14, 16, 18];
const WIDTH: usize = 900;

fn main() {
    let mut args = std::env::args().skip(1);
    let fonts = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| "target/fonts".into());
    let out = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| "target/text-sheet.ppm".into());
    let (mut painter, problems) = TextPainter::load(&FontFiles::in_dir(&fonts));
    for problem in &problems {
        eprintln!("sheet: {problem}");
    }

    // 1 段は、1 つの大きさと 1 つの背景。行の高さは、大きさの 1.6 倍。
    let schemes = [
        ([0xf4, 0xf4, 0xf0], [0x20, 0x20, 0x20]),
        ([0x1a, 0x1e, 0x28], [0xe8, 0xe8, 0xe8]),
    ];
    let rows: Vec<(u32, usize)> = SIZES
        .iter()
        .flat_map(|&size| (0..schemes.len()).map(move |scheme| (size, scheme)))
        .collect();
    let line_height = |size: u32| (size as f32 * 1.6).round() as usize;
    let height: usize = rows
        .iter()
        .map(|&(size, _)| line_height(size) * SAMPLES.len() + 8)
        .sum();
    let mut pixels = vec![0u8; WIDTH * height * 4];
    let mut top = 0;
    for (size, scheme) in rows {
        let (background, ink) = schemes[scheme];
        let block = line_height(size) * SAMPLES.len() + 8;
        for y in top..top + block {
            for x in 0..WIDTH {
                let at = (y * WIDTH + x) * 4;
                pixels[at..at + 4].copy_from_slice(&[
                    background[2],
                    background[1],
                    background[0],
                    0xff,
                ]);
            }
        }
        let mut canvas = Canvas::new(&mut pixels, WIDTH, height);
        for (i, sample) in SAMPLES.iter().enumerate() {
            let text = format!("{size}px  {sample}");
            painter.draw_line(
                &mut canvas,
                &text,
                Line {
                    size: size as f32,
                    color: ink,
                    x: 8,
                    top: (top + 4 + i * line_height(size)) as i32,
                    height: line_height(size) as i32,
                    max_width: WIDTH as i32 - 16,
                },
            );
        }
        top += block;
    }

    let mut ppm = format!("P6\n{WIDTH} {height}\n255\n").into_bytes();
    ppm.extend(pixels.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
    std::fs::write(&out, ppm).expect("write the sheet");
    eprintln!("sheet: wrote {}", out.display());
}
