//! タスクバーの絵を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! `paint` は Wayland を使わない関数なので、それだけを呼ぶ。アプリのアイコンは、テスト用のアイコンの置き場
//! (`crates/seinas-icons/tests/data/share`。`tools/make-test-icons.py` が作る)から読む。

use std::path::Path;

use seinas_golden::{Image, Runner, Tolerance};
use seinas_icons::IconCache;
use seinas_taskbar::{
    list_height, paint, paint_list, Clock, Entry, Pointer, Target, HEIGHT, ICON_SIZE, LIST_WIDTH,
};
use seinas_text::{FontFiles, TextPainter};

fn entry(app_id: &str, activated: bool, minimized: bool) -> Entry {
    Entry {
        title: app_id.to_owned(),
        app_id: app_id.to_owned(),
        activated,
        minimized,
    }
}

/// テスト用のアイコンを読んだ置き場。
fn icons(entries: &[Entry]) -> IconCache {
    let share = Path::new(env!("CARGO_MANIFEST_DIR")).join("../seinas-icons/tests/data/share");
    let mut cache = IconCache::new(ICON_SIZE as u32, vec![share]);
    for entry in entries {
        cache.load(&entry.app_id);
    }
    cache
}

fn clock(time: &str, date: Option<&str>) -> Clock {
    Clock {
        time: time.to_owned(),
        date: date.map(str::to_owned),
    }
}

/// タスクバーを描いて、RGB の画像にする。
fn bar(
    painter: &mut TextPainter,
    width: i32,
    entries: &[Entry],
    pointer: Pointer,
    clock: &Clock,
) -> Image {
    let torch = seinas_icons::torch(ICON_SIZE as u32);
    let pixels = paint(
        painter,
        width,
        entries,
        &icons(entries),
        &torch,
        pointer,
        clock,
    );
    Image::from_bgrx(width as usize, HEIGHT as usize, &pixels)
}

/// 1 つのケース:名前、幅、並べるウィンドウ、ポインター、時計。
type Case<'a> = (&'a str, i32, &'a [Entry], Pointer, Clock);

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-taskbar");

    // 前面(activated)のもの、背面のもの、アイコンの無いもの(既定のアイコン)。ポインターは 2 つ目のタスクの上。
    let four = [
        entry("seinas.test.files", true, false),
        entry("seinas.test.terminal", false, false),
        entry("seinas.test.notes", false, false),
        entry("seinas.test.missing", false, false),
    ];
    let hot_second = Pointer {
        hot: Some(Target::Task(1)),
        ..Pointer::default()
    };
    // 最小化しているもの(背面の点)と、時刻が取れないとき(1 段の --:--)。
    let minimized = [
        entry("seinas.test.files", false, true),
        entry("seinas.test.settings", true, false),
    ];
    // トーチを押している。
    let torch_pressed = Pointer {
        hot: Some(Target::Torch),
        pressed: Some(Target::Torch),
        ..Pointer::default()
    };
    // 入りきらないほどのタスク(間隔を詰め、末尾に「…」を置き、入らないものは隠す)。前面は見えるタスク。
    let many: Vec<Entry> = (0..12)
        .map(|i| entry("seinas.test.notes", i == 0, false))
        .collect();
    // 前面のタスクが隠れている(「…」に控えめなオレンジの点)。ポインターは「…」の上。
    let front_hidden: Vec<Entry> = (0..12)
        .map(|i| entry("seinas.test.notes", i == 10, false))
        .collect();
    let hot_more = Pointer {
        hot: Some(Target::More),
        ..Pointer::default()
    };

    // F1〜F3、F5、F6: フォントの要るもの。
    let cases: [Case; 5] = [
        (
            "f1-four-tasks",
            640,
            &four,
            hot_second,
            clock("12:34", Some("2026/10/26")),
        ),
        (
            "f2-minimized-and-no-clock",
            480,
            &minimized,
            Pointer::default(),
            Clock::unknown(),
        ),
        (
            "f3-torch-pressed",
            320,
            &[],
            torch_pressed,
            clock("09:05", Some("2026/01/02")),
        ),
        (
            "f5-too-many-tasks",
            480,
            &many,
            Pointer::default(),
            clock("23:59", Some("2026/12/31")),
        ),
        (
            "f6-hidden-front-task",
            480,
            &front_hidden,
            hot_more,
            clock("23:59", Some("2026/12/31")),
        ),
    ];
    let mut painter: Option<TextPainter> = None;
    for (name, width, entries, pointer, clock) in cases {
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
                bar(painter, width, entries, pointer, &clock),
            );
        });
    }

    // F7・F8: 隠れたタスクの一覧。前面のウィンドウの行(オレンジの線)、最小化しているもの(補助の文字の色)、
    // アイコンの無いもの、選んでいる行。F7 は全部の行が見える高さ、F8 は 3 行ぶんの高さで、1 行半ほどスクロールした所。
    let hidden = [
        entry("seinas.test.files", false, false),
        entry("seinas.test.terminal", true, false),
        entry("seinas.test.notes", false, true),
        Entry {
            title: "とても長いタイトルのウィンドウ(一覧の幅に収まらないので、切れる)".to_owned(),
            ..entry("seinas.test.missing", false, false)
        },
        entry("seinas.test.settings", false, false),
    ];
    let lists = [
        ("f7-hidden-list", list_height(hidden.len()), Some(3), 0),
        ("f8-hidden-list-scrolled", list_height(3), Some(1), 60),
    ];
    for (name, height, selected, scroll) in lists {
        run.with_fonts(name, |run, fonts: &Path| {
            let painter =
                painter.get_or_insert_with(|| TextPainter::load(&FontFiles::in_dir(fonts)).0);
            let pixels = paint_list(painter, height, &hidden, &icons(&hidden), selected, scroll);
            run.image(
                name,
                Tolerance::EXACT,
                Image::from_bgrx(LIST_WIDTH as usize, height as usize, &pixels),
            );
        });
    }

    // F4: フォントの要らないもの(アイコンと印と地だけ。時計の文字は出ない)。
    let mut no_fonts = TextPainter::without_fonts();
    run.image(
        "f4-no-fonts",
        Tolerance::EXACT,
        bar(
            &mut no_fonts,
            640,
            &four,
            hot_second,
            &clock("12:34", Some("2026/10/26")),
        ),
    );

    run.finish();
}
