//! フォントの置き場・一覧・設定のファイル・役ごとの選び方(ADR 0003)。
//!
//! fontdb を使うのは、この module と、描く所(`TextPainter` の中)だけ。ここから外へ出す型は、Seinas の型だけ。
//! 置き場を調べて一覧を作るのは、設定のファイルがフォントや置き場を指すときだけ。設定が無ければ、同梱の
//! フォントだけを読む(今までと同じ形)。

use std::{
    collections::HashSet,
    fmt,
    path::{Path, PathBuf},
};

use cosmic_text::fontdb;

use crate::{FALLBACK_FONT_FAMILY, UI_FONT_FAMILY};

/// 端末・コード用のフォント(同梱のもの)のファイルの名前と、その中のフォントの名前。
pub const MONOSPACE_FONT_FILE: &str = "BIZUDGothic-Regular.ttf";
pub const MONOSPACE_FONT_FAMILY: &str = "BIZ UDGothic";
/// システムのフォントの置き場。
pub const SYSTEM_FONT_DIR: &str = "/usr/share/fonts";
/// 1 ファイルの大きさの上限。これを越えるファイルは、一覧に入れない。
pub const MAX_FONT_FILE: u64 = 64 * 1024 * 1024;
/// 下のディレクトリを探す深さの上限。
const MAX_DEPTH: usize = 8;
/// 太さの既定(CSS の normal)。
pub const DEFAULT_WEIGHT: u16 = 400;

/// 役の指定(family の名前と太さ)。名前が無ければ、その役の既定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub family: Option<String>,
    pub weight: u16,
}

impl Default for Choice {
    fn default() -> Self {
        Choice {
            family: None,
            weight: DEFAULT_WEIGHT,
        }
    }
}

/// 設定のファイルの内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontConfig {
    pub ui: Choice,
    pub monospace: Choice,
    /// 無い文字のフォールバックの family の名前(順)。この後に、いつも同梱の GNU Unifont JP を足す。
    pub fallback: Vec<String>,
    /// 足す置き場。
    pub dirs: Vec<PathBuf>,
    /// システムと利用者の置き場を使うか(`default-dirs = no` で使わない)。
    pub default_dirs: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig {
            ui: Choice::default(),
            monospace: Choice::default(),
            fallback: Vec::new(),
            dirs: Vec::new(),
            default_dirs: true,
        }
    }
}

impl FontConfig {
    /// 置き場を調べる必要があるか(フォントの名前か、置き場を指しているか)。
    fn needs_scan(&self) -> bool {
        self.ui.family.is_some()
            || self.monospace.family.is_some()
            || !self.fallback.is_empty()
            || !self.dirs.is_empty()
    }
}

/// 設定のファイルの、読めなかった行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigProblem {
    /// 行の番号(1 から)。ファイルそのものを読めなかったときは 0。
    pub line: usize,
    pub reason: String,
}

impl fmt::Display for ConfigProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "font config: {}", self.reason)
        } else {
            write!(f, "font config, line {}: {}", self.line, self.reason)
        }
    }
}

/// 設定のファイルの内容を読む。1 行に `キー = 値`、`#` から行の終わりまではコメント。
///
/// 読めない行、知らないキー、範囲の外の値は、飛ばして、理由を返す。
pub fn parse_config(text: &str) -> (FontConfig, Vec<ConfigProblem>) {
    let mut config = FontConfig::default();
    let mut problems = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let mut problem = |reason: String| problems.push(ConfigProblem { line, reason });
        let Some((key, value)) = content.split_once('=') else {
            problem(format!("not a \"key = value\" line: {content}"));
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            problem(format!("{key} has no value"));
            continue;
        }
        let weight = |value: &str| {
            value
                .parse::<u16>()
                .ok()
                .filter(|w| (100..=900).contains(w))
                .ok_or_else(|| format!("{key} must be from 100 to 900: {value}"))
        };
        match key {
            "ui" => config.ui.family = Some(value.to_owned()),
            "monospace" => config.monospace.family = Some(value.to_owned()),
            "ui-weight" => match weight(value) {
                Ok(w) => config.ui.weight = w,
                Err(reason) => problem(reason),
            },
            "monospace-weight" => match weight(value) {
                Ok(w) => config.monospace.weight = w,
                Err(reason) => problem(reason),
            },
            "fallback" => config.fallback.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            ),
            "dir" => config.dirs.push(PathBuf::from(value)),
            "default-dirs" => match value {
                "yes" => config.default_dirs = true,
                "no" => config.default_dirs = false,
                _ => problem(format!("default-dirs must be yes or no: {value}")),
            },
            _ => problem(format!("unknown key: {key}")),
        }
    }
    (config, problems)
}

/// 設定のファイルを読む。ファイルが無ければ、既定の設定(問題なし)。読めなければ、既定の設定と、その理由。
pub fn read_config(path: &Path) -> (FontConfig, Vec<ConfigProblem>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (FontConfig::default(), Vec::new()),
        Err(e) => (
            FontConfig::default(),
            vec![ConfigProblem {
                line: 0,
                reason: format!("cannot read {}: {e}", path.display()),
            }],
        ),
    }
}

/// 置き場と設定のファイルを決めるのに使う、環境変数の値。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
}

impl Env {
    /// このプロセスの環境変数から読む。空の値は、無いものとして扱う。
    pub fn from_process() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Env {
            home: var("HOME"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            xdg_data_home: var("XDG_DATA_HOME"),
        }
    }

    /// 設定のファイルの既定の場所(`$XDG_CONFIG_HOME/seinas/fonts.conf`、無ければ `~/.config/seinas/fonts.conf`)。
    pub fn config_file(&self) -> Option<PathBuf> {
        let base = self
            .xdg_config_home
            .clone()
            .or_else(|| self.home.as_ref().map(|home| home.join(".config")))?;
        Some(base.join("seinas").join("fonts.conf"))
    }

    /// 利用者の置き場(`$XDG_DATA_HOME/fonts`、無ければ `~/.local/share/fonts`)。
    pub fn user_font_dir(&self) -> Option<PathBuf> {
        let base = self.xdg_data_home.clone().or_else(|| {
            self.home
                .as_ref()
                .map(|home| home.join(".local").join("share"))
        })?;
        Some(base.join("fonts"))
    }
}

/// 置き場のリスト(探す順。同じ名前のフォントは、後の置き場のものを使う)。
pub fn search_dirs(bundled: &Path, config: &FontConfig, env: &Env) -> Vec<PathBuf> {
    let mut dirs = vec![bundled.to_owned()];
    if config.default_dirs {
        dirs.push(PathBuf::from(SYSTEM_FONT_DIR));
        dirs.extend(env.user_font_dir());
    }
    dirs.extend(config.dirs.iter().cloned());
    dirs
}

/// 一覧の 1 項目(1 つの face)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEntry {
    pub path: PathBuf,
    /// TTC・OTC の中の何番目か。
    pub index: u32,
    /// フォントが持つ family の名前(全部の言語)。
    pub families: Vec<String>,
    pub weight: u16,
    pub italic: bool,
    pub monospaced: bool,
    /// 何番目の置き場にあったか(0 が同梱)。大きいほうが勝つ。
    pub rank: usize,
}

/// 置き場 `dirs` を順に調べて、一覧を作る。読めないディレクトリやファイルは、飛ばす。大きすぎて飛ばした
/// ファイルは、ログに出すことのリストにして返す。
pub fn scan(dirs: &[PathBuf]) -> (Vec<FontEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut notes = Vec::new();
    let mut seen = HashSet::new();
    for (rank, dir) in dirs.iter().enumerate() {
        let mut files = Vec::new();
        collect_files(dir, 0, &mut seen, &mut files, &mut notes);
        files.sort();
        for path in files {
            entries.extend(faces_in(&path, rank));
        }
    }
    (entries, notes)
}

/// `dir` の下のフォントのファイルを集める(深さ `MAX_DEPTH` まで。同じディレクトリは 2 度見ない)。
fn collect_files(
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    files: &mut Vec<PathBuf>,
    notes: &mut Vec<String>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !seen.insert(canonical) {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        // シンボリックリンクは、たどった先の種類で見る。
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            collect_files(&path, depth + 1, seen, files, notes);
        } else if metadata.is_file() && is_font_file(&path) {
            if metadata.len() > MAX_FONT_FILE {
                notes.push(format!(
                    "{} is larger than {} MiB; not listed",
                    path.display(),
                    MAX_FONT_FILE / 1024 / 1024
                ));
                continue;
            }
            files.push(path);
        }
    }
}

/// 拡張子が `.ttf`・`.otf`・`.ttc`・`.otc` か(大文字と小文字は区別しない)。
fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
}

/// ファイル `path` の中の face を、一覧の項目にする。
fn faces_in(path: &Path, rank: usize) -> Vec<FontEntry> {
    let mut db = fontdb::Database::new();
    // memmap の機能があるので、読むのは mmap で、要る所だけ。
    if db.load_font_file(path).is_err() {
        return Vec::new();
    }
    db.faces()
        .map(|face| FontEntry {
            path: path.to_owned(),
            index: face.index,
            families: face.families.iter().map(|(name, _)| name.clone()).collect(),
            weight: face.weight.0,
            italic: face.style != fontdb::Style::Normal,
            monospaced: face.monospaced,
            rank,
        })
        .collect()
}

/// 一覧 `entries` から、family が `family`(大文字と小文字を区別しない)のフォントを、太さ `weight` で選ぶ。
///
/// 名前が合うもののうち、いちばん後の置き場のものから、CSS のフォントの選び方で太さを決める(斜体でないものを
/// 先に見る)。
pub fn choose<'a>(entries: &'a [FontEntry], family: &str, weight: u16) -> Option<&'a FontEntry> {
    let named: Vec<&FontEntry> = entries
        .iter()
        .filter(|entry| {
            entry
                .families
                .iter()
                .any(|name| name.to_lowercase() == family.to_lowercase())
        })
        .collect();
    let rank = named.iter().map(|entry| entry.rank).max()?;
    let candidates: Vec<&FontEntry> = named
        .into_iter()
        .filter(|entry| entry.rank == rank)
        .collect();
    let upright: Vec<&FontEntry> = candidates
        .iter()
        .copied()
        .filter(|entry| !entry.italic)
        .collect();
    let pool = if upright.is_empty() {
        candidates
    } else {
        upright
    };
    pool.into_iter()
        .min_by_key(|entry| weight_order(weight, entry.weight))
}

/// CSS のフォントの選び方での、太さの近さ(小さいほうを選ぶ)。
fn weight_order(wanted: u16, actual: u16) -> (u8, u16) {
    let (wanted, actual) = (wanted as i32, actual as i32);
    let distance = (actual - wanted).unsigned_abs() as u16;
    let group = if actual == wanted {
        0
    } else if (400..=500).contains(&wanted) {
        // 400〜500:まず wanted から 500 まで太いほう、次に細いほう、最後に 500 より太いほう。
        if actual > wanted && actual <= 500 {
            1
        } else if actual < wanted {
            2
        } else {
            3
        }
    } else if wanted < 400 {
        // 細い指定:まず細いほう、次に太いほう。
        if actual < wanted {
            1
        } else {
            2
        }
    } else if actual > wanted {
        // 太い指定:まず太いほう、次に細いほう。
        1
    } else {
        2
    };
    (group, distance)
}

/// 役ごとに選んだフォント。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub path: PathBuf,
    pub index: u32,
    /// 描くときに使う family の名前(フォントが持つ名前のうち、最初のもの)。
    pub family: String,
}

impl Picked {
    fn from(entry: &FontEntry) -> Self {
        Picked {
            path: entry.path.clone(),
            index: entry.index,
            family: entry.families.first().cloned().unwrap_or_default(),
        }
    }
}

/// 役ごとに選んだ結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub ui: Picked,
    pub monospace: Picked,
    /// 無い文字のフォールバック(順)。最後は、いつも同梱の GNU Unifont JP。
    pub fallback: Vec<Picked>,
    /// 置き場を調べたか(設定がフォントか置き場を指していたか)。
    pub scanned: bool,
    /// 見つからなかった指定など(ログに出すもの)。
    pub notes: Vec<String>,
}

/// 同梱のフォント(既定の役のフォント)。名前で決まっているので、置き場を調べずに作る。
fn bundled(dir: &Path) -> (Picked, Picked, Picked) {
    let pick = |file: &str, family: &str| Picked {
        path: dir.join(file),
        index: 0,
        family: family.to_owned(),
    };
    (
        pick(crate::UI_FONT_FILE, UI_FONT_FAMILY),
        pick(MONOSPACE_FONT_FILE, MONOSPACE_FONT_FAMILY),
        pick(crate::FALLBACK_FONT_FILE, FALLBACK_FONT_FAMILY),
    )
}

/// 役ごとのフォントを決める。
///
/// 設定がフォントの名前か置き場を指していなければ、置き場を調べず、同梱のフォントを使う。指していれば、
/// 置き場を全部調べて選ぶ。名前の指定が見つからなければ、その役の既定(同梱)を使い、`notes` に書く。
pub fn resolve(bundled_dir: &Path, config: &FontConfig, env: &Env) -> Resolved {
    let (ui_default, mono_default, last_fallback) = bundled(bundled_dir);
    let mut resolved = Resolved {
        ui: ui_default.clone(),
        monospace: mono_default.clone(),
        fallback: vec![last_fallback.clone()],
        scanned: false,
        notes: Vec::new(),
    };
    if !config.needs_scan() {
        return resolved;
    }
    let (entries, mut notes) = scan(&search_dirs(bundled_dir, config, env));
    resolved.scanned = true;
    let pick = |choice: &Choice, default: &Picked, role: &str, notes: &mut Vec<String>| {
        let Some(family) = &choice.family else {
            return default.clone();
        };
        match choose(&entries, family, choice.weight) {
            Some(entry) => Picked::from(entry),
            None => {
                notes.push(format!(
                    "the {role} font \"{family}\" was not found; using \"{}\"",
                    default.family
                ));
                default.clone()
            }
        }
    };
    resolved.ui = pick(&config.ui, &ui_default, "ui", &mut notes);
    resolved.monospace = pick(&config.monospace, &mono_default, "monospace", &mut notes);
    let mut fallback = Vec::new();
    for family in &config.fallback {
        match choose(&entries, family, DEFAULT_WEIGHT) {
            Some(entry) => fallback.push(Picked::from(entry)),
            None => notes.push(format!("the fallback font \"{family}\" was not found")),
        }
    }
    // いつも、最後は同梱の GNU Unifont JP(利用者が消せない、最後のフォールバック)。
    fallback.retain(|picked| picked.family != FALLBACK_FONT_FAMILY);
    fallback.push(last_fallback);
    resolved.fallback = fallback;
    resolved.notes = notes;
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(family: &str, weight: u16, italic: bool, rank: usize) -> FontEntry {
        FontEntry {
            path: PathBuf::from(format!("/{rank}/{family}-{weight}.ttf")),
            index: 0,
            families: vec![family.to_owned()],
            weight,
            italic,
            monospaced: false,
            rank,
        }
    }

    #[test]
    fn the_config_is_key_value_lines_and_bad_lines_are_skipped() {
        let text = "# Seinas のフォントの設定\n\
                    ui = Noto Sans JP   # コメント\n\
                    ui-weight = 500\n\
                    monospace = BIZ UDGothic\n\
                    fallback = A, B ,, C\n\
                    fallback = D\n\
                    dir = /home/user/fonts\n\
                    default-dirs = no\n\
                    \n\
                    nonsense\n\
                    color = red\n\
                    monospace-weight = 1000\n\
                    ui =\n\
                    default-dirs = maybe\n";
        let (config, problems) = parse_config(text);
        assert_eq!(config.ui.family.as_deref(), Some("Noto Sans JP"));
        assert_eq!(config.ui.weight, 500);
        assert_eq!(config.monospace.family.as_deref(), Some("BIZ UDGothic"));
        assert_eq!(config.monospace.weight, DEFAULT_WEIGHT);
        assert_eq!(config.fallback, ["A", "B", "C", "D"]);
        assert_eq!(config.dirs, [PathBuf::from("/home/user/fonts")]);
        assert!(!config.default_dirs);
        let lines: Vec<usize> = problems.iter().map(|p| p.line).collect();
        assert_eq!(lines, [10, 11, 12, 13, 14]);
    }

    #[test]
    fn the_config_file_and_the_user_dir_come_from_the_environment() {
        let env = Env {
            home: Some("/home/u".into()),
            xdg_config_home: None,
            xdg_data_home: Some("/data".into()),
        };
        assert_eq!(
            env.config_file(),
            Some(PathBuf::from("/home/u/.config/seinas/fonts.conf"))
        );
        assert_eq!(env.user_font_dir(), Some(PathBuf::from("/data/fonts")));
        assert_eq!(Env::default().config_file(), None);
        // 置き場の順:同梱、システム、利用者、設定の dir。default-dirs = no なら、真ん中の 2 つを使わない。
        let mut config = FontConfig {
            dirs: vec!["/extra".into()],
            ..FontConfig::default()
        };
        let dirs = search_dirs(Path::new("/bundled"), &config, &env);
        assert_eq!(
            dirs,
            [
                PathBuf::from("/bundled"),
                PathBuf::from("/usr/share/fonts"),
                PathBuf::from("/data/fonts"),
                PathBuf::from("/extra")
            ]
        );
        config.default_dirs = false;
        assert_eq!(
            search_dirs(Path::new("/bundled"), &config, &env),
            [PathBuf::from("/bundled"), PathBuf::from("/extra")]
        );
    }

    #[test]
    fn a_later_dir_wins_and_the_weight_is_chosen_like_css() {
        let entries = vec![
            entry("Sans", 400, false, 0),
            entry("Sans", 400, false, 2),
            entry("Sans", 700, false, 2),
            entry("Sans", 300, false, 2),
            entry("Sans", 400, true, 2),
            entry("Other", 400, false, 3),
        ];
        // 後の置き場(2)のものが勝つ。名前は、大文字と小文字を区別しない。
        assert_eq!(
            choose(&entries, "sans", 400).unwrap().path,
            PathBuf::from("/2/Sans-400.ttf")
        );
        assert_eq!(choose(&entries, "Sans", 700).unwrap().weight, 700);
        // 600(太い指定)は、太いほう(700)が先。350(細い指定)は、細いほう(300)が先。
        assert_eq!(choose(&entries, "Sans", 600).unwrap().weight, 700);
        assert_eq!(choose(&entries, "Sans", 350).unwrap().weight, 300);
        // 450 は、500 までの太いほうが無いので、細いほう(400)。
        assert_eq!(choose(&entries, "Sans", 450).unwrap().weight, 400);
        assert!(!choose(&entries, "Sans", 400).unwrap().italic);
        assert!(choose(&entries, "Missing", 400).is_none());
    }

    #[test]
    fn without_a_config_the_bundled_fonts_are_used_without_scanning() {
        let resolved = resolve(
            Path::new("/bundled"),
            &FontConfig::default(),
            &Env::default(),
        );
        assert!(!resolved.scanned);
        assert_eq!(resolved.ui.family, UI_FONT_FAMILY);
        assert_eq!(
            resolved.ui.path,
            PathBuf::from("/bundled/BIZUDPGothic-Regular.ttf")
        );
        assert_eq!(resolved.monospace.family, MONOSPACE_FONT_FAMILY);
        assert_eq!(resolved.fallback.len(), 1);
        assert_eq!(resolved.fallback[0].family, FALLBACK_FONT_FAMILY);
    }
}
