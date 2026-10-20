//! Seinas: ZeikOS向けのWaylandコンポジタ。
//!
//! 今は、親のWayland(WSLgなど)の中にウィンドウを1つ開き、その中で子のクライアントを合成して見せる
//! 「入れ子」の形で動く。
//!
//! - `seinas-frontend`: Waylandのフロントエンド(Smithay)。子のクライアントの要求を受ける。
//! - `seinas-render`: 共通のレンダラー。フロントエンドが並べた要素を、1枚の絵に合成する。
//! - `parent`: バックエンド(SCTK)。合成した絵を、親のWaylandへ出す。
//!
//! 3つは1つの状態 [`Seinas`] にまとまり、1つのcalloopの上で動く。
//!
//! 環境変数:
//! - `WAYLAND_DISPLAY` / `XDG_RUNTIME_DIR`: 親のソケット。
//! - 子に見せるソケットは `seinas-0`(`XDG_RUNTIME_DIR` の下)。子は `WAYLAND_DISPLAY=seinas-0` でつなぐ。
//! - シェルの部品(タスクバーなど)のためのソケットは `seinas-0-shell`。ここからつないだクライアントにだけ、
//!   layer-shell を見せる(`docs/adr/0002-shell-components.md`)。
//! - IME のためのソケットは `seinas-0-ime`。ここからつないだクライアントにだけ、input-method と virtual-keyboard を
//!   見せる(`docs/adr/0004-ime-keyboard-path.md` の 1.6)。
//! - `SEINAS_FONTS`: フォントの置き場(タイトルを描くのに使う)。引数 `--fonts DIR` でも指定できる。どちらも
//!   無ければ `target/fonts`(`tools/fetch-fonts.sh` が置く場所)。フォントが読めなくても動く
//!   (タイトルの文字が出ないだけ)。
//!
//! 引数:`--fonts DIR`、`--font-config PATH`(フォントの設定)、`--config PATH`(Seinas の設定。ウィンドウの切り替えの
//! キーなど。無ければ `$XDG_CONFIG_HOME/seinas/seinas.conf`)、`--log-keys`(届いたキーをログに出す)。

mod parent;

use std::{cell::RefCell, error::Error, path::PathBuf, rc::Rc, time::Instant};

use seinas_frontend::{
    check_pings, font_dir, geometry::Size, reap_dead_clients, BuildFrontend, Config, Frontend,
    FrontendHost, KeyboardConfig,
};
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
/// シェルの部品のためのソケットの名前。
const SHELL_SOCKET: &str = "seinas-0-shell";
/// IME のためのソケットの名前(ADR 0004 の 1.6)。
const IME_SOCKET: &str = "seinas-0-ime";

/// コンポジタの状態。SmithayのハンドラもSCTKのハンドラも、この1つの型に実装する。
pub struct Seinas {
    /// Waylandのフロントエンド(子のクライアントに見せる側)。
    frontend: Frontend<Seinas>,
    /// 共通のレンダラー。
    painter: Painter,
    /// 親のWaylandへの提出(親に対してはクライアント)。
    parent: Parent,
    start: Instant,
    needs_redraw: bool,
    /// 描けない要素があった。切られたクライアントのクリーンアップが要る。
    reap_needed: bool,
    exit: bool,
    /// 最初の 1 枚を親へ present したか(ログに 1 回だけ出すため)。
    presented: bool,
}

impl Seinas {
    /// 子の画面を合成し、親へ出す。出した後、子へframeコールバックを返す。
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.parent.can_present() {
            // 親がまだ前の絵を使っているかもしれない。親のframeコールバックを待つ。
            return;
        }
        // 親のウィンドウの大きさが変わっていたら、出力と共通のレンダラーの大きさを合わせる(ドラッグの間に届いた
        // configure のうち、最後のものだけ)。
        if let Some((width, height)) = self.parent.take_pending_size() {
            match Painter::new(width, height) {
                Ok(painter) => {
                    self.painter = painter;
                    self.frontend.resize_output(Size::new(width, height));
                }
                Err(e) => {
                    error!("cannot resize to {width}x{height}: {e}");
                    self.exit = true;
                    return;
                }
            }
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
        if !self.presented {
            // 起動を見る側が、この行で「最初の 1 枚が出た」ことを知る。文言を変えるときは、探す側にも知らせる。
            self.presented = true;
            info!(
                "presented the first frame ({}x{})",
                self.painter.width(),
                self.painter.height()
            );
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

/// 起動の引数。
#[derive(Default)]
struct Options {
    /// 同梱のフォントの置き場(`--fonts DIR`)。
    fonts: Option<PathBuf>,
    /// フォントの設定のファイル(`--font-config PATH`)。
    font_config: Option<PathBuf>,
    /// Seinas の設定のファイル(`--config PATH`。ウィンドウの切り替えのキーなど)。Noneなら既定の場所。
    config: Option<PathBuf>,
    /// 届いたキーを、修飾キーと keysym の名前でログに出す(`--log-keys`)。
    log_keys: bool,
}

const USAGE: &str = "usage: seinas [--fonts DIR] [--font-config PATH] [--config PATH] [--log-keys]";

/// 引数を読む。
fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--fonts" => options.fonts = Some(value("--fonts")?),
            "--font-config" => options.font_config = Some(value("--font-config")?),
            "--config" => options.config = Some(value("--config")?),
            "--log-keys" => options.log_keys = true,
            other => return Err(format!("unknown argument: {other} ({USAGE})")),
        }
    }
    Ok(options)
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let options = parse_options(std::env::args().skip(1))?;

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
            // keymap は、親の keymap が届いたら、それに置き換える(parent.rs)。届くまでは xkbcommon の既定。
            keyboard: Some(KeyboardConfig::default()),
            font_dir: font_dir(options.fonts),
            font_config: options.font_config,
            ping_timeout: seinas_frontend::DEFAULT_PING_TIMEOUT,
            // 入れ子の構成なので、切り替えの既定に Alt+F6 と Alt+Shift+F6 を足す(親の側の Windows が Alt+Tab を
            // 先に使うため)。
            switch_keys: seinas_frontend::read_switch_keys(
                options
                    .config
                    .or_else(seinas_frontend::default_config_file)
                    .as_deref(),
                true,
            ),
            log_keys: options.log_keys,
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
    let shell_listener = ListeningSocket::bind(SHELL_SOCKET)?;
    info!("listening on {SHELL_SOCKET} for the shell components");
    event_loop.handle().insert_source(
        Generic::new(shell_listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Seinas| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept_shell(stream);
            }
            Ok(PostAction::Continue)
        },
    )?;
    let ime_listener = ListeningSocket::bind(IME_SOCKET)?;
    info!("listening on {IME_SOCKET} for the input method");
    event_loop.handle().insert_source(
        Generic::new(ime_listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Seinas| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept_ime(stream);
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
        presented: false,
    };

    while !state.exit {
        // 何か起きるまで待つ(親の Wayland の接続、子のソケットと接続)。決まった間隔では起きない。pong の期限が
        // あれば、そこまでで起きる。描く歩調は、親の frame コールバックで決まる。
        let deadline = state
            .frontend
            .pings
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        event_loop.dispatch(deadline, &mut state)?;
        // 子がcommitしたのに親のframeコールバックを待っていないとき(最初の1回など)は、ここで描く。親が前の絵を
        // まだ使っていれば、親の frame コールバックが届いたときに描く(`parent.rs`)。
        if state.needs_redraw {
            state.draw(&qh);
        }
        // 閉じるように頼んだクライアントの pong の期限を確かめる。
        check_pings(&mut state, Instant::now());
        // 接続を切ったクライアント(応答なしで閉じるボタンを押したもの、緊急解除で切った IME)のクリーンアップも、
        // ここで行う。
        let reap = std::mem::take(&mut state.frontend.reap_requested);
        if std::mem::take(&mut state.reap_needed) || reap {
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
