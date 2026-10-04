//! 候補: cosmic-text + swash。文字の並べ方(シェーピング)、行の組み方、フォントの切り替え(控え)、
//! 字形を描くことと覚えておくことを、まとめて持つ。
//!
//! フォントは、ファイルから読んだ中身を明示的に渡す。システムのフォントの探索(fontconfig)は、
//! 機能ごと外してある。ロケールも明示的に渡す。

use cosmic_text::{
    fontdb, Attrs, Buffer, CacheKeyFlags, Color, Family, FontSystem, Hinting, Metrics, Shaping,
    SwashCache, Wrap,
};
use text_trial_common::{hinting_requested, load_fonts, rss_kb, run, Canvas, Engine, FontKind};

struct Cosmic {
    fonts: FontSystem,
    cache: SwashCache,
    /// 字形のヒンティングを使うか。
    hint: bool,
}

impl Cosmic {
    /// 1行を組む。
    fn layout(&mut self, kind: FontKind, px: f32, text: &str) -> Buffer {
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(px, (px * 1.5).round()));
        buffer.set_wrap(Wrap::None);
        // UI用のフォントでは、字形を画素の境目に合わせて置く(小数の位置には置かない)。
        // 端末用のフォントでは、合わせない。合わせると、字形ごとに幅が切り上げられて(13pxなら半角が
        // 6.5→7px)、半角と全角の幅が1:2でなくなるため。
        buffer.set_hinting(match kind {
            FontKind::Ui => Hinting::Enabled,
            FontKind::Mono => Hinting::Disabled,
        });
        let mut attrs = Attrs::new().family(Family::Name(kind.family()));
        if !self.hint {
            // 字形そのもののヒンティングは、既定で使われる。切るときは、明示する。
            attrs = attrs.cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
        }
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }
}

impl Engine for Cosmic {
    fn name(&self) -> &'static str {
        if self.hint {
            "cosmic-text"
        } else {
            "cosmic-text-nohint"
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
        let mut buffer = self.layout(kind, px, text);
        let (line_y, width) = buffer
            .layout_runs()
            .next()
            .map(|run| (run.line_y, run.line_w))
            .unwrap_or((px, 0.0));
        // cosmic-textは、行の上の端を原点にして描く。ベースラインが、求める位置に来るようにずらす。
        let (left, top) = (x.round() as i32, (baseline - line_y).round() as i32);
        let text_color = Color::rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            text_color,
            |gx, gy, w, h, c| {
                for row in 0..h as i32 {
                    for column in 0..w as i32 {
                        canvas.blend(left + gx + column, top + gy + row, color, c.a());
                    }
                }
            },
        );
        width
    }

    fn measure(&mut self, kind: FontKind, px: f32, text: &str) -> f32 {
        self.layout(kind, px, text)
            .layout_runs()
            .next()
            .map(|run| run.line_w)
            .unwrap_or(0.0)
    }
}

fn main() {
    let (rss_start, _) = rss_kb();
    let (ui, mono, out) = load_fonts();
    // 使うフォントだけを入れた一覧を作る。システムのフォントは、読みに行かない。
    let mut db = fontdb::Database::new();
    db.load_font_data(ui);
    db.load_font_data(mono);
    // 4つ目の引数があれば、控えのフォントとして足す。主のフォントに無い文字は、一覧の中のほかの
    // フォントから探される。
    if let Some(path) = std::env::args().nth(4) {
        db.load_font_data(
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}")),
        );
    }
    let mut engine = Cosmic {
        fonts: FontSystem::new_with_locale_and_db("ja-JP".to_owned(), db),
        cache: SwashCache::new(),
        hint: hinting_requested(),
    };
    let (rss_after_load, _) = rss_kb();
    run(&mut engine, &out, rss_start, rss_after_load);
}
