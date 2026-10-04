//! Waylandの受け口(Smithay)。子のクライアントに見せる側。
//!
//! ここは共通の描画とは別になっている。受け口を持たない構成(fbdevの裏側で、共通の描画だけを動かす形)
//! では、このモジュールごと使わない。

use std::time::Duration;

use smithay::{
    backend::renderer::{
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        pixman::PixmanRenderer,
        utils::on_commit_buffer_handler,
    },
    delegate_compositor, delegate_seat, delegate_shm, delegate_xdg_shell,
    input::{
        pointer::{CursorImageStatus, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer, wl_seat, wl_surface::WlSurface},
        Client, DisplayHandle,
    },
    utils::{Serial, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_surface_tree_downward, CompositorClientState, CompositorHandler, CompositorState,
            SurfaceAttributes, TraversalAction,
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};
use tracing::info;

use crate::Seinas;

/// 受け口の状態。
pub struct Frontend {
    pub display_handle: DisplayHandle,
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    seat_state: SeatState<Seinas>,
    // 席は、持っている間だけ子に見える。
    _seat: Seat<Seinas>,
    pub pointer: PointerHandle<Seinas>,
    /// 子のtoplevelに伝える大きさ。
    size: Size<i32, smithay::utils::Logical>,
}

impl Frontend {
    pub fn new(display_handle: &DisplayHandle, width: i32, height: i32) -> Self {
        let compositor_state = CompositorState::new::<Seinas>(display_handle);
        let xdg_shell_state = XdgShellState::new::<Seinas>(display_handle);
        let shm_state = ShmState::new::<Seinas>(display_handle, vec![]);
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(display_handle, "seinas-seat");
        let pointer = seat.add_pointer();
        Frontend {
            display_handle: display_handle.clone(),
            compositor_state,
            xdg_shell_state,
            shm_state,
            seat_state,
            _seat: seat,
            pointer,
            size: Size::from((width, height)),
        }
    }

    /// ポインターの焦点を渡す相手。子のtoplevelは1つだけを(0, 0)に置くので、その先頭を返す。
    pub fn pointer_focus(&self) -> Option<WlSurface> {
        self.xdg_shell_state
            .toplevel_surfaces()
            .first()
            .map(|toplevel| toplevel.wl_surface().clone())
    }

    /// 子のtoplevelを、描画の要素として並べる(手前から順)。
    pub fn render_elements(
        &self,
        renderer: &mut PixmanRenderer,
    ) -> Vec<WaylandSurfaceRenderElement<PixmanRenderer>> {
        self.xdg_shell_state
            .toplevel_surfaces()
            .iter()
            .flat_map(|toplevel| {
                render_elements_from_surface_tree(
                    renderer,
                    toplevel.wl_surface(),
                    (0, 0),
                    1.0,
                    1.0,
                    Kind::Unspecified,
                )
            })
            .collect()
    }

    /// 子へframeコールバックを返す(「次の絵を描いてよい」の合図)。
    pub fn send_frame_callbacks(&self, elapsed: Duration) {
        let time = elapsed.as_millis() as u32;
        for toplevel in self.xdg_shell_state.toplevel_surfaces() {
            with_surface_tree_downward(
                toplevel.wl_surface(),
                (),
                |_, _, &()| TraversalAction::DoChildren(()),
                |_, states, &()| {
                    for callback in states
                        .cached_state
                        .get::<SurfaceAttributes>()
                        .current()
                        .frame_callbacks
                        .drain(..)
                    {
                        callback.done(time);
                    }
                },
                |_, _, &()| true,
            );
        }
    }
}

/// クライアントごとの状態。
#[derive(Default)]
pub struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        info!("a client connected");
    }
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        info!("a client disconnected");
    }
}

impl BufferHandler for Seinas {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Seinas {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.frontend.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // 受け付けるときに必ずClientStateを付けている。
        &client
            .get_data::<ClientState>()
            .expect("every client carries a ClientState")
            .compositor_state
    }
    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.needs_redraw = true;
    }
}

impl ShmHandler for Seinas {
    fn shm_state(&self) -> &ShmState {
        &self.frontend.shm_state
    }
}

impl XdgShellHandler for Seinas {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.frontend.xdg_shell_state
    }
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let size = self.frontend.size;
        surface.with_pending_state(|state| {
            state.size = Some(size);
        });
        surface.send_configure();
        info!("new toplevel");
    }
    fn toplevel_destroyed(&mut self, _surface: ToplevelSurface) {
        self.needs_redraw = true;
    }
    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

impl SeatHandler for Seinas {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;
    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.frontend.seat_state
    }
    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
}

delegate_compositor!(Seinas);
delegate_shm!(Seinas);
delegate_xdg_shell!(Seinas);
delegate_seat!(Seinas);
