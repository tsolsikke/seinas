//! トーチのメニューに出すアプリの一覧(`.desktop` のファイル。Wayland に触れない部分)。
//!
//! freedesktop.org の Desktop Entry の仕様(1.5)のうち、決めたキーだけを読む:`Type`(Application だけ)、`Name`
//! (`Name[ja]` などの国ごとの値を含む)、`Exec`、`Icon`、`Hidden`、`NoDisplay`、`Terminal`、`TryExec`、`Path`。
//! 対応しない指定は、黙って無視して違う条件で起こすことがないよう、その項目を一覧から外し、理由を返す
//! (`Terminal=true`、`DBusActivatable=true` だけで `Exec` が無いもの、Exec の中の知らないフィールドコード、
//! 引用していないシェルの記号など)。
//!
//! Exec は、シェルへ渡さない。仕様の書き方(二重引用符とバックスラッシュ、フィールドコード)に従って、実行ファイルと
//! 引数の列に直す。開くファイルや URL は渡さないので、`%f`・`%F`・`%u`・`%U` は消す。決まりの説明は
//! `docs/taskbar.md` の「トーチのメニュー」にある。

use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

/// 読む `.desktop` のファイルの大きさの上限(バイト)と、1 つの置き場から読むファイルの数の上限。
pub const MAX_DESKTOP_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_DESKTOP_FILES: usize = 256;

/// メニューに出す、1 つのアプリ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// desktop file id(`applications/` の下のファイルの名前。例:`org.example.Editor.desktop`)。
    pub id: String,
    /// 出す名前(国ごとの値があれば、それ)。
    pub name: String,
    /// アイコンの名前か、絶対パス(`Icon`)。
    pub icon: Option<String>,
    /// 実行ファイルと引数(`Exec` を直したもの)。先頭が実行ファイル。
    pub argv: Vec<String>,
    /// 作業中のディレクトリ(`Path`)。
    pub working_dir: Option<PathBuf>,
}

/// 1 つの `.desktop` のファイルを読んだ結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// メニューに出す。
    App(App),
    /// 出さない(アプリでない、`NoDisplay=true`、`TryExec` の実行ファイルが無い)。理由つき。
    Skip(String),
    /// `Hidden=true`(消されたもの。同じ id の、後の置き場のものも出さない)。
    Hidden,
}

/// 国ごとの値を選ぶための、ロケール(`ja_JP.UTF-8@modifier` の `ja`・`JP`・`modifier`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locale {
    pub lang: String,
    pub country: Option<String>,
    pub modifier: Option<String>,
}

impl Locale {
    /// `ja_JP.UTF-8@modifier` の形を読む。`C` と `POSIX` と空はNone。
    pub fn parse(text: &str) -> Option<Self> {
        if text.is_empty() || text == "C" || text == "POSIX" || text.starts_with("C.") {
            return None;
        }
        let (rest, modifier) = match text.split_once('@') {
            Some((rest, modifier)) => (rest, Some(modifier.to_owned())),
            None => (text, None),
        };
        let rest = rest.split('.').next().unwrap_or(rest);
        let (lang, country) = match rest.split_once('_') {
            Some((lang, country)) => (lang, Some(country.to_owned())),
            None => (rest, None),
        };
        (!lang.is_empty()).then(|| Locale {
            lang: lang.to_owned(),
            country,
            modifier,
        })
    }

    /// 環境変数 `LC_ALL`・`LC_MESSAGES`・`LANG` の、最初の空でない値から決める。
    pub fn from_env() -> Option<Self> {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|value| !value.is_empty())
            .and_then(|value| Locale::parse(&value))
    }

    /// 国ごとのキーの添え字を、合う順に(`ja_JP@m`、`ja_JP`、`ja@m`、`ja`)。
    fn candidates(&self) -> Vec<String> {
        let mut list = Vec::new();
        if let (Some(country), Some(modifier)) = (&self.country, &self.modifier) {
            list.push(format!("{}_{country}@{modifier}", self.lang));
        }
        if let Some(country) = &self.country {
            list.push(format!("{}_{country}", self.lang));
        }
        if let Some(modifier) = &self.modifier {
            list.push(format!("{}@{modifier}", self.lang));
        }
        list.push(self.lang.clone());
        list
    }
}

/// 文字列の値のエスケープ(`\s`・`\n`・`\t`・`\r`・`\\`)を戻す。知らないエスケープは誤り。
fn unescape(value: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // Exec の中の引用のエスケープ(`\"` など)は、Exec を直すときに見るので、そのまま残す。
            Some(other @ ('"' | '`' | '$')) => {
                out.push('\\');
                out.push(other);
            }
            Some(other) => return Err(format!("unknown escape \\{other}")),
            None => return Err("a backslash at the end".to_owned()),
        }
    }
    Ok(out)
}

/// 引用していない引数の中に書けない文字(仕様の「reserved characters」。空白は引数の区切り)。
const RESERVED: &[char] = &[
    '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
];

/// `Exec` の値(文字列の値のエスケープを戻したもの)を、実行ファイルと引数の列に直す。`name` は `%c`、`icon` は
/// `%i`、`desktop` は `%k` に使う。シェルへは渡さないので、シェルの書き方(引用していない記号)は誤りにする。
pub fn parse_exec(
    value: &str,
    name: &str,
    icon: Option<&str>,
    desktop: &Path,
) -> Result<Vec<String>, String> {
    // 1. 引数に分ける(二重引用符の中は 1 つの引数。中では `\"`・`\``・`\$`・`\\` を戻す)。
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut chars = value.chars().peekable();
    loop {
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            break;
        };
        if first == '"' {
            chars.next();
            let mut word = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => match chars.next() {
                        Some(c @ ('"' | '`' | '$' | '\\')) => word.push(c),
                        Some(c) => {
                            return Err(format!("unknown escape \\{c} in a quoted argument"))
                        }
                        None => return Err("an unterminated quoted argument".to_owned()),
                    },
                    Some(c) => word.push(c),
                    None => return Err("an unterminated quoted argument".to_owned()),
                }
            }
            if !matches!(chars.peek(), None | Some(' ')) {
                return Err("a quoted argument followed by other characters".to_owned());
            }
            if word.contains('%') {
                return Err("a field code inside a quoted argument".to_owned());
            }
            words.push((word, true));
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c == ' ' {
                    break;
                }
                if RESERVED.contains(&c) {
                    return Err(format!(
                        "the character {c:?} needs quoting (Exec is not passed to a shell)"
                    ));
                }
                word.push(c);
                chars.next();
            }
            words.push((word, false));
        }
    }
    // 2. フィールドコードを直す。
    let mut argv = Vec::new();
    for (word, quoted) in words {
        if quoted {
            argv.push(word);
            continue;
        }
        match word.as_str() {
            // 開くファイル・URL は渡さない。使われなくなったコードだけの引数も、引数ごと消す。
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if let Some(icon) = icon {
                    argv.push("--icon".to_owned());
                    argv.push(icon.to_owned());
                }
                continue;
            }
            _ => {}
        }
        let mut out = String::new();
        let mut rest = word.chars();
        while let Some(c) = rest.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match rest.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(name),
                Some('k') => out.push_str(&desktop.to_string_lossy()),
                // 使われなくなったコードは、仕様のとおり消す。
                Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                Some(code @ ('f' | 'F' | 'u' | 'U' | 'i')) => {
                    return Err(format!(
                        "the field code %{code} must be an argument by itself"
                    ))
                }
                Some(code) => return Err(format!("unknown field code %{code}")),
                None => return Err("a % at the end".to_owned()),
            }
        }
        argv.push(out);
    }
    if argv.first().is_none_or(|program| program.is_empty()) {
        return Err("no program in Exec".to_owned());
    }
    Ok(argv)
}

/// 実行ファイル `name` があるか(`/` を含めばそのパス、含まなければ `path`(`PATH` の値)の各々の下)。
fn executable_exists(name: &str, path: Option<&OsStr>) -> bool {
    if name.contains('/') {
        return Path::new(name).is_file();
    }
    path.is_some_and(|path| {
        std::env::split_paths(path).any(|dir| dir.is_absolute() && dir.join(name).is_file())
    })
}

/// `.desktop` のファイルの文 `text` を読む。`id` は desktop file id、`desktop` はファイルのパス(`%k` に使う)、
/// `path` は `TryExec` を探す `PATH` の値。メニューに出せないものは、理由を返す(誤りは `Err`)。
pub fn parse_desktop(
    text: &str,
    id: &str,
    desktop: &Path,
    locale: Option<&Locale>,
    path: Option<&OsStr>,
) -> Result<Parsed, String> {
    // [Desktop Entry] の組のキーと値(国ごとの値は、添え字つきで)。
    let mut keys: Vec<(String, Option<String>, String)> = Vec::new();
    let mut in_entry = false;
    let mut seen_group = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(group) = line.strip_prefix('[') {
            in_entry = group == "Desktop Entry]";
            if !seen_group && !in_entry {
                return Err("the first group is not [Desktop Entry]".to_owned());
            }
            seen_group = true;
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("a line without =: {line}"));
        };
        let key = key.trim_end();
        let value = value.trim_start();
        let (key, locale) = match key.split_once('[') {
            Some((key, rest)) => (key, Some(rest.trim_end_matches(']').to_owned())),
            None => (key, None),
        };
        keys.push((key.to_owned(), locale, value.to_owned()));
    }
    if !seen_group {
        return Err("no [Desktop Entry] group".to_owned());
    }
    let plain = |key: &str| {
        keys.iter()
            .rev()
            .find(|(k, l, _)| k == key && l.is_none())
            .map(|(_, _, v)| v.as_str())
    };
    let localized = |key: &str| -> Option<&str> {
        if let Some(locale) = locale {
            for candidate in locale.candidates() {
                if let Some((_, _, v)) = keys
                    .iter()
                    .rev()
                    .find(|(k, l, _)| k == key && l.as_deref() == Some(candidate.as_str()))
                {
                    return Some(v.as_str());
                }
            }
        }
        plain(key)
    };
    let boolean = |key: &str| -> Result<bool, String> {
        match plain(key) {
            None | Some("false") => Ok(false),
            Some("true") => Ok(true),
            Some(other) => Err(format!("{key} is not true or false: {other}")),
        }
    };
    if boolean("Hidden")? {
        return Ok(Parsed::Hidden);
    }
    match plain("Type") {
        Some("Application") => {}
        Some(other) => return Ok(Parsed::Skip(format!("Type={other}"))),
        None => return Err("no Type".to_owned()),
    }
    if boolean("NoDisplay")? {
        return Ok(Parsed::Skip("NoDisplay=true".to_owned()));
    }
    if boolean("Terminal")? {
        return Err("Terminal=true (starting in a terminal is not supported)".to_owned());
    }
    let name = unescape(localized("Name").ok_or("no Name")?)?;
    let icon = localized("Icon").map(unescape).transpose()?;
    let exec = match plain("Exec") {
        Some(exec) => unescape(exec)?,
        None if boolean("DBusActivatable")? => {
            return Err("DBusActivatable=true without Exec (D-Bus is not supported)".to_owned())
        }
        None => return Err("no Exec".to_owned()),
    };
    if let Some(try_exec) = plain("TryExec") {
        let try_exec = unescape(try_exec)?;
        if !executable_exists(&try_exec, path) {
            return Ok(Parsed::Skip(format!("TryExec {try_exec} is not found")));
        }
    }
    let working_dir = match plain("Path").map(unescape).transpose()? {
        Some(dir) if Path::new(&dir).is_absolute() => Some(PathBuf::from(dir)),
        Some(dir) => return Err(format!("Path is not absolute: {dir}")),
        None => None,
    };
    let argv = parse_exec(&exec, &name, icon.as_deref(), desktop)?;
    Ok(Parsed::App(App {
        id: id.to_owned(),
        name,
        icon,
        argv,
        working_dir,
    }))
}

/// データの置き場 `dirs`(先のものほど優先)の `applications/` の下の `.desktop` を読み、メニューに出すアプリを、
/// 名前の順に返す。同じ desktop file id は、先の置き場のものだけを使う(`Hidden=true` なら、後のものも出さない)。
/// 出せなかったものと読めなかったものは、理由をログの行として返す。サブディレクトリは見ない。
pub fn read_apps(
    dirs: &[PathBuf],
    locale: Option<&Locale>,
    path: Option<&OsStr>,
) -> (Vec<App>, Vec<String>) {
    let mut apps = Vec::new();
    let mut notes = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in dirs {
        let applications = dir.join("applications");
        let Ok(entries) = fs::read_dir(&applications) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| p.extension() == Some(OsStr::new("desktop")))
            .collect();
        files.sort();
        if files.len() > MAX_DESKTOP_FILES {
            notes.push(format!(
                "{}: only the first {MAX_DESKTOP_FILES} of {} .desktop files are read",
                applications.display(),
                files.len()
            ));
            files.truncate(MAX_DESKTOP_FILES);
        }
        for file in files {
            let Some(id) = file.file_name().and_then(OsStr::to_str).map(str::to_owned) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            let text = match fs::metadata(&file) {
                Ok(meta) if meta.len() > MAX_DESKTOP_FILE_BYTES => {
                    notes.push(format!(
                        "{} is not used: larger than {MAX_DESKTOP_FILE_BYTES} bytes",
                        file.display()
                    ));
                    continue;
                }
                Ok(_) => match fs::read_to_string(&file) {
                    Ok(text) => text,
                    Err(e) => {
                        notes.push(format!("{} is not used: {e}", file.display()));
                        continue;
                    }
                },
                Err(e) => {
                    notes.push(format!("{} is not used: {e}", file.display()));
                    continue;
                }
            };
            match parse_desktop(&text, &id, &file, locale, path) {
                Ok(Parsed::App(app)) => apps.push(app),
                Ok(Parsed::Skip(_)) | Ok(Parsed::Hidden) => {}
                Err(reason) => notes.push(format!("{} is not used: {reason}", file.display())),
            }
        }
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    (apps, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(value: &str) -> Result<Vec<String>, String> {
        parse_exec(
            value,
            "エディター",
            Some("editor"),
            Path::new("/usr/share/applications/editor.desktop"),
        )
    }

    fn words(list: &[&str]) -> Result<Vec<String>, String> {
        Ok(list.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn exec_is_split_into_a_program_and_arguments_without_a_shell() {
        assert_eq!(
            exec("editor --new-window"),
            words(&["editor", "--new-window"])
        );
        assert_eq!(exec("  editor   a  "), words(&["editor", "a"]));
        // 二重引用符の中は 1 つの引数。中のエスケープを戻す。
        assert_eq!(
            exec(r#""/opt/My Editor/editor" "say \"hi\" \$HOME \\ \`x\`""#),
            words(&["/opt/My Editor/editor", r#"say "hi" $HOME \ `x`"#])
        );
        // シェルの書き方は、引用しなければ誤り(シェルへは渡さない)。
        for bad in [
            "editor > log",
            "editor; rm x",
            "editor | cat",
            "editor $HOME",
            "a*",
            "editor 'x'",
        ] {
            assert!(exec(bad).is_err(), "{bad}");
        }
        assert!(exec(r#"editor "unterminated"#).is_err());
        assert!(exec(r#"editor "a"b"#).is_err());
        assert!(exec("").is_err());
    }

    #[test]
    fn field_codes_are_expanded_or_removed() {
        // 開くファイルと URL は渡さないので消す。%i はアイコン、%c は名前、%k はファイル、%% は %。
        assert_eq!(
            exec("editor %F --name=%c %i %k 100%%"),
            words(&[
                "editor",
                "--name=エディター",
                "--icon",
                "editor",
                "/usr/share/applications/editor.desktop",
                "100%"
            ])
        );
        assert_eq!(exec("editor %u %f %U"), words(&["editor"]));
        // 使われなくなったコードは消す。
        assert_eq!(exec("editor x%dy %m"), words(&["editor", "xy"]));
        // 知らないコード、ほかの文字と一緒の %f、引用の中のコードは誤り。
        assert!(exec("editor %z").is_err());
        assert!(exec("editor --file=%f").is_err());
        assert!(exec(r#"editor "%c""#).is_err());
        assert!(exec("editor %").is_err());
    }

    fn desktop(text: &str) -> Result<Parsed, String> {
        let ja = Locale::parse("ja_JP.UTF-8");
        parse_desktop(
            text,
            "x.desktop",
            Path::new("/a/x.desktop"),
            ja.as_ref(),
            None,
        )
    }

    fn app(text: &str) -> App {
        match desktop(text) {
            Ok(Parsed::App(app)) => app,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_desktop_entry_gives_a_name_an_icon_and_a_command() {
        let text = "# comment\n[Desktop Entry]\nType=Application\nName=Editor\nName[ja]=エディター\nIcon=editor\nExec=editor %F\n[Desktop Action new]\nName=New\nExec=other\n";
        assert_eq!(
            app(text),
            App {
                id: "x.desktop".to_owned(),
                name: "エディター".to_owned(),
                icon: Some("editor".to_owned()),
                argv: vec!["editor".to_owned()],
                working_dir: None,
            }
        );
        // ロケールが無ければ、国ごとでない値。ja_JP の値は ja より先。
        let none = parse_desktop(text, "x.desktop", Path::new("/a/x.desktop"), None, None);
        assert!(matches!(none, Ok(Parsed::App(App { ref name, .. })) if name == "Editor"));
        let both = "[Desktop Entry]\nType=Application\nName=A\nName[ja]=B\nName[ja_JP]=C\nExec=a\n";
        assert_eq!(app(both).name, "C");
        // 値のエスケープと Path。
        let path = "[Desktop Entry]\nType=Application\nName=A\\sB\nExec=a\nPath=/tmp\n";
        assert_eq!(app(path).name, "A B");
        assert_eq!(app(path).working_dir, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn entries_that_cannot_be_started_as_written_are_left_out() {
        let base = "[Desktop Entry]\nName=A\nExec=a\n";
        assert_eq!(
            desktop(&format!("{base}Type=Link\n")),
            Ok(Parsed::Skip("Type=Link".to_owned()))
        );
        assert_eq!(
            desktop(&format!("{base}Type=Application\nHidden=true\n")),
            Ok(Parsed::Hidden)
        );
        assert!(matches!(
            desktop(&format!("{base}Type=Application\nNoDisplay=true\n")),
            Ok(Parsed::Skip(_))
        ));
        assert!(matches!(
            desktop(&format!(
                "{base}Type=Application\nTryExec=/no/such/program\n"
            )),
            Ok(Parsed::Skip(_))
        ));
        // 対応しない指定は、黙って無視せずに誤りにする。
        for bad in [
            format!("{base}Type=Application\nTerminal=true\n"),
            "[Desktop Entry]\nType=Application\nName=A\nDBusActivatable=true\n".to_owned(),
            "[Desktop Entry]\nType=Application\nName=A\n".to_owned(),
            format!("{base}Type=Application\nPath=relative\n"),
            format!("{base}Type=Application\nNoDisplay=yes\n"),
            "[Other]\nType=Application\n".to_owned(),
            "[Desktop Entry]\nType=Application\nExec=a\n".to_owned(),
            "[Desktop Entry]\nType=Application\nName=A\nExec=a; b\n".to_owned(),
        ] {
            assert!(desktop(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn locales_are_read_from_the_usual_forms() {
        let full = Locale::parse("ja_JP.UTF-8@x").unwrap();
        assert_eq!(full.candidates(), ["ja_JP@x", "ja_JP", "ja@x", "ja"]);
        assert_eq!(Locale::parse("ja").unwrap().candidates(), ["ja"]);
        assert_eq!(Locale::parse("C"), None);
        assert_eq!(Locale::parse("C.UTF-8"), None);
        assert_eq!(Locale::parse(""), None);
    }

    #[test]
    fn apps_are_read_from_the_data_dirs_in_order() {
        let root = std::env::temp_dir().join(format!("seinas-apps-{}", std::process::id()));
        let (first, second) = (root.join("first"), root.join("second"));
        for dir in [&first, &second] {
            fs::create_dir_all(dir.join("applications")).unwrap();
        }
        let write = |dir: &Path, name: &str, text: &str| {
            fs::write(dir.join("applications").join(name), text).unwrap();
        };
        write(
            &first,
            "b.desktop",
            "[Desktop Entry]\nType=Application\nName=Beta\nExec=b\n",
        );
        write(&first, "gone.desktop", "[Desktop Entry]\nHidden=true\n");
        write(
            &first,
            "bad.desktop",
            "[Desktop Entry]\nType=Application\nName=Bad\nExec=a|b\n",
        );
        write(
            &second,
            "a.desktop",
            "[Desktop Entry]\nType=Application\nName=alpha\nExec=a\n",
        );
        // 先の置き場の同じ id(b、gone)が勝つ。
        write(
            &second,
            "b.desktop",
            "[Desktop Entry]\nType=Application\nName=Other\nExec=o\n",
        );
        write(
            &second,
            "gone.desktop",
            "[Desktop Entry]\nType=Application\nName=Gone\nExec=g\n",
        );
        write(&second, "readme.txt", "not a desktop file");
        let (apps, notes) = read_apps(&[first.clone(), second.clone()], None, None);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Beta"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("bad.desktop is not used"), "{notes:?}");
        fs::remove_dir_all(&root).unwrap();
    }
}
