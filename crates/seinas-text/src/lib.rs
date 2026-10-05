//! Seinasが自分で描く文字。
//!
//! フォントをファイルから読み、1行の文字を、呼ぶ側の画素の上に描く。システムのフォントは探さない
//! (fontconfigを使わない)。フォントは実行ファイルに埋め込まず、置き場所を呼ぶ側が教える。
//!
//! - 主のフォント(UI用)に無い文字は、控えのフォントで描く。
//! - 控えにも無い文字は、四角の枠で描く(字が欠けていることが分かるようにする)。
//! - フォントが読めないときも、失敗にはしない。読めなかった理由を返し、文字は描かないだけにする。
//!
//! 決まり(手段、大きさ、ヒンティング)は `docs/text-rendering.md` にまとめてある。

use std::{
    fmt,
    path::{Path, PathBuf},
};

use cosmic_text::{
    fontdb, Attrs, Buffer, CacheKeyFlags, Color, Fallback, Family, FontSystem, Hinting, Metrics,
    Shaping, SwashCache, Wrap,
};

use unicode_script::Script;

/// 主のフォント(UI用)のファイルの名前と、その中のフォントの名前。
pub const UI_FONT_FILE: &str = "BIZUDPGothic-Regular.ttf";
pub const UI_FONT_FAMILY: &str = "BIZ UDPGothic";
/// 控えのフォントのファイルの名前と、その中のフォントの名前。
pub const FALLBACK_FONT_FILE: &str = "unifont_jp-18.0.01.otf";
pub const FALLBACK_FONT_FAMILY: &str = "Unifont-JP";

/// 文字の大きさの既定(画素)。これより小さい大きさは、既定では使わない。
pub const DEFAULT_SIZE: f32 = 14.0;
/// 字形のヒンティングを入れる、いちばん小さい大きさ(画素)。これより小さいと、字の形が崩れる
/// (12px前後で「8」が「0」に見える)ので、切る。
pub const HINTING_FROM: f32 = 14.0;

/// 長すぎる行を切ったときに、末尾に付ける印。
const ELLIPSIS: &str = "…";
/// 1行として扱う文字数の上限。これより後ろは、どうせ収まらないので見ない。
const MAX_CHARS: usize = 512;
/// 文字の言語。漢字の字形の選び方などに使われる。
const LOCALE: &str = "ja-JP";

/// フォントのファイルの場所。
#[derive(Debug, Clone)]
pub struct FontFiles {
    /// 主のフォント(UI用)。
    pub ui: PathBuf,
    /// 控えのフォント。
    pub fallback: PathBuf,
}

impl FontFiles {
    /// `dir` の下に、決まった名前で置かれているものとする。
    pub fn in_dir(dir: &Path) -> Self {
        FontFiles {
            ui: dir.join(UI_FONT_FILE),
            fallback: dir.join(FALLBACK_FONT_FILE),
        }
    }
}

/// フォントを読めなかった理由。
#[derive(Debug)]
pub enum FontProblem {
    /// ファイルを読めなかった(無い、権限が無いなど)。
    Unreadable {
        path: PathBuf,
        error: std::io::Error,
    },
    /// ファイルは読めたが、求めるフォントとして使えなかった(壊れている、別のフォントである)。
    Unusable {
        path: PathBuf,
        expected: &'static str,
    },
}

impl fmt::Display for FontProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontProblem::Unreadable { path, error } => {
                write!(f, "cannot read the font {}: {error}", path.display())
            }
            FontProblem::Unusable { path, expected } => write!(
                f,
                "{} is not a usable font file of \"{expected}\"",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FontProblem {}

/// 文字を描く先。1画素は4バイトで、並びは B, G, R, X(`seinas-render` の描画結果と同じ)。
pub struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: i32,
    height: i32,
}

impl<'a> Canvas<'a> {
    /// `pixels` は、`width × height × 4` バイト(行の余りは無い)。
    ///
    /// # Panics
    ///
    /// 大きさが合わないとき。
    pub fn new(pixels: &'a mut [u8], width: usize, height: usize) -> Self {
        assert_eq!(pixels.len(), width * height * 4, "the canvas size");
        Canvas {
            pixels,
            width: width as i32,
            height: height as i32,
        }
    }

    /// (x, y)に、色 `color`(赤, 緑, 青)を、濃さ `alpha`(0〜255)で重ねる。外の点は捨てる。
    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], alpha: u8) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height || alpha == 0 {
            return;
        }
        let at = ((y * self.width + x) * 4) as usize;
        let alpha = alpha as u32;
        // 並びは B, G, R。
        for (channel, source) in [color[2], color[1], color[0]].into_iter().enumerate() {
            let below = self.pixels[at + channel] as u32;
            self.pixels[at + channel] =
                ((source as u32 * alpha + below * (255 - alpha) + 127) / 255) as u8;
        }
    }
}

/// 1行の描き方。
#[derive(Debug, Clone, Copy)]
pub struct Line {
    /// 文字の大きさ(画素)。
    pub size: f32,
    /// 文字の色(赤, 緑, 青)。
    pub color: [u8; 3],
    /// 行の左の端。
    pub x: i32,
    /// 行の上の端と高さ。文字は、この高さの中で、縦の中央に来る。
    pub top: i32,
    pub height: i32,
    /// 使ってよい幅。これに収まらなければ、末尾を切って「…」を付ける。
    pub max_width: i32,
}

/// 1行を描いた結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drawn {
    /// 描いた文字の幅(画素)。
    pub width: i32,
    /// 収まらずに、末尾を切ったか。
    pub truncated: bool,
    /// 控えのフォントで描いた字形の数。
    pub fallback_glyphs: usize,
    /// どのフォントにも無く、四角の枠で描いた字形の数。
    pub missing_glyphs: usize,
}

/// 主のフォントに無い文字を、どのフォントで探すか。控えのフォントだけを、どの文字の種類にも使う。
struct UnifontFallback;

impl Fallback for UnifontFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &[FALLBACK_FONT_FAMILY]
    }
    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }
    fn script_fallback(&self, _script: Script, _locale: &str) -> &[&'static str] {
        &[]
    }
}

/// 読み込んだフォント。
struct Fonts {
    system: FontSystem,
    cache: SwashCache,
    /// 行を組むときに、最初に使うフォントの名前。主のフォントが無ければ、控えのフォント。
    family: &'static str,
}

/// 文字を描くもの。フォントを持つ。
pub struct TextPainter {
    /// 使えるフォントが1つも無ければNone。そのときは、何も描かない。
    fonts: Option<Fonts>,
}

impl TextPainter {
    /// フォントを読む。読めないフォントがあっても失敗にはせず、読めなかった理由を一緒に返す。
    ///
    /// 主のフォントだけが読めないときは、控えのフォントで全部を描く。どちらも読めないときは、
    /// 何も描かない([`TextPainter::can_draw`] がfalseを返す)。
    pub fn load(files: &FontFiles) -> (Self, Vec<FontProblem>) {
        let mut db = fontdb::Database::new();
        let mut problems = Vec::new();
        let mut loaded = Vec::new();
        for (path, family) in [
            (&files.ui, UI_FONT_FAMILY),
            (&files.fallback, FALLBACK_FONT_FAMILY),
        ] {
            match load_font(&mut db, path, family) {
                Ok(()) => loaded.push(family),
                Err(problem) => problems.push(problem),
            }
        }
        let fonts = loaded.first().map(|&family| Fonts {
            system: FontSystem::new_with_locale_and_db_and_fallback(
                LOCALE.to_owned(),
                db,
                UnifontFallback,
            ),
            cache: SwashCache::new(),
            family,
        });
        (TextPainter { fonts }, problems)
    }

    /// フォントを1つも持たないもの。何も描かない。
    pub fn without_fonts() -> Self {
        TextPainter { fonts: None }
    }

    /// 文字を描けるか(使えるフォントが1つでもあるか)。
    pub fn can_draw(&self) -> bool {
        self.fonts.is_some()
    }

    /// `text` を1行で描く。フォントが無ければ、何も描かない。
    ///
    /// 改行などの制御文字は、空白として扱う。
    pub fn draw_line(&mut self, canvas: &mut Canvas<'_>, text: &str, line: Line) -> Drawn {
        let Some(fonts) = &mut self.fonts else {
            return Drawn::default();
        };
        if line.max_width <= 0 || line.height <= 0 || line.size <= 0.0 {
            return Drawn::default();
        }
        let text: String = text
            .chars()
            .take(MAX_CHARS)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let (buffer, truncated) = fonts.fit(&text, line);
        let mut drawn = Drawn {
            truncated,
            ..Drawn::default()
        };
        let ui_font = fonts.face_of(fonts.family);
        let color = Color::rgb(line.color[0], line.color[1], line.color[2]);
        for run in buffer.layout_runs().take(1) {
            drawn.width = run.line_w.ceil() as i32;
            for glyph in run.glyphs {
                if glyph.glyph_id == 0 {
                    // どのフォントにも無い文字。フォントが持つ「無い字」の形はまちまちなので、自分で描く。
                    drawn.missing_glyphs += 1;
                    draw_missing_box(canvas, glyph.x, glyph.w, run.line_y, line);
                    continue;
                }
                if Some(glyph.font_id) != ui_font {
                    drawn.fallback_glyphs += 1;
                }
                let physical = glyph.physical((line.x as f32, line.top as f32 + run.line_y), 1.0);
                fonts.cache.with_pixels(
                    &mut fonts.system,
                    physical.cache_key,
                    color,
                    |x, y, pixel| {
                        canvas.blend(physical.x + x, physical.y + y, line.color, pixel.a());
                    },
                );
            }
        }
        drawn
    }
}

impl Fonts {
    /// 名前が `family` のフォントの、一覧の中での番号。
    fn face_of(&self, family: &str) -> Option<fontdb::ID> {
        self.system
            .db()
            .faces()
            .find(|face| face.families.iter().any(|(name, _)| name == family))
            .map(|face| face.id)
    }

    /// 1行を組む。
    fn layout(&mut self, text: &str, line: Line) -> Buffer {
        // 行の高さを渡すと、文字はその中で縦の中央に置かれる。
        let mut buffer = Buffer::new(
            &mut self.system,
            Metrics::new(line.size, line.height as f32),
        );
        buffer.set_wrap(Wrap::None);
        // 字形を画素の境目に合わせて置く(小数の位置には置かない)。
        buffer.set_hinting(Hinting::Enabled);
        let mut attrs = Attrs::new().family(Family::Name(self.family));
        if line.size < HINTING_FROM {
            attrs = attrs.cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
        }
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.system, false);
        buffer
    }

    /// 幅に収まるように1行を組む。収まらなければ、末尾を切って「…」を付ける。切ったかどうかも返す。
    fn fit(&mut self, text: &str, line: Line) -> (Buffer, bool) {
        let max_width = line.max_width as f32;
        let buffer = self.layout(text, line);
        if line_width(&buffer) <= max_width {
            return (buffer, false);
        }
        // 「…」を足しても収まる所で切る。字形の並びから、切る場所の見当をつける。
        let ellipsis_width = line_width(&self.layout(ELLIPSIS, line));
        let mut keep = buffer
            .layout_runs()
            .next()
            .map(|run| {
                run.glyphs
                    .iter()
                    .filter(|glyph| glyph.x + glyph.w <= max_width - ellipsis_width)
                    .map(|glyph| glyph.end)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        loop {
            while !text.is_char_boundary(keep) {
                keep -= 1;
            }
            let shortened = format!("{}{ELLIPSIS}", text[..keep].trim_end());
            let buffer = self.layout(&shortened, line);
            // 見当が外れていたら(右から左へ書く文字が混ざるときなど)、1文字ずつ縮める。
            if line_width(&buffer) <= max_width || keep == 0 {
                return (buffer, true);
            }
            keep -= 1;
        }
    }
}

/// 組んだ1行の幅。
fn line_width(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .map(|run| run.line_w)
        .unwrap_or(0.0)
}

/// どのフォントにも無い文字の代わりに、四角の枠を描く。
fn draw_missing_box(canvas: &mut Canvas<'_>, x: f32, width: f32, baseline: f32, line: Line) {
    // 幅は字形の送りに合わせ、高さは大文字の高さくらいにする。字と字の間に、1画素ずつ空きを残す。
    let left = line.x + x.round() as i32 + 1;
    let right = left + (width.round() as i32 - 2).max(line.size as i32 / 3);
    let bottom = line.top + baseline.round() as i32;
    let top = bottom - (line.size * 0.7).round() as i32;
    for px in left..right {
        canvas.blend(px, top, line.color, 255);
        canvas.blend(px, bottom - 1, line.color, 255);
    }
    for py in top..bottom {
        canvas.blend(left, py, line.color, 255);
        canvas.blend(right - 1, py, line.color, 255);
    }
}

/// フォントのファイルを読んで、一覧に入れる。中に `family` という名前のフォントが無ければ、使えない。
fn load_font(
    db: &mut fontdb::Database,
    path: &Path,
    family: &'static str,
) -> Result<(), FontProblem> {
    let data = std::fs::read(path).map_err(|error| FontProblem::Unreadable {
        path: path.to_owned(),
        error,
    })?;
    // 壊れたファイルは、一覧に入らないだけで、失敗は返ってこない。入ったものを見て確かめる。
    let added = db.load_font_source(fontdb::Source::Binary(std::sync::Arc::new(data)));
    let usable = added.iter().any(|&id| {
        db.face(id)
            .is_some_and(|face| face.families.iter().any(|(name, _)| name == family))
    });
    if usable {
        return Ok(());
    }
    for id in added {
        db.remove_face(id);
    }
    Err(FontProblem::Unusable {
        path: path.to_owned(),
        expected: family,
    })
}
