//! 候補: swashだけ。文字の並べ方(シェーピング)と、字形を描くこと(ヒンティング付き)の両方を持つ。
//!
//! 行の折り返し、フォントの切り替え(控え)、双方向の文字の並べ替えは、自分で書く必要がある。
//! ここでは、1行を1つのまとまりとして、左から右へ並べている。

use std::collections::HashMap;

use swash::{
    scale::{image::Image, Render, ScaleContext, Source},
    shape::ShapeContext,
    text::Script,
    zeno::{Format, Vector},
    CacheKey, FontRef, GlyphId,
};
use text_trial_common::{hinting_requested, load_fonts, rss_kb, run, Canvas, Engine, FontKind};

struct Swash {
    ui: Vec<u8>,
    mono: Vec<u8>,
    /// フォントごとの、覚えの鍵。同じ鍵を使い続けると、swashがフォントごとの下ごしらえ(ヒンティングの
    /// 準備など)を覚えておいてくれる。
    keys: [CacheKey; 2],
    shape: ShapeContext,
    scale: ScaleContext,
    /// 描いた字形の覚え(フォント、字形の番号、大きさ)。
    cache: HashMap<(FontKind, GlyphId, u32), Option<Image>>,
    /// 字形のヒンティングを使うか。
    hint: bool,
}

/// 並べ終えた1つの字形。
struct Placed {
    id: GlyphId,
    x: f32,
    y: f32,
}

impl Swash {
    fn data(&self, kind: FontKind) -> &[u8] {
        match kind {
            FontKind::Ui => &self.ui,
            FontKind::Mono => &self.mono,
        }
    }

    /// 文字列を並べる。字形の並びと、全体の幅を返す。
    fn shape(&mut self, kind: FontKind, px: f32, text: &str) -> (Vec<Placed>, f32) {
        let (data, key) = match kind {
            FontKind::Ui => (&self.ui, self.keys[0]),
            FontKind::Mono => (&self.mono, self.keys[1]),
        };
        let font = FontRef {
            data,
            offset: 0,
            key,
        };
        let mut shaper = self
            .shape
            .builder(font)
            .script(Script::Hiragana)
            .size(px)
            .build();
        shaper.add_str(text);
        let mut placed = Vec::new();
        let mut pen = 0.0;
        shaper.shape_with(|cluster| {
            for glyph in cluster.glyphs {
                placed.push(Placed {
                    id: glyph.id,
                    x: pen + glyph.x,
                    y: glyph.y,
                });
                pen += glyph.advance;
            }
        });
        (placed, pen)
    }
}

impl Engine for Swash {
    fn name(&self) -> &'static str {
        if self.hint {
            "swash"
        } else {
            "swash-nohint"
        }
    }

    fn draw_line(
        &mut self,
        canvas: &mut Canvas,
        kind: FontKind,
        px: f32,
        x: f32,
        baseline: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        let (placed, width) = self.shape(kind, px, text);
        for glyph in placed {
            let key = (kind, glyph.id, px.to_bits());
            if !self.cache.contains_key(&key) {
                let (data, font_key) = match kind {
                    FontKind::Ui => (&self.ui, self.keys[0]),
                    FontKind::Mono => (&self.mono, self.keys[1]),
                };
                let font = FontRef {
                    data,
                    offset: 0,
                    key: font_key,
                };
                // ヒンティング(小さい字を画素の格子に合わせる処理)は、引数で切り替える。
                let mut scaler = self.scale.builder(font).size(px).hint(self.hint).build();
                let image = Render::new(&[Source::Outline])
                    .format(Format::Alpha)
                    .offset(Vector::new(0.0, 0.0))
                    .render(&mut scaler, glyph.id);
                self.cache.insert(key, image);
            }
            if let Some(image) = &self.cache[&key] {
                // 字形は、画素の境目に合わせて置く(小数の位置には置かない)。
                let left = (x + glyph.x).round() as i32 + image.placement.left;
                let top = (baseline - glyph.y).round() as i32 - image.placement.top;
                canvas.blend_mask(
                    left,
                    top,
                    image.placement.width as usize,
                    image.placement.height as usize,
                    &image.data,
                    color,
                );
            }
        }
        width
    }

    fn measure(&mut self, kind: FontKind, px: f32, text: &str) -> f32 {
        self.shape(kind, px, text).1
    }

    fn missing(&mut self, kind: FontKind, text: &str) -> Option<Vec<char>> {
        let font = FontRef::from_index(self.data(kind), 0).expect("a valid font");
        let charmap = font.charmap();
        let mut missing: Vec<char> = text
            .chars()
            .filter(|&c| c != ' ' && charmap.map(c) == 0)
            .collect();
        missing.sort();
        missing.dedup();
        Some(missing)
    }
}

fn main() {
    let (rss_start, _) = rss_kb();
    let (ui, mono, out) = load_fonts();
    let mut engine = Swash {
        ui,
        mono,
        keys: [CacheKey::new(), CacheKey::new()],
        shape: ShapeContext::new(),
        scale: ScaleContext::new(),
        cache: HashMap::new(),
        hint: hinting_requested(),
    };
    // フォントとして読めることを、先に確かめておく。
    for kind in FontKind::ALL {
        FontRef::from_index(engine.data(kind), 0).expect("a valid font");
    }
    let (rss_after_load, _) = rss_kb();
    run(&mut engine, &out, rss_start, rss_after_load);
}
