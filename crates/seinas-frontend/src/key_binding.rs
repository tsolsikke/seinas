//! キーの組み合わせの書き方(`Alt+Shift+Tab` など)と、ウィンドウの切り替えのキーの設定(`seinas.conf`)。
//!
//! 書き方:修飾キー(`Alt`・`Shift`・`Ctrl`・`Super`)と、キーの名前(xkb の keysym の名前。`Tab`、`F6`、`a` など。
//! 大文字と小文字は区別しない)を `+` でつなぐ。キーの名前は、終わりに 1 つだけ。
//!
//! 設定のファイルは `$XDG_CONFIG_HOME/seinas/seinas.conf`(読み方は `seinas-config`)。
//!
//! ```text
//! switch-next = Alt+Tab, Alt+F6
//! switch-prev = Alt+Shift+Tab, Alt+Shift+F6
//! ```
//!
//! `,` で区切って、いくつでも書ける。書いたキーは、既定のキーと置き換わる(足すのではない)。読めない値は、
//! ログに出して、そのキー(次へ・前へ)を既定のままにする。

use std::path::{Path, PathBuf};

use smithay::input::keyboard::{xkb, Keysym};
use tracing::{info, warn};

use crate::shortcuts::{is_modifier_key, Modifiers, Shortcut, SwitchKeys};

/// ウィンドウの切り替えのキーの設定のファイルの名前(`$XDG_CONFIG_HOME/seinas/` の下)。
pub const CONFIG_FILE: &str = "seinas.conf";

/// 設定のファイルの既定の場所(`$XDG_CONFIG_HOME/seinas/seinas.conf`、無ければ `~/.config/seinas/seinas.conf`)。
pub fn default_config_file() -> Option<PathBuf> {
    seinas_config::default_file(CONFIG_FILE)
}

/// `Alt+Shift+Tab` の形の文を、ショートカットに直す。
pub fn parse_shortcut(text: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::default();
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, names) = parts.split_last().ok_or("empty")?;
    for name in names {
        let flag = match name.to_ascii_lowercase().as_str() {
            "alt" => &mut modifiers.alt,
            "shift" => &mut modifiers.shift,
            "ctrl" | "control" => &mut modifiers.ctrl,
            "super" | "logo" | "win" => &mut modifiers.logo,
            "" => return Err(format!("an empty part in {text:?}")),
            other => return Err(format!("unknown modifier {other:?} in {text:?}")),
        };
        if *flag {
            return Err(format!("{name} is written twice in {text:?}"));
        }
        *flag = true;
    }
    if key.is_empty() {
        return Err(format!("no key name in {text:?}"));
    }
    let keysym = xkb::keysym_from_name(key, xkb::KEYSYM_CASE_INSENSITIVE);
    if keysym.raw() == 0 {
        return Err(format!("unknown key name {key:?} in {text:?}"));
    }
    if is_modifier_key(keysym.raw()) {
        return Err(format!(
            "{key:?} is a modifier; write it before the key, like Alt+Tab"
        ));
    }
    Ok(Shortcut {
        keysym: keysym.raw(),
        modifiers,
    })
}

/// 修飾キーと keysym を、`Ctrl+Alt+Shift+Super+F6` の形で書く(設定の書き方と同じ順と名前)。
pub fn describe(modifiers: Modifiers, keysym: u32) -> String {
    let mut text = String::new();
    for (on, name) in [
        (modifiers.ctrl, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.logo, "Super"),
    ] {
        if on {
            text.push_str(name);
            text.push('+');
        }
    }
    text.push_str(&xkb::keysym_get_name(Keysym::new(keysym)));
    text
}

/// `,` で区切ったショートカットの一覧を読む。1 つでも読めなければ、その理由。
fn parse_list(text: &str) -> Result<Vec<Shortcut>, String> {
    text.split(',').map(parse_shortcut).collect()
}

/// 設定の文 `text` から、切り替えのキーを決める。書いていないキーと、読めないキーは、既定(`nested` で変わる。
/// [`SwitchKeys::defaults`])のまま。読めなかったものは、理由を返す。
pub fn switch_keys_from(text: &str, nested: bool) -> (SwitchKeys, Vec<String>) {
    let mut keys = SwitchKeys::defaults(nested);
    let (entries, problems) = seinas_config::parse(text);
    let mut reasons: Vec<String> = problems
        .into_iter()
        .map(|problem| format!("line {}: {}", problem.line, problem.reason))
        .collect();
    for entry in entries {
        let target = match entry.key.as_str() {
            "switch-next" => &mut keys.next,
            "switch-prev" => &mut keys.previous,
            other => {
                reasons.push(format!("line {}: unknown key {other:?}", entry.line));
                continue;
            }
        };
        match parse_list(&entry.value) {
            Ok(list) => *target = list,
            Err(reason) => reasons.push(format!(
                "line {}: {} keeps the default: {reason}",
                entry.line, entry.key
            )),
        }
    }
    (keys, reasons)
}

/// 設定のファイル `path` から、切り替えのキーを決める(無ければ既定)。読めなかったものはログに出す。決めた
/// キーもログに出す。
pub fn read_switch_keys(path: Option<&Path>, nested: bool) -> SwitchKeys {
    let text = match path.map(std::fs::read_to_string) {
        Some(Ok(text)) => text,
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
            warn!("cannot read the settings {}: {e}", path.unwrap().display());
            String::new()
        }
        _ => String::new(),
    };
    let (keys, reasons) = switch_keys_from(&text, nested);
    for reason in reasons {
        let place = path
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        warn!("settings {place}: {reason}");
    }
    let names = |list: &[Shortcut]| {
        list.iter()
            .map(|key| describe(key.modifiers, key.keysym))
            .collect::<Vec<_>>()
            .join(", ")
    };
    info!(
        "switch windows: next {}; previous {}",
        names(&keys.next),
        names(&keys.previous)
    );
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::{
        keysym, SWITCH_NEXT, SWITCH_NEXT_NESTED, SWITCH_PREVIOUS, SWITCH_PREVIOUS_NESTED,
    };

    const ALT: Modifiers = Modifiers {
        ctrl: false,
        alt: true,
        shift: false,
        logo: false,
    };

    #[test]
    fn shortcuts_are_parsed() {
        assert_eq!(parse_shortcut("Alt+Tab"), Ok(SWITCH_NEXT));
        assert_eq!(parse_shortcut(" alt + shift + tab "), Ok(SWITCH_PREVIOUS));
        assert_eq!(parse_shortcut("Alt+F6"), Ok(SWITCH_NEXT_NESTED));
        let ctrl_super_a = parse_shortcut("Ctrl+Super+a").unwrap();
        assert_eq!(ctrl_super_a.keysym, 0x61);
        assert!(ctrl_super_a.modifiers.ctrl && ctrl_super_a.modifiers.logo);
        assert_eq!(
            parse_shortcut("F7").unwrap().modifiers,
            Modifiers::default()
        );
    }

    #[test]
    fn bad_shortcuts_are_refused() {
        for text in [
            "",
            "Alt+",
            "Alt+NoSuchKey",
            "Hyper+Tab",
            "Alt+Alt+Tab",
            "Alt+Shift_L",
            "Alt++Tab",
        ] {
            assert!(parse_shortcut(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn shortcuts_are_described_like_the_settings() {
        assert_eq!(describe(ALT, keysym::F6), "Alt+F6");
        let all = Modifiers {
            ctrl: true,
            alt: true,
            shift: true,
            logo: true,
        };
        assert_eq!(describe(all, keysym::TAB), "Ctrl+Alt+Shift+Super+Tab");
        assert_eq!(describe(Modifiers::default(), 0x61), "a");
    }

    #[test]
    fn the_settings_replace_the_defaults() {
        let (keys, reasons) = switch_keys_from(
            "switch-next = Alt+F7, Ctrl+Tab\nswitch-prev = Alt+Shift+F7\n",
            true,
        );
        assert!(reasons.is_empty(), "{reasons:?}");
        assert_eq!(
            keys.next,
            [
                parse_shortcut("Alt+F7").unwrap(),
                parse_shortcut("Ctrl+Tab").unwrap()
            ]
        );
        assert_eq!(keys.previous, [parse_shortcut("Alt+Shift+F7").unwrap()]);
    }

    #[test]
    fn unreadable_values_keep_the_defaults() {
        let (keys, reasons) = switch_keys_from(
            "switch-next = Alt+Nope\nswitch-prev = Alt+Shift+F8\ncolor = red\nbroken\n",
            false,
        );
        assert_eq!(keys.next, SwitchKeys::defaults(false).next);
        assert_eq!(keys.previous, [parse_shortcut("Alt+Shift+F8").unwrap()]);
        assert_eq!(reasons.len(), 3, "{reasons:?}");
        assert!(reasons[0].contains("not a \"key = value\" line"));
        assert!(reasons[1].contains("switch-next keeps the default"));
        assert!(reasons[2].contains("unknown key \"color\""));
    }

    #[test]
    fn no_settings_give_the_defaults() {
        assert_eq!(switch_keys_from("", false).0, SwitchKeys::defaults(false));
        assert_eq!(
            switch_keys_from("", true).0.previous,
            [SWITCH_PREVIOUS, SWITCH_PREVIOUS_NESTED]
        );
    }
}
