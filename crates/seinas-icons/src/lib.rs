//! Seinas のアイコン。
//!
//! - アプリのアイコンを探して読む所:app_id → `.desktop` のファイルの `Icon` → アイコンのテーマ(freedesktop の
//!   決まりの hicolor)→ `pixmaps`。読むのは PNG だけ(SVG は読まない)で、データの置き場(XDG のディレクトリの
//!   `icons` と `pixmaps`)のファイルだけ。クライアントから受け取った画像は読まない(ADR 0007)。読む前に、ファイルの大きさと、画像の
//!   幅・高さを確かめる([`MAX_ICON_FILE_BYTES`]、[`MAX_ICON_SIDE`])。見つからない・越える・読めないときは、既定の
//!   アイコン。読んだものは、app_id ごとに覚えて使い回す。
//! - Seinas が自分で描くアイコン:既定のアイコンと、トーチ(炎)。どちらも自作の簡単な絵柄で、外の画像を使わない。
//! - 小さな図形(角の丸い四角、点、四角)を、B, G, R, X の絵に重ねて描く所。
//!
//! 決まりの説明は `docs/ui-spec.md` の「アプリのアイコン」にある。

use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

/// 読むアイコンのファイルの大きさの上限(1 MiB)。アイコンのテーマの PNG は、いちばん大きい 512x512 でも、ふつうは
/// 数百 KB に収まる。これを越えるものは、読み始めない(展開の手間とメモリーを、コンポジタの中で抑えるため)。
pub const MAX_ICON_FILE_BYTES: u64 = 1024 * 1024;
/// 読むアイコンの幅と高さの上限(ピクセル)。hicolor のいちばん大きい大きさ(512x512)まで。これより大きな画像は、
/// どうせ 24 や 20 に縮めるので要らない。展開した絵は、上限でも 1 MiB(1 ピクセル 4 バイト)に収まる。
pub const MAX_ICON_SIDE: u32 = 512;

/// 色(赤, 緑, 青)。
pub type Rgb = [u8; 3];

/// アイコンの絵。1 ピクセルは R, G, B, A の 4 バイトで、色は不透明度を掛けた形(premultiplied)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Icon {
    /// 透明な絵。
    pub fn transparent(width: u32, height: u32) -> Self {
        Icon {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    /// (x, y) のピクセル(R, G, B, A。premultiplied)。
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    /// 幅 `width` の B, G, R, X の絵 `pixels` の、左上 (`x`, `y`) に重ねる。外へ出たぶんは描かない。
    pub fn draw_onto(&self, pixels: &mut [u8], width: i32, x: i32, y: i32) {
        let height = pixels.len() as i32 / 4 / width.max(1);
        for iy in 0..self.height as i32 {
            for ix in 0..self.width as i32 {
                let (px, py) = (x + ix, y + iy);
                if !(0..width).contains(&px) || !(0..height).contains(&py) {
                    continue;
                }
                let [r, g, b, a] = self.pixel(ix as u32, iy as u32);
                if a == 0 {
                    continue;
                }
                let at = ((py * width + px) * 4) as usize;
                let keep = 255 - u32::from(a);
                for (offset, value) in [(2, r), (1, g), (0, b)] {
                    let under = u32::from(pixels[at + offset]);
                    pixels[at + offset] = (u32::from(value) + (under * keep + 127) / 255) as u8;
                }
            }
        }
    }
}

// ---- 図形

/// 1 ピクセルを分けて数える数(縦と横に、それぞれこの数)。
const SAMPLES: u32 = 4;

/// 点 (x, y) が図形の中かを返す `inside` で決まる図形を、色 `color` で、B, G, R, X の絵に重ねる。調べるのは
/// 矩形 (`x0`, `y0`)〜(`x1`, `y1`)(端を含まない)の中だけ。縁は、1 ピクセルを分けて数えた割合で、なめらかにする。
pub fn fill_shape(
    pixels: &mut [u8],
    width: i32,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    color: Rgb,
    inside: impl Fn(f32, f32) -> bool,
) {
    let height = pixels.len() as i32 / 4 / width.max(1);
    for py in y0.max(0)..y1.min(height) {
        for px in x0.max(0)..x1.min(width) {
            let coverage = coverage(px, py, &inside);
            if coverage == 0 {
                continue;
            }
            let at = ((py * width + px) * 4) as usize;
            for (offset, value) in [(2, color[0]), (1, color[1]), (0, color[2])] {
                let under = u32::from(pixels[at + offset]);
                pixels[at + offset] = ((u32::from(value) * coverage
                    + under * (SAMPLES * SAMPLES - coverage)
                    + SAMPLES * SAMPLES / 2)
                    / (SAMPLES * SAMPLES)) as u8;
            }
        }
    }
}

/// ピクセル (px, py) のうち、図形の中にある分けた点の数(0〜SAMPLES²)。
fn coverage(px: i32, py: i32, inside: &impl Fn(f32, f32) -> bool) -> u32 {
    let mut count = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let x = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
            let y = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
            if inside(x, y) {
                count += 1;
            }
        }
    }
    count
}

/// 左上 (`x`, `y`)、大きさ `w` × `h`、角の丸み `radius` の四角が、点 (px, py) を含むか。
pub fn in_rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32, px: f32, py: f32) -> bool {
    if px < x || py < y || px >= x + w || py >= y + h {
        return false;
    }
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let cx = px.clamp(x + r, x + w - r);
    let cy = py.clamp(y + r, y + h - r);
    let (dx, dy) = (px - cx, py - cy);
    dx * dx + dy * dy <= r * r
}

/// 角の丸い四角を塗る。
pub fn fill_rounded_rect(
    pixels: &mut [u8],
    width: i32,
    (x, y, w, h): (i32, i32, i32, i32),
    radius: f32,
    color: Rgb,
) {
    let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
    fill_shape(pixels, width, (x, y, x + w, y + h), color, |px, py| {
        in_rounded_rect(xf, yf, wf, hf, radius, px, py)
    });
}

/// 中心 (`cx`, `cy`)、直径 `diameter` の円を塗る。
pub fn fill_circle(pixels: &mut [u8], width: i32, (cx, cy): (f32, f32), diameter: f32, color: Rgb) {
    let r = diameter / 2.0;
    let bounds = (
        (cx - r).floor() as i32,
        (cy - r).floor() as i32,
        (cx + r).ceil() as i32,
        (cy + r).ceil() as i32,
    );
    fill_shape(pixels, width, bounds, color, |px, py| {
        let (dx, dy) = (px - cx, py - cy);
        dx * dx + dy * dy <= r * r
    });
}

/// 四角を塗る(ピクセルの境に合った矩形。外へ出たぶんは描かない)。
pub fn fill_rect(pixels: &mut [u8], width: i32, (x, y, w, h): (i32, i32, i32, i32), color: Rgb) {
    let height = pixels.len() as i32 / 4 / width.max(1);
    for row in y.max(0)..(y + h).min(height) {
        for column in x.max(0)..(x + w).min(width) {
            let at = ((row * width + column) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&[color[2], color[1], color[0], 0xff]);
        }
    }
}

/// 図形(点 (x, y) が中かを返すもの)。
type Shape<'a> = &'a dyn Fn(f32, f32) -> bool;

/// 大きさ `size` の透明な絵に、図形を順に重ねた、アイコンの絵を作る(`layers` は、奥から順の、色と図形)。
fn paint_icon(size: u32, layers: &[(Rgb, Shape)]) -> Icon {
    let mut icon = Icon::transparent(size, size);
    for py in 0..size {
        for px in 0..size {
            let at = ((py * size + px) * 4) as usize;
            for &(color, inside) in layers {
                let a = coverage(px as i32, py as i32, &inside) * 255 / (SAMPLES * SAMPLES);
                if a == 0 {
                    continue;
                }
                let keep = 255 - a;
                for (offset, value) in color.iter().enumerate() {
                    let under = u32::from(icon.rgba[at + offset]);
                    icon.rgba[at + offset] =
                        ((u32::from(*value) * a + 127) / 255 + (under * keep + 127) / 255) as u8;
                }
                let under = u32::from(icon.rgba[at + 3]);
                icon.rgba[at + 3] = (a + (under * keep + 127) / 255) as u8;
            }
        }
    }
    icon
}

// ---- 自分で描くアイコン

/// トーチの外の炎と内の炎の色。外はアクセントの色、内は明るい黄。
pub const TORCH_OUTER: Rgb = [0xf0, 0x93, 0x11];
pub const TORCH_INNER: Rgb = [0xff, 0xd2, 0x7a];

/// 炎の形(しずくの形)の中か。`(u, v)` は、炎の矩形の中の位置で、u は横(-1〜1)、v は縦(上の端の先が 1、下の端が -1)。
fn in_flame(u: f32, v: f32) -> bool {
    if !(-1.0..=1.0).contains(&v) {
        return false;
    }
    // しずくの曲線:x = sin t · sin(t/2)^1.5、y = cos t。いちばん太い所(約 0.84)で 1 になるように割る。
    let t = v.acos();
    let half = t.sin() * (t / 2.0).sin().powf(1.5) / 0.84;
    u.abs() <= half
}

/// トーチのアイコン(大きさ `size` の正方形)。壁紙の火に合わせた、簡単な炎の絵柄(自作)。最終形は未決。
pub fn torch(size: u32) -> Icon {
    let s = size as f32;
    // 炎の矩形(左上と大きさ)から、炎の形の中かを決める。
    let flame = |left: f32, top: f32, w: f32, h: f32| {
        move |x: f32, y: f32| {
            let u = (x - (left + w / 2.0)) / (w / 2.0);
            let v = 1.0 - 2.0 * (y - top) / h;
            in_flame(u, v)
        }
    };
    let outer = flame(s * 0.18, s * 0.04, s * 0.64, s * 0.92);
    let inner = flame(s * 0.33, s * 0.42, s * 0.34, s * 0.52);
    paint_icon(size, &[(TORCH_OUTER, &outer), (TORCH_INNER, &inner)])
}

/// 既定のアイコンの色(外の枠と、上の端のタイトルバーの形の横の帯。中)。
pub const DEFAULT_ICON_FRAME: Rgb = [0x81, 0x90, 0xab];
pub const DEFAULT_ICON_BODY: Rgb = [0x22, 0x30, 0x4a];

/// アイコンが見つからないアプリの、既定のアイコン(大きさ `size` の正方形)。角の丸い四角の上の端に、タイトルバーの形の横の帯を
/// 付けた、ウィンドウの形の絵柄(自作)。
pub fn default_icon(size: u32) -> Icon {
    let s = size as f32;
    let (x, y, w, h) = (s * 0.08, s * 0.14, s * 0.84, s * 0.72);
    let radius = s * 0.14;
    let frame = move |px: f32, py: f32| in_rounded_rect(x, y, w, h, radius, px, py);
    let inset = (s / 16.0).max(1.0);
    let band = s * 0.16;
    let body = move |px: f32, py: f32| {
        in_rounded_rect(
            x + inset,
            y + band,
            w - 2.0 * inset,
            h - band - inset,
            (radius - inset).max(0.0),
            px,
            py,
        )
    };
    paint_icon(
        size,
        &[(DEFAULT_ICON_FRAME, &frame), (DEFAULT_ICON_BODY, &body)],
    )
}

// ---- アプリのアイコンを探す

/// データの置き場(XDG Base Directory の決まり)。`$XDG_DATA_HOME`(無ければ `$HOME/.local/share`)と、
/// `$XDG_DATA_DIRS` の各々(無ければ `/usr/local/share` と `/usr/share`)。相対パスは使わない。空の値は、無いもの
/// として渡すこと。
pub fn data_dirs(
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
    xdg_data_dirs: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let data_home = xdg_data_home
        .filter(|dir| dir.is_absolute())
        .map(Path::to_owned)
        .or_else(|| {
            home.filter(|dir| dir.is_absolute())
                .map(|home| home.join(".local/share"))
        });
    dirs.extend(data_home);
    let data_dirs: Vec<PathBuf> = xdg_data_dirs
        .map(|dirs| {
            std::env::split_paths(dirs)
                .filter(|dir| dir.is_absolute())
                .collect()
        })
        .filter(|dirs: &Vec<PathBuf>| !dirs.is_empty())
        .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
    dirs.extend(data_dirs);
    dirs
}

/// このプロセスの環境変数(`XDG_DATA_HOME`・`HOME`・`XDG_DATA_DIRS`)から決めた、データの置き場。
pub fn data_dirs_from_env() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let (data_home, home, dirs) = (var("XDG_DATA_HOME"), var("HOME"), var("XDG_DATA_DIRS"));
    data_dirs(
        data_home.as_deref().map(Path::new),
        home.as_deref().map(Path::new),
        dirs.as_deref(),
    )
}

/// `.desktop` のファイルの文 `text` から、`[Desktop Entry]` の `Icon` の値を取り出す(国ごとの値は見ない)。
pub fn parse_desktop_icon(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "Icon" {
                let value = value.trim();
                return (!value.is_empty()).then(|| value.to_owned());
            }
        }
    }
    None
}

/// app_id の `.desktop` のファイル(データの置き場の各々の `applications/<app_id>.desktop`。無ければ、小文字に
/// した名前)の `Icon` の値。
pub fn desktop_icon(app_id: &str, dirs: &[PathBuf]) -> Option<String> {
    let names = [app_id.to_owned(), app_id.to_lowercase()];
    for dir in dirs {
        for name in &names {
            let path = dir.join("applications").join(format!("{name}.desktop"));
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(icon) = parse_desktop_icon(&text) {
                    return Some(icon);
                }
            }
        }
    }
    None
}

/// アイコンのテーマの中で探す大きさの順:欲しい大きさ、それより大きいもの(小さい順)、小さいもの(大きい順)。
fn sizes_for(size: u32) -> Vec<u32> {
    let known = [16, 22, 24, 32, 48, 64, 96, 128, 256, 512];
    let mut sizes = vec![size];
    sizes.extend(known.iter().copied().filter(|&s| s > size));
    sizes.extend(known.iter().rev().copied().filter(|&s| s < size));
    sizes
}

/// アイコンの名前 `name` の PNG を、アイコンのテーマ(hicolor)と `pixmaps` から探す。探す所は、データの置き場の
/// 各々の `icons`(テーマ)と `pixmaps` だけ。絶対パスの名前は、データの置き場の中のファイルのときだけ使う。
pub fn find_icon_file(name: &str, dirs: &[PathBuf], size: u32) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        let is_png = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        // データの置き場の外のファイルは読まない。
        let inside = |root: &Path| {
            let (Ok(file), Ok(root)) = (path.canonicalize(), root.canonicalize()) else {
                return false;
            };
            file.starts_with(root)
        };
        let allowed = dirs.iter().any(|dir| inside(dir));
        return (is_png && allowed && path.is_file()).then(|| path.to_owned());
    }
    let name = name.strip_suffix(".png").unwrap_or(name);
    let themes: Vec<PathBuf> = dirs.iter().map(|dir| dir.join("icons")).collect();
    for size in sizes_for(size) {
        for base in &themes {
            let file = base
                .join("hicolor")
                .join(format!("{size}x{size}"))
                .join("apps")
                .join(format!("{name}.png"));
            if file.is_file() {
                return Some(file);
            }
        }
    }
    dirs.iter()
        .map(|dir| dir.join("pixmaps").join(format!("{name}.png")))
        .find(|file| file.is_file())
}

/// app_id のアイコンの PNG を探す。`.desktop` の `Icon` の名前で探し、無ければ app_id そのものを名前にして探す。
pub fn find_app_icon(app_id: &str, dirs: &[PathBuf], size: u32) -> Option<PathBuf> {
    if app_id.trim().is_empty() {
        return None;
    }
    let mut names: Vec<String> = desktop_icon(app_id, dirs).into_iter().collect();
    names.push(app_id.to_owned());
    names
        .iter()
        .find_map(|name| find_icon_file(name, dirs, size))
}

/// PNG のファイルを読み、大きさ `size` の正方形に収める(縦横の比を保ち、中央に置く)。ファイルの大きさが
/// [`MAX_ICON_FILE_BYTES`] を越えるもの、幅か高さが [`MAX_ICON_SIDE`] を越えるもの(画像のデータを読む前に断る)、
/// 読めないものは、理由を返す。
pub fn load_icon(path: &Path, size: u32) -> Result<Icon, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let bytes = file
        .metadata()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .len();
    if bytes > MAX_ICON_FILE_BYTES {
        return Err(format!(
            "{} is {bytes} bytes; icon files over {MAX_ICON_FILE_BYTES} bytes are not read",
            path.display()
        ));
    }
    let limits = png::Limits {
        bytes: (MAX_ICON_SIDE * MAX_ICON_SIDE * 4) as usize + (1 << 20),
    };
    let mut decoder = png::Decoder::new_with_limits(BufReader::new(file), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > MAX_ICON_SIDE || height > MAX_ICON_SIDE {
        return Err(format!(
            "{} is {width}x{height}; icons from 1x1 to {MAX_ICON_SIDE}x{MAX_ICON_SIDE} are read",
            path.display()
        ));
    }
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{} is too large", path.display()))?;
    let mut data = vec![0; buffer_size];
    let info = reader
        .next_frame(&mut data)
        .map_err(|e| format!("cannot decode {}: {e}", path.display()))?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(format!("{}: unexpected palette", path.display())),
    };
    // premultiplied の R, G, B, A にする。
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in data.chunks(info.line_size).take(height as usize) {
        for pixel in row.chunks(channels).take(width as usize) {
            let [r, g, b, a] = match *pixel {
                [v] => [v, v, v, 0xff],
                [v, a] => [v, v, v, a],
                [r, g, b] => [r, g, b, 0xff],
                [r, g, b, a] => [r, g, b, a],
                _ => [0, 0, 0, 0xff],
            };
            let mul = |v: u8| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
            rgba.extend_from_slice(&[mul(r), mul(g), mul(b), a]);
        }
    }
    let source = Icon {
        width,
        height,
        rgba,
    };
    Ok(fit(&source, size))
}

/// 絵 `source` を、大きさ `size` の正方形に収める(縦横の比を保ち、中央に置く)。
pub fn fit(source: &Icon, size: u32) -> Icon {
    let scale = (size as f32 / source.width as f32).min(size as f32 / source.height as f32);
    let w = ((source.width as f32 * scale).round() as u32).clamp(1, size);
    let h = ((source.height as f32 * scale).round() as u32).clamp(1, size);
    let resized = resample(source, w, h);
    let mut icon = Icon::transparent(size, size);
    let (ox, oy) = ((size - w) / 2, (size - h) / 2);
    for y in 0..h {
        for x in 0..w {
            let from = ((y * w + x) * 4) as usize;
            let to = (((y + oy) * size + x + ox) * 4) as usize;
            icon.rgba[to..to + 4].copy_from_slice(&resized.rgba[from..from + 4]);
        }
    }
    icon
}

/// 絵の大きさを `width` × `height` にする。どのピクセルも、元の絵と重なる面積の割合で混ぜる(縮めるときになめらか
/// で、大きくするときは近いピクセルの色になる)。
pub fn resample(source: &Icon, width: u32, height: u32) -> Icon {
    let (sx, sy) = (
        source.width as f32 / width as f32,
        source.height as f32 / height as f32,
    );
    let mut out = Icon::transparent(width, height);
    for y in 0..height {
        let (y0, y1) = (y as f32 * sy, (y + 1) as f32 * sy);
        for x in 0..width {
            let (x0, x1) = (x as f32 * sx, (x + 1) as f32 * sx);
            let mut sum = [0f32; 4];
            let mut total = 0.0;
            for iy in y0.floor() as u32..(y1.ceil() as u32).min(source.height) {
                let wy = (y1.min(iy as f32 + 1.0) - y0.max(iy as f32)).max(0.0);
                for ix in x0.floor() as u32..(x1.ceil() as u32).min(source.width) {
                    let wx = (x1.min(ix as f32 + 1.0) - x0.max(ix as f32)).max(0.0);
                    let weight = wx * wy;
                    let pixel = source.pixel(ix, iy);
                    for (s, v) in sum.iter_mut().zip(pixel) {
                        *s += f32::from(v) * weight;
                    }
                    total += weight;
                }
            }
            let at = ((y * width + x) * 4) as usize;
            for (offset, s) in sum.iter().enumerate() {
                out.rgba[at + offset] =
                    (s / total.max(f32::EPSILON)).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// app_id ごとのアイコン(大きさは 1 つ)を、1 回だけ探して読んで覚えておく所。
pub struct IconCache {
    size: u32,
    dirs: Vec<PathBuf>,
    icons: HashMap<String, Icon>,
    default: Icon,
}

/// アイコンを探した結果(ログに出すため)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// その PNG を読んだ。
    File(PathBuf),
    /// その PNG を見つけたが、上限を越えたか読めなかった(理由)。既定のアイコンを使う。
    Unreadable(PathBuf, String),
    /// 見つからなかった。既定のアイコンを使う。
    Missing,
}

impl IconCache {
    /// 大きさ `size` のアイコンを、データの置き場 `dirs` から探す。
    pub fn new(size: u32, dirs: Vec<PathBuf>) -> Self {
        IconCache {
            size,
            dirs,
            icons: HashMap::new(),
            default: default_icon(size),
        }
    }

    /// 環境変数から決めた置き場で探す。
    pub fn from_env(size: u32) -> Self {
        IconCache::new(size, data_dirs_from_env())
    }

    /// 既定のアイコン。
    pub fn default_icon(&self) -> &Icon {
        &self.default
    }

    /// app_id のアイコンを、まだ探していなければ探して覚える。初めて探したときは、その結果を返す(ログに出すため)。
    pub fn load(&mut self, app_id: &str) -> Option<Found> {
        if app_id.trim().is_empty() || self.icons.contains_key(app_id) {
            return None;
        }
        let (icon, found) = match find_app_icon(app_id, &self.dirs, self.size) {
            Some(path) => match load_icon(&path, self.size) {
                Ok(icon) => (icon, Found::File(path)),
                Err(e) => (self.default.clone(), Found::Unreadable(path, e)),
            },
            None => (self.default.clone(), Found::Missing),
        };
        self.icons.insert(app_id.to_owned(), icon);
        Some(found)
    }

    /// app_id のアイコン(探していなければ、既定のアイコン)。
    pub fn get(&self, app_id: &str) -> &Icon {
        self.icons.get(app_id).unwrap_or(&self.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のアイコンの置き場(`tools/make-test-icons.py` が作る)。
    fn test_data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/share")
    }

    #[test]
    fn the_data_directories_follow_the_xdg_rules() {
        assert_eq!(
            data_dirs(
                Some(Path::new("/d")),
                Some(Path::new("/h")),
                Some(OsStr::new("/a:/b"))
            ),
            ["/d", "/a", "/b"].map(PathBuf::from)
        );
        assert_eq!(
            data_dirs(None, Some(Path::new("/h")), None),
            ["/h/.local/share", "/usr/local/share", "/usr/share"].map(PathBuf::from)
        );
        // 相対パスは使わない。
        assert_eq!(
            data_dirs(Some(Path::new("d")), None, Some(OsStr::new("a"))),
            ["/usr/local/share", "/usr/share"].map(PathBuf::from)
        );
    }

    #[test]
    fn the_icon_key_of_the_desktop_entry_is_read() {
        let text = "[Desktop Entry]\nName=Files\nIcon[ja]=x\nIcon = folder \n[Desktop Action new]\nIcon=y\n";
        assert_eq!(parse_desktop_icon(text), Some("folder".to_owned()));
        assert_eq!(parse_desktop_icon("[Other]\nIcon=x\n"), None);
        assert_eq!(parse_desktop_icon("[Desktop Entry]\nIcon=\n"), None);
    }

    #[test]
    fn an_app_icon_is_found_through_the_desktop_entry_and_the_theme() {
        let dirs = vec![PathBuf::from("/nowhere"), test_data()];
        // .desktop の Icon(seinas-test-folder)の 24x24。
        assert_eq!(
            find_app_icon("seinas.test.files", &dirs, 24),
            Some(test_data().join("icons/hicolor/24x24/apps/seinas-test-folder.png"))
        );
        // .desktop が無くても、app_id の名前のアイコンがあれば、それ。24 が無ければ、大きいもの(48)。
        assert_eq!(
            find_app_icon("seinas.test.large", &dirs, 24),
            Some(test_data().join("icons/hicolor/48x48/apps/seinas.test.large.png"))
        );
        // テーマに無ければ pixmaps。
        assert_eq!(
            find_app_icon("seinas.test.pixmap", &dirs, 24),
            Some(test_data().join("pixmaps/seinas.test.pixmap.png"))
        );
        assert_eq!(find_app_icon("seinas.test.missing", &dirs, 24), None);
        assert_eq!(find_app_icon("", &dirs, 24), None);
    }

    #[test]
    fn icons_are_loaded_fitted_and_cached() {
        let dirs = vec![test_data()];
        let mut cache = IconCache::new(24, dirs);
        // 48x48 の緑の丸は、24x24 に縮める(中ほどは緑)。
        let large = test_data().join("icons/hicolor/48x48/apps/seinas.test.large.png");
        assert_eq!(cache.load("seinas.test.large"), Some(Found::File(large)));
        let icon = cache.get("seinas.test.large");
        assert_eq!(icon.width, 24);
        assert_eq!(icon.pixel(12, 12), [0x5c, 0xb8, 0x7a, 0xff]);
        // 2 回目は探さない。
        assert_eq!(cache.load("seinas.test.large"), None);
        assert_eq!(cache.load("seinas.test.missing"), Some(Found::Missing));
        assert_eq!(cache.get("seinas.test.missing"), cache.default_icon());
        assert_eq!(cache.get("never.asked"), cache.default_icon());
    }

    /// 一時の置き場(データの置き場の形)。
    fn temporary_share(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("seinas-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("icons/hicolor/24x24/apps")).unwrap();
        std::fs::create_dir_all(dir.join("applications")).unwrap();
        dir
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
    fn too_large_or_broken_icons_get_the_default_with_a_reason() {
        let share = temporary_share("limits");
        let apps = share.join("icons/hicolor/24x24/apps");
        // 513x512 の見出しだけを持つ PNG(幅が上限を越える。画像のデータを読む前に断る)。
        let header = [513u32.to_be_bytes(), 512u32.to_be_bytes()].concat();
        let wide = [
            &b"\x89PNG\r\n\x1a\n"[..],
            &chunk(b"IHDR", &[&header[..], &[8, 6, 0, 0, 0]].concat()),
            &chunk(b"IDAT", &[]),
            &chunk(b"IEND", &[]),
        ]
        .concat();
        std::fs::write(apps.join("seinas.test.wide.png"), wide).unwrap();
        // 1 MiB を越えるファイル(中身は読まない)。
        std::fs::write(
            apps.join("seinas.test.heavy.png"),
            vec![0u8; MAX_ICON_FILE_BYTES as usize + 1],
        )
        .unwrap();
        // PNG でないファイル。
        std::fs::write(apps.join("seinas.test.broken.png"), "not a png").unwrap();
        let mut cache = IconCache::new(24, vec![share.clone()]);
        for (app_id, words) in [
            ("seinas.test.wide", "513x512"),
            ("seinas.test.heavy", "1048577 bytes"),
            ("seinas.test.broken", "cannot read"),
        ] {
            match cache.load(app_id) {
                Some(Found::Unreadable(_, reason)) => assert!(reason.contains(words), "{reason}"),
                other => panic!("{app_id}: {other:?}"),
            }
            assert_eq!(cache.get(app_id), cache.default_icon());
        }
        std::fs::remove_dir_all(&share).unwrap();
    }

    #[test]
    fn an_absolute_icon_path_is_read_only_inside_the_data_directories() {
        let share = temporary_share("absolute");
        let outside = temporary_share("outside");
        let inside_icon = share.join("icons/hicolor/24x24/apps/inside.png");
        let outside_icon = outside.join("outside.png");
        for path in [&inside_icon, &outside_icon] {
            std::fs::copy(
                test_data().join("icons/hicolor/24x24/apps/seinas-test-folder.png"),
                path,
            )
            .unwrap();
        }
        for (app_id, icon) in [
            ("seinas.test.in", &inside_icon),
            ("seinas.test.out", &outside_icon),
        ] {
            std::fs::write(
                share.join(format!("applications/{app_id}.desktop")),
                format!("[Desktop Entry]\nIcon={}\n", icon.display()),
            )
            .unwrap();
        }
        let dirs = vec![share.clone()];
        assert_eq!(
            find_app_icon("seinas.test.in", &dirs, 24),
            Some(inside_icon)
        );
        // 置き場の外のファイルは、.desktop に書いてあっても読まない。
        assert_eq!(find_app_icon("seinas.test.out", &dirs, 24), None);
        std::fs::remove_dir_all(&share).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn a_wide_picture_is_centered_in_the_square() {
        // 4x2 の不透明な白を 4x4 に収めると、上下 1 ずつが透明。
        let wide = Icon {
            width: 4,
            height: 2,
            rgba: [255u8; 4].repeat(8),
        };
        let fitted = fit(&wide, 4);
        assert_eq!(fitted.pixel(0, 0)[3], 0);
        assert_eq!(fitted.pixel(0, 1), [255, 255, 255, 255]);
        assert_eq!(fitted.pixel(3, 2), [255, 255, 255, 255]);
        assert_eq!(fitted.pixel(0, 3)[3], 0);
        // 半分に縮めると、2x2 の白と黒は灰色。
        let checker = Icon {
            width: 2,
            height: 2,
            rgba: [[0, 0, 0, 255], [255, 255, 255, 255]].concat().repeat(2),
        };
        assert_eq!(resample(&checker, 1, 1).pixel(0, 0), [128, 128, 128, 255]);
    }

    #[test]
    fn the_drawn_icons_have_their_colors() {
        let torch = torch(24);
        // 炎の中ほどは不透明で、上の角と下の角は透明。
        assert_eq!(torch.pixel(12, 12)[3], 255);
        assert_eq!(torch.pixel(0, 0)[3], 0);
        assert_eq!(torch.pixel(23, 23)[3], 0);
        // 内の炎は明るい。
        assert!(torch.pixel(12, 18)[1] > torch.pixel(12, 6)[1]);
        let icon = default_icon(24);
        assert_eq!(&icon.pixel(12, 4)[..3], &DEFAULT_ICON_FRAME);
        assert_eq!(&icon.pixel(12, 12)[..3], &DEFAULT_ICON_BODY);
        assert_eq!(icon.pixel(0, 0)[3], 0);
    }

    #[test]
    fn shapes_are_blended_onto_the_picture() {
        // 4x4 の黒に、白の円(径 4、中心 (2, 2))を描くと、中央は白、角はにじむ。
        let mut pixels = [0u8, 0, 0, 0xff].repeat(16);
        fill_circle(&mut pixels, 4, (2.0, 2.0), 4.0, [255, 255, 255]);
        assert_eq!(&pixels[(4 + 1) * 4..(4 + 1) * 4 + 3], &[255, 255, 255]);
        assert!(pixels[0] > 0 && pixels[0] < 255);
        // アイコンを重ねる:半ば透けた赤(premultiplied で 128, 0, 0, 128)を黒の上に。
        let red = Icon {
            width: 1,
            height: 1,
            rgba: vec![128, 0, 0, 128],
        };
        let mut pixels = [0u8, 0, 0, 0xff].to_vec();
        red.draw_onto(&mut pixels, 1, 0, 0);
        assert_eq!(&pixels[..3], &[0, 0, 128]);
    }
}
