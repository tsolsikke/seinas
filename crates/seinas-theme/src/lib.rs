//! Seinas の見た目の値(テーマのトークン)。色・大きさ・余白を、ここ 1 か所にまとめる。
//!
//! コンポジタの飾り(タイトルバー)とタスクバーは、ここから値を読む。値を変えるときは、ここだけを変える。
//! どの値が決定で、どれが仮の値かは、`docs/ui-spec.md` にある。
//!
//! 色は (赤, 緑, 青)。

/// 色(赤, 緑, 青)。
pub type Rgb = [u8; 3];

/// 部品をまたいで使う色。
pub mod color {
    use super::Rgb;

    /// アクセント(前面のタスクの印、トーチの炎)。見本の画像から読んだ値。
    pub const ACCENT: Rgb = [0xf0, 0x93, 0x11];
    /// 選んでいるものを示す色(Alt+Tab の一覧のいまの候補の枠、最大化の予告の枠)。アクセントと同じオレンジに揃える
    /// (2026-10-26。運用者の決定)。線と薄い面にだけ使い、この色の塗りの上に小さな白い文字を置かない(白との
    /// コントラスト比は約 2.4 で、足りない)。
    pub const SELECTION: Rgb = ACCENT;
    /// 主な文字と、補助の文字。
    pub const TEXT: Rgb = [0xf8, 0xf8, 0xf8];
    pub const SUBTLE_TEXT: Rgb = [0x90, 0x9c, 0xb0];
    /// 区切りの線。
    pub const SEPARATOR: Rgb = [0x2f, 0x3d, 0x52];
}

/// Alt+Tab の一覧(コンポジタの中で描く)。
pub mod switcher {
    use super::Rgb;

    /// 文字の大きさ(ピクセル)。
    pub const TEXT_SIZE: f32 = 14.0;
    /// 一覧の地と、外の枠(タイトルバーの背面の地と、背面の枠と同じ)。
    pub const PANEL_BACKGROUND: Rgb = crate::title_bar::INACTIVE_BAR;
    pub const PANEL_BORDER: Rgb = crate::frame::INACTIVE_BORDER;
    /// 候補のタイトル(ふつうと、最小化しているもの)と、隠れた数の行の文字。
    pub const TITLE: Rgb = crate::color::TEXT;
    pub const MINIMIZED_TITLE: Rgb = crate::color::SUBTLE_TEXT;
    /// いまの候補の行:[`crate::color::SELECTION`] の枠(太さ [`SELECTED_BORDER_WIDTH`])と、控えめな地
    /// (`SELECTION` を 16% だけ [`PANEL_BACKGROUND`] に重ねた色)。文字は主な文字の色のまま。
    pub const SELECTED_BORDER_WIDTH: i32 = 2;
    pub const SELECTED_BACKGROUND: Rgb = [0x38, 0x32, 0x2e];
    pub const SELECTED_TEXT: Rgb = crate::color::TEXT;
}

/// 最大化の予告(ウィンドウを使える範囲の上端へ動かしたときに出す)。
pub mod maximize_preview {
    use super::Rgb;

    /// 使える範囲の内側に沿った枠(太さ [`BORDER_WIDTH`])と、その内の薄い面(不透明度 [`FACE_OPACITY`])の色。
    /// 背景のぼかしはしない。
    pub const COLOR: Rgb = crate::color::SELECTION;
    pub const BORDER_WIDTH: i32 = 2;
    pub const FACE_OPACITY: f32 = 0.12;
}

/// タスクバー(`seinas-taskbar`)。
pub mod taskbar {
    use super::{color, Rgb};

    /// 高さ(ピクセル)。exclusive zone も、この高さにする。
    pub const HEIGHT: i32 = 40;
    /// 地と、上の端の 1 ピクセルの線。
    pub const BACKGROUND: Rgb = [0x0f, 0x1b, 0x2f];
    pub const TOP_LINE: Rgb = [0x21, 0x30, 0x4a];

    /// アイコンの大きさ(正方形)。縦は中央に置く。
    pub const ICON_SIZE: i32 = 24;
    /// ポインターが乗ったとき・押したときに光る範囲(正方形。縦は中央)と、その角の丸み。押したときの反応を受ける
    /// 範囲は、これより広く、間隔の枠の全体(横は [`PITCH`]、縦はタスクバーの高さ)。
    pub const HIGHLIGHT_SIZE: i32 = 36;
    pub const HIGHLIGHT_RADIUS: f32 = 4.0;
    /// アイコンの間隔(中心から中心)。入りきらないときは、[`MIN_PITCH`] まで詰める。48 は、運用者が見本と並べて
    /// 決めた(2026-10-26。見本に最も近く、詰まって見えない)。
    pub const PITCH: i32 = 48;
    pub const MIN_PITCH: i32 = 36;
    /// ポインターが乗ったときと、押したときの、光る範囲の地。
    pub const HOT: Rgb = [0x1d, 0x2a, 0x40];
    pub const PRESSED: Rgb = [0x24, 0x34, 0x4d];

    /// 前面のタスクの印(アイコンの真下の線)の長さと太さと色。下の端から [`MARK_GAP`] 上に置く。
    pub const ACTIVE_MARK_WIDTH: i32 = 24;
    pub const MARK_HEIGHT: i32 = 3;
    pub const MARK_GAP: i32 = 3;
    pub const ACTIVE_MARK: Rgb = color::ACCENT;
    /// 背面のタスクの印(点)の径と色。中心の高さは、前面の線と同じ。
    pub const DOT_DIAMETER: f32 = 5.0;
    pub const DOT: Rgb = [0x81, 0x90, 0xab];

    /// 左の端と右の端の空き。時計の場所の幅と、まとまりとの間の空き。
    pub const LEFT_MARGIN: i32 = 8;
    pub const RIGHT_MARGIN: i32 = 14;
    pub const CLOCK_WIDTH: i32 = 100;
    pub const CLOCK_GAP: i32 = 8;
    /// 時計と日付の文字の大きさと色。2 段の各々の行の上の端と高さ(文字は、行の中で縦の中央)。
    pub const TEXT_SIZE: f32 = 14.0;
    pub const TEXT: Rgb = color::TEXT;
    pub const CLOCK_TIME_TOP: i32 = 2;
    pub const CLOCK_DATE_TOP: i32 = 20;
    pub const CLOCK_LINE_HEIGHT: i32 = 18;

    /// 入りきらないタスク(隠れたタスク)をまとめる「…」のボタン。まとまりの末尾に、タスクと同じ間隔の 1 つの場所を
    /// 取る。押したときの反応を受ける範囲は、光る範囲と同じ [`HIGHLIGHT_SIZE`] の正方形。記号は、径
    /// [`MORE_DOT_DIAMETER`] の 3 つの点を、中心の間を [`MORE_DOT_GAP`] にして横に並べる。
    pub const MORE_DOT_DIAMETER: f32 = 4.0;
    pub const MORE_DOT_GAP: i32 = 7;
    pub const MORE_DOT: Rgb = color::TEXT;
    /// 前面のタスクが隠れたときの、「…」の控えめな印(前面の線と同じ高さの、点。背面の点と同じ径)。
    pub const HIDDEN_ACTIVE_MARK: Rgb = color::ACCENT;

    /// 隠れたタスクの一覧(「…」を押すと、その上に出すポップアップ)。幅と、行の高さ、上下の空き。
    pub const LIST_WIDTH: i32 = 320;
    pub const LIST_ROW_HEIGHT: i32 = 40;
    pub const LIST_PADDING: i32 = 4;
    /// 一覧と、タスクバーの上の端との間の空き。
    pub const LIST_GAP: i32 = 4;
    /// 行の中の、アイコン(大きさは [`ICON_SIZE`])の左の空きと、アイコンからタイトルまでの空き、タイトルの右の空き。
    pub const LIST_ICON_LEFT: i32 = 12;
    pub const LIST_TEXT_GAP: i32 = 10;
    pub const LIST_TEXT_RIGHT: i32 = 12;
    /// タイトルの文字の大きさと色(最小化しているものは、補助の文字の色)。
    pub const LIST_TEXT_SIZE: f32 = 15.0;
    pub const LIST_TEXT: Rgb = color::TEXT;
    pub const LIST_MINIMIZED_TEXT: Rgb = color::SUBTLE_TEXT;
    /// 一覧の地と、外の枠(1 ピクセル)。
    pub const LIST_BACKGROUND: Rgb = BACKGROUND;
    pub const LIST_BORDER: Rgb = color::SEPARATOR;
    /// 選んでいる行(ポインターが乗っている行か、矢印キーで選んだ行)の地。
    pub const LIST_SELECTED: Rgb = HOT;
    /// 前面のウィンドウの行の印(行の左の端の、縦の線。幅 [`LIST_ACTIVE_MARK_WIDTH`]、高さはアイコンと同じ)。
    pub const LIST_ACTIVE_MARK: Rgb = color::ACCENT;
    pub const LIST_ACTIVE_MARK_WIDTH: i32 = 3;
}

/// ウィンドウのタイトルバー(コンポジタの飾り)。
pub mod title_bar {
    use super::{color, Rgb};

    /// 高さ(ピクセル)。
    pub const HEIGHT: i32 = 36;
    /// タイトルの文字の大きさ(ピクセル)。
    pub const TEXT_SIZE: f32 = 15.0;
    /// 左の端から、アプリのアイコンまでの空き。アイコンの大きさ(縦は中央)と、アイコンからタイトルまでの空き。
    pub const PADDING: i32 = 10;
    pub const ICON_SIZE: i32 = 20;
    pub const ICON_GAP: i32 = 8;
    /// ボタンの幅(ヒットの範囲。高さはタイトルバーと同じ)。
    pub const BUTTON_WIDTH: i32 = 44;
    /// 最小化・最大化・閉じるの記号の大きさ(ピクセル。線で描く)。
    pub const MARK_SIZE: i32 = 16;
    pub const CLOSE_MARK_SIZE: i32 = 16;

    /// activated のウィンドウの、地と文字。
    pub const ACTIVE_BAR: Rgb = [0x1a, 0x27, 0x3d];
    pub const ACTIVE_TITLE: Rgb = color::TEXT;
    /// activated でないウィンドウの、地と文字。
    pub const INACTIVE_BAR: Rgb = [0x15, 0x20, 0x34];
    pub const INACTIVE_TITLE: Rgb = color::SUBTLE_TEXT;
    /// ポインターが乗ったときの、閉じるボタンの地と記号。閉じるボタンの赤は、乗ったときだけ。
    pub const HOT_CLOSE_BUTTON: Rgb = [0xc4, 0x2b, 0x1c];
    pub const HOT_CLOSE_MARK: Rgb = [0xff, 0xff, 0xff];
    /// 応答なしのウィンドウの、閉じるボタンの地。
    pub const UNRESPONSIVE_CLOSE_BUTTON: Rgb = [0x8a, 0x1f, 0x14];
    /// ポインターが乗ったときの、最小化と最大化のボタンの地(activated のウィンドウと、そうでないもの)。
    pub const HOT_ACTIVE_BUTTON: Rgb = [0x24, 0x34, 0x4d];
    pub const HOT_INACTIVE_BUTTON: Rgb = [0x1d, 0x2a, 0x40];
}

/// ウィンドウの枠・角・影(コンポジタの飾り)。最大化しているときは、どれも付けない。
pub mod frame {
    use super::Rgb;

    /// 枠の太さ(ピクセル)。枠は、外枠(タイトルバーとコンテンツ)の外側に描き、並べ方と大きさの計算は変えない。
    pub const BORDER_WIDTH: i32 = 1;
    /// 枠の色(activated のウィンドウと、そうでないもの)。
    pub const ACTIVE_BORDER: Rgb = [0x4c, 0x70, 0xa4];
    pub const INACTIVE_BORDER: Rgb = [0x43, 0x54, 0x70];
    /// 角の丸み(枠の外側の線で測る)。外枠の角は、これから枠の太さを引いた丸みで欠く。
    pub const CORNER_RADIUS: i32 = 8;
    /// 影:下へのずれ、ぼけの幅(縁から外へ消えるまで)、いちばん濃い所の不透明度。
    pub const SHADOW_OFFSET_Y: i32 = 2;
    pub const SHADOW_BLUR: i32 = 12;
    pub const SHADOW_OPACITY: f32 = 0.35;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_row_ground_is_a_little_of_the_selection_over_the_panel() {
        let blend = |under: u8, over: u8| {
            (f32::from(under) + 0.16 * (f32::from(over) - f32::from(under))).round() as u8
        };
        let expected: Rgb =
            std::array::from_fn(|i| blend(switcher::PANEL_BACKGROUND[i], color::SELECTION[i]));
        assert_eq!(switcher::SELECTED_BACKGROUND, expected);
    }
}
