//! ウィンドウ管理の決まりが使う幾何の型。
//!
//! ADR 0001 の決まりで、ウィンドウ管理の決まり(スタック、ポインターの操作、タイトルバーの計算)は、土台
//! (Smithay)の型を使わない。Smithay の型との変換は、フロントエンドの中(`lib.rs`)で行う。
//! 座標は、画面の上の論理座標(拡大率は 1)。

/// 整数の点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 小数の点(ポインターの位置など)。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

/// 大きさ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

/// 矩形。左上の点 `loc` と、大きさ `size`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub loc: Point,
    pub size: Size,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }

    pub fn to_f64(self) -> PointF {
        PointF {
            x: self.x as f64,
            y: self.y as f64,
        }
    }
}

impl PointF {
    pub const fn new(x: f64, y: f64) -> Self {
        PointF { x, y }
    }
}

impl Size {
    pub const fn new(w: i32, h: i32) -> Self {
        Size { w, h }
    }
}

impl Rect {
    pub const fn new(loc: Point, size: Size) -> Self {
        Rect { loc, size }
    }

    /// 画面の左上を原点にした、大きさ `size` の矩形。
    pub const fn from_size(size: Size) -> Self {
        Rect {
            loc: Point { x: 0, y: 0 },
            size,
        }
    }

    /// 点 `point` が中にあるか。左と上の辺は含み、右と下の辺は含まない。
    pub fn contains(&self, point: PointF) -> bool {
        let (left, top) = (self.loc.x as f64, self.loc.y as f64);
        point.x >= left
            && point.y >= top
            && point.x < left + self.size.w as f64
            && point.y < top + self.size.h as f64
    }
}

impl From<(i32, i32)> for Point {
    fn from((x, y): (i32, i32)) -> Self {
        Point { x, y }
    }
}

impl From<(f64, f64)> for PointF {
    fn from((x, y): (f64, f64)) -> Self {
        PointF { x, y }
    }
}

impl From<(i32, i32)> for Size {
    fn from((w, h): (i32, i32)) -> Self {
        Size { w, h }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_contains_its_top_left_but_not_its_bottom_right() {
        let rect = Rect::new((10, 20).into(), (30, 40).into());
        assert!(rect.contains((10.0, 20.0).into()));
        assert!(rect.contains((39.9, 59.9).into()));
        assert!(!rect.contains((40.0, 30.0).into()));
        assert!(!rect.contains((20.0, 60.0).into()));
        assert!(!rect.contains((9.9, 30.0).into()));
        assert!(Rect::from_size((5, 5).into()).contains((0.0, 0.0).into()));
    }
}
