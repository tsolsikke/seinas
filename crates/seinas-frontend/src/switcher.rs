//! Alt+Tab でのウィンドウの切り替え(候補の順と、いまの候補)。
//!
//! 候補は、切り替えを始めたときのウィンドウを [`Stack::switch_order`](crate::Stack::switch_order) の順に
//! 並べたもの。先頭が、始めたときの activated のウィンドウ。Tab で次へ、Shift+Tab で前へ進み、端では反対の端へ
//! 回る。決める・取り消すのは、呼ぶ側が行う。

use crate::WindowId;

/// 切り替えの最中の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switcher {
    candidates: Vec<WindowId>,
    index: usize,
}

impl Switcher {
    /// 候補 `candidates` で切り替えを始め、最初の 1 歩を進める(`backward` なら前へ)。候補が無ければNone。
    pub fn start(candidates: Vec<WindowId>, backward: bool) -> Option<Self> {
        if candidates.is_empty() {
            return None;
        }
        let mut switcher = Switcher {
            candidates,
            index: 0,
        };
        switcher.step(backward);
        Some(switcher)
    }

    /// 次(`backward` なら前)の候補へ進む。
    pub fn step(&mut self, backward: bool) {
        let len = self.candidates.len();
        self.index = if backward {
            (self.index + len - 1) % len
        } else {
            (self.index + 1) % len
        };
    }

    /// いまの候補。
    pub fn current(&self) -> WindowId {
        self.candidates[self.index]
    }

    /// 去ったウィンドウ(`is_gone` が真のもの)を、候補から外す。いまの候補が去ったら、その次の候補にする。
    /// 候補が無くなったらfalse。
    pub fn remove(&mut self, is_gone: impl Fn(WindowId) -> bool) -> bool {
        let before = self.index;
        let mut index = 0;
        let mut kept = 0;
        self.candidates.retain(|&id| {
            let gone = is_gone(id);
            // いまの候補より前で外れた分だけ、番号を詰める。
            if gone && index < before {
                kept += 1;
            }
            index += 1;
            !gone
        });
        if self.candidates.is_empty() {
            return false;
        }
        self.index = (before - kept) % self.candidates.len();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Stack;

    /// 番号が 0、1、2…のウィンドウ(手前から a、b、c…の順に並べた Stack の番号)。
    fn ids(n: usize) -> Vec<WindowId> {
        let mut stack = Stack::default();
        for _ in 0..n {
            stack.add((), crate::geometry::Rect::from_size((640, 480).into()));
        }
        stack.switch_order()
    }

    #[test]
    fn tab_goes_forward_and_shift_tab_goes_back_around_the_ends() {
        let [a, b, c] = ids(3)[..] else { panic!() };
        let mut switcher = Switcher::start(vec![a, b, c], false).unwrap();
        assert_eq!(switcher.current(), b);
        switcher.step(false);
        assert_eq!(switcher.current(), c);
        switcher.step(false);
        assert_eq!(switcher.current(), a, "around the end");
        switcher.step(true);
        assert_eq!(switcher.current(), c, "back around the start");
        // Shift+Tab で始めると、いちばん後ろの候補から。
        let switcher = Switcher::start(vec![a, b, c], true).unwrap();
        assert_eq!(switcher.current(), c);
    }

    #[test]
    fn no_window_means_no_switching_and_one_window_stays() {
        assert_eq!(Switcher::start(Vec::new(), false), None);
        let [a] = ids(1)[..] else { panic!() };
        let mut switcher = Switcher::start(vec![a], false).unwrap();
        assert_eq!(switcher.current(), a);
        switcher.step(true);
        assert_eq!(switcher.current(), a);
    }

    #[test]
    fn a_closed_candidate_is_skipped() {
        let [a, b, c, d] = ids(4)[..] else { panic!() };
        let mut switcher = Switcher::start(vec![a, b, c, d], false).unwrap();
        switcher.step(false);
        assert_eq!(switcher.current(), c);
        // いまの候補より前(b)が去っても、いまの候補は変わらない。
        assert!(switcher.remove(|id| id == b));
        assert_eq!(switcher.current(), c);
        // いまの候補(c)が去ると、その次(d)になる。最後なら先頭へ回る。
        assert!(switcher.remove(|id| id == c));
        assert_eq!(switcher.current(), d);
        assert!(switcher.remove(|id| id == d));
        assert_eq!(switcher.current(), a);
        assert!(!switcher.remove(|id| id == a), "no candidate is left");
    }
}
