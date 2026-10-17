//! 裏側: 親のWayland(WSLgなど)への提出。親に対してはクライアントとして振る舞う(SCTK)。
//!
//! 共通の描画が作った絵を、親のwl_shmのバッファへ写して出す。親から届いたポインターの動きは、
//! フロントエンドを通して子へ渡す。カーソルの形(サイズ変更の境界の上など)は、フロントエンドが決め、親のカーソルの
//! テーマ(xcursor)の絵で出す。

use std::error::Error;

use seinas_frontend::{
    geometry::PointF, keyboard_input, pointer_input, release_all_keys, set_keymap,
    set_locked_modifiers, Cursor, Edges, FrontendHost, PointerInput,
};
use seinas_render::{FrameView, PixelFormat};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_dispatch2, delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{
            CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
        },
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use tracing::{info, warn};
use wayland_client::{
    globals::GlobalList,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle, WEnum,
};

use crate::Seinas;

/// 親に対するクライアント側の状態。
/// 親のウィンドウの大きさの下限。
const MIN_SIZE: (u32, u32) = (320, 240);

pub struct Parent {
    compositor: CompositorState,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    buffer: Option<Buffer>,
    pointer: Option<ThemedPointer>,
    /// 親のキーボード。キーと keymap を、子へ渡す。
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// 最後に親へ出したカーソルの形。ポインターが入ってきたら、出し直す(None にする)。
    cursor: Option<Cursor>,
    width: i32,
    height: i32,
    /// 親の configure で求められた大きさで、まだ合わせていないもの。次に描くときに合わせる。
    pending_size: Option<(i32, i32)>,
    configured: bool,
    frame_pending: bool,
}

impl Parent {
    /// 親にウィンドウを1つ開く。最初の大きさは `width` × `height`。親の configure で大きさが変わる
    /// ([`Parent::take_pending_size`])。
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<Seinas>,
        width: i32,
        height: i32,
    ) -> Result<Self, Box<dyn Error>> {
        let compositor = CompositorState::bind(globals, qh)?;
        let xdg_shell = XdgShell::bind(globals, qh)?;
        let shm = Shm::bind(globals, qh)?;
        let surface = compositor.create_surface(qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, qh);
        window.set_title("Seinas");
        window.set_app_id("seinas");
        // 小さすぎると、タイトルバーとタスクバーが入らないので、下限を置く。上限は置かない。
        window.set_min_size(Some(MIN_SIZE));
        window.commit();
        // バッファ2枚ぶん。親が1枚を使っている間に、もう1枚へ描けるようにする。
        let pool = SlotPool::new((width * height * 4 * 2) as usize, &shm)?;
        Ok(Parent {
            compositor,
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            shm,
            pool,
            window,
            buffer: None,
            pointer: None,
            keyboard: None,
            cursor: None,
            width,
            height,
            pending_size: None,
            configured: false,
            frame_pending: false,
        })
    }

    /// 親の configure で求められた大きさのうち、まだ合わせていないもの(最後に届いたもの)を取り出し、親へ出す
    /// バッファの大きさをそれに合わせる。呼ぶ側は、出力と共通のレンダラーの大きさも合わせる。
    ///
    /// ウィンドウの大きさを変えている間(ドラッグの間)、親は configure を続けて送ってくる。届くたびではなく、描くとき
    /// (親の frame callback で次の絵を出してよくなったとき)に、最後の大きさにだけ合わせる。こうすると、
    /// 出力を変える回数(クライアントへの configure と、壁紙などの再描画)が、親へ絵を出す回数を越えない。
    pub fn take_pending_size(&mut self) -> Option<(i32, i32)> {
        let size = self.pending_size.take()?;
        if size == (self.width, self.height) {
            return None;
        }
        (self.width, self.height) = size;
        // 前の大きさのバッファは使わない(pool は、要れば伸びる)。
        self.buffer = None;
        Some(size)
    }

    /// いま絵を出してよいか。親の最初のconfigureの前と、frameコールバックを待っている間は出さない。
    pub fn can_present(&self) -> bool {
        self.configured && !self.frame_pending
    }

    /// 描画結果を親のバッファへ写して出す。
    ///
    /// 親が使っているバッファには書かない。使用中なら、別のバッファを取ってそちらへ書く。
    pub fn present(
        &mut self,
        view: &FrameView<'_>,
        qh: &QueueHandle<Seinas>,
    ) -> Result<(), Box<dyn Error>> {
        let (width, height) = (self.width, self.height);
        if (view.width(), view.height()) != (width as usize, height as usize) {
            return Err("the frame size does not match the parent window".into());
        }
        let format = match view.format() {
            PixelFormat::Xrgb8888 => wl_shm::Format::Xrgb8888,
        };
        let stride = width * 4;

        let buffer = match &mut self.buffer {
            Some(buffer) => buffer,
            empty => empty.insert(self.pool.create_buffer(width, height, stride, format)?.0),
        };
        let dst = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                // 親がまだ前のバッファを離していない。新しいバッファを取る(前のものは親が離した後に戻る)。
                let (second, canvas) = self.pool.create_buffer(width, height, stride, format)?;
                *buffer = second;
                canvas
            }
        };
        view.copy_to(dst, stride as usize);

        let surface = self.window.wl_surface();
        surface.damage_buffer(0, 0, width, height);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface)?;
        self.window.commit();
        self.frame_pending = true;
        Ok(())
    }
}

impl CompositorHandler for Seinas {
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
    fn frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.parent.frame_pending = false;
        if self.needs_redraw {
            self.draw(qh);
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

impl OutputHandler for Seinas {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.parent.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for Seinas {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        info!("the parent asked to close");
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        // 大きさの指定があれば、次に描くときに合わせる(指定が無ければ、今の大きさのまま)。
        if let (Some(w), Some(h)) = configure.new_size {
            let size = (
                (w.get() as i32).max(MIN_SIZE.0 as i32),
                (h.get() as i32).max(MIN_SIZE.1 as i32),
            );
            if size != (self.parent.width, self.parent.height) {
                self.parent.pending_size = Some(size);
                self.needs_redraw = true;
            }
        }
        if !self.parent.configured {
            self.parent.configured = true;
            self.draw(qh);
        }
    }
}

impl SeatHandler for Seinas {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.parent.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.parent.keyboard.is_none() {
            self.parent.keyboard = Some(seat.get_keyboard(qh, ParentKeyboard));
        }
        if capability == Capability::Pointer && self.parent.pointer.is_none() {
            // カーソルの絵は、親のカーソルのテーマ(XCURSOR_THEME、XCURSOR_SIZE。無ければ既定)から読む。
            let surface = self.parent.compositor.create_surface(qh);
            match self.parent.seat_state.get_pointer_with_theme::<_, ()>(
                qh,
                &seat,
                self.parent.shm.wl_shm(),
                surface,
                ThemeSpec::default(),
            ) {
                Ok(pointer) => self.parent.pointer = Some(pointer),
                Err(e) => warn!("failed to get the parent pointer: {e}"),
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
            // 捨てると、親のポインターも離す。
            self.parent.pointer = None;
        }
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.parent.keyboard.take() {
                keyboard.release();
            }
            let time = self.start.elapsed().as_millis() as u32;
            release_all_keys(self, time);
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Seinas {
    /// 親から届いたポインターの動きを、子へ渡す。
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            if &event.surface != self.parent.window.wl_surface() {
                continue;
            }
            if let Enter { .. } = event.kind {
                // 入ってきたときは、親のカーソルの形が決まっていないので、出し直す。
                self.parent.cursor = None;
            }
            let location = PointF::from(event.position);
            let input = match event.kind {
                Enter { .. } | Motion { .. } => PointerInput::Motion(location),
                Leave { .. } => PointerInput::Leave,
                Press { button, .. } => PointerInput::Button {
                    location,
                    button,
                    pressed: true,
                },
                Release { button, .. } => PointerInput::Button {
                    location,
                    button,
                    pressed: false,
                },
                Axis { .. } => continue,
            };
            // 手前に出す、動かす、閉じるの操作と、子へ渡す相手は、受け口が決める。
            let time = self.start.elapsed().as_millis() as u32;
            pointer_input(self, input, time);
        }
        self.update_cursor(conn);
    }
}

impl Seinas {
    /// フロントエンドが決めたカーソルの形が変わっていたら、親のカーソルを変える。
    fn update_cursor(&mut self, conn: &Connection) {
        let cursor = self.frontend().cursor();
        if self.parent.cursor == Some(cursor) {
            return;
        }
        let Some(pointer) = &self.parent.pointer else {
            return;
        };
        match pointer.set_cursor(conn, cursor_icon(cursor)) {
            Ok(()) => self.parent.cursor = Some(cursor),
            // テーマが無いときなど。何度も出さないよう、出したことにしておく。
            Err(e) => {
                warn!("failed to set the parent cursor: {e}");
                self.parent.cursor = Some(cursor);
            }
        }
    }
}

/// カーソルの形を、カーソルのテーマの名前にする。
fn cursor_icon(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default => CursorIcon::Default,
        Cursor::Resize(edges) => match edges {
            Edges::TOP => CursorIcon::NResize,
            Edges::BOTTOM => CursorIcon::SResize,
            Edges::LEFT => CursorIcon::WResize,
            Edges::RIGHT => CursorIcon::EResize,
            Edges::TOP_LEFT => CursorIcon::NwResize,
            Edges::TOP_RIGHT => CursorIcon::NeResize,
            Edges::BOTTOM_LEFT => CursorIcon::SwResize,
            Edges::BOTTOM_RIGHT => CursorIcon::SeResize,
            _ => CursorIcon::Default,
        },
    }
}

/// 親のキーボードのデータ。親から届くキーと keymap を、子(Seinas の seat)へ渡す。
struct ParentKeyboard;

impl Dispatch2<wl_keyboard::WlKeyboard, Seinas> for ParentKeyboard {
    fn event(
        &self,
        state: &mut Seinas,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &Connection,
        _: &QueueHandle<Seinas>,
    ) {
        let time = state.start.elapsed().as_millis() as u32;
        match event {
            // 親の keymap を、そのまま子のクライアントへ配る。
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => match read_keymap(fd, size as usize) {
                Ok(keymap) => {
                    if set_keymap(state, keymap) {
                        info!("using the keymap of the parent ({size} bytes)");
                    }
                }
                Err(e) => warn!("cannot read the keymap of the parent: {e}"),
            },
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(key_state),
                ..
            } => {
                let pressed = key_state == wl_keyboard::KeyState::Pressed;
                keyboard_input(state, key, pressed, time);
            }
            // 親のフォーカスが外れたら、押されているキーを離したことにする(子で繰り返しが続かないように)。
            wl_keyboard::Event::Leave { .. } => release_all_keys(state, time),
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                if let Some(keyboard) = &state.frontend().keyboard {
                    keyboard.change_repeat_info(rate, delay);
                }
            }
            // 固定された修飾キー(Caps Lock、Num Lock)は、親に合わせる。押されている修飾キーは、子の側で、
            // キーから求める。
            wl_keyboard::Event::Modifiers { mods_locked, .. }
                if set_locked_modifiers(state, mods_locked) =>
            {
                info!("the locked modifiers follow the parent ({mods_locked:#x})");
            }
            _ => {}
        }
    }
}

/// 親が渡した keymap の fd から、`size` バイトを読む(終わりの NUL は除く)。
///
/// fd の読む位置を動かさないよう、位置を指定して読む(親が、同じ fd をほかにも渡していることがある)。
fn read_keymap(fd: std::os::fd::OwnedFd, size: usize) -> std::io::Result<String> {
    use std::os::unix::fs::FileExt;
    let file = std::fs::File::from(fd);
    let mut bytes = vec![0; size];
    file.read_exact_at(&mut bytes, 0)?;
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(std::io::Error::other)
}

impl ShmHandler for Seinas {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.parent.shm
    }
}

impl ProvidesRegistryState for Seinas {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.parent.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}

delegate_registry!(Seinas);
delegate_dispatch2!(Seinas);
