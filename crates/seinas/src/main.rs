//! Seinas: ZeikOS向けのWaylandコンポジタ。
//!
//! 今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中で子のクライアントを合成して見せる
//! 「入れ子」の形で動く。
//!
//! - `seinas-frontend`: Waylandの受け口(Smithay)。子のクライアントの要求を受ける。
//! - `seinas-render`: 共通の描画。受け口が並べた要素を、1枚の絵に合成する。
//! - `parent`: 裏側(SCTK)。合成した絵を、親のWaylandへ出す。
//!
//! 3つは1つの状態 [`Seinas`] にまとまり、1つのcalloopの上で動く。
//!
//! 環境変数:
//! - `WAYLAND_DISPLAY` / `XDG_RUNTIME_DIR`: 親のソケット。
//! - 子に見せるソケットは `seinas-0`(`XDG_RUNTIME_DIR` の下)。子は `WAYLAND_DISPLAY=seinas-0` でつなぐ。
//! - `SEINAS_FONTS`: フォントの置き場(題名を描くのに使う)。引数 `--fonts DIR` でも指定できる。どちらも
//!   無ければ `target/fonts`(`tools/fetch-fonts.sh` が置く場所)。フォントが読めなくても動く
//!   (題名の文字が出ないだけ)。
//!
//! 制限: 大きさは800x600に固定で、親からのサイズ変更には応じない。入力はポインターだけを渡す。

mod parent;

use std::{
    cell::RefCell,
    error::Error,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use seinas_frontend::{font_dir, reap_dead_clients, BuildFrontend, Config, Frontend, FrontendHost};
use seinas_render::Painter;
use smithay::reexports::{
    calloop::{generic::Generic, EventLoop, Interest, Mode, PostAction},
    wayland_server::{Display, ListeningSocket},
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use tracing::{error, info, warn};
use wayland_client::{globals::registry_queue_init, Connection, QueueHandle};

use parent::Parent;

/// 画面の大きさ(固定)。
const WIDTH: i32 = 800;
const HEIGHT: i32 = 600;
/// 子に見せるソケットの名前。
const CHILD_SOCKET: &str = "seinas-0";

/// コンポジタの状態。SmithayのハンドラもSCTKのハンドラも、この1つの型に実装する。
pub struct Seinas {
    /// Waylandの受け口(子のクライアントに見せる側)。
    frontend: Frontend<Seinas>,
    /// 共通の描画。
    painter: Painter,
    /// 親のWaylandへの提出(親に対してはクライアント)。
    parent: Parent,
    start: Instant,
    needs_redraw: bool,
    /// 描けない要素があった。切られたクライアントの後片付けが要る。
    reap_needed: bool,
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
        let mut reap_needed = false;
        let result = self
            .painter
            .paint(&elements)
            .map_err(Box::<dyn Error>::from)
            .and_then(|view| {
                if view.failed_elements() > 0 {
                    // たとえば、クライアントが共有メモリーを縮めていた場合。Smithayがそのクライアントを
                    // 切るので、ここでは絵をそのまま出して、動き続ける。
                    warn!("{} element(s) could not be drawn", view.failed_elements());
                    reap_needed = true;
                }
                self.parent.present(&view, qh)
            });
        self.reap_needed |= reap_needed;
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

impl FrontendHost for Seinas {
    fn frontend(&self) -> &Frontend<Self> {
        &self.frontend
    }
    fn frontend_mut(&mut self) -> &mut Frontend<Self> {
        &mut self.frontend
    }
    fn redraw_needed(&mut self) {
        self.needs_redraw = true;
    }
}

seinas_frontend::delegate_frontend!(Seinas);

/// 引数を読む。受け付けるのは `--fonts DIR`(フォントの置き場)だけ。
fn parse_fonts(mut args: impl Iterator<Item = String>) -> Result<Option<PathBuf>, String> {
    let mut fonts = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fonts" => fonts = Some(PathBuf::from(args.next().ok_or("--fonts needs a value")?)),
            other => {
                return Err(format!(
                    "unknown argument: {other} (usage: seinas [--fonts DIR])"
                ))
            }
        }
    }
    Ok(fonts)
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let fonts = parse_fonts(std::env::args().skip(1))?;

    let mut event_loop: EventLoop<Seinas> = EventLoop::try_new()?;

    // 源その1: 親のWaylandとの接続。
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init::<Seinas>(&conn)?;
    let qh = event_queue.handle();
    WaylandSource::new(conn.clone(), event_queue).insert(event_loop.handle())?;
    let parent = Parent::new(&globals, &qh, WIDTH, HEIGHT)?;

    // displayは、クライアントの要求を処理する源と、下のループの両方から使う。
    let display: Rc<RefCell<Display<Seinas>>> = Rc::new(RefCell::new(Display::new()?));
    let frontend = Seinas::build_frontend(
        &display.borrow().handle(),
        Config {
            width: WIDTH,
            height: HEIGHT,
            pointer: true,
            font_dir: font_dir(fonts),
        },
    );
    let painter = Painter::new(WIDTH, HEIGHT)?;

    // 源その2: 子の接続の受け付け。
    let listener = ListeningSocket::bind(CHILD_SOCKET)?;
    info!("listening on {CHILD_SOCKET} (run a client with WAYLAND_DISPLAY={CHILD_SOCKET})");
    event_loop.handle().insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Seinas| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept(stream);
            }
            Ok(PostAction::Continue)
        },
    )?;

    // 源その3: 子の要求の処理。
    let display_fd = display
        .borrow_mut()
        .backend()
        .poll_fd()
        .try_clone_to_owned()?;
    let dispatcher = display.clone();
    event_loop.handle().insert_source(
        Generic::new(display_fd, Interest::READ, Mode::Level),
        move |_, _, state: &mut Seinas| {
            // クライアントとのやり取りの失敗で、コンポジタ全体を止めない。
            if let Err(e) = dispatcher.borrow_mut().dispatch_clients(state) {
                error!("failed to dispatch the clients: {e}");
            }
            if let Err(e) = dispatcher.borrow_mut().flush_clients() {
                error!("failed to flush the clients: {e}");
            }
            Ok(PostAction::Continue)
        },
    )?;

    let mut state = Seinas {
        frontend,
        painter,
        parent,
        start: Instant::now(),
        needs_redraw: false,
        reap_needed: false,
        exit: false,
    };

    while !state.exit {
        event_loop.dispatch(Duration::from_millis(16), &mut state)?;
        // 子がcommitしたのに親のframeコールバックを待っていないとき(最初の1回など)は、ここで描く。
        if state.needs_redraw {
            state.draw(&qh);
        }
        if std::mem::take(&mut state.reap_needed) {
            reap_dead_clients(&mut display.borrow_mut(), &mut state);
        }
        if let Err(e) = state.frontend.display_handle.flush_clients() {
            error!("failed to flush the clients: {e}");
        }
        conn.flush()?;
    }
    info!("exiting");
    Ok(())
}
