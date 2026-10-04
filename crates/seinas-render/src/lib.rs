//! Seinasの共通の描画。
//!
//! 画面1枚ぶんの絵を、pixmanで自分のメモリーの上に作る。どこへ表示するか(親のWayland、fbdevなど)は
//! 知らない。Waylandの受け口(ソケット、wl_shmなど)にも依存しない。
//!
//! 使い方は3つの段に分かれる。
//!
//! 1. 呼ぶ側が、描きたい要素([`RenderElement`])を手前から順に並べる。
//! 2. [`Painter::paint`] が、背景と要素を合成して [`FrameView`] を返す。
//! 3. 呼ぶ側(裏側)が、[`FrameView`] の中身を自分の表示先へ写す。
//!
//! 境界の決まりは `docs/render-boundary.md` にまとめてある。

use std::fmt;

use smithay::{
    backend::renderer::{
        element::RenderElement,
        pixman::{PixmanError, PixmanRenderer},
        Bind, Color32F, Frame, Renderer,
    },
    utils::{Physical, Rectangle, Scale, Size, Transform},
};

/// 何も無い所の色。
pub const BACKGROUND: Color32F = Color32F::new(0.10, 0.12, 0.16, 1.0);

/// 描画結果の画素の形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 1画素は32ビット。メモリー上の並びは B, G, R, X(リトルエンディアンの `0xXXRRGGBB`)。
    /// Xの8ビットは意味を持たない。wl_shmの `xrgb8888`、pixmanの `x8r8g8b8` と同じ。
    Xrgb8888,
}

impl PixelFormat {
    /// 1画素のバイト数。
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Xrgb8888 => 4,
        }
    }
}

/// 描画の失敗。
#[derive(Debug)]
pub enum RenderError {
    /// 幅か高さが0以下、または大きすぎる。
    InvalidSize { width: i32, height: i32 },
    /// 描画先の画像を作れなかった(メモリー不足など)。
    CanvasAllocation,
    /// pixmanでの描画に失敗した。
    Pixman(PixmanError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::InvalidSize { width, height } => {
                write!(f, "invalid canvas size {width}x{height}")
            }
            RenderError::CanvasAllocation => write!(f, "failed to allocate the canvas"),
            RenderError::Pixman(e) => write!(f, "pixman rendering failed: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<PixmanError> for RenderError {
    fn from(e: PixmanError) -> Self {
        RenderError::Pixman(e)
    }
}

/// 画面1枚ぶんの絵を作るもの。描画先の画像(キャンバス)を1つ持ち、毎回そこへ描き直す。
pub struct Painter {
    renderer: PixmanRenderer,
    canvas: pixman::Image<'static, 'static>,
    width: i32,
    height: i32,
}

impl Painter {
    /// `width` × `height` 画素のキャンバスを持つ `Painter` を作る。
    pub fn new(width: i32, height: i32) -> Result<Self, RenderError> {
        let invalid = RenderError::InvalidSize { width, height };
        if width <= 0 || height <= 0 {
            return Err(invalid);
        }
        // バイト数がi32に収まる大きさに限る(wl_shmなど、表示先の多くがi32で大きさを表すため)。
        let bytes = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(PixelFormat::Xrgb8888.bytes_per_pixel() as i32));
        if bytes.is_none() {
            return Err(invalid);
        }
        let renderer = PixmanRenderer::new()?;
        let canvas = pixman::Image::new(
            pixman::FormatCode::X8R8G8B8,
            width as usize,
            height as usize,
            true,
        )
        .map_err(|_| RenderError::CanvasAllocation)?;
        Ok(Painter {
            renderer,
            canvas,
            width,
            height,
        })
    }

    /// キャンバスの幅(画素)。
    pub fn width(&self) -> i32 {
        self.width
    }

    /// キャンバスの高さ(画素)。
    pub fn height(&self) -> i32 {
        self.height
    }

    /// 要素を作るときに使うレンダラ。
    ///
    /// クライアントのバッファを取り込む要素などは、作るときにレンダラを求める。
    /// そのために貸し出すだけで、描画そのものは [`Painter::paint`] で行う。
    pub fn renderer(&mut self) -> &mut PixmanRenderer {
        &mut self.renderer
    }

    /// 背景を塗り、その上に `elements` を合成する。
    ///
    /// `elements` は手前から順に並べる(先頭がいちばん手前)。毎回、全面を描き直す。
    /// 返す [`FrameView`] は、次に `paint` を呼ぶまでの間だけ読める。
    pub fn paint<E>(&mut self, elements: &[E]) -> Result<FrameView<'_>, RenderError>
    where
        E: RenderElement<PixmanRenderer>,
    {
        let size = Size::<i32, Physical>::from((self.width, self.height));
        let whole = Rectangle::from_size(size);
        let scale = Scale::from(1.0);
        {
            let mut target = self.renderer.bind(&mut self.canvas)?;
            let mut frame = self.renderer.render(&mut target, size, Transform::Normal)?;
            frame.clear(BACKGROUND, &[whole])?;
            // 奥から手前へ重ねる。
            for element in elements.iter().rev() {
                let geometry = element.geometry(scale);
                let Some(mut visible) = whole.intersection(geometry) else {
                    continue;
                };
                // 描き直す範囲は、要素の左上を原点にして渡す。
                visible.loc -= geometry.loc;
                element.draw(&mut frame, element.src(), geometry, &[visible], &[])?;
            }
            // pixmanの描画は同期で終わるので、返ってくる同期点を待つ必要は無い。
            let _ = frame.finish()?;
        }
        Ok(self.view())
    }

    fn view(&self) -> FrameView<'_> {
        let stride = self.canvas.stride();
        // SAFETY: `data()` は、このPainterが持つ生きた画像の先頭を指す。画像は `stride × 高さ` バイトの
        // 続いた領域で、pixmanが確保して画像と同じ間だけ保つ。返すスライスは `&self` を借りているので、
        // その間はPainterを通じた書き込み(`paint` は `&mut self`)が起きない。
        let data = unsafe {
            std::slice::from_raw_parts(
                self.canvas.data() as *const u8,
                stride * self.height as usize,
            )
        };
        FrameView {
            width: self.width as usize,
            height: self.height as usize,
            stride,
            format: PixelFormat::Xrgb8888,
            data,
        }
    }
}

/// 描画結果。[`Painter`] のキャンバスを読むだけの窓で、画素の持ち主は `Painter` のまま。
#[derive(Clone, Copy)]
pub struct FrameView<'a> {
    width: usize,
    height: usize,
    stride: usize,
    format: PixelFormat,
    data: &'a [u8],
}

impl<'a> FrameView<'a> {
    /// 幅(画素)。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 高さ(画素)。
    pub fn height(&self) -> usize {
        self.height
    }

    /// 1行のバイト数。`幅 × 4` 以上で、ちょうどとは限らない。
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// 画素の形式。
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// 1行ぶんの、意味のあるバイト(`幅 × 4`)。
    pub fn row_bytes(&self) -> usize {
        self.width * self.format.bytes_per_pixel()
    }

    /// 上から `y` 行目の画素(`幅 × 4` バイト)。
    ///
    /// # Panics
    ///
    /// `y` が高さ以上のとき。
    pub fn row(&self, y: usize) -> &'a [u8] {
        assert!(
            y < self.height,
            "row {y} is out of range (height {})",
            self.height
        );
        &self.data[y * self.stride..y * self.stride + self.row_bytes()]
    }

    /// 全体を、行の幅が `dst_stride` バイトの `dst` へ写す。行の余り(`幅 × 4` より後ろ)には触れない。
    ///
    /// # Panics
    ///
    /// `dst_stride` が `幅 × 4` より小さいとき、`dst` が足りないとき。
    pub fn copy_to(&self, dst: &mut [u8], dst_stride: usize) {
        let row_bytes = self.row_bytes();
        assert!(
            dst_stride >= row_bytes,
            "the destination stride is too small"
        );
        assert!(
            dst.len() >= dst_stride * (self.height - 1) + row_bytes,
            "the destination is too small"
        );
        for y in 0..self.height {
            dst[y * dst_stride..y * dst_stride + row_bytes].copy_from_slice(self.row(y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::element::{solid::SolidColorRenderElement, Id, Kind};

    /// [`BACKGROUND`] を8ビットに直した値(B, G, R, X)。pixmanは端数を切り捨てる。
    const BACKGROUND_PIXEL: [u8; 4] = [0x28, 0x1e, 0x19, 0xff];

    fn solid(x: i32, y: i32, w: i32, h: i32, color: [f32; 4]) -> SolidColorRenderElement {
        SolidColorRenderElement::new(
            Id::new(),
            Rectangle::new((x, y).into(), (w, h).into()),
            0usize,
            Color32F::from(color),
            Kind::Unspecified,
        )
    }

    fn pixel(view: &FrameView<'_>, x: usize, y: usize) -> [u8; 4] {
        view.row(y)[x * 4..x * 4 + 4].try_into().unwrap()
    }

    /// Xの8ビットは意味を持たないので、比べるときは落とす。
    fn rgb(p: [u8; 4]) -> [u8; 3] {
        [p[2], p[1], p[0]]
    }

    #[test]
    fn empty_scene_is_all_background() {
        let mut painter = Painter::new(64, 48).unwrap();
        let view = painter.paint::<SolidColorRenderElement>(&[]).unwrap();
        assert_eq!((view.width(), view.height()), (64, 48));
        assert_eq!(view.format(), PixelFormat::Xrgb8888);
        assert!(view.stride() >= 64 * 4);
        for y in 0..48 {
            for x in 0..64 {
                assert_eq!(
                    rgb(pixel(&view, x, y)),
                    rgb(BACKGROUND_PIXEL),
                    "at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn elements_are_composited_front_to_back() {
        let mut painter = Painter::new(32, 32).unwrap();
        // 先頭が手前。赤(手前)が緑(奥)の一部を隠す。
        let elements = [
            solid(8, 8, 8, 8, [1.0, 0.0, 0.0, 1.0]),
            solid(4, 4, 16, 16, [0.0, 1.0, 0.0, 1.0]),
        ];
        let view = painter.paint(&elements).unwrap();
        assert_eq!(rgb(pixel(&view, 10, 10)), [0xff, 0x00, 0x00]);
        assert_eq!(rgb(pixel(&view, 5, 5)), [0x00, 0xff, 0x00]);
        assert_eq!(rgb(pixel(&view, 19, 19)), [0x00, 0xff, 0x00]);
        assert_eq!(rgb(pixel(&view, 20, 20)), rgb(BACKGROUND_PIXEL));
        assert_eq!(rgb(pixel(&view, 0, 0)), rgb(BACKGROUND_PIXEL));
    }

    #[test]
    fn elements_outside_the_canvas_are_clipped() {
        let mut painter = Painter::new(16, 16).unwrap();
        let elements = [
            solid(-4, -4, 8, 8, [0.0, 0.0, 1.0, 1.0]),
            solid(100, 100, 8, 8, [1.0, 1.0, 1.0, 1.0]),
        ];
        let view = painter.paint(&elements).unwrap();
        assert_eq!(rgb(pixel(&view, 3, 3)), [0x00, 0x00, 0xff]);
        assert_eq!(rgb(pixel(&view, 4, 4)), rgb(BACKGROUND_PIXEL));
        assert_eq!(rgb(pixel(&view, 15, 15)), rgb(BACKGROUND_PIXEL));
    }

    #[test]
    fn every_paint_starts_from_the_background() {
        let mut painter = Painter::new(16, 16).unwrap();
        painter
            .paint(&[solid(0, 0, 16, 16, [1.0, 1.0, 1.0, 1.0])])
            .unwrap();
        let view = painter.paint::<SolidColorRenderElement>(&[]).unwrap();
        assert_eq!(rgb(pixel(&view, 8, 8)), rgb(BACKGROUND_PIXEL));
    }

    #[test]
    fn copy_to_respects_the_destination_stride() {
        let mut painter = Painter::new(4, 3).unwrap();
        let view = painter
            .paint(&[solid(0, 0, 4, 3, [1.0, 1.0, 1.0, 1.0])])
            .unwrap();
        let dst_stride = 4 * 4 + 8;
        let mut dst = vec![0x55u8; dst_stride * 3];
        view.copy_to(&mut dst, dst_stride);
        for y in 0..3 {
            let row = &dst[y * dst_stride..(y + 1) * dst_stride];
            for x in 0..4 {
                assert_eq!(row[x * 4..x * 4 + 3], [0xff, 0xff, 0xff]);
            }
            assert!(
                row[16..].iter().all(|&b| b == 0x55),
                "the padding must stay untouched"
            );
        }
    }

    #[test]
    fn invalid_sizes_are_rejected() {
        assert!(matches!(
            Painter::new(0, 10),
            Err(RenderError::InvalidSize { .. })
        ));
        assert!(matches!(
            Painter::new(10, -1),
            Err(RenderError::InvalidSize { .. })
        ));
        assert!(matches!(
            Painter::new(i32::MAX, i32::MAX),
            Err(RenderError::InvalidSize { .. })
        ));
    }
}
