//! 壁紙の設定の読み方と、壁紙の絵の作り方。Wayland を使わないので、ここだけでテストできる。
//!
//! 決まりの説明は `docs/wallpaper.md` にある。

use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

/// 設定のファイルの名前(置き場は `$XDG_CONFIG_HOME/seinas/`)。
pub const CONFIG_FILE: &str = "wallpaper.conf";
/// 色の既定。コンポジタの背景の色(seinas-render の BACKGROUND を 8 ビットにしたもの)と同じにして、設定が
/// 無いときの見た目を変えない。
pub const DEFAULT_COLOR: [u8; 3] = [0x19, 0x1e, 0x28];
/// 読む画像の大きさ(ピクセルの数)の上限。4096x4096 まで。
///
/// 4K(3840x2160)の画像まで入る大きさにしつつ、読み込んだ絵(1 ピクセル 4 バイトで、上限なら 64 MiB)を、
/// 縮めるまでの間だけメモリーに置いても困らない大きさにした。これより大きな画像は、どうせ画面に合わせて縮める
/// ので、利用者に縮めてもらう。
pub const MAX_IMAGE_PIXELS: u64 = 4096 * 4096;

/// 画像の置き方(Windows の「デスクトップの背景」に倣う)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// 塗りつぶし:縦横の比を保って、画面を全部覆うまで拡大・縮小し、はみ出た所を切る(中央に合わせる)。
    #[default]
    Fill,
    /// ページ幅に合わせる:縦横の比を保って、画像の全体が入るまで拡大・縮小する。余白は色で塗る。
    Fit,
    /// 中央に表示:大きさを変えずに、中央に置く。はみ出た所は切り、余白は色で塗る。
    Center,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "fill" => Some(Mode::Fill),
            "fit" => Some(Mode::Fit),
            "center" => Some(Mode::Center),
            _ => None,
        }
    }
}

/// 壁紙の設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 色(画像が無いとき、画像の余白、画像の透けた所)。
    pub color: [u8; 3],
    /// 画像(PNG)。Noneなら色だけ。
    pub image: Option<PathBuf>,
    pub mode: Mode,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            color: DEFAULT_COLOR,
            image: None,
            mode: Mode::default(),
        }
    }
}

/// 設定の文 `text` を読む。`image` の相対パスは、`base`(設定のファイルの置き場)から見たものにする。
/// 読めない行、知らないキー、読めない値は、飛ばして理由を返す。
pub fn parse_config(text: &str, base: &Path) -> (Config, Vec<seinas_config::Problem>) {
    let (entries, problems) = seinas_config::parse(text);
    from_entries(entries, problems, base)
}

/// 設定のファイル `path` を読む。無ければ既定の設定(色だけ)。
pub fn read_config(path: &Path) -> (Config, Vec<seinas_config::Problem>) {
    let (entries, problems) = seinas_config::read(path);
    let base = path.parent().unwrap_or(Path::new("."));
    from_entries(entries, problems, base)
}

fn from_entries(
    entries: Vec<seinas_config::Entry>,
    mut problems: Vec<seinas_config::Problem>,
    base: &Path,
) -> (Config, Vec<seinas_config::Problem>) {
    let mut config = Config::default();
    for seinas_config::Entry { line, key, value } in entries {
        let problem = |reason: String| seinas_config::Problem::at(line, reason);
        match key.as_str() {
            "color" => match parse_color(&value) {
                Some(color) => config.color = color,
                None => problems.push(problem(format!(
                    "color must be six hexadecimal digits (RRGGBB): {value}"
                ))),
            },
            "image" => config.image = Some(base.join(value)),
            "mode" => match Mode::parse(&value) {
                Some(mode) => config.mode = mode,
                None => problems.push(problem(format!(
                    "mode must be fill, fit or center: {value}"
                ))),
            },
            _ => problems.push(problem(format!("unknown key: {key}"))),
        }
    }
    problems.sort_by_key(|p| p.line);
    (config, problems)
}

/// `RRGGBB`(16 進の 6 桁)を色にする。`#` は設定のファイルではコメントになるので、付けない。
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&text[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// 読み込んだ画像。1 ピクセル 4 バイト(R、G、B、A)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// PNG のファイルを読む。大きさが [`MAX_IMAGE_PIXELS`] を越えるもの、読めないものは、理由を返す。
pub fn load_png(path: &Path) -> Result<Picture, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    // 上限の大きさの絵(1 ピクセル 4 バイト)と、読むための作業の分。
    let limits = png::Limits {
        bytes: MAX_IMAGE_PIXELS as usize * 4 + (1 << 20),
    };
    let mut decoder = png::Decoder::new_with_limits(BufReader::new(file), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(format!(
            "{} is {width}x{height}; images over {MAX_IMAGE_PIXELS} pixels are not used",
            path.display()
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{} is too large", path.display()))?;
    let mut data = vec![0; size];
    let info = reader
        .next_frame(&mut data)
        .map_err(|e| format!("cannot decode {}: {e}", path.display()))?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        // EXPAND で、色の表(パレット)は RGB か RGBA になる。
        png::ColorType::Indexed => return Err(format!("{}: unexpected palette", path.display())),
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in data.chunks(info.line_size).take(height as usize) {
        for pixel in row.chunks(channels).take(width as usize) {
            rgba.extend_from_slice(&match *pixel {
                [g] => [g, g, g, 0xff],
                [g, a] => [g, g, g, a],
                [r, g, b] => [r, g, b, 0xff],
                [r, g, b, a] => [r, g, b, a],
                _ => [0, 0, 0, 0xff],
            });
        }
    }
    Ok(Picture {
        width,
        height,
        rgba,
    })
}

/// 壁紙の絵を作る。幅 `width`・高さ `height` の XRGB8888(リトルエンディアンで B、G、R、X の順)。
///
/// 画像が無ければ色 `color` だけ。画像は置き方 `mode` で置き、余白と透けた所は `color` にする。
pub fn paint(
    picture: Option<&Picture>,
    mode: Mode,
    color: [u8; 3],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let ground = [color[2], color[1], color[0], 0xff];
    let mut pixels = ground.repeat(w * h);
    let Some(picture) = picture.filter(|p| p.width > 0 && p.height > 0) else {
        return pixels;
    };
    if w == 0 || h == 0 {
        return pixels;
    }
    let (pw, ph) = (picture.width as f64, picture.height as f64);
    // 置く大きさ(画像を何ピクセルにするか)。
    let (sw, sh) = match mode {
        Mode::Center => (picture.width, picture.height),
        Mode::Fill | Mode::Fit => {
            let (sx, sy) = (width as f64 / pw, height as f64 / ph);
            let scale = if mode == Mode::Fill {
                sx.max(sy)
            } else {
                sx.min(sy)
            };
            (
                ((pw * scale).round() as u32).max(1),
                ((ph * scale).round() as u32).max(1),
            )
        }
    };
    let scaled = if (sw, sh) == (picture.width, picture.height) {
        picture.clone()
    } else {
        resample(picture, sw, sh)
    };
    // 中央に合わせる。画面からはみ出た所は切る。
    let left = (i64::from(width) - i64::from(sw)) / 2;
    let top = (i64::from(height) - i64::from(sh)) / 2;
    for y in 0..h {
        let sy = y as i64 - top;
        if sy < 0 || sy >= i64::from(sh) {
            continue;
        }
        for x in 0..w {
            let sx = x as i64 - left;
            if sx < 0 || sx >= i64::from(sw) {
                continue;
            }
            let at = (sy as usize * sw as usize + sx as usize) * 4;
            let [r, g, b, a] = [
                scaled.rgba[at],
                scaled.rgba[at + 1],
                scaled.rgba[at + 2],
                scaled.rgba[at + 3],
            ];
            // 透けた所は、色の上に重ねる。
            let over = |fg: u8, bg: u8| {
                ((u32::from(fg) * u32::from(a) + u32::from(bg) * (255 - u32::from(a)) + 127) / 255)
                    as u8
            };
            let out = (y * w + x) * 4;
            pixels[out..out + 4].copy_from_slice(&[
                over(b, color[2]),
                over(g, color[1]),
                over(r, color[0]),
                0xff,
            ]);
        }
    }
    pixels
}

/// 画像を `width` × `height` に拡大・縮小する(縦と横に分けた三角形のフィルター。縮めるときは、フィルターの
/// 幅を縮める割合だけ広げて、間引きのギザギザを抑える)。
pub fn resample(picture: &Picture, width: u32, height: u32) -> Picture {
    let horizontal = resample_axis(
        &picture.rgba,
        picture.width as usize,
        picture.height as usize,
        width as usize,
        true,
    );
    let rgba = resample_axis(
        &horizontal,
        width as usize,
        picture.height as usize,
        height as usize,
        false,
    );
    Picture {
        width,
        height,
        rgba,
    }
}

/// 1 つの向き(`horizontal` なら横)だけ、長さを `to` にする。`width` × `height` の RGBA を受け取る。
fn resample_axis(rgba: &[u8], width: usize, height: usize, to: usize, horizontal: bool) -> Vec<u8> {
    let from = if horizontal { width } else { height };
    let (out_w, out_h) = if horizontal {
        (to, height)
    } else {
        (width, to)
    };
    let mut out = vec![0u8; out_w * out_h * 4];
    let scale = from as f64 / to as f64;
    let support = scale.max(1.0);
    // 出力の各位置の、元の位置ごとの重み(先に求めておく)。
    let weights: Vec<(usize, Vec<f64>)> = (0..to)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale - 0.5;
            let first = ((center - support).floor().max(0.0)) as usize;
            let last = ((center + support).ceil() as usize).min(from - 1);
            let mut list: Vec<f64> = (first..=last)
                .map(|j| (1.0 - (j as f64 - center).abs() / support).max(0.0))
                .collect();
            let sum: f64 = list.iter().sum();
            if sum > 0.0 {
                list.iter_mut().for_each(|w| *w /= sum);
            } else {
                // 端で重みが無いときは、いちばん近いものを使う。
                list = vec![0.0; last + 1 - first];
                let nearest = (center.round().max(0.0) as usize).clamp(first, last);
                list[nearest - first] = 1.0;
            }
            (first, list)
        })
        .collect();
    let index = |x: usize, y: usize, w: usize| (y * w + x) * 4;
    for y in 0..out_h {
        for x in 0..out_w {
            let (i, fixed) = if horizontal { (x, y) } else { (y, x) };
            let (first, list) = &weights[i];
            let mut sum = [0.0f64; 4];
            for (k, weight) in list.iter().enumerate() {
                let j = first + k;
                let at = if horizontal {
                    index(j, fixed, width)
                } else {
                    index(fixed, j, width)
                };
                for c in 0..4 {
                    sum[c] += f64::from(rgba[at + c]) * weight;
                }
            }
            let at = index(x, y, out_w);
            for c in 0..4 {
                out[at + c] = sum[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, rgba: [u8; 4]) -> Picture {
        Picture {
            width,
            height,
            rgba: rgba.repeat((width * height) as usize),
        }
    }

    /// (x, y) の色(R、G、B)。
    fn at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i + 2], pixels[i + 1], pixels[i]]
    }

    #[test]
    fn the_config_reads_color_image_and_mode() {
        let (config, problems) = parse_config(
            "color = 102030\nimage = pictures/sky.png\nmode = fit\nmode = tiled\ncolor = #123456\nsize = 3\n",
            Path::new("/home/u/.config/seinas"),
        );
        assert_eq!(
            config,
            Config {
                color: [0x10, 0x20, 0x30],
                image: Some(PathBuf::from("/home/u/.config/seinas/pictures/sky.png")),
                mode: Mode::Fit,
            }
        );
        // `#` から後はコメントなので、`color = #123456` は値の無い行になる。
        assert_eq!(
            problems.iter().map(|p| p.line).collect::<Vec<_>>(),
            [4, 5, 6]
        );
        let (config, _) = parse_config("image = /abs.png\n", Path::new("/x"));
        assert_eq!(config.image, Some(PathBuf::from("/abs.png")));
        assert_eq!(parse_config("", Path::new("/x")).0, Config::default());
    }

    #[test]
    fn without_a_picture_only_the_color_is_painted() {
        let pixels = paint(None, Mode::Fill, DEFAULT_COLOR, 4, 3);
        assert_eq!(pixels.len(), 4 * 3 * 4);
        assert!(pixels.chunks(4).all(|p| p == [0x28, 0x1e, 0x19, 0xff]));
    }

    #[test]
    fn fit_leaves_margins_and_fill_covers() {
        // 横長の画面(8x4)に、正方形の赤(2x2)。
        let red = picture(2, 2, [0xff, 0, 0, 0xff]);
        let fit = paint(Some(&red), Mode::Fit, [0, 0, 0xff], 8, 4);
        assert_eq!(at(&fit, 8, 0, 0), [0, 0, 0xff], "a margin on the left");
        assert_eq!(at(&fit, 8, 4, 2), [0xff, 0, 0]);
        assert_eq!(at(&fit, 8, 7, 3), [0, 0, 0xff], "a margin on the right");
        let fill = paint(Some(&red), Mode::Fill, [0, 0, 0xff], 8, 4);
        assert!(fill.chunks(4).all(|p| p == [0, 0, 0xff, 0xff]));
    }

    #[test]
    fn center_keeps_the_size_and_crops() {
        let red = picture(2, 2, [0xff, 0, 0, 0xff]);
        let small = paint(Some(&red), Mode::Center, [0, 0, 0], 6, 4);
        assert_eq!(at(&small, 6, 2, 1), [0xff, 0, 0]);
        assert_eq!(at(&small, 6, 3, 2), [0xff, 0, 0]);
        assert_eq!(at(&small, 6, 1, 1), [0, 0, 0]);
        assert_eq!(at(&small, 6, 4, 2), [0, 0, 0]);
        // 画面より大きな画像は、中央を切り出す。
        let big = picture(10, 10, [0, 0xff, 0, 0xff]);
        let cropped = paint(Some(&big), Mode::Center, [0, 0, 0], 4, 4);
        assert!(cropped.chunks(4).all(|p| p == [0, 0xff, 0, 0xff]));
    }

    #[test]
    fn transparent_pixels_show_the_color() {
        let clear = picture(1, 1, [0xff, 0xff, 0xff, 0]);
        let half = picture(1, 1, [0xff, 0xff, 0xff, 0x80]);
        assert_eq!(
            at(&paint(Some(&clear), Mode::Center, [0, 0, 0], 1, 1), 1, 0, 0),
            [0, 0, 0]
        );
        assert_eq!(
            at(&paint(Some(&half), Mode::Center, [0, 0, 0], 1, 1), 1, 0, 0),
            [0x80, 0x80, 0x80]
        );
    }

    /// PNG の chunk(長さ、種類、データ、CRC)。
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut crc = 0xffff_ffffu32;
        for &byte in kind.iter().chain(data) {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        [
            &(data.len() as u32).to_be_bytes()[..],
            kind,
            data,
            &(!crc).to_be_bytes(),
        ]
        .concat()
    }

    #[test]
    fn a_too_large_or_broken_image_is_refused() {
        let dir = std::env::temp_dir().join(format!("seinas-wallpaper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 5000x4000(上限の 4096x4096 より多い)の RGB の見出しだけを持つ PNG。画像のデータを読む前に断る。
        let header = [5000u32.to_be_bytes(), 4000u32.to_be_bytes()].concat();
        let ihdr = [&header[..], &[8, 2, 0, 0, 0]].concat();
        let large = [
            &b"\x89PNG\r\n\x1a\n"[..],
            &chunk(b"IHDR", &ihdr),
            &chunk(b"IDAT", &[]),
            &chunk(b"IEND", &[]),
        ]
        .concat();
        let path = dir.join("large.png");
        std::fs::write(&path, large).unwrap();
        let error = load_png(&path).unwrap_err();
        assert!(error.contains("5000x4000"), "{error}");
        // PNG でないファイルと、無いファイル。
        let text = dir.join("text.png");
        std::fs::write(&text, "not a png").unwrap();
        assert!(load_png(&text).is_err());
        assert!(load_png(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_test_image_is_read() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/test-48x32.png");
        let picture = load_png(&path).expect("the test image");
        assert_eq!((picture.width, picture.height), (48, 32));
        // 左上の角は白の枠、右の端の 8 列は半ば透けている。
        assert_eq!(&picture.rgba[..4], &[0xff, 0xff, 0xff, 0xff]);
        assert_eq!(picture.rgba[(10 * 48 + 45) * 4 + 3], 128);
    }

    #[test]
    fn resampling_keeps_a_flat_color_and_averages_when_shrinking() {
        let gray = picture(7, 5, [0x40, 0x80, 0xc0, 0xff]);
        for (w, h) in [(3, 2), (20, 13), (7, 5)] {
            let out = resample(&gray, w, h);
            assert!(
                out.rgba.chunks(4).all(|p| p == [0x40, 0x80, 0xc0, 0xff]),
                "{w}x{h}"
            );
        }
        // 黒と白が 1 列ずつの縞(4x1)を 1x1 に縮めると、灰色になる(間引いて黒か白にならない)。
        let stripes = Picture {
            width: 4,
            height: 1,
            rgba: [[0, 0, 0, 0xff], [0xff, 0xff, 0xff, 0xff]]
                .concat()
                .repeat(2),
        };
        let out = resample(&stripes, 1, 1);
        assert!((0x60..=0xa0).contains(&out.rgba[0]), "{}", out.rgba[0]);
    }
}
