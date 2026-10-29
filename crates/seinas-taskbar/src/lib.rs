//! Seinas のタスクバーの、並べ方・描き方・時計(Wayland に触れない部分)。
//!
//! バイナリ(`src/main.rs`)が、layer-shell と foreign-toplevel のクライアントとして動き、ここの関数で
//! 描いて、ポインターの位置からトーチかタスクを選ぶ。決まりの説明は `docs/taskbar.md` と `docs/ui-spec.md` にある。

use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use seinas_icons::{fill_circle, fill_rect, fill_rounded_rect, Icon, IconCache};
use seinas_text::{Canvas, Line, TextPainter};

// 大きさと色は、テーマのトークン(`seinas-theme`)から読む。説明もそこにある。
pub use seinas_theme::taskbar::{
    ACTIVE_MARK, ACTIVE_MARK_WIDTH, BACKGROUND, CLOCK_DATE_TOP, CLOCK_GAP, CLOCK_LINE_HEIGHT,
    CLOCK_TIME_TOP, CLOCK_WIDTH, DOT, DOT_DIAMETER, HEIGHT, HIDDEN_ACTIVE_MARK, HIGHLIGHT_RADIUS,
    HIGHLIGHT_SIZE, HOT, ICON_SIZE, LEFT_MARGIN, LIST_ACTIVE_MARK, LIST_ACTIVE_MARK_WIDTH,
    LIST_BACKGROUND, LIST_BORDER, LIST_GAP, LIST_ICON_LEFT, LIST_MINIMIZED_TEXT, LIST_PADDING,
    LIST_ROW_HEIGHT, LIST_SELECTED, LIST_TEXT, LIST_TEXT_GAP, LIST_TEXT_RIGHT, LIST_TEXT_SIZE,
    LIST_WIDTH, MARK_GAP, MARK_HEIGHT, MIN_PITCH, MORE_DOT, MORE_DOT_DIAMETER, MORE_DOT_GAP, PITCH,
    PRESSED, RIGHT_MARGIN, TEXT, TEXT_SIZE, TOP_LINE,
};

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
    /// アイコンを探す名前(無ければ空)。
    pub app_id: String,
    pub activated: bool,
    pub minimized: bool,
}

/// タスクバーの上で、ポインターで選べるもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Torch,
    /// 並べたタスクの番号(並べた順)。
    Task(usize),
    /// 隠れたタスクをまとめる「…」のボタン。
    More,
}

/// トーチとタスクと「…」の並べ方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// まとまりの左の端と、アイコンの間隔(中心から中心)。
    pub left: i32,
    pub pitch: i32,
    /// 並べられたタスクの数。入りきらないタスク(番号が `tasks` から後ろ)は隠れ、「…」の一覧に出す。
    pub tasks: usize,
    /// 「…」のボタンを、まとまりの末尾に置くか(隠れたタスクがあるとき)。
    pub more: bool,
}

impl Layout {
    /// `index` 番目の場所(トーチが 0、タスク `i` が `i + 1`)の、光る範囲(左上と大きさ)。縦は中央。
    pub fn highlight_rect(&self, index: usize) -> (i32, i32, i32, i32) {
        let center = self.left + index as i32 * self.pitch + self.pitch / 2;
        (
            center - HIGHLIGHT_SIZE / 2,
            (HEIGHT - HIGHLIGHT_SIZE) / 2,
            HIGHLIGHT_SIZE,
            HIGHLIGHT_SIZE,
        )
    }

    /// `index` 番目の場所の、押したときの反応を受ける範囲(間隔の枠の全体。縦はタスクバーの高さの全体)。アイコンの
    /// 間に、押しても何も起きない隙間を作らない。
    pub fn slot_rect(&self, index: usize) -> (i32, i32, i32, i32) {
        (self.left + index as i32 * self.pitch, 0, self.pitch, HEIGHT)
    }

    /// `index` 番目の場所の、アイコンの左上。
    pub fn icon_origin(&self, index: usize) -> (i32, i32) {
        let center = self.left + index as i32 * self.pitch + self.pitch / 2;
        (center - ICON_SIZE / 2, (HEIGHT - ICON_SIZE) / 2)
    }

    /// 「…」の場所の番号(タスクの後ろ)。
    pub fn more_index(&self) -> usize {
        self.tasks + 1
    }

    /// 場所 `target` の番号。並べていないもの(隠れたタスク、無い「…」)はNone。
    pub fn index(&self, target: Target) -> Option<usize> {
        match target {
            Target::Torch => Some(0),
            Target::Task(i) => (i < self.tasks).then_some(i + 1),
            Target::More => self.more.then(|| self.more_index()),
        }
    }

    /// 点 (`x`, `y`) にある、トーチかタスクか「…」。トーチとタスクは間隔の枠の全体、「…」は光る範囲
    /// ([`HIGHLIGHT_SIZE`] の正方形)で受ける。
    pub fn target_at(&self, x: f64, y: f64) -> Option<Target> {
        let inside = |(left, top, w, h): (i32, i32, i32, i32)| {
            x >= left as f64 && x < (left + w) as f64 && y >= top as f64 && y < (top + h) as f64
        };
        if self.more && inside(self.highlight_rect(self.more_index())) {
            return Some(Target::More);
        }
        (0..=self.tasks)
            .find(|&index| inside(self.slot_rect(index)))
            .map(|index| match index {
                0 => Target::Torch,
                i => Target::Task(i - 1),
            })
    }
}

/// 幅が `width` のタスクバーに、トーチと `tasks` 個のタスクを並べる。
///
/// まとまり(トーチ → タスク → 「…」)は中央に置く。右の端の時計の場所([`CLOCK_WIDTH`] と空き)にかかるときは左へ
/// ずらし、それでも入らなければ、間隔を [`MIN_PITCH`] まで詰める。それでも全部は入らないときは、まず末尾に「…」の
/// 場所を取ってから、並べるタスクの数を数え直す。入らないタスク(後ろのもの)は隠れ、「…」の一覧に出す。前面の
/// ウィンドウのタスクが隠れても、見えるタスクと入れ替えない。
pub fn layout(width: i32, tasks: usize) -> Layout {
    let right = width - RIGHT_MARGIN - CLOCK_WIDTH - CLOCK_GAP;
    let available = (right - LEFT_MARGIN).max(0);
    let slots = tasks as i32 + 1;
    let (pitch, shown, more) = if slots * MIN_PITCH <= available {
        let pitch = if slots * PITCH <= available {
            PITCH
        } else {
            (available / slots).clamp(MIN_PITCH, PITCH)
        };
        (pitch, tasks, false)
    } else {
        // トーチと「…」の 2 つの場所を除いた残りに、タスクを並べる。「…」の場所も無いほど狭ければ、トーチだけ。
        let places = available / MIN_PITCH;
        let more = places >= 2;
        let shown = if more { places - 2 } else { 0 };
        (MIN_PITCH, shown as usize, more)
    };
    let group = (shown as i32 + 1 + i32::from(more)) * pitch;
    let left = ((width - group) / 2).min(right - group).max(LEFT_MARGIN);
    Layout {
        left,
        pitch,
        tasks: shown,
        more,
    }
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

/// 時計に出すもの。上の段に時刻、下の段に日付(無ければ時刻だけの 1 段)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    pub time: String,
    pub date: Option<String>,
}

impl Clock {
    /// 時刻が取れないときの表示(`--:--` の 1 段)。
    pub fn unknown() -> Self {
        Clock {
            time: NO_CLOCK.to_owned(),
            date: None,
        }
    }
}

/// ポインターの状態(乗っているものと、押しているもの)と、隠れたタスクの一覧を開いているか(開いている間は、
/// 「…」を押しているときと同じ地にする)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pointer {
    pub hot: Option<Target>,
    pub pressed: Option<Target>,
    pub list_open: bool,
}

/// タスクバーの絵を作る。幅は `width`、高さは [`HEIGHT`]。ピクセルの並びは B, G, R, X。
///
/// `icons` は、`entries` の app_id のアイコン(読んでいなければ既定のアイコン)、`torch` はトーチのアイコン。
pub fn paint(
    painter: &mut TextPainter,
    width: i32,
    entries: &[Entry],
    icons: &IconCache,
    torch: &Icon,
    pointer: Pointer,
    clock: &Clock,
) -> Vec<u8> {
    let width = width.max(1);
    let mut pixels = bgrx(BACKGROUND).repeat((width * HEIGHT) as usize);
    fill_rect(&mut pixels, width, (0, 0, width, 1), TOP_LINE);
    let layout = layout(width, entries.len());
    // 乗っている・押しているものの地。一覧を開いている間の「…」は、押しているときと同じ地。
    let open = pointer.list_open.then_some(Target::More);
    for (target, color) in [
        (pointer.hot, HOT),
        (pointer.pressed, PRESSED),
        (open, PRESSED),
    ] {
        if let Some(index) = target.and_then(|t| layout.index(t)) {
            fill_rounded_rect(
                &mut pixels,
                width,
                layout.highlight_rect(index),
                HIGHLIGHT_RADIUS,
                color,
            );
        }
    }
    // トーチ(印は付けない)。
    let (x, y) = layout.icon_origin(0);
    torch.draw_onto(&mut pixels, width, x, y);
    // タスク:アイコンと、前面(activated)は線、背面(最小化を含む)は点。
    let mark_top = HEIGHT - MARK_GAP - MARK_HEIGHT;
    for (index, entry) in entries.iter().take(layout.tasks).enumerate() {
        let (x, y) = layout.icon_origin(index + 1);
        icons.get(&entry.app_id).draw_onto(&mut pixels, width, x, y);
        let center = x + ICON_SIZE / 2;
        if entry.activated && !entry.minimized {
            fill_rect(
                &mut pixels,
                width,
                (
                    center - ACTIVE_MARK_WIDTH / 2,
                    mark_top,
                    ACTIVE_MARK_WIDTH,
                    MARK_HEIGHT,
                ),
                ACTIVE_MARK,
            );
        } else {
            fill_circle(
                &mut pixels,
                width,
                (center as f32, mark_top as f32 + MARK_HEIGHT as f32 / 2.0),
                DOT_DIAMETER,
                DOT,
            );
        }
    }
    if layout.more {
        paint_more(&mut pixels, width, &layout, &entries[layout.tasks..]);
    }
    paint_clock(painter, &mut pixels, width, clock);
    pixels
}

/// 「…」のボタン(3 つの点)を描く。隠れたタスクに前面のウィンドウのものがあれば、前面の線と同じ高さに、控えめな
/// オレンジの点を付ける。
fn paint_more(pixels: &mut [u8], width: i32, layout: &Layout, hidden: &[Entry]) {
    let (x, y) = layout.icon_origin(layout.more_index());
    let (cx, cy) = (x + ICON_SIZE / 2, y + ICON_SIZE / 2);
    for step in -1..=1 {
        fill_circle(
            pixels,
            width,
            ((cx + step * MORE_DOT_GAP) as f32, cy as f32),
            MORE_DOT_DIAMETER,
            MORE_DOT,
        );
    }
    if hidden.iter().any(|e| e.activated && !e.minimized) {
        let mark_top = HEIGHT - MARK_GAP - MARK_HEIGHT;
        fill_circle(
            pixels,
            width,
            (cx as f32, mark_top as f32 + MARK_HEIGHT as f32 / 2.0),
            DOT_DIAMETER,
            HIDDEN_ACTIVE_MARK,
        );
    }
}

/// 隠れたタスクの一覧の、全部の行を出せる高さ(行が `rows` のとき)。
pub fn list_height(rows: usize) -> i32 {
    rows as i32 * LIST_ROW_HEIGHT + 2 * LIST_PADDING
}

/// 高さが `height` の一覧で、行が `rows` のときの、スクロールの最大(ピクセル)。全部が見えるなら 0。
pub fn list_max_scroll(rows: usize, height: i32) -> i32 {
    (rows as i32 * LIST_ROW_HEIGHT - (height - 2 * LIST_PADDING)).max(0)
}

/// 一覧の中の高さ `y` にある行(スクロールが `scroll` のとき)。上下の空きの上と、行の無い所はNone。
pub fn list_row_at(y: f64, rows: usize, height: i32, scroll: i32) -> Option<usize> {
    if y < LIST_PADDING as f64 || y >= (height - LIST_PADDING) as f64 {
        return None;
    }
    let row = ((y - LIST_PADDING as f64 + scroll as f64) / LIST_ROW_HEIGHT as f64) as usize;
    (row < rows).then_some(row)
}

/// 行 `row` が見えるスクロール(今のスクロール `scroll` から、いちばん少なく動かす)。
pub fn list_scroll_to(row: usize, rows: usize, height: i32, scroll: i32) -> i32 {
    let visible = height - 2 * LIST_PADDING;
    let top = row as i32 * LIST_ROW_HEIGHT;
    let scroll = if top < scroll {
        top
    } else if top + LIST_ROW_HEIGHT > scroll + visible {
        top + LIST_ROW_HEIGHT - visible
    } else {
        scroll
    };
    scroll.clamp(0, list_max_scroll(rows, height))
}

/// 一覧の中で、選ぶ行を動かすキー。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// キー `key` で、選んでいる行 `selected` を動かした後の行(行が `rows`、高さが `height` のとき)。選んでいる行が
/// 無ければ、下へは先頭、上へは末尾から。行が無ければNone。
pub fn list_move(selected: Option<usize>, rows: usize, height: i32, key: ListKey) -> Option<usize> {
    let last = rows.checked_sub(1)?;
    let page = (((height - 2 * LIST_PADDING) / LIST_ROW_HEIGHT).max(1)) as usize;
    Some(match (key, selected) {
        (ListKey::Home, _) => 0,
        (ListKey::End, _) => last,
        (ListKey::Down, None) | (ListKey::PageDown, None) => 0,
        (ListKey::Up, None) | (ListKey::PageUp, None) => last,
        (ListKey::Down, Some(row)) => (row + 1).min(last),
        (ListKey::Up, Some(row)) => row.saturating_sub(1),
        (ListKey::PageDown, Some(row)) => (row + page).min(last),
        (ListKey::PageUp, Some(row)) => row.saturating_sub(page),
    })
}

/// 隠れたタスクの一覧の絵を作る。幅は [`LIST_WIDTH`]、高さは `height`。ピクセルの並びは B, G, R, X。
///
/// 行は `entries` の順。各々の行に、アイコンとタイトル(最小化しているものは補助の文字の色)を出し、前面の
/// ウィンドウの行には、左の端にオレンジの縦の線を付ける。`selected` の行の地は、選んでいる色。`scroll` だけ、
/// 行を上へずらして出す(上下の空きには、行を描かない)。
pub fn paint_list(
    painter: &mut TextPainter,
    height: i32,
    entries: &[Entry],
    icons: &IconCache,
    selected: Option<usize>,
    scroll: i32,
) -> Vec<u8> {
    let width = LIST_WIDTH;
    let height = height.max(2 * LIST_PADDING + 1);
    let mut pixels = bgrx(LIST_BACKGROUND).repeat((width * height) as usize);
    // 行は、全部の行の高さの別の絵に描いてから、見える所だけをコピーする。
    let rows_height = (entries.len() as i32 * LIST_ROW_HEIGHT).max(1);
    let mut rows = bgrx(LIST_BACKGROUND).repeat((width * rows_height) as usize);
    let text_left = LIST_ICON_LEFT + ICON_SIZE + LIST_TEXT_GAP;
    for (row, entry) in entries.iter().enumerate() {
        let top = row as i32 * LIST_ROW_HEIGHT;
        if selected == Some(row) {
            fill_rect(
                &mut rows,
                width,
                (1, top, width - 2, LIST_ROW_HEIGHT),
                LIST_SELECTED,
            );
        }
        let icon_top = top + (LIST_ROW_HEIGHT - ICON_SIZE) / 2;
        if entry.activated && !entry.minimized {
            fill_rect(
                &mut rows,
                width,
                (2, icon_top, LIST_ACTIVE_MARK_WIDTH, ICON_SIZE),
                LIST_ACTIVE_MARK,
            );
        }
        icons
            .get(&entry.app_id)
            .draw_onto(&mut rows, width, LIST_ICON_LEFT, icon_top);
        painter.draw_line(
            &mut Canvas::new(&mut rows, width as usize, rows_height as usize),
            &entry.title,
            Line {
                size: LIST_TEXT_SIZE,
                color: if entry.minimized {
                    LIST_MINIMIZED_TEXT
                } else {
                    LIST_TEXT
                },
                x: text_left,
                top,
                height: LIST_ROW_HEIGHT,
                max_width: width - text_left - LIST_TEXT_RIGHT,
            },
        );
    }
    let visible = height - 2 * LIST_PADDING;
    let scroll = scroll.clamp(0, list_max_scroll(entries.len(), height));
    let stride = (width * 4) as usize;
    for y in 0..visible.min(rows_height - scroll) {
        let from = ((scroll + y) as usize) * stride;
        let to = ((LIST_PADDING + y) as usize) * stride;
        pixels[to..to + stride].copy_from_slice(&rows[from..from + stride]);
    }
    // 外の枠(1 ピクセル)。
    for rect in [
        (0, 0, width, 1),
        (0, height - 1, width, 1),
        (0, 0, 1, height),
        (width - 1, 0, 1, height),
    ] {
        fill_rect(&mut pixels, width, rect, LIST_BORDER);
    }
    pixels
}

/// 時計を右の端に描く。2 段(時刻と日付)は、左の端をそろえ、広いほうの右の端を [`RIGHT_MARGIN`] の内に置く。
fn paint_clock(painter: &mut TextPainter, pixels: &mut [u8], width: i32, clock: &Clock) {
    let line = |x, top, height| Line {
        size: TEXT_SIZE,
        color: TEXT,
        x,
        top,
        height,
        max_width: CLOCK_WIDTH,
    };
    let mut lines = vec![(clock.time.as_str(), CLOCK_TIME_TOP, CLOCK_LINE_HEIGHT)];
    match &clock.date {
        Some(date) => lines.push((date.as_str(), CLOCK_DATE_TOP, CLOCK_LINE_HEIGHT)),
        // 1 段なら、高さの全体の中で縦の中央。
        None => lines[0] = (clock.time.as_str(), 0, HEIGHT),
    }
    // 幅は、別の場所に一度描いて測る。
    let mut scratch = vec![0u8; (CLOCK_WIDTH * HEIGHT * 4) as usize];
    let widest = lines
        .iter()
        .map(|&(text, top, height)| {
            painter
                .draw_line(
                    &mut Canvas::new(&mut scratch, CLOCK_WIDTH as usize, HEIGHT as usize),
                    text,
                    line(0, top, height),
                )
                .width
        })
        .max()
        .unwrap_or(0);
    let x = width - RIGHT_MARGIN - widest;
    for (text, top, height) in lines {
        painter.draw_line(
            &mut Canvas::new(pixels, width as usize, HEIGHT as usize),
            text,
            line(x, top, height),
        );
    }
}

fn bgrx(color: [u8; 3]) -> [u8; 4] {
    [color[2], color[1], color[0], 0xff]
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

/// 日付の表示(`YYYY/MM/DD`)。`now` は UNIX 時刻、`offset` は時差(秒)。
pub fn date_text(now: Duration, offset: i32) -> String {
    let local = now.as_secs() as i64 + offset as i64;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    format!("{year:04}/{month:02}/{day:02}")
}

/// 時計に出すもの(時刻と日付の 2 段)。時刻が無ければ `--:--` の 1 段。
pub fn clock_for(now: Option<Duration>, offset: i32) -> Clock {
    match now {
        Some(now) => Clock {
            time: clock_text(Some(now), offset),
            date: Some(date_text(now, offset)),
        },
        None => Clock::unknown(),
    }
}

/// 1970-01-01 からの日数を、グレゴリオ暦の年・月・日にする(Howard Hinnant の civil_from_days の考え方)。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
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
    fn the_torch_and_the_tasks_are_centered() {
        // 800 の幅に、トーチとタスク 2 つ:3 × 48 = 144 を中央に。
        let three = layout(800, 2);
        assert_eq!(
            three,
            Layout {
                left: 328,
                pitch: 48,
                tasks: 2,
                more: false
            }
        );
        // 光る範囲は 36 × 36 で、縦は中央。押す範囲は間隔の枠の全体。アイコンは 24 × 24。
        assert_eq!(three.highlight_rect(0), (334, 2, 36, 36));
        assert_eq!(three.slot_rect(1), (376, 0, 48, 40));
        assert_eq!(three.icon_origin(1), (388, 8));
        // タスクが無くても、トーチは中央。
        assert_eq!(layout(800, 0).left, 376);
    }

    #[test]
    fn a_long_group_moves_left_and_then_gets_narrower() {
        // 右の端の時計の場所は 14 + 100 + 8 = 122。そこにかかるときは、左へずらす。
        let reserved = RIGHT_MARGIN + CLOCK_WIDTH + CLOCK_GAP;
        assert_eq!(reserved, 122);
        let shifted = layout(480, 5);
        assert_eq!(shifted.pitch, 48);
        assert_eq!(shifted.left + 6 * 48, 480 - reserved);
        // それでも入らなければ、間隔を詰める(36 まで)。
        let narrower = layout(400, 6);
        assert_eq!(narrower.pitch, (400 - reserved - LEFT_MARGIN) / 7);
        assert_eq!(narrower.left + 7 * narrower.pitch, 400 - reserved);
        assert_eq!(narrower.tasks, 6);
        // 36 でも入らないときは、まず末尾に「…」の場所を取り、残りに並べる(後ろのタスクは隠れる)。
        let narrow = layout(320, 10);
        assert_eq!(narrow.pitch, 36);
        assert!(narrow.more);
        assert_eq!(
            narrow.tasks,
            (320 - reserved - LEFT_MARGIN) as usize / 36 - 2
        );
        assert_eq!(narrow.left + (narrow.tasks as i32 + 2) * 36, 320 - reserved);
        // ちょうど入るときは、「…」を出さない。
        let fits = (320 - reserved - LEFT_MARGIN) as usize / 36 - 1;
        assert!(!layout(320, fits).more);
        assert!(layout(320, fits + 1).more);
        assert_eq!(layout(320, fits + 1).tasks, fits - 1);
        // とても狭くても、落ちない(タスクも「…」も出さない)。
        assert_eq!(layout(80, 3).tasks, 0);
        assert!(!layout(80, 3).more);
    }

    #[test]
    fn how_many_tasks_fit_at_1280_and_1920() {
        // 並べられる幅は、幅 - 左の空き 8 - 右の 122。1280 では 1150、1920 では 1790。
        // 1280:間隔 48 のままなら 1150 / 48 = 23 の場所(トーチを除いて 22 のタスク)。詰めると 1150 / 36 = 31 の
        // 場所(30 のタスク)まで。31 目からは出さない。
        assert_eq!(layout(1280, 22).pitch, 48);
        assert_eq!(layout(1280, 23).pitch, 47);
        assert_eq!(layout(1280, 30).tasks, 30);
        assert!(!layout(1280, 30).more);
        // 31 からは、「…」の場所を取って 29 を並べ、残りは隠れる。
        assert_eq!(layout(1280, 31).tasks, 29);
        assert!(layout(1280, 31).more);
        // 1920:間隔 48 のままなら 1790 / 48 = 37 の場所(36 のタスク)。詰めると 1790 / 36 = 49 の場所(48 の
        // タスク)まで。
        assert_eq!(layout(1920, 36).pitch, 48);
        assert!(layout(1920, 37).pitch < 48);
        assert_eq!(layout(1920, 48).tasks, 48);
        assert_eq!(layout(1920, 49).tasks, 47);
        assert!(layout(1920, 49).more);
    }

    #[test]
    fn a_point_selects_the_torch_or_a_task() {
        let three = layout(800, 2);
        // 押す範囲は、間隔の枠の全体(縦はタスクバーの高さの全体)。アイコンの間に隙間は無い。
        assert_eq!(three.target_at(328.0, 0.0), Some(Target::Torch));
        assert_eq!(three.target_at(375.9, 39.9), Some(Target::Torch));
        assert_eq!(three.target_at(376.0, 20.0), Some(Target::Task(0)));
        assert_eq!(three.target_at(471.9, 20.0), Some(Target::Task(1)));
        // まとまりの外(左右)と、時計の上は、どれでもない。
        assert_eq!(three.target_at(327.9, 20.0), None);
        assert_eq!(three.target_at(472.0, 20.0), None);
        assert_eq!(three.target_at(400.0, 40.0), None);
        assert_eq!(three.target_at(760.0, 20.0), None);
    }

    #[test]
    fn the_more_button_is_hit_only_inside_its_highlight() {
        // 400 の幅に 10 のタスク:並べられる幅は 270(7 つの場所)。トーチ、タスク 5 つ、「…」。
        let many = layout(400, 10);
        assert_eq!((many.pitch, many.tasks, many.more), (36, 5, true));
        assert_eq!(many.more_index(), 6);
        let (x, y, w, h) = many.highlight_rect(6);
        assert_eq!((w, h), (HIGHLIGHT_SIZE, HIGHLIGHT_SIZE));
        let (x, y) = (x as f64, y as f64);
        assert_eq!(many.target_at(x, y), Some(Target::More));
        assert_eq!(many.target_at(x + 35.9, y + 35.9), Some(Target::More));
        // 光る範囲の外(上下の端)は、どれでもない。
        assert_eq!(many.target_at(x + 10.0, y - 0.1), None);
        assert_eq!(many.target_at(x + 10.0, y + 36.0), None);
        // 隠れたタスクと、無い「…」には、場所が無い。
        assert_eq!(many.index(Target::Task(4)), Some(5));
        assert_eq!(many.index(Target::Task(5)), None);
        assert_eq!(layout(800, 2).index(Target::More), None);
    }

    #[test]
    fn the_list_scrolls_to_show_a_row_and_moves_with_keys() {
        // 行が 10、全部の高さは 408。高さ 168(見える行は 4)なら、スクロールは 0〜240。
        assert_eq!(list_height(10), 408);
        assert_eq!(list_max_scroll(10, 408), 0);
        assert_eq!(list_max_scroll(10, 168), 240);
        // 行のヒット:上下の空き(4)の上は無し。
        assert_eq!(list_row_at(3.9, 10, 168, 0), None);
        assert_eq!(list_row_at(4.0, 10, 168, 0), Some(0));
        assert_eq!(list_row_at(44.0, 10, 168, 0), Some(1));
        assert_eq!(list_row_at(4.0, 10, 168, 80), Some(2));
        assert_eq!(list_row_at(164.0, 10, 168, 0), None);
        // 見えない行を選ぶと、いちばん少なく動かして見せる。
        assert_eq!(list_scroll_to(5, 10, 168, 0), 80);
        assert_eq!(list_scroll_to(1, 10, 168, 80), 40);
        assert_eq!(list_scroll_to(3, 10, 168, 80), 80);
        assert_eq!(list_scroll_to(9, 10, 168, 0), 240);
        // キー:先頭・末尾で止まる。選んでいなければ、下は先頭、上は末尾から。ページは見える行の数。
        assert_eq!(list_move(None, 10, 168, ListKey::Down), Some(0));
        assert_eq!(list_move(None, 10, 168, ListKey::Up), Some(9));
        assert_eq!(list_move(Some(9), 10, 168, ListKey::Down), Some(9));
        assert_eq!(list_move(Some(0), 10, 168, ListKey::Up), Some(0));
        assert_eq!(list_move(Some(2), 10, 168, ListKey::PageDown), Some(6));
        assert_eq!(list_move(Some(2), 10, 168, ListKey::PageUp), Some(0));
        assert_eq!(list_move(Some(2), 10, 168, ListKey::End), Some(9));
        assert_eq!(list_move(Some(2), 10, 168, ListKey::Home), Some(0));
        assert_eq!(list_move(None, 0, 168, ListKey::Down), None);
    }

    #[test]
    fn pressing_a_button_restores_activates_or_minimizes() {
        let entry = |activated, minimized| Entry {
            title: String::new(),
            app_id: String::new(),
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
        // 日付と、2 段の時計。時差で日付が変わる。
        assert_eq!(date_text(now, 0), "2026/10/11");
        assert_eq!(date_text(now, -5 * 3600), "2026/10/10");
        assert_eq!(
            clock_for(Some(now), DEFAULT_UTC_OFFSET),
            Clock {
                time: "12:04".to_owned(),
                date: Some("2026/10/11".to_owned())
            }
        );
        assert_eq!(clock_for(None, 0), Clock::unknown());
        // うるう年と、年の境。
        assert_eq!(date_text(Duration::from_secs(951_782_400), 0), "2000/02/29");
        assert_eq!(
            date_text(Duration::from_secs(1_798_761_599), 0),
            "2026/12/31"
        );
        assert_eq!(
            date_text(Duration::from_secs(1_798_761_600), 0),
            "2027/01/01"
        );
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
