//! Seinas の設定のファイルの読み方(ADR 0003 の決定 6)。
//!
//! 形式は、1 行に `キー = 値`。`#` から行の終わりまでは、コメント。空の行は飛ばす。キーと値の前後の空白は
//! 取る。キーの意味は、使う側(フォント、壁紙)が決める。読めない行は、飛ばして理由を返す(止まらない)。
//!
//! 置き場は `$XDG_CONFIG_HOME/seinas/`(無ければ `~/.config/seinas/`)。

use std::path::{Path, PathBuf};

/// 1 つの `キー = 値` の行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 行の番号(1 から)。
    pub line: usize,
    pub key: String,
    pub value: String,
}

/// 読めなかった行か、読めなかったファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 行の番号(1 から)。ファイルそのものを読めなかったときは 0。
    pub line: usize,
    pub reason: String,
}

impl Problem {
    /// 行 `line` の問題。
    pub fn at(line: usize, reason: impl Into<String>) -> Self {
        Problem {
            line,
            reason: reason.into(),
        }
    }
}

/// 設定の文 `text` を、`キー = 値` の行に分ける。`=` の無い行と、値の無い行は、理由を返す。
pub fn parse(text: &str) -> (Vec<Entry>, Vec<Problem>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let Some((key, value)) = content.split_once('=') else {
            problems.push(Problem::at(
                line,
                format!("not a \"key = value\" line: {content}"),
            ));
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            problems.push(Problem::at(line, format!("{key} has no value")));
            continue;
        }
        entries.push(Entry {
            line,
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }
    (entries, problems)
}

/// 設定のファイル `path` を読んで、行に分ける。ファイルが無ければ、行も問題も無い。読めなければ、その理由
/// (行の番号は 0)。
pub fn read(path: &Path) -> (Vec<Entry>, Vec<Problem>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
        Err(e) => (
            Vec::new(),
            vec![Problem::at(
                0,
                format!("cannot read {}: {e}", path.display()),
            )],
        ),
    }
}

/// Seinas の設定の置き場(`$XDG_CONFIG_HOME/seinas`、無ければ `$HOME/.config/seinas`)。どちらも無ければNone。
pub fn config_dir(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let base = xdg_config_home
        .map(Path::to_owned)
        .or_else(|| home.map(|home| home.join(".config")))?;
    Some(base.join("seinas"))
}

/// このプロセスの環境変数 `XDG_CONFIG_HOME`・`HOME` から決めた、設定のファイル `name` の既定の場所。空の値は、
/// 無いものとして扱う。
pub fn default_file(name: &str) -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let (config, home) = (var("XDG_CONFIG_HOME"), var("HOME"));
    Some(config_dir(config.as_deref(), home.as_deref())?.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_comments_and_problems() {
        let (entries, problems) =
            parse("# comment\n\nui = Noto Sans  # after\n  dir=/a/b \nbroken\nempty =\n");
        assert_eq!(
            entries,
            [
                Entry {
                    line: 3,
                    key: "ui".into(),
                    value: "Noto Sans".into()
                },
                Entry {
                    line: 4,
                    key: "dir".into(),
                    value: "/a/b".into()
                },
            ]
        );
        assert_eq!(problems.iter().map(|p| p.line).collect::<Vec<_>>(), [5, 6]);
    }

    #[test]
    fn the_config_dir_prefers_xdg_config_home() {
        let dir = config_dir(Some(Path::new("/x")), Some(Path::new("/home/u")));
        assert_eq!(dir, Some(PathBuf::from("/x/seinas")));
        let dir = config_dir(None, Some(Path::new("/home/u")));
        assert_eq!(dir, Some(PathBuf::from("/home/u/.config/seinas")));
        assert_eq!(config_dir(None, None), None);
    }

    #[test]
    fn a_missing_file_is_no_problem() {
        let (entries, problems) = read(Path::new("/no/such/seinas/file.conf"));
        assert!(entries.is_empty() && problems.is_empty());
    }
}
