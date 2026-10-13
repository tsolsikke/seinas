//! ウィンドウの切り替えの一覧の絵(切り替えの最中に、出力の中央に出す)。
//!
//! ADR 0001 の決まりで、Smithay の型を使わない。並べ方([`switcher_layout`])と、絵([`paint_switcher`])を分け、
//! どちらも Smithay 無しで確かめられるようにする。絵のピクセルの形式は、ほかの絵と同じく B, G, R, X。
//!
//! - 候補を、切り替えの順(使った順)に 1 行ずつ並べ、今の候補の行を強調する。
//! - タイトルは `seinas-text` で描き、幅に収まらなければ末尾を切って「…」を付ける。タイトルの無いウィンドウは
//!   「(タイトルなし)」と書く。
//! - 最小化しているウィンドウは、行の左に「−」の印を付け、タイトルを暗い色で書く。
//! - 一覧は、出力の中に収める。幅は [`PANEL_WIDTH`] までで、出力が狭ければ縮める。行が入りきらないときは、今の
//!   候補が見える範囲だけを出し、上と下に、隠れた数を「▲ ほか N 件」「▼ ほか N 件」の行で出す(行の数は、
//!   [`MAX_ROWS`] まで)。

use seinas_text::{Canvas, Line, TextPainter};

use crate::{
    decoration::{ACTIVE_BAR, ACTIVE_TITLE, TITLE_SIZE},
    geometry::{Point, Rect, Size},
};

/// 一覧の幅の上限(ピクセル)。
pub const PANEL_WIDTH: i32 = 480;
/// 出力の端から空ける幅。
pub const PANEL_MARGIN: i32 = 16;
/// 一覧の内側の余白。
pub const PANEL_PADDING: i32 = 8;
/// 1 行の高さ。
pub const ROW_HEIGHT: i32 = 28;
/// 行の左の、印を描く所の幅。
const GUTTER: i32 = 20;
/// 出す行の数の上限。
pub const MAX_ROWS: usize = 9;
/// 枠の太さ。
const BORDER: i32 = 2;

/// 一覧の地の色、枠の色、今の候補の行の色(赤, 緑, 青)。
pub const SWITCHER_BACKGROUND: [u8; 3] = [0x20, 0x24, 0x2c];
pub const SWITCHER_BORDER: [u8; 3] = [0x6a, 0x9c, 0xe0];
pub const SWITCHER_HIGHLIGHT: [u8; 3] = ACTIVE_BAR;
/// タイトルの色(ふつう、今の候補、最小化しているもの)。
const TITLE: [u8; 3] = [0xd8, 0xdc, 0xe4];
const MINIMIZED_TITLE: [u8; 3] = [0x8c, 0x92, 0x9c];
/// 隠れた数の行の色。
const MORE: [u8; 3] = [0xa0, 0xa6, 0xb0];
/// タイトルの無いウィンドウに出す文字。
pub const UNTITLED: &str = "(タイトルなし)";

/// 一覧の 1 つの候補。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitcherEntry {
    pub title: String,
    pub minimized: bool,
}

/// 一覧の 1 行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// 候補(番号は、候補の一覧の中の位置)。
    Entry(usize),
    /// 上に隠れた候補の数。
    MoreAbove(usize),
    /// 下に隠れた候補の数。
    MoreBelow(usize),
}

/// 一覧の置き方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitcherLayout {
    /// 出力の上の、一覧の矩形。
    pub rect: Rect,
    /// 上から順の行。
    pub rows: Vec<Row>,
}

/// `count` 個の候補の、`current` 番目が今の候補のとき、大きさ `output` の出力に置く一覧の置き方。候補が無ければ
/// None。
pub fn switcher_layout(count: usize, current: usize, output: Size) -> Option<SwitcherLayout> {
    if count == 0 || output.w <= 0 || output.h <= 0 {
        return None;
    }
    let current = current.min(count - 1);
    let room = (output.h - 2 * PANEL_MARGIN - 2 * PANEL_PADDING) / ROW_HEIGHT;
    let max_rows = (room.max(1) as usize).min(MAX_ROWS);
    let rows = if count <= max_rows {
        (0..count).map(Row::Entry).collect()
    } else if max_rows < 3 {
        // 隠れた数を出す行が取れないほど低いときは、今の候補だけ。
        vec![Row::Entry(current)]
    } else {
        // 上と下の両方に隠れた数の行を出すとして、今の候補が中ほどに来るように始めを決める。どちらかの端に
        // 届いたら、その側の行を候補に回す。
        let (start, end) = if current < max_rows - 1 - (max_rows - 2) / 2 {
            (0, max_rows - 1)
        } else if current >= count - (max_rows - 1) + (max_rows - 2) / 2 {
            (count - (max_rows - 1), count)
        } else {
            let start = current - (max_rows - 2) / 2;
            (start, start + max_rows - 2)
        };
        let mut rows = Vec::new();
        if start > 0 {
            rows.push(Row::MoreAbove(start));
        }
        rows.extend((start..end).map(Row::Entry));
        if end < count {
            rows.push(Row::MoreBelow(count - end));
        }
        rows
    };
    let width = PANEL_WIDTH.min(output.w - 2 * PANEL_MARGIN).max(1);
    let height = (2 * PANEL_PADDING + rows.len() as i32 * ROW_HEIGHT).min(output.h);
    let loc = Point::new((output.w - width) / 2, ((output.h - height) / 2).max(0));
    Some(SwitcherLayout {
        rect: Rect::new(loc, Size::new(width, height)),
        rows,
    })
}

/// 一覧の絵(B, G, R, X)を描く。大きさは `layout.rect.size`。
pub fn paint_switcher(
    painter: &mut TextPainter,
    entries: &[SwitcherEntry],
    current: usize,
    layout: &SwitcherLayout,
) -> Vec<u8> {
    let Size { w, h } = layout.rect.size;
    let bgrx = |c: [u8; 3]| [c[2], c[1], c[0], 0xff];
    let mut pixels = bgrx(SWITCHER_BORDER).repeat((w * h) as usize);
    fill(
        &mut pixels,
        w,
        BORDER,
        BORDER,
        w - 2 * BORDER,
        h - 2 * BORDER,
        SWITCHER_BACKGROUND,
    );
    for (index, row) in layout.rows.iter().enumerate() {
        let top = PANEL_PADDING + index as i32 * ROW_HEIGHT;
        let left = PANEL_PADDING + GUTTER;
        let max_width = w - left - PANEL_PADDING;
        let (text, color) = match *row {
            Row::Entry(i) => {
                let entry = &entries[i];
                let chosen = i == current;
                if chosen {
                    fill(
                        &mut pixels,
                        w,
                        PANEL_PADDING / 2,
                        top + 1,
                        w - PANEL_PADDING,
                        ROW_HEIGHT - 2,
                        SWITCHER_HIGHLIGHT,
                    );
                }
                if entry.minimized {
                    // 最小化の印(タイトルバーの最小化のボタンと同じ「−」)。
                    let mark = if chosen {
                        ACTIVE_TITLE
                    } else {
                        MINIMIZED_TITLE
                    };
                    fill(
                        &mut pixels,
                        w,
                        PANEL_PADDING + 4,
                        top + ROW_HEIGHT / 2 - 1,
                        GUTTER - 8,
                        2,
                        mark,
                    );
                }
                let color = match (chosen, entry.minimized) {
                    (true, _) => ACTIVE_TITLE,
                    (false, true) => MINIMIZED_TITLE,
                    (false, false) => TITLE,
                };
                let title = if entry.title.is_empty() {
                    UNTITLED.to_owned()
                } else {
                    entry.title.clone()
                };
                (title, color)
            }
            Row::MoreAbove(n) => (format!("▲ ほか {n} 件"), MORE),
            Row::MoreBelow(n) => (format!("▼ ほか {n} 件"), MORE),
        };
        painter.draw_line(
            &mut Canvas::new(&mut pixels, w as usize, h as usize),
            &text,
            Line {
                size: TITLE_SIZE,
                color,
                x: left,
                top,
                height: ROW_HEIGHT,
                max_width,
            },
        );
    }
    pixels
}

/// 矩形 (x, y, width, height) を色 `color` で塗る(絵の外は切る)。
fn fill(pixels: &mut [u8], stride: i32, x: i32, y: i32, width: i32, height: i32, color: [u8; 3]) {
    let rows = pixels.len() as i32 / 4 / stride.max(1);
    for row in y.max(0)..(y + height).min(rows) {
        for column in x.max(0)..(x + width).min(stride) {
            let at = ((row * stride + column) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&[color[2], color[1], color[0], 0xff]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: Size = Size { w: 1280, h: 720 };

    fn entries(rows: &[Row]) -> Vec<Row> {
        rows.to_vec()
    }

    #[test]
    fn a_few_candidates_are_all_shown_in_the_center() {
        let layout = switcher_layout(3, 1, OUTPUT).unwrap();
        assert_eq!(
            layout.rows,
            entries(&[Row::Entry(0), Row::Entry(1), Row::Entry(2)])
        );
        let height = 2 * PANEL_PADDING + 3 * ROW_HEIGHT;
        assert_eq!(
            layout.rect,
            Rect::new(
                Point::new((1280 - PANEL_WIDTH) / 2, (720 - height) / 2),
                Size::new(PANEL_WIDTH, height)
            )
        );
        assert!(switcher_layout(0, 0, OUTPUT).is_none());
    }

    #[test]
    fn many_candidates_show_the_current_one_and_the_hidden_counts() {
        // 20 個、行は 9 まで。
        let start = switcher_layout(20, 0, OUTPUT).unwrap().rows;
        assert_eq!(start.len(), MAX_ROWS);
        assert_eq!(start[0], Row::Entry(0));
        assert_eq!(start[8], Row::MoreBelow(12));
        let middle = switcher_layout(20, 10, OUTPUT).unwrap().rows;
        assert_eq!(middle.len(), MAX_ROWS);
        assert_eq!(middle[0], Row::MoreAbove(7));
        assert!(middle.contains(&Row::Entry(10)));
        assert_eq!(middle[8], Row::MoreBelow(6));
        let end = switcher_layout(20, 19, OUTPUT).unwrap().rows;
        assert_eq!(end[0], Row::MoreAbove(12));
        assert_eq!(end[8], Row::Entry(19));
        // どの今の候補でも、それが出ていて、隠れた数と出ている候補の数を足すと 20。
        for current in 0..20 {
            let rows = switcher_layout(20, current, OUTPUT).unwrap().rows;
            assert!(rows.contains(&Row::Entry(current)), "{current}: {rows:?}");
            let total: usize = rows
                .iter()
                .map(|row| match row {
                    Row::Entry(_) => 1,
                    Row::MoreAbove(n) | Row::MoreBelow(n) => *n,
                })
                .sum();
            assert_eq!(total, 20, "{current}: {rows:?}");
        }
    }

    #[test]
    fn the_panel_stays_inside_a_small_output() {
        let small = Size::new(320, 240);
        for count in [1, 3, 6, 7, 30] {
            for current in [0, count / 2, count - 1] {
                let layout = switcher_layout(count, current, small).unwrap();
                let r = layout.rect;
                assert!(r.loc.x >= 0 && r.loc.y >= 0, "{count} {current}: {r:?}");
                assert!(r.loc.x + r.size.w <= 320 && r.loc.y + r.size.h <= 240);
                assert!(layout.rows.contains(&Row::Entry(current)));
            }
        }
        // 240 の高さには 6 行(240 - 32 - 16 = 192、192 / 28 = 6)。
        assert_eq!(switcher_layout(30, 15, small).unwrap().rows.len(), 6);
        // とても低い出力では、今の候補だけ。
        let tiny = Size::new(200, 60);
        assert_eq!(switcher_layout(5, 3, tiny).unwrap().rows, [Row::Entry(3)]);
    }
}
