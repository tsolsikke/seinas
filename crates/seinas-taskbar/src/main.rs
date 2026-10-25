//! seinas-taskbar: Seinas のタスクバー。
//!
//! コンポジタとは別のプログラムで、画面の下に layer surface(wlr-layer-shell)を出し、foreign-toplevel
//! (wlr-foreign-toplevel-management)で、開いているウィンドウの一覧を受け取ってボタンを並べる。右の端に
//! 時計を出す。シェルの部品のためのソケットへつなぐこと(`WAYLAND_DISPLAY=seinas-0-shell`)。
//!
//! 引数:
//!
//! - `--fonts DIR`:同梱のフォントの置き場(無ければ環境変数 `SEINAS_FONTS`、それも無ければ `target/fonts`)。
//! - `--font-config PATH`:フォントの設定のファイル(無ければ `$XDG_CONFIG_HOME/seinas/fonts.conf`。ADR 0003)。
//! - `--utc-offset ±HH:MM`(か `UTC`):時計の時差(無ければ設定のファイルの `utc-offset`、それも無ければ環境変数
//!   `TZ` の簡単な形、それも無ければ +09:00)。
//! - `--config PATH`:タスクバーの設定のファイル(無ければ `$XDG_CONFIG_HOME/seinas/taskbar.conf`)。
//! - `--connect-timeout MS`:コンポジタのソケットがまだ無いときに、最初の接続を試し直す期限(ミリ秒。無ければ
//!   [`CONNECT_TIMEOUT`])。間隔は [`CONNECT_INTERVAL`]。
//! - `--round-trip-timeout MS`:つないだ後の最初の往復の答えを待つ期限(ミリ秒。無ければ [`ROUND_TRIP_TIMEOUT`])。
//!   どちらもテストで短くするためのもの。
//! - `--fixed-time SECONDS`:時計を、この UNIX 時刻に止める(正解の画像を取るためのもの。時計は進まない)。
//!
//! アプリのアイコンは、データの置き場(`XDG_DATA_HOME`・`XDG_DATA_DIRS`)から探して読む(`seinas-icons`。ADR 0007)。
//!
//! 決まりの説明は `docs/taskbar.md` にある。

use std::{
    error::Error,
    fmt::Display,
    io,
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use seinas_icons::{Found, Icon, IconCache};
use seinas_taskbar::{
    action_for, choose_offset, clock_for, format_utc_offset, layout, not_ready_yet, paint,
    read_config, socket_path, until_next_minute, wall_clock, Action, Clock, Entry, Pointer, Target,
    CONFIG_FILE, CONNECT_INTERVAL, CONNECT_TIMEOUT, HEIGHT, ICON_SIZE, ROUND_TRIP_TIMEOUT,
};
use seinas_text::{FontSetup, TextPainter};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use wayland_client::{
    backend::{ObjectData, WaylandError},
    globals::registry_queue_init,
    protocol::{wl_callback, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// 左のボタンの番号(Linux の入力の決まり)。
const LEFT_BUTTON: u32 = 0x110;
/// フォントの置き場を指定する環境変数と、指定が無いときの場所(コンポジタと同じ)。
const FONTS_ENV: &str = "SEINAS_FONTS";
const DEFAULT_FONT_DIR: &str = "target/fonts";

/// 一覧の中の、1 つのウィンドウ。
struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    /// done で確定した見え方。
    entry: Entry,
    /// done までに届いたもの。
    title: Option<String>,
    app_id: Option<String>,
    states: Option<(bool, bool)>,
    /// 一度でも done が届いたか。届くまでは並べない。
    shown: bool,
}

struct Taskbar {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    buffer: Option<Buffer>,
    qh: QueueHandle<Taskbar>,
    /// コンポジタが決めた幅。最初の configure までは 0。
    width: i32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    pointer: Option<wl_pointer::WlPointer>,
    /// ポインターが乗っているものと、押しているもの。
    hot: Pointer,
    toplevels: Vec<Toplevel>,
    text: TextPainter,
    /// アプリのアイコン(app_id ごとに 1 回だけ探す)と、トーチのアイコン。
    icons: IconCache,
    torch: Icon,
    /// 時計の時差(秒)と、いま出しているもの。`fixed_time` があれば、時計はその時刻に止める。
    offset: i32,
    clock: Clock,
    fixed_time: Option<Duration>,
}

impl Taskbar {
    /// 並べるウィンドウ(done が届いたもの)。
    fn entries(&self) -> Vec<Entry> {
        self.toplevels
            .iter()
            .filter(|t| t.shown)
            .map(|t| t.entry.clone())
            .collect()
    }

    fn shown(&self) -> Vec<&Toplevel> {
        self.toplevels.iter().filter(|t| t.shown).collect()
    }

    /// 再描画が要ることを覚え、描けるなら描く。
    fn redraw(&mut self) {
        self.dirty = true;
        self.draw();
    }

    /// 絵を描いて出す。最初の configure の前と、frame callback を待っている間は描かない。
    fn draw(&mut self) {
        if !self.configured || self.frame_pending || self.width <= 0 {
            return;
        }
        let (width, stride) = (self.width, self.width * 4);
        let entries = self.entries();
        let pixels = paint(
            &mut self.text,
            width,
            &entries,
            &self.icons,
            &self.torch,
            self.hot,
            &self.clock,
        );
        let buffer = match self.buffer.take() {
            Some(buffer) if buffer.height() == HEIGHT && buffer.stride() == stride => buffer,
            _ => match self
                .pool
                .create_buffer(width, HEIGHT, stride, wl_shm::Format::Xrgb8888)
            {
                Ok((buffer, _)) => buffer,
                Err(e) => {
                    eprintln!("seinas-taskbar: cannot create a buffer: {e}");
                    return;
                }
            },
        };
        let buffer = match self.pool.canvas(&buffer) {
            Some(canvas) => {
                canvas.copy_from_slice(&pixels);
                buffer
            }
            None => {
                // コンポジタがまだ前のバッファを使っている。新しいバッファを取る。
                match self
                    .pool
                    .create_buffer(width, HEIGHT, stride, wl_shm::Format::Xrgb8888)
                {
                    Ok((second, canvas)) => {
                        canvas.copy_from_slice(&pixels);
                        second
                    }
                    Err(e) => {
                        eprintln!("seinas-taskbar: cannot create a buffer: {e}");
                        return;
                    }
                }
            }
        };
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, width, HEIGHT);
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("seinas-taskbar: cannot attach the buffer: {e}");
            return;
        }
        self.layer.commit();
        self.buffer = Some(buffer);
        self.frame_pending = true;
        self.dirty = false;
    }

    /// いまの時刻(止めていれば、その時刻)。
    fn now(&self) -> Option<Duration> {
        self.fixed_time.or_else(wall_clock)
    }

    /// 時計を読み直す。表示が変わったら再描画する。
    fn update_clock(&mut self) {
        let clock = clock_for(self.now(), self.offset);
        if clock != self.clock {
            self.clock = clock;
            self.redraw();
        }
    }

    /// ボタン `index` が押された。
    fn press(&mut self, index: usize) {
        let Some(toplevel) = self
            .shown()
            .get(index)
            .map(|t| (t.handle.clone(), t.entry.clone()))
        else {
            return;
        };
        let (handle, entry) = toplevel;
        match action_for(&entry) {
            Action::Restore => {
                handle.unset_minimized();
                self.activate(&handle);
            }
            Action::Activate => self.activate(&handle),
            Action::Minimize => handle.set_minimized(),
        }
    }

    fn activate(&self, handle: &ZwlrForeignToplevelHandleV1) {
        // seat は 1 つ(Seinas の seat)。起動したときからある seat も、ここで分かる。
        match self.seat_state.seats().next() {
            Some(seat) => handle.activate(&seat),
            None => eprintln!("seinas-taskbar: no seat to activate a window with"),
        }
    }
}

impl CompositorHandler for Taskbar {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw();
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Taskbar {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Taskbar {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        eprintln!("seinas-taskbar: the compositor closed the taskbar");
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let width = configure.new_size.0 as i32;
        if width != self.width {
            self.width = width;
            self.dirty = true;
        }
        if !self.configured {
            self.configured = true;
            self.dirty = true;
        }
        if self.dirty {
            self.draw();
        }
    }
}

impl SeatHandler for Taskbar {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(e) => eprintln!("seinas-taskbar: cannot use the pointer: {e}"),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Taskbar {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            let count = self.shown().len();
            let (x, y) = event.position;
            let mut pointer = self.hot;
            pointer.hot = match event.kind {
                Leave { .. } => None,
                _ => layout(self.width, count).target_at(x, y),
            };
            match event.kind {
                Press {
                    button: LEFT_BUTTON,
                    ..
                } => pointer.pressed = pointer.hot,
                Release {
                    button: LEFT_BUTTON,
                    ..
                }
                | Leave { .. } => pointer.pressed = None,
                _ => {}
            }
            if pointer != self.hot {
                self.hot = pointer;
                self.redraw();
            }
            if let (
                Press {
                    button: LEFT_BUTTON,
                    ..
                },
                Some(target),
            ) = (&event.kind, pointer.hot)
            {
                match target {
                    // メニューはまだ無い。
                    Target::Torch => {
                        eprintln!("seinas-taskbar: the torch was pressed (no menu yet)")
                    }
                    Target::Task(index) => self.press(index),
                }
            }
        }
    }
}

impl ShmHandler for Taskbar {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// foreign-toplevel の manager のデータ。
struct ManagerData;

impl Dispatch2<ZwlrForeignToplevelManagerV1, Taskbar> for ManagerData {
    fn event(
        &self,
        state: &mut Taskbar,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<Taskbar>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.toplevels.push(Toplevel {
                    handle: toplevel,
                    entry: Entry::default(),
                    title: None,
                    app_id: None,
                    states: None,
                    shown: false,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                eprintln!("seinas-taskbar: the compositor finished the window list");
            }
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<Taskbar>) -> Arc<dyn ObjectData> {
        match opcode {
            zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => {
                qh.make_data::<ZwlrForeignToplevelHandleV1, _>(HandleData)
            }
            _ => panic!("unexpected child of zwlr_foreign_toplevel_manager_v1: {opcode}"),
        }
    }
}

/// foreign-toplevel の handle のデータ。
struct HandleData;

impl Dispatch2<ZwlrForeignToplevelHandleV1, Taskbar> for HandleData {
    fn event(
        &self,
        state: &mut Taskbar,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Taskbar>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event, State};
        let Some(index) = state.toplevels.iter().position(|t| &t.handle == handle) else {
            return;
        };
        let toplevel = &mut state.toplevels[index];
        match event {
            Event::Title { title } => toplevel.title = Some(title),
            Event::AppId { app_id } => toplevel.app_id = Some(app_id),
            Event::State { state: bytes } => {
                let states: Vec<u32> = bytes
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                toplevel.states = Some((
                    states.contains(&(State::Activated as u32)),
                    states.contains(&(State::Minimized as u32)),
                ));
            }
            Event::Done => {
                let title = toplevel.title.clone().unwrap_or_default();
                let app_id = toplevel.app_id.clone().unwrap_or_default();
                let (activated, minimized) = toplevel.states.unwrap_or_default();
                toplevel.entry = Entry {
                    // タイトルが無ければ app_id。
                    title: if title.is_empty() {
                        app_id.clone()
                    } else {
                        title
                    },
                    app_id: app_id.clone(),
                    activated,
                    minimized,
                };
                toplevel.shown = true;
                // アイコンは、app_id ごとに初めてのときだけ探して読む(`seinas-icons`)。
                match state.icons.load(&app_id) {
                    Some(Found::File(path)) => {
                        eprintln!("seinas-taskbar: the icon of {app_id}: {}", path.display())
                    }
                    Some(Found::Unreadable(path, reason)) => eprintln!(
                        "seinas-taskbar: the icon of {app_id} ({}) is not used: {reason}; the default icon is used",
                        path.display()
                    ),
                    Some(Found::Missing) => eprintln!(
                        "seinas-taskbar: no icon for {app_id}; the default icon is used"
                    ),
                    None => {}
                }
                state.redraw();
            }
            Event::Closed => {
                handle.destroy();
                state.toplevels.remove(index);
                // 順が変わったので、ポインターの下のものは、次に動いたときに決め直す。
                state.hot = Pointer::default();
                state.redraw();
            }
            _ => {}
        }
    }
}

/// 起動のときの指定。
struct Options {
    fonts: PathBuf,
    /// フォントの設定のファイル(`--font-config`)。Noneなら、既定の場所(ADR 0003)。
    font_config: Option<PathBuf>,
    /// `--utc-offset` の値(読むのは、設定のファイルと合わせて決めるとき)。
    utc_offset: Option<String>,
    /// タスクバーの設定のファイル(`--config`)。Noneなら、既定の場所。
    config: Option<PathBuf>,
    /// 最初の接続を試し直す期限(`--connect-timeout`)と、最初の往復を待つ期限(`--round-trip-timeout`)。
    connect_timeout: Duration,
    round_trip_timeout: Duration,
    /// 時計を止める時刻(`--fixed-time`。UNIX 時刻)。
    fixed_time: Option<Duration>,
}

fn parse_options(args: &[String], fonts_env: Option<String>) -> Result<Options, String> {
    let mut fonts = None;
    let mut font_config = None;
    let mut utc_offset = None;
    let mut config = None;
    let mut connect_timeout = CONNECT_TIMEOUT;
    let mut round_trip_timeout = ROUND_TRIP_TIMEOUT;
    let mut fixed_time = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--fonts" => fonts = Some(PathBuf::from(value()?)),
            "--font-config" => font_config = Some(PathBuf::from(value()?)),
            "--utc-offset" => utc_offset = Some(value()?),
            "--config" => config = Some(PathBuf::from(value()?)),
            "--connect-timeout" => connect_timeout = millis(arg, &value()?)?,
            "--round-trip-timeout" => round_trip_timeout = millis(arg, &value()?)?,
            "--fixed-time" => {
                let text = value()?;
                let seconds = text
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --fixed-time value: {text}"))?;
                fixed_time = Some(Duration::from_secs(seconds));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Options {
        fonts: fonts
            .or_else(|| fonts_env.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_FONT_DIR)),
        font_config,
        utc_offset,
        config,
        connect_timeout,
        round_trip_timeout,
        fixed_time,
    })
}

/// 期限の引数の値(ミリ秒。1 以上)。
fn millis(arg: &str, text: &str) -> Result<Duration, String> {
    text.parse()
        .ok()
        .filter(|&ms| ms >= 1)
        .map(Duration::from_millis)
        .ok_or(format!("invalid {arg} value: {text}"))
}

/// コンポジタへつなぐ。コンポジタがまだ受けられないとき([`not_ready_yet`])は、[`CONNECT_INTERVAL`] ずつ眠って
/// (`std::thread::sleep`。musl では clock_nanosleep)、最初の試しから `timeout` の間、試し直す(単調増加の時計で
/// 判定する)。起動の順序で、コンポジタより先に起こされることがあるため(`docs/taskbar.md` の「起動の順序」)。
/// 試し直すのは、この最初の接続だけ(つないだ後に切れたら、終わる)。
fn connect(timeout: Duration) -> Result<Connection, Box<dyn Error>> {
    // 親が接続を渡してきたときは、試し直す相手がいない。
    if std::env::var_os("WAYLAND_SOCKET").is_some() {
        return Ok(Connection::connect_to_env()
            .map_err(|e| format!("cannot connect to the compositor (WAYLAND_SOCKET): {e}"))?);
    }
    let path = socket_path(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
    )
    .map_err(|e| format!("cannot connect to the compositor: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut tried = 0;
    let stream = loop {
        tried += 1;
        let e = match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e) => e,
        };
        let Some(reason) = not_ready_yet(&e) else {
            return Err(format!(
                "cannot connect to the compositor at {}: {e}",
                path.display()
            )
            .into());
        };
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "the compositor did not appear at {} within {} ms ({reason}: {e}; tried {tried} times)",
                path.display(),
                timeout.as_millis()
            )
            .into());
        }
        if tried == 1 {
            eprintln!(
                "seinas-taskbar: the compositor is not ready at {} ({reason}: {e}); trying again every {} ms for up to {} ms",
                path.display(),
                CONNECT_INTERVAL.as_millis(),
                timeout.as_millis()
            );
        }
        std::thread::sleep(CONNECT_INTERVAL.min(deadline - now));
    };
    if tried > 1 {
        eprintln!("seinas-taskbar: connected to the compositor after {tried} tries");
    }
    Ok(Connection::from_socket(stream).map_err(|e| {
        format!(
            "cannot connect to the compositor at {}: {e}",
            path.display()
        )
    })?)
}

/// 最初の往復(`wl_display.sync`)の答えが届いたか。
struct FirstRoundTrip {
    done: bool,
}

impl Dispatch<wl_callback::WlCallback, ()> for FirstRoundTrip {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.done = true;
        }
    }
}

/// 初期化の段階 `stage` で接続が失敗したことを、元の原因つきの文にする。プロトコルエラーなら、その中身を書く。
fn failed_during(conn: &Connection, stage: &str, error: impl Display) -> String {
    let cause = match conn.protocol_error() {
        Some(e) => format!(
            "protocol error {} on {}@{}: {}",
            e.code, e.object_interface, e.object_id, e.message
        ),
        None => error.to_string(),
    };
    format!("the connection to the compositor failed during {stage}: {cause}")
}

/// つないだ後、同じ接続で最初の往復(`wl_display.sync`)の答えを、`timeout` まで待つ(単調増加の時計で判定する)。
///
/// wayland-client の `registry_queue_init` の往復(`Connection::roundtrip`)には期限が無いので、その前に、期限つきで
/// 1 回往復する。`prepare_read` と、期限つきの `poll` で待つ。切断・プロトコルエラーは、試し直さずにすぐ返す。
fn first_round_trip(conn: &Connection, timeout: Duration) -> Result<(), String> {
    const STAGE: &str = "the first round trip";
    let deadline = Instant::now() + timeout;
    let mut queue = conn.new_event_queue::<FirstRoundTrip>();
    conn.display().sync(&queue.handle(), ());
    let mut state = FirstRoundTrip { done: false };
    let mut waited = false;
    loop {
        queue
            .dispatch_pending(&mut state)
            .map_err(|e| failed_during(conn, STAGE, e))?;
        if state.done {
            if waited {
                eprintln!("seinas-taskbar: the compositor answered the first round trip");
            }
            return Ok(());
        }
        match conn.flush() {
            Err(WaylandError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(failed_during(conn, STAGE, e)),
            Ok(()) => {}
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let fd = guard.connection_fd().as_raw_fd();
        if !readable(fd, Duration::ZERO)? {
            if !waited {
                waited = true;
                eprintln!(
                    "seinas-taskbar: connected; waiting up to {} ms for the compositor to answer the first round trip",
                    timeout.as_millis()
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || !readable(fd, remaining)? {
                return Err(format!(
                    "the compositor did not answer the first round trip within {} ms (the initialization timed out)",
                    timeout.as_millis()
                ));
            }
        }
        match guard.read() {
            Err(WaylandError::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(failed_during(conn, STAGE, e)),
            Ok(_) => {}
        }
    }
}

/// `fd` が読めるようになるまで、`timeout` まで待つ(`poll`。読めるか、相手が閉じたらtrue)。
fn readable(fd: RawFd, timeout: Duration) -> Result<bool, String> {
    let ms = timeout.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: pollfd は、書き込める 1 つの pollfd を指している。
        let ready = unsafe { libc::poll(&mut pollfd, 1, ms) };
        if ready >= 0 {
            return Ok(ready > 0);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(format!("cannot wait for the compositor: {e}"));
        }
    }
}

/// 時計の時差を、起動オプション・設定のファイル・環境変数 `TZ` から決め、決めたことと、読めなかったものを
/// ログに出す。
fn clock_offset(options: &Options, tz: Option<&str>) -> i32 {
    let mut notes = Vec::new();
    let config = options
        .config
        .clone()
        .or_else(|| seinas_config::default_file(CONFIG_FILE))
        .and_then(|path| {
            let (offset, problems) = read_config(&path);
            notes.extend(problems);
            offset
        });
    let (offset, from) = choose_offset(options.utc_offset.as_deref(), config, tz, &mut notes);
    for note in notes {
        eprintln!("seinas-taskbar: {note}");
    }
    eprintln!(
        "seinas-taskbar: the clock is UTC{} (from {from})",
        format_utc_offset(offset)
    );
    offset
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_options(&args, std::env::var(FONTS_ENV).ok())?;
    let offset = clock_offset(&options, std::env::var("TZ").ok().as_deref());
    let (text, messages) = TextPainter::load_setup(&FontSetup {
        bundled_dir: options.fonts.clone(),
        config_file: options.font_config.clone(),
    });
    for message in messages {
        eprintln!("seinas-taskbar: {message}");
    }

    let conn = connect(options.connect_timeout)?;
    first_round_trip(&conn, options.round_trip_timeout)?;
    let (globals, event_queue) = registry_queue_init::<Taskbar>(&conn)
        .map_err(|e| failed_during(&conn, "the registry round trip", e))?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Taskbar> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| format!("cannot watch the connection: {e}"))?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| {
        format!(
            "the compositor does not offer zwlr_layer_shell_v1 (connect to the shell socket): {e}"
        )
    })?;
    globals
        .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ManagerData)
        .map_err(|e| {
            format!("the compositor does not offer zwlr_foreign_toplevel_manager_v1: {e}")
        })?;
    let shm = Shm::bind(&globals, &qh)?;

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Top, Some("seinas-taskbar"), None);
    layer.set_anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_size(0, HEIGHT as u32);
    layer.set_exclusive_zone(HEIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();
    // 幅は configure まで分からない。バッファ 2 枚ぶん(800 の幅を見込む。広ければ pool が伸びる)。
    let pool = SlotPool::new((800 * HEIGHT * 4 * 2) as usize, &shm)?;

    let mut state = Taskbar {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        buffer: None,
        qh: qh.clone(),
        width: 0,
        configured: false,
        frame_pending: false,
        dirty: false,
        exit: false,
        pointer: None,
        hot: Pointer::default(),
        toplevels: Vec::new(),
        text,
        icons: IconCache::from_env(ICON_SIZE as u32),
        torch: seinas_icons::torch(ICON_SIZE as u32),
        offset,
        clock: clock_for(options.fixed_time.or_else(wall_clock), offset),
        fixed_time: options.fixed_time,
    };
    // 時計は、分が変わった少し後に起きて再描画する。時刻が取れないときは、60 秒ごとに読み直す。時刻を止めて
    // いるときは、起きない。
    if let Some(fixed) = options.fixed_time {
        eprintln!(
            "seinas-taskbar: the clock is fixed at {} seconds",
            fixed.as_secs()
        );
    } else {
        event_loop
            .handle()
            .insert_source(
                Timer::from_duration(until_next_minute(wall_clock())),
                |_, _, state: &mut Taskbar| {
                    state.update_clock();
                    TimeoutAction::ToDuration(until_next_minute(wall_clock()))
                },
            )
            .map_err(|e| format!("cannot start the clock: {e}"))?;
    }
    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| format!("the connection to the compositor was lost: {e}"))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seinas-taskbar: {e}");
            ExitCode::FAILURE
        }
    }
}

delegate_registry!(Taskbar);
impl ProvidesRegistryState for Taskbar {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}
smithay_client_toolkit::delegate_dispatch2!(Taskbar);

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_offset_and_the_config_come_from_the_arguments() {
        let options = parse_options(
            &args(&["--utc-offset", "UTC", "--config", "/c/taskbar.conf"]),
            None,
        )
        .unwrap();
        assert_eq!(options.utc_offset.as_deref(), Some("UTC"));
        assert_eq!(options.config, Some(PathBuf::from("/c/taskbar.conf")));
        assert!(parse_options(&args(&["--bogus"]), None).is_err());
        assert!(parse_options(&args(&["--utc-offset"]), None).is_err());
    }

    #[test]
    fn the_timeouts_come_from_the_arguments() {
        let timeouts = |list: &[&str]| {
            parse_options(&args(list), None).map(|o| (o.connect_timeout, o.round_trip_timeout))
        };
        assert_eq!(timeouts(&[]), Ok((CONNECT_TIMEOUT, ROUND_TRIP_TIMEOUT)));
        assert_eq!(
            timeouts(&["--connect-timeout", "250", "--round-trip-timeout", "40"]),
            Ok((Duration::from_millis(250), Duration::from_millis(40)))
        );
        for bad in [
            &["--connect-timeout", "0"][..],
            &["--connect-timeout", "x"],
            &["--round-trip-timeout", "-1"],
            &["--round-trip-timeout"],
        ] {
            assert!(timeouts(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_fonts_come_from_the_argument_then_the_environment() {
        let fonts = |list: &[&str], env: Option<&str>| {
            parse_options(&args(list), env.map(str::to_owned)).map(|o| o.fonts)
        };
        assert_eq!(fonts(&[], None), Ok(PathBuf::from("target/fonts")));
        assert_eq!(fonts(&[], Some("/f")), Ok(PathBuf::from("/f")));
        assert_eq!(
            fonts(&["--fonts", "/a"], Some("/f")),
            Ok(PathBuf::from("/a"))
        );
    }
}
