//! ウィンドウの飾り。今は、上に付ける題名の帯(タイトルバー)だけ。
//!
//! 帯は、Seinasの側で描く(クライアントには、xdg-decorationで「サーバーの側で飾りを描く」と伝える)。
//! 帯の絵は、題名・幅・選ばれているかが変わったときにだけ作り直し、それまでは覚えておいたものを使う。
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

// 色は (赤, 緑, 青)。
/// 選ばれているウィンドウの、帯と題名の色。
pub const ACTIVE_BAR: [u8; 3] = [0x2f, 0x6f, 0xb5];
pub const ACTIVE_TITLE: [u8; 3] = [0xff, 0xff, 0xff];
/// 選ばれていないウィンドウの、帯と題名の色。
pub const INACTIVE_BAR: [u8; 3] = [0x4a, 0x50, 0x5c];
pub const INACTIVE_TITLE: [u8; 3] = [0xc8, 0xcc, 0xd4];

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
    buffer: MemoryRenderBuffer,
}

impl TitleBar {
    /// 帯の絵を作る。
    pub fn new(painter: &mut TextPainter, text: &str, width: i32, active: bool) -> Self {
        let pixels = paint_title_bar(painter, text, width, active);
        TitleBar {
            text: text.to_owned(),
            width,
            active,
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
    pub fn matches(&self, text: &str, width: i32, active: bool) -> bool {
        self.text == text && self.width == width && self.active == active
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
/// 題名は左に寄せる。帯に収まらなければ、末尾を切って「…」を付ける。
pub fn paint_title_bar(painter: &mut TextPainter, text: &str, width: i32, active: bool) -> Vec<u8> {
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
            max_width: width - TITLE_PADDING * 2,
        },
    );
    pixels
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
            let pixels = paint_title_bar(&mut painter, "zeyes", 100, active);
            assert_eq!(pixels.len(), 100 * TITLE_BAR_HEIGHT as usize * 4);
            assert!(pixels
                .chunks(4)
                .all(|pixel| pixel[..3] == [color[2], color[1], color[0]]));
        }
    }
}
