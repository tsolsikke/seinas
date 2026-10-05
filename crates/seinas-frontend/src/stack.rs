//! ウィンドウの並び(重ね方)と、置き場所の決まり。
//!
//! Waylandのオブジェクトに触れずに確かめられるよう、並べるものの型 `T` を決めずに書いてある。
//! 受け口は、`T` をtoplevelにして使う。決まりの説明は `docs/window-placement.md` にある。

use smithay::utils::{Logical, Point, Rectangle, Size};

/// 新しいウィンドウを、前のものからずらす量(画素)。右へも下へも、この量だけずらす。
pub const CASCADE_STEP: i32 = 32;

/// 置き場所の数。ウィンドウの左上が、画面の短い辺の半分を超えない範囲に収まるだけ用意する。
pub fn slot_count(screen: Size<i32, Logical>) -> usize {
    ((screen.w.min(screen.h) / 2 / CASCADE_STEP).max(1)) as usize
}

/// 新しいウィンドウの置き場所の番号を決める。
///
/// 空いている中で、いちばん小さい番号を選ぶ。全部ふさがっていれば、先頭の番号から順に、もう一度使う
/// (同じ位置に重なる)。`occupied` は、いまあるウィンドウの番号(動かされて番号を手放したものは入れない)。
pub fn next_slot(occupied: &[usize], slots: usize) -> usize {
    (0..slots)
        .find(|slot| !occupied.contains(slot))
        .unwrap_or(occupied.len() % slots)
}

/// 置き場所の番号から、左上の位置を求める。
pub fn slot_location(slot: usize) -> Point<i32, Logical> {
    let offset = slot as i32 * CASCADE_STEP;
    (offset, offset).into()
}

/// 並びの中のウィンドウを見分ける番号。ウィンドウが去るまで変わらず、使い回さない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

/// 置き場所の決まった、1つのウィンドウ。
pub struct Placed<T> {
    pub item: T,
    pub id: WindowId,
    /// 置き場所の番号。0が左上で、1つ増えるごとに右下へずれる。動かされたウィンドウは、番号を
    /// 手放してNoneになる(その番号は、次の新しいウィンドウが使える)。
    pub slot: Option<usize>,
    /// 画面の中での、左上の位置。
    pub location: Point<i32, Logical>,
}

/// ウィンドウの並び。先頭がいちばん手前。
///
/// いちばん手前の1つだけが「選ばれている(activated)」ウィンドウである。
pub struct Stack<T> {
    entries: Vec<Placed<T>>,
    /// 次に加えるウィンドウに付ける番号。
    next_id: u64,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Stack {
            entries: Vec::new(),
            next_id: 0,
        }
    }
}

impl<T> Stack<T> {
    /// 手前から順に並べる。
    pub fn iter(&self) -> impl Iterator<Item = &Placed<T>> {
        self.entries.iter()
    }

    /// 手前から順に、「選ばれているか」を添えて並べる。選ばれているのは、いちばん手前の1つだけ。
    pub fn iter_with_activation(&self) -> impl Iterator<Item = (&Placed<T>, bool)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry, index == 0))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 新しいウィンドウを、いちばん手前に加える。置き場所は、前のものからずらして決める。
    pub fn add(&mut self, item: T, screen: Size<i32, Logical>) -> WindowId {
        let occupied: Vec<usize> = self.entries.iter().filter_map(|entry| entry.slot).collect();
        let slot = next_slot(&occupied, slot_count(screen));
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.entries.insert(
            0,
            Placed {
                item,
                id,
                slot: Some(slot),
                location: slot_location(slot),
            },
        );
        id
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
    /// 動かしたウィンドウは、置き場所の番号を手放す。重なる順は変えない。
    pub fn move_to(&mut self, id: WindowId, location: Point<i32, Logical>) -> bool {
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

    /// `is_gone` が真を返すウィンドウを、並びから外す。残ったものの順と置き場所は変わらない。
    pub fn remove(&mut self, is_gone: impl Fn(&T) -> bool) {
        self.entries.retain(|entry| !is_gone(&entry.item));
    }

    /// 画面の上の点 `point` にある、いちばん手前のウィンドウが、手前から何番目か。
    ///
    /// `size_of` は、ウィンドウの大きさを返す。まだ絵を出していないウィンドウ(大きさが無い)には当たらない。
    pub fn index_at(
        &self,
        point: Point<f64, Logical>,
        size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> Option<usize> {
        self.entries.iter().position(|entry| {
            size_of(&entry.item).is_some_and(|size| {
                Rectangle::new(entry.location, size)
                    .to_f64()
                    .contains(point)
            })
        })
    }

    /// 手前から `index` 番目のウィンドウを、いちばん手前に出す。順が変わったらtrue。
    ///
    /// ほかのウィンドウどうしの順と、どのウィンドウの置き場所も、変えない。
    pub fn raise(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.entries.len() {
            return false;
        }
        let entry = self.entries.remove(index);
        self.entries.insert(0, entry);
        true
    }

    /// 点 `point` にある、いちばん手前のウィンドウを、いちばん手前に出す。順が変わったらtrue。
    ///
    /// すでにいちばん手前のとき、どのウィンドウの上でもないときは、何もしない。
    pub fn raise_at(
        &mut self,
        point: Point<f64, Logical>,
        size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> bool {
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
    /// 試験のウィンドウの大きさ(どれも同じ)。
    const SIZE: (i32, i32) = (320, 240);

    fn size(_: &&str) -> Option<Size<i32, Logical>> {
        Some(SIZE.into())
    }

    fn at(x: f64, y: f64) -> Point<f64, Logical> {
        (x, y).into()
    }

    /// 手前から順の、名前の並び。
    fn order(stack: &Stack<&'static str>) -> Vec<&'static str> {
        stack.iter().map(|entry| entry.item).collect()
    }

    /// 名前と置き場所の番号・位置。名前の順に並べる(重ねる順とは別に、置き場所だけを比べるため)。
    fn places(stack: &Stack<&'static str>) -> Vec<(&'static str, Option<usize>, (i32, i32))> {
        let mut places: Vec<_> = stack
            .iter()
            .map(|entry| (entry.item, entry.slot, (entry.location.x, entry.location.y)))
            .collect();
        places.sort();
        places
    }

    /// 選ばれているウィンドウの名前(いつも、ちょうど1つか、ウィンドウが無ければ0)。
    fn activated(stack: &Stack<&'static str>) -> Vec<&'static str> {
        stack
            .iter_with_activation()
            .filter(|(_, active)| *active)
            .map(|(entry, _)| entry.item)
            .collect()
    }

    /// a(番号0、左上)、b(番号1)、c(番号2)の順につないだ並び。手前から c, b, a。
    fn three() -> Stack<&'static str> {
        let mut stack = Stack::default();
        for name in ["a", "b", "c"] {
            stack.add(name, SCREEN.into());
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
        assert_eq!(slot_location(0), Point::from((0, 0)));
        assert_eq!(slot_location(1), Point::from((32, 32)));
        assert_eq!(slot_location(3), Point::from((96, 96)));

        assert_eq!(slot_count((800, 600).into()), 9);
        assert_eq!(slot_count((640, 480).into()), 7);
        assert_eq!(slot_count((1024, 768).into()), 12);
        // どんなに小さい画面でも、1つはある。
        assert_eq!(slot_count((40, 30).into()), 1);
        // いちばん遠い置き場所でも、左上は、画面の短い辺の半分より手前にある。
        for (w, h) in [(800, 600), (640, 480), (1024, 768), (600, 800)] {
            let last = slot_location(slot_count((w, h).into()) - 1);
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

        // 奥のaを押す: aが手前に出る。cとbの順は、そのまま。
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
        // aを手前に出してから、bを外す。空くのはbの番号1で、次のウィンドウはそこに入る。
        stack.raise_at(at(10.0, 10.0), size);
        stack.remove(|name| *name == "b");
        stack.add("d", SCREEN.into());
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
        let a = stack.add("a", SCREEN.into());
        let b = stack.add("b", SCREEN.into());
        stack.add("c", SCREEN.into());

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
        stack.add("d", SCREEN.into());
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

        // 当たり判定は、動かした先で行われる。
        assert_eq!(stack.index_at(at(600.0, 430.0), size), stack.position(a));

        // 去ったウィンドウは、動かせない。番号は、使い回さない。
        stack.remove(|name| *name == "a");
        assert!(!stack.move_to(a, (0, 0).into()));
        assert!(stack.get(a).is_none());
        let e = stack.add("e", SCREEN.into());
        assert_ne!(e, a);
    }

    #[test]
    fn exactly_the_front_window_is_activated() {
        let mut stack: Stack<&'static str> = Stack::default();
        assert!(activated(&stack).is_empty());

        // つないだとき: 新しいものが選ばれ、前のものからは外れる。
        stack.add("a", SCREEN.into());
        assert_eq!(activated(&stack), ["a"]);
        stack.add("b", SCREEN.into());
        assert_eq!(activated(&stack), ["b"]);
        stack.add("c", SCREEN.into());
        assert_eq!(activated(&stack), ["c"]);

        // 手前に出したとき: 出したものが選ばれる。
        stack.raise_at(at(10.0, 10.0), size);
        assert_eq!(activated(&stack), ["a"]);

        // 奥のものが去っても、選ばれているものは変わらない。
        stack.remove(|name| *name == "b");
        assert_eq!(activated(&stack), ["a"]);

        // 手前が去ったとき: その下にあったものが選ばれる。
        stack.remove(|name| *name == "a");
        assert_eq!(activated(&stack), ["c"]);
        stack.remove(|name| *name == "c");
        assert!(activated(&stack).is_empty());
        assert!(stack.is_empty());
    }
}
