//! タイトルバーの出力を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! `paint_title_bar` は Smithay を使わない関数なので、それだけを呼ぶ。

use std::path::{Path, PathBuf};

use seinas_frontend::{
    geometry::Size, paint_switcher, paint_title_bar, switcher_layout, IconCache, SwitcherEntry,
    TitleButton, TitleButtons, TitleLook, TITLE_BAR_HEIGHT, TITLE_ICON_SIZE,
};
use seinas_golden::{Image, Runner, Tolerance};
use seinas_text::{FontFiles, TextPainter};

/// タイトルバーを、既定のアプリのアイコンで描いて、RGB の画像にする(欠いた角は、透明の代わりに黒の上に重ねた色)。
fn bar(painter: &mut TextPainter, title: &str, width: i32, active: bool, look: TitleLook) -> Image {
    let icons = IconCache::new(TITLE_ICON_SIZE as u32, Vec::new());
    let pixels = paint_title_bar(painter, title, width, active, look, icons.default_icon());
    Image::from_bgrx(width as usize, TITLE_BAR_HEIGHT as usize, &pixels)
}

/// テスト用のアイコンの置き場(データの置き場の形。`tools/make-test-icons.py` が作る)。
fn test_icons() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../seinas-icons/tests/data/share")
}

/// 切り替えの一覧を、大きさ `output` の出力に置いたときの絵にする(一覧の大きさの画像)。
fn switcher(
    painter: &mut TextPainter,
    entries: &[SwitcherEntry],
    current: usize,
    output: Size,
) -> Image {
    let layout = switcher_layout(entries.len(), current, output).expect("a layout");
    let pixels = paint_switcher(painter, entries, current, &layout);
    Image::from_bgrx(
        layout.rect.size.w as usize,
        layout.rect.size.h as usize,
        &pixels,
    )
}

fn entry(title: &str, minimized: bool) -> SwitcherEntry {
    SwitcherEntry {
        title: title.to_owned(),
        minimized,
    }
}

/// 閉じるボタンだけのタイトルバー。`hot` なら、閉じるボタンにポインターが乗っている。
fn close_only(hot: bool) -> TitleLook {
    TitleLook {
        buttons: TitleButtons::CLOSE_ONLY,
        maximized: false,
        hot: hot.then_some(TitleButton::Close),
        unresponsive: false,
    }
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
                // 利用者の設定のファイルに左右されないよう、同梱のフォントだけを直に読む。
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
                bar(painter, title, width, active, close_only(hot)),
            );
        });
    }

    // C6・C7: フォントの要らないもの。
    let mut no_fonts = TextPainter::without_fonts();
    run.image(
        "c6-no-fonts",
        Tolerance::EXACT,
        bar(&mut no_fonts, "zeyes", 200, true, close_only(true)),
    );
    // close ボタン(44 px)より狭い幅。
    run.image(
        "c7-narrower-than-close-button",
        Tolerance::EXACT,
        bar(&mut no_fonts, "zeyes", 10, true, close_only(true)),
    );

    // C8〜C10: 最小化・最大化のボタン(フォントの要らないもの。ボタンの印は線で描く)。
    let all = |maximized, hot| TitleLook {
        buttons: TitleButtons::ALL,
        maximized,
        hot,
        unresponsive: false,
    };
    run.image(
        "c8-all-buttons",
        Tolerance::EXACT,
        bar(&mut no_fonts, "", 200, true, all(false, None)),
    );
    run.image(
        "c9-maximized-hot-maximize",
        Tolerance::EXACT,
        bar(
            &mut no_fonts,
            "",
            200,
            true,
            all(true, Some(TitleButton::Maximize)),
        ),
    );
    run.image(
        "c10-inactive-hot-minimize-no-maximize",
        Tolerance::EXACT,
        bar(
            &mut no_fonts,
            "",
            200,
            false,
            TitleLook {
                buttons: TitleButtons {
                    minimize: true,
                    maximize: false,
                },
                maximized: false,
                hot: Some(TitleButton::Minimize),
                unresponsive: false,
            },
        ),
    );

    // C12: テスト用のアイコンの置き場から読んだ、アプリのアイコン(フォントの要らないもの)。
    let mut icons = IconCache::new(TITLE_ICON_SIZE as u32, vec![test_icons()]);
    icons.load("seinas.test.files");
    let pixels = paint_title_bar(
        &mut no_fonts,
        "",
        200,
        true,
        close_only(false),
        icons.get("seinas.test.files"),
    );
    run.image(
        "c12-app-icon",
        Tolerance::EXACT,
        Image::from_bgrx(200, TITLE_BAR_HEIGHT as usize, &pixels),
    );

    // C11: 応答なしのウィンドウ(フォントの要るもの)。長いタイトルでも、後ろの「(応答なし)」は切らずに出し、
    // 閉じるボタンの地の色を変える。
    run.with_fonts("c11-not-responding-long-title", |run, fonts: &Path| {
        let painter = painter.get_or_insert_with(|| TextPainter::load(&FontFiles::in_dir(fonts)).0);
        run.image(
            "c11-not-responding-long-title",
            Tolerance::EXACT,
            bar(
                painter,
                "とても長いタイトルのウィンドウです。どこまでも続きます",
                200,
                true,
                TitleLook {
                    unresponsive: true,
                    ..close_only(false)
                },
            ),
        );
    });

    // S1〜S3: 切り替えの一覧。
    // S1(フォントの要るもの):日本語のタイトル、長いタイトルの切り詰め、最小化の印、タイトルの無いウィンドウ。
    // 2 つ目(長いタイトル)が今の候補。
    run.with_fonts("s1-switcher", |run, fonts: &Path| {
        let painter = painter.get_or_insert_with(|| TextPainter::load(&FontFiles::in_dir(fonts)).0);
        let entries = [
            entry("文字入力(text-input)", false),
            entry(
                "とても長いタイトルのウィンドウです。どこまでも、どこまでも、どこまでも続きます",
                false,
            ),
            entry("最小化したウィンドウ", true),
            entry("", false),
        ];
        run.image(
            "s1-switcher",
            Tolerance::EXACT,
            switcher(painter, &entries, 1, Size::new(1280, 720)),
        );
    });
    // S2(フォントの要るもの):320x240 の出力に 20 個。今の候補(10、最小化している)の前後だけを出し、上と下に
    // 隠れた数を出す。
    run.with_fonts("s2-switcher-many-small-output", |run, fonts: &Path| {
        let painter = painter.get_or_insert_with(|| TextPainter::load(&FontFiles::in_dir(fonts)).0);
        let entries: Vec<SwitcherEntry> = (0..20)
            .map(|i| entry(&format!("ウィンドウ {i}"), i % 5 == 0))
            .collect();
        run.image(
            "s2-switcher-many-small-output",
            Tolerance::EXACT,
            switcher(painter, &entries, 10, Size::new(320, 240)),
        );
    });
    // S3(フォントの要らないもの):枠、地、強調、最小化の印の位置。
    run.image(
        "s3-switcher-no-fonts",
        Tolerance::EXACT,
        switcher(
            &mut no_fonts,
            &[entry("a", false), entry("b", true), entry("c", false)],
            1,
            Size::new(640, 480),
        ),
    );

    run.finish();
}
