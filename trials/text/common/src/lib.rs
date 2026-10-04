//! 文字を描く手段の試しで、どの候補にも共通の部分。
//!
//! 同じ文字列を、同じフォントのファイルから、同じ大きさ・同じ色で描かせて、Xrgb8888の画素(pixmanで
//! 合成できる形)にする。画像(PPM)と、速さ・メモリー・文字の幅の記録を出す。

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

/// 試す文字列。1つが1行。
pub const SAMPLES: &[&str] = &[
    "ひらがな: あいうえお かきくけこ ぱぴぷぺぽ ゃゅょっ",
    "カタカナ: アイウエオ ガギグゲゴ ヴァイオリン ャュョッ",
    "漢字(第1水準): 亜唖娃阿哀愛 日本語 東京都 画面 設定",
    "漢字(第2水準): 弌丐丕个丱 鬱蠢顰 檸檬 躊躇 齟齬",
    "半角カナ: ｱｲｳｴｵ ｶﾞｷﾞｸﾞｹﾞｺﾞ ﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ ｰ｡｢｣､･",
    "英数字: Il1 O0 | Illegal1 = O0o 0123456789 rn m",
    "円記号とバックスラッシュ: ¥1,980 \\path\\to\\file ~ ` ^ _",
    "結合濁点: か\u{3099} は\u{309A} ウ\u{3099} き\u{3099}  合成済み: が ぱ ヴ ぎ",
    "罫線: ┌─┬─┐ │ ├─┼─┤ └─┴─┘ ━┃╋ ═║╬",
    "和欧混植: Seinasは ZeikOS 向けの Wayland コンポジタです (v0.1, 60fps)。",
    "幅の確かめ:",
    "|12345678|abcdefgh|",
    "|あいうえ|亜唖娃阿|",
    "|ｱｲｳｴｵｶｷｸ|┌─┬─┐|",
];

/// 試す大きさ(px)。
pub const SIZES: &[f32] = &[12.0, 14.0, 16.0, 18.0];

/// 使うフォント。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontKind {
    /// BIZ UDPゴシック(UI用)。
    Ui,
    /// BIZ UDゴシック(端末・コード用。半角と全角の幅が1:2)。
    Mono,
}

impl FontKind {
    pub const ALL: [FontKind; 2] = [FontKind::Ui, FontKind::Mono];

    pub fn file_name(self) -> &'static str {
        match self {
            FontKind::Ui => "BIZUDPGothic-Regular.ttf",
            FontKind::Mono => "BIZUDGothic-Regular.ttf",
        }
    }

    /// フォントの中に書かれている名前(ファミリー名)。
    pub fn family(self) -> &'static str {
        match self {
            FontKind::Ui => "BIZ UDPGothic",
            FontKind::Mono => "BIZ UDGothic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FontKind::Ui => "udp",
            FontKind::Mono => "ud",
        }
    }
}

/// 背景と文字の色。
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub label: &'static str,
    /// 0xRRGGBB。
    pub background: u32,
    pub foreground: u32,
}

pub const THEMES: [Theme; 2] = [
    Theme {
        label: "light",
        background: 0xf5_f5_f5,
        foreground: 0x20_20_20,
    },
    Theme {
        label: "dark",
        background: 0x19_1e_28,
        foreground: 0xe8_e8_e8,
    },
];

/// 描く先。Xrgb8888の画素(1画素が0x00RRGGBB)。
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: u32) -> Self {
        Canvas {
            width,
            height,
            pixels: vec![background; width * height],
        }
    }

    /// (x, y)に、色 `color` を、濃さ `coverage`(0〜255)で重ねる。画面の外は捨てる。
    pub fn blend(&mut self, x: i32, y: i32, color: u32, coverage: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height || coverage == 0
        {
            return;
        }
        let at = y as usize * self.width + x as usize;
        let under = self.pixels[at];
        let a = coverage as u32;
        let mix = |shift: u32| {
            let (c, u) = ((color >> shift) & 0xff, (under >> shift) & 0xff);
            ((c * a + u * (255 - a) + 127) / 255) << shift
        };
        self.pixels[at] = mix(16) | mix(8) | mix(0);
    }

    /// 左上が(x, y)、大きさが `width` × `height` の濃さの絵(1画素1バイト)を、色 `color` で重ねる。
    pub fn blend_mask(
        &mut self,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        mask: &[u8],
        color: u32,
    ) {
        for row in 0..height {
            for column in 0..width {
                self.blend(
                    x + column as i32,
                    y + row as i32,
                    color,
                    mask[row * width + column],
                );
            }
        }
    }

    /// PPM(P6)で書く。
    pub fn write_ppm(&self, path: &Path) -> std::io::Result<()> {
        let mut image = format!("P6 {} {} 255\n", self.width, self.height).into_bytes();
        image.extend(
            self.pixels
                .iter()
                .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]),
        );
        fs::write(path, image)
    }
}

/// 候補が実装するもの。
pub trait Engine {
    /// 候補の名前(ファイル名に使う)。
    fn name(&self) -> &'static str;

    /// 文字列 `text` を1行、左端が `x`、ベースラインが `baseline` の位置に描く。進んだ幅(px)を返す。
    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &mut self,
        canvas: &mut Canvas,
        font: FontKind,
        px: f32,
        x: f32,
        baseline: f32,
        text: &str,
        color: u32,
    ) -> f32;

    /// 描かずに、文字列の幅(px)だけを求める。
    fn measure(&mut self, font: FontKind, px: f32, text: &str) -> f32;

    /// フォントに字形が無い文字。調べられない候補は、Noneを返す。
    fn missing(&mut self, _font: FontKind, _text: &str) -> Option<Vec<char>> {
        None
    }
}

/// 3つ目の引数が `nohint` なら、字形のヒンティング(小さい字を画素の格子に合わせる処理)を切る。
pub fn hinting_requested() -> bool {
    std::env::args().nth(3).as_deref() != Some("nohint")
}

/// フォントのファイルを読む。置き場所は、引数1つ目(無ければ `../../target/fonts`)。
pub fn load_fonts() -> (Vec<u8>, Vec<u8>, PathBuf) {
    let mut args = std::env::args().skip(1);
    let fonts = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "../../target/fonts".to_owned()),
    );
    let out = PathBuf::from(args.next().unwrap_or_else(|| "out".to_owned()));
    let read = |kind: FontKind| {
        let path = fonts.join(kind.file_name());
        fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "cannot read {}: {e} (run tools/fetch-fonts.sh first)",
                path.display()
            )
        })
    };
    (read(FontKind::Ui), read(FontKind::Mono), out)
}

/// いまの常駐メモリー(kB)と、その最大(kB)。
pub fn rss_kb() -> (u64, u64) {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

fn line_height(px: f32) -> f32 {
    (px * 1.5).round()
}

/// 1枚の絵を描く。文字列の全部を、1つのフォント・1つの大きさ・1つの配色で描く。
pub fn draw_page(engine: &mut dyn Engine, font: FontKind, px: f32, theme: Theme) -> Canvas {
    let line = line_height(px);
    let margin = 8.0;
    let width = (px * 44.0) as usize + 2 * margin as usize;
    let height = (line * SAMPLES.len() as f32 + 2.0 * margin) as usize;
    let mut canvas = Canvas::new(width, height, theme.background);
    for (index, text) in SAMPLES.iter().enumerate() {
        // ベースラインは、行の上から px だけ下に置く(どの候補でも同じ位置)。
        let baseline = margin + line * index as f32 + px;
        engine.draw_line(
            &mut canvas,
            font,
            px,
            margin,
            baseline,
            text,
            theme.foreground,
        );
    }
    canvas
}

/// 大きさを1pxずつ変えて、見分けにくい字を並べた絵を描く。小さい字のつぶれ方を見るためのもの。
pub fn draw_sizes_page(engine: &mut dyn Engine, font: FontKind, theme: Theme) -> Canvas {
    const TEXT: &str = "0123456789 8B3 S5 eao6 Il1 O0 ぱば ポボ 鬱曜 ━─";
    let margin = 8.0;
    let sizes: Vec<f32> = (10..=20).map(|px| px as f32).collect();
    let height: f32 = sizes.iter().map(|px| line_height(*px)).sum::<f32>() + 2.0 * margin;
    let mut canvas = Canvas::new(20 * 36 + 16, height as usize, theme.background);
    let mut top = margin;
    for px in sizes {
        let text = format!("{px:>2}px {TEXT}");
        engine.draw_line(
            &mut canvas,
            font,
            px,
            margin,
            top + px,
            &text,
            theme.foreground,
        );
        top += line_height(px);
    }
    canvas
}

/// 候補を一通り動かす。画像を書き、速さ・メモリー・文字の幅を記録する。
///
/// `after_load` は、フォントを読み込み終えた時点の常駐メモリー(kB)。
pub fn run(engine: &mut dyn Engine, out: &Path, rss_start: u64, rss_after_load: u64) {
    fs::create_dir_all(out).expect("create the output directory");
    let name = engine.name();
    let mut report = String::new();
    report.push_str(&format!("engine: {name}\n"));
    report.push_str(&format!(
        "rss: start {rss_start} kB, after loading the fonts {rss_after_load} kB\n"
    ));

    // 1回目(何も覚えていない状態)。画像を書く。書き出しの時間は、数えない。
    let mut cold = std::time::Duration::ZERO;
    for font in FontKind::ALL {
        for &px in SIZES {
            for theme in THEMES {
                let started = Instant::now();
                let canvas = draw_page(engine, font, px, theme);
                cold += started.elapsed();
                let file = format!(
                    "{name}-{}-{}-{}px.ppm",
                    font.label(),
                    theme.label,
                    px as u32
                );
                canvas.write_ppm(&out.join(file)).expect("write the image");
            }
        }
    }
    for font in FontKind::ALL {
        for theme in THEMES {
            let canvas = draw_sizes_page(engine, font, theme);
            let file = format!("{name}-sizes-{}-{}.ppm", font.label(), theme.label);
            canvas.write_ppm(&out.join(file)).expect("write the image");
        }
    }
    let pages = FontKind::ALL.len() * SIZES.len() * THEMES.len();
    let (rss_after_cold, _) = rss_kb();

    // 2回目以降(字形を覚えた状態)。書き出しはしない。
    const REPEAT: usize = 20;
    let warm = Instant::now();
    for _ in 0..REPEAT {
        for font in FontKind::ALL {
            for &px in SIZES {
                for theme in THEMES {
                    std::hint::black_box(draw_page(engine, font, px, theme));
                }
            }
        }
    }
    let warm = warm.elapsed() / REPEAT as u32;
    let (rss_after_warm, rss_peak) = rss_kb();

    let characters: usize = SAMPLES.iter().map(|s| s.chars().count()).sum();
    report.push_str(&format!(
        "pages per pass: {pages} ({} lines, {characters} characters each)\n",
        SAMPLES.len()
    ));
    report.push_str(&format!(
        "time: first pass {:.2} ms ({:.3} ms per page), later passes {:.2} ms ({:.3} ms per page)\n",
        cold.as_secs_f64() * 1e3,
        cold.as_secs_f64() * 1e3 / pages as f64,
        warm.as_secs_f64() * 1e3,
        warm.as_secs_f64() * 1e3 / pages as f64,
    ));
    report.push_str(&format!(
        "rss: after the first pass {rss_after_cold} kB, after {REPEAT} more passes {rss_after_warm} kB, peak {rss_peak} kB\n"
    ));

    // 端末用のフォントで、半角と全角の幅を比べる。
    report.push_str("widths with BIZ UDGothic (px): size, A, あ, ｱ, ─, ratio あ/A, 8 half-width, 4 full-width\n");
    for &px in &[12.0, 13.0, 14.0, 15.0, 16.0, 18.0] {
        let w = |engine: &mut dyn Engine, text: &str| engine.measure(FontKind::Mono, px, text);
        let (a, hira, kana, rule) = (
            w(engine, "A"),
            w(engine, "あ"),
            w(engine, "ｱ"),
            w(engine, "─"),
        );
        report.push_str(&format!(
            "  {px:>4}: {a:.3} {hira:.3} {kana:.3} {rule:.3}  ratio {:.3}  {:.3} {:.3}\n",
            hira / a,
            w(engine, "12345678"),
            w(engine, "あいうえ"),
        ));
    }
    // 半角と全角が混ざった行の幅。半角の幅が半端になる大きさ(13pxなど)で、候補ごとの差が出る。
    for &px in &[13.0, 14.0, 15.0] {
        let text = "8B3 S5 ぱば 0123456789";
        report.push_str(&format!(
            "width of \"{text}\" with BIZ UDGothic at {px}px: {:.3} (22 half-width cells = {:.3})\n",
            engine.measure(FontKind::Mono, px, text),
            22.0 * px / 2.0
        ));
    }
    // 結合濁点: 「か」+結合濁点の幅が、「が」1文字の幅と同じなら、1つの字として扱われている。
    for font in FontKind::ALL {
        let combined = engine.measure(font, 16.0, "か\u{3099}");
        let precomposed = engine.measure(font, 16.0, "が");
        let base = engine.measure(font, 16.0, "か");
        report.push_str(&format!(
            "combining dakuten with {} at 16px: か+U+3099 = {combined:.3}, が = {precomposed:.3}, か = {base:.3}\n",
            font.family()
        ));
    }
    for font in FontKind::ALL {
        let all: String = SAMPLES.concat();
        if let Some(missing) = engine.missing(font, &all) {
            let list: Vec<String> = missing
                .iter()
                .map(|c| format!("{c}(U+{:04X})", *c as u32))
                .collect();
            report.push_str(&format!(
                "characters without a glyph in {}: {}\n",
                font.family(),
                list.join(" ")
            ));
        }
    }
    print!("{report}");
    fs::write(out.join(format!("{name}-report.txt")), report).expect("write the report");
}
