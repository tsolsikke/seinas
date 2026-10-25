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

    /// 選んでいるものを示す色(Alt+Tab の一覧のいまの候補、最大化の予告の枠)。
    pub const SELECTION: Rgb = [0x2f, 0x6f, 0xb5];
    /// アクセント(前面のタスクの印、トーチの炎)。見本の画像から読んだ値。
    pub const ACCENT: Rgb = [0xf0, 0x93, 0x11];
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
    /// いまの候補の行の文字(地は [`crate::color::SELECTION`])。
    pub const SELECTED_TEXT: Rgb = [0xff, 0xff, 0xff];
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
}

/// ウィンドウのタイトルバー(コンポジタの飾り)。
pub mod title_bar {
    use super::Rgb;

    /// 高さ(ピクセル)。
    pub const HEIGHT: i32 = 24;
    /// タイトルの文字の大きさ(ピクセル)。
    pub const TEXT_SIZE: f32 = 14.0;
    /// 左右の端から、タイトルまでの空き。
    pub const PADDING: i32 = 8;
    /// ボタンの幅(ヒットの範囲。高さはタイトルバーと同じ)。
    pub const BUTTON_WIDTH: i32 = HEIGHT;
    /// 最小化と最大化の記号の大きさと、閉じるの「×」の大きさ。
    pub const MARK_SIZE: i32 = 10;
    pub const CLOSE_MARK_SIZE: i32 = 10;

    /// activated のウィンドウの、地と文字。
    pub const ACTIVE_BAR: Rgb = [0x2f, 0x6f, 0xb5];
    pub const ACTIVE_TITLE: Rgb = [0xff, 0xff, 0xff];
    /// activated でないウィンドウの、地と文字。
    pub const INACTIVE_BAR: Rgb = [0x4a, 0x50, 0x5c];
    pub const INACTIVE_TITLE: Rgb = [0xc8, 0xcc, 0xd4];
    /// ポインターが乗ったときの、閉じるボタンの地と記号。
    pub const HOT_CLOSE_BUTTON: Rgb = [0xc4, 0x2b, 0x1c];
    pub const HOT_CLOSE_MARK: Rgb = [0xff, 0xff, 0xff];
    /// 応答なしのウィンドウの、閉じるボタンの地。
    pub const UNRESPONSIVE_CLOSE_BUTTON: Rgb = [0x8a, 0x1f, 0x14];
    /// ポインターが乗ったときの、最小化と最大化のボタンの地(activated のウィンドウと、そうでないもの)。
    pub const HOT_ACTIVE_BUTTON: Rgb = [0x46, 0x82, 0xc4];
    pub const HOT_INACTIVE_BUTTON: Rgb = [0x5c, 0x63, 0x70];
}
