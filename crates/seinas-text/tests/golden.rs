//! テキスト描画の出力を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! 黒の上に白で 1 行を描き、カバレッジ(0〜255)を PGM で保存する。戻り値(`Drawn`)は `cases.txt` に書く。
//! フォントは tools/fetch-fonts.sh が置いたものを使い、SHA-256 も確かめる。

use std::fmt::Write as _;
use std::path::Path;

use seinas_golden::{Image, Runner, Tolerance};
use seinas_text::{Canvas, Drawn, FontFiles, Line, TextPainter};

const WIDTH: usize = 320;
const HEIGHT: usize = 24;

/// 文字列。名前はファイルの名前に使う。
const TEXTS: [(&str, &str); 4] = [
    ("ascii", "The quick brown fox 0123456789"),
    ("kana", "ひらがなとカタカナ"),
    ("kanji", "漢字の表示 東京都"),
    // ═ と ☃ は BIZ UDPゴシックに無く、GNU Unifont JP で描かれる。U+E000 はどちらにも無く、四角の枠になる。
    ("fallback", "a═b ☃ \u{e000}"),
];

/// 大きさ。13 px はヒンティングなし、14 px と 16 px はヒンティングあり。
const SIZES: [u32; 3] = [13, 14, 16];

/// 見分けにくい字や、日本語で気をつける字。既定の大きさ(14 px 以上)だけで比べる。
const DETAILS: [(&str, &str); 6] = [
    ("confusable", "Il1| O0o 8B 5S"),
    ("yen-backslash", "¥ \\ ＼ ￥"),
    // 結合濁点・半濁点(U+3099、U+309A)は、合成済みの字と同じに描かれる。
    ("dakuten", "が か\u{3099} ぱ は\u{309a}"),
    // ─ などは UI のフォントにあり、═ ║ は GNU Unifont JP で描かれる。
    ("box-drawing", "─│┌┐└┘┼ ═║"),
    ("halfwidth-kana", "ｱｲｳｴｵ ｶﾞﾊﾟｰ"),
    // JIS X 0208 の第 2 水準の漢字。
    ("kanji-level2", "弌丐丕 蘂鬮 凜熙"),
];
const DETAIL_SIZES: [u32; 2] = [14, 16];

/// 1 行を描いて、カバレッジの画像と戻り値を返す。
fn draw(painter: &mut TextPainter, text: &str, size: u32) -> (Image, Drawn) {
    let mut pixels = vec![0u8; WIDTH * HEIGHT * 4];
    let drawn = painter.draw_line(
        &mut Canvas::new(&mut pixels, WIDTH, HEIGHT),
        text,
        Line {
            size: size as f32,
            color: [0xff, 0xff, 0xff],
            x: 4,
            top: 0,
            height: HEIGHT as i32,
            max_width: WIDTH as i32 - 8,
        },
    );
    // 白で描いたので、B, G, R は同じ値(カバレッジ)になる。
    let coverage = pixels
        .chunks(4)
        .map(|pixel| {
            assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2], "a gray pixel");
            pixel[0]
        })
        .collect();
    (Image::gray(WIDTH, HEIGHT, coverage), drawn)
}

/// 1 つのケースを描いて比べ、戻り値を `cases` に 1 行足す。フォントは、最初に要るときに読む。
fn check(
    run: &mut Runner,
    painter: &mut Option<TextPainter>,
    cases: &mut String,
    name: &str,
    text: &str,
    size: u32,
) {
    let case = format!("b-{name}-{size}px");
    run.with_fonts(&case, |run, fonts: &Path| {
        let painter = painter.get_or_insert_with(|| {
            let (painter, problems) = TextPainter::load(&FontFiles::in_dir(fonts));
            assert!(
                problems.is_empty(),
                "the fonts could not be loaded: {problems:?}"
            );
            painter
        });
        let (image, drawn) = draw(painter, text, size);
        run.image(&case, Tolerance::EXACT, image);
        let _ = writeln!(
            cases,
            "{case} {} {} {} {}",
            drawn.width, drawn.truncated, drawn.fallback_glyphs, drawn.missing_glyphs
        );
    });
}

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-text");
    let mut painter: Option<TextPainter> = None;
    let mut cases = String::from("# case width truncated fallback_glyphs missing_glyphs\n");
    // cases.txt の行の順は、前からある 12 ケースを先に、後から足したケースを後にする。
    for size in SIZES {
        for (name, text) in TEXTS {
            check(&mut run, &mut painter, &mut cases, name, text, size);
        }
    }
    for size in DETAIL_SIZES {
        for (name, text) in DETAILS {
            check(&mut run, &mut painter, &mut cases, name, text, size);
        }
    }
    let complete =
        cases.lines().count() == 1 + SIZES.len() * TEXTS.len() + DETAIL_SIZES.len() * DETAILS.len();
    run.with_fonts("cases.txt", |run, _| {
        assert!(complete, "every case was drawn");
        run.text("cases.txt", &cases);
    });
    run.finish();
}
