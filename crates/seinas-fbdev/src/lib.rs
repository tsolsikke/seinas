//! Seinasの裏側の1つ: Linuxのfbdev(`/dev/fb0`)の形の画面へ、共通の描画の結果を出す。
//!
//! Waylandのソケットは開かない。依存は共通の描画(`seinas-render`)とlibcだけである。
//!
//! - `layout`: 画面の形式(1画素のビット数、色の位置、1行のバイト数)を調べ、描画結果を
//!   その形式に直して写す。形式の差はここで吸収し、共通の描画には持ち込まない。
//! - `device`: 装置を開き、画面の情報を読み、画面の領域を対応づける。
//! - `present`: 書いた絵を画面へ反映する処理。OSごとに差し替えられる。
//! - `picture`: 確かめやすい、決まった絵。
//! - `fake`: 装置を開かずに描くための、メモリーの上の偽の画面。

pub mod device;
pub mod fake;
pub mod layout;
pub mod picture;
pub mod present;

use std::{fmt, io};

use seinas_render::RenderError;

/// fbdevの裏側の失敗。
#[derive(Debug)]
pub enum FbError {
    /// 装置を開けなかった。
    Open { path: String, source: io::Error },
    /// 画面の情報を読めなかった(`what` は読もうとしたもの)。
    Query {
        what: &'static str,
        source: io::Error,
    },
    /// 画面の領域を対応づけられなかった。
    Map(io::Error),
    /// 1画素のビット数に対応していない(32ビットだけに対応)。
    UnsupportedDepth { bits_per_pixel: u32 },
    /// 色の並びに対応していない(赤・緑・青がそれぞれ8ビットで、バイトの境目にそろっているものだけに対応)。
    UnsupportedChannel {
        channel: &'static str,
        offset: u32,
        length: u32,
    },
    /// 2つの色が同じ位置を使っている。
    OverlappingChannels,
    /// 画面の大きさが0、または大きすぎる。
    InvalidSize { width: u32, height: u32 },
    /// 1行のバイト数が、幅×4より小さい。
    LineTooShort { line_length: u32, needed: u32 },
    /// 画面の領域が、見えている範囲を収めるには小さい。
    ScreenTooSmall { len: usize, needed: usize },
    /// 描画結果の大きさが、画面と合わない。
    SizeMismatch {
        frame: (usize, usize),
        screen: (usize, usize),
    },
    /// 画面への反映に失敗した。
    Present(io::Error),
    /// 共通の描画が失敗した。
    Render(RenderError),
}

impl fmt::Display for FbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FbError::Open { path, source } => write!(f, "cannot open {path}: {source}"),
            FbError::Query { what, source } => write!(f, "cannot read {what}: {source}"),
            FbError::Map(e) => write!(f, "cannot map the screen: {e}"),
            FbError::UnsupportedDepth { bits_per_pixel } => {
                write!(f, "unsupported depth: {bits_per_pixel} bits per pixel (only 32 is supported)")
            }
            FbError::UnsupportedChannel {
                channel,
                offset,
                length,
            } => write!(
                f,
                "unsupported {channel} channel: offset {offset}, length {length} (only 8 bits on a byte boundary is supported)"
            ),
            FbError::OverlappingChannels => write!(f, "two color channels share the same position"),
            FbError::InvalidSize { width, height } => write!(f, "invalid screen size {width}x{height}"),
            FbError::LineTooShort { line_length, needed } => {
                write!(f, "the line length {line_length} is shorter than {needed}")
            }
            FbError::ScreenTooSmall { len, needed } => {
                write!(f, "the screen memory ({len} bytes) is smaller than {needed} bytes")
            }
            FbError::SizeMismatch { frame, screen } => write!(
                f,
                "the frame is {}x{} but the screen is {}x{}",
                frame.0, frame.1, screen.0, screen.1
            ),
            FbError::Present(e) => write!(f, "cannot present the frame: {e}"),
            FbError::Render(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FbError {}

impl From<RenderError> for FbError {
    fn from(e: RenderError) -> Self {
        FbError::Render(e)
    }
}
