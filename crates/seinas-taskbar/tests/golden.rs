//! タスクバーの絵を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! `paint` は Wayland を使わない関数なので、それだけを呼ぶ。

use std::path::Path;

use seinas_golden::{Image, Runner, Tolerance};
use seinas_taskbar::{paint, Entry, HEIGHT, NO_CLOCK};
use seinas_text::{FontFiles, TextPainter};

fn entry(title: &str, activated: bool, minimized: bool) -> Entry {
    Entry {
        title: title.to_owned(),
        activated,
        minimized,
    }
}

/// タスクバーを描いて、RGB の画像にする。
fn bar(
    painter: &mut TextPainter,
    width: i32,
    entries: &[Entry],
    hot: Option<usize>,
    clock: &str,
) -> Image {
    let pixels = paint(painter, width, entries, hot, clock);
    Image::from_bgrx(width as usize, HEIGHT as usize, &pixels)
}

/// 1 つのケース:名前、幅、並べるウィンドウ、ポインターが乗っているボタン、時計の文字。
type Case<'a> = (&'a str, i32, &'a [Entry], Option<usize>, &'a str);

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-taskbar");

    // activated のもの、ポインターが乗っているもの、長いタイトル。
    let three = [
        entry("zeyes", true, false),
        entry("一つ目のウィンドウ", false, false),
        entry(
            "とても長いタイトルのウィンドウです。どこまでも続きます",
            false,
            false,
        ),
    ];
    // 最小化しているものと、時刻が取れないときの表示。
    let minimized = [entry("zeyes", false, true), entry("端末", true, false)];

    // F1〜F3: フォントの要るもの。
    let cases: [Case; 3] = [
        ("f1-three-windows", 480, &three, Some(1), "12:34"),
        ("f2-minimized-and-no-clock", 480, &minimized, None, NO_CLOCK),
        ("f3-no-windows", 320, &[], None, "09:05"),
    ];
    let mut painter: Option<TextPainter> = None;
    for (name, width, entries, hot, clock) in cases {
        run.with_fonts(name, |run, fonts: &Path| {
            let painter = painter.get_or_insert_with(|| {
                let (painter, problems) = TextPainter::load(&FontFiles::in_dir(fonts));
                assert!(
                    problems.is_empty(),
                    "the fonts could not be loaded: {problems:?}"
                );
                painter
            });
            run.image(
                name,
                Tolerance::EXACT,
                bar(painter, width, entries, hot, clock),
            );
        });
    }

    // F4: フォントの要らないもの(ボタンの地と印だけ)。
    let mut no_fonts = TextPainter::without_fonts();
    run.image(
        "f4-no-fonts",
        Tolerance::EXACT,
        bar(&mut no_fonts, 480, &three, Some(1), "12:34"),
    );

    run.finish();
}
