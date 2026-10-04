//! seinas-fbdev: 共通の描画の結果を、fbdevの画面へ出す。
//!
//! 決まった絵(背景、枠、四隅の印、中央の印)を、指定した時間だけ表示して終わる。
//! Waylandのソケットは開かない。
//!
//! 使い方:
//!
//! ```text
//! seinas-fbdev [--device PATH] [--hold-ms N]
//! seinas-fbdev --dry-run [--size WIDTHxHEIGHT] [--dump FILE]
//! ```
//!
//! - `--device PATH`: 装置のパス。無ければ環境変数 `SEINAS_FBDEV`、それも無ければ `/dev/fb0`。
//! - `--hold-ms N`: 絵を表示しておく時間(ミリ秒)。既定は3000。
//! - `--dry-run`: 装置を開かず、メモリー上の偽の画面へ描いて、中身の要約を出す。
//! - `--size WIDTHxHEIGHT`: `--dry-run` のときの画面の大きさ。既定は800x600。
//! - `--dump FILE`: `--dry-run` で描いた絵を、PPM形式の画像としてファイルに書く。
//!
//! 終わるときの後片付け: 始める前の画面の中身を覚えておき、終わるときに書き戻す。
//! その後、対応づけを外して装置を閉じる。画面の設定(解像度など)は、読むだけで変えない。

use std::{
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use seinas_fbdev::{
    device::Fbdev,
    layout::{Bitfield, Layout, ScreenInfo},
    picture::test_picture,
    present::WriteThrough,
};
use seinas_render::Painter;

const DEFAULT_DEVICE: &str = "/dev/fb0";
const DEVICE_ENV: &str = "SEINAS_FBDEV";
const DEFAULT_HOLD_MS: u64 = 3000;
const DEFAULT_DRY_RUN_SIZE: (u32, u32) = (800, 600);

const USAGE: &str = "usage: seinas-fbdev [--device PATH] [--hold-ms N]
       seinas-fbdev --dry-run [--size WIDTHxHEIGHT] [--dump FILE]";

struct Options {
    device: PathBuf,
    hold: Duration,
    dry_run: bool,
    size: (u32, u32),
    dump: Option<PathBuf>,
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        device: std::env::var_os(DEVICE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DEVICE)),
        hold: Duration::from_millis(DEFAULT_HOLD_MS),
        dry_run: false,
        size: DEFAULT_DRY_RUN_SIZE,
        dump: None,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--device" => options.device = PathBuf::from(value("--device")?),
            "--hold-ms" => {
                let text = value("--hold-ms")?;
                let ms = text
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --hold-ms value: {text}"))?;
                options.hold = Duration::from_millis(ms);
            }
            "--dry-run" => options.dry_run = true,
            "--size" => {
                let text = value("--size")?;
                options.size =
                    parse_size(&text).ok_or_else(|| format!("invalid --size value: {text}"))?;
            }
            "--dump" => options.dump = Some(PathBuf::from(value("--dump")?)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(options)
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// 装置を開かずに、メモリー上の偽の画面へ描く。
fn dry_run(size: (u32, u32), dump: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let info = ScreenInfo {
        width: size.0,
        height: size.1,
        x_offset: 0,
        y_offset: 0,
        bits_per_pixel: 32,
        line_length: size.0.saturating_mul(4),
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
    let mut screen = vec![0u8; layout.required_len()];
    let mut painter = Painter::new(layout.width() as i32, layout.height() as i32)?;
    let view = painter.paint(&test_picture(painter.width(), painter.height()))?;
    layout.blit(&view, &mut screen)?;
    println!(
        "seinas-fbdev: dry run: painted {}x{} ({} bytes), checksum {:016x}",
        layout.width(),
        layout.height(),
        screen.len(),
        checksum(&screen)
    );
    if let Some(path) = dump {
        // 偽の画面は B, G, R, X の順なので、PPMの R, G, B の順に並べ替えて書く。
        let mut image = format!("P6 {} {} 255\n", layout.width(), layout.height()).into_bytes();
        image.extend(
            screen
                .chunks_exact(4)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]),
        );
        std::fs::write(path, image).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// 装置を開いて絵を出し、`hold` の間だけ表示して、元の画面に戻す。
fn show(device: &Path, hold: Duration) -> Result<(), Box<dyn Error>> {
    let mut fb = Fbdev::open(device)?;
    let mut presenter = WriteThrough;
    let layout = *fb.layout();
    println!(
        "seinas-fbdev: {}: {}x{}, {} bits per pixel, line length {}",
        device.display(),
        layout.width(),
        layout.height(),
        fb.info().bits_per_pixel,
        fb.info().line_length
    );

    // 後片付けのために、始める前の画面の中身を覚えておく。
    let before = fb.pixels().to_vec();

    let mut painter = Painter::new(layout.width() as i32, layout.height() as i32)?;
    let view = painter.paint(&test_picture(painter.width(), painter.height()))?;
    fb.show(&view, &mut presenter)?;
    std::thread::sleep(hold);

    fb.pixels().copy_from_slice(&before);
    fb.present(&mut presenter)?;
    Ok(())
}

/// 中身の要約(FNV-1a、64ビット)。同じ絵なら同じ値になる。
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn main() -> ExitCode {
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("seinas-fbdev: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = if options.dry_run {
        dry_run(options.size, options.dump.as_deref())
    } else {
        show(&options.device, options.hold)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seinas-fbdev: {e}");
            ExitCode::FAILURE
        }
    }
}
