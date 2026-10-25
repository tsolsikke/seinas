//! ポインターでのウィンドウの操作(動かす、閉じる、最大化、最小化、サイズ変更)の決まり。
//!
//! ボタンを押した所が、ウィンドウのどの部分か(コンテンツ、タイトルバー、タイトルバーのボタン、外枠の外の
//! サイズ変更の境界)で、することを決める。
//! Waylandのオブジェクトに触れずに確かめられるよう、並べるものの型 `T` を決めずに書いてある。
//! 決まりの説明は `docs/window-placement.md` にある。

use crate::geometry::{Point, PointF, Rect, Size};

use crate::{
    decoration::{
        clamp_outer_location, frame_contains, TitleButton, TitleButtons, TITLE_BAR_HEIGHT,
    },
    resize::{edges_at, resized_content, Edges, ResizeStart, SizeLimits},
    stack::{Stack, WindowId},
};

/// 左のボタンの番号(Linuxの入力の決まり)。動かす・閉じる操作は、このボタンだけで行う。
pub const LEFT_BUTTON: u32 = 0x110;
/// タイトルバーを押してから、これだけ(ピクセル)動くまでは、ウィンドウを動かさない。raise するだけのつもりで
/// 押したときに、ずれないようにする。
pub const DRAG_SLACK: f64 = 4.0;
/// 最大化しているウィンドウのタイトルバーを押してから、これだけ(ピクセル)動いたら、元の大きさに戻して動かし
/// 始める。[`DRAG_SLACK`] の 2 倍にしてある。
///
/// ダブルクリックの 2 回目は、1 回目の押下から [`DRAG_SLACK`] の中で押す決まりなので、ダブルクリックの
/// つもりの手の揺れ(1 回目の押下から離すまでの、4 ピクセル程度のずれ)では、ウィンドウが元の大きさに戻らない
/// ようにする。元に戻すと、ウィンドウの大きさと位置が大きく変わるので、ふつうの移動より、はっきり動かした
/// ときだけにする。
pub const RESTORE_DRAG_SLACK: f64 = 2.0 * DRAG_SLACK;
/// ダブルクリックとみなす、2 回の押下の間の時間(ミリ秒)。2 回目は、1 回目から [`DRAG_SLACK`] の中で押すこと。
pub const DOUBLE_CLICK_MS: u32 = 500;

/// ウィンドウの中の部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// クライアントが描くコンテンツ。
    Content,
    /// タイトルバー(ボタンを除く)。
    Bar,
    /// タイトルバーのボタン。
    Button(TitleButton),
}

/// 外枠の左上が `location`、外枠の大きさが `outer`、タイトルバーのボタンが `buttons` のウィンドウの中で、
/// 点 `point` がどの部分か。ウィンドウの外ならNone。`rounded`(最大化していない)なら、角を欠いた所もウィンドウの外。
pub fn part_at(
    location: Point,
    outer: Size,
    buttons: TitleButtons,
    rounded: bool,
    point: PointF,
) -> Option<Part> {
    if !frame_contains(Rect::new(location, outer), rounded, point) {
        return None;
    }
    if point.y >= (location.y + TITLE_BAR_HEIGHT) as f64 {
        return Some(Part::Content);
    }
    Some(match buttons.at(outer.w, point.x - location.x as f64) {
        Some(button) => Part::Button(button),
        None => Part::Bar,
    })
}

/// ウィンドウの測り方。操作の決まりが、ウィンドウ(`T`)について知りたいこと。
pub trait Measure<T> {
    /// 外枠の大きさ。まだ絵を出していなければNone(ポインターが当たらない)。
    fn outer_size(&self, item: &T) -> Option<Size>;
    /// クライアントが決めた、最小と最大の大きさ。サイズ変更できないウィンドウはNone。
    fn limits(&self, item: &T) -> Option<SizeLimits>;
    /// タイトルバーに出すボタン。
    fn buttons(&self, item: &T) -> TitleButtons;
}

/// 点の下にあるもの。いちばん手前のウィンドウから順に、外枠の中か、外枠の外のサイズ変更の境界かを見る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Part(Part),
    Edges(Edges),
}

/// 点 `point` の下にある、いちばん手前のウィンドウ(手前からの順、番号、外枠の左上)と、その中のもの。
///
/// 最小化しているウィンドウには当たらない。最大化しているウィンドウと、`limits` がNoneのウィンドウには、
/// サイズ変更の境界が無い。
fn target_at<T>(
    stack: &Stack<T>,
    point: PointF,
    measure: &impl Measure<T>,
) -> Option<(usize, WindowId, Point, Target)> {
    stack.iter().enumerate().find_map(|(index, entry)| {
        if entry.minimized {
            return None;
        }
        let outer = measure.outer_size(&entry.item)?;
        let buttons = measure.buttons(&entry.item);
        let target = match part_at(entry.location, outer, buttons, !entry.is_maximized(), point) {
            Some(part) => Target::Part(part),
            None => {
                if entry.is_maximized() {
                    return None;
                }
                measure.limits(&entry.item)?;
                Target::Edges(edges_at(Rect::new(entry.location, outer), point)?)
            }
        };
        Some((index, entry.id, entry.location, target))
    })
}

/// ポインターの形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// ふつうの矢印。
    Default,
    /// サイズ変更(動かす辺の向き)。
    Resize(Edges),
}

/// クライアントへ送る configure(サイズ変更の大きさ)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configure {
    pub window: WindowId,
    /// コンテンツの大きさ。
    pub size: Size,
    /// サイズ変更の最中か(xdg_toplevel の resizing の状態)。
    pub resizing: bool,
}

/// ウィンドウに対してすること(タイトルバーのボタンと、ダブルクリック)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// クライアントに、閉じるように頼む。
    Close,
    Minimize,
    /// 最大化か、最大化していれば元に戻す。
    ToggleMaximize,
    /// 最大化する(移動の最中に、使える範囲の上端で離した)。
    Maximize,
    /// 最大化を戻し、外枠の左上を `location` に置く(最大化しているウィンドウのタイトルバーをドラッグした)。
    Restore {
        location: Point,
    },
}

/// ボタンを押している間の状態。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// 何も grab していない。
    None,
    /// タイトルバーを grab している。
    Bar {
        window: WindowId,
        /// 押した点と、そのときのウィンドウの位置。
        press: PointF,
        origin: Point,
        /// もう動かし始めたか([`DRAG_SLACK`] を超えたか)。
        moving: bool,
        /// 動かせるか。ダブルクリックの 2 回目では、動かさない。
        movable: bool,
        /// 最大化の予告を出しているか(動かしている最中に、ポインターが使える範囲の上端にある)。
        snap: bool,
    },
    /// タイトルバーのボタンを押している。
    Button {
        window: WindowId,
        button: TitleButton,
        /// いま、ポインターがそのボタンの上にあるか。上で離したときにだけ、働く。
        inside: bool,
    },
    /// サイズ変更の境界を押している。
    Resize {
        window: WindowId,
        start: ResizeStart,
        /// 最後に configure で伝えた、コンテンツの大きさ。
        size: Size,
    },
    /// コンテンツ(か、何も無い所)を押している。ポインターのイベントは、クライアントへ渡す。
    Client {
        /// 押されているボタンの数。
        buttons: u32,
    },
}

/// ポインターのイベントを、だれに渡すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deliver {
    /// ポインターの下にあるウィンドウのコンテンツへ、いつもどおり渡す。
    ToClient,
    /// だれにも渡さない(Seinasが自分で使った)。
    Nobody,
}

/// 1つのイベントを処理した結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// 画面を描き直す必要があるか。
    pub redraw: bool,
    /// 重なる順が変わったか(activated なウィンドウが変わったかもしれない)。
    pub raised: bool,
    /// ウィンドウに対してすること(閉じるように頼む、最小化、最大化と元に戻す)。
    pub command: Option<(WindowId, Command)>,
    /// クライアントへ送る configure(サイズ変更)。
    pub configure: Option<Configure>,
    pub deliver: Deliver,
}

impl Outcome {
    /// 何も変えず、クライアントへ渡すだけの結果。
    pub fn to_client() -> Self {
        Outcome::new(Deliver::ToClient)
    }

    fn new(deliver: Deliver) -> Self {
        Outcome {
            redraw: false,
            raised: false,
            command: None,
            configure: None,
            deliver,
        }
    }

    /// 閉じるように頼むウィンドウ。
    pub fn close(&self) -> Option<WindowId> {
        match self.command {
            Some((window, Command::Close)) => Some(window),
            _ => None,
        }
    }
}

/// ポインターでの操作の状態。
#[derive(Debug)]
pub struct Interaction {
    grab: Grab,
    /// ボタンを押していないときに、ポインターが乗っているタイトルバーのボタン。
    hover: Option<(WindowId, TitleButton)>,
    /// ボタンを押していないときに、ポインターがサイズ変更の境界の上にあれば、その辺。
    hover_edges: Option<Edges>,
    /// 最後に見たポインターの位置。
    last: PointF,
    /// 最後にタイトルバーを左のボタンで押したウィンドウ、点、時刻(ダブルクリックを見分ける)。
    last_bar_press: Option<(WindowId, PointF, u32)>,
}

impl Default for Interaction {
    fn default() -> Self {
        Interaction {
            grab: Grab::None,
            hover: None,
            hover_edges: None,
            last: (0.0, 0.0).into(),
            last_bar_press: None,
        }
    }
}

impl Interaction {
    /// ウィンドウ `window` のタイトルバーで、目立たせて描くボタン。
    ///
    /// ボタンを押していないときは、ポインターが乗っているボタン。ボタンを押している間は、ポインターが
    /// そのボタンの上にあるとき(外へ出すと、取り消しになることが分かる)。
    pub fn hot_button(&self, window: WindowId) -> Option<TitleButton> {
        match self.grab {
            Grab::None => self
                .hover
                .filter(|(hovered, _)| *hovered == window)
                .map(|(_, button)| button),
            Grab::Button {
                window: pressed,
                button,
                inside,
            } => (pressed == window && inside).then_some(button),
            _ => None,
        }
    }

    /// ウィンドウ `window` の閉じるボタンを、目立たせて描くか。
    pub fn close_button_is_hot(&self, window: WindowId) -> bool {
        self.hot_button(window) == Some(TitleButton::Close)
    }

    /// ボタンを押していない(何も grab していない)か。
    pub fn is_idle(&self) -> bool {
        self.grab == Grab::None
    }

    /// ウィンドウを動かしている最中か。
    pub fn is_moving(&self) -> bool {
        matches!(self.grab, Grab::Bar { moving: true, .. })
    }

    /// 最大化の予告を出しているウィンドウ(動かしている最中に、ポインターが使える範囲の上端にあり、ここで
    /// 離すと最大化する)。
    pub fn maximize_preview(&self) -> Option<WindowId> {
        match self.grab {
            Grab::Bar {
                window, snap: true, ..
            } => Some(window),
            _ => None,
        }
    }

    /// サイズ変更の最中なら、そのウィンドウと、始めたときの様子。
    pub fn resizing(&self) -> Option<(WindowId, ResizeStart)> {
        match self.grab {
            Grab::Resize { window, start, .. } => Some((window, start)),
            _ => None,
        }
    }

    /// いまのポインターの形。サイズ変更の境界の上か、サイズ変更の最中は、その辺の向き。
    pub fn cursor(&self) -> Cursor {
        match self.grab {
            Grab::Resize { start, .. } => Cursor::Resize(start.edges),
            Grab::None => self.hover_edges.map_or(Cursor::Default, Cursor::Resize),
            _ => Cursor::Default,
        }
    }

    /// ボタンを押していないときの、ポインターが乗っているものを忘れる(ウィンドウを最小化したときなど)。
    /// 目立たせていたボタンがあればtrue。
    pub fn forget_hover(&mut self) -> bool {
        self.hover_edges = None;
        self.hover.take().is_some()
    }

    /// ポインターが `point` へ動いた。
    pub fn motion<T>(
        &mut self,
        stack: &mut Stack<T>,
        area: Rect,
        point: PointF,
        measure: &impl Measure<T>,
    ) -> Outcome {
        self.last = point;
        match self.grab {
            Grab::None => {
                let mut outcome = Outcome::new(Deliver::ToClient);
                outcome.redraw = self.update_hover(stack, point, measure);
                // サイズ変更の境界の上では、その下のウィンドウには渡さない。
                if self.hover_edges.is_some() {
                    outcome.deliver = Deliver::Nobody;
                }
                outcome
            }
            Grab::Resize {
                window,
                start,
                size,
            } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                let Some(limits) = stack
                    .get(window)
                    .filter(|entry| !entry.is_maximized() && !entry.minimized)
                    .and_then(|entry| measure.limits(&entry.item))
                else {
                    // grab していたウィンドウが去ったか、サイズ変更できなくなった。離すまで、何もしない。
                    return outcome;
                };
                let wanted = resized_content(start, point, limits, area);
                if wanted != size {
                    self.grab = Grab::Resize {
                        window,
                        start,
                        size: wanted,
                    };
                    outcome.configure = Some(Configure {
                        window,
                        size: wanted,
                        resizing: true,
                    });
                }
                outcome
            }
            Grab::Client { .. } => Outcome::new(Deliver::ToClient),
            Grab::Bar {
                window,
                press,
                origin,
                moving,
                movable,
                snap,
            } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                let Some((outer, maximized, restore, buttons)) =
                    stack.get(window).and_then(|entry| {
                        Some((
                            measure.outer_size(&entry.item)?,
                            entry.is_maximized(),
                            entry.restore,
                            measure.buttons(&entry.item),
                        ))
                    })
                else {
                    // grab していたウィンドウが去った。ボタンを離すまで、何もしない。
                    return outcome;
                };
                let (dx, dy) = (point.x - press.x, point.y - press.y);
                let slack = if maximized {
                    RESTORE_DRAG_SLACK
                } else {
                    DRAG_SLACK
                };
                let starts = movable && !moving && dx.hypot(dy) >= slack;
                if starts {
                    // 動かし始めたら、ダブルクリックにはしない。
                    self.last_bar_press = None;
                }
                if starts && maximized {
                    // 元の大きさに戻し、ポインターが、元の大きさのタイトルバーの、横の同じ割合の所に来るように置く。
                    let width = restore
                        .and_then(|restore| restore.content)
                        .map_or(outer.w, |size| size.w);
                    let ratio = ((press.x - origin.x as f64) / outer.w as f64).clamp(0.0, 1.0);
                    // ボタンの上には来ないようにする。
                    let last = (width - buttons.width() - 1).max(0) as f64;
                    let offset = (ratio * width as f64).clamp(0.0, last);
                    // 縦は、押した点の、外枠の上からの高さのまま(クライアントから頼まれた移動では、コンテンツの中を
                    // 押していることがあるので、元の大きさの外枠に入るようにする)。
                    let height = restore
                        .and_then(|restore| restore.content)
                        .map_or(outer.h, |size| size.h + TITLE_BAR_HEIGHT);
                    let offset_y =
                        (press.y - origin.y as f64).clamp(0.0, (height - 1).max(0) as f64);
                    let wanted = (
                        (point.x - offset).round() as i32,
                        (point.y - offset_y).round() as i32,
                    );
                    let location = clamp_outer_location(wanted.into(), width, area);
                    self.grab = Grab::Bar {
                        window,
                        press: point,
                        origin: location,
                        moving: true,
                        movable,
                        snap: false,
                    };
                    outcome.command = Some((window, Command::Restore { location }));
                    outcome.redraw = true;
                    return outcome;
                }
                let moving = moving || starts;
                // 動かしている最中に、ポインターが使える範囲の上端に触れたら、最大化の予告を出す。大きさの
                // 決まったウィンドウ(最大化のボタンが無いもの)では、出さない。
                let now_snap =
                    moving && !maximized && buttons.maximize && point.y <= area.loc.y as f64;
                self.grab = Grab::Bar {
                    window,
                    press,
                    origin,
                    moving,
                    movable,
                    snap: now_snap,
                };
                outcome.redraw = now_snap != snap;
                if moving && !maximized {
                    let wanted = (origin.x + dx.round() as i32, origin.y + dy.round() as i32);
                    let location = clamp_outer_location(wanted.into(), outer.w, area);
                    outcome.redraw |= stack.move_to(window, location);
                }
                outcome
            }
            Grab::Button {
                window,
                button,
                inside,
            } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                let now = button_under(stack, point, measure) == Some((window, button));
                self.grab = Grab::Button {
                    window,
                    button,
                    inside: now,
                };
                outcome.redraw = now != inside;
                outcome
            }
        }
    }

    /// ポインターが画面の外へ出た。押しているボタンは、離すまで grab したままにする。
    pub fn leave(&mut self) -> Outcome {
        let mut outcome = Outcome::new(Deliver::Nobody);
        self.hover_edges = None;
        outcome.redraw = self.hover.take().is_some() && self.grab == Grab::None;
        outcome
    }

    /// 点 `point` で、ボタン `button` が押された。`time` は、押した時刻(ミリ秒)。
    pub fn press<T>(
        &mut self,
        stack: &mut Stack<T>,
        point: PointF,
        button: u32,
        time: u32,
        measure: &impl Measure<T>,
    ) -> Outcome {
        self.last = point;
        match &mut self.grab {
            // タイトルバーやボタン、サイズ変更の境界を grab している間の、ほかのボタンは、使わない。
            Grab::Bar { .. } | Grab::Button { .. } | Grab::Resize { .. } => {
                return Outcome::new(Deliver::Nobody)
            }
            Grab::Client { buttons } => {
                *buttons += 1;
                return Outcome::new(Deliver::ToClient);
            }
            Grab::None => {}
        }
        let hit = target_at(stack, point, measure);
        let Some((index, window, origin, target)) = hit else {
            // どのウィンドウの上でもない。
            self.grab = Grab::Client { buttons: 1 };
            return Outcome::new(Deliver::ToClient);
        };
        // どのボタンでも、どの部分でも、押したウィンドウを raise する。
        let raised = stack.raise(index);
        let was_hot = self.hot_button(window);
        let mut command = None;
        let deliver = match target {
            Target::Edges(edges) if button == LEFT_BUTTON => {
                let content = measure
                    .outer_size(&stack.get(window).expect("the window").item)
                    .map(|outer| Size::new(outer.w, outer.h - TITLE_BAR_HEIGHT))
                    .expect("a window that was hit has a size");
                let start = ResizeStart {
                    edges,
                    press: point,
                    location: origin,
                    content,
                };
                self.grab = Grab::Resize {
                    window,
                    start,
                    size: content,
                };
                Deliver::Nobody
            }
            // サイズ変更の境界の上の、左でないボタンは、raise するだけ。
            Target::Edges(_) => Deliver::Nobody,
            Target::Part(Part::Content) => {
                self.grab = Grab::Client { buttons: 1 };
                Deliver::ToClient
            }
            Target::Part(Part::Bar) if button == LEFT_BUTTON => {
                // 同じウィンドウのタイトルバーを、短い時間の中で、ほぼ同じ所でもう一度押したら、ダブルクリック。
                let double = self.last_bar_press.is_some_and(|(last, at, when)| {
                    last == window
                        && time.wrapping_sub(when) <= DOUBLE_CLICK_MS
                        && (point.x - at.x).hypot(point.y - at.y) < DRAG_SLACK
                });
                if double {
                    self.last_bar_press = None;
                    command = Some((window, Command::ToggleMaximize));
                } else {
                    self.last_bar_press = Some((window, point, time));
                }
                // 最大化しているウィンドウも、grab できる(動かすと、元の大きさに戻る)。
                self.grab = Grab::Bar {
                    window,
                    press: point,
                    origin,
                    moving: false,
                    movable: !double,
                    snap: false,
                };
                Deliver::Nobody
            }
            Target::Part(Part::Button(pressed)) if button == LEFT_BUTTON => {
                self.grab = Grab::Button {
                    window,
                    button: pressed,
                    inside: true,
                };
                Deliver::Nobody
            }
            // タイトルバーの上の、左でないボタンは、raise するだけ。
            Target::Part(Part::Bar | Part::Button(_)) => Deliver::Nobody,
        };
        self.hover = None;
        self.hover_edges = None;
        Outcome {
            redraw: raised || was_hot != self.hot_button(window) || command.is_some(),
            raised,
            command,
            configure: None,
            deliver,
        }
    }

    /// 点 `point` で、ボタン `button` が離された。
    pub fn release<T>(
        &mut self,
        stack: &mut Stack<T>,
        point: PointF,
        button: u32,
        measure: &impl Measure<T>,
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
            // タイトルバーやボタン、サイズ変更の境界を grab している間の、ほかのボタンは、使わない。
            Grab::Bar { .. } | Grab::Button { .. } | Grab::Resize { .. }
                if button != LEFT_BUTTON =>
            {
                return Outcome::new(Deliver::Nobody)
            }
            Grab::Bar { window, snap, .. } => {
                let mut outcome = Outcome::new(Deliver::Nobody);
                // 最大化の予告を出している所で離したら、最大化する。
                if *snap {
                    outcome.command = Some((*window, Command::Maximize));
                    outcome.redraw = true;
                }
                outcome
            }
            Grab::Resize { window, size, .. } => {
                // 最後の大きさを、resizing の状態を外して伝える。
                let mut outcome = Outcome::new(Deliver::Nobody);
                outcome.configure = Some(Configure {
                    window: *window,
                    size: *size,
                    resizing: false,
                });
                outcome
            }
            Grab::Button {
                window,
                button: pressed,
                ..
            } => {
                let (window, pressed) = (*window, *pressed);
                let mut outcome = Outcome::new(Deliver::Nobody);
                // ボタンの上で離したときにだけ、働く。外へ出して離すと、取り消し。
                if button_under(stack, point, measure) == Some((window, pressed)) {
                    outcome.command = Some((
                        window,
                        match pressed {
                            TitleButton::Close => Command::Close,
                            TitleButton::Minimize => Command::Minimize,
                            TitleButton::Maximize => Command::ToggleMaximize,
                        },
                    ));
                }
                outcome.redraw = true;
                outcome
            }
        };
        self.grab = Grab::None;
        outcome.redraw |= self.update_hover(stack, point, measure);
        outcome
    }

    /// クライアントが、自分のウィンドウ `window` を動かしてほしいと頼んできた(xdg_toplevel.move。自分でタイトル
    /// バーを描くアプリなど)。受けたら、頼みが届くまでの間にポインターが動いたぶんの結果を返す。受けなければNone。
    ///
    /// 受けるのは、コンテンツの上でボタンが押されている間だけ。受けたら、そのボタンを離すまで、タイトルバーを
    /// grab したときと同じに動かす(ドラッグのしきい値は置かず、すぐ動かす)。`press` は、そのボタンを押した点。
    /// 呼ぶ側は、頼んできたのが、押されているクライアントであることを確かめておくこと。
    ///
    /// 最大化しているウィンドウは、タイトルバーを grab したときと同じく、[`RESTORE_DRAG_SLACK`] 動いた所で、元の
    /// 大きさに戻して([`Command::Restore`])動かし続ける。上端での最大化の予告も、同じく出す。
    pub fn begin_client_move<T>(
        &mut self,
        stack: &mut Stack<T>,
        area: Rect,
        window: WindowId,
        press: PointF,
        measure: &impl Measure<T>,
    ) -> Option<Outcome> {
        let (Grab::Client { .. }, Some(entry)) = (self.grab, stack.get(window)) else {
            return None;
        };
        self.grab = Grab::Bar {
            window,
            press,
            origin: entry.location,
            // 最大化していれば、しきい値を越えるまでは動かさない(越えたら元の大きさに戻す)。
            moving: !entry.is_maximized(),
            movable: true,
            snap: false,
        };
        Some(self.motion(stack, area, self.last, measure))
    }

    /// クライアントが、自分のウィンドウ `window` の辺 `edges` でサイズ変更してほしいと頼んできた
    /// (xdg_toplevel.resize)。受けたら、そのボタンを離すまで、サイズ変更の境界を grab したときと同じに扱い、
    /// 頼みが届くまでの間にポインターが動いたぶんの configure を返す。受けなければNone。
    ///
    /// 受ける条件は [`Interaction::begin_client_move`] と同じで、加えて、サイズ変更できるウィンドウである
    /// こと(最大化していない、`limits` がNoneでない)。`press` は、そのボタンを押した点。
    pub fn begin_client_resize<T>(
        &mut self,
        stack: &mut Stack<T>,
        area: Rect,
        window: WindowId,
        edges: Edges,
        press: PointF,
        measure: &impl Measure<T>,
    ) -> Option<Outcome> {
        let (Grab::Client { .. }, Some(entry)) = (self.grab, stack.get(window)) else {
            return None;
        };
        if edges.is_empty() || entry.is_maximized() {
            return None;
        }
        measure.limits(&entry.item)?;
        let outer = measure.outer_size(&entry.item)?;
        let content = Size::new(outer.w, outer.h - TITLE_BAR_HEIGHT);
        self.grab = Grab::Resize {
            window,
            start: ResizeStart {
                edges,
                press,
                location: entry.location,
                content,
            },
            size: content,
        };
        Some(self.motion(stack, area, self.last, measure))
    }

    /// ボタンを押していないときの、タイトルバーのボタンとサイズ変更の境界の上にあるかどうかを更新する。
    /// 目立たせるボタンが変わったらtrue(再描画が要る)。
    fn update_hover<T>(
        &mut self,
        stack: &Stack<T>,
        point: PointF,
        measure: &impl Measure<T>,
    ) -> bool {
        let target = target_at(stack, point, measure);
        let hover = match target {
            Some((_, window, _, Target::Part(Part::Button(button)))) => Some((window, button)),
            _ => None,
        };
        self.hover_edges = match target {
            Some((_, _, _, Target::Edges(edges))) => Some(edges),
            _ => None,
        };
        std::mem::replace(&mut self.hover, hover) != hover
    }
}

/// 点 `point` が、見えているタイトルバーのボタンの上にあれば、そのウィンドウとボタン。手前のウィンドウの
/// サイズ変更の境界に隠れている所は、見えていないものとして扱う。
fn button_under<T>(
    stack: &Stack<T>,
    point: PointF,
    measure: &impl Measure<T>,
) -> Option<(WindowId, TitleButton)> {
    match target_at(stack, point, measure)? {
        (_, window, _, Target::Part(Part::Button(button))) => Some((window, button)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i32, i32) = (640, 480);
    /// テストのウィンドウの外枠の大きさ(どれも同じ)。タイトルバーが36、コンテンツが320x240。
    const OUTER: (i32, i32) = (320, 240 + TITLE_BAR_HEIGHT);
    const RIGHT_BUTTON: u32 = 0x111;

    /// テストのウィンドウの測り方。外枠はどれも同じ大きさ。
    struct Windows;

    impl Measure<&'static str> for Windows {
        fn outer_size(&self, _: &&'static str) -> Option<Size> {
            Some(OUTER.into())
        }
        /// 「fixed」という名前のウィンドウだけは、サイズ変更できない。
        fn limits(&self, name: &&'static str) -> Option<SizeLimits> {
            (*name != "fixed").then(SizeLimits::default)
        }
        /// 名前が「full」で始まるウィンドウには、3 つのボタンがある。ほかは、閉じるボタンだけ。
        fn buttons(&self, name: &&'static str) -> TitleButtons {
            if name.starts_with("full") {
                TitleButtons::ALL
            } else {
                TitleButtons::CLOSE_ONLY
            }
        }
    }

    fn at(x: f64, y: f64) -> PointF {
        (x, y).into()
    }

    /// スタックと、操作の状態。イベントを1つずつ送って、結果を見る。
    struct World {
        stack: Stack<&'static str>,
        interaction: Interaction,
        /// 次に押す時刻(ミリ秒)。押すたびに 1 秒進める(ダブルクリックにならないように)。
        time: u32,
    }

    impl World {
        /// a(番号0、左上)、b(番号1、(32, 32))の順につないだスタック。手前はb。
        fn two() -> Self {
            let mut stack = Stack::default();
            stack.add("a", Rect::from_size(SCREEN.into()));
            stack.add("b", Rect::from_size(SCREEN.into()));
            World {
                stack,
                interaction: Interaction::default(),
                time: 0,
            }
        }

        fn motion(&mut self, x: f64, y: f64) -> Outcome {
            self.interaction.motion(
                &mut self.stack,
                Rect::from_size(SCREEN.into()),
                at(x, y),
                &Windows,
            )
        }

        fn press(&mut self, x: f64, y: f64, button: u32) -> Outcome {
            self.time += 1000;
            self.press_at(x, y, button, self.time)
        }

        /// 時刻 `time` に押す。
        fn press_at(&mut self, x: f64, y: f64, button: u32, time: u32) -> Outcome {
            self.interaction
                .press(&mut self.stack, at(x, y), button, time, &Windows)
        }

        fn release(&mut self, x: f64, y: f64, button: u32) -> Outcome {
            self.interaction
                .release(&mut self.stack, at(x, y), button, &Windows)
        }

        fn begin_client_move(&mut self, window: WindowId, x: f64, y: f64) -> bool {
            self.client_move(window, x, y).is_some()
        }

        fn client_move(&mut self, window: WindowId, x: f64, y: f64) -> Option<Outcome> {
            self.interaction.begin_client_move(
                &mut self.stack,
                Rect::from_size(SCREEN.into()),
                window,
                at(x, y),
                &Windows,
            )
        }

        fn id(&self, name: &str) -> WindowId {
            self.stack
                .iter()
                .find(|entry| entry.item == name)
                .expect("the window")
                .id
        }

        /// 名前が `name` のウィンドウの位置と、スロット番号。
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
        let part =
            |x: f64, y: f64| part_at(location, outer, TitleButtons::CLOSE_ONLY, false, at(x, y));
        // タイトルバーは、上の36ピクセル。その右の端の44ピクセルが、閉じるボタン。
        assert_eq!(part(32.0, 32.0), Some(Part::Bar));
        assert_eq!(part(307.9, 67.9), Some(Part::Bar));
        assert_eq!(part(308.0, 32.0), Some(Part::Button(TitleButton::Close)));
        assert_eq!(part(351.9, 67.9), Some(Part::Button(TitleButton::Close)));
        // その下がコンテンツ。
        assert_eq!(part(32.0, 68.0), Some(Part::Content));
        assert_eq!(part(351.0, 68.0), Some(Part::Content));
        assert_eq!(part(351.9, 307.9), Some(Part::Content));
        // 外。
        assert_eq!(part(31.9, 40.0), None);
        assert_eq!(part(352.0, 40.0), None);
        assert_eq!(part(100.0, 308.0), None);
        // 角を丸めたウィンドウ(最大化していない)では、角を欠いた所(丸み 7 の外)も外。
        let rounded =
            |x: f64, y: f64| part_at(location, outer, TitleButtons::CLOSE_ONLY, true, at(x, y));
        assert_eq!(rounded(32.5, 32.5), None);
        assert_eq!(rounded(351.5, 32.5), None);
        assert_eq!(rounded(32.5, 307.5), None);
        assert_eq!(rounded(351.5, 307.5), None);
        assert_eq!(rounded(36.0, 36.0), Some(Part::Bar));
        assert_eq!(rounded(39.0, 32.5), Some(Part::Bar));
        assert_eq!(rounded(347.0, 36.0), Some(Part::Button(TitleButton::Close)));
        assert_eq!(rounded(36.0, 304.0), Some(Part::Content));
    }

    #[test]
    fn dragging_the_bar_moves_the_window_after_a_slack_of_four_pixels() {
        let mut world = World::two();
        // bのタイトルバー(閉じるボタンでない所)を、左のボタンで押す。クライアントには渡さない。
        let pressed = world.press(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(pressed.deliver, Deliver::Nobody);
        assert!(!pressed.redraw, "b is already in front");

        // 4ピクセルに届かない間は、動かない。番号も持ったまま。
        for (x, y) in [(103.0, 40.0), (100.0, 43.9), (102.0, 42.0)] {
            let moved = world.motion(x, y);
            assert_eq!((moved.redraw, moved.deliver), (false, Deliver::Nobody));
            assert_eq!(world.place("b"), ((32, 32), Some(1)));
        }
        assert!(!world.interaction.is_moving());

        // 4ピクセル動くと、動き始める。ウィンドウは、押した点からのずれのぶんだけ動く(しきい値のぶんも含む)。
        assert!(world.motion(104.0, 40.0).redraw);
        assert!(world.interaction.is_moving());
        assert_eq!(world.place("b"), ((36, 32), None));
        assert!(world.motion(150.0, 100.0).redraw);
        assert_eq!(world.place("b"), ((82, 92), None));
        // 動き始めた後は、押した点の近くへ戻っても、付いてくる。
        assert!(world.motion(101.0, 41.0).redraw);
        assert_eq!(world.place("b"), ((33, 33), None));
        // 同じ点へのイベントでは、描き直さない。
        assert!(!world.motion(101.0, 41.0).redraw);

        // 離すと終わる。その後の動きでは、動かない。
        assert_eq!(
            world.release(101.0, 41.0, LEFT_BUTTON).deliver,
            Deliver::Nobody
        );
        assert!(!world.interaction.is_moving());
        assert_eq!(world.motion(300.0, 200.0).deliver, Deliver::ToClient);
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
        // aのタイトルバーは、bの上にのぞいている。
        let pressed = world.press(100.0, 10.0, LEFT_BUTTON);
        assert!(pressed.raised && pressed.redraw);
        assert_eq!(world.order(), ["a", "b"]);
        world.motion(100.0, 60.0);
        assert_eq!(world.place("a"), ((0, 50), None));
        world.release(100.0, 60.0, LEFT_BUTTON);
        // 次の新しいウィンドウは、aが手放した番号0に入る。
        world.stack.add("c", Rect::from_size(SCREEN.into()));
        assert_eq!(world.place("c"), ((0, 0), Some(0)));
        assert_eq!(world.place("a"), ((0, 50), None));
    }

    #[test]
    fn a_dragged_window_stays_reachable() {
        let mut world = World::two();
        world.press(100.0, 40.0, LEFT_BUTTON);
        // 上へ: タイトルバーの上の端は、画面の上の端で止まる。
        world.motion(100.0, -200.0);
        assert_eq!(world.place("b").0, (32, 0));
        // 下へ: タイトルバーの全体が残る。
        world.motion(100.0, 2000.0);
        assert_eq!(world.place("b").0, (32, 444));
        // 左へ: タイトルバーの右の 180 ピクセル(48 と、ボタン 3 つ分の 132)が残る。右へ: 左の 48 ピクセルが残る。
        world.motion(-2000.0, 100.0);
        assert_eq!(world.place("b").0, (-140, 92));
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
        assert_eq!(world.release(310.0, 10.0, RIGHT_BUTTON).close(), None);
    }

    #[test]
    fn the_close_button_works_when_released_on_it() {
        let mut world = World::two();
        let b = world.id("b");
        // bの閉じるボタン(タイトルバーの右の端)。
        let pressed = world.press(340.0, 40.0, LEFT_BUTTON);
        assert_eq!((pressed.deliver, pressed.close()), (Deliver::Nobody, None));
        assert!(world.interaction.close_button_is_hot(b));
        // ボタンの中で動かしても、ウィンドウは動かない。
        let moved = world.motion(345.0, 50.0);
        assert_eq!((moved.redraw, moved.deliver), (false, Deliver::Nobody));
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
        // 上で離すと、閉じるように頼む。
        let released = world.release(345.0, 50.0, LEFT_BUTTON);
        assert_eq!(released.close(), Some(b));
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
        assert_eq!(world.release(200.0, 100.0, LEFT_BUTTON).close(), None);

        // 別のウィンドウの閉じるボタンの上で離しても、閉じない。
        world.press(340.0, 40.0, LEFT_BUTTON);
        world.motion(310.0, 10.0);
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close(), None);
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
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close(), Some(a));

        // 隠れている閉じるボタンには、当たらない。手前のウィンドウの部分として扱う。
        let mut world = World::two();
        world.stack.move_to(world.id("b"), (40, 0).into());
        let pressed = world.press(310.0, 10.0, LEFT_BUTTON);
        assert!(!pressed.raised);
        assert_eq!(world.release(310.0, 10.0, LEFT_BUTTON).close(), None);
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

        // タイトルバーを grab して動かしている間は、どのボタンも目立たせない。
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(310.0, 10.0);
        assert!(!world.interaction.close_button_is_hot(a));
        assert!(!world.interaction.close_button_is_hot(b));
    }

    #[test]
    fn the_content_gets_the_pointer_and_the_bar_does_not() {
        let mut world = World::two();
        // コンテンツの上でのボタンは、クライアントへ渡す。押したまま外へ出ても、離すまでは渡し続ける。
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
        // まだ右を押しているので、タイトルバーの上で左を押しても、grab しない。
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

        // コンテンツを押すと、奥のウィンドウも raise される。
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
        // タイトルバーを grab しているときの頼みも、受けない。
        world.press(100.0, 40.0, LEFT_BUTTON);
        assert!(!world.begin_client_move(b, 100.0, 40.0));
        world.release(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(world.place("b"), ((32, 32), Some(1)));

        // コンテンツを押している間の頼みは、受ける。頼みが届くまでの間に動いたぶんは、受けたときに動く。
        world.press(100.0, 100.0, LEFT_BUTTON);
        world.motion(102.0, 100.0);
        assert_eq!(world.place("b"), ((32, 32), Some(1)));
        assert!(world.begin_client_move(b, 100.0, 100.0));
        assert!(world.interaction.is_moving());
        assert_eq!(world.place("b"), ((34, 32), None));
        // しきい値は無く、1ピクセルでも動く。イベントは、クライアントには渡さない。
        let moved = world.motion(101.0, 100.0);
        assert_eq!((moved.redraw, moved.deliver), (true, Deliver::Nobody));
        assert_eq!(world.place("b"), ((33, 32), None));
        // 画面の外の限りは、同じ。
        world.motion(100.0, -500.0);
        assert_eq!(world.place("b").0, (32, 0));
        // 離すと終わる。離したイベントは、クライアントには渡さない。
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
        assert_eq!(world.release(340.0, 40.0, LEFT_BUTTON).close(), None);
    }

    #[test]
    fn the_band_outside_the_frame_resizes_the_window() {
        let mut world = World::two();
        let (a, b) = (world.id("a"), world.id("b"));
        // bの下の辺の外側(外枠は y 32〜308)。ポインターの形が変わり、だれにも渡さない。
        let hovered = world.motion(200.0, 312.0);
        assert_eq!(hovered.deliver, Deliver::Nobody);
        assert_eq!(world.interaction.cursor(), Cursor::Resize(Edges::BOTTOM));
        // 境界の外へ出ると、元に戻る。
        assert_eq!(world.motion(400.0, 400.0).deliver, Deliver::ToClient);
        assert_eq!(world.interaction.cursor(), Cursor::Default);

        // 奥のaの右の辺の外側を押すと、aが raise され、サイズ変更が始まる。
        let pressed = world.press(324.0, 20.0, LEFT_BUTTON);
        assert!(pressed.raised);
        assert_eq!(pressed.deliver, Deliver::Nobody);
        assert_eq!(pressed.configure, None);
        assert_eq!(world.order(), ["a", "b"]);
        assert_eq!(world.interaction.cursor(), Cursor::Resize(Edges::RIGHT));
        assert_eq!(world.interaction.resizing().map(|(w, _)| w), Some(a));

        // 動かすと、resizing の状態つきで、新しいコンテンツの大きさを伝える。同じ大きさなら、送らない。
        let moved = world.motion(344.0, 20.0);
        assert_eq!(moved.deliver, Deliver::Nobody);
        assert_eq!(
            moved.configure,
            Some(Configure {
                window: a,
                size: Size::new(340, 240),
                resizing: true
            })
        );
        assert_eq!(world.motion(344.0, 25.0).configure, None);
        // 位置は、ここでは変えない(右の辺なので、左上は動かない)。
        assert_eq!(world.place("a"), ((0, 0), Some(0)));

        // 離すと、最後の大きさを resizing の状態を外して伝え、終わる。
        let released = world.release(344.0, 25.0, LEFT_BUTTON);
        assert_eq!(
            released.configure,
            Some(Configure {
                window: a,
                size: Size::new(340, 240),
                resizing: false
            })
        );
        assert_eq!(world.interaction.resizing(), None);

        // 左でないボタンでは、raise するだけ。
        let right = world.press(200.0, 300.0, RIGHT_BUTTON);
        assert!(right.raised);
        assert_eq!(world.order(), ["b", "a"]);
        assert_eq!(world.interaction.resizing(), None);
        world.release(200.0, 300.0, RIGHT_BUTTON);
        assert_eq!(world.id("b"), b);
    }

    #[test]
    fn a_window_that_cannot_be_resized_has_no_band() {
        let mut stack = Stack::default();
        stack.add("fixed", Rect::from_size(SCREEN.into()));
        let mut world = World {
            stack,
            interaction: Interaction::default(),
            time: 0,
        };
        // 外枠(y 0〜264)のすぐ下でも、ポインターの形は変わらず、クライアントへ渡す。
        assert_eq!(world.motion(100.0, 268.0).deliver, Deliver::ToClient);
        assert_eq!(world.interaction.cursor(), Cursor::Default);
        assert_eq!(
            world.press(100.0, 268.0, LEFT_BUTTON).deliver,
            Deliver::ToClient
        );
        assert_eq!(world.interaction.resizing(), None);
    }

    #[test]
    fn a_client_can_ask_to_resize_while_its_button_is_pressed() {
        let mut world = World::two();
        let b = world.id("b");
        let area = Rect::from_size(SCREEN.into());
        // ボタンを押していないときの頼みは、受けない。
        assert!(world
            .interaction
            .begin_client_resize(
                &mut world.stack,
                area,
                b,
                Edges::BOTTOM_RIGHT,
                at(200.0, 200.0),
                &Windows
            )
            .is_none());
        // bのコンテンツを押している間なら、受ける。辺の指定が無いものは、受けない。
        world.press(200.0, 200.0, LEFT_BUTTON);
        assert!(world
            .interaction
            .begin_client_resize(
                &mut world.stack,
                area,
                b,
                Edges::default(),
                at(200.0, 200.0),
                &Windows
            )
            .is_none());
        let begun = world
            .interaction
            .begin_client_resize(
                &mut world.stack,
                area,
                b,
                Edges::BOTTOM_RIGHT,
                at(200.0, 200.0),
                &Windows,
            )
            .expect("the resize begins");
        assert_eq!(begun.configure, None);
        assert_eq!(
            world.interaction.cursor(),
            Cursor::Resize(Edges::BOTTOM_RIGHT)
        );
        assert_eq!(
            world.motion(210.0, 220.0).configure,
            Some(Configure {
                window: b,
                size: Size::new(330, 260),
                resizing: true
            })
        );
        assert_eq!(
            world
                .release(210.0, 220.0, LEFT_BUTTON)
                .configure
                .map(|c| c.resizing),
            Some(false)
        );
    }

    /// ボタンが 3 つある「full-a」(左上)と「full-b」((32, 32))。手前は full-b。
    fn two_with_buttons() -> World {
        let mut stack = Stack::default();
        stack.add("full-a", Rect::from_size(SCREEN.into()));
        stack.add("full-b", Rect::from_size(SCREEN.into()));
        World {
            stack,
            interaction: Interaction::default(),
            time: 0,
        }
    }

    #[test]
    fn the_title_buttons_minimize_and_maximize() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        // full-b の外枠は x 32〜352。閉じるは 308〜352、最大化は 264〜308、最小化は 220〜264。
        world.motion(286.0, 40.0);
        assert_eq!(world.interaction.hot_button(b), Some(TitleButton::Maximize));
        world.press(286.0, 40.0, LEFT_BUTTON);
        assert_eq!(
            world.release(286.0, 40.0, LEFT_BUTTON).command,
            Some((b, Command::ToggleMaximize))
        );
        world.press(242.0, 40.0, LEFT_BUTTON);
        assert_eq!(
            world.release(242.0, 40.0, LEFT_BUTTON).command,
            Some((b, Command::Minimize))
        );
        // 押したまま外へ出して離すと、取り消し。
        world.press(242.0, 40.0, LEFT_BUTTON);
        world.motion(200.0, 40.0);
        assert_eq!(world.interaction.hot_button(b), None);
        assert_eq!(world.release(200.0, 40.0, LEFT_BUTTON).command, None);
        // ボタンが閉じるだけのウィンドウでは、同じ所はタイトルバー。
        let mut plain = World::two();
        plain.press(286.0, 40.0, LEFT_BUTTON);
        assert_eq!(plain.release(286.0, 40.0, LEFT_BUTTON).command, None);
    }

    #[test]
    fn a_double_click_on_the_bar_toggles_maximize() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        let first = world.press_at(100.0, 40.0, LEFT_BUTTON, 1000);
        assert_eq!(first.command, None);
        world.release(100.0, 40.0, LEFT_BUTTON);
        // 500 ミリ秒の中で、4 ピクセルより近い所を押すと、ダブルクリック。
        let second = world.press_at(102.0, 41.0, LEFT_BUTTON, 1400);
        assert_eq!(second.command, Some((b, Command::ToggleMaximize)));
        // 2 回目の押下のまま動かしても、ウィンドウは動かない。
        world.motion(200.0, 200.0);
        assert_eq!(world.place("full-b").0, (32, 32));
        world.release(200.0, 200.0, LEFT_BUTTON);
        // 遅い、遠い、別のウィンドウ、のどれでも、ダブルクリックにはならない。
        world.press_at(100.0, 40.0, LEFT_BUTTON, 3000);
        world.release(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(world.press_at(100.0, 40.0, LEFT_BUTTON, 3501).command, None);
        world.release(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(world.press_at(110.0, 40.0, LEFT_BUTTON, 3600).command, None);
        world.release(110.0, 40.0, LEFT_BUTTON);
        // 3 回目の押下は、新しい 1 回目になる。
        assert_eq!(
            world.press_at(110.0, 40.0, LEFT_BUTTON, 3700).command,
            Some((b, Command::ToggleMaximize))
        );
    }

    #[test]
    fn dragging_the_bar_of_a_maximized_window_restores_it_under_the_pointer() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        let area = Rect::from_size(SCREEN.into());
        // 元の大きさは 240x120(外枠の幅 240。右の端の 132 がボタン)。最大化している間の外枠の幅は 320。
        world.stack.maximize(b, area, Some((240, 120).into()));
        // 横の 1/4 の所を押す。8 ピクセルより手前では、動かない。
        world.press(80.0, 10.0, LEFT_BUTTON);
        let near = world.motion(87.0, 10.0);
        assert_eq!((near.command, near.redraw), (None, false));
        assert!(!world.interaction.is_moving());
        // 8 ピクセル動かすと、元の大きさに戻す。ポインターは、元の大きさのタイトルバーの 1/4(60)の所に来る。
        let restore = world.motion(88.0, 10.0);
        assert_eq!(
            restore.command,
            Some((
                b,
                Command::Restore {
                    location: (28, 0).into()
                }
            ))
        );
        assert!(world.interaction.is_moving());
        // 戻すのは呼ぶ側(フロントエンド)が行う。その後は、ふつうに動く。
        world.stack.unmaximize(b, area);
        world.stack.move_to(b, (28, 0).into());
        world.motion(98.0, 30.0);
        assert_eq!(world.place("full-b"), ((38, 20), None));
        assert_eq!(world.release(98.0, 30.0, LEFT_BUTTON).command, None);

        // ボタンの近く(横の 9/16。ボタンは 188〜320)を押しても、戻したときに、ポインターはボタンの上に来ない
        // (240 - 132 - 1 = 107)。
        world.stack.maximize(b, area, Some((240, 120).into()));
        world.press(180.0, 10.0, LEFT_BUTTON);
        let restore = world.motion(190.0, 10.0);
        assert_eq!(
            restore.command,
            Some((
                b,
                Command::Restore {
                    location: (83, 0).into()
                }
            ))
        );
        world.release(190.0, 10.0, LEFT_BUTTON);
    }

    #[test]
    fn a_client_move_of_a_maximized_window_restores_it_after_the_slack() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        let area = Rect::from_size(SCREEN.into());
        world.stack.maximize(b, area, Some((240, 120).into()));
        // コンテンツの中(自分で描いたタイトルバーなど)を押して、move を頼む。まだ動かない。
        world.press(100.0, 40.0, LEFT_BUTTON);
        let start = world
            .client_move(b, 100.0, 40.0)
            .expect("the move is taken");
        assert_eq!(start.command, None);
        assert!(!world.interaction.is_moving());
        assert_eq!(world.motion(107.0, 40.0).command, None);
        // 8 ピクセル動かすと、元の大きさに戻す。横は押した点の割合(100 / 320 = 5/16)の所、縦は押した高さのまま。
        assert_eq!(
            world.motion(108.0, 40.0).command,
            Some((
                b,
                Command::Restore {
                    location: (33, 0).into()
                }
            ))
        );
        assert!(world.interaction.is_moving());
        world.stack.unmaximize(b, area);
        world.stack.move_to(b, (33, 0).into());
        // そのまま上端へ動かすと、最大化の予告が出て、離すと最大化する。
        world.motion(108.0, 60.0);
        world.motion(108.0, 0.0);
        assert_eq!(world.interaction.maximize_preview(), Some(b));
        assert_eq!(
            world.release(108.0, 0.0, LEFT_BUTTON).command,
            Some((b, Command::Maximize))
        );
    }

    #[test]
    fn a_maximized_window_keeps_its_clicks_and_double_clicks() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        let area = Rect::from_size(SCREEN.into());
        world.stack.maximize(b, area, Some((160, 120).into()));
        // しきい値の手前で離せば、何も起きない。
        world.press_at(80.0, 10.0, LEFT_BUTTON, 1000);
        world.motion(85.0, 13.0);
        let release = world.release(85.0, 13.0, LEFT_BUTTON);
        assert_eq!(release.command, None);
        assert_eq!(world.place("full-b").0, (0, 0));
        // ダブルクリックは、今までどおり元に戻す(2 回目の押下では、動かさない)。
        assert_eq!(
            world.press_at(82.0, 11.0, LEFT_BUTTON, 1300).command,
            Some((b, Command::ToggleMaximize))
        );
        assert_eq!(world.motion(150.0, 100.0).command, None);
        world.release(150.0, 100.0, LEFT_BUTTON);
    }

    #[test]
    fn the_top_of_the_area_previews_and_makes_a_maximize() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        // full-b(外枠の左上 (32, 32))のタイトルバーを grab して、上の端へ。
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(100.0, 20.0);
        assert_eq!(world.interaction.maximize_preview(), None);
        let top = world.motion(100.0, 0.0);
        assert!(top.redraw);
        assert_eq!(world.interaction.maximize_preview(), Some(b));
        // 離れると、予告を消す。
        let away = world.motion(100.0, 30.0);
        assert!(away.redraw);
        assert_eq!(world.interaction.maximize_preview(), None);
        // 上の端で離すと、最大化する。
        world.motion(150.0, 0.0);
        let release = world.release(150.0, 0.0, LEFT_BUTTON);
        assert_eq!(release.command, Some((b, Command::Maximize)));
        assert_eq!(world.interaction.maximize_preview(), None);
        // 上の端から離れて離したら、最大化しない。
        world.press(100.0, 10.0, LEFT_BUTTON);
        world.motion(100.0, 0.0);
        world.motion(120.0, 50.0);
        assert_eq!(world.release(120.0, 50.0, LEFT_BUTTON).command, None);
    }

    #[test]
    fn the_top_of_a_narrowed_area_is_its_own_top() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        // 上に高さ 40 の layer surface があると、使える範囲の上端は y 40。
        let area = Rect::new((0, 40).into(), (640, 440).into());
        world.stack.move_to(b, (32, 72).into());
        world.press(100.0, 80.0, LEFT_BUTTON);
        let motion = |world: &mut World, y: f64| {
            world
                .interaction
                .motion(&mut world.stack, area, at(100.0, y), &Windows);
            world.interaction.maximize_preview()
        };
        assert_eq!(motion(&mut world, 41.0), None);
        assert_eq!(motion(&mut world, 40.0), Some(b));
    }

    #[test]
    fn a_window_of_a_fixed_size_is_not_maximized_at_the_top() {
        // ボタンが閉じるだけのウィンドウ(最大化のボタンが無い)。
        let mut world = World::two();
        world.press(100.0, 40.0, LEFT_BUTTON);
        world.motion(100.0, 0.0);
        assert!(world.interaction.is_moving());
        assert_eq!(world.interaction.maximize_preview(), None);
        assert_eq!(world.release(100.0, 0.0, LEFT_BUTTON).command, None);
    }

    #[test]
    fn a_maximized_window_is_not_resized() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        let area = Rect::new((0, 40).into(), (640, 440).into());
        world.stack.maximize(b, area, None);
        // 外枠のすぐ外(上の y 36)にも、サイズ変更の境界は無い(後ろの full-a のコンテンツに当たる)。
        assert_eq!(world.motion(100.0, 36.0).deliver, Deliver::ToClient);
        assert_eq!(world.interaction.cursor(), Cursor::Default);
        // クライアントからのサイズ変更の頼みは、受けない(移動の頼みは、元の大きさに戻して動かす。下のテスト)。
        world.press(100.0, 100.0, LEFT_BUTTON);
        assert!(world
            .interaction
            .begin_client_resize(
                &mut world.stack,
                area,
                b,
                Edges::RIGHT,
                at(100.0, 100.0),
                &Windows
            )
            .is_none());
    }

    #[test]
    fn a_minimized_window_is_not_hit() {
        let mut world = two_with_buttons();
        let b = world.id("full-b");
        world.stack.set_minimized(b, true);
        // full-b のタイトルバーの所(100, 40)は、後ろの full-a のコンテンツに当たる。
        world.motion(100.0, 40.0);
        assert_eq!(world.interaction.hot_button(b), None);
        let pressed = world.press(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(pressed.deliver, Deliver::ToClient);
        assert!(pressed.raised);
        world.release(100.0, 40.0, LEFT_BUTTON);
        assert_eq!(world.order(), ["full-a", "full-b"]);
    }
}
