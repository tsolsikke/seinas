//! ウィンドウのスタック(重ね方)と、スロットの決まり。
//!
//! Waylandのオブジェクトに触れずに確かめられるよう、並べるものの型 `T` を決めずに書いてある。
//! 受け口は、`T` をtoplevelにして使う。決まりの説明は `docs/window-placement.md` にある。

use crate::geometry::{Point, PointF, Rect, Size};

/// 新しいウィンドウを、前のものからずらす量(ピクセル)。右へも下へも、この量だけずらす。
pub const CASCADE_STEP: i32 = 32;

/// スロットの数。ウィンドウの左上が、使える範囲の短い辺の半分を超えない範囲に収まるだけ用意する。
pub fn slot_count(area: Size) -> usize {
    ((area.w.min(area.h) / 2 / CASCADE_STEP).max(1)) as usize
}

/// 新しいウィンドウのスロット番号を決める。
///
/// 空いている中で、いちばん小さい番号を選ぶ。全部ふさがっていれば、先頭の番号から順に、もう一度使う
/// (同じ位置に重なる)。`occupied` は、いまあるウィンドウの番号(動かされて番号を手放したものは入れない)。
pub fn next_slot(occupied: &[usize], slots: usize) -> usize {
    (0..slots)
        .find(|slot| !occupied.contains(slot))
        .unwrap_or(occupied.len() % slots)
}

/// スロット番号から、使える範囲 `area` の中での左上の位置を求める。
pub fn slot_location(slot: usize, area: Rect) -> Point {
    let offset = slot as i32 * CASCADE_STEP;
    (area.loc.x + offset, area.loc.y + offset).into()
}

/// スタックの中のウィンドウを見分ける番号。ウィンドウが去るまで変わらず、使い回さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

/// スロットの決まった、1つのウィンドウ。
pub struct Placed<T> {
    pub item: T,
    pub id: WindowId,
    /// スロット番号。0が左上で、1つ増えるごとに右下へずれる。動かされたウィンドウは、番号を
    /// 手放してNoneになる(その番号は、次の新しいウィンドウが使える)。
    pub slot: Option<usize>,
    /// 画面の中での、左上の位置。
    pub location: Point,
    /// 最小化しているか。最小化しているウィンドウは、描かず、ポインターも当たらず、activated にもならない。
    pub minimized: bool,
    /// 最大化しているときの、元に戻すための位置と大きさ。最大化していなければNone。
    pub restore: Option<Restore>,
}

impl<T> Placed<T> {
    /// 最大化しているか。
    pub fn is_maximized(&self) -> bool {
        self.restore.is_some()
    }
}

/// 最大化する前の様子。元に戻すときに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restore {
    pub location: Point,
    pub slot: Option<usize>,
    /// 最大化する前に、クライアントへ伝えていたコンテンツの大きさ(伝えていなければNone)。
    pub content: Option<Size>,
}

/// ウィンドウのスタック。先頭がいちばん手前。
///
/// いちばん手前の1つだけが「activated」ウィンドウである。
pub struct Stack<T> {
    entries: Vec<Placed<T>>,
    /// 次に加えるウィンドウに付ける番号。
    next_id: u64,
    /// 見せるだけのウィンドウ(Alt+Tab の切り替えの候補)。重なる順と activated は変えずに、いちばん手前に
    /// 描き、ポインターも当てる。最小化していても描く。
    preview: Option<WindowId>,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Stack {
            entries: Vec::new(),
            next_id: 0,
            preview: None,
        }
    }
}

impl<T> Stack<T> {
    /// 手前から順に並べる。
    pub fn iter(&self) -> impl Iterator<Item = &Placed<T>> {
        self.entries.iter()
    }

    /// 手前から順に、activated かどうかを添えて並べる。activated なのは、最小化していないもののうち、
    /// いちばん手前の 1 つだけ。
    pub fn iter_with_activation(&self) -> impl Iterator<Item = (&Placed<T>, bool)> {
        let active = self.entries.iter().position(|entry| !entry.minimized);
        self.entries
            .iter()
            .enumerate()
            .map(move |(index, entry)| (entry, Some(index) == active))
    }

    /// 見せる順(手前から)に、activated かどうかを添えて並べる。見せるだけのウィンドウがあれば、それを
    /// いちばん手前にする。最小化しているウィンドウは、見せるだけのものでなければ並べない。
    pub fn iter_shown(&self) -> impl Iterator<Item = (&Placed<T>, bool)> {
        let preview = self.preview;
        let first = self
            .iter_with_activation()
            .filter(move |(entry, _)| Some(entry.id) == preview);
        let rest = self
            .iter_with_activation()
            .filter(move |(entry, _)| Some(entry.id) != preview && !entry.minimized);
        first.chain(rest)
    }

    /// 見せるだけのウィンドウを `id` にする(Noneで止める)。変わったらtrue。
    pub fn set_preview(&mut self, id: Option<WindowId>) -> bool {
        let id = id.filter(|&id| self.get(id).is_some());
        std::mem::replace(&mut self.preview, id) != id
    }

    /// 見せるだけのウィンドウ。
    pub fn preview(&self) -> Option<WindowId> {
        self.preview
    }

    /// Alt+Tab で切り替える順。activated のウィンドウを先に、ほかを重なる順(手前から)に並べる。最小化して
    /// いるウィンドウも入る。
    ///
    /// ウィンドウは activate したときに raise するので、重なる順は、最後に activated になった順と同じになる
    /// (最小化しても順は変わらないので、activated のウィンドウだけを先に出す)。
    pub fn switch_order(&self) -> Vec<WindowId> {
        let mut order: Vec<(WindowId, bool)> = self
            .iter_with_activation()
            .map(|(entry, active)| (entry.id, active))
            .collect();
        order.sort_by_key(|&(_, active)| !active);
        order.into_iter().map(|(id, _)| id).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 新しいウィンドウを、いちばん手前に加える。スロットは、使える範囲 `area` の中で、前のものから
    /// ずらして決める。
    pub fn add(&mut self, item: T, area: Rect) -> WindowId {
        let occupied: Vec<usize> = self.entries.iter().filter_map(|entry| entry.slot).collect();
        let slot = next_slot(&occupied, slot_count(area.size));
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.entries.insert(
            0,
            Placed {
                item,
                id,
                slot: Some(slot),
                location: slot_location(slot, area),
                minimized: false,
                restore: None,
            },
        );
        id
    }

    /// 使える範囲が `area` に変わったときに、ウィンドウの位置を直す。位置が変わったウィンドウがあればtrue。
    ///
    /// スロットにあるウィンドウは、新しい範囲の同じスロットへ移す。動かされた(スロットの無い)ウィンドウは、
    /// タイトルバーが新しい範囲に入るように直す(`clamp` は、外枠の幅と左上の位置から、置く位置を返す)。
    /// 重なる順は変えない。
    pub fn rearrange(
        &mut self,
        area: Rect,
        size_of: impl Fn(&T) -> Option<Size>,
        clamp: impl Fn(Point, i32, Rect) -> Point,
    ) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            let location = match entry.slot {
                // 最大化しているウィンドウは、新しい範囲の左上へ(大きさは、呼ぶ側が configure で伝え直す)。
                _ if entry.restore.is_some() => area.loc,
                Some(slot) => slot_location(slot, area),
                None => match size_of(&entry.item) {
                    Some(size) => clamp(entry.location, size.w, area),
                    None => entry.location,
                },
            };
            if location != entry.location {
                entry.location = location;
                changed = true;
            }
        }
        changed
    }

    /// 番号が `id` のウィンドウ。もう去っていればNone。
    pub fn get(&self, id: WindowId) -> Option<&Placed<T>> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// 番号が `id` のウィンドウが、手前から何番目か。
    pub fn position(&self, id: WindowId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    /// 番号が `id` のウィンドウを、左上が `location` に来るように動かす。位置が変わったらtrue。
    ///
    /// 動かしたウィンドウは、スロット番号を手放す。重なる順は変えない。
    pub fn move_to(&mut self, id: WindowId, location: Point) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.location == location {
            return false;
        }
        entry.location = location;
        entry.slot = None;
        true
    }

    /// 番号が `id` のウィンドウを最大化する。左上を使える範囲 `area` の左上にし、元の位置とスロット番号、
    /// 伝えていた大きさ `content` を覚える。最大化したらtrue(すでに最大化していればfalse)。
    pub fn maximize(&mut self, id: WindowId, area: Rect, content: Option<Size>) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.restore.is_some() {
            return false;
        }
        entry.restore = Some(Restore {
            location: entry.location,
            slot: entry.slot,
            content,
        });
        entry.location = area.loc;
        entry.slot = None;
        true
    }

    /// 番号が `id` のウィンドウの最大化を戻す。スロット番号と位置を最大化する前に戻し、覚えていた様子を返す。
    /// 最大化していなければNone。
    ///
    /// スロットにあったウィンドウは、いまの使える範囲 `area` の、そのスロットの位置へ戻す(最大化している間に、
    /// 使える範囲が変わっていることがある)。動かされていたウィンドウは、前の位置へ戻す(範囲に入れるのは、
    /// 呼ぶ側が行う)。
    pub fn unmaximize(&mut self, id: WindowId, area: Rect) -> Option<Restore> {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id)?;
        let restore = entry.restore.take()?;
        entry.location = match restore.slot {
            Some(slot) => slot_location(slot, area),
            None => restore.location,
        };
        entry.slot = restore.slot;
        Some(restore)
    }

    /// 番号が `id` のウィンドウを、最小化する(`minimized` がtrue)か、戻す。変わったらtrue。
    ///
    /// 重なる順は変えない(戻したウィンドウを raise するのは、呼ぶ側が [`Stack::raise`] で行う)。
    pub fn set_minimized(&mut self, id: WindowId, minimized: bool) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        std::mem::replace(&mut entry.minimized, minimized) != minimized
    }

    /// `is_gone` が真を返すウィンドウを、スタックから外す。残ったものの順とスロットは変わらない。
    pub fn remove(&mut self, is_gone: impl Fn(&T) -> bool) {
        self.entries.retain(|entry| !is_gone(&entry.item));
        if self.preview.is_some_and(|id| self.get(id).is_none()) {
            self.preview = None;
        }
    }

    /// 画面の上の点 `point` にある、いちばん手前のウィンドウが、手前から何番目か。
    ///
    /// `size_of` は、ウィンドウの大きさを返す。まだ絵を出していないウィンドウ(大きさが無い)と、最小化している
    /// ウィンドウには当たらない。見せるだけのウィンドウがあれば、それを先に見る([`Stack::iter_shown`] の順)。
    pub fn index_at(&self, point: PointF, size_of: impl Fn(&T) -> Option<Size>) -> Option<usize> {
        let id = self
            .iter_shown()
            .find(|(entry, _)| {
                size_of(&entry.item)
                    .is_some_and(|size| Rect::new(entry.location, size).contains(point))
            })
            .map(|(entry, _)| entry.id)?;
        self.position(id)
    }

    /// 手前から `index` 番目のウィンドウを、raise する。順が変わったらtrue。
    ///
    /// ほかのウィンドウどうしの順と、どのウィンドウのスロットも、変えない。
    pub fn raise(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.entries.len() {
            return false;
        }
        let entry = self.entries.remove(index);
        self.entries.insert(0, entry);
        true
    }

    /// 点 `point` にある、いちばん手前のウィンドウを、raise する。順が変わったらtrue。
    ///
    /// すでにいちばん手前のとき、どのウィンドウの上でもないときは、何もしない。
    pub fn raise_at(&mut self, point: PointF, size_of: impl Fn(&T) -> Option<Size>) -> bool {
        match self.index_at(point, size_of) {
            Some(index) => self.raise(index),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i32, i32) = (640, 480);
    /// テストのウィンドウの大きさ(どれも同じ)。
    const SIZE: (i32, i32) = (320, 240);

    fn size(_: &&str) -> Option<Size> {
        Some(SIZE.into())
    }

    fn at(x: f64, y: f64) -> PointF {
        (x, y).into()
    }

    /// 手前から順の、名前の並び。
    fn order(stack: &Stack<&'static str>) -> Vec<&'static str> {
        stack.iter().map(|entry| entry.item).collect()
    }

    /// 名前とスロット番号・位置。名前の順に並べる(重ねる順とは別に、スロットだけを比べるため)。
    fn places(stack: &Stack<&'static str>) -> Vec<(&'static str, Option<usize>, (i32, i32))> {
        let mut places: Vec<_> = stack
            .iter()
            .map(|entry| (entry.item, entry.slot, (entry.location.x, entry.location.y)))
            .collect();
        places.sort();
        places
    }

    /// activated なウィンドウの名前(いつも、ちょうど1つか、ウィンドウが無ければ0)。
    fn activated(stack: &Stack<&'static str>) -> Vec<&'static str> {
        stack
            .iter_with_activation()
            .filter(|(_, active)| *active)
            .map(|(entry, _)| entry.item)
            .collect()
    }

    /// a(番号0、左上)、b(番号1)、c(番号2)の順につないだスタック。手前から c, b, a。
    fn three() -> Stack<&'static str> {
        let mut stack = Stack::default();
        for name in ["a", "b", "c"] {
            stack.add(name, Rect::from_size(SCREEN.into()));
        }
        stack
    }

    #[test]
    fn a_new_window_takes_the_lowest_free_slot() {
        assert_eq!(next_slot(&[], 7), 0);
        assert_eq!(next_slot(&[0], 7), 1);
        assert_eq!(next_slot(&[1, 0], 7), 2);
        // 途中が空けば、そこを使う。
        assert_eq!(next_slot(&[2, 0], 7), 1);
        assert_eq!(next_slot(&[2, 1], 7), 0);
    }

    #[test]
    fn slots_wrap_around_when_all_are_taken() {
        assert_eq!(next_slot(&[0, 1, 2], 3), 0);
        assert_eq!(next_slot(&[0, 1, 2, 0], 3), 1);
        assert_eq!(next_slot(&[0, 1, 2, 0, 1], 3), 2);
        assert_eq!(next_slot(&[0], 1), 0);
    }

    #[test]
    fn slots_step_down_and_right_and_stay_in_the_upper_left_half() {
        let screen = Rect::from_size((640, 480).into());
        assert_eq!(slot_location(0, screen), Point::from((0, 0)));
        assert_eq!(slot_location(1, screen), Point::from((32, 32)));
        assert_eq!(slot_location(3, screen), Point::from((96, 96)));
        // 使える範囲が画面の左上から始まらないとき(上にタスクバーがあるときなど)は、その左上が基準。
        let below_a_bar = Rect::new((0, 40).into(), (640, 440).into());
        assert_eq!(slot_location(0, below_a_bar), Point::from((0, 40)));
        assert_eq!(slot_location(2, below_a_bar), Point::from((64, 104)));

        assert_eq!(slot_count((800, 600).into()), 9);
        assert_eq!(slot_count((640, 480).into()), 7);
        assert_eq!(slot_count((1024, 768).into()), 12);
        // どんなに小さい画面でも、1つはある。
        assert_eq!(slot_count((40, 30).into()), 1);
        // いちばん遠いスロットでも、左上は、画面の短い辺の半分より手前にある。
        for (w, h) in [(800, 600), (640, 480), (1024, 768), (600, 800)] {
            let last = slot_location(
                slot_count((w, h).into()) - 1,
                Rect::from_size((w, h).into()),
            );
            assert!(last.x < w.min(h) / 2 && last.y < w.min(h) / 2);
        }
    }

    #[test]
    fn new_windows_go_in_front_with_an_offset() {
        let stack = three();
        assert_eq!(order(&stack), ["c", "b", "a"]);
        assert_eq!(
            places(&stack),
            [
                ("a", Some(0), (0, 0)),
                ("b", Some(1), (32, 32)),
                ("c", Some(2), (64, 64))
            ]
        );
    }

    #[test]
    fn the_front_window_under_the_point_is_chosen() {
        let stack = three();
        // 3つとも重なっている所(c、b、aの順に手前)では、いちばん手前のc。
        assert_eq!(stack.index_at(at(100.0, 100.0), size), Some(0));
        // cが届かず、bとaが重なっている所では、b。
        assert_eq!(stack.index_at(at(40.0, 40.0), size), Some(1));
        assert_eq!(stack.index_at(at(200.0, 50.0), size), Some(1));
        // aだけがある所(左と上の端)では、a。
        assert_eq!(stack.index_at(at(10.0, 10.0), size), Some(2));
        assert_eq!(stack.index_at(at(10.0, 200.0), size), Some(2));
        // cだけがある所(右と下の端)では、c。
        assert_eq!(stack.index_at(at(380.0, 300.0), size), Some(0));
        // どのウィンドウの上でもない所。
        assert_eq!(stack.index_at(at(500.0, 400.0), size), None);
        assert_eq!(stack.index_at(at(10.0, 300.0), size), None);
        // 端の扱い: 左上の点は含み、右下の外側の点は含まない。
        assert_eq!(stack.index_at(at(0.0, 0.0), size), Some(2));
        assert_eq!(stack.index_at(at(384.0, 304.0), size), None);
    }

    #[test]
    fn a_window_without_a_size_is_never_hit() {
        let stack = three();
        // cがまだ絵を出していないとき、cのある所ではその下のbに当たる。
        let size_without_c = |name: &&str| (*name != "c").then(|| SIZE.into());
        assert_eq!(stack.index_at(at(100.0, 100.0), size_without_c), Some(1));
        assert_eq!(stack.index_at(at(380.0, 300.0), size_without_c), None);
    }

    #[test]
    fn raising_keeps_the_other_order_and_every_place() {
        let mut stack = three();
        let before = places(&stack);

        // 奥のaを押す: aが raise される。cとbの順は、そのまま。
        assert!(stack.raise_at(at(10.0, 10.0), size));
        assert_eq!(order(&stack), ["a", "c", "b"]);
        assert_eq!(places(&stack), before, "raising must not move any window");

        // 真ん中にあるcを押す(aの外で、cのある所)。
        assert!(stack.raise_at(at(380.0, 300.0), size));
        assert_eq!(order(&stack), ["c", "a", "b"]);
        assert_eq!(places(&stack), before);

        // すでにいちばん手前のcを押しても、何も変わらない。
        assert!(!stack.raise_at(at(380.0, 300.0), size));
        assert_eq!(order(&stack), ["c", "a", "b"]);

        // どのウィンドウの上でもない所を押しても、何も変わらない。
        assert!(!stack.raise_at(at(500.0, 400.0), size));
        assert_eq!(order(&stack), ["c", "a", "b"]);
        assert_eq!(places(&stack), before);

        // 範囲の外の番号は、何もしない。
        assert!(!stack.raise(3));
        assert_eq!(order(&stack), ["c", "a", "b"]);
    }

    #[test]
    fn a_raised_window_keeps_its_slot_for_later_windows() {
        let mut stack = three();
        // aを raise してから、bを外す。空くのはbの番号1で、次のウィンドウはそこに入る。
        stack.raise_at(at(10.0, 10.0), size);
        stack.remove(|name| *name == "b");
        stack.add("d", Rect::from_size(SCREEN.into()));
        assert_eq!(order(&stack), ["d", "a", "c"]);
        assert_eq!(
            places(&stack),
            [
                ("a", Some(0), (0, 0)),
                ("c", Some(2), (64, 64)),
                ("d", Some(1), (32, 32))
            ]
        );
    }

    #[test]
    fn a_moved_window_gives_up_its_slot_and_keeps_its_position() {
        let mut stack: Stack<&'static str> = Stack::default();
        let a = stack.add("a", Rect::from_size(SCREEN.into()));
        let b = stack.add("b", Rect::from_size(SCREEN.into()));
        stack.add("c", Rect::from_size(SCREEN.into()));

        // 奥のaを動かす: 位置が変わり、番号を手放す。重なる順は変わらない。
        assert!(stack.move_to(a, (300, 200).into()));
        assert_eq!(order(&stack), ["c", "b", "a"]);
        assert_eq!(
            places(&stack),
            [
                ("a", None, (300, 200)),
                ("b", Some(1), (32, 32)),
                ("c", Some(2), (64, 64))
            ]
        );
        // 同じ位置へ「動かして」も、何も変わらない。
        assert!(!stack.move_to(a, (300, 200).into()));
        assert!(!stack.move_to(b, (32, 32).into()));
        assert_eq!(stack.get(b).unwrap().slot, Some(1));

        // 次の新しいウィンドウは、aが手放した番号0(左上)に入る。aは、動かした先に残る。
        stack.add("d", Rect::from_size(SCREEN.into()));
        assert_eq!(order(&stack), ["d", "c", "b", "a"]);
        assert_eq!(
            places(&stack),
            [
                ("a", None, (300, 200)),
                ("b", Some(1), (32, 32)),
                ("c", Some(2), (64, 64)),
                ("d", Some(0), (0, 0))
            ]
        );

        // ヒット判定は、動かした先で行われる。
        assert_eq!(stack.index_at(at(600.0, 430.0), size), stack.position(a));

        // 去ったウィンドウは、動かせない。番号は、使い回さない。
        stack.remove(|name| *name == "a");
        assert!(!stack.move_to(a, (0, 0).into()));
        assert!(stack.get(a).is_none());
        let e = stack.add("e", Rect::from_size(SCREEN.into()));
        assert_ne!(e, a);
    }

    #[test]
    fn windows_follow_the_usable_area_when_it_changes() {
        let mut stack: Stack<&'static str> = Stack::default();
        let screen = Rect::from_size(SCREEN.into());
        stack.add("a", screen);
        stack.add("b", screen);
        let c = stack.add("c", screen);
        // c は動かされて、スロットを手放している。
        stack.move_to(c, (300, 2).into());
        // 上に高さ 40 のタスクバーが来た: スロットのウィンドウは、新しい範囲の同じスロットへ移る。
        // 動かされたウィンドウは、タイトルバーが範囲に入るように下がる。
        let below_a_bar = Rect::new((0, 40).into(), (640, 440).into());
        let clamp = |p: Point, _: i32, area: Rect| Point::new(p.x, p.y.max(area.loc.y));
        assert!(stack.rearrange(below_a_bar, size, clamp));
        assert_eq!(
            places(&stack),
            [
                ("a", Some(0), (0, 40)),
                ("b", Some(1), (32, 72)),
                ("c", None, (300, 40))
            ]
        );
        // 同じ範囲で、もう一度呼んでも、何も変わらない。
        assert!(!stack.rearrange(below_a_bar, size, clamp));
        // 次の新しいウィンドウも、新しい範囲を基準に置かれる。
        stack.remove(|name| *name == "b");
        stack.add("d", below_a_bar);
        assert_eq!(
            stack
                .iter()
                .find(|e| e.item == "d")
                .map(|e| (e.slot, e.location)),
            Some((Some(1), Point::new(32, 72)))
        );
    }

    #[test]
    fn exactly_the_front_window_is_activated() {
        let mut stack: Stack<&'static str> = Stack::default();
        assert!(activated(&stack).is_empty());

        // つないだとき: 新しいものが選ばれ、前のものからは外れる。
        stack.add("a", Rect::from_size(SCREEN.into()));
        assert_eq!(activated(&stack), ["a"]);
        stack.add("b", Rect::from_size(SCREEN.into()));
        assert_eq!(activated(&stack), ["b"]);
        stack.add("c", Rect::from_size(SCREEN.into()));
        assert_eq!(activated(&stack), ["c"]);

        // raise したとき: 出したものが選ばれる。
        stack.raise_at(at(10.0, 10.0), size);
        assert_eq!(activated(&stack), ["a"]);

        // 奥のものが去っても、activated なものは変わらない。
        stack.remove(|name| *name == "b");
        assert_eq!(activated(&stack), ["a"]);

        // 手前が去ったとき: その下にあったものが選ばれる。
        stack.remove(|name| *name == "a");
        assert_eq!(activated(&stack), ["c"]);
        stack.remove(|name| *name == "c");
        assert!(activated(&stack).is_empty());
        assert!(stack.is_empty());
    }

    #[test]
    fn a_maximized_window_fills_the_area_and_comes_back() {
        let mut stack = three();
        let b = stack.iter().find(|e| e.item == "b").unwrap().id;
        let area = Rect::new((0, 40).into(), (640, 400).into());
        assert!(stack.maximize(b, area, Some((320, 240).into())));
        assert!(!stack.maximize(b, area, None), "already maximized");
        let entry = stack.get(b).unwrap();
        assert!(entry.is_maximized());
        assert_eq!((entry.location, entry.slot), ((0, 40).into(), None));
        // 使える範囲が変わると、最大化しているウィンドウは、新しい範囲の左上へ。
        stack.rearrange(Rect::from_size(SCREEN.into()), size, |p, _, _| p);
        assert_eq!(stack.get(b).unwrap().location, (0, 0).into());
        // 元に戻すと、位置とスロット番号が戻り、伝えていた大きさが返る。
        let restore = stack.unmaximize(b, Rect::from_size(SCREEN.into())).unwrap();
        assert_eq!(restore.content, Some((320, 240).into()));
        let entry = stack.get(b).unwrap();
        assert_eq!((entry.location, entry.slot), ((32, 32).into(), Some(1)));
        assert!(stack
            .unmaximize(b, Rect::from_size(SCREEN.into()))
            .is_none());
    }

    #[test]
    fn the_switch_order_starts_with_the_activated_window() {
        let mut stack = three();
        let id =
            |stack: &Stack<&'static str>, name| stack.iter().find(|e| e.item == name).unwrap().id;
        let names = |stack: &Stack<&'static str>| {
            stack
                .switch_order()
                .into_iter()
                .map(|id| stack.get(id).unwrap().item)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&stack), ["c", "b", "a"]);
        // 手前の c を最小化すると、activated の b が先で、c はその次(最後に activated だった順)。
        stack.set_minimized(id(&stack, "c"), true);
        assert_eq!(names(&stack), ["b", "c", "a"]);
        // 全部最小化すると、重なる順のまま。
        stack.set_minimized(id(&stack, "b"), true);
        stack.set_minimized(id(&stack, "a"), true);
        assert_eq!(names(&stack), ["c", "b", "a"]);
        assert!(Stack::<&str>::default().switch_order().is_empty());
    }

    #[test]
    fn a_preview_is_shown_and_hit_in_front_without_changing_the_order() {
        let mut stack = three();
        let a = stack.iter().find(|e| e.item == "a").unwrap().id;
        let c = stack.iter().find(|e| e.item == "c").unwrap().id;
        stack.set_minimized(a, true);
        assert_eq!(stack.index_at(at(10.0, 10.0), size), None);
        // 最小化している a を見せると、いちばん手前に描かれ、当たる。重なる順と activated は変わらない。
        assert!(stack.set_preview(Some(a)));
        assert!(!stack.set_preview(Some(a)));
        let shown: Vec<_> = stack
            .iter_shown()
            .map(|(e, active)| (e.item, active))
            .collect();
        assert_eq!(shown, [("a", false), ("c", true), ("b", false)]);
        assert_eq!(stack.index_at(at(100.0, 100.0), size), Some(2));
        assert_eq!(order(&stack), ["c", "b", "a"]);
        assert_eq!(activated(&stack), ["c"]);
        // 見せるのを止めると、元どおり。去ったウィンドウは、見せるのも止まる。
        assert!(stack.set_preview(None));
        assert_eq!(stack.index_at(at(100.0, 100.0), size), Some(0));
        stack.set_preview(Some(c));
        stack.remove(|name| *name == "c");
        assert_eq!(stack.preview(), None);
        assert!(!stack.set_preview(Some(c)), "a window that is gone");
    }

    #[test]
    fn a_minimized_window_is_neither_hit_nor_activated() {
        let mut stack = three();
        let c = stack.iter().find(|e| e.item == "c").unwrap().id;
        assert!(stack.set_minimized(c, true));
        assert!(!stack.set_minimized(c, true));
        // 手前の c は当たらず、その下の b が当たる。activated は b に移る。
        assert_eq!(stack.index_at(at(100.0, 100.0), size), Some(1));
        assert_eq!(activated(&stack), ["b"]);
        // 重なる順は変わらない。
        assert_eq!(order(&stack), ["c", "b", "a"]);
        assert!(stack.set_minimized(c, false));
        assert_eq!(activated(&stack), ["c"]);
        // 全部最小化すると、activated のウィンドウは無い。
        let ids: Vec<WindowId> = stack.iter().map(|e| e.id).collect();
        for id in ids {
            stack.set_minimized(id, true);
        }
        assert!(activated(&stack).is_empty());
        assert_eq!(stack.index_at(at(10.0, 10.0), size), None);
    }
}
