//! 正解データ(golden)のテストの部品。
//!
//! 置き換える前の出力を、正解データとして `tests/golden/` に保存しておき、今の出力と機械で比べる。
//! 決まりと手順は `docs/golden.md` にある。
//!
//! 使い方(`harness = false` のテストから):
//!
//! ```text
//! let mut run = Runner::new(env!("CARGO_MANIFEST_DIR"), "seinas-render");
//! run.image("a1-solid", Tolerance::EXACT, || make_image());
//! run.finish();
//! ```
//!
//! 環境変数:
//!
//! - `SEINAS_UPDATE_GOLDEN=1`: 比べるかわりに、今の出力で正解データを作り直す(`tools/update-golden.sh` が付ける)。
//! - `SEINAS_SKIP_FONT_GOLDEN=1`: フォントが無いとき、フォントの要るケースをスキップする(手元だけ。CI では効かない)。
//! - `CI`: CI の上で動いている印。フォントが無ければ、いつも失敗にする。

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

pub const UPDATE_ENV: &str = "SEINAS_UPDATE_GOLDEN";
pub const SKIP_FONTS_ENV: &str = "SEINAS_SKIP_FONT_GOLDEN";

/// ワークスペースの置き場所(この crate の 2 つ上)。
fn workspace_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // メッセージに出す道を短くするため、`..` を取り除く。
    root.canonicalize().unwrap_or(root)
}

fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty() && value != "0")
}

/// 画像。ピクセルは行ごとに並べ、1 ピクセルは `channels` バイト(3 なら R, G, B、1 ならグレー)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
}

impl Image {
    /// RGB の画像(PPM で保存する)。
    pub fn rgb(width: usize, height: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width * height * 3, "the size of an RGB image");
        Image {
            width,
            height,
            channels: 3,
            data,
        }
    }

    /// グレーの画像(PGM で保存する)。
    pub fn gray(width: usize, height: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width * height, "the size of a gray image");
        Image {
            width,
            height,
            channels: 1,
            data,
        }
    }

    /// B, G, R, X の順の 4 バイトのピクセル(Seinas の描画結果の形式)から、RGB の画像を作る。X は捨てる。
    pub fn from_bgrx(width: usize, height: usize, bgrx: &[u8]) -> Self {
        assert_eq!(bgrx.len(), width * height * 4, "the size of a BGRX image");
        let data = bgrx
            .chunks(4)
            .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]])
            .collect();
        Image::rgb(width, height, data)
    }

    fn extension(&self) -> &'static str {
        match self.channels {
            1 => "pgm",
            _ => "ppm",
        }
    }

    /// PPM(P6)か PGM(P5)のバイト列。
    pub fn encode(&self) -> Vec<u8> {
        let magic = if self.channels == 1 { "P5" } else { "P6" };
        let mut bytes = format!("{magic}\n{} {}\n255\n", self.width, self.height).into_bytes();
        bytes.extend(&self.data);
        bytes
    }

    /// PPM(P6)か PGM(P5)を読む。[`Image::encode`] が書いた形だけを受け付ける。
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut fields = Vec::new();
        let mut at = 0;
        // ヘッダーは、空白で区切った 4 つの語(形式、幅、高さ、最大値)と、その後の 1 文字の空白。
        while fields.len() < 4 {
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let start = at;
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if start == at {
                return Err("the header is too short".to_owned());
            }
            fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
        }
        at += 1;
        let channels = match fields[0].as_str() {
            "P6" => 3,
            "P5" => 1,
            other => return Err(format!("unsupported format {other}")),
        };
        let number = |text: &str| {
            text.parse::<usize>()
                .map_err(|_| format!("bad number {text}"))
        };
        let (width, height) = (number(&fields[1])?, number(&fields[2])?);
        if fields[3] != "255" {
            return Err(format!("unsupported maximum {}", fields[3]));
        }
        let data = bytes.get(at..).unwrap_or_default().to_vec();
        if data.len() != width * height * channels {
            return Err(format!(
                "{} bytes of pixels, expected {}",
                data.len(),
                width * height * channels
            ));
        }
        Ok(Image {
            width,
            height,
            channels,
            data,
        })
    }
}

/// 許す差。今は、どの対象も [`Tolerance::EXACT`](完全一致)で比べる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tolerance {
    /// ちがってよいピクセルの数。
    pub max_differing: usize,
    /// 1 つのチャンネルの差の上限(この大きさまでの差は、ちがうと数えない)。
    pub max_channel_diff: u8,
}

impl Tolerance {
    pub const EXACT: Tolerance = Tolerance {
        max_differing: 0,
        max_channel_diff: 0,
    };
}

/// 2 つの出力の差。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Difference {
    /// ちがうピクセル(raw のときはバイト)の数。
    pub differing: usize,
    /// いちばん大きいチャンネル(raw のときはバイト)の差。
    pub max_channel_diff: u8,
    /// ちがう所を囲む矩形(左, 上, 右, 下。右と下を含む)。ちがう所が無ければ None。
    pub bounds: Option<(usize, usize, usize, usize)>,
}

impl Difference {
    fn describe(&self) -> String {
        match self.bounds {
            None => "no difference".to_owned(),
            Some((left, top, right, bottom)) => format!(
                "{} differing, max channel diff {}, bounds x {left}..={right} y {top}..={bottom}",
                self.differing, self.max_channel_diff
            ),
        }
    }
}

/// 同じ大きさの 2 つの値の並びを比べる。`row` は 1 行の要素の数、`channels` は 1 要素のバイト数。
/// `tolerance` 以下のチャンネルの差は、ちがうと数えない。
pub fn compare(
    expected: &[u8],
    actual: &[u8],
    row: usize,
    channels: usize,
    tolerance: u8,
) -> Difference {
    assert_eq!(expected.len(), actual.len(), "compare the same sizes");
    let mut difference = Difference::default();
    for (index, (e, a)) in expected
        .chunks(channels)
        .zip(actual.chunks(channels))
        .enumerate()
    {
        let max = e
            .iter()
            .zip(a)
            .map(|(e, a)| e.abs_diff(*a))
            .max()
            .unwrap_or(0);
        difference.max_channel_diff = difference.max_channel_diff.max(max);
        if max <= tolerance {
            continue;
        }
        difference.differing += 1;
        let (x, y) = (index % row.max(1), index / row.max(1));
        difference.bounds = Some(match difference.bounds {
            None => (x, y, x, y),
            Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
        });
    }
    difference
}

/// 差を見やすくした画像。同じ所は正解を暗くしたもの、ちがう所は赤。
fn diff_image(expected: &Image, actual: &Image) -> Image {
    let pixels = expected.width * expected.height;
    let mut data = Vec::with_capacity(pixels * 3);
    for index in 0..pixels {
        let e = &expected.data[index * expected.channels..(index + 1) * expected.channels];
        let a = &actual.data[index * actual.channels..(index + 1) * actual.channels];
        if e == a {
            let gray = (e.iter().map(|&v| v as u32).sum::<u32>() / e.len() as u32 / 3) as u8;
            data.extend([gray, gray, gray]);
        } else {
            data.extend([0xff, 0x00, 0x00]);
        }
    }
    Image::rgb(expected.width, expected.height, data)
}

/// フォントが、正解データを作ったときと同じものか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fonts {
    /// `tools/fetch-fonts.sh` の版と同じ。中身は、フォントの置き場。
    Ready(PathBuf),
    /// 足りないファイルがある。
    Missing(Vec<String>),
    /// SHA-256 が `tools/fetch-fonts.sh` の値とちがう。
    Mismatch(Vec<String>),
}

/// `target/fonts` のフォントを確かめる。確かめるのは、`tools/fetch-fonts.sh` が取得するフォントのファイル
/// (`.ttf` と `.otf`)の全部で、SHA-256 はそのスクリプトに書かれた値と比べる。
pub fn check_fonts() -> Fonts {
    let root = workspace_root();
    let dir = root.join("target/fonts");
    let script =
        fs::read_to_string(root.join("tools/fetch-fonts.sh")).expect("read tools/fetch-fonts.sh");
    let (mut missing, mut mismatch) = (Vec::new(), Vec::new());
    // fetch "$URL/.../ファイル名" SHA-256 の行を読む。
    for line in script.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("fetch ") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(url), Some(sha)) = (words.next(), words.next()) else {
            continue;
        };
        let name = url.trim_matches('"').rsplit('/').next().unwrap_or_default();
        if !(name.ends_with(".ttf") || name.ends_with(".otf")) {
            continue;
        }
        match fs::read(dir.join(name)) {
            Err(_) => missing.push(name.to_owned()),
            Ok(bytes) => {
                let actual: String = Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                if actual != sha {
                    mismatch.push(format!("{name} (sha256 {actual}, expected {sha})"));
                }
            }
        }
    }
    if !mismatch.is_empty() {
        Fonts::Mismatch(mismatch)
    } else if !missing.is_empty() {
        Fonts::Missing(missing)
    } else {
        Fonts::Ready(dir)
    }
}

/// golden テストの実行。ケースを 1 つずつ比べ(作り直しのモードなら作り直し)、最後にまとめを出す。
pub struct Runner {
    crate_name: String,
    golden_dir: PathBuf,
    actual_dir: PathBuf,
    update: bool,
    /// フォントを確かめた結果。フォントの要るケースが来たときに、初めて確かめる(SHA-256 の計算に時間がかかるため)。
    fonts: Option<Fonts>,
    allow_font_skip: bool,
    checked: usize,
    skipped: usize,
    updated: usize,
    failures: Vec<String>,
}

impl Runner {
    /// `manifest_dir` はテストする crate の `CARGO_MANIFEST_DIR`。正解データは、その `tests/golden/` に置く。
    pub fn new(manifest_dir: &str, crate_name: &str) -> Self {
        Runner {
            crate_name: crate_name.to_owned(),
            golden_dir: Path::new(manifest_dir).join("tests/golden"),
            actual_dir: workspace_root()
                .join("target/golden-actual")
                .join(crate_name),
            update: env_is_set(UPDATE_ENV),
            fonts: None,
            // スキップしてよいのは、手元で明示したときだけ。CI と作り直しのときは、いつも失敗にする。
            allow_font_skip: env_is_set(SKIP_FONTS_ENV)
                && !env_is_set("CI")
                && !env_is_set(UPDATE_ENV),
            checked: 0,
            skipped: 0,
            updated: 0,
            failures: Vec::new(),
        }
    }

    /// フォントの要るケース。フォントが使えれば `case` にフォントの置き場を渡して動かす。
    ///
    /// フォントが無いときは、スキップしてよいとき(手元で `SEINAS_SKIP_FONT_GOLDEN=1`)だけスキップし、
    /// それ以外は失敗にする。SHA-256 がちがうときは、いつも失敗にする。
    pub fn with_fonts(&mut self, name: &str, case: impl FnOnce(&mut Self, &Path)) {
        let fonts = self.fonts.get_or_insert_with(check_fonts).clone();
        match fonts {
            Fonts::Ready(dir) => case(self, &dir),
            Fonts::Missing(_) if self.allow_font_skip => self.skipped += 1,
            Fonts::Missing(files) => self.failures.push(format!(
                "{name}: the fonts are missing ({}); run tools/fetch-fonts.sh, or set {SKIP_FONTS_ENV}=1 to skip locally",
                files.join(", ")
            )),
            Fonts::Mismatch(files) => self.failures.push(format!(
                "{name}: the fonts differ from tools/fetch-fonts.sh: {}",
                files.join("; ")
            )),
        }
    }

    /// 画像のケース。
    pub fn image(&mut self, name: &str, tolerance: Tolerance, actual: Image) {
        let file = format!("{name}.{}", actual.extension());
        let path = self.golden_dir.join(&file);
        let expected = fs::read(&path).ok().map(|bytes| Image::decode(&bytes));
        if self.update {
            let note = match &expected {
                Some(Ok(old))
                    if old.width == actual.width
                        && old.height == actual.height
                        && old.channels == actual.channels =>
                {
                    compare(&old.data, &actual.data, actual.width, actual.channels, 0).describe()
                }
                Some(_) => "the size or the format changed".to_owned(),
                None => "created".to_owned(),
            };
            self.write_golden(&file, &actual.encode(), &note);
            return;
        }
        self.checked += 1;
        let expected = match expected {
            None => return self.fail_missing(&file),
            Some(Err(error)) => {
                return self
                    .failures
                    .push(format!("{file}: cannot read the golden data: {error}"))
            }
            Some(Ok(expected)) => expected,
        };
        if (expected.width, expected.height, expected.channels)
            != (actual.width, actual.height, actual.channels)
        {
            self.save_actual(&file, &actual.encode());
            return self.failures.push(format!(
                "{file}: size or format differs: expected {}x{}x{}, actual {}x{}x{}",
                expected.width,
                expected.height,
                expected.channels,
                actual.width,
                actual.height,
                actual.channels
            ));
        }
        let difference = compare(
            &expected.data,
            &actual.data,
            actual.width,
            actual.channels,
            tolerance.max_channel_diff,
        );
        if difference.differing > tolerance.max_differing {
            self.save_actual(&file, &actual.encode());
            self.save_actual(
                &format!("{name}.diff.ppm"),
                &diff_image(&expected, &actual).encode(),
            );
            self.failures.push(format!(
                "{file}: {} (tolerance: {} pixels, channel diff {}); actual and diff written to {}",
                difference.describe(),
                tolerance.max_differing,
                tolerance.max_channel_diff,
                self.actual_dir.display()
            ));
        }
    }

    /// バイト列のケース(fbdev のバッファなど)。いつも完全一致で比べる。`row` は 1 行のバイト数
    /// (ちがう所を囲む矩形を、バイトの位置と行で出すのに使う)。
    pub fn raw(&mut self, name: &str, row: usize, actual: &[u8]) {
        let file = format!("{name}.bin");
        let expected = fs::read(self.golden_dir.join(&file)).ok();
        if self.update {
            let note = match &expected {
                Some(old) if old.len() == actual.len() => {
                    compare(old, actual, row, 1, 0).describe()
                }
                Some(_) => "the size changed".to_owned(),
                None => "created".to_owned(),
            };
            self.write_golden(&file, actual, &note);
            return;
        }
        self.checked += 1;
        let Some(expected) = expected else {
            return self.fail_missing(&file);
        };
        if expected.len() != actual.len() {
            self.save_actual(&file, actual);
            return self.failures.push(format!(
                "{file}: {} bytes, expected {}",
                actual.len(),
                expected.len()
            ));
        }
        let difference = compare(&expected, actual, row, 1, 0);
        if difference.differing > 0 {
            self.save_actual(&file, actual);
            self.failures.push(format!(
                "{file}: {} (bytes; x is the byte in a row of {row}); actual written to {}",
                difference.describe(),
                self.actual_dir.display()
            ));
        }
    }

    /// 文字のケース(1 行に 1 つの値を書いたもの)。いつも完全一致で比べる。
    pub fn text(&mut self, name: &str, actual: &str) {
        let file = name.to_owned();
        let expected = fs::read_to_string(self.golden_dir.join(&file)).ok();
        if self.update {
            let note = match &expected {
                Some(old) => {
                    let changed = old
                        .lines()
                        .zip(actual.lines())
                        .filter(|(o, a)| o != a)
                        .count()
                        + old.lines().count().abs_diff(actual.lines().count());
                    format!("{changed} line(s) changed")
                }
                None => "created".to_owned(),
            };
            self.write_golden(&file, actual.as_bytes(), &note);
            return;
        }
        self.checked += 1;
        let Some(expected) = expected else {
            return self.fail_missing(&file);
        };
        if expected != actual {
            self.save_actual(&file, actual.as_bytes());
            let mut message = format!("{file}: lines differ:");
            for (e, a) in expected.lines().zip(actual.lines()).filter(|(e, a)| e != a) {
                let _ = write!(message, "\n    expected: {e}\n    actual:   {a}");
            }
            if expected.lines().count() != actual.lines().count() {
                let _ = write!(
                    message,
                    "\n    {} lines, expected {}",
                    actual.lines().count(),
                    expected.lines().count()
                );
            }
            self.failures.push(message);
        }
    }

    fn fail_missing(&mut self, file: &str) {
        self.failures.push(format!(
            "{file}: no golden data in {}; run tools/update-golden.sh",
            self.golden_dir.display()
        ));
    }

    fn write_golden(&mut self, file: &str, bytes: &[u8], note: &str) {
        fs::create_dir_all(&self.golden_dir).expect("create the golden directory");
        fs::write(self.golden_dir.join(file), bytes).expect("write the golden data");
        self.updated += 1;
        println!("golden {}: {file}: {note}", self.crate_name);
    }

    fn save_actual(&self, file: &str, bytes: &[u8]) {
        let _ = fs::create_dir_all(&self.actual_dir);
        let _ = fs::write(self.actual_dir.join(file), bytes);
    }

    /// まとめを出す。失敗があれば、それを全部出して、終了コード 1 で終わる。
    pub fn finish(self) {
        if self.update {
            println!(
                "golden {}: updated {} file(s) in {}",
                self.crate_name,
                self.updated,
                self.golden_dir.display()
            );
            if !self.failures.is_empty() {
                for failure in &self.failures {
                    eprintln!("golden {}: FAILED {failure}", self.crate_name);
                }
                std::process::exit(1);
            }
            return;
        }
        println!(
            "golden {}: {} checked, {} skipped (no fonts), {} failed",
            self.crate_name,
            self.checked,
            self.skipped,
            self.failures.len()
        );
        if self.skipped > 0 {
            println!(
                "golden {}: {} case(s) were skipped because the fonts are missing and {SKIP_FONTS_ENV} is set",
                self.crate_name, self.skipped
            );
        }
        if !self.failures.is_empty() {
            for failure in &self.failures {
                eprintln!("golden {}: FAILED {failure}", self.crate_name);
            }
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_round_trip_through_ppm_and_pgm() {
        let rgb = Image::rgb(2, 1, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(Image::decode(&rgb.encode()).unwrap(), rgb);
        assert!(rgb.encode().starts_with(b"P6\n2 1\n255\n"));
        let gray = Image::gray(3, 2, vec![0, 10, 20, 30, 40, 255]);
        assert_eq!(Image::decode(&gray.encode()).unwrap(), gray);
        assert!(gray.encode().starts_with(b"P5\n3 2\n255\n"));
        // ピクセルの数が合わないものは読まない。
        assert!(Image::decode(b"P6\n2 1\n255\n\x01\x02").is_err());
        assert!(Image::decode(b"P3\n1 1\n255\n1 2 3").is_err());
    }

    #[test]
    fn bgrx_pixels_become_rgb() {
        let image = Image::from_bgrx(2, 1, &[1, 2, 3, 0xff, 4, 5, 6, 0]);
        assert_eq!(image.data, [3, 2, 1, 6, 5, 4]);
    }

    #[test]
    fn the_comparison_reports_count_maximum_and_bounds() {
        let expected = vec![0u8; 4 * 3 * 3];
        let mut actual = expected.clone();
        assert_eq!(compare(&expected, &actual, 4, 3, 0), Difference::default());
        // (1, 0) の緑を 7、(2, 2) の青を 200 にする。
        actual[3 + 1] = 7;
        actual[(2 * 4 + 2) * 3 + 2] = 200;
        let difference = compare(&expected, &actual, 4, 3, 0);
        assert_eq!(difference.differing, 2);
        assert_eq!(difference.max_channel_diff, 200);
        assert_eq!(difference.bounds, Some((1, 0, 2, 2)));
        // 許す差より小さいチャンネルの差は、数えない(いちばん大きい差は、そのまま出す)。
        let difference = compare(&expected, &actual, 4, 3, 10);
        assert_eq!(difference.differing, 1);
        assert_eq!(difference.bounds, Some((2, 2, 2, 2)));
        assert_eq!(difference.max_channel_diff, 200);
    }

    #[test]
    fn the_diff_image_marks_differing_pixels_red() {
        let expected = Image::rgb(2, 1, vec![30, 30, 30, 90, 90, 90]);
        let actual = Image::rgb(2, 1, vec![30, 30, 30, 91, 90, 90]);
        let diff = diff_image(&expected, &actual);
        assert_eq!(diff.data, [10, 10, 10, 0xff, 0, 0]);
    }

    #[test]
    fn the_fonts_are_checked_against_the_fetch_script() {
        // 置いてあれば Ready、無ければ Missing。どちらでも、fetch-fonts.sh の読み取りは通る。
        match check_fonts() {
            Fonts::Ready(dir) => assert!(dir.ends_with("target/fonts")),
            Fonts::Missing(files) => assert!(!files.is_empty()),
            Fonts::Mismatch(files) => panic!("the fonts differ: {files:?}"),
        }
    }
}
