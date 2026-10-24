//! Seinas のタスクバーの、並べ方・描き方・時計(Wayland に触れない部分)。
//!
//! バイナリ(`src/main.rs`)が、layer-shell と foreign-toplevel のクライアントとして動き、ここの関数で
//! 描いて、ポインターの位置からボタンを選ぶ。決まりの説明は `docs/taskbar.md` にある。

use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use seinas_text::{Canvas, Line, TextPainter};

/// タスクバーの高さ(ピクセル)。exclusive zone も、この高さにする。
pub const HEIGHT: i32 = 40;
/// ボタンの上の端と高さ。
pub const BUTTON_TOP: i32 = 4;
pub const BUTTON_HEIGHT: i32 = 32;
/// 左の端と、ボタンどうしの空き。
pub const LEFT_MARGIN: i32 = 4;
pub const BUTTON_GAP: i32 = 4;
/// ボタンの幅の上限。ウィンドウが多くて入りきらないときは、そろって狭くなる。
pub const BUTTON_MAX_WIDTH: i32 = 200;
/// ボタンの中の、タイトルの左右の空き。
pub const TEXT_PADDING: i32 = 10;
/// 文字の大きさ(ピクセル)。
pub const TEXT_SIZE: f32 = 14.0;
/// 右の端の、時計の場所の幅。時計の文字は、この中で右に寄せる。
pub const CLOCK_WIDTH: i32 = 72;
/// ボタンの下の端の、印(インジケーター)の高さと幅。
pub const INDICATOR_HEIGHT: i32 = 3;
pub const ACTIVE_INDICATOR_WIDTH: i32 = 24;
pub const INDICATOR_WIDTH: i32 = 8;

// 色は (赤, 緑, 青)。
/// タスクバーの地。
pub const BACKGROUND: [u8; 3] = [0x20, 0x24, 0x2c];
/// activated のウィンドウのボタンの地。
pub const ACTIVE_BUTTON: [u8; 3] = [0x3a, 0x40, 0x4c];
/// ポインターが乗っているボタンの地(activated でないもの)。
pub const HOT_BUTTON: [u8; 3] = [0x2e, 0x33, 0x3d];
/// activated のウィンドウの印(アクセントの色)。
pub const ACTIVE_INDICATOR: [u8; 3] = [0x4c, 0xc2, 0xff];
/// そのほかの、開いているウィンドウの印。
pub const INDICATOR: [u8; 3] = [0x8a, 0x90, 0x9c];
/// 文字の色。最小化しているウィンドウのタイトルは、薄くする。
pub const TEXT: [u8; 3] = [0xf0, 0xf2, 0xf5];
pub const MINIMIZED_TEXT: [u8; 3] = [0x9a, 0x9f, 0xa8];

/// 時刻が取れないときの、時計の表示。
pub const NO_CLOCK: &str = "--:--";
/// 壁の時計が、これより前を指していたら、合わせられていないものとして扱う(2020-01-01T00:00:00Z)。
pub const EARLIEST_WALL_CLOCK: u64 = 1_577_836_800;
/// 時差の指定が無いときの時差(秒)。日本の時刻(+09:00)。
pub const DEFAULT_UTC_OFFSET: i32 = 9 * 3600;

/// タスクバーに並べる、1 つのウィンドウ。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    /// タイトル。無ければ app_id。
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
}

/// ボタンの、左の端と幅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button {
    pub x: i32,
    pub width: i32,
}

/// 幅が `width` のタスクバーに、`count` 個のボタンを並べる。左から順に、同じ幅で並べる。
pub fn buttons(width: i32, count: usize) -> Vec<Button> {
    if count == 0 {
        return Vec::new();
    }
    let n = count as i32;
    let available = width - LEFT_MARGIN - CLOCK_WIDTH - BUTTON_GAP;
    let each = ((available - BUTTON_GAP * (n - 1)) / n).clamp(1, BUTTON_MAX_WIDTH);
    (0..n)
        .map(|i| Button {
            x: LEFT_MARGIN + i * (each + BUTTON_GAP),
            width: each,
        })
        .collect()
}

/// 点 (`x`, `y`) にあるボタンの番号。
pub fn button_at(width: i32, count: usize, x: f64, y: f64) -> Option<usize> {
    if y < BUTTON_TOP as f64 || y >= (BUTTON_TOP + BUTTON_HEIGHT) as f64 {
        return None;
    }
    buttons(width, count)
        .iter()
        .position(|b| x >= b.x as f64 && x < (b.x + b.width) as f64)
}

/// ボタンを押したときに、コンポジタへ頼むこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 最小化を戻して、activate する。
    Restore,
    /// activate する(raise して activated にする)。
    Activate,
    /// 最小化する(activated のウィンドウのボタンを押したとき)。
    Minimize,
}

/// ボタン `entry` を押したときに頼むこと(Windows のタスクバーと同じ)。
pub fn action_for(entry: &Entry) -> Action {
    if entry.minimized {
        Action::Restore
    } else if entry.activated {
        Action::Minimize
    } else {
        Action::Activate
    }
}

/// タスクバーの絵を作る。幅は `width`、高さは [`HEIGHT`]。ピクセルの並びは B, G, R, X。
///
/// `hot` は、ポインターが乗っているボタン。`clock` は、右の端に出す文字(時刻か [`NO_CLOCK`])。
pub fn paint(
    painter: &mut TextPainter,
    width: i32,
    entries: &[Entry],
    hot: Option<usize>,
    clock: &str,
) -> Vec<u8> {
    let width = width.max(1);
    let mut pixels = bgrx(BACKGROUND).repeat((width * HEIGHT) as usize);
    for (index, (entry, button)) in entries
        .iter()
        .zip(buttons(width, entries.len()))
        .enumerate()
    {
        let ground = if entry.activated {
            Some(ACTIVE_BUTTON)
        } else if hot == Some(index) {
            Some(HOT_BUTTON)
        } else {
            None
        };
        if let Some(ground) = ground {
            fill(
                &mut pixels,
                width,
                (button.x, BUTTON_TOP, button.width, BUTTON_HEIGHT),
                ground,
            );
        }
        // 印:activated はアクセントの色で長く、ほかの開いているウィンドウは短く。最小化しているものには無い。
        let indicator = if entry.minimized {
            None
        } else if entry.activated {
            Some((ACTIVE_INDICATOR_WIDTH, ACTIVE_INDICATOR))
        } else {
            Some((INDICATOR_WIDTH, INDICATOR))
        };
        if let Some((length, color)) = indicator {
            let length = length.min(button.width);
            fill(
                &mut pixels,
                width,
                (
                    button.x + (button.width - length) / 2,
                    BUTTON_TOP + BUTTON_HEIGHT - INDICATOR_HEIGHT,
                    length,
                    INDICATOR_HEIGHT,
                ),
                color,
            );
        }
        painter.draw_line(
            &mut Canvas::new(&mut pixels, width as usize, HEIGHT as usize),
            &entry.title,
            Line {
                size: TEXT_SIZE,
                color: if entry.minimized {
                    MINIMIZED_TEXT
                } else {
                    TEXT
                },
                x: button.x + TEXT_PADDING,
                top: BUTTON_TOP,
                height: BUTTON_HEIGHT - INDICATOR_HEIGHT,
                max_width: button.width - TEXT_PADDING * 2,
            },
        );
    }
    // 時計は右に寄せる。幅は、別の場所に一度描いて測る。
    let clock_line = |x| Line {
        size: TEXT_SIZE,
        color: TEXT,
        x,
        top: 0,
        height: HEIGHT,
        max_width: CLOCK_WIDTH - TEXT_PADDING * 2,
    };
    let mut scratch = vec![0u8; (CLOCK_WIDTH * HEIGHT * 4) as usize];
    let measured = painter.draw_line(
        &mut Canvas::new(&mut scratch, CLOCK_WIDTH as usize, HEIGHT as usize),
        clock,
        clock_line(TEXT_PADDING),
    );
    painter.draw_line(
        &mut Canvas::new(&mut pixels, width as usize, HEIGHT as usize),
        clock,
        clock_line(width - TEXT_PADDING - measured.width),
    );
    pixels
}

fn bgrx(color: [u8; 3]) -> [u8; 4] {
    [color[2], color[1], color[0], 0xff]
}

/// 矩形 (x, y, 幅, 高さ) を塗る。外へ出たぶんは描かない。
fn fill(pixels: &mut [u8], width: i32, (x, y, w, h): (i32, i32, i32, i32), color: [u8; 3]) {
    let height = pixels.len() as i32 / 4 / width;
    for row in y.max(0)..(y + h).min(height) {
        for column in x.max(0)..(x + w).min(width) {
            let at = ((row * width + column) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&bgrx(color));
        }
    }
}

/// 壁の時計(UNIX 時刻)。取れないとき(clock_gettime が失敗した、合わせられていない)はNone。
///
/// std の `SystemTime::now` は、clock_gettime が失敗すると panic するので、自分で呼ぶ。
pub fn wall_clock() -> Option<Duration> {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: now は、書き込める timespec を指している。
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) } != 0 {
        return None;
    }
    let seconds = u64::try_from(now.tv_sec).ok()?;
    (seconds >= EARLIEST_WALL_CLOCK)
        .then(|| Duration::new(seconds, now.tv_nsec.clamp(0, 999_999_999) as u32))
}

/// 時計の表示(時:分、24 時間)。`now` は UNIX 時刻、`offset` は時差(秒)。時刻が無ければ [`NO_CLOCK`]。
pub fn clock_text(now: Option<Duration>, offset: i32) -> String {
    let Some(now) = now else {
        return NO_CLOCK.to_owned();
    };
    let local = now.as_secs() as i64 + offset as i64;
    let minutes = local.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// 次に時計を再描画するまでの時間。分が変わった少し後。時刻が無ければ 60 秒後(取れるようになったかを見る)。
pub fn until_next_minute(now: Option<Duration>) -> Duration {
    // 分が変わる少し前に起きて、前の分を描かないよう、少し遅らせる。
    const MARGIN: Duration = Duration::from_millis(5);
    let Some(now) = now else {
        return Duration::from_secs(60);
    };
    let into_minute = Duration::new(now.as_secs() % 60, now.subsec_nanos());
    Duration::from_secs(60) - into_minute + MARGIN
}

/// 設定のファイルの名前(置き場は `$XDG_CONFIG_HOME/seinas/`)。
pub const CONFIG_FILE: &str = "taskbar.conf";
/// 時差の範囲(秒)。-12:00 から +14:00 まで(世界で使われている時差の範囲)。
pub const MIN_UTC_OFFSET: i32 = -12 * 3600;
pub const MAX_UTC_OFFSET: i32 = 14 * 3600;

/// 時差の指定(`+09:00`、`-05:30`、`+9`、`UTC`)を、秒にする。範囲は -12:00〜+14:00、分は 0・15・30・45 だけ。
/// 読めない値と、範囲の外の値はNone。
pub fn parse_utc_offset(text: &str) -> Option<i32> {
    if text.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':').unwrap_or((rest, "0"));
    if hours.is_empty() || hours.len() > 2 || minutes.is_empty() || minutes.len() > 2 {
        return None;
    }
    let (hours, minutes): (i32, i32) = (hours.parse().ok()?, minutes.parse().ok()?);
    if !(0..60).contains(&minutes) || minutes % 15 != 0 {
        return None;
    }
    let offset = sign * (hours * 3600 + minutes * 60);
    (MIN_UTC_OFFSET..=MAX_UTC_OFFSET)
        .contains(&offset)
        .then_some(offset)
}

/// 時差を `+09:00` の形の文字にする。
pub fn format_utc_offset(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// タスクバーの設定のファイル(`taskbar.conf`)を読み、`utc-offset` の時差を返す(書いていなければNone)。読めない
/// 行・知らないキー・読めない値は、理由のリストに入れる(読めない値は、無いものとして扱う)。
pub fn read_config(path: &Path) -> (Option<i32>, Vec<String>) {
    let (entries, problems) = seinas_config::read(path);
    let mut offset = None;
    let mut notes: Vec<String> = problems
        .into_iter()
        .map(|p| match p.line {
            0 => p.reason,
            line => format!("{}, line {line}: {}", path.display(), p.reason),
        })
        .collect();
    for entry in entries {
        match entry.key.as_str() {
            "utc-offset" => match parse_utc_offset(&entry.value) {
                Some(value) => offset = Some(value),
                None => notes.push(format!(
                    "{}, line {}: utc-offset must be UTC or from -12:00 to +14:00 in steps of 15 minutes: {}",
                    path.display(),
                    entry.line,
                    entry.value
                )),
            },
            other => notes.push(format!(
                "{}, line {}: unknown key: {other}",
                path.display(),
                entry.line
            )),
        }
    }
    (offset, notes)
}

/// 時計の時差を決める。起動オプション `--utc-offset` の値 `argument`、設定のファイルの値 `config`、環境変数 `TZ`
/// の値 `tz` の順に見て、使えるものが無ければ [`DEFAULT_UTC_OFFSET`]。読めない起動オプションは、理由を
/// `notes` に入れて、無いものとして扱う。返すのは、時差と、どこから決めたか。
pub fn choose_offset(
    argument: Option<&str>,
    config: Option<i32>,
    tz: Option<&str>,
    notes: &mut Vec<String>,
) -> (i32, &'static str) {
    if let Some(text) = argument {
        match parse_utc_offset(text) {
            Some(offset) => return (offset, "--utc-offset"),
            None => notes.push(format!(
                "invalid --utc-offset: {text} (use UTC or +HH:MM from -12:00 to +14:00 in steps of 15 minutes)"
            )),
        }
    }
    if let Some(offset) = config {
        return (offset, CONFIG_FILE);
    }
    if let Some(offset) = tz.and_then(offset_from_tz) {
        return (offset, "TZ");
    }
    (DEFAULT_UTC_OFFSET, "the default")
}

/// POSIX の TZ のうち、夏時間の無い簡単な形(`JST-9`、`UTC0`、`<+09>-9`、`UTC`)から、時差(秒)を求める。
///
/// タイムゾーンのデータベース(`/usr/share/zoneinfo`)は読まない(ZeikOS に無いため)。読めない形はNone。
pub fn offset_from_tz(tz: &str) -> Option<i32> {
    let rest = if let Some(quoted) = tz.strip_prefix('<') {
        quoted.split_once('>')?.1
    } else {
        let name_end = tz
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(tz.len());
        if name_end < 3 {
            return None;
        }
        &tz[name_end..]
    };
    if rest.is_empty() {
        return matches!(tz, "UTC" | "GMT").then_some(0);
    }
    // POSIX では、符号が逆(東が負)。
    let (sign, digits) = match rest.as_bytes()[0] {
        b'+' => (-1, &rest[1..]),
        b'-' => (1, &rest[1..]),
        _ => (-1, rest),
    };
    let mut parts = digits.split(':');
    let hours: i32 = parts.next()?.parse().ok()?;
    let minutes: i32 = parts.next().map_or(Some(0), |m| m.parse().ok())?;
    if parts.next().is_some() || !(0..=24).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

/// コンポジタへの最初の接続を試し直す間隔と、試し直す期限の既定(最初の試しから)。期限は単調増加の時計で
/// 判定する(`docs/taskbar.md` の「起動の順序」)。
pub const CONNECT_INTERVAL: Duration = Duration::from_millis(100);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// つないだ後の最初の往復(`wl_display.sync`)の答えを待つ期限の既定(つないだ時から)。
pub const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(30);

/// つなぐソケットのパス。`display`(`WAYLAND_DISPLAY`)が絶対パスならそのまま、そうでなければ `runtime_dir`
/// (`XDG_RUNTIME_DIR`。絶対パスに限る)の下(wayland-client の `Connection::connect_to_env` と同じ決まり)。
pub fn socket_path(
    display: Option<&OsStr>,
    runtime_dir: Option<&OsStr>,
) -> Result<PathBuf, String> {
    let display = Path::new(display.ok_or("WAYLAND_DISPLAY is not set")?);
    if display.is_absolute() {
        return Ok(display.to_owned());
    }
    match runtime_dir.map(Path::new) {
        Some(dir) if dir.is_absolute() => Ok(dir.join(display)),
        Some(_) => Err("XDG_RUNTIME_DIR is not an absolute path".to_owned()),
        None => Err(format!(
            "XDG_RUNTIME_DIR is not set (needed for WAYLAND_DISPLAY={})",
            display.display()
        )),
    }
}

/// 接続の失敗が、コンポジタがまだ受けられないためなら、その errno の名前。このときだけ、試し直す。
///
/// - `ENOENT`:ソケットがまだ無い。
/// - `ECONNREFUSED`:ソケットはあるが、listen する相手がいない。
/// - `EAGAIN`:ZeikOS で、接続のテーブルが満杯(Linux の blocking connect は、相手の backlog が満杯なら待つ)。
pub fn not_ready_yet(error: &io::Error) -> Option<&'static str> {
    match error.raw_os_error()? {
        libc::ENOENT => Some("ENOENT"),
        libc::ECONNREFUSED => Some("ECONNREFUSED"),
        libc::EAGAIN => Some("EAGAIN"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_path_follows_wayland_display_and_xdg_runtime_dir() {
        let path = |display: Option<&str>, dir: Option<&str>| {
            socket_path(display.map(OsStr::new), dir.map(OsStr::new))
        };
        assert_eq!(
            path(Some("/tmp/s-shell.sock"), None),
            Ok(PathBuf::from("/tmp/s-shell.sock"))
        );
        assert_eq!(
            path(Some("seinas-0-shell"), Some("/run/user/1000")),
            Ok(PathBuf::from("/run/user/1000/seinas-0-shell"))
        );
        assert!(path(None, Some("/run/user/1000")).is_err());
        assert!(path(Some("seinas-0-shell"), None).is_err());
        assert!(path(Some("seinas-0-shell"), Some("run")).is_err());
    }

    #[test]
    fn only_a_missing_socket_no_listener_or_a_full_table_is_tried_again() {
        let reason = |errno| not_ready_yet(&io::Error::from_raw_os_error(errno));
        assert_eq!(reason(libc::ENOENT), Some("ENOENT"));
        assert_eq!(reason(libc::ECONNREFUSED), Some("ECONNREFUSED"));
        assert_eq!(reason(libc::EAGAIN), Some("EAGAIN"));
        assert_eq!(reason(libc::EACCES), None);
        assert_eq!(reason(libc::ENAMETOOLONG), None);
        assert_eq!(
            not_ready_yet(&io::Error::from(io::ErrorKind::NotFound)),
            None
        );
    }

    #[test]
    fn buttons_share_the_width_up_to_a_limit() {
        // 800 の幅:使えるのは 800 - 4 - 72 - 4 = 720。
        assert_eq!(buttons(800, 0), Vec::new());
        assert_eq!(buttons(800, 1), vec![Button { x: 4, width: 200 }]);
        assert_eq!(
            buttons(800, 2),
            vec![Button { x: 4, width: 200 }, Button { x: 208, width: 200 }]
        );
        // 5 つなら (720 - 16) / 5 = 140。
        let five = buttons(800, 5);
        assert_eq!(five[0], Button { x: 4, width: 140 });
        assert_eq!(
            five[4],
            Button {
                x: 4 + 4 * 144,
                width: 140
            }
        );
        assert!(five[4].x + five[4].width <= 800 - CLOCK_WIDTH);
        // とても狭くても、幅は 1 より小さくしない。
        assert!(buttons(80, 3).iter().all(|b| b.width == 1));
    }

    #[test]
    fn a_point_selects_a_button() {
        assert_eq!(button_at(800, 2, 4.0, 4.0), Some(0));
        assert_eq!(button_at(800, 2, 203.9, 35.9), Some(0));
        // ボタンの間、上下の空き、時計の上は、どれでもない。
        assert_eq!(button_at(800, 2, 205.0, 20.0), None);
        assert_eq!(button_at(800, 2, 100.0, 3.9), None);
        assert_eq!(button_at(800, 2, 100.0, 36.0), None);
        assert_eq!(button_at(800, 2, 780.0, 20.0), None);
        assert_eq!(button_at(800, 2, 208.0, 20.0), Some(1));
    }

    #[test]
    fn pressing_a_button_restores_activates_or_minimizes() {
        let entry = |activated, minimized| Entry {
            title: String::new(),
            activated,
            minimized,
        };
        assert_eq!(action_for(&entry(false, false)), Action::Activate);
        assert_eq!(action_for(&entry(true, false)), Action::Minimize);
        assert_eq!(action_for(&entry(false, true)), Action::Restore);
    }

    #[test]
    fn the_clock_shows_hours_and_minutes_or_dashes() {
        // 2026-10-11T03:04:59Z。
        let now = Duration::from_secs(1_791_687_899);
        assert_eq!(clock_text(Some(now), 0), "03:04");
        assert_eq!(clock_text(Some(now), DEFAULT_UTC_OFFSET), "12:04");
        assert_eq!(clock_text(Some(now), -5 * 3600), "22:04");
        assert_eq!(clock_text(None, 0), "--:--");
        // 分が変わる少し後に起きる。時刻が無ければ 60 秒後。
        assert_eq!(
            until_next_minute(Some(now + Duration::from_millis(500))),
            Duration::from_millis(505)
        );
        assert_eq!(until_next_minute(None), Duration::from_secs(60));
    }

    #[test]
    fn the_wall_clock_on_this_machine_is_set() {
        assert!(wall_clock().is_some());
    }

    #[test]
    fn utc_offsets_are_read_within_the_range_in_steps_of_15_minutes() {
        assert_eq!(parse_utc_offset("+09:00"), Some(9 * 3600));
        assert_eq!(parse_utc_offset("-05:30"), Some(-(5 * 3600 + 1800)));
        assert_eq!(parse_utc_offset("+05:45"), Some(5 * 3600 + 45 * 60));
        assert_eq!(parse_utc_offset("+9"), Some(9 * 3600));
        assert_eq!(parse_utc_offset("+00:00"), Some(0));
        assert_eq!(parse_utc_offset("UTC"), Some(0));
        assert_eq!(parse_utc_offset("utc"), Some(0));
        // 範囲の端。
        assert_eq!(parse_utc_offset("+14:00"), Some(14 * 3600));
        assert_eq!(parse_utc_offset("-12:00"), Some(-12 * 3600));
        assert_eq!(parse_utc_offset("+14:15"), None);
        assert_eq!(parse_utc_offset("-12:15"), None);
        assert_eq!(parse_utc_offset("+15:00"), None);
        // 15 分刻みでない分と、読めない形。
        assert_eq!(parse_utc_offset("+05:20"), None);
        assert_eq!(parse_utc_offset("+05:60"), None);
        for bad in [
            "09:00", "", "+", "+:30", "+09:", "+009:00", "JST", "+9h", "UTC+9",
        ] {
            assert_eq!(parse_utc_offset(bad), None, "{bad}");
        }
        assert_eq!(format_utc_offset(9 * 3600), "+09:00");
        assert_eq!(format_utc_offset(-(5 * 3600 + 1800)), "-05:30");
        assert_eq!(format_utc_offset(0), "+00:00");
    }

    #[test]
    fn the_offset_comes_from_the_argument_the_config_tz_and_the_default() {
        let mut notes = Vec::new();
        let mut choose = |argument: Option<&str>, config: Option<i32>, tz: Option<&str>| {
            choose_offset(argument, config, tz, &mut notes)
        };
        assert_eq!(
            choose(None, None, None),
            (DEFAULT_UTC_OFFSET, "the default")
        );
        assert_eq!(choose(None, None, Some("UTC0")), (0, "TZ"));
        assert_eq!(
            choose(None, Some(-5 * 3600), Some("UTC0")),
            (-5 * 3600, CONFIG_FILE)
        );
        assert_eq!(
            choose(Some("UTC"), Some(-5 * 3600), None),
            (0, "--utc-offset")
        );
        // 読めない起動オプションは、理由を残して、次のものを使う。
        assert_eq!(
            choose(Some("+25:00"), None, None),
            (DEFAULT_UTC_OFFSET, "the default")
        );
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn the_config_file_sets_the_offset_and_reports_bad_lines() {
        let dir = std::env::temp_dir().join(format!("seinas-taskbar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
        std::fs::write(&path, "# 時計\nutc-offset = UTC\n").unwrap();
        assert_eq!(read_config(&path), (Some(0), Vec::new()));
        std::fs::write(&path, "utc-offset = -05:30\n").unwrap();
        assert_eq!(read_config(&path).0, Some(-(5 * 3600 + 1800)));
        // 読めない値は、無いものとして扱い(既定で動く)、理由を返す。
        std::fs::write(&path, "utc-offset = Asia/Tokyo\ncolor = red\n").unwrap();
        let (offset, notes) = read_config(&path);
        assert_eq!(offset, None);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("line 1"), "{notes:?}");
        // ファイルが無ければ、何も無い。
        assert_eq!(read_config(&dir.join("missing.conf")), (None, Vec::new()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_clock_shows_the_default_offset_and_utc() {
        // 2026-10-12 03:34:56 UTC。
        let now = Some(Duration::from_secs(1_791_776_096));
        assert_eq!(clock_text(now, DEFAULT_UTC_OFFSET), "12:34");
        assert_eq!(clock_text(now, parse_utc_offset("UTC").unwrap()), "03:34");
        assert_eq!(
            clock_text(now, parse_utc_offset("-05:30").unwrap()),
            "22:04"
        );
    }

    #[test]
    fn offsets_come_from_a_simple_tz() {
        assert_eq!(offset_from_tz("JST-9"), Some(9 * 3600));
        assert_eq!(offset_from_tz("<+0530>-5:30"), Some(5 * 3600 + 1800));
        assert_eq!(offset_from_tz("EST5"), Some(-5 * 3600));
        assert_eq!(offset_from_tz("UTC0"), Some(0));
        assert_eq!(offset_from_tz("UTC"), Some(0));
        // 夏時間のある形と、データベースの名前は読まない。
        assert_eq!(offset_from_tz("EST5EDT"), None);
        assert_eq!(offset_from_tz("Asia/Tokyo"), None);
        assert_eq!(offset_from_tz(":Asia/Tokyo"), None);
    }
}
