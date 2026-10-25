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
    use super::Rgb;

    /// 高さ(ピクセル)。exclusive zone も、この高さにする。
    pub const HEIGHT: i32 = 40;
    /// ボタンの上の端と高さ。
    pub const BUTTON_TOP: i32 = 4;
    pub const BUTTON_HEIGHT: i32 = 32;
    /// 左の端と、ボタンどうしの空き。
    pub const LEFT_MARGIN: i32 = 4;
    pub const BUTTON_GAP: i32 = 4;
    /// ボタンの幅の上限。
    pub const BUTTON_MAX_WIDTH: i32 = 200;
    /// ボタンの中の、タイトルの左右の空き。
    pub const TEXT_PADDING: i32 = 10;
    /// 文字の大きさ(ピクセル)。
    pub const TEXT_SIZE: f32 = 14.0;
    /// 右の端の、時計の場所の幅。
    pub const CLOCK_WIDTH: i32 = 72;
    /// ボタンの下の端の、印の高さと幅。
    pub const INDICATOR_HEIGHT: i32 = 3;
    pub const ACTIVE_INDICATOR_WIDTH: i32 = 24;
    pub const INDICATOR_WIDTH: i32 = 8;

    /// タスクバーの地。
    pub const BACKGROUND: Rgb = [0x20, 0x24, 0x2c];
    /// activated のウィンドウのボタンの地。
    pub const ACTIVE_BUTTON: Rgb = [0x3a, 0x40, 0x4c];
    /// ポインターが乗っているボタンの地。
    pub const HOT_BUTTON: Rgb = [0x2e, 0x33, 0x3d];
    /// activated のウィンドウの印。
    pub const ACTIVE_INDICATOR: Rgb = [0x4c, 0xc2, 0xff];
    /// そのほかの、開いているウィンドウの印。
    pub const INDICATOR: Rgb = [0x8a, 0x90, 0x9c];
    /// 文字の色と、最小化しているウィンドウのタイトルの色。
    pub const TEXT: Rgb = [0xf0, 0xf2, 0xf5];
    pub const MINIMIZED_TEXT: Rgb = [0x9a, 0x9f, 0xa8];
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
    /// ボタンの幅(当たりの範囲。高さはタイトルバーと同じ)。
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
