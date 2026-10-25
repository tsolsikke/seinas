//! ウィンドウの枠・角・影(最大化していないウィンドウ)と、コンテンツの下の角を欠く要素。
//!
//! - 枠は、外枠(タイトルバーとコンテンツ)の外側に、太さ [`BORDER_WIDTH`] で描く。まっすぐな所は単色の矩形、角は
//!   [`CORNER_RADIUS`] の丸みの弧(縁をなめらかにした、不透明度つきの絵)。
//! - 影は、前もって作って使い回す。ウィンドウの大きさごとに、上下左右の 4 つの矩形の絵を 1 回作って覚え、大きさが
//!   変わったときだけ作り直す。
//! - コンテンツの下の 2 つの角は、[`FRAME_RADIUS`] の丸みで欠く。pixman は、描く範囲(damage)をクリップに使うので、
//!   欠く所を除いた矩形の一覧を渡す([`ClippedSurface`])。上の 2 つの角は、タイトルバーの絵が欠く(`decoration.rs`)。
//!
//! 決まりの説明は `docs/ui-spec.md` の「ウィンドウの枠・角・影」にある。

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
                Element, Id, Kind, RenderElement, UnderlyingStorage,
            },
            pixman::{PixmanError, PixmanFrame, PixmanRenderer},
            utils::{CommitCounter, DamageSet, OpaqueRegions},
        },
    },
    utils::{Buffer as BufferCoords, Physical, Rectangle, Scale, Transform},
};
use tracing::warn;

use crate::{
    decoration::{BORDER_WIDTH, CORNER_RADIUS, FRAME_RADIUS},
    geometry::Rect,
};

/// 影のずれ・ぼけの幅・濃さ(`seinas-theme` の `frame`)。
const SHADOW_OFFSET_Y: i32 = seinas_theme::frame::SHADOW_OFFSET_Y;
const SHADOW_BLUR: i32 = seinas_theme::frame::SHADOW_BLUR;
const SHADOW_OPACITY: f32 = seinas_theme::frame::SHADOW_OPACITY;
/// 影の 4 つの矩形の太さ:矩形の縁から、内へ [`SHADOW_BLUR`] の半分、外へ [`SHADOW_BLUR`]。
const SHADOW_INSIDE: i32 = SHADOW_BLUR / 2;
const SHADOW_BAND: i32 = SHADOW_INSIDE + SHADOW_BLUR;

/// 1 ピクセルを分けて数える数(縦と横に、それぞれこの数)。
const SAMPLES: i32 = 4;

/// 枠の角の絵(左上・右上・左下・右下の順)。大きさは [`CORNER_RADIUS`] の正方形。
pub struct BorderCorners {
    corners: [MemoryRenderBuffer; 4],
}

impl BorderCorners {
    /// 色 `color` の枠の角を作る。
    pub fn new(color: [u8; 3]) -> Self {
        let r = CORNER_RADIUS;
        let corner = |flip_x: bool, flip_y: bool| {
            let mut pixels = vec![0u8; (r * r * 4) as usize];
            for y in 0..r {
                for x in 0..r {
                    // 左上の角に直した位置。弧の中心は (r, r)。
                    let (cx, cy) = (
                        if flip_x { r - 1 - x } else { x },
                        if flip_y { r - 1 - y } else { y },
                    );
                    let mut count = 0;
                    for sy in 0..SAMPLES {
                        for sx in 0..SAMPLES {
                            let px = cx as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                            let py = cy as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                            let d = ((px - r as f32).powi(2) + (py - r as f32).powi(2)).sqrt();
                            if d <= r as f32 && d > (r - BORDER_WIDTH) as f32 {
                                count += 1;
                            }
                        }
                    }
                    let a = (count * 255 / (SAMPLES * SAMPLES)) as u32;
                    let at = ((y * r + x) * 4) as usize;
                    let mul = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
                    pixels[at..at + 4].copy_from_slice(&[
                        mul(color[2]),
                        mul(color[1]),
                        mul(color[0]),
                        a as u8,
                    ]);
                }
            }
            MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (r, r),
                1,
                Transform::Normal,
                None,
            )
        };
        BorderCorners {
            corners: [
                corner(false, false),
                corner(true, false),
                corner(false, true),
                corner(true, true),
            ],
        }
    }
}

/// 外枠 `outer` の外側の枠を、描画の要素(角の絵 4 つと、まっすぐな所の矩形 4 つ)にする。手前から順。
///
/// まっすぐな所は、矩形(x, y, 幅, 高さ)で返す(呼ぶ側が、枠の色の単色の要素にする)。
pub fn border_elements(
    renderer: &mut PixmanRenderer,
    outer: Rect,
    corners: &BorderCorners,
) -> (
    Vec<MemoryRenderBufferRenderElement<PixmanRenderer>>,
    Vec<[i32; 4]>,
) {
    let (b, r) = (BORDER_WIDTH, CORNER_RADIUS);
    // 枠の外側の矩形。
    let (x, y) = (outer.loc.x - b, outer.loc.y - b);
    let (w, h) = (outer.size.w + 2 * b, outer.size.h + 2 * b);
    let places = [
        (x, y),
        (x + w - r, y),
        (x, y + h - r),
        (x + w - r, y + h - r),
    ];
    let images = corners
        .corners
        .iter()
        .zip(places)
        .filter_map(|(buffer, (px, py))| {
            MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (f64::from(px), f64::from(py)),
                buffer,
                None,
                None,
                None,
                Kind::Unspecified,
            )
            .map_err(|e| warn!("failed to prepare a window border: {e}"))
            .ok()
        })
        .collect();
    // まっすぐな所(角を除く)。上、下、左、右。
    let lines = vec![
        [x + r, y, w - 2 * r, b],
        [x + r, y + h - b, w - 2 * r, b],
        [x, y + r, b, h - 2 * r],
        [x + w - b, y + r, b, h - 2 * r],
    ];
    (images, lines)
}

/// 影の濃さ(0〜1)。`d` は、影の矩形の縁からの外向きの距離(内側は負)。
fn shadow_falloff(d: f32) -> f32 {
    let t = ((d + SHADOW_INSIDE as f32) / SHADOW_BAND as f32).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// 1 つのウィンドウの影の、上下左右の 4 つの矩形の絵と、作ったときの大きさ。
pub struct Shadow {
    size: (i32, i32),
    bands: [MemoryRenderBuffer; 4],
}

impl Shadow {
    /// 大きさ `w` × `h`(枠の外側の矩形)の影を作る。
    pub fn new(w: i32, h: i32) -> Self {
        let band = SHADOW_BAND;
        // 矩形の中の点の濃さ。(fx, fy) は、影の矩形の縁からの外向きの距離(横と縦)。
        let alpha = |fx: f32, fy: f32| SHADOW_OPACITY * shadow_falloff(fx) * shadow_falloff(fy);
        let image = |iw: i32, ih: i32, at: &dyn Fn(i32, i32) -> (f32, f32)| {
            let mut pixels = vec![0u8; (iw.max(1) * ih.max(1) * 4) as usize];
            for y in 0..ih {
                for x in 0..iw {
                    let (fx, fy) = at(x, y);
                    let a = (alpha(fx, fy) * 255.0).round() as u8;
                    let i = ((y * iw + x) * 4) as usize;
                    pixels[i + 3] = a;
                }
            }
            MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (iw.max(1), ih.max(1)),
                1,
                Transform::Normal,
                None,
            )
        };
        // 横の距離:左の縁から外へ(負は内)、右の縁から外へ、の大きいほう。
        let outside_x = |x: i32| {
            let px = x as f32 + 0.5 - SHADOW_BLUR as f32;
            (-px).max(px - w as f32)
        };
        let outside_y = |y: i32| {
            let py = y as f32 + 0.5 - SHADOW_BLUR as f32;
            (-py).max(py - h as f32)
        };
        let full_w = w + 2 * SHADOW_BLUR;
        let side_h = (h - 2 * SHADOW_INSIDE).max(0);
        // 上と下の矩形は、横の全体(角を含む)。左右の矩形は、その間。
        let top = image(full_w, band, &|x, y| (outside_x(x), outside_y(y)));
        let bottom = image(full_w, band, &|x, y| {
            (outside_x(x), outside_y(y + h + SHADOW_BLUR - SHADOW_INSIDE))
        });
        let left = image(band, side_h, &|x, y| {
            (outside_x(x), outside_y(y + SHADOW_BLUR + SHADOW_INSIDE))
        });
        let right = image(band, side_h, &|x, y| {
            (
                outside_x(x + w + SHADOW_BLUR - SHADOW_INSIDE),
                outside_y(y + SHADOW_BLUR + SHADOW_INSIDE),
            )
        });
        Shadow {
            size: (w, h),
            bands: [top, bottom, left, right],
        }
    }

    /// 同じ大きさで作った影か。
    pub fn matches(&self, w: i32, h: i32) -> bool {
        self.size == (w, h)
    }

    /// 外枠 `outer` のウィンドウの影を、描画の要素にする。影の矩形は、枠の外側の矩形を、下へ [`seinas_theme::frame::SHADOW_OFFSET_Y`]
    /// ずらしたもの。
    pub fn elements(
        &self,
        renderer: &mut PixmanRenderer,
        outer: Rect,
    ) -> Vec<MemoryRenderBufferRenderElement<PixmanRenderer>> {
        let b = BORDER_WIDTH;
        let (x, y) = (outer.loc.x - b, outer.loc.y - b + SHADOW_OFFSET_Y);
        let (w, h) = self.size;
        let places = [
            (x - SHADOW_BLUR, y - SHADOW_BLUR),
            (x - SHADOW_BLUR, y + h - SHADOW_INSIDE),
            (x - SHADOW_BLUR, y + SHADOW_INSIDE),
            (x + w - SHADOW_INSIDE, y + SHADOW_INSIDE),
        ];
        self.bands
            .iter()
            .zip(places)
            .filter_map(|(buffer, (px, py))| {
                MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    (f64::from(px), f64::from(py)),
                    buffer,
                    None,
                    None,
                    None,
                    Kind::Unspecified,
                )
                .map_err(|e| warn!("failed to prepare a window shadow: {e}"))
                .ok()
            })
            .collect()
    }
}

/// 外枠 `outer` のウィンドウの、コンテンツの下の 2 つの角で、欠くピクセルの数(行ごと)。`row` は、外枠の下の端から
/// 数えた行(0 がいちばん下)。ピクセルの中心が、[`FRAME_RADIUS`] の丸みの外なら欠く。
fn cut_in_row(row: i32) -> i32 {
    let r = FRAME_RADIUS as f32;
    if row >= FRAME_RADIUS {
        return 0;
    }
    let dy = r - (row as f32 + 0.5);
    let inset = r - (r * r - dy * dy).max(0.0).sqrt();
    // ピクセルの中心(x + 0.5)が inset より左なら欠く。
    (inset - 0.5).ceil().max(0.0) as i32
}

/// 矩形 `rect`(画面の上)から、外枠 `outer` の下の 2 つの角の欠く所を除いた、矩形の一覧。
pub fn without_bottom_corners(rect: Rect, outer: Rect) -> Vec<Rect> {
    let bottom = outer.loc.y + outer.size.h;
    let zone_top = bottom - FRAME_RADIUS;
    let (top, end) = (rect.loc.y, rect.loc.y + rect.size.h);
    let (left, right) = (rect.loc.x, rect.loc.x + rect.size.w);
    let mut out = Vec::new();
    let mut band = |y0: i32, y1: i32, x0: i32, x1: i32| {
        if y1 > y0 && x1 > x0 {
            out.push(Rect::new((x0, y0).into(), (x1 - x0, y1 - y0).into()));
        }
    };
    // 角の行より上(と、外枠より下)は、そのまま。
    band(top, end.min(zone_top), left, right);
    band(top.max(bottom), end, left, right);
    for y in top.max(zone_top)..end.min(bottom) {
        let cut = cut_in_row(bottom - 1 - y);
        let (cut_left, cut_right) = (outer.loc.x + cut, outer.loc.x + outer.size.w - cut);
        // 外枠の左の外、欠いた所を除いた中、外枠の右の外。
        band(y, y + 1, left, right.min(outer.loc.x));
        band(y, y + 1, left.max(cut_left), right.min(cut_right));
        band(y, y + 1, left.max(outer.loc.x + outer.size.w), right);
    }
    out
}

/// コンテンツの要素を包み、外枠 `outer` の下の 2 つの角を欠いて描く要素。
pub struct ClippedSurface {
    inner: WaylandSurfaceRenderElement<PixmanRenderer>,
    outer: Rect,
}

impl ClippedSurface {
    pub fn new(inner: WaylandSurfaceRenderElement<PixmanRenderer>, outer: Rect) -> Self {
        ClippedSurface { inner, outer }
    }
}

impl Element for ClippedSurface {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn src(&self) -> Rectangle<f64, BufferCoords> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    // 欠いた所は透けるので、不透明な所は言わない(下の要素を省かせない)。
    fn opaque_regions(&self, _scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        OpaqueRegions::default()
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<PixmanRenderer> for ClippedSurface {
    fn draw(
        &self,
        frame: &mut PixmanFrame<'_, '_>,
        src: Rectangle<f64, BufferCoords>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), PixmanError> {
        // damage は、要素の左上を原点にした矩形。画面の上に直して角を除き、また戻す。
        let clipped: Vec<Rectangle<i32, Physical>> = damage
            .iter()
            .flat_map(|d| {
                let global = Rect::new(
                    (d.loc.x + dst.loc.x, d.loc.y + dst.loc.y).into(),
                    (d.size.w, d.size.h).into(),
                );
                without_bottom_corners(global, self.outer)
            })
            .map(|r| {
                Rectangle::new(
                    (r.loc.x - dst.loc.x, r.loc.y - dst.loc.y).into(),
                    (r.size.w, r.size.h).into(),
                )
            })
            .collect();
        if clipped.is_empty() {
            return Ok(());
        }
        self.inner.draw(frame, src, dst, &clipped, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut PixmanRenderer) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bottom_corners_are_cut_by_rows() {
        // 丸み 7:いちばん下の行は左右で 4 ずつ(ピクセルの中心 3.5 が、丸みの外の 4.4 より左)、上へ行くほど少なく、7 行目からは欠かない。
        let cuts: Vec<i32> = (0..8).map(cut_in_row).collect();
        assert_eq!(cuts[0], 4);
        assert!(cuts.windows(2).all(|w| w[0] >= w[1]), "{cuts:?}");
        assert_eq!(cuts[7], 0);
        // 100x50 の外枠のコンテンツ(y 36〜50)の矩形から、下の角を除く。
        let outer = Rect::new((10, 0).into(), (100, 50).into());
        let content = Rect::new((10, 36).into(), (100, 14).into());
        let parts = without_bottom_corners(content, outer);
        let covered: i32 = parts.iter().map(|r| r.size.w * r.size.h).sum();
        let cut: i32 = (0..FRAME_RADIUS).map(|row| 2 * cut_in_row(row)).sum();
        assert_eq!(covered, 100 * 14 - cut);
        // いちばん下の行は、左右の 4 を除いた 1 本。
        assert!(parts.contains(&Rect::new((14, 49).into(), (92, 1).into())));
    }

    #[test]
    fn the_shadow_fades_outward() {
        assert!((shadow_falloff(-(SHADOW_INSIDE as f32)) - 1.0).abs() < 1e-6);
        assert!(shadow_falloff(0.0) > shadow_falloff(4.0));
        assert_eq!(shadow_falloff(SHADOW_BLUR as f32), 0.0);
    }
}
