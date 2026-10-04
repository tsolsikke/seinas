//! Waylandの受け口(Smithay)。クライアントに見せる側。
//!
//! 受け口は、裏側(絵をどこへ出すか)を知らない。入れ子の構成(`seinas`)と、fbdevへ出す構成
//! (`seinas-standalone`)の両方が、この受け口を使う。
//!
//! Smithayのハンドラは、コンポジタの状態の型そのものに実装する決まりになっている。そこで、
//! 使う側は次の2つを行う。
//!
//! 1. 状態の型に [`FrontendHost`] を実装する(受け口の置き場所と、描き直しの知らせ方を教える)。
//! 2. [`delegate_frontend!`] を呼ぶ(Smithayのハンドラと、受け口の作り方をその型に実装する)。
//!
//! 受け口を持たない構成(共通の描画だけ)では、このクレートごと使わない。

use std::{sync::Arc, time::Duration};

pub use smithay;

use smithay::{
    backend::renderer::{
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        pixman::PixmanRenderer,
    },
    input::{pointer::PointerHandle, Seat, SeatHandler, SeatState},
    output::Output,
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::wl_surface::WlSurface,
        Client, Display, DisplayHandle, Resource,
    },
    utils::{Logical, Size},
    wayland::{
        compositor::{
            with_surface_tree_downward, CompositorClientState, CompositorState, SurfaceAttributes,
            TraversalAction,
        },
        shell::xdg::XdgShellState,
        shm::ShmState,
    },
};
use tracing::{error, info};

/// 受け口の作り方の指定。
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// 画面の幅と高さ(画素)。wl_outputで公開し、toplevelにも伝える。
    pub width: i32,
    pub height: i32,
    /// 席(wl_seat)とポインターを公開するか。入力を扱わない構成ではfalseにする。
    pub pointer: bool,
}

/// 受け口の状態。`D` は、コンポジタの状態の型。
pub struct Frontend<D: SeatHandler> {
    pub display_handle: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<D>,
    /// 席は、持っている間だけクライアントに見える。入力を扱わない構成ではNone。
    pub seat: Option<Seat<D>>,
    pub pointer: Option<PointerHandle<D>>,
    /// 画面(wl_output)。
    pub output: Output,
    /// クライアントのtoplevelに伝える大きさ。
    pub size: Size<i32, Logical>,
}

/// コンポジタの状態の型が、受け口を使うために実装するもの。
pub trait FrontendHost: SeatHandler + Sized {
    /// 受け口の置き場所。
    fn frontend(&self) -> &Frontend<Self>;
    fn frontend_mut(&mut self) -> &mut Frontend<Self>;
    /// クライアントの画面が変わり、描き直しが要るようになったときに呼ばれる。
    fn redraw_needed(&mut self);
}

/// 受け口を作る。[`delegate_frontend!`] が、状態の型に実装する。
pub trait BuildFrontend: SeatHandler + Sized {
    fn build_frontend(display_handle: &DisplayHandle, config: Config) -> Frontend<Self>;
}

impl<D: SeatHandler> Frontend<D> {
    /// つないできたクライアントを受け入れる。
    pub fn accept(&mut self, stream: std::os::unix::net::UnixStream) {
        if let Err(e) = self
            .display_handle
            .insert_client(stream, Arc::new(ClientState::default()))
        {
            error!("failed to accept a client: {e}");
        }
    }

    /// ポインターの焦点を渡す相手。toplevelは(0, 0)に重ねて置くので、その先頭を返す。
    pub fn pointer_focus(&self) -> Option<WlSurface> {
        self.xdg_shell_state
            .toplevel_surfaces()
            .first()
            .map(|toplevel| toplevel.wl_surface().clone())
    }

    /// クライアントのtoplevelを、描画の要素として並べる(手前から順)。
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

    /// クライアントへframeコールバックを返す(「次の絵を描いてよい」の合図)。
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

/// 切られたクライアントの後片付けを、いますぐ行わせる。
///
/// 描いている最中に切られたクライアント(共有メモリーを縮めていた場合など)は、そのままだと、ほかの
/// クライアントから次の要求が届くまで、ソケットもウィンドウも残り続ける。描けない要素があったときに、
/// これを呼んで片付ける。イベントループの処理の外(クライアントの要求を処理していないとき)で呼ぶこと。
pub fn reap_dead_clients<D: FrontendHost + 'static>(display: &mut Display<D>, state: &mut D) {
    // 後片付けは、どれか1つのクライアントの要求を処理させると、あわせて行われる。
    let client = state
        .frontend()
        .xdg_shell_state
        .toplevel_surfaces()
        .iter()
        .find_map(|toplevel| toplevel.wl_surface().client());
    if let Some(client) = client {
        // 切られたクライアントを選んだ場合は失敗が返るが、後片付けは行われる。
        let _ = display.backend().dispatch_single_client(state, client.id());
    }
}

/// クライアントごとの状態。
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        info!("a client connected");
    }
    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        info!("a client disconnected: {reason:?}");
    }
}

/// クライアントごとのコンポジタの状態を取り出す。[`delegate_frontend!`] が使う。
pub fn client_compositor_state(client: &Client) -> &CompositorClientState {
    // 受け付けるときに必ずClientStateを付けている。
    &client
        .get_data::<ClientState>()
        .expect("every client carries a ClientState")
        .compositor_state
}

/// 状態の型 `$ty` に、Smithayのハンドラと、受け口の作り方([`BuildFrontend`])を実装する。
///
/// `$ty` は [`FrontendHost`] を実装していること。
#[macro_export]
macro_rules! delegate_frontend {
    ($ty:ty) => {
        impl $crate::smithay::wayland::buffer::BufferHandler for $ty {
            fn buffer_destroyed(
                &mut self,
                _buffer: &$crate::smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
            ) {
            }
        }

        impl $crate::smithay::wayland::compositor::CompositorHandler for $ty {
            fn compositor_state(&mut self) -> &mut $crate::smithay::wayland::compositor::CompositorState {
                &mut $crate::FrontendHost::frontend_mut(self).compositor_state
            }
            fn client_compositor_state<'a>(
                &self,
                client: &'a $crate::smithay::reexports::wayland_server::Client,
            ) -> &'a $crate::smithay::wayland::compositor::CompositorClientState {
                $crate::client_compositor_state(client)
            }
            fn commit(
                &mut self,
                surface: &$crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
            ) {
                $crate::smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
                $crate::FrontendHost::redraw_needed(self);
            }
        }

        impl $crate::smithay::wayland::shm::ShmHandler for $ty {
            fn shm_state(&self) -> &$crate::smithay::wayland::shm::ShmState {
                &$crate::FrontendHost::frontend(self).shm_state
            }
        }

        impl $crate::smithay::wayland::shell::xdg::XdgShellHandler for $ty {
            fn xdg_shell_state(&mut self) -> &mut $crate::smithay::wayland::shell::xdg::XdgShellState {
                &mut $crate::FrontendHost::frontend_mut(self).xdg_shell_state
            }
            fn new_toplevel(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                let frontend = $crate::FrontendHost::frontend(self);
                let size = frontend.size;
                frontend.output.enter(surface.wl_surface());
                surface.with_pending_state(|state| {
                    state.size = Some(size);
                });
                surface.send_configure();
                $crate::log_new_toplevel();
            }
            fn toplevel_destroyed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::redraw_needed(self);
            }
            fn new_popup(
                &mut self,
                _surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                _positioner: $crate::smithay::wayland::shell::xdg::PositionerState,
            ) {
            }
            fn grab(
                &mut self,
                _surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                _seat: $crate::smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
                _serial: $crate::smithay::utils::Serial,
            ) {
            }
            fn reposition_request(
                &mut self,
                _surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                _positioner: $crate::smithay::wayland::shell::xdg::PositionerState,
                _token: u32,
            ) {
            }
        }

        impl $crate::smithay::input::SeatHandler for $ty {
            type KeyboardFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            type PointerFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            type TouchFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            fn seat_state(&mut self) -> &mut $crate::smithay::input::SeatState<Self> {
                &mut $crate::FrontendHost::frontend_mut(self).seat_state
            }
            fn focus_changed(
                &mut self,
                _seat: &$crate::smithay::input::Seat<Self>,
                _focused: Option<&$crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
            ) {
            }
            fn cursor_image(
                &mut self,
                _seat: &$crate::smithay::input::Seat<Self>,
                _image: $crate::smithay::input::pointer::CursorImageStatus,
            ) {
            }
        }

        impl $crate::smithay::wayland::output::OutputHandler for $ty {}

        impl $crate::BuildFrontend for $ty {
            fn build_frontend(
                display_handle: &$crate::smithay::reexports::wayland_server::DisplayHandle,
                config: $crate::Config,
            ) -> $crate::Frontend<Self> {
                use $crate::smithay::{
                    input::SeatState,
                    wayland::{compositor::CompositorState, shell::xdg::XdgShellState, shm::ShmState},
                };

                let compositor_state = CompositorState::new::<Self>(display_handle);
                let xdg_shell_state = XdgShellState::new::<Self>(display_handle);
                let shm_state = ShmState::new::<Self>(display_handle, vec![]);
                let mut seat_state = SeatState::new();
                let (seat, pointer) = if config.pointer {
                    let mut seat = seat_state.new_wl_seat(display_handle, "seinas-seat");
                    let pointer = seat.add_pointer();
                    (Some(seat), Some(pointer))
                } else {
                    (None, None)
                };
                let output = $crate::new_output(config.width, config.height);
                output.create_global::<Self>(display_handle);
                $crate::Frontend {
                    display_handle: display_handle.clone(),
                    compositor_state,
                    xdg_shell_state,
                    shm_state,
                    seat_state,
                    seat,
                    pointer,
                    output,
                    size: (config.width, config.height).into(),
                }
            }
        }

        $crate::smithay::delegate_compositor!($ty);
        $crate::smithay::delegate_shm!($ty);
        $crate::smithay::delegate_xdg_shell!($ty);
        $crate::smithay::delegate_seat!($ty);
        $crate::smithay::delegate_output!($ty);
    };
}

/// 画面(wl_output)を、`width` × `height` 画素の1つの表示モードで作る。[`delegate_frontend!`] が使う。
pub fn new_output(width: i32, height: i32) -> Output {
    use smithay::{
        output::{Mode, PhysicalProperties, Scale, Subpixel},
        utils::Transform,
    };
    let output = Output::new(
        "seinas-0".to_owned(),
        PhysicalProperties {
            // 実際の大きさ(ミリメートル)は分からないので、0にしておく。
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Seinas".to_owned(),
            model: "screen".to_owned(),
        },
    );
    let mode = Mode {
        size: (width, height).into(),
        // 垂直同期の周期は分からないので、60Hzと名乗る(単位はmHz)。
        refresh: 60_000,
    };
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output
}

/// [`delegate_frontend!`] が使う。
pub fn log_new_toplevel() {
    info!("new toplevel");
}
