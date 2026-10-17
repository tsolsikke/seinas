//! Seinasが自分で描く文字。
//!
//! フォントをファイルから読み、1行の文字を、呼ぶ側のピクセルの上に描く。fontconfig は使わない。フォントは実行
//! ファイルに埋め込まず、同梱のフォントの置き場を呼ぶ側が教える。利用者が設定のファイルで選んだフォントは、
//! 置き場を自分で調べて使う(`fonts` の module。ADR 0003)。フォントのファイルは、mmap で読む。
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
    fontdb, Attrs, Buffer, CacheKeyFlags, Fallback, Family, FontSystem, Hinting, Metrics, Shaping,
    SwashCache, SwashContent, Wrap,
};

pub mod fonts;
mod glyph_cache;

pub use glyph_cache::{GlyphCache, GLYPH_CACHE_LIMIT};

use unicode_script::Script;

/// 主のフォント(UI用)のファイルの名前と、その中のフォントの名前。
pub const UI_FONT_FILE: &str = "BIZUDPGothic-Regular.ttf";
pub const UI_FONT_FAMILY: &str = "BIZ UDPGothic";
/// 控えのフォントのファイルの名前と、その中のフォントの名前。
pub const FALLBACK_FONT_FILE: &str = "unifont_jp-18.0.01.otf";
pub const FALLBACK_FONT_FAMILY: &str = "Unifont-JP";

/// 文字の大きさの既定(ピクセル)。これより小さい大きさは、既定では使わない。
pub const DEFAULT_SIZE: f32 = 14.0;
/// 字形のヒンティングを入れる、いちばん小さい大きさ(ピクセル)。これより小さいと、字の形が崩れる
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
    Unusable { path: PathBuf, expected: String },
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

/// 文字を描く先。1ピクセルは4バイトで、並びは B, G, R, X(`seinas-render` の描画結果と同じ)。
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
    /// 文字の大きさ(ピクセル)。
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
    /// 描いた文字の幅(ピクセル)。
    pub width: i32,
    /// 収まらずに、末尾を切ったか。
    pub truncated: bool,
    /// 控えのフォントで描いた字形の数。
    pub fallback_glyphs: usize,
    /// どのフォントにも無く、四角の枠で描いた字形の数。
    pub missing_glyphs: usize,
}

/// 主のフォントに無い文字を、どのフォントで探すか。フォールバックのフォントを、順に、どの文字の種類にも使う。
struct ListFallback(Vec<&'static str>);

impl Fallback for ListFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &self.0
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
    /// グリフの絵を作るもの(覚えは使わない)と、上限のある覚え。
    swash: SwashCache,
    glyphs: GlyphCache,
    /// 行を組むときに、最初に使うフォントの名前。主のフォントが無ければ、控えのフォント。
    family: &'static str,
    /// フォールバックのフォントの名前(順)。
    fallback: Vec<&'static str>,
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
        Self::load_faces(
            (&files.ui, UI_FONT_FAMILY),
            &[(&files.fallback, FALLBACK_FONT_FAMILY)],
        )
    }

    /// 設定 `setup` で選んだフォントを読む(ADR 0003)。ログに出すこと(設定の読めなかった行、見つからなかった
    /// フォント、読めなかったフォント)を、一緒に返す。
    ///
    /// 設定のファイルが無ければ、[`TextPainter::load`] で同梱のフォントを読んだときと同じになる。
    pub fn load_setup(setup: &FontSetup) -> (Self, Vec<String>) {
        let env = fonts::Env::from_process();
        let config_file = setup.config_file.clone().or_else(|| env.config_file());
        let (config, config_problems) = match &config_file {
            Some(path) => fonts::read_config(path),
            None => (fonts::FontConfig::default(), Vec::new()),
        };
        let resolved = fonts::resolve(&setup.bundled_dir, &config, &env);
        let mut messages: Vec<String> = config_problems.iter().map(|p| p.to_string()).collect();
        messages.extend(resolved.notes.iter().cloned());
        let fallback: Vec<(&Path, &str)> = resolved
            .fallback
            .iter()
            .map(|picked| (picked.path.as_path(), picked.family.as_str()))
            .collect();
        let (painter, problems) =
            Self::load_faces((&resolved.ui.path, &resolved.ui.family), &fallback);
        messages.extend(problems.iter().map(|p| p.to_string()));
        (painter, messages)
    }

    /// UI のフォント `ui` と、フォールバックのフォント `fallback`(順)を読む。
    fn load_faces(ui: (&Path, &str), fallback: &[(&Path, &str)]) -> (Self, Vec<FontProblem>) {
        let mut db = fontdb::Database::new();
        let mut problems = Vec::new();
        let mut loaded: Vec<&'static str> = Vec::new();
        for &(path, family) in std::iter::once(&ui).chain(fallback) {
            match load_font(&mut db, path, family, true) {
                // cosmic-text は、フォールバックの名前を 'static で持つので、プロセスの間は残しておく
                // (役ごとに 1 つずつで、数は少ない)。
                Ok(()) => loaded.push(leak(family)),
                Err(problem) => problems.push(problem),
            }
        }
        let ui_loaded = loaded.first() == Some(&leak(ui.1));
        let fallback_names: Vec<&'static str> = if ui_loaded {
            loaded[1..].to_vec()
        } else {
            loaded.clone()
        };
        let fonts = loaded.first().map(|&family| Fonts {
            system: FontSystem::new_with_locale_and_db_and_fallback(
                LOCALE.to_owned(),
                db,
                ListFallback(fallback_names.clone()),
            ),
            swash: SwashCache::new(),
            glyphs: GlyphCache::new(),
            family,
            fallback: fallback_names,
        });
        (TextPainter { fonts }, problems)
    }

    /// フォントを1つも持たないもの。何も描かない。
    pub fn without_fonts() -> Self {
        TextPainter { fonts: None }
    }

    /// 使っているフォントの名前(ログに出す形)。例:`ui "BIZ UDPGothic", fallback "Unifont-JP"`。
    pub fn describe(&self) -> String {
        match &self.fonts {
            None => "no fonts".to_owned(),
            Some(fonts) => format!(
                "ui \"{}\", fallback {}",
                fonts.family,
                fonts
                    .fallback
                    .iter()
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
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
                let Fonts {
                    system,
                    swash,
                    glyphs,
                    ..
                } = fonts;
                let image = glyphs.get_or_insert_with(physical.cache_key, || {
                    swash.get_image_uncached(system, physical.cache_key)
                });
                if let Some(image) = image {
                    blend_glyph(canvas, image, physical.x, physical.y, line.color);
                }
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
        // 字形をピクセルの境目に合わせて置く(小数の位置には置かない)。
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

/// グリフの絵を、(x, y)を基準に、色 `color` で重ねる。濃さは、絵の濃さ(カラーの絵なら、その透明度)。
fn blend_glyph(
    canvas: &mut Canvas<'_>,
    image: &cosmic_text::SwashImage,
    x: i32,
    y: i32,
    color: [u8; 3],
) {
    let left = x + image.placement.left;
    let top = y - image.placement.top;
    let (width, height) = (image.placement.width as i32, image.placement.height as i32);
    let step = match image.content {
        SwashContent::Mask => 1,
        SwashContent::Color => 4,
        // cosmic-text も描かない形(swash に頼まないので、来ない)。
        SwashContent::SubpixelMask => return,
    };
    for row in 0..height {
        for column in 0..width {
            let at = ((row * width + column) as usize) * step + (step - 1);
            if let Some(&alpha) = image.data.get(at) {
                canvas.blend(left + column, top + row, color, alpha);
            }
        }
    }
}

/// 名前を、プロセスの間ずっと残る形にする(同じ名前は 1 度だけ)。
fn leak(name: &str) -> &'static str {
    use std::sync::Mutex;
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().expect("the names");
    if let Some(&kept) = names.iter().find(|&&kept| kept == name) {
        return kept;
    }
    let kept: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(kept);
    kept
}

/// 同梱のフォントの置き場と、設定のファイル。[`TextPainter::load_setup`] に渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSetup {
    /// 同梱のフォントの置き場(今までの `--fonts`、`SEINAS_FONTS`)。
    pub bundled_dir: PathBuf,
    /// 設定のファイル(`--font-config`)。Noneなら、`$XDG_CONFIG_HOME/seinas/fonts.conf`(無ければ
    /// `~/.config/seinas/fonts.conf`)。
    pub config_file: Option<PathBuf>,
}

/// どのフォントにも無い文字の代わりに、四角の枠を描く。
fn draw_missing_box(canvas: &mut Canvas<'_>, x: f32, width: f32, baseline: f32, line: Line) {
    // 幅は字形の送りに合わせ、高さは大文字の高さくらいにする。字と字の間に、1ピクセルずつ空きを残す。
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
///
/// `mmap` なら、ファイルを mmap で読む(描くときに触ったページだけがメモリーに乗り、ページキャッシュは
/// プロセスの間で共有される)。mmap できなければ、ヒープへ丸ごと読む。
fn load_font(
    db: &mut fontdb::Database,
    path: &Path,
    family: &str,
    mmap: bool,
) -> Result<(), FontProblem> {
    let unreadable = |error| FontProblem::Unreadable {
        path: path.to_owned(),
        error,
    };
    std::fs::metadata(path).map_err(unreadable)?;
    if mmap {
        let added = db.load_font_source(fontdb::Source::File(path.to_owned()));
        if keep_family(db, &added, family) {
            // いま mmap しておく(cosmic-text も、使うときに同じことをする)。できなければ、ヒープへ読む。
            // SAFETY: mmap したファイルが、使っている間に書き換えられると、壊れた内容を読むことになる。
            // フォントの置き場のファイルは、動いている間に書き換えないものとする(fontconfig を使うアプリと同じ)。
            let mapped = added
                .iter()
                .all(|&id| unsafe { db.make_shared_face_data(id) }.is_some());
            if mapped {
                return Ok(());
            }
            for &id in &added {
                db.remove_face(id);
            }
        }
    }
    let data = std::fs::read(path).map_err(unreadable)?;
    // 壊れたファイルは、一覧に入らないだけで、失敗は返ってこない。入ったものを見て確かめる。
    let added = db.load_font_source(fontdb::Source::Binary(std::sync::Arc::new(data)));
    if keep_family(db, &added, family) {
        return Ok(());
    }
    Err(FontProblem::Unusable {
        path: path.to_owned(),
        expected: family.to_owned(),
    })
}

/// 入れた face `added` の中に、`family` という名前のものがあればtrue。無ければ、入れたものを外してfalse。
fn keep_family(db: &mut fontdb::Database, added: &[fontdb::ID], family: &str) -> bool {
    let usable = added.iter().any(|&id| {
        db.face(id)
            .is_some_and(|face| face.families.iter().any(|(name, _)| name == family))
    });
    if !usable {
        for &id in added {
            db.remove_face(id);
        }
    }
    usable
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テストのフォントの置き場(`tools/fetch-fonts.sh` が置く)。無ければ、テストを飛ばす。
    fn font_dir() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts");
        dir.join(UI_FONT_FILE).exists().then_some(dir)
    }

    #[test]
    fn a_font_is_read_by_mmap_or_into_the_heap() {
        let Some(dir) = font_dir() else {
            return;
        };
        let path = dir.join(UI_FONT_FILE);
        for mmap in [true, false] {
            let mut db = fontdb::Database::new();
            load_font(&mut db, &path, UI_FONT_FAMILY, mmap).expect("the font");
            let face = db.faces().next().expect("a face");
            let mapped = matches!(face.source, fontdb::Source::SharedFile(..));
            assert_eq!(mapped, mmap, "mmap {mmap}");
        }
        // 別の名前を求めると、使えない。
        let mut db = fontdb::Database::new();
        assert!(matches!(
            load_font(&mut db, &path, "Another", true),
            Err(FontProblem::Unusable { .. })
        ));
        assert_eq!(db.faces().count(), 0);
        assert!(matches!(
            load_font(&mut db, &dir.join("missing.ttf"), UI_FONT_FAMILY, true),
            Err(FontProblem::Unreadable { .. })
        ));
    }
}
