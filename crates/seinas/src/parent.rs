//! 裏側: 親のWayland(WSLgなど)への提出。親に対してはクライアントとして振る舞う(SCTK)。
//!
//! 共通の描画が作った絵を、親のwl_shmのバッファへ写して出す。親から届いたポインターの動きは、
//! 受け口を通して子へ渡す。

use std::error::Error;

use seinas_frontend::{pointer_input, PointerInput};
use seinas_render::{FrameView, PixelFormat};
use smithay::utils::{Logical, Point};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
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
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::Seinas;

/// 親に対するクライアント側の状態。
pub struct Parent {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    buffer: Option<Buffer>,
    pointer: Option<wl_pointer::WlPointer>,
    width: i32,
    height: i32,
    configured: bool,
    frame_pending: bool,
}

impl Parent {
    /// 親にウィンドウを1つ開く。大きさは `width` × `height` に固定する。
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
        window.set_min_size(Some((width as u32, height as u32)));
        window.set_max_size(Some((width as u32, height as u32)));
        window.commit();
        // バッファ2枚ぶん。親が1枚を使っている間に、もう1枚へ描けるようにする。
        let pool = SlotPool::new((width * height * 4 * 2) as usize, &shm)?;
        Ok(Parent {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, qh),
            output_state: OutputState::new(globals, qh),
            shm,
            pool,
            window,
            buffer: None,
            pointer: None,
            width,
            height,
            configured: false,
            frame_pending: false,
        })
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
        // 大きさは固定。親が別の大きさを求めても、同じ大きさのバッファを出し続ける。
        if let (Some(w), Some(h)) = configure.new_size {
            if (w.get() as i32, h.get() as i32) != (self.parent.width, self.parent.height) {
                warn!(
                    "the parent configured {w}x{h}; keeping {}x{} (resizing is not supported)",
                    self.parent.width, self.parent.height
                );
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
        if capability == Capability::Pointer && self.parent.pointer.is_none() {
            match self.parent.seat_state.get_pointer(qh, &seat) {
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
            if let Some(pointer) = self.parent.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Seinas {
    /// 親から届いたポインターの動きを、子へ渡す。
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            if &event.surface != self.parent.window.wl_surface() {
                continue;
            }
            let location = Point::<f64, Logical>::from(event.position);
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
    }
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
