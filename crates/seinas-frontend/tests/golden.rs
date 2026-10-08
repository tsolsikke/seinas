//! タイトルバーの出力を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! `paint_title_bar` は Smithay を使わない関数なので、それだけを呼ぶ。

use std::path::Path;

use seinas_frontend::{load_fonts, paint_title_bar, TITLE_BAR_HEIGHT};
use seinas_golden::{Image, Runner, Tolerance};
use seinas_text::TextPainter;

/// タイトルバーを描いて、RGB の画像にする。
fn bar(painter: &mut TextPainter, title: &str, width: i32, active: bool, hot: bool) -> Image {
    let pixels = paint_title_bar(painter, title, width, active, hot);
    Image::from_bgrx(width as usize, TITLE_BAR_HEIGHT as usize, &pixels)
}

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-frontend");

    // C1〜C5: フォントの要るもの。
    let cases: [(&str, &str, i32, bool, bool); 5] = [
        ("c1-active", "zeyes", 200, true, false),
        ("c2-active-hover", "zeyes", 200, true, true),
        ("c3-inactive", "zeyes", 200, false, false),
        ("c4-inactive-hover", "zeyes", 200, false, true),
        (
            "c5-long-title",
            "とても長い題名のウィンドウです。どこまでも続きます",
            200,
            true,
            false,
        ),
    ];
    let mut painter: Option<TextPainter> = None;
    for (name, title, width, active, hot) in cases {
        run.with_fonts(name, |run, fonts: &Path| {
            let painter = painter.get_or_insert_with(|| {
                let painter = load_fonts(fonts);
                assert!(painter.can_draw(), "the fonts could not be loaded");
                painter
            });
            run.image(
                name,
                Tolerance::EXACT,
                bar(painter, title, width, active, hot),
            );
        });
    }

    // C6・C7: フォントの要らないもの。
    let mut no_fonts = TextPainter::without_fonts();
    run.image(
        "c6-no-fonts",
        Tolerance::EXACT,
        bar(&mut no_fonts, "zeyes", 200, true, true),
    );
    // close ボタン(24 px)より狭い幅。
    run.image(
        "c7-narrower-than-close-button",
        Tolerance::EXACT,
        bar(&mut no_fonts, "zeyes", 10, true, true),
    );

    run.finish();
}
