//! 候補: fontdue。小さな、字形を描くだけのクレート。文字の並べ方(シェーピング)は行わない。
//!
//! ここでは、1文字ずつ字形を求めて、進む幅のぶんだけ右へ進める、いちばん単純な並べ方をしている。

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};
use text_trial_common::{load_fonts, rss_kb, run, Canvas, Engine, FontKind};

struct Fontdue {
    ui: Font,
    mono: Font,
    /// 描いた字形の覚え(フォント、文字、大きさ)。fontdue自身は覚えを持たない。
    cache: HashMap<(FontKind, char, u32), (Metrics, Vec<u8>)>,
}

impl Fontdue {
    fn font(&self, kind: FontKind) -> &Font {
        match kind {
            FontKind::Ui => &self.ui,
            FontKind::Mono => &self.mono,
        }
    }
}

impl Engine for Fontdue {
    fn name(&self) -> &'static str {
        "fontdue"
    }

    fn draw_line(
        &mut self,
        canvas: &mut Canvas,
        font: FontKind,
        px: f32,
        x: f32,
        baseline: f32,
        text: &str,
        color: u32,
    ) -> f32 {
        let mut pen = x;
        for ch in text.chars() {
            let key = (font, ch, px.to_bits());
            if !self.cache.contains_key(&key) {
                let glyph = self.font(font).rasterize(ch, px);
                self.cache.insert(key, glyph);
            }
            let (metrics, mask) = &self.cache[&key];
            // fontdueの字形は、左下が原点。ymin は、ベースラインから字形の下の端まで。
            let left = pen.round() as i32 + metrics.xmin;
            let top = baseline.round() as i32 - metrics.ymin - metrics.height as i32;
            canvas.blend_mask(left, top, metrics.width, metrics.height, mask, color);
            pen += metrics.advance_width;
        }
        pen - x
    }

    fn measure(&mut self, font: FontKind, px: f32, text: &str) -> f32 {
        text.chars()
            .map(|ch| self.font(font).metrics(ch, px).advance_width)
            .sum()
    }
}

fn main() {
    let (rss_start, _) = rss_kb();
    let (ui, mono, out) = load_fonts();
    let load =
        |bytes: Vec<u8>| Font::from_bytes(bytes, FontSettings::default()).expect("a valid font");
    let mut engine = Fontdue {
        ui: load(ui),
        mono: load(mono),
        cache: HashMap::new(),
    };
    let (rss_after_load, _) = rss_kb();
    run(&mut engine, &out, rss_start, rss_after_load);
}
