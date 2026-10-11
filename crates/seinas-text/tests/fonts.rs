//! 利用者が選んだフォント(ADR 0003)の、設定の読み方・役の選び方・置き場の順を、テスト用の一時ディレクトリに
//! 同梱のフォントを置いて確かめる。ホストのシステムのフォントには左右されない(`default-dirs = no` で動かす)。
//!
//! 同梱のフォントは `tools/fetch-fonts.sh` が `target/fonts` に置くもの。無ければ、失敗にする。

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use seinas_text::{
    fonts::{self, Env},
    Canvas, FontFiles, FontSetup, Line, TextPainter, FALLBACK_FONT_FAMILY, FALLBACK_FONT_FILE,
    UI_FONT_FAMILY, UI_FONT_FILE,
};

const MONOSPACE_FILE: &str = fonts::MONOSPACE_FONT_FILE;
const MONOSPACE_FAMILY: &str = fonts::MONOSPACE_FONT_FAMILY;

fn font_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts");
    assert!(
        dir.join(MONOSPACE_FILE).exists(),
        "the fonts are missing; run tools/fetch-fonts.sh first"
    );
    dir
}

/// テストごとの一時ディレクトリ。
struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "seinas-fonts-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("a temporary directory");
        Temp(dir)
    }

    /// 下に `name` のディレクトリを作り、同梱のフォント `files` をコピーする。
    fn dir_with(&self, name: &str, files: &[&str]) -> PathBuf {
        let dir = self.0.join(name);
        fs::create_dir_all(&dir).expect("a directory");
        for file in files {
            fs::copy(font_dir().join(file), dir.join(file)).expect("copy a font");
        }
        dir
    }

    fn config(&self, text: &str) -> PathBuf {
        let path = self.0.join("fonts.conf");
        fs::write(&path, text).expect("write the config");
        path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `text` を 14 px で描いたときの幅と、ピクセル。
fn draw(painter: &mut TextPainter, text: &str) -> (i32, Vec<u8>) {
    let (width, height) = (300, 24);
    let mut pixels = vec![0u8; width * height * 4];
    let drawn = painter.draw_line(
        &mut Canvas::new(&mut pixels, width, height),
        text,
        Line {
            size: 14.0,
            color: [255, 255, 255],
            x: 0,
            top: 0,
            height: height as i32,
            max_width: width as i32,
        },
    );
    (drawn.width, pixels)
}

#[test]
fn without_a_config_file_the_bundled_fonts_draw_the_same_pixels() {
    let temp = Temp::new();
    let bundled = temp.dir_with("bundled", &[UI_FONT_FILE, FALLBACK_FONT_FILE]);
    let (mut painter, messages) = TextPainter::load_setup(&FontSetup {
        bundled_dir: bundled.clone(),
        config_file: Some(temp.0.join("missing.conf")),
    });
    assert!(messages.is_empty(), "{messages:?}");
    let (mut direct, _) = TextPainter::load(&FontFiles::in_dir(&bundled));
    for text in ["Seinas", "日本語のタイトル ═"] {
        assert_eq!(draw(&mut painter, text), draw(&mut direct, text), "{text}");
    }
}

#[test]
fn a_named_font_in_an_added_dir_draws_the_ui() {
    let temp = Temp::new();
    let bundled = temp.dir_with("bundled", &[UI_FONT_FILE, FALLBACK_FONT_FILE]);
    let extra = temp.dir_with("extra", &[MONOSPACE_FILE]);
    let config = temp.config(&format!(
        "ui = {MONOSPACE_FAMILY}\ndefault-dirs = no\ndir = {}\n",
        extra.display()
    ));
    let (config_read, problems) = fonts::read_config(&config);
    assert!(problems.is_empty());
    let resolved = fonts::resolve(&bundled, &config_read, &Env::default());
    assert!(resolved.scanned);
    assert_eq!(resolved.ui.family, MONOSPACE_FAMILY);
    assert_eq!(resolved.ui.path, extra.join(MONOSPACE_FILE));
    assert!(resolved.notes.is_empty(), "{:?}", resolved.notes);

    // 描くと、等幅のフォント(半角 7 px)になる。同梱の UI のフォント(プロポーショナル)とは幅が違う。
    let (mut painter, messages) = TextPainter::load_setup(&FontSetup {
        bundled_dir: bundled.clone(),
        config_file: Some(config),
    });
    assert!(messages.is_empty(), "{messages:?}");
    let (mut default, _) = TextPainter::load(&FontFiles::in_dir(&bundled));
    let (chosen_width, _) = draw(&mut painter, "iiii");
    let (default_width, _) = draw(&mut default, "iiii");
    assert_eq!(chosen_width, 28);
    assert!(default_width < chosen_width, "{default_width}");
}

#[test]
fn a_later_dir_wins() {
    let temp = Temp::new();
    let bundled = temp.dir_with("bundled", &[UI_FONT_FILE, FALLBACK_FONT_FILE]);
    let first = temp.dir_with("first", &[MONOSPACE_FILE]);
    let second = temp.dir_with("second", &[MONOSPACE_FILE]);
    let (config, problems) = fonts::parse_config(&format!(
        "ui = {MONOSPACE_FAMILY}\ndefault-dirs = no\ndir = {}\ndir = {}\n",
        first.display(),
        second.display()
    ));
    assert!(problems.is_empty());
    let resolved = fonts::resolve(&bundled, &config, &Env::default());
    assert_eq!(resolved.ui.path, second.join(MONOSPACE_FILE));
    // 同梱の置き場にあるものより、足した置き場のものが勝つ。
    let bundled_too = temp.dir_with(
        "bundled-too",
        &[UI_FONT_FILE, FALLBACK_FONT_FILE, MONOSPACE_FILE],
    );
    let resolved = fonts::resolve(&bundled_too, &config, &Env::default());
    assert_eq!(resolved.ui.path, second.join(MONOSPACE_FILE));
}

#[test]
fn default_dirs_no_leaves_out_the_system_and_the_user_dirs() {
    let temp = Temp::new();
    let bundled = temp.dir_with("bundled", &[UI_FONT_FILE, FALLBACK_FONT_FILE]);
    // 利用者の置き場に等幅のフォントを置いても、default-dirs = no なら見ない。
    let data = temp.0.join("data");
    let user = data.join("fonts");
    fs::create_dir_all(&user).expect("the user dir");
    fs::copy(font_dir().join(MONOSPACE_FILE), user.join(MONOSPACE_FILE)).expect("copy");
    let env = Env {
        home: None,
        xdg_config_home: None,
        xdg_data_home: Some(data),
    };
    let (config, _) = fonts::parse_config(&format!("ui = {MONOSPACE_FAMILY}\ndefault-dirs = no\n"));
    let resolved = fonts::resolve(&bundled, &config, &env);
    assert_eq!(
        resolved.ui.family, UI_FONT_FAMILY,
        "fell back to the default"
    );
    assert_eq!(resolved.notes.len(), 1, "{:?}", resolved.notes);
    // default-dirs が yes(既定)なら、利用者の置き場から見つかる。
    let (config, _) = fonts::parse_config(&format!("ui = {MONOSPACE_FAMILY}\n"));
    let resolved = fonts::resolve(&bundled, &config, &env);
    assert_eq!(resolved.ui.path, user.join(MONOSPACE_FILE));
}

#[test]
fn the_fallback_ends_with_unifont_and_bad_lines_are_reported() {
    let temp = Temp::new();
    let bundled = temp.dir_with("bundled", &[UI_FONT_FILE, FALLBACK_FONT_FILE]);
    let extra = temp.dir_with("extra", &[MONOSPACE_FILE]);
    let config = temp.config(&format!(
        "fallback = {MONOSPACE_FAMILY}, {FALLBACK_FONT_FAMILY}, No Such Font\n\
         default-dirs = no\n\
         dir = {}\n\
         this is not a setting\n\
         ui-weight = heavy\n",
        extra.display()
    ));
    let (config_read, problems) = fonts::read_config(&config);
    assert_eq!(problems.iter().map(|p| p.line).collect::<Vec<_>>(), [4, 5]);
    let resolved = fonts::resolve(&bundled, &config_read, &Env::default());
    let fallback: Vec<&str> = resolved
        .fallback
        .iter()
        .map(|p| p.family.as_str())
        .collect();
    assert_eq!(fallback, [MONOSPACE_FAMILY, FALLBACK_FONT_FAMILY]);
    assert_eq!(resolved.fallback[1].path, bundled.join(FALLBACK_FONT_FILE));

    // 読めない行と、見つからないフォントは、ログに出すことのリストに入る。描くのは止まらない。
    let (mut painter, messages) = TextPainter::load_setup(&FontSetup {
        bundled_dir: bundled,
        config_file: Some(config),
    });
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("line 4"), "{messages:?}");
    assert!(messages[2].contains("No Such Font"), "{messages:?}");
    assert!(painter.can_draw());
    assert!(draw(&mut painter, "タイトル").0 > 0);
}
