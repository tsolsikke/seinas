//! ウィンドウの飾り。上に付けるタイトルバーと、その右の端のボタン(最小化、最大化と元に戻す、閉じる)。
//!
//! 帯は、Seinasの側で描く(クライアントには、xdg-decorationで「サーバーの側で飾りを描く」と伝える)。
//! ここにあるのは、大きさと位置の計算と、帯の絵(バイト列)を作る所だけで、土台(Smithay)を使わない。
//! 帯の絵を描画の要素にする所は `title_bar.rs` にある。
//! 決まりの説明は `docs/window-placement.md` にある。

use std::path::Path;

use seinas_text::{Canvas, FontSetup, Line, TextPainter, DEFAULT_SIZE};
use tracing::{info, warn};

use crate::geometry::{Point, Rect, Size};

/// 題名の帯の高さ(画素)。
pub const TITLE_BAR_HEIGHT: i32 = 24;
/// 題名の文字の大きさ(画素)。
pub const TITLE_SIZE: f32 = DEFAULT_SIZE;
/// 帯の左右の端から、題名までの空き(画素)。
pub const TITLE_PADDING: i32 = 8;
/// タイトルバーのボタンの大きさ(ピクセル)。タイトルバーの右の端から、タイトルバーと同じ高さの正方形で並べる。
pub const TITLE_BUTTON_SIZE: i32 = TITLE_BAR_HEIGHT;
/// 閉じるボタンの大きさ(画素)。帯の右の端に、帯と同じ高さの正方形で置く。
pub const CLOSE_BUTTON_SIZE: i32 = TITLE_BUTTON_SIZE;
/// 最小化と最大化の印の大きさ(ピクセル)。ボタンの中央に描く。
const MARK_SIZE: i32 = 10;
/// 閉じるボタンの「×」の印の大きさ(画素)。ボタンの中央に描く。
const CLOSE_MARK_SIZE: i32 = 10;
/// ウィンドウを動かしても、帯のうち、横にこれだけ(画素)は画面の中に残す。帯をもう一度掴めるようにする。
/// 左へ出すときは、右の端のボタン(いちばん多いときの 3 つ)の分を足して残す。
pub const MIN_VISIBLE_BAR: i32 = 48;
/// タイトルバーのボタンが、いちばん多いとき(最小化、最大化、閉じる)の幅。
pub const MAX_BUTTONS_WIDTH: i32 = 3 * TITLE_BUTTON_SIZE;

// 色は (赤, 緑, 青)。
/// 選ばれているウィンドウの、帯と題名の色。
pub const ACTIVE_BAR: [u8; 3] = [0x2f, 0x6f, 0xb5];
pub const ACTIVE_TITLE: [u8; 3] = [0xff, 0xff, 0xff];
/// 選ばれていないウィンドウの、帯と題名の色。
pub const INACTIVE_BAR: [u8; 3] = [0x4a, 0x50, 0x5c];
pub const INACTIVE_TITLE: [u8; 3] = [0xc8, 0xcc, 0xd4];
/// ポインターを乗せたときの、閉じるボタンの地と印の色。
pub const HOT_CLOSE_BUTTON: [u8; 3] = [0xc4, 0x2b, 0x1c];
pub const HOT_CLOSE_MARK: [u8; 3] = [0xff, 0xff, 0xff];
/// ポインターを乗せたときの、最小化と最大化のボタンの地の色(activated のウィンドウと、そうでないもの)。
pub const HOT_ACTIVE_BUTTON: [u8; 3] = [0x46, 0x82, 0xc4];
pub const HOT_INACTIVE_BUTTON: [u8; 3] = [0x5c, 0x63, 0x70];

/// タイトルバーのボタン。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleButton {
    Minimize,
    /// 最大化か、最大化しているときは元に戻す。
    Maximize,
    Close,
}

/// タイトルバーに出すボタン。閉じるボタンは、いつも出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct TitleButtons {
    /// 最小化のボタン。戻す手段(foreign-toplevel の manager を bind しているシェルの部品)があるときだけ出す。
    pub minimize: bool,
    /// 最大化のボタン。大きさの決まったウィンドウ(最小と最大が同じ)には出さない。
    pub maximize: bool,
}

impl TitleButtons {
    /// 閉じるボタンだけ。
    pub const CLOSE_ONLY: TitleButtons = TitleButtons {
        minimize: false,
        maximize: false,
    };
    /// 3 つとも。
    pub const ALL: TitleButtons = TitleButtons {
        minimize: true,
        maximize: true,
    };

    /// 出すボタン。右の端から順(閉じる、最大化、最小化)。
    pub fn shown(self) -> Vec<TitleButton> {
        let mut buttons = vec![TitleButton::Close];
        if self.maximize {
            buttons.push(TitleButton::Maximize);
        }
        if self.minimize {
            buttons.push(TitleButton::Minimize);
        }
        buttons
    }

    /// ボタンの並びの幅。
    pub fn width(self) -> i32 {
        self.shown().len() as i32 * TITLE_BUTTON_SIZE
    }

    /// 幅が `width` のタイトルバーで、左の端から `x` の所にあるボタン。
    pub fn at(self, width: i32, x: f64) -> Option<TitleButton> {
        let from_right = width as f64 - x;
        if from_right <= 0.0 {
            return None;
        }
        let index = ((from_right - 1e-9).max(0.0) / TITLE_BUTTON_SIZE as f64) as usize;
        self.shown().get(index).copied()
    }

    /// ボタン `button` の左の端(幅が `width` のタイトルバーで)。出していなければNone。
    pub fn left_of(self, width: i32, button: TitleButton) -> Option<i32> {
        let index = self.shown().iter().position(|&b| b == button)? as i32;
        Some(width - (index + 1) * TITLE_BUTTON_SIZE)
    }
}

/// タイトルバーの見た目のうち、タイトルと色のほかのもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct TitleLook {
    pub buttons: TitleButtons,
    /// 最大化しているか(最大化のボタンを、元に戻すの印で描く)。
    pub maximized: bool,
    /// ポインターが乗っているボタン(押している間は、押したボタンの上にあるとき)。
    pub hot: Option<TitleButton>,
}

/// 帯を含めた、ウィンドウの外形の大きさ。`content` は、クライアントが描く中身の大きさ。
pub fn outer_size(content: Size) -> Size {
    (content.w, content.h + TITLE_BAR_HEIGHT).into()
}

/// 外形の左上が `outer` にあるウィンドウの、中身の左上。中身は、帯の下に置く。
pub fn content_location(outer: Point) -> Point {
    (outer.x, outer.y + TITLE_BAR_HEIGHT).into()
}

/// 画面の大きさが `screen` のとき、クライアントに伝える中身の大きさ。帯のぶんだけ低い。
pub fn content_size_for(screen: Size) -> Size {
    (screen.w, (screen.h - TITLE_BAR_HEIGHT).max(1)).into()
}

/// 外枠の幅が `width` のウィンドウを、左上が `wanted` に来るように動かしたいとき、実際に置く位置。
///
/// タイトルバーをもう一度 grab できるように、使える範囲(`area`。画面から layer-shell の exclusive zone を
/// 除いた矩形)の外へ出せる範囲を限る。
/// - 上: タイトルバーの上の端は、使える範囲の上の端より上へ行かない。
/// - 下: タイトルバーの全体が、使える範囲の中に残る。
/// - 右へ出すとき: タイトルバーの左の [`MIN_VISIBLE_BAR`] ピクセルが、使える範囲の中に残る。
/// - 左へ出すとき: タイトルバーの右の [`MIN_VISIBLE_BAR`] + [`MAX_BUTTONS_WIDTH`] ピクセル(ボタンの手前に、
///   grab できる所が残る)が、使える範囲の中に残る。
/// - どちらも、タイトルバーがそれより短ければ、その全体が残る。
pub fn clamp_outer_location(wanted: Point, width: i32, area: Rect) -> Point {
    let keep_left_part = MIN_VISIBLE_BAR.min(width);
    let keep_right_part = (MIN_VISIBLE_BAR + MAX_BUTTONS_WIDTH).min(width);
    let (left, top) = (area.loc.x, area.loc.y);
    let (right, bottom) = (left + area.size.w, top + area.size.h);
    let x = wanted
        .x
        .min(right - keep_left_part)
        .max(left + keep_right_part - width);
    let y = wanted.y.min(bottom - TITLE_BAR_HEIGHT).max(top);
    (x, y).into()
}

/// 帯に出す文字を決める。題名があれば題名、無ければapp_id、それも無ければ何も出さない。
pub fn title_text<'a>(title: Option<&'a str>, app_id: Option<&'a str>) -> &'a str {
    [title, app_id]
        .into_iter()
        .flatten()
        .find(|text| !text.trim().is_empty())
        .unwrap_or("")
}

/// 同梱のフォントの置き場 `dir` と、設定のファイル `config`(Noneなら既定の場所)で、フォントを読む
/// (ADR 0003)。読めないフォントや、設定の読めない行があっても止まらず、原因をログに出す。
///
/// 1つも読めなければ、題名の文字は出ない(帯だけを描く)。
pub fn load_fonts(dir: &Path, config: Option<&Path>) -> TextPainter {
    let setup = FontSetup {
        bundled_dir: dir.to_owned(),
        config_file: config.map(Path::to_owned),
    };
    let (painter, messages) = TextPainter::load_setup(&setup);
    for message in &messages {
        warn!("{message}");
    }
    if !painter.can_draw() {
        warn!(
            "no font could be loaded from {}; window titles will not be shown",
            dir.display()
        );
    } else {
        info!(
            "fonts: {} (bundled fonts in {})",
            painter.describe(),
            dir.display()
        );
    }
    painter
}

/// 帯の絵を作る。幅は `width`、高さは [`TITLE_BAR_HEIGHT`]。画素の並びは B, G, R, X。
///
/// タイトルは左に寄せる。右の端には、`look` のボタンを並べ(右から、閉じる、最大化、最小化)、タイトルは、その手前に
/// 収める。収まらなければ、末尾を切って「…」を付ける。`look.hot` のボタンは、目立たせて描く。
pub fn paint_title_bar(
    painter: &mut TextPainter,
    text: &str,
    width: i32,
    active: bool,
    look: TitleLook,
) -> Vec<u8> {
    let (bar, title) = if active {
        (ACTIVE_BAR, ACTIVE_TITLE)
    } else {
        (INACTIVE_BAR, INACTIVE_TITLE)
    };
    let width = width.max(1);
    let mut pixels = [bar[2], bar[1], bar[0], 0xff].repeat((width * TITLE_BAR_HEIGHT) as usize);
    painter.draw_line(
        &mut Canvas::new(&mut pixels, width as usize, TITLE_BAR_HEIGHT as usize),
        text,
        Line {
            size: TITLE_SIZE,
            color: title,
            x: TITLE_PADDING,
            top: 0,
            height: TITLE_BAR_HEIGHT,
            max_width: width - look.buttons.width() - TITLE_PADDING * 2,
        },
    );
    let (ground, mark) = if look.hot == Some(TitleButton::Close) {
        (Some(HOT_CLOSE_BUTTON), HOT_CLOSE_MARK)
    } else {
        (None, title)
    };
    paint_close_button(&mut pixels, width, ground, mark);
    let hot_ground = if active {
        HOT_ACTIVE_BUTTON
    } else {
        HOT_INACTIVE_BUTTON
    };
    for button in [TitleButton::Maximize, TitleButton::Minimize] {
        if let Some(left) = look.buttons.left_of(width, button) {
            let ground = (look.hot == Some(button)).then_some(hot_ground);
            let points = mark_points(button, look.maximized);
            paint_button(&mut pixels, width, left, ground, title, &points);
        }
    }
    pixels
}

/// ボタン `button` の印の点(ボタンの左上からの位置)。フォントが無くても出るように、線で描く。
fn mark_points(button: TitleButton, maximized: bool) -> Vec<(i32, i32)> {
    let inset = (TITLE_BUTTON_SIZE - MARK_SIZE) / 2;
    let (low, high) = (inset, inset + MARK_SIZE - 1);
    let mut points = Vec::new();
    let mut outline = |a: (i32, i32), b: (i32, i32)| {
        for x in a.0..=b.0 {
            points.push((x, a.1));
            points.push((x, b.1));
        }
        for y in a.1..=b.1 {
            points.push((a.0, y));
            points.push((b.0, y));
        }
    };
    match (button, maximized) {
        // 「−」:太さ 2 の横の線。
        (TitleButton::Minimize, _) => {
            for x in low..=high {
                points.push((x, TITLE_BUTTON_SIZE / 2 - 1));
                points.push((x, TITLE_BUTTON_SIZE / 2));
            }
        }
        // 「□」:太さ 1 の四角。
        (TitleButton::Maximize, false) => outline((low, low), (high, high)),
        // 元に戻す:2 つの重なった四角。奥の四角は、手前の四角に隠れない所(上と右)だけ。
        (TitleButton::Maximize, true) => {
            outline((low, low + 2), (high - 2, high));
            points.extend((low + 2..=high).map(|x| (x, low)));
            points.extend((low..=high - 2).map(|y| (high, y)));
        }
        (TitleButton::Close, _) => {}
    }
    points
}

/// 左の端が `left` のボタンを描く。`ground` があれば地をその色で塗り、`points` に印の色 `mark` の点を打つ。
fn paint_button(
    pixels: &mut [u8],
    width: i32,
    left: i32,
    ground: Option<[u8; 3]>,
    mark: [u8; 3],
    points: &[(i32, i32)],
) {
    let mut put = |x: i32, y: i32, color: [u8; 3]| {
        // 幅がボタンより狭いウィンドウでは、はみ出したぶんを描かない。
        if (0..width).contains(&x) && (0..TITLE_BAR_HEIGHT).contains(&y) {
            let at = ((y * width + x) * 4) as usize;
            pixels[at..at + 3].copy_from_slice(&[color[2], color[1], color[0]]);
        }
    };
    if let Some(ground) = ground {
        for y in 0..TITLE_BAR_HEIGHT {
            for x in left..left + TITLE_BUTTON_SIZE {
                put(x, y, ground);
            }
        }
    }
    for &(x, y) in points {
        put(left + x, y, mark);
    }
}

/// 帯の右の端に、閉じるボタンを描く。`ground` があれば、ボタンの地をその色で塗る。
///
/// 「×」の印は、フォントが無くても出るように、文字ではなく線で描く。
fn paint_close_button(pixels: &mut [u8], width: i32, ground: Option<[u8; 3]>, mark: [u8; 3]) {
    let left = width - CLOSE_BUTTON_SIZE;
    let mut put = |x: i32, y: i32, color: [u8; 3]| {
        // 幅がボタンより狭いウィンドウでは、はみ出したぶんを描かない。
        if (0..width).contains(&x) && (0..TITLE_BAR_HEIGHT).contains(&y) {
            let at = ((y * width + x) * 4) as usize;
            pixels[at..at + 3].copy_from_slice(&[color[2], color[1], color[0]]);
        }
    };
    if let Some(ground) = ground {
        for y in 0..TITLE_BAR_HEIGHT {
            for x in left..width {
                put(x, y, ground);
            }
        }
    }
    // 2本の斜めの線。太さは2画素。
    let inset = (CLOSE_BUTTON_SIZE - CLOSE_MARK_SIZE) / 2;
    for step in 0..CLOSE_MARK_SIZE {
        let y = inset + step;
        for x in [
            left + inset + step,
            left + inset + CLOSE_MARK_SIZE - 1 - step,
        ] {
            put(x, y, mark);
            put(x - 1, y, mark);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 閉じるボタンだけで、閉じるボタンにポインターが乗っている見た目。
    fn close_hot() -> TitleLook {
        TitleLook {
            hot: Some(TitleButton::Close),
            ..TitleLook::default()
        }
    }

    #[test]
    fn the_bar_shows_the_title_or_else_the_app_id() {
        assert_eq!(title_text(Some("zeyes"), Some("seinas.zeyes-min")), "zeyes");
        assert_eq!(
            title_text(None, Some("seinas.zeyes-min")),
            "seinas.zeyes-min"
        );
        // 空の題名は、無いものとして扱う。
        assert_eq!(
            title_text(Some(""), Some("seinas.zeyes-min")),
            "seinas.zeyes-min"
        );
        assert_eq!(title_text(Some("  "), None), "");
        assert_eq!(title_text(None, None), "");
    }

    #[test]
    fn the_content_sits_below_the_bar() {
        assert_eq!(outer_size((320, 240).into()), (320, 264).into());
        assert_eq!(content_location((32, 32).into()), (32, 56).into());
        assert_eq!(content_size_for((640, 480).into()), (640, 456).into());
    }

    #[test]
    fn without_fonts_the_bar_is_painted_without_a_title() {
        let mut painter = TextPainter::without_fonts();
        for (active, color) in [(true, ACTIVE_BAR), (false, INACTIVE_BAR)] {
            let pixels = paint_title_bar(&mut painter, "zeyes", 100, active, TitleLook::default());
            assert_eq!(pixels.len(), 100 * TITLE_BAR_HEIGHT as usize * 4);
            // 閉じるボタンの手前は、帯の色だけ。
            assert!((0..TITLE_BAR_HEIGHT)
                .all(|y| (0..100 - CLOSE_BUTTON_SIZE).all(|x| pixel(&pixels, 100, x, y) == color)));
        }
    }

    /// 帯の絵の(x, y)の色(赤, 緑, 青)。
    fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 3] {
        let at = ((y * width + x) * 4) as usize;
        [pixels[at + 2], pixels[at + 1], pixels[at]]
    }

    #[test]
    fn the_close_button_is_drawn_at_the_right_end_without_fonts() {
        let mut painter = TextPainter::without_fonts();
        let width = 100;
        let left = width - CLOSE_BUTTON_SIZE;

        // ふだん: 地は帯の色のままで、中央に、題名と同じ色の「×」がある。
        let pixels = paint_title_bar(&mut painter, "", width, true, TitleLook::default());
        assert_eq!(pixel(&pixels, width, left, 0), ACTIVE_BAR);
        assert_eq!(
            pixel(&pixels, width, width - 1, TITLE_BAR_HEIGHT - 1),
            ACTIVE_BAR
        );
        // 「×」の中心と、4つの端。
        for (x, y) in [(11, 11), (12, 12), (7, 7), (16, 7), (7, 16), (16, 16)] {
            assert_eq!(
                pixel(&pixels, width, left + x, y),
                ACTIVE_TITLE,
                "({x}, {y})"
            );
        }
        // 「×」の上下左右の真ん中は、空いている。
        for (x, y) in [(12, 7), (12, 16), (7, 12), (17, 12)] {
            assert_eq!(pixel(&pixels, width, left + x, y), ACTIVE_BAR, "({x}, {y})");
        }
        let inactive = paint_title_bar(&mut painter, "", width, false, TitleLook::default());
        assert_eq!(pixel(&inactive, width, left + 12, 12), INACTIVE_TITLE);

        // ポインターを乗せたとき: 地が赤、印が白になる。ボタンの外は変わらない。
        let hot = paint_title_bar(&mut painter, "", width, true, close_hot());
        assert_eq!(pixel(&hot, width, left, 0), HOT_CLOSE_BUTTON);
        assert_eq!(
            pixel(&hot, width, width - 1, TITLE_BAR_HEIGHT - 1),
            HOT_CLOSE_BUTTON
        );
        assert_eq!(pixel(&hot, width, left + 12, 12), HOT_CLOSE_MARK);
        assert_eq!(pixel(&hot, width, left - 1, 12), ACTIVE_BAR);

        // ボタンより狭いウィンドウでも、落ちない。
        paint_title_bar(&mut painter, "", 10, true, close_hot());
    }

    #[test]
    fn a_moved_window_keeps_its_bar_reachable() {
        let screen = Rect::from_size((640, 480).into());
        let clamp = |x: i32, y: i32| {
            let p = clamp_outer_location((x, y).into(), 320, screen);
            (p.x, p.y)
        };
        // 画面の中なら、そのまま。
        assert_eq!(clamp(100, 50), (100, 50));
        // 上: 帯の上の端は、画面の上の端より上へ行かない。
        assert_eq!(clamp(100, -30), (100, 0));
        // 下: 帯の全体が残る(中身は、画面の下へ出てよい)。
        assert_eq!(clamp(100, 456), (100, 456));
        assert_eq!(clamp(100, 470), (100, 456));
        // 左へ: タイトルバーの右の 48 + 72(ボタン 3 つ)= 120 ピクセルが残る。右へ: 左の 48 ピクセルが残る。
        assert_eq!(clamp(-200, 50), (-200, 50));
        assert_eq!(clamp(-300, 50), (-200, 50));
        assert_eq!(clamp(592, 50), (592, 50));
        assert_eq!(clamp(700, 50), (592, 50));
        // 隅でも、両方の限りが働く。
        assert_eq!(clamp(-1000, -1000), (-200, 0));
        assert_eq!(clamp(1000, 1000), (592, 456));
        // 帯が48画素より短いウィンドウは、帯の全体が残る。
        let narrow = clamp_outer_location((-100, 10).into(), 30, screen);
        assert_eq!((narrow.x, narrow.y), (0, 10));
        let narrow = clamp_outer_location((700, 10).into(), 30, screen);
        assert_eq!((narrow.x, narrow.y), (610, 10));
    }

    #[test]
    fn the_buttons_line_up_from_the_right_end() {
        let all = TitleButtons::ALL;
        assert_eq!(
            all.shown(),
            [
                TitleButton::Close,
                TitleButton::Maximize,
                TitleButton::Minimize
            ]
        );
        assert_eq!(all.width(), 72);
        // 幅 200:閉じるは 176〜200、最大化は 152〜176、最小化は 128〜152。
        assert_eq!(all.at(200, 199.9), Some(TitleButton::Close));
        assert_eq!(all.at(200, 176.0), Some(TitleButton::Close));
        assert_eq!(all.at(200, 175.9), Some(TitleButton::Maximize));
        assert_eq!(all.at(200, 152.0), Some(TitleButton::Maximize));
        assert_eq!(all.at(200, 140.0), Some(TitleButton::Minimize));
        assert_eq!(all.at(200, 127.9), None);
        assert_eq!(all.at(200, 200.0), None);
        assert_eq!(all.left_of(200, TitleButton::Minimize), Some(128));
        // 最大化のボタンが無ければ、最小化のボタンが閉じるボタンの隣に来る。
        let no_maximize = TitleButtons {
            minimize: true,
            maximize: false,
        };
        assert_eq!(no_maximize.at(200, 160.0), Some(TitleButton::Minimize));
        assert_eq!(no_maximize.left_of(200, TitleButton::Maximize), None);
        assert_eq!(TitleButtons::CLOSE_ONLY.at(200, 160.0), None);
    }

    #[test]
    fn the_buttons_are_drawn_without_fonts() {
        let mut painter = TextPainter::without_fonts();
        let look = TitleLook {
            buttons: TitleButtons::ALL,
            maximized: false,
            hot: Some(TitleButton::Minimize),
        };
        let pixels = paint_title_bar(&mut painter, "", 200, true, look);
        // 最小化(128〜152)の地は、ポインターが乗っている色。「−」は y 11 と 12、x 135〜144。
        assert_eq!(pixel(&pixels, 200, 128, 0), HOT_ACTIVE_BUTTON);
        assert_eq!(pixel(&pixels, 200, 135, 11), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 144, 12), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 135, 13), HOT_ACTIVE_BUTTON);
        // 最大化(152〜176)の「□」は、x 159〜168、y 7〜16 の枠。
        assert_eq!(pixel(&pixels, 200, 152, 0), ACTIVE_BAR);
        assert_eq!(pixel(&pixels, 200, 159, 7), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 168, 16), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 163, 11), ACTIVE_BAR);
        // 最大化しているときは、元に戻すの印(手前の四角は x 159〜166、y 9〜16)。
        let maximized = paint_title_bar(
            &mut painter,
            "",
            200,
            true,
            TitleLook {
                maximized: true,
                ..look
            },
        );
        assert_eq!(pixel(&maximized, 200, 159, 9), ACTIVE_TITLE);
        assert_eq!(pixel(&maximized, 200, 168, 7), ACTIVE_TITLE);
        assert_eq!(pixel(&maximized, 200, 159, 7), ACTIVE_BAR);
    }
}
