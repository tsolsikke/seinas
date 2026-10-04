//! 確かめやすい、決まった絵。
//!
//! 画面の写しを見て、向き・色の並び・端の位置が合っているかを確かめられるようにしてある。
//!
//! - 背景: 共通の描画の背景色(暗い青)。
//! - 枠: 画面の端にぴったり沿った白い線。端が欠けていないかが分かる。
//! - 四隅の印: 左上が赤、右上が緑、左下が青、右下が黄。上下左右の向きと、色の並びが分かる。
//! - 中央の印: 白い四角。

use seinas_render::{solid_rect, SolidRect};

pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
pub const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
pub const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
pub const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
pub const YELLOW: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

/// 枠の太さ(画素)。
pub fn frame_thickness(width: i32, height: i32) -> i32 {
    (width.min(height) / 150).max(1)
}

/// 四隅の印の1辺(画素)。
pub fn mark_size(width: i32, height: i32) -> i32 {
    (width.min(height) / 10).max(2)
}

/// `width` × `height` の画面に描く、決まった絵の要素(手前から順)。
pub fn test_picture(width: i32, height: i32) -> Vec<SolidRect> {
    let t = frame_thickness(width, height);
    let m = mark_size(width, height);
    // 印は、枠の内側に接して置く。
    let (left, top) = (t, t);
    let (right, bottom) = (width - t - m, height - t - m);
    vec![
        solid_rect(left, top, m, m, RED),
        solid_rect(right, top, m, m, GREEN),
        solid_rect(left, bottom, m, m, BLUE),
        solid_rect(right, bottom, m, m, YELLOW),
        solid_rect((width - m) / 2, (height - m) / 2, m, m, WHITE),
        solid_rect(0, 0, width, t, WHITE),
        solid_rect(0, height - t, width, t, WHITE),
        solid_rect(0, 0, t, height, WHITE),
        solid_rect(width - t, 0, t, height, WHITE),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use seinas_render::{FrameView, Painter};

    /// (x, y)の画素を(赤, 緑, 青)で読む。
    fn rgb(view: &FrameView<'_>, x: i32, y: i32) -> [u8; 3] {
        let row = view.row(y as usize);
        let at = x as usize * 4;
        [row[at + 2], row[at + 1], row[at]]
    }

    fn check(width: i32, height: i32) {
        let mut painter = Painter::new(width, height).unwrap();
        let view = painter.paint(&test_picture(width, height)).unwrap();
        let t = frame_thickness(width, height);
        let m = mark_size(width, height);
        let white = [0xff, 0xff, 0xff];
        let background = [0x19, 0x1e, 0x28];

        // 枠は、四辺の端の画素まで届く。
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
            (width / 2, 0),
            (width / 2, height - 1),
            (0, height / 2),
            (width - 1, height / 2),
        ] {
            assert_eq!(rgb(&view, x, y), white, "frame at ({x}, {y})");
        }
        // 四隅の印。
        let inset = t + m / 2;
        assert_eq!(rgb(&view, inset, inset), [0xff, 0, 0]);
        assert_eq!(rgb(&view, width - 1 - inset, inset), [0, 0xff, 0]);
        assert_eq!(rgb(&view, inset, height - 1 - inset), [0, 0, 0xff]);
        assert_eq!(
            rgb(&view, width - 1 - inset, height - 1 - inset),
            [0xff, 0xff, 0]
        );
        // 中央の印と、何も無い所。
        assert_eq!(rgb(&view, width / 2, height / 2), white);
        assert_eq!(rgb(&view, width / 2, height / 4), background);
    }

    #[test]
    fn the_picture_has_its_frame_marks_and_background() {
        check(800, 600);
        check(1024, 768);
        check(64, 48);
    }
}
