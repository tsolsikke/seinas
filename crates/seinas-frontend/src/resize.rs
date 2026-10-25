//! 縁と角でのサイズ変更の決まり。
//!
//! Wayland のオブジェクトに触れずに確かめられるよう、Seinas の幾何の型だけで書いてある(ADR 0001)。
//! 決まりの説明は `docs/window-placement.md` にある。

use crate::{
    decoration::in_cut_corner,
    geometry::{Point, PointF, Rect, Size},
};

/// 外枠の外側の、サイズ変更に使う境界の幅(ピクセル)。境界は見えない。
pub const RESIZE_BORDER: i32 = 8;
/// 角として扱う長さ(ピクセル)。左右の境界の上下の端から、これだけの所までは、上か下の辺も一緒に動かす
/// (上下の境界も同じ)。角に当てやすくするため。
pub const CORNER_LENGTH: i32 = 16;
/// Seinas が決める、コンテンツの最小の大きさ。クライアントの最小の大きさが、これより小さくても、これを守る。
pub const MIN_CONTENT_SIZE: Size = Size { w: 96, h: 48 };

/// サイズ変更で動かす辺。角は、2 つの辺の組み合わせ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl Edges {
    pub const TOP: Edges = Edges::new(true, false, false, false);
    pub const BOTTOM: Edges = Edges::new(false, true, false, false);
    pub const LEFT: Edges = Edges::new(false, false, true, false);
    pub const RIGHT: Edges = Edges::new(false, false, false, true);
    pub const TOP_LEFT: Edges = Edges::new(true, false, true, false);
    pub const TOP_RIGHT: Edges = Edges::new(true, false, false, true);
    pub const BOTTOM_LEFT: Edges = Edges::new(false, true, true, false);
    pub const BOTTOM_RIGHT: Edges = Edges::new(false, true, false, true);

    pub const fn new(top: bool, bottom: bool, left: bool, right: bool) -> Self {
        Edges {
            top,
            bottom,
            left,
            right,
        }
    }

    /// どの辺も動かさないか。
    pub fn is_empty(self) -> bool {
        self == Edges::default()
    }
}

/// 外枠が `outer` のウィンドウで、点 `point` がサイズ変更の境界の上にあれば、動かす辺。
///
/// 境界は、外枠の外側の [`RESIZE_BORDER`] の幅と、外枠の角を欠いた所(`decoration::in_cut_corner`。角の 2 つの辺を
/// 動かす)。そのほかの外枠の中と、境界の外はNone。
pub fn edges_at(outer: Rect, point: PointF) -> Option<Edges> {
    let grown = Rect::new(
        Point::new(outer.loc.x - RESIZE_BORDER, outer.loc.y - RESIZE_BORDER),
        Size::new(
            outer.size.w + 2 * RESIZE_BORDER,
            outer.size.h + 2 * RESIZE_BORDER,
        ),
    );
    if !grown.contains(point) || (outer.contains(point) && !in_cut_corner(outer, point)) {
        return None;
    }
    let (left, top) = (outer.loc.x as f64, outer.loc.y as f64);
    let (right, bottom) = (
        (outer.loc.x + outer.size.w) as f64,
        (outer.loc.y + outer.size.h) as f64,
    );
    let corner = CORNER_LENGTH as f64;
    // 角を欠いた所は、外枠の中だが、近い角の 2 つの辺を動かす。
    let cut = outer.contains(point);
    let middle = |a: f64, low: f64, high: f64| a < (low + high) / 2.0;
    let mut edges = Edges::new(
        point.y < top || (cut && middle(point.y, top, bottom)),
        point.y >= bottom || (cut && !middle(point.y, top, bottom)),
        point.x < left || (cut && middle(point.x, left, right)),
        point.x >= right || (cut && !middle(point.x, left, right)),
    );
    // 角の近くは、もう 1 つの辺も動かす。
    if edges.left || edges.right {
        edges.top |= point.y < top + corner;
        edges.bottom |= point.y >= bottom - corner;
    }
    if edges.top || edges.bottom {
        edges.left |= point.x < left + corner;
        edges.right |= point.x >= right - corner;
    }
    Some(edges)
}

/// 修飾キーを押しながら外枠 `outer` の中の点 `point` を押して大きさを変えるときに、動かす辺。外枠を縦と横に
/// 3 つずつに分け、点に近い側の辺を動かす(左の 3 分の 1 なら左の辺、右の 3 分の 1 なら右の辺。縦も同じ)。真ん中
/// なら、右下の角。
pub fn edges_toward(outer: Rect, point: PointF) -> Edges {
    let third = |start: i32, length: i32, at: f64| {
        let offset = (at - f64::from(start)) / f64::from(length.max(1));
        (offset < 1.0 / 3.0, offset >= 2.0 / 3.0)
    };
    let (left, right) = third(outer.loc.x, outer.size.w, point.x);
    let (top, bottom) = third(outer.loc.y, outer.size.h, point.y);
    let edges = Edges::new(top, bottom, left, right);
    if edges.is_empty() {
        Edges::BOTTOM_RIGHT
    } else {
        edges
    }
}

/// サイズ変更を始めたときの様子。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResizeStart {
    pub edges: Edges,
    /// 押した点。
    pub press: PointF,
    /// 外枠の左上の位置と、コンテンツの大きさ。
    pub location: Point,
    pub content: Size,
}

/// クライアントが決めた、コンテンツの最小と最大の大きさ。0 は「決めていない」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SizeLimits {
    pub min: Size,
    pub max: Size,
}

/// ポインターが `point` にあるときの、コンテンツの新しい大きさ。
///
/// 動かす辺だけを、押した点からのずれのぶん動かす。最小(クライアントの最小と [`MIN_CONTENT_SIZE`] の
/// 大きいほう)と、クライアントの最大を守る。上の辺を動かすときは、タイトルバーが使える範囲 `area` の上に
/// 出ないようにする。
pub fn resized_content(start: ResizeStart, point: PointF, limits: SizeLimits, area: Rect) -> Size {
    let (dx, dy) = (
        (point.x - start.press.x).round() as i32,
        (point.y - start.press.y).round() as i32,
    );
    let edges = start.edges;
    let mut w = start.content.w;
    let mut h = start.content.h;
    if edges.right {
        w += dx;
    } else if edges.left {
        w -= dx;
    }
    if edges.bottom {
        h += dy;
    } else if edges.top {
        h -= dy;
        // 下の端は動かないので、外枠の上の端(タイトルバーがあればその上の端)が area の上に来る高さまで。外枠の
        // 下の端は、外枠の左上 + タイトルバー + コンテンツなので、タイトルバーの高さによらず、外枠の左上から
        // area の上の端までの分だけ伸ばせる。
        h = h.min(start.location.y + start.content.h - area.loc.y);
    }
    let limit = |value: i32, min: i32, max: i32, own_min: i32| {
        let min = min.max(own_min);
        let value = if max > 0 {
            value.min(max.max(min))
        } else {
            value
        };
        value.max(min)
    };
    // 動かさない向きは、そのまま(クライアントが最小より小さい大きさで出していても、変えない)。
    if edges.left || edges.right {
        w = limit(w, limits.min.w, limits.max.w, MIN_CONTENT_SIZE.w);
    }
    if edges.top || edges.bottom {
        h = limit(h, limits.min.h, limits.max.h, MIN_CONTENT_SIZE.h);
    }
    Size::new(w, h)
}

/// コンテンツの大きさが `now` になったときの、外枠の左上の位置。動かさない辺(右か下)の位置を保つ。
pub fn anchored_location(start: ResizeStart, now: Size) -> Point {
    let x = if start.edges.left {
        start.location.x + start.content.w - now.w
    } else {
        start.location.x
    };
    let y = if start.edges.top {
        start.location.y + start.content.h - now.h
    } else {
        start.location.y
    };
    Point::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoration::TITLE_BAR_HEIGHT;

    const AREA: Rect = Rect {
        loc: Point { x: 0, y: 0 },
        size: Size { w: 640, h: 480 },
    };

    /// 外枠の左上が (100, 100)、コンテンツが 200x150(外枠は 200x186)のウィンドウ。
    fn outer() -> Rect {
        Rect::new(Point::new(100, 100), Size::new(200, 150 + TITLE_BAR_HEIGHT))
    }

    fn start(edges: Edges, press: (f64, f64)) -> ResizeStart {
        ResizeStart {
            edges,
            press: press.into(),
            location: Point::new(100, 100),
            content: Size::new(200, 150),
        }
    }

    #[test]
    fn the_band_outside_the_frame_has_four_sides_and_four_corners() {
        let at = |x: f64, y: f64| edges_at(outer(), (x, y).into());
        // 外枠は x 100〜300、y 100〜286。境界は、その外側の 8 ピクセル。
        assert_eq!(at(150.0, 92.0), Some(Edges::TOP));
        assert_eq!(at(150.0, 99.9), Some(Edges::TOP));
        assert_eq!(at(150.0, 286.0), Some(Edges::BOTTOM));
        assert_eq!(at(150.0, 293.9), Some(Edges::BOTTOM));
        assert_eq!(at(92.0, 200.0), Some(Edges::LEFT));
        assert_eq!(at(300.0, 200.0), Some(Edges::RIGHT));
        assert_eq!(at(307.9, 200.0), Some(Edges::RIGHT));
        // 本当の角。
        assert_eq!(at(95.0, 95.0), Some(Edges::TOP_LEFT));
        assert_eq!(at(305.0, 95.0), Some(Edges::TOP_RIGHT));
        assert_eq!(at(95.0, 290.0), Some(Edges::BOTTOM_LEFT));
        assert_eq!(at(305.0, 290.0), Some(Edges::BOTTOM_RIGHT));
        // 角から 16 ピクセルまでは、角として扱う。
        assert_eq!(at(95.0, 115.9), Some(Edges::TOP_LEFT));
        assert_eq!(at(95.0, 116.0), Some(Edges::LEFT));
        assert_eq!(at(284.0, 95.0), Some(Edges::TOP_RIGHT));
        assert_eq!(at(283.9, 95.0), Some(Edges::TOP));
        assert_eq!(at(305.0, 270.0), Some(Edges::BOTTOM_RIGHT));
        assert_eq!(at(115.9, 290.0), Some(Edges::BOTTOM_LEFT));
        // 外枠の角を欠いた所(丸み 7 の外)は、その角。
        assert_eq!(at(100.5, 100.5), Some(Edges::TOP_LEFT));
        assert_eq!(at(299.5, 100.5), Some(Edges::TOP_RIGHT));
        assert_eq!(at(100.5, 285.5), Some(Edges::BOTTOM_LEFT));
        assert_eq!(at(299.5, 285.5), Some(Edges::BOTTOM_RIGHT));
        // そのほかの外枠の中と、境界の外は、当たらない。
        assert_eq!(at(103.0, 103.0), None);
        assert_eq!(at(100.0, 110.0), None);
        assert_eq!(at(299.9, 280.0), None);
        assert_eq!(at(91.9, 200.0), None);
        assert_eq!(at(150.0, 294.0), None);
        assert_eq!(at(308.0, 200.0), None);
    }

    #[test]
    fn a_modifier_resize_moves_the_edges_near_the_point() {
        // 外枠は x 100〜300、y 100〜286。3 つに分けた境は、x 166.7・233.3、y 162・224。
        let at = |x: f64, y: f64| edges_toward(outer(), (x, y).into());
        assert_eq!(at(110.0, 110.0), Edges::TOP_LEFT);
        assert_eq!(at(290.0, 110.0), Edges::TOP_RIGHT);
        assert_eq!(at(110.0, 280.0), Edges::BOTTOM_LEFT);
        assert_eq!(at(290.0, 280.0), Edges::BOTTOM_RIGHT);
        assert_eq!(at(200.0, 110.0), Edges::TOP);
        assert_eq!(at(200.0, 280.0), Edges::BOTTOM);
        assert_eq!(at(110.0, 190.0), Edges::LEFT);
        assert_eq!(at(290.0, 190.0), Edges::RIGHT);
        // 真ん中は、右下の角。
        assert_eq!(at(200.0, 190.0), Edges::BOTTOM_RIGHT);
    }

    #[test]
    fn each_edge_and_corner_changes_the_size_and_keeps_the_opposite_edge() {
        let none = SizeLimits::default();
        // (辺、押した点、動かした点、新しいコンテンツの大きさ、新しい左上)
        let cases = [
            (
                Edges::RIGHT,
                (304.0, 200.0),
                (354.0, 230.0),
                (250, 150),
                (100, 100),
            ),
            (
                Edges::BOTTOM,
                (200.0, 278.0),
                (230.0, 318.0),
                (200, 190),
                (100, 100),
            ),
            (
                Edges::LEFT,
                (96.0, 200.0),
                (66.0, 230.0),
                (230, 150),
                (70, 100),
            ),
            (
                Edges::TOP,
                (200.0, 96.0),
                (230.0, 76.0),
                (200, 170),
                (100, 80),
            ),
            (
                Edges::TOP_LEFT,
                (96.0, 96.0),
                (126.0, 136.0),
                (170, 110),
                (130, 140),
            ),
            (
                Edges::TOP_RIGHT,
                (304.0, 96.0),
                (344.0, 86.0),
                (240, 160),
                (100, 90),
            ),
            (
                Edges::BOTTOM_LEFT,
                (96.0, 278.0),
                (56.0, 268.0),
                (240, 140),
                (60, 100),
            ),
            (
                Edges::BOTTOM_RIGHT,
                (304.0, 278.0),
                (284.0, 298.0),
                (180, 170),
                (100, 100),
            ),
        ];
        for (edges, press, point, size, location) in cases {
            let start = start(edges, press);
            let now = resized_content(start, point.into(), none, AREA);
            assert_eq!(now, size.into(), "{edges:?}");
            assert_eq!(anchored_location(start, now), location.into(), "{edges:?}");
        }
    }

    #[test]
    fn the_size_keeps_the_limits() {
        let shrink = |edges, limits| {
            resized_content(
                start(edges, (304.0, 278.0)),
                (0.0, 0.0).into(),
                limits,
                AREA,
            )
        };
        let grow = |edges, limits| {
            resized_content(
                start(edges, (304.0, 278.0)),
                (900.0, 900.0).into(),
                limits,
                AREA,
            )
        };
        // クライアントが決めていなければ、Seinas の最小だけを守り、大きいほうは限らない。
        let none = SizeLimits::default();
        assert_eq!(shrink(Edges::BOTTOM_RIGHT, none), MIN_CONTENT_SIZE);
        assert_eq!(grow(Edges::BOTTOM_RIGHT, none), Size::new(796, 772));
        // クライアントの最小と最大を守る。最小が Seinas のものより小さければ、Seinas のもの。
        let limits = SizeLimits {
            min: Size::new(150, 20),
            max: Size::new(300, 0),
        };
        assert_eq!(shrink(Edges::BOTTOM_RIGHT, limits), Size::new(150, 48));
        assert_eq!(grow(Edges::BOTTOM_RIGHT, limits), Size::new(300, 772));
        // 最大が最小より小さいときは、最小を守る。
        let odd = SizeLimits {
            min: Size::new(150, 60),
            max: Size::new(120, 50),
        };
        assert_eq!(grow(Edges::BOTTOM_RIGHT, odd), Size::new(150, 60));
        // 動かさない向きは、限りに合わなくても変えない。
        assert_eq!(shrink(Edges::RIGHT, limits), Size::new(150, 150));
        assert_eq!(shrink(Edges::BOTTOM, limits), Size::new(200, 48));
    }

    #[test]
    fn the_top_edge_stops_at_the_top_of_the_usable_area() {
        // 使える範囲の上の端が 40(上にパネルがある)。下の端(286)は動かないので、コンテンツの高さは
        // 286 - 36 - 40 = 210 まで。
        let area = Rect::new(Point::new(0, 40), Size::new(640, 440));
        let start = start(Edges::TOP, (200.0, 96.0));
        let now = resized_content(start, (200.0, 0.0).into(), SizeLimits::default(), area);
        assert_eq!(now, Size::new(200, 210));
        assert_eq!(anchored_location(start, now), Point::new(100, 40));
    }
}
