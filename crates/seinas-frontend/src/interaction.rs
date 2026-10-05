//! ポインターでのウィンドウの操作(動かす、閉じる)の決まり。
//!
//! ボタンを押した所が、ウィンドウのどの部分か(中身、題名の帯、閉じるボタン)で、することを決める。
//! Waylandのオブジェクトに触れずに確かめられるよう、並べるものの型 `T` を決めずに書いてある。
//! 決まりの説明は `docs/window-placement.md` にある。

use smithay::utils::{Logical, Point, Rectangle, Size};

use crate::{
    decoration::{clamp_outer_location, CLOSE_BUTTON_SIZE, TITLE_BAR_HEIGHT},
    stack::{Stack, WindowId},
};

/// 左のボタンの番号(Linuxの入力の決まり)。動かす・閉じる操作は、このボタンだけで行う。
pub const LEFT_BUTTON: u32 = 0x110;
/// 帯を押してから、これだけ(画素)動くまでは、ウィンドウを動かさない。手前に出すだけのつもりで
/// 押したときに、ずれないようにする。
pub const DRAG_SLACK: f64 = 4.0;

/// ウィンドウの中の部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// クライアントが描く中身。
    Content,
    /// 題名の帯(閉じるボタンを除く)。
    Bar,
    /// 閉じるボタン。
    CloseButton,
}

/// 外形の左上が `location`、外形の大きさが `outer` のウィンドウの中で、点 `point` がどの部分か。
/// ウィンドウの外ならNone。
pub fn part_at(
    location: Point<i32, Logical>,
    outer: Size<i32, Logical>,
    point: Point<f64, Logical>,
) -> Option<Part> {
    if !Rectangle::new(location, outer).to_f64().contains(point) {
        return None;
    }
    Some(if point.y >= (location.y + TITLE_BAR_HEIGHT) as f64 {
        Part::Content
    } else if point.x >= (location.x + outer.w - CLOSE_BUTTON_SIZE) as f64 {
        Part::CloseButton
    } else {
        Part::Bar
    })
}

/// ボタンを押している間の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// 何も掴んでいない。
    None,
    /// 帯を掴んでいる。
    Bar {
        window: WindowId,
        /// 押した点と、そのときのウィンドウの位置。
        press: Point<f64, Logical>,
        origin: Point<i32, Logical>,
        /// もう動かし始めたか([`DRAG_SLACK`] を超えたか)。
        moving: bool,
    },
    /// 閉じるボタンを押している。
    Close {
        window: WindowId,
        /// いま、ポインターがそのボタンの上にあるか。上で離したときにだけ、閉じる。
        inside: bool,
    },
    /// 中身(か、何も無い所)を押している。ポインターの知らせは、クライアントへ渡す。
    Client {
        /// 押されているボタンの数。
        buttons: u32,
    },
}

/// ポインターの知らせを、だれに渡すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deliver {
    /// ポインターの下にあるウィンドウの中身へ、いつもどおり渡す。
    ToClient,
    /// だれにも渡さない(Seinasが自分で使った)。
    Nobody,
}

/// 1つの知らせを処理した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// 画面を描き直す必要があるか。
    pub redraw: bool,
    /// 重なる順が変わったか(選ばれているウィンドウが変わったかもしれない)。
    pub raised: bool,
    /// 閉じるように頼むウィンドウ。
    pub close: Option<WindowId>,
    pub deliver: Deliver,
}

impl Outcome {
    fn new(deliver: Deliver) -> Self {
        Outcome {
            redraw: false,
            raised: false,
            close: None,
            deliver,
        }
    }
}

/// ポインターでの操作の状態。
#[derive(Debug)]
pub struct Interaction {
    grab: Grab,
    /// ボタンを押していないときに、ポインターが閉じるボタンの上にあるウィンドウ。
    hover: Option<WindowId>,
    /// 最後に見たポインターの位置。
    last: Point<f64, Logical>,
}

impl Default for Interaction {
    fn default() -> Self {
        Interaction {
            grab: Grab::None,
            hover: None,
            last: (0.0, 0.0).into(),
        }
    }
}

impl Interaction {
    /// ウィンドウ `window` の閉じるボタンを、目立たせて描くか。
    ///
    /// ボタンを押していないときは、ポインターがその上にあるとき。閉じるボタンを押している間は、
    /// ポインターがそのボタンの上にあるとき(外へ出すと、取り消しになることが分かる)。
    pub fn close_button_is_hot(&self, window: WindowId) -> bool {
        match self.grab {
            Grab::None => self.hover == Some(window),
            Grab::Close {
                window: pressed,
                inside,
            } => pressed == window && inside,
            _ => false,
        }
    }

    /// ウィンドウを動かしている最中か。
    pub fn is_moving(&self) -> bool {
        matches!(self.grab, Grab::Bar { moving: true, .. })
    }

    /// ポインターが `point` へ動いた。
    ///
    /// `outer_size_of` は、ウィンドウの外形の大きさを返す(まだ絵を出していなければNone)。
    pub fn motion<T>(
        &mut self,
        stack: &mut Stack<T>,
        screen: Size<i32, Logical>,
        point: Point<f64, Logical>,
        outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> Outcome {
        self.last = point;
        match self.grab {
            Grab::None => {
                let mut outcome = Outcome::new(Deliver::ToClient);
                outcome.redraw = self.update_hover(stack, point, outer_size_of);
                outcome
            }
            Grab::Client { .. } => Outcome::new(Deliver::ToClient),
            Grab::Bar {
                window,
                press,
                origin,
                moving,
            } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                let Some(width) = stack
                    .get(window)
                    .and_then(|entry| outer_size_of(&entry.item))
                    .map(|size| size.w)
                else {
                    // 掴んでいたウィンドウが去った。ボタンを離すまで、何もしない。
                    return outcome;
                };
                let (dx, dy) = (point.x - press.x, point.y - press.y);
                let moving = moving || dx.hypot(dy) >= DRAG_SLACK;
                self.grab = Grab::Bar {
                    window,
                    press,
                    origin,
                    moving,
                };
                if moving {
                    let wanted = (origin.x + dx.round() as i32, origin.y + dy.round() as i32);
                    let location = clamp_outer_location(wanted.into(), width, screen);
                    outcome.redraw = stack.move_to(window, location);
                }
                outcome
            }
            Grab::Close { window, inside } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                let now = close_button_under(stack, point, outer_size_of) == Some(window);
                self.grab = Grab::Close {
                    window,
                    inside: now,
                };
                outcome.redraw = now != inside;
                outcome
            }
        }
    }

    /// ポインターが画面の外へ出た。押しているボタンは、離すまで掴んだままにする。
    pub fn leave(&mut self) -> Outcome {
        let mut outcome = Outcome::new(Deliver::Nobody);
        outcome.redraw = self.hover.take().is_some() && self.grab == Grab::None;
        outcome
    }

    /// 点 `point` で、ボタン `button` が押された。
    pub fn press<T>(
        &mut self,
        stack: &mut Stack<T>,
        point: Point<f64, Logical>,
        button: u32,
        outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> Outcome {
        self.last = point;
        match &mut self.grab {
            // 帯や閉じるボタンを掴んでいる間の、ほかのボタンは、使わない。
            Grab::Bar { .. } | Grab::Close { .. } => return Outcome::new(Deliver::Nobody),
            Grab::Client { buttons } => {
                *buttons += 1;
                return Outcome::new(Deliver::ToClient);
            }
            Grab::None => {}
        }
        let hit = stack.index_at(point, &outer_size_of).and_then(|index| {
            let entry = stack.iter().nth(index)?;
            let part = part_at(entry.location, outer_size_of(&entry.item)?, point)?;
            Some((index, entry.id, entry.location, part))
        });
        let Some((index, window, origin, part)) = hit else {
            // どのウィンドウの上でもない。
            self.grab = Grab::Client { buttons: 1 };
            return Outcome::new(Deliver::ToClient);
        };
        // どのボタンでも、どの部分でも、押したウィンドウを手前に出す。
        let raised = stack.raise(index);
        let was_hot = self.close_button_is_hot(window);
        let deliver = match part {
            Part::Content => {
                self.grab = Grab::Client { buttons: 1 };
                Deliver::ToClient
            }
            Part::Bar if button == LEFT_BUTTON => {
                self.grab = Grab::Bar {
                    window,
                    press: point,
                    origin,
                    moving: false,
                };
                Deliver::Nobody
            }
            Part::CloseButton if button == LEFT_BUTTON => {
                self.grab = Grab::Close {
                    window,
                    inside: true,
                };
                Deliver::Nobody
            }
            // 帯の上の、左でないボタンは、手前に出すだけ。
            Part::Bar | Part::CloseButton => Deliver::Nobody,
        };
        self.hover = None;
        Outcome {
            redraw: raised || was_hot != self.close_button_is_hot(window),
            raised,
            close: None,
            deliver,
        }
    }

    /// 点 `point` で、ボタン `button` が離された。
    pub fn release<T>(
        &mut self,
        stack: &mut Stack<T>,
        point: Point<f64, Logical>,
        button: u32,
        outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> Outcome {
        self.last = point;
        let mut outcome = match &mut self.grab {
            Grab::None => return Outcome::new(Deliver::ToClient),
            Grab::Client { buttons } => {
                *buttons = buttons.saturating_sub(1);
                if *buttons > 0 {
                    // ほかのボタンが、まだ押されている。
                    return Outcome::new(Deliver::ToClient);
                }
                Outcome::new(Deliver::ToClient)
            }
            // 帯や閉じるボタンを掴んでいる間の、ほかのボタンは、使わない。
            Grab::Bar { .. } | Grab::Close { .. } if button != LEFT_BUTTON => {
                return Outcome::new(Deliver::Nobody)
            }
            Grab::Bar { .. } => Outcome::new(Deliver::Nobody),
            Grab::Close { window, .. } => {
                let window = *window;
                let mut outcome = Outcome::new(Deliver::Nobody);
                // ボタンの上で離したときにだけ、閉じる。外へ出して離すと、取り消し。
                if close_button_under(stack, point, &outer_size_of) == Some(window) {
                    outcome.close = Some(window);
                }
                outcome.redraw = true;
                outcome
            }
        };
        self.grab = Grab::None;
        outcome.redraw |= self.update_hover(stack, point, outer_size_of);
        outcome
    }

    /// クライアントが、自分のウィンドウ `window` を動かしてほしいと頼んできた。受けたらtrue。
    ///
    /// 受けるのは、中身の上でボタンが押されている間だけ。受けたら、そのボタンを離すまで、帯を
    /// 掴んだときと同じに動かす(遊びは置かず、すぐ動かす)。`press` は、そのボタンを押した点。
    /// 頼みが届くまでの間にポインターが動いたぶんは、ここで動かす。呼ぶ側は、頼んできたのが、
    /// 押されているクライアントであることを確かめておくこと。
    pub fn begin_client_move<T>(
        &mut self,
        stack: &mut Stack<T>,
        screen: Size<i32, Logical>,
        window: WindowId,
        press: Point<f64, Logical>,
        outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> bool {
        let (Grab::Client { .. }, Some(entry)) = (self.grab, stack.get(window)) else {
            return false;
        };
        self.grab = Grab::Bar {
            window,
            press,
            origin: entry.location,
            moving: true,
        };
        self.motion(stack, screen, self.last, outer_size_of);
        true
    }

    /// ボタンを押していないときの、閉じるボタンの上にあるかどうかを更新する。変わったらtrue。
    fn update_hover<T>(
        &mut self,
        stack: &Stack<T>,
        point: Point<f64, Logical>,
        outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
    ) -> bool {
        let hover = close_button_under(stack, point, outer_size_of);
        std::mem::replace(&mut self.hover, hover) != hover
    }
}

/// 点 `point` が、見えている閉じるボタンの上にあれば、そのウィンドウ。
fn close_button_under<T>(
    stack: &Stack<T>,
    point: Point<f64, Logical>,
    outer_size_of: impl Fn(&T) -> Option<Size<i32, Logical>>,
) -> Option<WindowId> {
    let index = stack.index_at(point, &outer_size_of)?;
    let entry = stack.iter().nth(index)?;
    let part = part_at(entry.location, outer_size_of(&entry.item)?, point)?;
    (part == Part::CloseButton).then_some(entry.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i32, i32) = (640, 480);
    /// 試験のウィンドウの外形の大きさ(どれも同じ)。帯が24、中身が320x240。
    const OUTER: (i32, i32) = (320, 264);
    const RIGHT_BUTTON: u32 = 0x111;

    fn size(_: &&str) -> Option<Size<i32, Logical>> {
        Some(OUTER.into())
    }

    fn at(x: f64, y: f64) -> Point<f64, Logical> {
        (x, y).into()
    }

    /// 並びと、操作の状態。知らせを1つずつ送って、結果を見る。
    struct World {
        stack: Stack<&'static str>,
        interaction: Interaction,
    }

    impl World {
        /// a(番号0、左上)、b(番号1、(32, 32))の順につないだ並び。手前はb。
        fn two() -> Self {
            let mut stack = Stack::default();
            stack.add("a", SCREEN.into());
            stack.add("b", SCREEN.into());
            World {
                stack,
                interaction: Interaction::default(),
            }
        }

        fn motion(&mut self, x: f64, y: f64) -> Outcome {
            self.interaction
                .motion(&mut self.stack, SCREEN.into(), at(x, y), size)
        }

        fn press(&mut self, x: f64, y: f64, button: u32) -> Outcome {
            self.interaction
                .press(&mut self.stack, at(x, y), button, size)
        }

        fn release(&mut self, x: f64, y: f64, button: u32) -> Outcome {
            self.interaction
                .release(&mut self.stack, at(x, y), button, size)
        }

        fn begin_client_move(&mut self, window: WindowId, x: f64, y: f64) -> bool {
            self.interaction.begin_client_move(
                &mut self.stack,
                SCREEN.into(),
                window,
                at(x, y),
                size,
            )
        }

        fn id(&self, name: &str) -> WindowId {
            self.stack
                .iter()
                .find(|entry| entry.item == name)
                .expect("the window")
                .id
        }

        /// 名前が `name` のウィンドウの位置と、置き場所の番号。
        fn place(&self, name: &str) -> ((i32, i32), Option<usize>) {
            let entry = self.stack.get(self.id(name)).unwrap();
            ((entry.location.x, entry.location.y), entry.slot)
        }

        fn order(&self) -> Vec<&'static str> {
            self.stack.iter().map(|entry| entry.item).collect()
        }
    }

    #[test]
    fn a_window_has_three_parts() {
        let (location, outer) = ((32, 32).into(), OUTER.into());
        let part = |x: f64, y: f64| part_at(location, outer, at(x, y));
        // 帯は、上の24画素。その右の端の24画素が、閉じるボタン。
        assert_eq!(part(32.0, 32.0), Some(Part::Bar));
        assert_eq!(part(327.9, 55.9), Some(Part::Bar));
        assert_eq!(part(328.0, 32.0), Some(Part::CloseButton));
        assert_eq!(part(351.9, 55.9), Some(Part::CloseButton));
        // その下が中身。
        assert_eq!(part(32.0, 56.0), Some(Part::Content));
        assert_eq!(part(351.0, 56.0), Some(Part::Content));
        assert_eq!(part(351.9, 295.9), Some(Part::Content));
        // 外。
        assert_eq!(part(31.9, 40.0), None);
        assert_eq!(part(352.0, 40.0), None);
        assert_eq!(part(100.0, 296.0), None);
    }

    #[test]
    fn dragging_the_bar_moves_the_window_after_a_slack_of_four_pixels() {
        let mut world = World::two();
        // bの帯(閉じるボタンでない所)を、左のボタンで押す。クライアントには渡さない。
        let pressed = world.press(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(pressed.deliver, Deliver::Nobody);
        assert!(!pressed.redraw, "b is already in front");

        // 4画素に届かない間は、動かない。番号も持ったまま。
        for (x, y) in [(103.0, 40.0), (100.0, 43.9), (102.0, 42.0)] {
            let moved = world.motion(x, y);
            assert_eq!((moved.redraw, moved.deliver), (false, Deliver::Nobody));
            assert_eq!(world.place("b"), ((32, 32), Some(1)));
        }
        assert!(!world.interaction.is_moving());

        // 4画素動くと、動き始める。ウィンドウは、押した点からのずれのぶんだけ動く(遊びのぶんも含む)。
        assert!(world.motion(104.0, 40.0).redraw);
        assert!(world.interaction.is_moving());
        assert_eq!(world.place("b"), ((36, 32), None));
        assert!(world.motion(150.0, 100.0).redraw);
        assert_eq!(world.place("b"), ((82, 92), None));
        // 動き始めた後は、押した点の近くへ戻っても、付いてくる。
        assert!(world.motion(101.0, 41.0).redraw);
        assert_eq!(world.place("b"), ((33, 33), None));
        // 同じ点への知らせでは、描き直さない。
        assert!(!world.motion(101.0, 41.0).redraw);

        // 離すと終わる。その後の動きでは、動かない。
        assert_eq!(
            world.release(101.0, 41.0, LEFT_BUTTON).deliver,
            Deliver::Nobody
        );
        assert!(!world.interaction.is_moving());
        assert_eq!(world.motion(300.0, 300.0).deliver, Deliver::ToClient);
        assert_eq!(world.place("b"), ((33, 33), None));
        // 動かしていないaは、そのまま。
        assert_eq!(world.place("a"), ((0, 0), Some(0)));
    }

    #[test]
    fn a_click_on_the_bar_without_moving_keeps_the_slot() {
        let mut world = World::two();
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(102.0, 41.0);
        world.release(102.0, 41.0, LEFT_BUTTON);
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
    }

    #[test]
    fn pressing_the_bar_of_a_window_behind_raises_it_and_starts_a_drag() {
        let mut world = World::two();
        // aの帯は、bの上にのぞいている。
        let pressed = world.press(100.0, 10.0, LEFT_BUTTON);
        assert!(pressed.raised && pressed.redraw);
        assert_eq!(world.order(), ["a", "b"]);
        world.motion(100.0, 60.0);
        assert_eq!(world.place("a"), ((0, 50), None));
        world.release(100.0, 60.0, LEFT_BUTTON);
        // 次の新しいウィンドウは、aが手放した番号0に入る。
        world.stack.add("c", SCREEN.into());
        assert_eq!(world.place("c"), ((0, 0), Some(0)));
        assert_eq!(world.place("a"), ((0, 50), None));
    }

    #[test]
    fn a_dragged_window_stays_reachable() {
        let mut world = World::two();
        world.press(100.0, 40.0, LEFT_BUTTON);
        // 上へ: 帯の上の端は、画面の上の端で止まる。
        world.motion(100.0, -200.0);
        assert_eq!(world.place("b").0, (32, 0));
        // 下へ: 帯の全体が残る。
        world.motion(100.0, 2000.0);
        assert_eq!(world.place("b").0, (32, 456));
        // 左へ、右へ: 帯のうち48画素が残る。
        world.motion(-2000.0, 100.0);
        assert_eq!(world.place("b").0, (-272, 92));
        world.motion(2000.0, 100.0);
        assert_eq!(world.place("b").0, (592, 92));
        // 限りに当たった後も、ポインターが戻れば、付いてくる。
        world.motion(110.0, 50.0);
        assert_eq!(world.place("b").0, (42, 42));
    }

    #[test]
    fn other_buttons_on_the_bar_only_raise() {
        let mut world = World::two();
        let pressed = world.press(100.0, 10.0, RIGHT_BUTTON);
        assert!(pressed.raised);
        assert_eq!(pressed.deliver, Deliver::Nobody);
        world.motion(200.0, 100.0);
        assert_eq!(world.place("a"), ((0, 0), Some(0)));
        assert_eq!(
            world.release(200.0, 100.0, RIGHT_BUTTON).deliver,
            Deliver::ToClient
        );

        // 右のボタンでは、閉じるボタンも働かない。
        world.press(310.0, 10.0, RIGHT_BUTTON);
        assert_eq!(world.release(310.0, 10.0, RIGHT_BUTTON).close, None);
    }

    #[test]
    fn the_close_button_works_when_released_on_it() {
        let mut world = World::two();
        let b = world.id("b");
        // bの閉じるボタン(帯の右の端)。
        let pressed = world.press(340.0, 40.0, LEFT_BUTTON);
        assert_eq!((pressed.deliver, pressed.close), (Deliver::Nobody, None));
        assert!(world.interaction.close_button_is_hot(b));
        // ボタンの中で動かしても、ウィンドウは動かない。
        let moved = world.motion(345.0, 50.0);
        assert_eq!((moved.redraw, moved.deliver), (false, Deliver::Nobody));
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
        // 上で離すと、閉じるように頼む。
        let released = world.release(345.0, 50.0, LEFT_BUTTON);
        assert_eq!(released.close, Some(b));
        assert_eq!(released.deliver, Deliver::Nobody);
    }

    #[test]
    fn leaving_the_close_button_before_releasing_cancels() {
        let mut world = World::two();
        let b = world.id("b");
        world.press(340.0, 40.0, LEFT_BUTTON);
        // 外へ出すと、目立たせるのをやめる。ウィンドウは動かない。
        assert!(world.motion(200.0, 100.0).redraw);
        assert!(!world.interaction.close_button_is_hot(b));
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
        // 戻すと、また目立たせる。
        assert!(world.motion(340.0, 40.0).redraw);
        assert!(world.interaction.close_button_is_hot(b));
        // 外で離すと、取り消し。
        world.motion(200.0, 100.0);
        assert_eq!(world.release(200.0, 100.0, LEFT_BUTTON).close, None);

        // 別のウィンドウの閉じるボタンの上で離しても、閉じない。
        world.press(340.0, 40.0, LEFT_BUTTON);
        world.motion(310.0, 10.0);
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close, None);
        assert_eq!(world.order(), ["b", "a"]);
    }

    #[test]
    fn the_close_button_of_a_window_behind_works_with_one_click() {
        let mut world = World::two();
        let a = world.id("a");
        // aの閉じるボタンは(296..320, 0..24)。bは(32, 32)から始まるので、隠れていない。
        let pressed = world.press(310.0, 10.0, LEFT_BUTTON);
        assert!(pressed.raised, "the window comes to the front first");
        assert_eq!(world.order(), ["a", "b"]);
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close, Some(a));

        // 隠れている閉じるボタンには、当たらない。手前のウィンドウの部分として扱う。
        let mut world = World::two();
        world.stack.move_to(world.id("b"), (40, 0).into());
        let pressed = world.press(310.0, 10.0, LEFT_BUTTON);
        assert!(!pressed.raised);
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close, None);
    }

    #[test]
    fn the_close_button_is_hot_while_the_pointer_is_on_it() {
        let mut world = World::two();
        let (a, b) = (world.id("a"), world.id("b"));
        assert!(!world.motion(100.0, 100.0).redraw);
        assert!(world.motion(340.0, 40.0).redraw);
        assert!(world.interaction.close_button_is_hot(b));
        assert!(!world.interaction.close_button_is_hot(a));
        assert!(!world.motion(341.0, 41.0).redraw);
        // 奥のウィンドウのボタンへ移ると、入れ替わる。
        assert!(world.motion(310.0, 10.0).redraw);
        assert!(world.interaction.close_button_is_hot(a));
        assert!(!world.interaction.close_button_is_hot(b));
        // 画面の外へ出ると、消える。
        assert!(world.interaction.leave().redraw);
        assert!(!world.interaction.close_button_is_hot(a));
        assert!(!world.interaction.leave().redraw);

        // 帯を掴んで動かしている間は、どのボタンも目立たせない。
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(310.0, 10.0);
        assert!(!world.interaction.close_button_is_hot(a));
        assert!(!world.interaction.close_button_is_hot(b));
    }

    #[test]
    fn the_content_gets_the_pointer_and_the_bar_does_not() {
        let mut world = World::two();
        // 中身の上でのボタンは、クライアントへ渡す。押したまま外へ出ても、離すまでは渡し続ける。
        assert_eq!(
            world.press(100.0, 100.0, LEFT_BUTTON).deliver,
            Deliver::ToClient
        );
        assert_eq!(world.motion(100.0, 40.0).deliver, Deliver::ToClient);
        assert_eq!(
            world.place("b"),
            ((32, 32), Some(1)),
            "this is not a drag of the bar"
        );
        assert_eq!(
            world.release(100.0, 40.0, LEFT_BUTTON).deliver,
            Deliver::ToClient
        );

        // 2つのボタンを押したときは、両方を離すまで。
        world.press(100.0, 100.0, LEFT_BUTTON);
        world.press(100.0, 100.0, RIGHT_BUTTON);
        world.release(100.0, 100.0, LEFT_BUTTON);
        // まだ右を押しているので、帯の上で左を押しても、掴まない。
        assert_eq!(
            world.press(100.0, 40.0, LEFT_BUTTON).deliver,
            Deliver::ToClient
        );
        world.release(100.0, 40.0, LEFT_BUTTON);
        world.release(100.0, 40.0, RIGHT_BUTTON);
        assert_eq!(
            world.press(100.0, 40.0, LEFT_BUTTON).deliver,
            Deliver::Nobody
        );
        world.release(100.0, 40.0, LEFT_BUTTON);

        // 中身を押すと、奥のウィンドウも手前に出る。
        let pressed = world.press(10.0, 200.0, LEFT_BUTTON);
        assert!(pressed.raised);
        assert_eq!(pressed.deliver, Deliver::ToClient);
        assert_eq!(world.order(), ["a", "b"]);
        world.release(10.0, 200.0, LEFT_BUTTON);

        // どのウィンドウの上でもない所。
        let pressed = world.press(600.0, 400.0, LEFT_BUTTON);
        assert!(!pressed.raised && !pressed.redraw);
    }

    #[test]
    fn a_client_may_ask_to_be_moved_only_while_a_button_is_held_on_it() {
        let mut world = World::two();
        let b = world.id("b");
        // 何も押していないときの頼みは、受けない。
        assert!(!world.begin_client_move(b, 100.0, 100.0));
        // 帯を掴んでいるときの頼みも、受けない。
        world.press(100.0, 40.0, LEFT_BUTTON);
        assert!(!world.begin_client_move(b, 100.0, 40.0));
        world.release(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(world.place("b"), ((32, 32), Some(1)));

        // 中身を押している間の頼みは、受ける。頼みが届くまでの間に動いたぶんは、受けたときに動く。
        world.press(100.0, 100.0, LEFT_BUTTON);
        world.motion(102.0, 100.0);
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
        assert!(world.begin_client_move(b, 100.0, 100.0));
        assert!(world.interaction.is_moving());
        assert_eq!(world.place("b"), ((34, 32), None));
        // 遊びは無く、1画素でも動く。知らせは、クライアントには渡さない。
        let moved = world.motion(101.0, 100.0);
        assert_eq!((moved.redraw, moved.deliver), (true, Deliver::Nobody));
        assert_eq!(world.place("b"), ((33, 32), None));
        // 画面の外の限りは、同じ。
        world.motion(100.0, -500.0);
        assert_eq!(world.place("b").0, (32, 0));
        // 離すと終わる。離した知らせは、クライアントには渡さない。
        assert_eq!(
            world.release(100.0, 50.0, LEFT_BUTTON).deliver,
            Deliver::Nobody
        );
        assert!(!world.interaction.is_moving());

        // もう無いウィンドウの頼みは、受けない。
        world.press(100.0, 100.0, LEFT_BUTTON);
        world.stack.remove(|name| *name == "b");
        assert!(!world.begin_client_move(b, 100.0, 100.0));
    }

    #[test]
    fn a_window_that_leaves_while_being_dragged_is_forgotten() {
        let mut world = World::two();
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(150.0, 80.0);
        world.stack.remove(|name| *name == "b");
        // 残ったaは、動かない。
        let moved = world.motion(200.0, 200.0);
        assert_eq!((moved.redraw, moved.deliver), (false, Deliver::Nobody));
        assert_eq!(world.place("a"), ((0, 0), Some(0)));
        world.release(200.0, 200.0, LEFT_BUTTON);
        // その後は、いつもどおり。
        assert_eq!(
            world.press(100.0, 10.0, LEFT_BUTTON).deliver,
            Deliver::Nobody
        );

        // 閉じるボタンを押している間に去ったときも、同じ。
        let mut world = World::two();
        world.press(340.0, 40.0, LEFT_BUTTON);
        world.stack.remove(|name| *name == "b");
        assert_eq!(world.release(340.0, 40.0, LEFT_BUTTON).close, None);
    }
}
