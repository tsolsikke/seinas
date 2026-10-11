//! Seinas のショートカット(コンポジタが使うキー)の判定。
//!
//! キーは、IME とアプリへ渡す前に、ここで見る(ADR 0004)。IME が固まっていても、ショートカットが効くように
//! するため。Wayland のオブジェクトにも xkb の型にも触れずに確かめられるよう、keysym は数で受け取る。
//! 一覧は、今は空(ショートカットは、まだ無い)。

/// ショートカットの判定に使う、修飾キーの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Windows キーに当たるもの。
    pub logo: bool,
}

/// 1 つのショートカット。keysym(xkb の数)と、そのとき押されている修飾キーの組。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub keysym: u32,
    pub modifiers: Modifiers,
}

/// 押した・離したキーを、どうするか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// IME かアプリへ渡す。
    Forward,
    /// ショートカットとして Seinas が使う(押したとき)。
    Use(Shortcut),
    /// Seinas が使ったキーを離した。どこへも渡さない(押したことを知らないアプリに、離したことだけが届かない
    /// ようにする)。
    Swallow,
}

/// ショートカットの一覧と、使っている間のキー。
#[derive(Debug, Default)]
pub struct Shortcuts {
    list: Vec<Shortcut>,
    /// Seinas が使って、まだ離されていないキー(evdev のキーの番号)。
    held: Vec<u32>,
}

impl Shortcuts {
    pub fn new(list: Vec<Shortcut>) -> Self {
        Shortcuts {
            list,
            held: Vec::new(),
        }
    }

    /// キー `code`(evdev の番号)を押した(`pressed`)・離したときに、どうするか。`keysym` と `modifiers` は、
    /// そのときの keymap と修飾キーの状態で読んだもの。
    pub fn filter(
        &mut self,
        code: u32,
        pressed: bool,
        keysym: u32,
        modifiers: Modifiers,
    ) -> Decision {
        if !pressed {
            return match self.held.iter().position(|&held| held == code) {
                Some(index) => {
                    self.held.remove(index);
                    Decision::Swallow
                }
                None => Decision::Forward,
            };
        }
        let wanted = Shortcut { keysym, modifiers };
        match self.list.iter().find(|&&shortcut| shortcut == wanted) {
            Some(&shortcut) => {
                if !self.held.contains(&code) {
                    self.held.push(code);
                }
                Decision::Use(shortcut)
            }
            None => Decision::Forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xkb の keysym の数。
    const TAB: u32 = 0xff09;
    const LETTER_A: u32 = 0x61;
    const ALT: Modifiers = Modifiers {
        ctrl: false,
        alt: true,
        shift: false,
        logo: false,
    };

    #[test]
    fn without_shortcuts_every_key_is_forwarded() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(shortcuts.filter(15, true, TAB, ALT), Decision::Forward);
        assert_eq!(shortcuts.filter(15, false, TAB, ALT), Decision::Forward);
    }

    #[test]
    fn a_used_key_is_swallowed_until_it_is_released() {
        let alt_tab = Shortcut {
            keysym: TAB,
            modifiers: ALT,
        };
        let mut shortcuts = Shortcuts::new(vec![alt_tab]);
        // 修飾キーが違えば、使わない。
        assert_eq!(
            shortcuts.filter(15, true, TAB, Modifiers::default()),
            Decision::Forward
        );
        assert_eq!(
            shortcuts.filter(15, false, TAB, Modifiers::default()),
            Decision::Forward
        );
        // Alt+Tab は使い、離したことも渡さない。Alt を先に離しても、同じ。
        assert_eq!(shortcuts.filter(15, true, TAB, ALT), Decision::Use(alt_tab));
        assert_eq!(
            shortcuts.filter(15, false, TAB, Modifiers::default()),
            Decision::Swallow
        );
        // ほかのキーは、使っている間も渡す。
        assert_eq!(shortcuts.filter(30, true, LETTER_A, ALT), Decision::Forward);
        // 押し続けて繰り返しの押下が来ても、離すのは 1 回。
        shortcuts.filter(15, true, TAB, ALT);
        shortcuts.filter(15, true, TAB, ALT);
        assert_eq!(shortcuts.filter(15, false, TAB, ALT), Decision::Swallow);
        assert_eq!(shortcuts.filter(15, false, TAB, ALT), Decision::Forward);
    }
}
