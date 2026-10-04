//! 比べるための土台。文字を描く手段を何も入れずに、同じ流れ(フォントを読む、絵を作る、書き出す)だけを行う。
//! 実行ファイルの大きさとメモリーの、増える前の値を測るのに使う。

use text_trial_common::{load_fonts, rss_kb, run, Canvas, Engine, FontKind};

struct Nothing;

impl Engine for Nothing {
    fn name(&self) -> &'static str {
        "baseline"
    }
    fn draw_line(
        &mut self,
        _: &mut Canvas,
        _: FontKind,
        _: f32,
        _: f32,
        _: f32,
        _: &str,
        _: u32,
    ) -> f32 {
        0.0
    }
    fn measure(&mut self, _: FontKind, _: f32, _: &str) -> f32 {
        1.0
    }
}

fn main() {
    let (rss_start, _) = rss_kb();
    let (ui, mono, out) = load_fonts();
    std::hint::black_box((&ui, &mono));
    let (rss_after_load, _) = rss_kb();
    run(&mut Nothing, &out, rss_start, rss_after_load);
}
