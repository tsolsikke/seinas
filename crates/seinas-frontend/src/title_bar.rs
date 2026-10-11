//! タイトルバーの絵を、描画の要素(Smithay のメモリーのイメージ)にする所。
//!
//! タイトルバーの絵は、タイトル・幅・activated かどうか・ボタンの見た目が変わったときにだけ作り直し、
//! それまでは覚えておいたものを使う。絵そのものは `decoration.rs` の `paint_title_bar` が作る。

use seinas_text::TextPainter;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::element::{
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            Kind,
        },
        renderer::pixman::PixmanRenderer,
    },
    utils::Transform,
};
use tracing::warn;

use crate::{
    decoration::{paint_title_bar, TitleLook, TITLE_BAR_HEIGHT},
    geometry::Point,
};

/// 1つのウィンドウの帯の絵と、それを作ったときの条件。
pub struct TitleBar {
    text: String,
    width: i32,
    active: bool,
    look: TitleLook,
    buffer: MemoryRenderBuffer,
}

impl TitleBar {
    /// タイトルバーの絵を作る。`look` は、ボタンの見た目。
    pub fn new(
        painter: &mut TextPainter,
        text: &str,
        width: i32,
        active: bool,
        look: TitleLook,
    ) -> Self {
        let pixels = paint_title_bar(painter, text, width, active, look);
        TitleBar {
            text: text.to_owned(),
            width,
            active,
            look,
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
    pub fn matches(&self, text: &str, width: i32, active: bool, look: TitleLook) -> bool {
        self.text == text && self.width == width && self.active == active && self.look == look
    }

    /// 左上を `location` に置いた、描画の要素。
    pub fn element(
        &self,
        renderer: &mut PixmanRenderer,
        location: Point,
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
