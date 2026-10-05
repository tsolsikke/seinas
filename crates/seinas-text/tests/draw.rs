//! フォントを読んで、1行の文字を描けることを確かめる。
//!
//! フォントは、`tools/fetch-fonts.sh` が置いたものを使う(環境変数 `SEINAS_FONTS` で場所を変えられる)。

use std::path::PathBuf;

use seinas_text::{Canvas, Drawn, FontFiles, FontProblem, Line, TextPainter, DEFAULT_SIZE};

const WIDTH: usize = 200;
const HEIGHT: usize = 24;
/// 下地の色と文字の色(赤, 緑, 青)。
const GROUND: [u8; 3] = [0x20, 0x40, 0x60];
const INK: [u8; 3] = [0xff, 0xff, 0xff];

fn font_dir() -> PathBuf {
    std::env::var_os("SEINAS_FONTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts"))
}

fn painter() -> TextPainter {
    let (painter, problems) = TextPainter::load(&FontFiles::in_dir(&font_dir()));
    assert!(
        problems.is_empty(),
        "the fonts could not be loaded (run tools/fetch-fonts.sh): {problems:?}"
    );
    painter
}

/// 下地だけの絵に `text` を描く。絵は、行ごとの「下地でない画素か」の並びで返す。
fn draw(painter: &mut TextPainter, text: &str, max_width: i32) -> (Drawn, Vec<Vec<bool>>) {
    let mut pixels = [GROUND[2], GROUND[1], GROUND[0], 0xff].repeat(WIDTH * HEIGHT);
    let drawn = painter.draw_line(
        &mut Canvas::new(&mut pixels, WIDTH, HEIGHT),
        text,
        Line {
            size: DEFAULT_SIZE,
            color: INK,
            x: 8,
            top: 0,
            height: HEIGHT as i32,
            max_width,
        },
    );
    let ground = [GROUND[2], GROUND[1], GROUND[0]];
    let inked = pixels
        .chunks(WIDTH * 4)
        .map(|row| row.chunks(4).map(|pixel| pixel[..3] != ground).collect())
        .collect();
    (drawn, inked)
}

/// 下地でない画素がある列の、左の端と右の端。
fn inked_columns(inked: &[Vec<bool>]) -> Option<(usize, usize)> {
    let columns: Vec<usize> = (0..WIDTH)
        .filter(|&x| inked.iter().any(|row| row[x]))
        .collect();
    Some((*columns.first()?, *columns.last()?))
}

#[test]
fn a_line_is_drawn_inside_its_box() {
    let mut painter = painter();
    assert!(painter.can_draw());
    let (drawn, inked) = draw(&mut painter, "zeyes ウィンドウ", 180);
    assert!(!drawn.truncated);
    assert_eq!((drawn.fallback_glyphs, drawn.missing_glyphs), (0, 0));
    let (left, right) = inked_columns(&inked).expect("some pixels of the text");
    assert!(left >= 8, "the text starts at the left end of the line");
    assert!(right < 8 + drawn.width as usize + 1);
    // 縦の中央に置かれる。上と下の端には、何も描かれない。
    assert!(inked[0].iter().chain(&inked[HEIGHT - 1]).all(|&ink| !ink));
    assert!(inked[HEIGHT / 2].iter().any(|&ink| ink));
}

#[test]
fn a_long_line_is_cut_and_ends_with_an_ellipsis() {
    let mut painter = painter();
    let long = "とても長い題名のウィンドウです。どこまでも続きます";
    let (whole, _) = draw(&mut painter, long, 10_000);
    assert!(!whole.truncated);

    let (drawn, inked) = draw(&mut painter, long, 100);
    assert!(drawn.truncated);
    assert!(drawn.width <= 100);
    let (_, right) = inked_columns(&inked).unwrap();
    assert!(right < 8 + 100, "nothing is drawn beyond the width");

    // 切った行は、「…」を付けて幅に収まる中で、いちばん長い頭の部分と同じ絵になる。
    let longest = (1..long.chars().count())
        .map(|count| long.chars().take(count).chain(['…']).collect::<String>())
        .map(|text| (draw(&mut painter, &text, 10_000), text))
        .take_while(|((drawn, _), _)| drawn.width <= 100)
        .last();
    let ((same, inked_same), text) = longest.expect("a prefix that fits");
    assert!(text.chars().count() > 4, "{text}");
    assert_eq!(same.width, drawn.width);
    assert_eq!(inked_same, inked);

    // 「…」も入らないほど狭いときも、はみ出さない。
    let (_, inked) = draw(&mut painter, long, 3);
    assert!(inked_columns(&inked).is_none_or(|(_, right)| right < 8 + 14));
}

#[test]
fn a_character_missing_from_the_ui_font_is_drawn_with_the_fallback_font() {
    let mut painter = painter();
    // U+2550(罫線)は、BIZ UDPゴシックには無く、GNU Unifont JPにある。
    let (drawn, inked) = draw(&mut painter, "a═b", 180);
    assert_eq!((drawn.fallback_glyphs, drawn.missing_glyphs), (1, 0));
    // 二重の横線なので、同じ行に、横に続いた画素がある。
    let longest_run = inked
        .iter()
        .map(|row| {
            row.split(|&ink| !ink)
                .map(|run| run.len())
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap();
    assert!(longest_run >= 6, "the box-drawing line is drawn");
}

#[test]
fn a_character_missing_from_every_font_is_drawn_as_a_box() {
    let mut painter = painter();
    // U+E000(私用の文字)は、どちらのフォントにも無い。
    let (drawn, inked) = draw(&mut painter, "\u{e000}", 180);
    assert_eq!(drawn.missing_glyphs, 1);
    let (left, right) = inked_columns(&inked).expect("a box");
    // 枠なので、左と右の端の列は縦に続き、中は空いている。
    let column = |x: usize| inked.iter().filter(|row| row[x]).count();
    assert!(column(left) >= 8 && column(right) >= 8);
    assert_eq!(column((left + right) / 2), 2);
}

#[test]
fn control_characters_do_not_break_the_line() {
    let mut painter = painter();
    let (drawn, inked) = draw(&mut painter, "a\nb\tc\u{7}", 180);
    assert_eq!(drawn.missing_glyphs, 0);
    assert!(inked_columns(&inked).is_some());
}

#[test]
fn missing_font_files_are_reported_and_nothing_is_drawn() {
    let files = FontFiles::in_dir(&PathBuf::from("/nonexistent/seinas-fonts"));
    let (mut painter, problems) = TextPainter::load(&files);
    assert_eq!(problems.len(), 2);
    assert!(problems
        .iter()
        .all(|problem| matches!(problem, FontProblem::Unreadable { .. })));
    assert!(!painter.can_draw());
    let (drawn, inked) = draw(&mut painter, "zeyes", 180);
    assert_eq!(drawn, Drawn::default());
    assert!(inked_columns(&inked).is_none());
}

#[test]
fn a_broken_ui_font_is_reported_and_the_fallback_font_is_used() {
    let dir = std::env::temp_dir().join(format!("seinas-text-broken-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = dir.join("broken.ttf");
    std::fs::write(&broken, b"this is not a font").unwrap();
    let files = FontFiles {
        ui: broken.clone(),
        fallback: FontFiles::in_dir(&font_dir()).fallback,
    };
    let (mut painter, problems) = TextPainter::load(&files);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        matches!(&problems[..], [FontProblem::Unusable { path, .. }] if *path == broken),
        "{problems:?}"
    );
    // 控えのフォントだけで描く。
    assert!(painter.can_draw());
    let (drawn, inked) = draw(&mut painter, "zeyes", 180);
    assert_eq!(drawn.missing_glyphs, 0);
    assert!(inked_columns(&inked).is_some());
}

#[test]
fn a_font_file_of_another_font_is_not_used() {
    // 控えのフォントのファイルを、主のフォントとして渡す。
    let fonts = FontFiles::in_dir(&font_dir());
    let files = FontFiles {
        ui: fonts.fallback.clone(),
        fallback: fonts.fallback,
    };
    let (painter, problems) = TextPainter::load(&files);
    assert!(matches!(&problems[..], [FontProblem::Unusable { .. }]));
    assert!(painter.can_draw());
}
