//! Seinas のショートカット(コンポジタが使うキー)の判定。
//!
//! キーは、IME とアプリへ渡す前に、ここで見る(ADR 0004)。IME が固まっていても、ショートカットが効くように
//! するため。Wayland のオブジェクトにも xkb の型にも触れずに確かめられるよう、keysym は数で受け取る。
//! 一覧は、Alt+Tab と Alt+Shift+Tab(ウィンドウの切り替え)だけ。

/// ショートカットの判定に使う、修飾キーの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Windows キーに当たるもの。
    pub logo: bool,
}

/// xkb の keysym の数(使うものだけ)。
pub mod keysym {
    pub const TAB: u32 = 0xff09;
    /// Shift を押したときの Tab。
    pub const ISO_LEFT_TAB: u32 = 0xfe20;
    pub const ESCAPE: u32 = 0xff1b;
}

/// 修飾キー(Shift、Ctrl、Alt、Caps Lock など)の keysym か。
pub fn is_modifier_key(keysym: u32) -> bool {
    // Shift_L〜Hyper_R と、ISO_Lock〜ISO_Level5_Lock(AltGr など)と、Num_Lock。
    (0xffe1..=0xffee).contains(&keysym) || (0xfe01..=0xfe13).contains(&keysym) || keysym == 0xff7f
}

/// Alt+Tab:次のウィンドウへ切り替える。
pub const SWITCH_NEXT: Shortcut = Shortcut {
    keysym: keysym::TAB,
    modifiers: Modifiers {
        ctrl: false,
        alt: true,
        shift: false,
        logo: false,
    },
};

/// Alt+Shift+Tab:前のウィンドウへ切り替える。
pub const SWITCH_PREVIOUS: Shortcut = Shortcut {
    keysym: keysym::ISO_LEFT_TAB,
    modifiers: Modifiers {
        ctrl: false,
        alt: true,
        shift: true,
        logo: false,
    },
};

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

    /// Seinas のショートカットの一覧(ウィンドウの切り替え)。
    pub fn seinas() -> Self {
        Self::new(vec![SWITCH_NEXT, SWITCH_PREVIOUS])
    }

    /// 一覧に無いキー `code` の押下を Seinas が使ったことにする(離したことも、どこへも渡さない)。
    pub fn hold(&mut self, code: u32) {
        if !self.held.contains(&code) {
            self.held.push(code);
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

    use keysym::TAB;
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

    #[test]
    fn alt_shift_tab_is_the_shifted_tab() {
        let mut shortcuts = Shortcuts::seinas();
        let alt_shift = Modifiers { shift: true, ..ALT };
        assert_eq!(
            shortcuts.filter(15, true, keysym::ISO_LEFT_TAB, alt_shift),
            Decision::Use(SWITCH_PREVIOUS)
        );
        // Ctrl も押していれば、使わない。
        let ctrl_alt = Modifiers { ctrl: true, ..ALT };
        assert_eq!(shortcuts.filter(15, true, TAB, ctrl_alt), Decision::Forward);
    }

    #[test]
    fn a_held_key_is_swallowed_on_release() {
        let mut shortcuts = Shortcuts::seinas();
        shortcuts.hold(1);
        assert_eq!(
            shortcuts.filter(1, false, keysym::ESCAPE, ALT),
            Decision::Swallow
        );
        assert_eq!(
            shortcuts.filter(1, false, keysym::ESCAPE, ALT),
            Decision::Forward
        );
        assert!(is_modifier_key(0xffe9), "Alt_L");
        assert!(is_modifier_key(0xffe5), "Caps_Lock");
        assert!(!is_modifier_key(TAB));
        assert!(!is_modifier_key(keysym::ESCAPE));
    }
}
