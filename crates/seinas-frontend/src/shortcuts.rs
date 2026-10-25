//! Seinas のショートカット(コンポジタが使うキー)の判定。
//!
//! キーは、IME とアプリへ渡す前に、ここで見る(ADR 0004)。IME が固まっていても、ショートカットが効くように
//! するため。Wayland のオブジェクトにも xkb の型にも触れずに確かめられるよう、keysym は数で受け取る。
//! 一覧は、ウィンドウの切り替え(次へ・前へ)と、IME の緊急解除(ADR 0004 の 1.5)。キーは設定で変えられる
//! (`key_binding.rs`)。

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
    pub const F6: u32 = 0xffc3;
    pub const F12: u32 = 0xffc9;
}

/// 修飾キー(Shift、Ctrl、Alt、Caps Lock など)の keysym か。
pub fn is_modifier_key(keysym: u32) -> bool {
    // Shift_L〜Hyper_R と、ISO_Lock〜ISO_Level5_Lock(AltGr など)と、Num_Lock。
    (0xffe1..=0xffee).contains(&keysym) || (0xfe01..=0xfe13).contains(&keysym) || keysym == 0xff7f
}

const ALT: Modifiers = Modifiers {
    ctrl: false,
    alt: true,
    shift: false,
    logo: false,
};
const ALT_SHIFT: Modifiers = Modifiers {
    ctrl: false,
    alt: true,
    shift: true,
    logo: false,
};

/// Alt+Tab:次のウィンドウへ切り替える(既定)。
pub const SWITCH_NEXT: Shortcut = Shortcut {
    keysym: keysym::TAB,
    modifiers: ALT,
};

/// Alt+Shift+Tab:前のウィンドウへ切り替える(既定)。Shift を押した Tab の keysym は ISO_Left_Tab になるが、
/// Shift を外した keysym(Tab)でも当たる([`Shortcuts::filter`])。
pub const SWITCH_PREVIOUS: Shortcut = Shortcut {
    keysym: keysym::TAB,
    modifiers: ALT_SHIFT,
};

/// Alt+F6:入れ子の構成で足す、次のウィンドウへの切り替え(親の側の Windows が Alt+Tab を先に使うため)。
pub const SWITCH_NEXT_NESTED: Shortcut = Shortcut {
    keysym: keysym::F6,
    modifiers: ALT,
};

/// Alt+Shift+F6:入れ子の構成で足す、前のウィンドウへの切り替え。
pub const SWITCH_PREVIOUS_NESTED: Shortcut = Shortcut {
    keysym: keysym::F6,
    modifiers: ALT_SHIFT,
};

/// Alt+Shift+F12:IME の緊急解除(既定。入れ子では届くことを確かめた。ZeikOS では未確認。ADR 0004 の 1.5)。
pub const RELEASE_INPUT_METHOD: Shortcut = Shortcut {
    keysym: keysym::F12,
    modifiers: ALT_SHIFT,
};

/// ウィンドウを動かす修飾キーの既定(Alt)。押しながら、ウィンドウのどこでも、左のボタンでドラッグすると動かし、
/// 右のボタンでドラッグすると大きさを変える(Seinas が飾りを描かないウィンドウのため)。入れ子の構成(WSLg)で、
/// Alt は届く(Alt+Shift+F12 で確かめた。2026-10-15)。Super(Windows キー)は、Windows が先に使う。
pub const WINDOW_DRAG: Modifiers = ALT;

/// 1 つのショートカット。keysym(xkb の数)と、そのとき押されている修飾キーの組。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub keysym: u32,
    pub modifiers: Modifiers,
}

/// ショートカットで行うこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 次のウィンドウへ切り替える(切り替えを始めるか、次の候補へ進む)。
    SwitchNext,
    /// 前のウィンドウへ切り替える。
    SwitchPrevious,
    /// 切り替えを取り消す(切り替えの最中の Esc)。
    CancelSwitch,
    /// IME の緊急解除(IME の接続を切り、次のキーから直接アプリへ渡す)。
    ReleaseInputMethod,
}

/// Seinas のショートカットのキー:ウィンドウの切り替え(次へ・前へ)と、IME の緊急解除。どれも 1 つ以上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchKeys {
    pub next: Vec<Shortcut>,
    pub previous: Vec<Shortcut>,
    pub release_input_method: Vec<Shortcut>,
    /// ウィンドウを動かす修飾キー(既定は `WINDOW_DRAG`。Alt)。押している修飾キーが、これとちょうど同じときに働く。
    pub window_drag: Modifiers,
}

impl SwitchKeys {
    /// 既定のキー。どの構成でも Alt+Tab と Alt+Shift+Tab。入れ子の構成(`nested`)では、Alt+F6 と Alt+Shift+F6 を
    /// 足す(親の側の Windows が Alt+Tab を先に使い、Seinas に届かないため。Alt+Tab も残す)。IME の緊急解除は、
    /// どの構成でも Alt+Shift+F12。
    pub fn defaults(nested: bool) -> Self {
        let mut keys = SwitchKeys {
            next: vec![SWITCH_NEXT],
            previous: vec![SWITCH_PREVIOUS],
            release_input_method: vec![RELEASE_INPUT_METHOD],
            window_drag: WINDOW_DRAG,
        };
        if nested {
            keys.next.push(SWITCH_NEXT_NESTED);
            keys.previous.push(SWITCH_PREVIOUS_NESTED);
        }
        keys
    }
}

/// 押した・離したキーを、どうするか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// IME かアプリへ渡す。
    Forward,
    /// ショートカットとして Seinas が使う(押したとき)。使ったショートカットと、行うこと。
    Use(Shortcut, Action),
    /// Seinas が使ったキーを離した。どこへも渡さない(押したことを知らないアプリに、離したことだけが届かない
    /// ようにする)。
    Swallow,
}

/// ショートカットの一覧と、使っている間のキー。
#[derive(Debug, Default)]
pub struct Shortcuts {
    list: Vec<(Shortcut, Action)>,
    /// Seinas が使って、まだ離されていないキー(evdev のキーの番号)。
    held: Vec<u32>,
}

impl Shortcuts {
    pub fn new(list: Vec<(Shortcut, Action)>) -> Self {
        Shortcuts {
            list,
            held: Vec::new(),
        }
    }

    /// Seinas のショートカットの一覧(ウィンドウの切り替えと IME の緊急解除のキー `keys`)。
    pub fn switching(keys: &SwitchKeys) -> Self {
        let next = keys.next.iter().map(|&key| (key, Action::SwitchNext));
        let previous = keys
            .previous
            .iter()
            .map(|&key| (key, Action::SwitchPrevious));
        let release = keys
            .release_input_method
            .iter()
            .map(|&key| (key, Action::ReleaseInputMethod));
        Self::new(next.chain(previous).chain(release).collect())
    }

    /// 既定のキーでの一覧(入れ子でない構成のもの)。
    pub fn seinas() -> Self {
        Self::switching(&SwitchKeys::defaults(false))
    }

    /// 一覧に無いキー `code` の押下を Seinas が使ったことにする(離したことも、どこへも渡さない)。
    pub fn hold(&mut self, code: u32) {
        if !self.held.contains(&code) {
            self.held.push(code);
        }
    }

    /// キー `code`(evdev の番号)を押した(`pressed`)・離したときに、どうするか。`keysym` は、そのときの keymap と
    /// 修飾キーの状態で読んだもの、`raw_keysym` は、修飾キーを外して読んだもの(Shift+Tab の Tab など)。どちらかが
    /// 一覧の keysym と同じで、修飾キーが同じなら、使う。
    pub fn filter(
        &mut self,
        code: u32,
        pressed: bool,
        keysym: u32,
        raw_keysym: u32,
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
        let found = self.list.iter().find(|(shortcut, _)| {
            shortcut.modifiers == modifiers
                && (shortcut.keysym == keysym || shortcut.keysym == raw_keysym)
        });
        match found {
            // IME の緊急解除は、押し続けたときの繰り返しの押下では、もう一度しない(離すまで 1 回)。
            Some(&(_, Action::ReleaseInputMethod)) if self.held.contains(&code) => {
                Decision::Swallow
            }
            Some(&(shortcut, action)) => {
                if !self.held.contains(&code) {
                    self.held.push(code);
                }
                Decision::Use(shortcut, action)
            }
            None => Decision::Forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use keysym::{F6, ISO_LEFT_TAB, TAB};
    const LETTER_A: u32 = 0x61;

    #[test]
    fn without_shortcuts_every_key_is_forwarded() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(shortcuts.filter(15, true, TAB, TAB, ALT), Decision::Forward);
        assert_eq!(
            shortcuts.filter(15, false, TAB, TAB, ALT),
            Decision::Forward
        );
    }

    #[test]
    fn a_used_key_is_swallowed_until_it_is_released() {
        let mut shortcuts = Shortcuts::seinas();
        let none = Modifiers::default();
        // 修飾キーが違えば、使わない。
        assert_eq!(
            shortcuts.filter(15, true, TAB, TAB, none),
            Decision::Forward
        );
        assert_eq!(
            shortcuts.filter(15, false, TAB, TAB, none),
            Decision::Forward
        );
        // Alt+Tab は使い、離したことも渡さない。Alt を先に離しても、同じ。
        assert_eq!(
            shortcuts.filter(15, true, TAB, TAB, ALT),
            Decision::Use(SWITCH_NEXT, Action::SwitchNext)
        );
        assert_eq!(
            shortcuts.filter(15, false, TAB, TAB, none),
            Decision::Swallow
        );
        // ほかのキーは、使っている間も渡す。
        assert_eq!(
            shortcuts.filter(30, true, LETTER_A, LETTER_A, ALT),
            Decision::Forward
        );
        // 押し続けて繰り返しの押下が来ても、離すのは 1 回。
        shortcuts.filter(15, true, TAB, TAB, ALT);
        shortcuts.filter(15, true, TAB, TAB, ALT);
        assert_eq!(
            shortcuts.filter(15, false, TAB, TAB, ALT),
            Decision::Swallow
        );
        assert_eq!(
            shortcuts.filter(15, false, TAB, TAB, ALT),
            Decision::Forward
        );
    }

    #[test]
    fn the_release_key_is_used_once_until_it_is_released() {
        let mut shortcuts = Shortcuts::seinas();
        let f12 = keysym::F12;
        assert_eq!(
            shortcuts.filter(88, true, f12, f12, ALT_SHIFT),
            Decision::Use(RELEASE_INPUT_METHOD, Action::ReleaseInputMethod)
        );
        // 押し続けたときの繰り返しの押下は、使って捨てる(もう一度はしない)。
        assert_eq!(
            shortcuts.filter(88, true, f12, f12, ALT_SHIFT),
            Decision::Swallow
        );
        assert_eq!(
            shortcuts.filter(88, false, f12, f12, ALT_SHIFT),
            Decision::Swallow
        );
        // 離した後は、また使う。修飾キーが違えば、使わない。
        assert_eq!(
            shortcuts.filter(88, true, f12, f12, ALT_SHIFT),
            Decision::Use(RELEASE_INPUT_METHOD, Action::ReleaseInputMethod)
        );
        shortcuts.filter(88, false, f12, f12, ALT_SHIFT);
        assert_eq!(shortcuts.filter(88, true, f12, f12, ALT), Decision::Forward);
    }

    #[test]
    fn alt_shift_tab_matches_the_shifted_and_the_plain_tab() {
        let mut shortcuts = Shortcuts::seinas();
        assert_eq!(
            shortcuts.filter(15, true, ISO_LEFT_TAB, TAB, ALT_SHIFT),
            Decision::Use(SWITCH_PREVIOUS, Action::SwitchPrevious)
        );
        // Ctrl も押していれば、使わない。
        let ctrl_alt = Modifiers { ctrl: true, ..ALT };
        assert_eq!(
            shortcuts.filter(16, true, TAB, TAB, ctrl_alt),
            Decision::Forward
        );
    }

    #[test]
    fn the_nested_defaults_add_alt_f6() {
        let plain = SwitchKeys::defaults(false);
        assert_eq!(plain.next, [SWITCH_NEXT]);
        assert_eq!(plain.previous, [SWITCH_PREVIOUS]);
        let nested = SwitchKeys::defaults(true);
        assert_eq!(nested.next, [SWITCH_NEXT, SWITCH_NEXT_NESTED]);
        assert_eq!(nested.previous, [SWITCH_PREVIOUS, SWITCH_PREVIOUS_NESTED]);
        let mut shortcuts = Shortcuts::switching(&nested);
        assert_eq!(
            shortcuts.filter(64, true, F6, F6, ALT),
            Decision::Use(SWITCH_NEXT_NESTED, Action::SwitchNext)
        );
        assert_eq!(
            shortcuts.filter(64, true, F6, F6, ALT_SHIFT),
            Decision::Use(SWITCH_PREVIOUS_NESTED, Action::SwitchPrevious)
        );
        // 入れ子でない既定には、Alt+F6 は無い。
        assert_eq!(
            Shortcuts::seinas().filter(64, true, F6, F6, ALT),
            Decision::Forward
        );
    }

    #[test]
    fn a_held_key_is_swallowed_on_release() {
        let mut shortcuts = Shortcuts::seinas();
        shortcuts.hold(1);
        assert_eq!(
            shortcuts.filter(1, false, keysym::ESCAPE, keysym::ESCAPE, ALT),
            Decision::Swallow
        );
        assert_eq!(
            shortcuts.filter(1, false, keysym::ESCAPE, keysym::ESCAPE, ALT),
            Decision::Forward
        );
        assert!(is_modifier_key(0xffe9), "Alt_L");
        assert!(is_modifier_key(0xffe5), "Caps_Lock");
        assert!(!is_modifier_key(TAB));
        assert!(!is_modifier_key(keysym::ESCAPE));
    }
}
