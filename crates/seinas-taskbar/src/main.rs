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
//! - `--connect-tries N`:コンポジタのソケットがまだ無いときに、最初の接続を試す回数(最初の 1 回を含む。無ければ
//!   [`CONNECT_TRIES`]。1 なら試し直さない)。間隔は [`CONNECT_INTERVAL`]。
//!
//! 決まりの説明は `docs/taskbar.md` にある。

use std::{error::Error, os::unix::net::UnixStream, path::PathBuf, process::ExitCode, sync::Arc};

use seinas_taskbar::{
    action_for, button_at, choose_offset, clock_text, format_utc_offset, not_ready_yet, paint,
    read_config, socket_path, until_next_minute, wall_clock, Action, Entry, CONFIG_FILE,
    CONNECT_INTERVAL, CONNECT_TRIES, HEIGHT,
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
    backend::ObjectData,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
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
    /// ポインターが乗っているボタン。
    hot: Option<usize>,
    toplevels: Vec<Toplevel>,
    text: TextPainter,
    /// 時計の時差(秒)と、いま出している文字。
    offset: i32,
    clock: String,
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
        let pixels = paint(&mut self.text, width, &entries, self.hot, &self.clock);
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

    /// 時計を読み直す。表示が変わったら再描画する。
    fn update_clock(&mut self) {
        let text = clock_text(wall_clock(), self.offset);
        if text != self.clock {
            self.clock = text;
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
            let hot = match event.kind {
                Leave { .. } => None,
                _ => button_at(self.width, count, x, y),
            };
            if hot != self.hot {
                self.hot = hot;
                self.redraw();
            }
            if let (
                Press {
                    button: LEFT_BUTTON,
                    ..
                },
                Some(index),
            ) = (&event.kind, hot)
            {
                self.press(index);
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
                    title: if title.is_empty() { app_id } else { title },
                    activated,
                    minimized,
                };
                toplevel.shown = true;
                state.redraw();
            }
            Event::Closed => {
                handle.destroy();
                state.toplevels.remove(index);
                // 順が変わったので、ポインターの下のボタンは、次に動いたときに決め直す。
                state.hot = None;
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
    /// 最初の接続を試す回数(`--connect-tries`。1 以上)。
    connect_tries: u32,
}

fn parse_options(args: &[String], fonts_env: Option<String>) -> Result<Options, String> {
    let mut fonts = None;
    let mut font_config = None;
    let mut utc_offset = None;
    let mut config = None;
    let mut connect_tries = CONNECT_TRIES;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--fonts" => fonts = Some(PathBuf::from(value()?)),
            "--font-config" => font_config = Some(PathBuf::from(value()?)),
            "--utc-offset" => utc_offset = Some(value()?),
            "--config" => config = Some(PathBuf::from(value()?)),
            "--connect-tries" => {
                let text = value()?;
                connect_tries = text
                    .parse()
                    .ok()
                    .filter(|&tries| tries >= 1)
                    .ok_or(format!("invalid --connect-tries value: {text}"))?;
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
        connect_tries,
    })
}

/// コンポジタへつなぐ。コンポジタがまだ受けられないとき([`not_ready_yet`])は、[`CONNECT_INTERVAL`] ずつ眠って
/// (`std::thread::sleep`。musl では clock_nanosleep)、合わせて `tries` 回まで試す。起動の順序で、コンポジタより先に起こされることがあるため
/// (`docs/taskbar.md` の「起動の順序」)。試し直すのは、この最初の接続だけ(つないだ後に切れたら、終わる)。
fn connect(tries: u32) -> Result<Connection, Box<dyn Error>> {
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
    let mut tried = 0;
    let stream = loop {
        tried += 1;
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e) if not_ready_yet(&e).is_some() && tried < tries => {
                if tried == 1 {
                    let reason = not_ready_yet(&e).unwrap_or_default();
                    eprintln!(
                        "seinas-taskbar: the compositor is not ready at {} ({reason}: {e}); trying again every {} ms, up to {tries} tries",
                        path.display(),
                        CONNECT_INTERVAL.as_millis()
                    );
                }
                std::thread::sleep(CONNECT_INTERVAL);
            }
            Err(e) if not_ready_yet(&e).is_some() => {
                return Err(format!(
                    "the compositor did not appear at {}: gave up after {tried} tries ({e})",
                    path.display()
                )
                .into())
            }
            Err(e) => {
                return Err(format!(
                    "cannot connect to the compositor at {}: {e}",
                    path.display()
                )
                .into())
            }
        }
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

    let conn = connect(options.connect_tries)?;
    let (globals, event_queue) = registry_queue_init::<Taskbar>(&conn)?;
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
        hot: None,
        toplevels: Vec::new(),
        text,
        offset,
        clock: clock_text(wall_clock(), offset),
    };
    // 時計は、分が変わった少し後に起きて再描画する。時刻が取れないときは、60 秒ごとに読み直す。
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
    fn the_connect_tries_come_from_the_argument() {
        let tries = |list: &[&str]| parse_options(&args(list), None).map(|o| o.connect_tries);
        assert_eq!(tries(&[]), Ok(CONNECT_TRIES));
        assert_eq!(tries(&["--connect-tries", "1"]), Ok(1));
        assert!(tries(&["--connect-tries", "0"]).is_err());
        assert!(tries(&["--connect-tries", "x"]).is_err());
        assert!(tries(&["--connect-tries"]).is_err());
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
