//! ウィンドウの飾り。上に付けるタイトルバーと、その右の端のボタン(最小化、最大化と元に戻す、閉じる)。
//!
//! タイトルバーは、Seinasの側で描く(クライアントには、xdg-decorationで「サーバーの側で飾りを描く」と伝える)。
//! ここにあるのは、大きさと位置の計算と、タイトルバーの絵(バイト列)を作る所だけで、土台(Smithay)を使わない。
//! タイトルバーの絵を描画の要素にする所は `title_bar.rs` にある。
//! 決まりの説明は `docs/window-placement.md` にある。

use std::path::Path;

use seinas_icons::{fill_shape, Icon};
use seinas_text::{Canvas, FontSetup, Line, TextPainter};
use tracing::{info, warn};

use crate::geometry::{Point, PointF, Rect, Size};

// 大きさと色は、テーマのトークン(`seinas-theme` の `title_bar`)から読む。
/// タイトルバーの高さ(ピクセル)。
pub const TITLE_BAR_HEIGHT: i32 = seinas_theme::title_bar::HEIGHT;
/// タイトルの文字の大きさ(ピクセル)。
pub const TITLE_SIZE: f32 = seinas_theme::title_bar::TEXT_SIZE;
/// タイトルバーの左の端から、アプリのアイコンまでの空き(ピクセル)。右の端のボタンの手前にも、同じだけ空ける。
pub const TITLE_PADDING: i32 = seinas_theme::title_bar::PADDING;
/// タイトルバーの左のアプリのアイコンの大きさ(縦は中央)と、アイコンからタイトルまでの空き。
pub const TITLE_ICON_SIZE: i32 = seinas_theme::title_bar::ICON_SIZE;
pub const TITLE_ICON_GAP: i32 = seinas_theme::title_bar::ICON_GAP;
/// タイトルの文字の左の端。
pub const TITLE_TEXT_X: i32 = TITLE_PADDING + TITLE_ICON_SIZE + TITLE_ICON_GAP;
/// 枠の太さと角の丸み(`seinas-theme` の `frame`)。外枠の角は、[`FRAME_RADIUS`] の丸みで欠く。
pub const BORDER_WIDTH: i32 = seinas_theme::frame::BORDER_WIDTH;
pub const CORNER_RADIUS: i32 = seinas_theme::frame::CORNER_RADIUS;
pub const FRAME_RADIUS: i32 = CORNER_RADIUS - BORDER_WIDTH;
/// タイトルバーのボタンの幅(ピクセル)。タイトルバーの右の端から、タイトルバーと同じ高さで並べる。
pub const TITLE_BUTTON_SIZE: i32 = seinas_theme::title_bar::BUTTON_WIDTH;
/// 閉じるボタンの幅(ピクセル)。タイトルバーの右の端に置く。
pub const CLOSE_BUTTON_SIZE: i32 = TITLE_BUTTON_SIZE;
/// 最小化と最大化の記号の大きさ(ピクセル)。ボタンの中央に描く。
const MARK_SIZE: i32 = seinas_theme::title_bar::MARK_SIZE;
/// 閉じるボタンの「×」の大きさ(ピクセル)。ボタンの中央に描く。
const CLOSE_MARK_SIZE: i32 = seinas_theme::title_bar::CLOSE_MARK_SIZE;
/// ウィンドウを動かしても、タイトルバーのうち、横にこれだけ(ピクセル)は画面の中に残す。タイトルバーをもう一度 grab できるようにする。
/// 左へ出すときは、右の端のボタン(いちばん多いときの 3 つ)の分を足して残す。
pub const MIN_VISIBLE_BAR: i32 = 48;
/// タイトルバーのボタンが、いちばん多いとき(最小化、最大化、閉じる)の幅。
pub const MAX_BUTTONS_WIDTH: i32 = 3 * TITLE_BUTTON_SIZE;

// 色は (赤, 緑, 青)。activated なウィンドウと、そうでないウィンドウの、タイトルバーとタイトルの色。ポインターを
// 乗せたときの、閉じるボタンの地と印の色。応答なしのウィンドウの、閉じるボタンの地の色(印は `HOT_CLOSE_MARK`。
// ポインターを乗せたときは `HOT_CLOSE_BUTTON`)。
pub use seinas_theme::title_bar::{
    ACTIVE_BAR, ACTIVE_TITLE, HOT_ACTIVE_BUTTON, HOT_CLOSE_BUTTON, HOT_CLOSE_MARK,
    HOT_INACTIVE_BUTTON, INACTIVE_BAR, INACTIVE_TITLE, UNRESPONSIVE_CLOSE_BUTTON,
};
/// 応答なしのウィンドウで、タイトルの後に付ける文字。
pub const NOT_RESPONDING: &str = "(応答なし)";

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
    /// 応答なしか(タイトルの後に [`NOT_RESPONDING`] を付け、閉じるボタンの地の色を変える)。
    pub unresponsive: bool,
}

/// 外枠 `outer` の中の点 `point` が、角を欠いた所(外枠の矩形の中で、角の丸み [`FRAME_RADIUS`] の外)か。外枠の外の
/// 点はfalse。
pub fn in_cut_corner(outer: Rect, point: PointF) -> bool {
    if !outer.contains(point) {
        return false;
    }
    let r = f64::from(FRAME_RADIUS);
    let (x, y) = (
        point.x - f64::from(outer.loc.x),
        point.y - f64::from(outer.loc.y),
    );
    let (w, h) = (f64::from(outer.size.w), f64::from(outer.size.h));
    let cx = if x < r {
        r
    } else if x > w - r {
        w - r
    } else {
        return false;
    };
    let cy = if y < r {
        r
    } else if y > h - r {
        h - r
    } else {
        return false;
    };
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy > r * r
}

/// ウィンドウの形(外枠の矩形。`rounded` なら、角を欠いた形)が、点 `point` を含むか。角を欠いた所は、そのウィンドウの
/// ものとして扱わない(クリックは後ろへ渡り、縁での大きさ変更の範囲に入る)。最大化しているウィンドウは、角を欠かない。
pub fn frame_contains(outer: Rect, rounded: bool, point: PointF) -> bool {
    outer.contains(point) && !(rounded && in_cut_corner(outer, point))
}

/// Seinas が描くタイトルバーの高さ。装飾の取り決めで server_side になっているウィンドウ(`decorated`)は
/// [`TITLE_BAR_HEIGHT`]、そのほか(取り決めていない、client_side)は 0(タイトルバーも枠も付けない)。
pub fn bar_height(decorated: bool) -> i32 {
    if decorated {
        TITLE_BAR_HEIGHT
    } else {
        0
    }
}

/// タイトルバーを含めた、ウィンドウの外枠の大きさ。`content` は、クライアントが描くコンテンツの大きさ、`bar` は
/// Seinas が描くタイトルバーの高さ([`bar_height`])。
pub fn outer_size(content: Size, bar: i32) -> Size {
    (content.w, content.h + bar).into()
}

/// 外枠の左上が `outer` にあるウィンドウの、コンテンツの左上。コンテンツは、高さ `bar` のタイトルバーの下に置く。
pub fn content_location(outer: Point, bar: i32) -> Point {
    (outer.x, outer.y + bar).into()
}

/// 画面の大きさが `screen` のとき、クライアントに伝えるコンテンツの大きさ。高さ `bar` のタイトルバーのぶんだけ低い。
pub fn content_size_for(screen: Size, bar: i32) -> Size {
    (screen.w, (screen.h - bar).max(1)).into()
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

/// タイトルバーに出す文字を決める。タイトルがあればタイトル、無ければapp_id、それも無ければ何も出さない。
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
/// 1つも読めなければ、タイトルの文字は出ない(タイトルバーだけを描く)。
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

/// タイトルバーの絵を作る。幅は `width`、高さは [`TITLE_BAR_HEIGHT`]。1 ピクセルは B, G, R, A の 4 バイトで、色は不透明度を
/// 掛けた形(premultiplied)。最大化していなければ、上の 2 つの角を [`FRAME_RADIUS`] の丸みで欠く(透明にする)。
///
/// 左の端にアプリのアイコン `icon` を置き、その右にタイトルを寄せる。右の端には、`look` のボタンを並べ(右から、閉じる、
/// 最大化、最小化)、タイトルは、その手前に収める。収まらなければ、末尾を切って「…」を付ける。`look.hot` のボタンは、
/// 目立たせて描く。
pub fn paint_title_bar(
    painter: &mut TextPainter,
    text: &str,
    width: i32,
    active: bool,
    look: TitleLook,
    icon: &Icon,
) -> Vec<u8> {
    let (bar, title) = if active {
        (ACTIVE_BAR, ACTIVE_TITLE)
    } else {
        (INACTIVE_BAR, INACTIVE_TITLE)
    };
    let width = width.max(1);
    let mut pixels = [bar[2], bar[1], bar[0], 0xff].repeat((width * TITLE_BAR_HEIGHT) as usize);
    icon.draw_onto(
        &mut pixels,
        width,
        TITLE_PADDING,
        (TITLE_BAR_HEIGHT - TITLE_ICON_SIZE) / 2,
    );
    let line = |x: i32, max_width: i32| Line {
        size: TITLE_SIZE,
        color: title,
        x,
        top: 0,
        height: TITLE_BAR_HEIGHT,
        max_width,
    };
    let room = width - look.buttons.width() - TITLE_TEXT_X - TITLE_PADDING;
    let canvas_size = (width as usize, TITLE_BAR_HEIGHT as usize);
    // 応答なしなら、タイトルの後に印の文字を付ける。タイトルが長くても印が見えるよう、印の幅を先に測って、
    // タイトルに使える幅から引く。
    let suffix = look.unresponsive.then(|| {
        let mut scratch = vec![0; canvas_size.0 * canvas_size.1 * 4];
        let canvas = &mut Canvas::new(&mut scratch, canvas_size.0, canvas_size.1);
        painter
            .draw_line(canvas, NOT_RESPONDING, line(0, room))
            .width
    });
    let drawn = painter.draw_line(
        &mut Canvas::new(&mut pixels, canvas_size.0, canvas_size.1),
        text,
        line(TITLE_TEXT_X, room - suffix.unwrap_or(0)),
    );
    if suffix.is_some() {
        let x = TITLE_TEXT_X + drawn.width;
        painter.draw_line(
            &mut Canvas::new(&mut pixels, canvas_size.0, canvas_size.1),
            NOT_RESPONDING,
            line(x, room - drawn.width),
        );
    }
    let (ground, mark) = if look.hot == Some(TitleButton::Close) {
        (Some(HOT_CLOSE_BUTTON), HOT_CLOSE_MARK)
    } else if look.unresponsive {
        (Some(UNRESPONSIVE_CLOSE_BUTTON), HOT_CLOSE_MARK)
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
    if !look.maximized {
        cut_top_corners(&mut pixels, width);
    }
    pixels
}

/// 記号の左上(ボタンの左上から)。記号は、ボタンの中央に置く。
const MARK_LEFT: i32 = (TITLE_BUTTON_SIZE - MARK_SIZE) / 2;
const MARK_TOP: i32 = (TITLE_BAR_HEIGHT - MARK_SIZE) / 2;

/// ボタン `button` の記号の点(ボタンの左上からの位置)。フォントが無くても出るように、太さ 1 の線で描く。
fn mark_points(button: TitleButton, maximized: bool) -> Vec<(i32, i32)> {
    let (left, top) = (MARK_LEFT, MARK_TOP);
    let (right, bottom) = (left + MARK_SIZE - 1, top + MARK_SIZE - 1);
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
        // 「−」:記号の高さの中ほどの、横の線。
        (TitleButton::Minimize, _) => {
            for x in left..=right {
                points.push((x, top + MARK_SIZE / 2));
            }
        }
        // 「□」:四角。
        (TitleButton::Maximize, false) => outline((left, top), (right, bottom)),
        // 元に戻す:2 つの重なった四角。奥の四角は、手前の四角に隠れない所(上と右)だけ。
        (TitleButton::Maximize, true) => {
            outline((left, top + 3), (right - 3, bottom));
            points.extend((left + 3..=right).map(|x| (x, top)));
            points.extend((top..=bottom - 3).map(|y| (right, y)));
        }
        (TitleButton::Close, _) => {}
    }
    points
}

/// 左の端が `left` のボタンを描く。`ground` があれば地をその色で塗り、`points` に記号の色 `mark` の点を打つ。
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

/// タイトルバーの右の端に、閉じるボタンを描く。`ground` があれば、ボタンの地をその色で塗る。
///
/// 「×」は、フォントが無くても出るように、文字ではなく、縁をなめらかにした 2 本の斜めの線で描く。
fn paint_close_button(pixels: &mut [u8], width: i32, ground: Option<[u8; 3]>, mark: [u8; 3]) {
    let left = width - CLOSE_BUTTON_SIZE;
    if let Some(ground) = ground {
        for y in 0..TITLE_BAR_HEIGHT {
            for x in left.max(0)..width {
                let at = ((y * width + x) * 4) as usize;
                pixels[at..at + 3].copy_from_slice(&[ground[2], ground[1], ground[0]]);
            }
        }
    }
    let x0 = (left + (CLOSE_BUTTON_SIZE - CLOSE_MARK_SIZE) / 2) as f32;
    let y0 = ((TITLE_BAR_HEIGHT - CLOSE_MARK_SIZE) / 2) as f32;
    let size = CLOSE_MARK_SIZE as f32;
    // 2 本の対角線から、太さの半分(0.7)の中。
    let on_mark = |x: f32, y: f32| {
        let (u, v) = (x - x0, y - y0);
        if u < 0.0 || v < 0.0 || u > size || v > size {
            return false;
        }
        let half = 0.7 * std::f32::consts::SQRT_2;
        (u - v).abs() <= half || (u + v - size).abs() <= half
    };
    let bounds = (
        x0 as i32 - 1,
        y0 as i32 - 1,
        (x0 + size) as i32 + 1,
        (y0 + size) as i32 + 1,
    );
    fill_shape(pixels, width, bounds, mark, on_mark);
}

/// タイトルバーの絵(B, G, R, A。不透明)の、上の 2 つの角を、[`FRAME_RADIUS`] の丸みで欠く。縁は、1 ピクセルを
/// 分けて数えた割合の不透明度にし、色にも掛ける(premultiplied)。
fn cut_top_corners(pixels: &mut [u8], width: i32) {
    let outer = Rect::new(Point::new(0, 0), Size::new(width, TITLE_BAR_HEIGHT * 4));
    for y in 0..FRAME_RADIUS.min(TITLE_BAR_HEIGHT) {
        for x in (0..FRAME_RADIUS.min(width)).chain((width - FRAME_RADIUS).max(0)..width) {
            let mut inside = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = PointF::new(
                        f64::from(x) + (f64::from(sx) + 0.5) / 4.0,
                        f64::from(y) + (f64::from(sy) + 0.5) / 4.0,
                    );
                    if !in_cut_corner(outer, p) {
                        inside += 1;
                    }
                }
            }
            let at = ((y * width + x) * 4) as usize;
            let alpha = inside * 255 / 16;
            for c in &mut pixels[at..at + 3] {
                *c = ((u32::from(*c) * alpha + 127) / 255) as u8;
            }
            pixels[at + 3] = alpha as u8;
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

    /// 透明なアイコンで、タイトルバーの絵を作る。
    fn paint(
        painter: &mut TextPainter,
        text: &str,
        width: i32,
        active: bool,
        look: TitleLook,
    ) -> Vec<u8> {
        let icon = Icon::transparent(TITLE_ICON_SIZE as u32, TITLE_ICON_SIZE as u32);
        paint_title_bar(painter, text, width, active, look, &icon)
    }

    #[test]
    fn the_bar_shows_the_title_or_else_the_app_id() {
        assert_eq!(title_text(Some("zeyes"), Some("seinas.zeyes-min")), "zeyes");
        assert_eq!(
            title_text(None, Some("seinas.zeyes-min")),
            "seinas.zeyes-min"
        );
        // 空のタイトルは、無いものとして扱う。
        assert_eq!(
            title_text(Some(""), Some("seinas.zeyes-min")),
            "seinas.zeyes-min"
        );
        assert_eq!(title_text(Some("  "), None), "");
        assert_eq!(title_text(None, None), "");
    }

    #[test]
    fn the_content_sits_below_the_bar() {
        let bar = bar_height(true);
        assert_eq!(outer_size((320, 240).into(), bar), (320, 276).into());
        assert_eq!(content_location((32, 32).into(), bar), (32, 68).into());
        assert_eq!(content_size_for((640, 480).into(), bar), (640, 444).into());
        // Seinas が飾りを描かないウィンドウは、外枠とコンテンツが同じ。
        let none = bar_height(false);
        assert_eq!(none, 0);
        assert_eq!(outer_size((320, 240).into(), none), (320, 240).into());
        assert_eq!(content_location((32, 32).into(), none), (32, 32).into());
        assert_eq!(content_size_for((640, 480).into(), none), (640, 480).into());
    }

    #[test]
    fn without_fonts_the_bar_is_painted_without_a_title() {
        let mut painter = TextPainter::without_fonts();
        for (active, color) in [(true, ACTIVE_BAR), (false, INACTIVE_BAR)] {
            let pixels = paint(&mut painter, "zeyes", 100, active, TitleLook::default());
            assert_eq!(pixels.len(), 100 * TITLE_BAR_HEIGHT as usize * 4);
            // 閉じるボタンの手前は、角を欠いた所の下では、タイトルバーの色だけ。
            assert!((FRAME_RADIUS..TITLE_BAR_HEIGHT)
                .all(|y| (0..100 - CLOSE_BUTTON_SIZE).all(|x| pixel(&pixels, 100, x, y) == color)));
        }
    }

    /// タイトルバーの絵の(x, y)の色(赤, 緑, 青)。
    fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 3] {
        let at = ((y * width + x) * 4) as usize;
        [pixels[at + 2], pixels[at + 1], pixels[at]]
    }

    /// タイトルバーの絵の(x, y)の不透明度。
    fn alpha(pixels: &[u8], width: i32, x: i32, y: i32) -> u8 {
        pixels[((y * width + x) * 4 + 3) as usize]
    }

    #[test]
    fn the_icon_sits_at_the_left_and_the_top_corners_are_cut() {
        let mut painter = TextPainter::without_fonts();
        let side = TITLE_ICON_SIZE as u32;
        let icon = Icon {
            width: side,
            height: side,
            rgba: [0x10, 0x80, 0xf0, 0xff].repeat((side * side) as usize),
        };
        let width = 200;
        let pixels = paint_title_bar(&mut painter, "", width, true, TitleLook::default(), &icon);
        // アイコンは、左から 10、縦は中央(36 の中の 8〜28)。
        let top = (TITLE_BAR_HEIGHT - TITLE_ICON_SIZE) / 2;
        assert_eq!(top, 8);
        assert_eq!(
            pixel(&pixels, width, TITLE_PADDING, top),
            [0x10, 0x80, 0xf0]
        );
        assert_eq!(
            pixel(
                &pixels,
                width,
                TITLE_PADDING + TITLE_ICON_SIZE - 1,
                top + TITLE_ICON_SIZE - 1
            ),
            [0x10, 0x80, 0xf0]
        );
        assert_eq!(pixel(&pixels, width, TITLE_PADDING - 1, top), ACTIVE_BAR);
        assert_eq!(pixel(&pixels, width, TITLE_PADDING, top - 1), ACTIVE_BAR);
        assert_eq!(TITLE_TEXT_X, 38);
        // 上の 2 つの角は透明で、丸みの内側は不透明。下の角は欠かない。
        assert_eq!(alpha(&pixels, width, 0, 0), 0);
        assert_eq!(alpha(&pixels, width, width - 1, 0), 0);
        assert_eq!(alpha(&pixels, width, FRAME_RADIUS, 0), 255);
        assert_eq!(alpha(&pixels, width, 0, FRAME_RADIUS), 255);
        assert_eq!(alpha(&pixels, width, 3, 3), 255);
        assert_eq!(alpha(&pixels, width, 0, TITLE_BAR_HEIGHT - 1), 255);
        // いちばん上の行の丸みの縁(x 4。丸みの外の線が 4.4 を通る)は半ばの不透明度で、色にも不透明度を掛けてある。
        let edge = alpha(&pixels, width, 4, 0);
        assert!(0 < edge && edge < 255, "{edge}");
        assert!(pixel(&pixels, width, 4, 0)[2] < ACTIVE_BAR[2]);
        // 最大化しているときは、角を欠かない。
        let maximized = paint_title_bar(
            &mut painter,
            "",
            width,
            true,
            TitleLook {
                maximized: true,
                ..TitleLook::default()
            },
            &icon,
        );
        assert_eq!(alpha(&maximized, width, 0, 0), 255);
        assert_eq!(pixel(&maximized, width, 0, 0), ACTIVE_BAR);
    }

    #[test]
    fn the_cut_corners_are_outside_the_frame() {
        let outer = Rect::new((100, 50).into(), (200, 100).into());
        let at = |x: f64, y: f64| PointF::new(x, y);
        // 4 つの角の、いちばん外のピクセルは、欠いた所。
        for (x, y) in [(100.5, 50.5), (299.5, 50.5), (100.5, 149.5), (299.5, 149.5)] {
            assert!(in_cut_corner(outer, at(x, y)), "({x}, {y})");
            assert!(!frame_contains(outer, true, at(x, y)), "({x}, {y})");
            assert!(frame_contains(outer, false, at(x, y)), "({x}, {y})");
        }
        // 丸みの内側と、辺の上と、中は、欠かない。
        for (x, y) in [
            (103.0, 53.0),
            (100.5, 57.5),
            (107.5, 50.5),
            (200.0, 50.5),
            (200.0, 100.0),
        ] {
            assert!(!in_cut_corner(outer, at(x, y)), "({x}, {y})");
            assert!(frame_contains(outer, true, at(x, y)), "({x}, {y})");
        }
        // 外枠の外は、どちらでも含まない。
        assert!(!frame_contains(outer, false, at(99.5, 60.0)));
        assert!(!in_cut_corner(outer, at(99.5, 50.5)));
    }

    #[test]
    fn the_close_button_is_drawn_at_the_right_end_without_fonts() {
        let mut painter = TextPainter::without_fonts();
        let width = 100;
        let left = width - CLOSE_BUTTON_SIZE;
        // 「×」は、ボタン(44x36)の中央の 16x16(左上はボタンの左上から (14, 10))。
        let (x0, y0) = (left + 14, 10);

        // ふだん: 地はタイトルバーの色のままで、中央に、タイトルと同じ色の「×」がある。
        let pixels = paint(&mut painter, "", width, true, TitleLook::default());
        assert_eq!(pixel(&pixels, width, left, 0), ACTIVE_BAR);
        assert_eq!(
            pixel(&pixels, width, width - 1, TITLE_BAR_HEIGHT - 1),
            ACTIVE_BAR
        );
        // 「×」の中心と、4 つの端。
        for (x, y) in [(7, 7), (8, 8), (0, 0), (15, 0), (0, 15), (15, 15)] {
            assert_eq!(
                pixel(&pixels, width, x0 + x, y0 + y),
                ACTIVE_TITLE,
                "({x}, {y})"
            );
        }
        // 「×」の上下左右の真ん中は、空いている。
        for (x, y) in [(7, 0), (8, 15), (0, 7), (15, 8)] {
            assert_eq!(
                pixel(&pixels, width, x0 + x, y0 + y),
                ACTIVE_BAR,
                "({x}, {y})"
            );
        }
        let inactive = paint(&mut painter, "", width, false, TitleLook::default());
        assert_eq!(pixel(&inactive, width, x0 + 7, y0 + 7), INACTIVE_TITLE);

        // ポインターを乗せたとき: 地が赤、印が白になる。ボタンの外は変わらない。
        let hot = paint(&mut painter, "", width, true, close_hot());
        assert_eq!(pixel(&hot, width, left, 0), HOT_CLOSE_BUTTON);
        assert_eq!(
            pixel(&hot, width, width - 1, TITLE_BAR_HEIGHT - 1),
            HOT_CLOSE_BUTTON
        );
        assert_eq!(pixel(&hot, width, x0 + 7, y0 + 7), HOT_CLOSE_MARK);
        assert_eq!(pixel(&hot, width, left - 1, 12), ACTIVE_BAR);

        // ボタンより狭いウィンドウでも、落ちない。
        paint(&mut painter, "", 10, true, close_hot());
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
        // 上: タイトルバーの上の端は、画面の上の端より上へ行かない。
        assert_eq!(clamp(100, -30), (100, 0));
        // 下: タイトルバーの全体が残る(コンテンツは、画面の下へ出てよい)。
        assert_eq!(clamp(100, 444), (100, 444));
        assert_eq!(clamp(100, 470), (100, 444));
        // 左へ: タイトルバーの右の 48 + 132(ボタン 3 つ)= 180 ピクセルが残る。右へ: 左の 48 ピクセルが残る。
        assert_eq!(clamp(-140, 50), (-140, 50));
        assert_eq!(clamp(-300, 50), (-140, 50));
        assert_eq!(clamp(592, 50), (592, 50));
        assert_eq!(clamp(700, 50), (592, 50));
        // 隅でも、両方の限りが働く。
        assert_eq!(clamp(-1000, -1000), (-140, 0));
        assert_eq!(clamp(1000, 1000), (592, 444));
        // タイトルバーが48ピクセルより短いウィンドウは、タイトルバーの全体が残る。
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
        assert_eq!(all.width(), 132);
        // 幅 200:閉じるは 156〜200、最大化は 112〜156、最小化は 68〜112。
        assert_eq!(all.at(200, 199.9), Some(TitleButton::Close));
        assert_eq!(all.at(200, 156.0), Some(TitleButton::Close));
        assert_eq!(all.at(200, 155.9), Some(TitleButton::Maximize));
        assert_eq!(all.at(200, 112.0), Some(TitleButton::Maximize));
        assert_eq!(all.at(200, 100.0), Some(TitleButton::Minimize));
        assert_eq!(all.at(200, 67.9), None);
        assert_eq!(all.at(200, 200.0), None);
        assert_eq!(all.left_of(200, TitleButton::Minimize), Some(68));
        // 最大化のボタンが無ければ、最小化のボタンが閉じるボタンの隣に来る。
        let no_maximize = TitleButtons {
            minimize: true,
            maximize: false,
        };
        assert_eq!(no_maximize.at(200, 150.0), Some(TitleButton::Minimize));
        assert_eq!(no_maximize.left_of(200, TitleButton::Maximize), None);
        assert_eq!(TitleButtons::CLOSE_ONLY.at(200, 150.0), None);
    }

    #[test]
    fn a_window_that_does_not_respond_has_a_dark_red_close_button() {
        let mut painter = TextPainter::without_fonts();
        let look = TitleLook {
            buttons: TitleButtons::CLOSE_ONLY,
            unresponsive: true,
            ..TitleLook::default()
        };
        // 閉じるボタン(156〜200)の地は暗い赤、印は白。ポインターを乗せると、ふつうの赤。
        let pixels = paint(&mut painter, "", 200, false, look);
        assert_eq!(pixel(&pixels, 200, 156, 0), UNRESPONSIVE_CLOSE_BUTTON);
        assert_eq!(pixel(&pixels, 200, 170, 10), HOT_CLOSE_MARK);
        assert_eq!(pixel(&pixels, 200, 155, 0), INACTIVE_BAR);
        let hot = TitleLook {
            hot: Some(TitleButton::Close),
            ..look
        };
        let pixels = paint(&mut painter, "", 200, false, hot);
        assert_eq!(pixel(&pixels, 200, 156, 0), HOT_CLOSE_BUTTON);
    }

    #[test]
    fn the_buttons_are_drawn_without_fonts() {
        let mut painter = TextPainter::without_fonts();
        let look = TitleLook {
            buttons: TitleButtons::ALL,
            maximized: false,
            hot: Some(TitleButton::Minimize),
            unresponsive: false,
        };
        let pixels = paint(&mut painter, "", 200, true, look);
        // 最小化(68〜112)の地は、ポインターが乗っている色。「−」は y 18、x 82〜97。
        assert_eq!(pixel(&pixels, 200, 68, 0), HOT_ACTIVE_BUTTON);
        assert_eq!(pixel(&pixels, 200, 82, 18), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 97, 18), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 82, 19), HOT_ACTIVE_BUTTON);
        assert_eq!(pixel(&pixels, 200, 82, 17), HOT_ACTIVE_BUTTON);
        // 最大化(112〜156)の「□」は、x 126〜141、y 10〜25 の枠。
        assert_eq!(pixel(&pixels, 200, 112, 0), ACTIVE_BAR);
        assert_eq!(pixel(&pixels, 200, 126, 10), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 141, 25), ACTIVE_TITLE);
        assert_eq!(pixel(&pixels, 200, 133, 17), ACTIVE_BAR);
        // 最大化しているときは、元に戻すの印(手前の四角は x 126〜138、y 13〜25。奥の四角の上の辺は y 10)。
        let maximized = paint(
            &mut painter,
            "",
            200,
            true,
            TitleLook {
                maximized: true,
                ..look
            },
        );
        assert_eq!(pixel(&maximized, 200, 126, 13), ACTIVE_TITLE);
        assert_eq!(pixel(&maximized, 200, 138, 25), ACTIVE_TITLE);
        assert_eq!(pixel(&maximized, 200, 141, 10), ACTIVE_TITLE);
        assert_eq!(pixel(&maximized, 200, 126, 10), ACTIVE_BAR);
    }
}
