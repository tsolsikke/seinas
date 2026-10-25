//! Seinas のアイコン。
//!
//! - アプリのアイコンを探す所:app_id → `.desktop` のファイルの `Icon` → アイコンのテーマ(freedesktop の
//!   決まりの hicolor)→ `pixmaps`。探すのは PNG だけ(SVG は探さない)。
//!   - PNG を読むクレートは、壁紙のほかに入れない決まり(CI と pre-push の check-deps)なので、今は見つけても読まず、
//!     既定のアイコンを出す。読むかどうかは、運用者が決める(`docs/ui-spec.md` の「アプリのアイコン」)。
//! - Seinas が自分で描くアイコン:既定のアイコンと、トーチ(炎)。どちらも自作の簡単な絵柄で、外の画像を使わない。
//! - 小さな図形(角の丸い四角、点、四角)を、B, G, R, X の絵に重ねて描く所。
//!
//! 決まりの説明は `docs/ui-spec.md` の「アプリのアイコン」にある。

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

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

/// アイコンの名前 `name` の PNG を、アイコンのテーマ(hicolor)と `pixmaps` から探す。探す所は、`$HOME/.icons` と、
/// データの置き場の各々の `icons`(テーマ)と `pixmaps`。
pub fn find_icon_file(
    name: &str,
    dirs: &[PathBuf],
    home: Option<&Path>,
    size: u32,
) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        let is_png = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        return (is_png && path.is_file()).then(|| path.to_owned());
    }
    let name = name.strip_suffix(".png").unwrap_or(name);
    let mut themes: Vec<PathBuf> = home.map(|home| home.join(".icons")).into_iter().collect();
    themes.extend(dirs.iter().map(|dir| dir.join("icons")));
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
pub fn find_app_icon(
    app_id: &str,
    dirs: &[PathBuf],
    home: Option<&Path>,
    size: u32,
) -> Option<PathBuf> {
    if app_id.trim().is_empty() {
        return None;
    }
    let mut names: Vec<String> = desktop_icon(app_id, dirs).into_iter().collect();
    names.push(app_id.to_owned());
    names
        .iter()
        .find_map(|name| find_icon_file(name, dirs, home, size))
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

/// app_id ごとのアイコン(大きさは 1 つ)を、1 回だけ探して覚えておく所。
pub struct IconCache {
    size: u32,
    dirs: Vec<PathBuf>,
    home: Option<PathBuf>,
    icons: HashMap<String, Icon>,
    default: Icon,
}

/// アイコンを探した結果(ログに出すため)。どちらも、今は既定のアイコンを使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// その PNG を見つけた(まだ読まない)。
    File(PathBuf),
    /// 見つからなかった。
    Missing,
}

impl IconCache {
    /// 大きさ `size` のアイコンを、データの置き場 `dirs` と、ホームの置き場 `home` から探す。
    pub fn new(size: u32, dirs: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        IconCache {
            size,
            dirs,
            home,
            icons: HashMap::new(),
            default: default_icon(size),
        }
    }

    /// 環境変数から決めた置き場で探す。
    pub fn from_env(size: u32) -> Self {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        IconCache::new(size, data_dirs_from_env(), home)
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
        let found = match find_app_icon(app_id, &self.dirs, self.home.as_deref(), self.size) {
            Some(path) => Found::File(path),
            None => Found::Missing,
        };
        self.icons.insert(app_id.to_owned(), self.default.clone());
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
            find_app_icon("seinas.test.files", &dirs, None, 24),
            Some(test_data().join("icons/hicolor/24x24/apps/seinas-test-folder.png"))
        );
        // .desktop が無くても、app_id の名前のアイコンがあれば、それ。24 が無ければ、大きいもの(48)。
        assert_eq!(
            find_app_icon("seinas.test.large", &dirs, None, 24),
            Some(test_data().join("icons/hicolor/48x48/apps/seinas.test.large.png"))
        );
        // テーマに無ければ pixmaps。
        assert_eq!(
            find_app_icon("seinas.test.pixmap", &dirs, None, 24),
            Some(test_data().join("pixmaps/seinas.test.pixmap.png"))
        );
        assert_eq!(find_app_icon("seinas.test.missing", &dirs, None, 24), None);
        assert_eq!(find_app_icon("", &dirs, None, 24), None);
    }

    #[test]
    fn icons_are_looked_up_once_and_the_default_is_shown() {
        let dirs = vec![test_data()];
        let mut cache = IconCache::new(24, dirs, None);
        // 見つけた PNG は、今は読まずに、既定のアイコンを出す。
        assert_eq!(
            cache.load("seinas.test.large"),
            Some(Found::File(
                test_data().join("icons/hicolor/48x48/apps/seinas.test.large.png")
            ))
        );
        assert_eq!(cache.get("seinas.test.large"), cache.default_icon());
        // 2 回目は探さない。
        assert_eq!(cache.load("seinas.test.large"), None);
        assert_eq!(cache.load("seinas.test.missing"), Some(Found::Missing));
        assert_eq!(cache.get("never.asked"), cache.default_icon());
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
