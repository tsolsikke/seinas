//! ウィンドウの飾り。上に付ける題名の帯(タイトルバー)と、その右の端の閉じるボタン。
//!
//! 帯は、Seinasの側で描く(クライアントには、xdg-decorationで「サーバーの側で飾りを描く」と伝える)。
//! 帯の絵は、題名・幅・選ばれているか・閉じるボタンの見た目が変わったときにだけ作り直し、それまでは
//! 覚えておいたものを使う。
//! 決まりの説明は `docs/window-placement.md` にある。

use std::path::Path;

use seinas_text::{Canvas, FontFiles, Line, TextPainter, DEFAULT_SIZE};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::element::{
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            Kind,
        },
        renderer::pixman::PixmanRenderer,
    },
    utils::{Logical, Point, Size, Transform},
};
use tracing::{info, warn};

/// 題名の帯の高さ(画素)。
pub const TITLE_BAR_HEIGHT: i32 = 24;
/// 題名の文字の大きさ(画素)。
pub const TITLE_SIZE: f32 = DEFAULT_SIZE;
/// 帯の左右の端から、題名までの空き(画素)。
pub const TITLE_PADDING: i32 = 8;
/// 閉じるボタンの大きさ(画素)。帯の右の端に、帯と同じ高さの正方形で置く。
pub const CLOSE_BUTTON_SIZE: i32 = TITLE_BAR_HEIGHT;
/// 閉じるボタンの「×」の印の大きさ(画素)。ボタンの中央に描く。
const CLOSE_MARK_SIZE: i32 = 10;
/// ウィンドウを動かしても、帯のうち、横にこれだけ(画素)は画面の中に残す。帯をもう一度掴めるようにする。
pub const MIN_VISIBLE_BAR: i32 = 48;

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

/// 帯を含めた、ウィンドウの外形の大きさ。`content` は、クライアントが描く中身の大きさ。
pub fn outer_size(content: Size<i32, Logical>) -> Size<i32, Logical> {
    (content.w, content.h + TITLE_BAR_HEIGHT).into()
}

/// 外形の左上が `outer` にあるウィンドウの、中身の左上。中身は、帯の下に置く。
pub fn content_location(outer: Point<i32, Logical>) -> Point<i32, Logical> {
    (outer.x, outer.y + TITLE_BAR_HEIGHT).into()
}

/// 画面の大きさが `screen` のとき、クライアントに伝える中身の大きさ。帯のぶんだけ低い。
pub fn content_size_for(screen: Size<i32, Logical>) -> Size<i32, Logical> {
    (screen.w, (screen.h - TITLE_BAR_HEIGHT).max(1)).into()
}

/// 外形の幅が `width` のウィンドウを、左上が `wanted` に来るように動かしたいとき、実際に置く位置。
///
/// 帯をもう一度掴めるように、画面の外へ出せる範囲を限る。
/// - 上: 帯の上の端は、画面の上の端より上へ行かない。
/// - 下: 帯の全体が、画面の中に残る。
/// - 左右: 帯のうち、少なくとも [`MIN_VISIBLE_BAR`] 画素(帯がそれより短ければ、帯の全体)が、画面の中に残る。
pub fn clamp_outer_location(
    wanted: Point<i32, Logical>,
    width: i32,
    screen: Size<i32, Logical>,
) -> Point<i32, Logical> {
    let keep = MIN_VISIBLE_BAR.min(width);
    let x = wanted.x.min(screen.w - keep).max(keep - width);
    let y = wanted.y.min(screen.h - TITLE_BAR_HEIGHT).max(0);
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

/// `dir` の下のフォントを読む。読めないフォントがあっても止まらず、原因をログに出す。
///
/// 1つも読めなければ、題名の文字は出ない(帯だけを描く)。
pub fn load_fonts(dir: &Path) -> TextPainter {
    let (painter, problems) = TextPainter::load(&FontFiles::in_dir(dir));
    for problem in &problems {
        warn!("{problem}");
    }
    if !painter.can_draw() {
        warn!(
            "no font could be loaded from {}; window titles will not be shown",
            dir.display()
        );
    } else if problems.is_empty() {
        info!("fonts loaded from {}", dir.display());
    }
    painter
}

/// 1つのウィンドウの帯の絵と、それを作ったときの条件。
pub struct TitleBar {
    text: String,
    width: i32,
    active: bool,
    close_hot: bool,
    buffer: MemoryRenderBuffer,
}

impl TitleBar {
    /// 帯の絵を作る。`close_hot` は、閉じるボタンを目立たせて描くか。
    pub fn new(
        painter: &mut TextPainter,
        text: &str,
        width: i32,
        active: bool,
        close_hot: bool,
    ) -> Self {
        let pixels = paint_title_bar(painter, text, width, active, close_hot);
        TitleBar {
            text: text.to_owned(),
            width,
            active,
            close_hot,
            buffer: MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Xrgb8888,
                (width, TITLE_BAR_HEIGHT),
                1,
                Transform::Normal,
                None,
            ),
        }
    }

    /// 同じ条件で作った絵か(作り直さなくてよいか)。
    pub fn matches(&self, text: &str, width: i32, active: bool, close_hot: bool) -> bool {
        self.text == text
            && self.width == width
            && self.active == active
            && self.close_hot == close_hot
    }

    /// 左上を `location` に置いた、描画の要素。
    pub fn element(
        &self,
        renderer: &mut PixmanRenderer,
        location: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<PixmanRenderer>> {
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            // 拡大率は1なので、画面の上の位置と画素の位置は同じ。
            (location.x as f64, location.y as f64),
            &self.buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .map_err(|e| warn!("failed to prepare a title bar: {e}"))
        .ok()
    }
}

/// 帯の絵を作る。幅は `width`、高さは [`TITLE_BAR_HEIGHT`]。画素の並びは B, G, R, X。
///
/// 題名は左に寄せる。右の端には閉じるボタンを置き、題名は、その手前に収める。収まらなければ、
/// 末尾を切って「…」を付ける。`close_hot` なら、閉じるボタンを目立たせて描く。
pub fn paint_title_bar(
    painter: &mut TextPainter,
    text: &str,
    width: i32,
    active: bool,
    close_hot: bool,
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
            max_width: width - CLOSE_BUTTON_SIZE - TITLE_PADDING * 2,
        },
    );
    let (ground, mark) = if close_hot {
        (Some(HOT_CLOSE_BUTTON), HOT_CLOSE_MARK)
    } else {
        (None, title)
    };
    paint_close_button(&mut pixels, width, ground, mark);
    pixels
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
            let pixels = paint_title_bar(&mut painter, "zeyes", 100, active, false);
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
        let pixels = paint_title_bar(&mut painter, "", width, true, false);
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
        let inactive = paint_title_bar(&mut painter, "", width, false, false);
        assert_eq!(pixel(&inactive, width, left + 12, 12), INACTIVE_TITLE);

        // ポインターを乗せたとき: 地が赤、印が白になる。ボタンの外は変わらない。
        let hot = paint_title_bar(&mut painter, "", width, true, true);
        assert_eq!(pixel(&hot, width, left, 0), HOT_CLOSE_BUTTON);
        assert_eq!(
            pixel(&hot, width, width - 1, TITLE_BAR_HEIGHT - 1),
            HOT_CLOSE_BUTTON
        );
        assert_eq!(pixel(&hot, width, left + 12, 12), HOT_CLOSE_MARK);
        assert_eq!(pixel(&hot, width, left - 1, 12), ACTIVE_BAR);

        // ボタンより狭いウィンドウでも、落ちない。
        paint_title_bar(&mut painter, "", 10, true, true);
    }

    #[test]
    fn a_moved_window_keeps_its_bar_reachable() {
        let screen: Size<i32, Logical> = (640, 480).into();
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
        // 左右: 帯のうち48画素が残る。
        assert_eq!(clamp(-272, 50), (-272, 50));
        assert_eq!(clamp(-300, 50), (-272, 50));
        assert_eq!(clamp(592, 50), (592, 50));
        assert_eq!(clamp(700, 50), (592, 50));
        // 隅でも、両方の限りが働く。
        assert_eq!(clamp(-1000, -1000), (-272, 0));
        assert_eq!(clamp(1000, 1000), (592, 456));
        // 帯が48画素より短いウィンドウは、帯の全体が残る。
        let narrow = clamp_outer_location((-100, 10).into(), 30, screen);
        assert_eq!((narrow.x, narrow.y), (0, 10));
        let narrow = clamp_outer_location((700, 10).into(), 30, screen);
        assert_eq!((narrow.x, narrow.y), (610, 10));
    }
}
