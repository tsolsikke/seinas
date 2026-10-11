//! seinas-taskbar: Seinas のタスクバー。
//!
//! コンポジタとは別のプログラムで、画面の下に layer surface(wlr-layer-shell)を出し、foreign-toplevel
//! (wlr-foreign-toplevel-management)で、開いているウィンドウの一覧を受け取ってボタンを並べる。右の端に
//! 時計を出す。シェルの部品のためのソケットへつなぐこと(`WAYLAND_DISPLAY=seinas-0-shell`)。
//!
//! 引数:
//!
//! - `--fonts DIR`:フォントの置き場(無ければ環境変数 `SEINAS_FONTS`、それも無ければ `target/fonts`)。
//! - `--utc-offset ±HH:MM`:時計の時差(無ければ環境変数 `TZ` の簡単な形、それも無ければ +09:00)。
//!
//! 決まりの説明は `docs/taskbar.md` にある。

use std::{error::Error, path::PathBuf, process::ExitCode, sync::Arc};

use seinas_taskbar::{
    action_for, button_at, clock_text, offset_from_tz, paint, parse_utc_offset, until_next_minute,
    wall_clock, Action, Entry, DEFAULT_UTC_OFFSET, HEIGHT,
};
use seinas_text::{FontFiles, TextPainter};
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
    offset: i32,
}

fn parse_options(
    args: &[String],
    fonts_env: Option<String>,
    tz: Option<String>,
) -> Result<Options, String> {
    let mut fonts = None;
    let mut offset = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--fonts" => fonts = Some(PathBuf::from(value()?)),
            "--utc-offset" => {
                let text = value()?;
                offset = Some(
                    parse_utc_offset(&text)
                        .ok_or_else(|| format!("invalid --utc-offset: {text} (use +HH:MM)"))?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Options {
        fonts: fonts
            .or_else(|| fonts_env.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_FONT_DIR)),
        offset: offset
            .or_else(|| tz.as_deref().and_then(offset_from_tz))
            .unwrap_or(DEFAULT_UTC_OFFSET),
    })
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_options(
        &args,
        std::env::var(FONTS_ENV).ok(),
        std::env::var("TZ").ok(),
    )?;
    let (text, problems) = TextPainter::load(&FontFiles::in_dir(&options.fonts));
    for problem in problems {
        eprintln!("seinas-taskbar: {problem}");
    }

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to the compositor (WAYLAND_DISPLAY): {e}"))?;
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
        offset: options.offset,
        clock: clock_text(wall_clock(), options.offset),
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
    fn the_offset_comes_from_the_argument_then_tz_then_japan() {
        let offset = |list: &[&str], tz: Option<&str>| {
            parse_options(&args(list), None, tz.map(str::to_owned)).map(|o| o.offset)
        };
        assert_eq!(offset(&[], None), Ok(9 * 3600));
        assert_eq!(offset(&[], Some("UTC0")), Ok(0));
        // 読めない TZ(データベースの名前など)は、既定に戻る。
        assert_eq!(offset(&[], Some("Asia/Tokyo")), Ok(9 * 3600));
        assert_eq!(
            offset(&["--utc-offset", "-05:00"], Some("UTC0")),
            Ok(-5 * 3600)
        );
        assert!(offset(&["--utc-offset", "9"], None).is_err());
        assert!(offset(&["--bogus"], None).is_err());
    }

    #[test]
    fn the_fonts_come_from_the_argument_then_the_environment() {
        let fonts = |list: &[&str], env: Option<&str>| {
            parse_options(&args(list), env.map(str::to_owned), None).map(|o| o.fonts)
        };
        assert_eq!(fonts(&[], None), Ok(PathBuf::from("target/fonts")));
        assert_eq!(fonts(&[], Some("/f")), Ok(PathBuf::from("/f")));
        assert_eq!(
            fonts(&["--fonts", "/a"], Some("/f")),
            Ok(PathBuf::from("/a"))
        );
    }
}
