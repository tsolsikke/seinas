//! Seinas: ZeikOS向けのWaylandコンポジタ。
//!
//! 今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中で子のクライアントを合成して見せる
//! 「入れ子」の形で動く。
//!
//! - `frontend`: Waylandの受け口(Smithay)。子のクライアントの要求を受ける。
//! - `seinas-render`: 共通の描画。受け口が並べた要素を、1枚の絵に合成する。
//! - `parent`: 裏側(SCTK)。合成した絵を、親のWaylandへ出す。
//!
//! 3つは1つの状態 [`Seinas`] にまとまり、1つのcalloopの上で動く。
//!
//! 環境変数:
//! - `WAYLAND_DISPLAY` / `XDG_RUNTIME_DIR`: 親のソケット。
//! - 子に見せるソケットは `seinas-0`(`XDG_RUNTIME_DIR` の下)。子は `WAYLAND_DISPLAY=seinas-0` でつなぐ。
//!
//! 制限: 大きさは800x600に固定で、親からのサイズ変更には応じない。入力はポインターだけを渡す。

mod frontend;
mod parent;

use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use seinas_render::Painter;
use smithay::reexports::{
    calloop::{generic::Generic, EventLoop, Interest, Mode, PostAction},
    wayland_server::{Display, ListeningSocket},
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use tracing::{error, info};
use wayland_client::{globals::registry_queue_init, Connection, QueueHandle};

use frontend::{ClientState, Frontend};
use parent::Parent;

/// 画面の大きさ(固定)。
const WIDTH: i32 = 800;
const HEIGHT: i32 = 600;
/// 子に見せるソケットの名前。
const CHILD_SOCKET: &str = "seinas-0";

/// コンポジタの状態。SmithayのハンドラもSCTKのハンドラも、この1つの型に実装する。
pub struct Seinas {
    /// Waylandの受け口(子のクライアントに見せる側)。
    frontend: Frontend,
    /// 共通の描画。
    painter: Painter,
    /// 親のWaylandへの提出(親に対してはクライアント)。
    parent: Parent,
    start: Instant,
    needs_redraw: bool,
    exit: bool,
}

impl Seinas {
    /// 子の画面を合成し、親へ出す。出した後、子へframeコールバックを返す。
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.parent.can_present() {
            // 親がまだ前の絵を使っているかもしれない。親のframeコールバックを待つ。
            return;
        }
        let elements = self.frontend.render_elements(self.painter.renderer());
        let result = self
            .painter
            .paint(&elements)
            .map_err(Box::<dyn Error>::from)
            .and_then(|view| self.parent.present(&view, qh));
        drop(elements);
        if let Err(e) = result {
            error!("drawing failed: {e}");
            self.exit = true;
            return;
        }
        self.needs_redraw = false;
        self.frontend.send_frame_callbacks(self.start.elapsed());
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let mut event_loop: EventLoop<Seinas> = EventLoop::try_new()?;

    // 源その1: 親のWaylandとの接続。
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init::<Seinas>(&conn)?;
    let qh = event_queue.handle();
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle())?;
    let parent = Parent::new(&globals, &qh, WIDTH, HEIGHT)?;

    let mut display: Display<Seinas> = Display::new()?;
    let frontend = Frontend::new(&display.handle(), WIDTH, HEIGHT);
    let painter = Painter::new(WIDTH, HEIGHT)?;

    // 源その2: 子の接続の受け付け。
    let listener = ListeningSocket::bind(CHILD_SOCKET)?;
    info!("listening on {CHILD_SOCKET} (run a client with WAYLAND_DISPLAY={CHILD_SOCKET})");
    event_loop.handle().insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Seinas| {
            while let Some(stream) = listener.accept()? {
                if let Err(e) = state
                    .frontend
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    error!("failed to accept a client: {e}");
                }
            }
            Ok(PostAction::Continue)
        },
    )?;

    // 源その3: 子の要求の処理。displayは閉包が持つ。
    let display_fd = display.backend().poll_fd().try_clone_to_owned()?;
    event_loop.handle().insert_source(
        Generic::new(display_fd, Interest::READ, Mode::Level),
        move |_, _, state: &mut Seinas| {
            display.dispatch_clients(state)?;
            display.flush_clients()?;
            Ok(PostAction::Continue)
        },
    )?;

    let mut state = Seinas {
        frontend,
        painter,
        parent,
        start: Instant::now(),
        needs_redraw: false,
        exit: false,
    };

    while !state.exit {
        event_loop.dispatch(Duration::from_millis(16), &mut state)?;
        // 子がcommitしたのに親のframeコールバックを待っていないとき(最初の1回など)は、ここで描く。
        if state.needs_redraw {
            state.draw(&qh);
        }
        if let Err(e) = state.frontend.display_handle.flush_clients() {
            error!("failed to flush the clients: {e}");
        }
        conn.flush()?;
    }
    info!("exiting");
    Ok(())
}
