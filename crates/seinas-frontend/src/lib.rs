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

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

pub use smithay;

mod decoration;
mod interaction;
mod stack;
pub use decoration::{
    clamp_outer_location, content_location, content_size_for, load_fonts, outer_size,
    paint_title_bar, title_text, TitleBar, ACTIVE_BAR, ACTIVE_TITLE, CLOSE_BUTTON_SIZE,
    HOT_CLOSE_BUTTON, HOT_CLOSE_MARK, INACTIVE_BAR, INACTIVE_TITLE, MIN_VISIBLE_BAR,
    TITLE_BAR_HEIGHT, TITLE_PADDING, TITLE_SIZE,
};
pub use interaction::{part_at, Deliver, Interaction, Outcome, Part, DRAG_SLACK, LEFT_BUTTON};
pub use stack::{next_slot, slot_count, slot_location, Placed, Stack, WindowId, CASCADE_STEP};

use seinas_text::TextPainter;
use smithay::{
    backend::input::ButtonState,
    backend::renderer::{
        element::{
            memory::MemoryRenderBufferRenderElement,
            render_elements,
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        pixman::PixmanRenderer,
        utils::with_renderer_surface_state,
    },
    input::{
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::Output,
    reexports::wayland_protocols::xdg::{
        decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        shell::server::xdg_toplevel,
    },
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason, ObjectId},
        protocol::wl_surface::WlSurface,
        Client, Display, DisplayHandle, Resource,
    },
    utils::{Logical, Point, Serial, Size, SERIAL_COUNTER},
    wayland::{
        compositor::{
            with_states, with_surface_tree_downward, CompositorClientState, CompositorState,
            SurfaceAttributes, TraversalAction,
        },
        shell::xdg::{
            decoration::XdgDecorationState, ToplevelSurface, XdgShellState, XdgToplevelSurfaceData,
        },
        shm::ShmState,
    },
};
use tracing::{error, info};

/// フォントの置き場を指定する環境変数。
pub const FONTS_ENV: &str = "SEINAS_FONTS";
/// フォントの置き場の指定が無いときの場所。`tools/fetch-fonts.sh` が置く、開発用の場所(作業中の
/// ディレクトリから見た位置)。
pub const DEFAULT_FONT_DIR: &str = "target/fonts";

/// フォントの置き場。引数での指定 `chosen` があればそれ、無ければ環境変数 [`FONTS_ENV`]、それも
/// 無ければ [`DEFAULT_FONT_DIR`]。
pub fn font_dir(chosen: Option<PathBuf>) -> PathBuf {
    chosen
        .or_else(|| std::env::var_os(FONTS_ENV).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FONT_DIR))
}

/// 受け口の作り方の指定。
#[derive(Debug, Clone)]
pub struct Config {
    /// 画面の幅と高さ(画素)。wl_outputで公開する。
    pub width: i32,
    pub height: i32,
    /// 席(wl_seat)とポインターを公開するか。入力を扱わない構成ではfalseにする。
    pub pointer: bool,
    /// フォントの置き場。題名を描くのに使う。読めなくても動く(題名の文字が出ないだけ)。
    pub font_dir: PathBuf,
}

render_elements! {
    /// 画面に描く要素。クライアントの画面と、Seinasが描く題名の帯。
    pub WindowElement<=PixmanRenderer>;
    Surface=WaylandSurfaceRenderElement<PixmanRenderer>,
    TitleBar=MemoryRenderBufferRenderElement<PixmanRenderer>,
}

/// 受け口の状態。`D` は、コンポジタの状態の型。
pub struct Frontend<D: SeatHandler> {
    pub display_handle: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<D>,
    /// 席は、持っている間だけクライアントに見える。入力を扱わない構成ではNone。
    pub seat: Option<Seat<D>>,
    pub pointer: Option<PointerHandle<D>>,
    /// 画面(wl_output)。
    pub output: Output,
    /// 画面の大きさ。
    pub size: Size<i32, Logical>,
    /// ウィンドウの並び。先頭がいちばん手前で、それだけが「選ばれている(activated)」。
    /// 置き場所は、題名の帯を含めた外形の左上。
    pub windows: Stack<ToplevelSurface>,
    /// 題名を描くもの。
    pub text: TextPainter,
    /// 作った題名の帯の絵。ウィンドウ(の画面)ごとに覚えておく。
    pub title_bars: HashMap<ObjectId, TitleBar>,
    /// ポインターでの操作(動かす、閉じる)の状態。
    pub interaction: Interaction,
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

    /// 新しいウィンドウを、いちばん手前に加える。置き場所は、前のものからずらして決める。
    ///
    /// 新しいウィンドウが「選ばれている」ものになり、前のものからは外れる。新しいウィンドウへの
    /// 最初のconfigureは、呼ぶ側が送る。
    pub fn add_window(&mut self, toplevel: ToplevelSurface) {
        self.windows.add(toplevel, self.size);
        self.update_activation();
    }

    /// 去ったウィンドウを、並びから外す。その下にあったものが見えるようになる。
    ///
    /// 去ったのがいちばん手前なら、その下にあったものが「選ばれている」ものになる。
    pub fn remove_window(&mut self, toplevel: &ToplevelSurface) {
        self.windows
            .remove(|window| window.wl_surface() == toplevel.wl_surface());
        self.title_bars.remove(&toplevel.wl_surface().id());
        self.update_activation();
    }

    /// いちばん手前のウィンドウにだけ、activatedの状態を付ける。変わったウィンドウには、configureで知らせる。
    fn update_activation(&self) {
        for (window, active) in self.windows.iter_with_activation() {
            let toplevel = &window.item;
            toplevel.with_pending_state(|state| {
                if active {
                    state.states.set(xdg_toplevel::State::Activated);
                } else {
                    state.states.unset(xdg_toplevel::State::Activated);
                }
            });
            // まだ最初のconfigureを送っていないウィンドウには、最初のconfigureと一緒に伝わる。
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
    }

    /// 画面の上の点 `point` にある、いちばん手前のウィンドウの中身。その画面と、中身の左上の位置を返す。
    ///
    /// ポインターの焦点を渡す相手を決めるのに使う。点が題名の帯の上にあるときは、Noneを返す
    /// (帯はSeinasのもので、クライアントには渡さない)。
    pub fn window_under(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let index = self.windows.index_at(point, window_size)?;
        let window = self.windows.iter().nth(index)?;
        let content = content_location(window.location).to_f64();
        (point.y >= content.y).then(|| (window.item.wl_surface().clone(), content))
    }

    /// ウィンドウを、描画の要素として並べる(手前から順)。1つのウィンドウは、題名の帯と、その下の中身。
    pub fn render_elements(&mut self, renderer: &mut PixmanRenderer) -> Vec<WindowElement> {
        let mut elements = Vec::new();
        for (window, active) in self.windows.iter_with_activation() {
            let surface = window.item.wl_surface();
            // まだ絵を出していないウィンドウには、帯も付けない。
            if let Some(content) = content_size(&window.item) {
                let text = toplevel_title(&window.item);
                let hot = self.interaction.close_button_is_hot(window.id);
                let bar = self
                    .title_bars
                    .entry(surface.id())
                    .and_modify(|bar| {
                        if !bar.matches(&text, content.w, active, hot) {
                            *bar = TitleBar::new(&mut self.text, &text, content.w, active, hot);
                        }
                    })
                    .or_insert_with(|| {
                        TitleBar::new(&mut self.text, &text, content.w, active, hot)
                    });
                elements.extend(
                    bar.element(renderer, window.location)
                        .map(WindowElement::TitleBar),
                );
            }
            let content = content_location(window.location);
            elements.extend(
                render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                    renderer,
                    surface,
                    // 拡大率は1なので、画面の上の位置と画素の位置は同じ。
                    (content.x, content.y),
                    1.0,
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(WindowElement::Surface),
            );
        }
        elements
    }

    /// クライアントが、飾りの描き方を尋ねてきた・変えようとしたときに呼ぶ。いつも、サーバーの側
    /// (Seinas)で描くと答える。
    pub fn decorate_on_server_side(&self, toplevel: &ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    /// クライアントへframeコールバックを返す(「次の絵を描いてよい」の合図)。
    pub fn send_frame_callbacks(&self, elapsed: Duration) {
        let time = elapsed.as_millis() as u32;
        for window in self.windows.iter() {
            with_surface_tree_downward(
                window.item.wl_surface(),
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

/// ポインターの知らせ。裏側(親のWayland、試験用の入力の口)が、受け口へ渡す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerInput {
    /// 画面の上の点へ動いた(入ってきたときも、これ)。
    Motion(Point<f64, Logical>),
    /// 画面の外へ出た。
    Leave,
    /// 点 `location` で、ボタン `button`(Linuxの入力の番号)が押された・離された。
    Button {
        location: Point<f64, Logical>,
        button: u32,
        pressed: bool,
    },
}

/// ポインターの知らせを処理する。`time` は、起動からの時間(ミリ秒)。
///
/// 押した所のウィンドウを手前に出し、題名の帯と閉じるボタンの上での操作(動かす、閉じる)はSeinasが
/// 自分で行い、それ以外は、ポインターの下にあるウィンドウの中身へ渡す。席(wl_seat)を公開して
/// いない構成では、何もしない。
pub fn pointer_input<D>(state: &mut D, input: PointerInput, time: u32)
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let frontend = state.frontend_mut();
    let Some(pointer) = frontend.pointer.clone() else {
        return;
    };
    let screen = frontend.size;
    let (location, outcome) = match input {
        PointerInput::Motion(location) => (
            location,
            frontend
                .interaction
                .motion(&mut frontend.windows, screen, location, window_size),
        ),
        PointerInput::Leave => (pointer.current_location(), frontend.interaction.leave()),
        PointerInput::Button {
            location,
            button,
            pressed: true,
        } => (
            location,
            frontend
                .interaction
                .press(&mut frontend.windows, location, button, window_size),
        ),
        PointerInput::Button {
            location,
            button,
            pressed: false,
        } => (
            location,
            frontend
                .interaction
                .release(&mut frontend.windows, location, button, window_size),
        ),
    };
    if outcome.raised {
        frontend.update_activation();
    }
    if let Some(toplevel) = outcome.close.and_then(|id| frontend.windows.get(id)) {
        // 閉じるように頼むだけ。応じるかどうかは、クライアントしだい。
        toplevel.item.send_close();
    }
    let focus = match outcome.deliver {
        Deliver::ToClient => frontend.window_under(location),
        Deliver::Nobody => None,
    };
    if outcome.redraw {
        state.redraw_needed();
    }

    let serial = SERIAL_COUNTER.next_serial();
    let motion = MotionEvent {
        location,
        serial,
        time,
    };
    match input {
        PointerInput::Motion(_) => pointer.motion(state, focus, &motion),
        PointerInput::Leave => pointer.motion(state, None, &motion),
        PointerInput::Button {
            button, pressed, ..
        } => {
            pointer.motion(state, focus, &motion);
            // Seinasが自分で使ったボタンは、クライアントには渡さない。離した知らせだけは、渡す相手を
            // 無しにしたうえで、Smithayに伝える(クライアントから頼まれた移動では、押した知らせを
            // Smithayが覚えているので、離したことも教えておく)。
            if outcome.deliver == Deliver::ToClient || !pressed {
                let state_of_button = if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                pointer.button(
                    state,
                    &ButtonEvent {
                        serial,
                        time,
                        button,
                        state: state_of_button,
                    },
                );
            }
        }
    }
    pointer.frame(state);
}

/// クライアントが、自分のウィンドウを動かしてほしいと頼んできた(xdg_toplevelのmove)。
/// [`delegate_frontend!`] が使う。
///
/// 受けるのは、そのクライアントの上でボタンが押されている間の頼みだけ。`serial` が、いま押されている
/// ボタンのものでなければ、何もしない。
pub fn client_move_request<D>(state: &mut D, toplevel: &ToplevelSurface, serial: Serial)
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let frontend = state.frontend_mut();
    let Some(pointer) = frontend.pointer.clone() else {
        return;
    };
    // 押されているボタンが、このウィンドウの上で押されたものか。押した点も、ここで分かる。
    let press = pointer
        .grab_start_data()
        .filter(|_| pointer.has_grab(serial))
        .filter(|start| {
            start.focus.as_ref().is_some_and(|(surface, _)| {
                surface.id().same_client_as(&toplevel.wl_surface().id())
            })
        })
        .map(|start| start.location);
    let window = frontend
        .windows
        .iter()
        .find(|window| window.item.wl_surface() == toplevel.wl_surface())
        .map(|window| window.id);
    let (Some(press), Some(window)) = (press, window) else {
        return;
    };
    let screen = frontend.size;
    if !frontend.interaction.begin_client_move(
        &mut frontend.windows,
        screen,
        window,
        press,
        window_size,
    ) {
        return;
    }
    state.redraw_needed();
    // ここから先、ボタンを離すまでのポインターの知らせは、Seinasが使う。クライアントからは外す。
    let location = pointer.current_location();
    pointer.unset_grab(state, serial, 0);
    pointer.motion(
        state,
        None,
        &MotionEvent {
            location,
            serial: SERIAL_COUNTER.next_serial(),
            time: 0,
        },
    );
    pointer.frame(state);
}

/// ウィンドウの中身(クライアントが描く所)の大きさ。まだ絵を出していなければNone。
fn content_size(toplevel: &ToplevelSurface) -> Option<Size<i32, Logical>> {
    with_renderer_surface_state(toplevel.wl_surface(), |state| state.surface_size())?
}

/// 題名の帯を含めた、ウィンドウの外形の大きさ。まだ絵を出していなければNone。
fn window_size(toplevel: &ToplevelSurface) -> Option<Size<i32, Logical>> {
    content_size(toplevel).map(outer_size)
}

/// 題名の帯に出す文字。クライアントが付けた題名、無ければapp_id、それも無ければ空。
fn toplevel_title(toplevel: &ToplevelSurface) -> String {
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|data| data.lock().expect("the toplevel data"));
        match &data {
            Some(data) => title_text(data.title.as_deref(), data.app_id.as_deref()).to_owned(),
            None => String::new(),
        }
    })
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
        .windows
        .iter()
        .find_map(|window| window.item.wl_surface().client());
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
                let frontend = $crate::FrontendHost::frontend_mut(self);
                // 中身に使える大きさ(画面から、題名の帯のぶんを引いたもの)を伝える。
                let size = $crate::content_size_for(frontend.size);
                frontend.output.enter(surface.wl_surface());
                surface.with_pending_state(|state| {
                    state.size = Some(size);
                });
                // 並びに加えると、このウィンドウが「選ばれている」ものになる。その状態も、最初の
                // configureで一緒に伝える。
                frontend.add_window(surface.clone());
                surface.send_configure();
                $crate::log_new_toplevel();
                // 前に手前だったウィンドウの帯の色が変わる。
                $crate::FrontendHost::redraw_needed(self);
            }
            fn move_request(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface,
                _seat: $crate::smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
                serial: $crate::smithay::utils::Serial,
            ) {
                $crate::client_move_request(self, &surface, serial);
            }
            fn title_changed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::redraw_needed(self);
            }
            fn app_id_changed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::redraw_needed(self);
            }
            fn toplevel_destroyed(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend_mut(self).remove_window(&surface);
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

        impl $crate::smithay::wayland::shell::xdg::decoration::XdgDecorationHandler for $ty {
            fn new_decoration(&mut self, toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
            }
            fn request_mode(
                &mut self,
                toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface,
                _mode: $crate::smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
            ) {
                // クライアントが自分で描きたいと言っても、帯はSeinasが描く。
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
            }
            fn unset_mode(&mut self, toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
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
                    wayland::{
                        compositor::CompositorState,
                        shell::xdg::{decoration::XdgDecorationState, XdgShellState},
                        shm::ShmState,
                    },
                };

                let compositor_state = CompositorState::new::<Self>(display_handle);
                // 最大化・最小化・全画面は、できない。できることの一覧を空にして、クライアントに伝える。
                let xdg_shell_state = XdgShellState::new_with_capabilities::<Self>(
                    display_handle,
                    [] as [$crate::smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities; 0],
                );
                let xdg_decoration_state = XdgDecorationState::new::<Self>(display_handle);
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
                    xdg_decoration_state,
                    shm_state,
                    seat_state,
                    seat,
                    pointer,
                    output,
                    size: (config.width, config.height).into(),
                    windows: $crate::Stack::default(),
                    text: $crate::load_fonts(&config.font_dir),
                    title_bars: Default::default(),
                    interaction: Default::default(),
                }
            }
        }

        $crate::smithay::delegate_compositor!($ty);
        $crate::smithay::delegate_shm!($ty);
        $crate::smithay::delegate_xdg_shell!($ty);
        $crate::smithay::delegate_xdg_decoration!($ty);
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
