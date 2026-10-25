//! ウィンドウの上に、タイトルバーが描かれることを、ピクセルで確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - タイトルバーは、ウィンドウのコンテンツの上に付く。高さは24で、幅はコンテンツと同じ。
//! - タイトルバーには、クライアントが付けたタイトルを、左に寄せて出す。タイトルが無ければapp_id、それも無ければ何も出さない。
//! - タイトルバーに収まらないタイトルは、末尾を切って「…」を付ける。
//! - タイトルバーの色は、activated な(いちばん手前の)ウィンドウと、そうでないウィンドウとで違う。
//! - フォントが読めないときは、タイトルバーだけを描く。
//!
//! テスト用のクライアント(`test-client window`)のウィンドウは、コンテンツが200x150の橙。

mod common;

use std::path::Path;

use common::{
    bar_color, title_ink, upper_bar_color, upper_title_ink, Compositor, Shot, ACTIVE_BAR, AROUND,
    BACKGROUND, BAR, BUTTONS, FRAME_RADIUS, INACTIVE_BAR, TITLE_LEFT, TITLE_PADDING,
};

/// テスト用のクライアントのウィンドウの、コンテンツの大きさと色(橙)。
const WINDOW_WIDTH: usize = 200;
const WINDOW_HEIGHT: usize = 150;
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
/// スロットを1つずらす量。
const STEP: usize = 32;

/// 外枠の左上が(`x0`, `y0`)にあるテスト用のウィンドウが、タイトルバーの色 `bar` で描かれているか。
fn shows_window(shot: &Shot, x0: usize, y0: usize, bar: [u8; 3]) -> bool {
    bar_color(shot, x0, y0, WINDOW_WIDTH) == Some(bar)
        // タイトルバーの左の端も(丸く欠いた角の下で)、タイトルバーの色。
        && shot.pixel(x0, y0 + FRAME_RADIUS) == bar
        // コンテンツは、タイトルバーのすぐ下から始まる(下の角は丸く欠いてあるので、その上の行を見る)。
        && shot.pixel(x0, y0 + BAR) == ORANGE
        && shot.pixel(x0 + WINDOW_WIDTH - 1, y0 + BAR + WINDOW_HEIGHT - 1 - FRAME_RADIUS) == ORANGE
}

/// タイトルに使える範囲の右の端。右の端のボタン(最大化と閉じる)と、その手前の空きを除く。
const TITLE_END: usize = WINDOW_WIDTH - BUTTONS - TITLE_PADDING;

/// タイトルの文字のピクセルが、置かれてよい範囲(アイコンの右の空きから、閉じるボタンの手前の空きまで)に収まって
/// いるか。
fn ink_stays_inside(ink: &[(usize, usize)]) -> bool {
    ink.iter()
        .all(|&(x, _)| (TITLE_LEFT..TITLE_END).contains(&x))
}

/// 文字のピクセルの位置を、いちばん左の列を0にそろえる(置かれた位置によらず、形を比べるため)。
fn shape(ink: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let left = ink.iter().map(|&(x, _)| x).min().unwrap_or(0);
    ink.iter().map(|&(x, y)| (x - left, y)).collect()
}

/// 左上のウィンドウのタイトルバーに、文字のピクセルが `enough` 個以上出るまで待ち、その位置を返す。
fn wait_for_title(seinas: &Compositor, what: &str, enough: usize) -> Vec<(usize, usize)> {
    let shot = seinas.wait_for_shot(what, |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR) && title_ink(shot, 0, 0, WINDOW_WIDTH).len() >= enough
    });
    title_ink(&shot, 0, 0, WINDOW_WIDTH)
}

#[test]
fn the_title_is_drawn_on_a_bar_above_the_window() {
    let mut seinas = Compositor::start("ta");
    let mut client = seinas.connect_test_client(&["window", "--title", "zeyes"]);

    // タイトルバーがあり、タイトルの位置に文字のピクセルがある。
    let ink = wait_for_title(&seinas, "the title bar with a title", 30);
    assert!(ink_stays_inside(&ink));
    // 左に寄せてある。「zeyes」は短いので、タイトルバーの右の半分には何も無い。
    let left = ink.iter().map(|&(x, _)| x).min().unwrap();
    let right = ink.iter().map(|&(x, _)| x).max().unwrap();
    assert!((TITLE_LEFT..TITLE_LEFT + 3).contains(&left), "{left}");
    assert!(right < WINDOW_WIDTH / 2, "{right}");
    // タイトルバーの外(ウィンドウの右と、コンテンツの下)は、背景のまま。
    let shot = seinas.shot();
    assert_eq!(shot.pixel(WINDOW_WIDTH + AROUND, 2), BACKGROUND);
    assert_eq!(shot.pixel(2, BAR + WINDOW_HEIGHT + AROUND), BACKGROUND);

    // タイトルを変えると、描き直される。
    client.send_line("title 別の題名");
    let changed = seinas.wait_for_shot("the new title", |shot| {
        let now = title_ink(shot, 0, 0, WINDOW_WIDTH);
        now.len() >= 30 && now != ink
    });
    let changed = title_ink(&changed, 0, 0, WINDOW_WIDTH);
    assert!(ink_stays_inside(&changed));

    // 元のタイトルに戻すと、元の絵に戻る。
    client.send_line("title zeyes");
    seinas.wait_for_screen("the first title again", |shot| {
        title_ink(shot, 0, 0, WINDOW_WIDTH) == ink
    });
    assert!(seinas.process.is_running());
}

#[test]
fn the_bar_of_the_activated_window_has_its_own_color() {
    let mut seinas = Compositor::start("tb");

    // 1つ目は、つないだときに activated になっている。
    let _first = seinas.connect_test_client(&["window", "--title", "first"]);
    let alone = wait_for_title(&seinas, "the first window, activated", 30);

    // 2つ目をつなぐと、2つ目が選ばれ、1つ目のタイトルバーは、activated でない色に変わる。タイトルは、そのまま出ている。
    let second = seinas.connect_test_client(&["window", "--title", "second"]);
    let shot = seinas.wait_for_shot("the second window activated instead of the first", |shot| {
        shows_window(shot, STEP, STEP, ACTIVE_BAR)
            && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
            && !title_ink(shot, STEP, STEP, WINDOW_WIDTH).is_empty()
    });
    assert_ne!(ACTIVE_BAR, INACTIVE_BAR);
    // 1 つ目のタイトルバーの下のほうは、2 つ目(と、その枠と影)に隠れるので、隠れない上の行で、文字の形を比べる。
    let uncovered = STEP - AROUND;
    let behind = upper_title_ink(&shot, 0, 0, WINDOW_WIDTH, uncovered);
    let alone_above: Vec<(usize, usize)> = alone
        .iter()
        .copied()
        .filter(|&(_, y)| y < uncovered)
        .collect();
    assert!(!behind.is_empty());
    assert_eq!(shape(&behind), shape(&alone_above));

    // 2つ目が去ると、1つ目が、また activated の色に戻る。
    drop(second);
    seinas.wait_for_screen("the first window activated again", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR) && shot.pixel(STEP + 190, STEP + 170) == BACKGROUND
    });
    assert!(seinas.process.is_running());
}

#[test]
fn a_window_without_a_title_shows_its_app_id_or_else_nothing() {
    let mut seinas = Compositor::start("tc");

    // タイトルもapp_idも無い: タイトルバーだけが出る。
    let mut bare = seinas.connect_test_client(&["window"]);
    bare.wait_for_line("window: drawn");
    let shot = seinas.wait_for_shot("a bar without a title", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    assert!(title_ink(&shot, 0, 0, WINDOW_WIDTH).is_empty());

    // 後からタイトルを付けると、出る。空のタイトルに戻すと、消える。
    bare.send_line("title seinas.test");
    let titled = wait_for_title(&seinas, "the title set later", 30);
    bare.send_line("title ");
    seinas.wait_for_screen("the bar without a title again", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR) && title_ink(shot, 0, 0, WINDOW_WIDTH).is_empty()
    });
    drop(bare);
    seinas.wait_for_screen("the background", |shot| shot.pixel(2, 2) == BACKGROUND);

    // タイトルが無く、app_idがある: app_idが出る。同じ文字のタイトルと、同じ絵になる。
    let _named = seinas.connect_test_client(&["window", "--app-id", "seinas.test"]);
    let app_id = wait_for_title(&seinas, "the app_id in place of the title", 30);
    assert_eq!(app_id, titled);
    assert!(seinas.process.is_running());
}

#[test]
fn a_long_title_is_cut_to_fit_the_bar() {
    let seinas = Compositor::start("td");
    let long = "とても長い題名のウィンドウです。どこまでも続きます";
    let client = seinas.connect_test_client(&["window", "--title", long]);
    let ink = wait_for_title(&seinas, "the long title", 30);
    // 左右の空きには、はみ出さない。タイトルバーの右のほうまで、文字が来ている。
    assert!(ink_stays_inside(&ink));
    let right = ink.iter().map(|&(x, _)| x).max().unwrap();
    assert!(right > TITLE_END - 16, "{right}");

    // 切った印の「…」で終わる。印は、横に並んだ小さな点なので、右の端のピクセルは、2〜3行にしか無い。
    let mut rows: Vec<usize> = ink
        .iter()
        .filter(|&&(x, _)| x + 6 > right)
        .map(|&(_, y)| y)
        .collect();
    rows.dedup();
    assert!(rows.len() <= 3, "{rows:?}");
    drop(client);
    seinas.wait_for_screen("the background", |shot| shot.pixel(2, 2) == BACKGROUND);

    // 切った後の絵は、頭の部分に「…」を付けたタイトルと同じになる。
    let prefixes: Vec<String> = (4..long.chars().count())
        .map(|count| long.chars().take(count).chain(['…']).collect())
        .collect();
    let mut matched = false;
    for prefix in prefixes {
        let client = seinas.connect_test_client(&["window", "--title", &prefix]);
        let shorter = wait_for_title(&seinas, "a shorter title", 30);
        drop(client);
        seinas.wait_for_screen("the background", |shot| shot.pixel(2, 2) == BACKGROUND);
        if shorter == ink {
            matched = true;
            break;
        }
    }
    assert!(
        matched,
        "the cut title must be a prefix followed by an ellipsis"
    );
}

#[test]
fn a_character_missing_from_the_ui_font_is_drawn_with_the_fallback_font() {
    let mut seinas = Compositor::start("te");
    // U+2550(二重の横線の罫線)は、BIZ UDPゴシックには無く、控えのGNU Unifont JPにある。
    let mut client = seinas.connect_test_client(&["window", "--title", "═"]);
    let line = wait_for_title(&seinas, "the box-drawing character", 8);
    let column = |ink: &[(usize, usize)], x: usize| ink.iter().filter(|&&(ix, _)| ix == x).count();
    let left = |ink: &[(usize, usize)]| ink.iter().map(|&(x, _)| x).min().unwrap();
    // 横線が2本なので、どの列も、上から下まで続いてはいない。
    assert!(column(&line, left(&line)) <= 6, "{line:?}");

    // どのフォントにも無い文字(私用の文字 U+E000)は、四角の枠になる。枠の左の辺は、縦に続いている。
    client.send_line("title \u{e000}");
    let shot = seinas.wait_for_shot("the box for a missing character", |shot| {
        let now = title_ink(shot, 0, 0, WINDOW_WIDTH);
        now.len() >= 8 && now != line
    });
    let missing = title_ink(&shot, 0, 0, WINDOW_WIDTH);
    assert!(column(&missing, left(&missing)) >= 8, "{missing:?}");
    assert!(seinas.process.is_running());
}

#[test]
fn without_fonts_the_bar_is_drawn_without_a_title() {
    let mut seinas = Compositor::start_with_fonts("tf", Path::new("/nonexistent/seinas-fonts"));
    let first = seinas.connect_test_client(&["window", "--title", "zeyes"]);
    first.wait_for_line("window: drawn");
    let shot = seinas.wait_for_shot("a bar without a title", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    assert!(title_ink(&shot, 0, 0, WINDOW_WIDTH).is_empty());

    // タイトルバーの色の切り替えは、フォントが無くても働く。
    let _second = seinas.connect_test_client(&["window", "--title", "second"]);
    seinas.wait_for_screen("two bars", |shot| {
        shows_window(shot, STEP, STEP, ACTIVE_BAR)
            && upper_bar_color(shot, 0, 0, WINDOW_WIDTH) == INACTIVE_BAR
    });
    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );

    // 原因は、ログに出ている。
    let log = seinas.log();
    assert!(
        log.contains("cannot read the font /nonexistent/seinas-fonts/BIZUDPGothic-Regular.ttf"),
        "{log}"
    );
    assert!(
        log.contains("cannot read the font /nonexistent/seinas-fonts/unifont_jp-18.0.01.otf"),
        "{log}"
    );
    assert!(log.contains("window titles will not be shown"), "{log}");
}

#[test]
fn a_broken_font_file_does_not_stop_the_compositor() {
    // プライマリのフォントの名前で壊れたファイルを置き、フォールバックのフォントは置かない。
    let dir = std::env::temp_dir().join(format!("seinas-broken-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("BIZUDPGothic-Regular.ttf"), b"this is not a font").unwrap();
    let mut seinas = Compositor::start_with_fonts("tg", &dir);
    std::fs::remove_dir_all(&dir).unwrap();

    let client = seinas.connect_test_client(&["window", "--title", "zeyes"]);
    client.wait_for_line("window: drawn");
    let shot = seinas.wait_for_shot("a bar without a title", |shot| {
        shows_window(shot, 0, 0, ACTIVE_BAR)
    });
    assert!(title_ink(&shot, 0, 0, WINDOW_WIDTH).is_empty());
    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
    assert!(
        seinas.log().contains("is not a usable font file"),
        "{}",
        seinas.log()
    );
}

#[test]
fn the_client_is_told_that_the_server_draws_the_decoration() {
    let seinas = Compositor::start("th");
    let client = seinas.connect_test_client(&["window", "--decoration"]);
    // 最初の configure のまとまりまでの行。装飾の取り決めは最初の commit の前に始めるので、答え
    // (zxdg_toplevel_decoration_v1.configure)も、このまとまりに入る。
    let mut lines = Vec::new();
    loop {
        let line = client.next_line("the first configure");
        let done = line.starts_with("window: configure ");
        lines.push(line);
        if done {
            break;
        }
    }
    // 最大化はできると伝える(最小化と全画面はできない)。
    assert!(
        lines.contains(&"window: wm_capabilities [maximize]".to_owned()),
        "{lines:?}"
    );
    // xdg-decorationで尋ねると、サーバーの側で描くと答える。
    assert!(
        lines.contains(&"window: decoration server-side".to_owned()),
        "{lines:?}"
    );
    // 最初のconfigureで伝える大きさは、画面(640x480)からタイトルバーの高さを引いたもの。
    assert_eq!(
        lines.last().unwrap(),
        &format!("window: configure 640x{} [activated]", 480 - BAR)
    );
}
