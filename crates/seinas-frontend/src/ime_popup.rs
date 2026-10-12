//! IME の候補の窓(input popup surface)を置く位置の決まり(ADR 0004 の 5)。
//!
//! ADR 0001 の決まりで、Smithay の型を使わない。

use crate::geometry::{Point, Rect, Size};

/// 大きさ `size` の候補の窓を、画面の上のカーソルの矩形 `cursor` の近くに置く位置(左上)。
///
/// - カーソルの矩形の下に、左の端をそろえて置く。
/// - 画面の下の端からはみ出すなら、カーソルの矩形の上に置く。
/// - それでも画面(大きさ `output`)からはみ出す分は、画面の中へ寄せる(右の端、左の端、上の端)。候補の窓が画面
///   より大きければ、左上をそろえる。
pub fn popup_location(output: Size, cursor: Rect, size: Size) -> Point {
    let below = cursor.loc.y + cursor.size.h;
    let y = if below + size.h <= output.h {
        below
    } else {
        cursor.loc.y - size.h
    };
    let clamp = |value: i32, length: i32, limit: i32| value.min(limit - length).max(0);
    Point {
        x: clamp(cursor.loc.x, size.w, output.w),
        y: clamp(y, size.h, output.h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: Size = Size { w: 640, h: 480 };
    const POPUP: Size = Size { w: 40, h: 30 };

    fn cursor(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn below_the_cursor() {
        assert_eq!(
            popup_location(OUTPUT, cursor(10, 44, 2, 16), POPUP),
            (10, 60).into()
        );
    }

    #[test]
    fn above_the_cursor_at_the_bottom_edge() {
        // 下に置くと 472 + 30 > 480 なので、上(464 - 30)。
        assert_eq!(
            popup_location(OUTPUT, cursor(10, 464, 2, 8), POPUP),
            (10, 434).into()
        );
        // ちょうど入るなら、下。
        assert_eq!(
            popup_location(OUTPUT, cursor(10, 434, 2, 16), POPUP),
            (10, 450).into()
        );
    }

    #[test]
    fn inside_the_right_and_left_edges() {
        assert_eq!(
            popup_location(OUTPUT, cursor(620, 44, 2, 16), POPUP),
            (600, 60).into()
        );
        assert_eq!(
            popup_location(OUTPUT, cursor(-20, 44, 2, 16), POPUP),
            (0, 60).into()
        );
    }

    #[test]
    fn inside_the_top_edge() {
        // 上にも入らない(低い画面で、カーソルが上にある)ときは、上の端へ寄せる。
        let short = Size { w: 640, h: 40 };
        assert_eq!(
            popup_location(short, cursor(10, 5, 2, 16), POPUP),
            (10, 0).into()
        );
    }

    #[test]
    fn a_popup_larger_than_the_output() {
        let large = Size { w: 800, h: 600 };
        assert_eq!(
            popup_location(OUTPUT, cursor(10, 44, 2, 16), large),
            (0, 0).into()
        );
    }
}
