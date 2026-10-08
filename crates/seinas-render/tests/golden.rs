//! 描画の合成の出力を、正解データ(tests/golden/)と比べる。決まりは docs/golden.md にある。
//!
//! 入力は、単純な場面の説明(四角の位置と色、イメージのピクセル)にしてある。公開 API が変わっても、
//! 同じ場面を作れば、同じ正解データで比べられる。

use seinas_golden::{Image, Runner, Tolerance};
use seinas_render::{solid_rect, Painter, SolidRect};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements, Kind,
            },
            pixman::PixmanRenderer,
        },
    },
    utils::Transform,
};

render_elements! {
    Element<=PixmanRenderer>;
    Solid=SolidRect,
    Memory=MemoryRenderBufferRenderElement<PixmanRenderer>,
}

/// 描画結果を、RGB の画像にする。
fn render(
    width: i32,
    height: i32,
    scene: impl FnOnce(&mut PixmanRenderer) -> Vec<Element>,
) -> Image {
    let mut painter = Painter::new(width, height).expect("a painter");
    let elements = scene(painter.renderer());
    let view = painter.paint(&elements).expect("paint");
    assert_eq!(view.failed_elements(), 0);
    let bgrx: Vec<u8> = (0..view.height())
        .flat_map(|y| view.row(y).to_vec())
        .collect();
    Image::from_bgrx(view.width(), view.height(), &bgrx)
}

fn solid(x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) -> Element {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    Element::Solid(solid_rect(x, y, w, h, [r, g, b, 1.0]))
}

/// メモリーのイメージの要素。`pixels` は B, G, R, A(または X)の順。
fn memory(
    renderer: &mut PixmanRenderer,
    x: i32,
    y: i32,
    (width, height): (i32, i32),
    format: Fourcc,
    pixels: &[u8],
) -> Element {
    let buffer =
        MemoryRenderBuffer::from_slice(pixels, format, (width, height), 1, Transform::Normal, None);
    Element::Memory(
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (x as f64, y as f64),
            &buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .expect("import a memory image"),
    )
}

/// 16x12 の、色が場所ごとに変わる不透明なイメージ(Xrgb8888)。
fn opaque_pattern() -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..12u8 {
        for x in 0..16u8 {
            pixels.extend([x * 16, y * 20, 0x80 + x * 4, 0xff]);
        }
    }
    pixels
}

/// 16x12 の、半透明のイメージ(Argb8888、premultiplied alpha)。alpha は横に 0 から 255 まで変わり、
/// どの色の成分も alpha 以下にしてある。
fn translucent_pattern() -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..12u32 {
        for x in 0..16u32 {
            let alpha = x * 255 / 15;
            // 色(premultiply する前): 赤は y で変わり、緑は一定、青は x で変わる。
            let (r, g, b) = (y * 255 / 11, 0x80, 255 - x * 16);
            let premultiply = |c: u32| (c * alpha + 127) / 255;
            let (r, g, b) = (premultiply(r), premultiply(g), premultiply(b));
            assert!(r <= alpha && g <= alpha && b <= alpha);
            pixels.extend([b as u8, g as u8, r as u8, alpha as u8]);
        }
    }
    pixels
}

fn main() {
    let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-render");

    // A1: 単色の四角の重なりと、画面の 4 辺での切り取り。先頭がいちばん手前。
    let a1 = render(64, 48, |_| {
        vec![
            solid(20, 16, 16, 12, [0xff, 0xff, 0xff]),
            solid(-8, -8, 24, 20, [0xff, 0x00, 0x00]),
            solid(10, 10, 40, 30, [0x00, 0x80, 0x40]),
            solid(50, 36, 30, 30, [0x33, 0x66, 0x99]),
            solid(-6, 30, 20, 30, [0xff, 0xcc, 0x00]),
            solid(40, -10, 12, 16, [0x80, 0x00, 0x80]),
        ]
    });
    run.image("a1-solid-overlap-and-clip", Tolerance::EXACT, a1);

    // A2: 不透明なメモリーのイメージを、単色の四角の上に重ね、右上と左下で切り取る。
    let a2 = render(64, 48, |renderer| {
        let pattern = opaque_pattern();
        vec![
            memory(renderer, 56, -4, (16, 12), Fourcc::Xrgb8888, &pattern),
            memory(renderer, -6, 40, (16, 12), Fourcc::Xrgb8888, &pattern),
            memory(renderer, 20, 18, (16, 12), Fourcc::Xrgb8888, &pattern),
            solid(8, 8, 48, 32, [0x20, 0x40, 0x60]),
        ]
    });
    run.image("a2-memory-opaque", Tolerance::EXACT, a2);

    // A3: 半透明のメモリーのイメージ(premultiplied alpha)を、2 色の四角と背景の上に重ねる。
    let a3 = render(64, 48, |renderer| {
        let pattern = translucent_pattern();
        vec![
            memory(renderer, 8, 6, (16, 12), Fourcc::Argb8888, &pattern),
            memory(renderer, 26, 20, (16, 12), Fourcc::Argb8888, &pattern),
            memory(renderer, 54, 40, (16, 12), Fourcc::Argb8888, &pattern),
            solid(0, 0, 32, 48, [0xff, 0xff, 0xff]),
            solid(32, 0, 32, 24, [0x00, 0x00, 0x00]),
        ]
    });
    run.image("a3-memory-translucent", Tolerance::EXACT, a3);

    // A4: 要素なし(背景だけ)。
    let a4 = render(16, 16, |_| Vec::new());
    run.image("a4-background-only", Tolerance::EXACT, a4);

    run.finish();
}
