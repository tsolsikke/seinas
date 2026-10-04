//! 偽の画面。装置を開かずに、メモリーの上へ描く。
//!
//! 装置の無い環境(WSLやCIなど)で、描いた結果を確かめるために使う。中身はPPM形式の画像として
//! 書き出せる。

use std::{fs, io, path::Path};

use seinas_render::FrameView;

use crate::{
    layout::{Bitfield, Layout, ScreenInfo},
    FbError,
};

/// メモリーの上の画面。形式は、共通の描画と同じ並び(B, G, R, X)で、行に余りは無い。
pub struct FakeScreen {
    layout: Layout,
    pixels: Vec<u8>,
}

impl FakeScreen {
    /// `width` × `height` 画素の偽の画面を作る。中身は0(黒)で始まる。
    pub fn new(width: u32, height: u32) -> Result<Self, FbError> {
        let info = ScreenInfo {
            width,
            height,
            x_offset: 0,
            y_offset: 0,
            bits_per_pixel: 32,
            line_length: width.saturating_mul(4),
            red: Bitfield {
                offset: 16,
                length: 8,
            },
            green: Bitfield {
                offset: 8,
                length: 8,
            },
            blue: Bitfield {
                offset: 0,
                length: 8,
            },
            transp: Bitfield::default(),
        };
        let layout = Layout::new(&info)?;
        let pixels = vec![0u8; layout.required_len()];
        Ok(FakeScreen { layout, pixels })
    }

    /// 画面の形式。
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// 画面の中身(B, G, R, Xの順で、1行は幅×4バイト)。
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// 描画結果を、偽の画面へ写す。
    pub fn show(&mut self, view: &FrameView<'_>) -> Result<(), FbError> {
        self.layout.blit(view, &mut self.pixels)
    }

    /// 中身を、PPM形式(P6)の画像としてファイルに書く。
    ///
    /// 読む側が書きかけを見ないように、別の名前で書いてから、名前を付け替える。
    pub fn write_ppm(&self, path: &Path) -> io::Result<()> {
        let mut image =
            format!("P6 {} {} 255\n", self.layout.width(), self.layout.height()).into_bytes();
        // PPMは R, G, B の順。
        image.extend(
            self.pixels
                .chunks_exact(4)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]),
        );
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        fs::write(&partial, image)?;
        fs::rename(&partial, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seinas_render::{solid_rect, Painter};

    #[test]
    fn the_fake_screen_receives_the_frame_and_writes_a_ppm() {
        let mut screen = FakeScreen::new(4, 2).unwrap();
        assert!(screen.pixels().iter().all(|&b| b == 0));

        let mut painter = Painter::new(4, 2).unwrap();
        let view = painter
            .paint(&[solid_rect(0, 0, 1, 1, [1.0, 0.0, 0.0, 1.0])])
            .unwrap();
        screen.show(&view).unwrap();
        // 左上は赤(B, G, Rの順で 00 00 ff)。
        assert_eq!(screen.pixels()[..3], [0x00, 0x00, 0xff]);

        let path = std::env::temp_dir().join(format!("seinas-fake-{}.ppm", std::process::id()));
        screen.write_ppm(&path).unwrap();
        let image = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let header = b"P6 4 2 255\n";
        assert_eq!(&image[..header.len()], header);
        assert_eq!(image.len(), header.len() + 4 * 2 * 3);
        // PPMでは R, G, B の順で ff 00 00。
        assert_eq!(image[header.len()..header.len() + 3], [0xff, 0x00, 0x00]);
    }
}
