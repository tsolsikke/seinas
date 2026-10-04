//! 画面の形式と、描画結果をその形式に直して写す処理。
//!
//! 共通の描画が返すのは `Xrgb8888`(メモリー上でB, G, R, Xの順)の1種類だけである。
//! 画面の側の違い(色の位置、1行のバイト数、見えている範囲の位置)は、ここで吸収する。

use seinas_render::{FrameView, PixelFormat};

use crate::FbError;

/// 1つの色が、1画素の中で占める位置(Linuxの `fb_bitfield` と同じ意味)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bitfield {
    /// 下から数えた、始まりのビット。
    pub offset: u32,
    /// ビットの数。0なら、その色は無い。
    pub length: u32,
}

/// 画面の情報のうち、絵を写すのに要るもの。装置から読んだ値を、そのまま入れる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenInfo {
    /// 見えている範囲の幅と高さ(画素)。
    pub width: u32,
    pub height: u32,
    /// 見えている範囲の左上が、画面の領域のどこにあるか(画素)。
    pub x_offset: u32,
    pub y_offset: u32,
    /// 1画素のビット数。
    pub bits_per_pixel: u32,
    /// 1行のバイト数。
    pub line_length: u32,
    pub red: Bitfield,
    pub green: Bitfield,
    pub blue: Bitfield,
    /// 透明度。無い画面では長さが0。
    pub transp: Bitfield,
}

/// 確かめ終えた画面の形式。ここまで来たものは、必ず写せる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    width: usize,
    height: usize,
    line_length: usize,
    /// 見えている範囲の左上の画素が、画面の領域の何バイト目から始まるか。
    start: usize,
    /// 1画素(4バイト)の中での、赤・緑・青のバイトの位置。
    red: usize,
    green: usize,
    blue: usize,
    /// 透明度のバイトの位置(ある画面だけ)。不透明(0xff)を書く。
    alpha: Option<usize>,
}

const BYTES_PER_PIXEL: usize = 4;

impl Layout {
    /// 画面の情報を確かめて、写し方を決める。対応しない形式は、名前のある失敗で断る。
    pub fn new(info: &ScreenInfo) -> Result<Self, FbError> {
        if info.bits_per_pixel != 32 {
            return Err(FbError::UnsupportedDepth {
                bits_per_pixel: info.bits_per_pixel,
            });
        }
        let invalid_size = FbError::InvalidSize {
            width: info.width,
            height: info.height,
        };
        if info.width == 0 || info.height == 0 {
            return Err(invalid_size);
        }
        // 共通の描画は、幅×高さ×4がi32に収まる大きさまでしか描けない。
        let fits = (info.width as u64) * (info.height as u64) * (BYTES_PER_PIXEL as u64)
            <= i32::MAX as u64;
        if !fits {
            return Err(invalid_size);
        }

        let red = byte_position("red", info.red)?;
        let green = byte_position("green", info.green)?;
        let blue = byte_position("blue", info.blue)?;
        let alpha = match info.transp.length {
            0 => None,
            _ => Some(byte_position("transparency", info.transp)?),
        };
        let mut used = [false; BYTES_PER_PIXEL];
        for position in [Some(red), Some(green), Some(blue), alpha]
            .into_iter()
            .flatten()
        {
            if std::mem::replace(&mut used[position], true) {
                return Err(FbError::OverlappingChannels);
            }
        }

        let needed = info.width * BYTES_PER_PIXEL as u32;
        if info.line_length < needed {
            return Err(FbError::LineTooShort {
                line_length: info.line_length,
                needed,
            });
        }
        let line_length = info.line_length as usize;
        let start = info.y_offset as usize * line_length + info.x_offset as usize * BYTES_PER_PIXEL;
        Ok(Layout {
            width: info.width as usize,
            height: info.height as usize,
            line_length,
            start,
            red,
            green,
            blue,
            alpha,
        })
    }

    /// 見えている範囲の幅(画素)。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 見えている範囲の高さ(画素)。
    pub fn height(&self) -> usize {
        self.height
    }

    /// 見えている範囲を収めるのに要る、画面の領域のバイト数。
    pub fn required_len(&self) -> usize {
        self.start + self.line_length * (self.height - 1) + self.width * BYTES_PER_PIXEL
    }

    /// 描画結果を、この形式に直して `screen`(画面の領域)へ写す。
    ///
    /// 行の余り(幅×4より後ろ)と、見えている範囲の外には触れない。
    pub fn blit(&self, view: &FrameView<'_>, screen: &mut [u8]) -> Result<(), FbError> {
        if (view.width(), view.height()) != (self.width, self.height) {
            return Err(FbError::SizeMismatch {
                frame: (view.width(), view.height()),
                screen: (self.width, self.height),
            });
        }
        if screen.len() < self.required_len() {
            return Err(FbError::ScreenTooSmall {
                len: screen.len(),
                needed: self.required_len(),
            });
        }
        // 共通の描画の形式は1種類だけ。増えたら、ここで分ける。
        let PixelFormat::Xrgb8888 = view.format();
        // 描画結果のメモリー上の並びは B, G, R, X。
        let same_order = (self.blue, self.green, self.red) == (0, 1, 2) && self.alpha.is_none();
        let row_bytes = self.width * BYTES_PER_PIXEL;
        for y in 0..self.height {
            let at = self.start + y * self.line_length;
            let dst = &mut screen[at..at + row_bytes];
            let src = view.row(y);
            if same_order {
                dst.copy_from_slice(src);
                continue;
            }
            for (out, pixel) in dst
                .chunks_exact_mut(BYTES_PER_PIXEL)
                .zip(src.chunks_exact(BYTES_PER_PIXEL))
            {
                out[self.blue] = pixel[0];
                out[self.green] = pixel[1];
                out[self.red] = pixel[2];
                if let Some(alpha) = self.alpha {
                    out[alpha] = 0xff;
                }
            }
        }
        Ok(())
    }
}

/// 色の位置を、1画素の中のバイトの位置(0〜3)に直す。8ビットで、バイトの境目にそろったものだけを受ける。
fn byte_position(channel: &'static str, field: Bitfield) -> Result<usize, FbError> {
    if field.length != 8 || !field.offset.is_multiple_of(8) || field.offset > 24 {
        return Err(FbError::UnsupportedChannel {
            channel,
            offset: field.offset,
            length: field.length,
        });
    }
    // x86-64はリトルエンディアンなので、下のビットほど前のバイトにある。
    Ok((field.offset / 8) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seinas_render::{solid_rect, Painter, SolidRect};

    const WIDTH: u32 = 8;
    const HEIGHT: u32 = 6;

    fn field(offset: u32) -> Bitfield {
        Bitfield { offset, length: 8 }
    }

    /// Linuxでよくある形式(メモリー上でB, G, R, X)。
    fn xrgb() -> ScreenInfo {
        ScreenInfo {
            width: WIDTH,
            height: HEIGHT,
            x_offset: 0,
            y_offset: 0,
            bits_per_pixel: 32,
            line_length: WIDTH * 4,
            red: field(16),
            green: field(8),
            blue: field(0),
            transp: Bitfield::default(),
        }
    }

    /// 左上が赤、右上が緑、左下が青、ほかは白の絵を描いて、`info` の形式の偽の画面へ写す。
    fn paint_to(info: &ScreenInfo, screen: &mut [u8]) -> Result<(), FbError> {
        let layout = Layout::new(info)?;
        let mut painter = Painter::new(WIDTH as i32, HEIGHT as i32).unwrap();
        let elements: [SolidRect; 4] = [
            solid_rect(0, 0, 1, 1, [1.0, 0.0, 0.0, 1.0]),
            solid_rect(WIDTH as i32 - 1, 0, 1, 1, [0.0, 1.0, 0.0, 1.0]),
            solid_rect(0, HEIGHT as i32 - 1, 1, 1, [0.0, 0.0, 1.0, 1.0]),
            solid_rect(0, 0, WIDTH as i32, HEIGHT as i32, [1.0, 1.0, 1.0, 1.0]),
        ];
        let view = painter.paint(&elements).unwrap();
        layout.blit(&view, screen)
    }

    /// 偽の画面の(x, y)の画素を、(赤, 緑, 青)で読む。
    fn rgb(info: &ScreenInfo, screen: &[u8], x: u32, y: u32) -> [u8; 3] {
        let at = ((info.y_offset + y) * info.line_length + (info.x_offset + x) * 4) as usize;
        let pixel = u32::from_le_bytes(screen[at..at + 4].try_into().unwrap());
        let channel = |f: Bitfield| ((pixel >> f.offset) & 0xff) as u8;
        [channel(info.red), channel(info.green), channel(info.blue)]
    }

    fn assert_picture(info: &ScreenInfo, screen: &[u8]) {
        assert_eq!(rgb(info, screen, 0, 0), [0xff, 0, 0]);
        assert_eq!(rgb(info, screen, WIDTH - 1, 0), [0, 0xff, 0]);
        assert_eq!(rgb(info, screen, 0, HEIGHT - 1), [0, 0, 0xff]);
        assert_eq!(rgb(info, screen, WIDTH - 1, HEIGHT - 1), [0xff, 0xff, 0xff]);
        assert_eq!(rgb(info, screen, 3, 3), [0xff, 0xff, 0xff]);
    }

    #[test]
    fn the_common_format_is_copied_as_is() {
        let info = xrgb();
        let mut screen = vec![0u8; (info.line_length * HEIGHT) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        // 左上の赤は、メモリー上で B, G, R の順に 00 00 ff。
        assert_eq!(screen[..3], [0x00, 0x00, 0xff]);
    }

    #[test]
    fn a_longer_line_keeps_its_padding_untouched() {
        let info = ScreenInfo {
            line_length: WIDTH * 4 + 24,
            ..xrgb()
        };
        let mut screen = vec![0x55u8; (info.line_length * HEIGHT) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        for line in screen.chunks_exact(info.line_length as usize) {
            assert!(line[(WIDTH * 4) as usize..].iter().all(|&b| b == 0x55));
        }
    }

    #[test]
    fn a_different_color_order_is_converted() {
        // メモリー上で R, G, B, X の順の画面。
        let info = ScreenInfo {
            red: field(0),
            green: field(8),
            blue: field(16),
            ..xrgb()
        };
        let mut screen = vec![0u8; (info.line_length * HEIGHT) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        assert_eq!(screen[..3], [0xff, 0x00, 0x00]);
    }

    #[test]
    fn colors_in_the_upper_bytes_are_converted() {
        // メモリー上で X, B, G, R の順の画面(赤がいちばん上のバイト)。
        let info = ScreenInfo {
            red: field(24),
            green: field(16),
            blue: field(8),
            ..xrgb()
        };
        let mut screen = vec![0u8; (info.line_length * HEIGHT) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        assert_eq!(screen[1..4], [0x00, 0x00, 0xff]);
    }

    #[test]
    fn a_transparency_channel_is_made_opaque() {
        let info = ScreenInfo {
            transp: field(24),
            ..xrgb()
        };
        let mut screen = vec![0u8; (info.line_length * HEIGHT) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        assert!(screen.chunks_exact(4).all(|pixel| pixel[3] == 0xff));
    }

    #[test]
    fn the_visible_area_can_start_inside_the_screen_memory() {
        let info = ScreenInfo {
            x_offset: 2,
            y_offset: 3,
            line_length: (WIDTH + 4) * 4,
            ..xrgb()
        };
        let mut screen = vec![0x55u8; (info.line_length * (HEIGHT + 5)) as usize];
        paint_to(&info, &mut screen).unwrap();
        assert_picture(&info, &screen);
        // 見えている範囲より上の行と、左の2画素には触れない。
        let line = info.line_length as usize;
        assert!(screen[..3 * line].iter().all(|&b| b == 0x55));
        assert!(screen[3 * line..3 * line + 8].iter().all(|&b| b == 0x55));
    }

    #[test]
    fn unsupported_formats_are_rejected_by_name() {
        let depth = ScreenInfo {
            bits_per_pixel: 16,
            ..xrgb()
        };
        assert!(matches!(
            Layout::new(&depth),
            Err(FbError::UnsupportedDepth { bits_per_pixel: 16 })
        ));

        let narrow = ScreenInfo {
            green: Bitfield {
                offset: 5,
                length: 6,
            },
            ..xrgb()
        };
        assert!(matches!(
            Layout::new(&narrow),
            Err(FbError::UnsupportedChannel {
                channel: "green",
                ..
            })
        ));

        let overlapping = ScreenInfo {
            red: field(0),
            ..xrgb()
        };
        assert!(matches!(
            Layout::new(&overlapping),
            Err(FbError::OverlappingChannels)
        ));

        let short_line = ScreenInfo {
            line_length: WIDTH * 4 - 1,
            ..xrgb()
        };
        assert!(matches!(
            Layout::new(&short_line),
            Err(FbError::LineTooShort { .. })
        ));

        let empty = ScreenInfo { width: 0, ..xrgb() };
        assert!(matches!(
            Layout::new(&empty),
            Err(FbError::InvalidSize { .. })
        ));
    }

    #[test]
    fn a_small_screen_or_a_wrong_frame_size_is_rejected() {
        let info = xrgb();
        let mut small = vec![0u8; (info.line_length * HEIGHT) as usize - 1];
        assert!(matches!(
            paint_to(&info, &mut small),
            Err(FbError::ScreenTooSmall { .. })
        ));

        let layout = Layout::new(&info).unwrap();
        let mut painter = Painter::new(4, 4).unwrap();
        let view = painter.paint::<SolidRect>(&[]).unwrap();
        let mut screen = vec![0u8; (info.line_length * HEIGHT) as usize];
        assert!(matches!(
            layout.blit(&view, &mut screen),
            Err(FbError::SizeMismatch { .. })
        ));
    }
}
