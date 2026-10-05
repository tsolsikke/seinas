//! seinas-standalone: 親のWaylandを使わない構成のSeinas。
//!
//! Waylandの受け口(`seinas-frontend`)で受けたクライアントの画面を、共通の描画(`seinas-render`)で
//! 合成し、fbdevの裏側(`seinas-fbdev`)で画面へ出す。入れ子の裏側(SCTK、wayland-client)は含まない。
//!
//! 使い方:
//!
//! ```text
//! seinas-standalone [--device PATH] [--socket PATH] [--fonts DIR]
//! seinas-standalone --fake WIDTHxHEIGHT [--dump FILE] [--socket PATH] [--fonts DIR]
//! ```
//!
//! - `--device PATH`: 画面の装置。無ければ環境変数 `SEINAS_FBDEV`、それも無ければ `/dev/fb0`。
//! - `--fake WIDTHxHEIGHT`: 装置を開かず、その大きさの偽の画面(メモリー上)へ描く。
//! - `--dump FILE`: 偽の画面の中身を、描くたびにPPM形式の画像としてファイルに書く。
//! - `--socket PATH`: クライアントを待ち受けるソケットの場所。無ければ環境変数 `SEINAS_SOCKET`、
//!   それも無ければ `XDG_RUNTIME_DIR` の下の `seinas-0`。
//! - `--fonts DIR`: フォントの置き場(題名を描くのに使う)。無ければ環境変数 `SEINAS_FONTS`、それも
//!   無ければ `target/fonts`(`tools/fetch-fonts.sh` が置く場所)。フォントが読めなくても動く
//!   (題名の文字が出ないだけ)。
//!
//! 描く時機: クライアントがcommitして描き直しが要るときにだけ描き、描いた後にframeコールバックを
//! 返す。fbdevには垂直同期の知らせが無いので、描く回数は1秒に60回までに抑える。何も起きていない間は、
//! 何もせずに待つ。
//!
//! 制限: 入力は扱わない(wl_seatは公開しない)。クライアントのウィンドウは、決まった位置に少しずつ
//! ずらして重ねる(動かせない)。
//! 止めるにはシグナルで終わらせる。終わるときの後片付け(画面を元に戻す、ソケットのファイルを消す)は
//! していない。

mod screen;

use std::{
    cell::RefCell,
    error::Error,
    path::PathBuf,
    process::ExitCode,
    rc::Rc,
    time::{Duration, Instant},
};

use seinas_frontend::{font_dir, reap_dead_clients, BuildFrontend, Config, Frontend, FrontendHost};
use seinas_render::Painter;
use smithay::reexports::{
    calloop::{generic::Generic, EventLoop, Interest, Mode, PostAction},
    wayland_server::{Display, ListeningSocket},
};
use tracing::{error, info, warn};

use screen::Screen;

const DEFAULT_DEVICE: &str = "/dev/fb0";
const DEVICE_ENV: &str = "SEINAS_FBDEV";
const SOCKET_ENV: &str = "SEINAS_SOCKET";
/// 場所の指定が無いときの、ソケットの名前(`XDG_RUNTIME_DIR` の下)。
const DEFAULT_SOCKET_NAME: &str = "seinas-0";
/// 描く間隔の下限。1秒に60回まで。
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

const USAGE: &str = "usage: seinas-standalone [--device PATH] [--socket PATH] [--fonts DIR]
       seinas-standalone --fake WIDTHxHEIGHT [--dump FILE] [--socket PATH] [--fonts DIR]";

struct Options {
    device: PathBuf,
    fake: Option<(u32, u32)>,
    dump: Option<PathBuf>,
    socket: Option<PathBuf>,
    fonts: Option<PathBuf>,
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        device: std::env::var_os(DEVICE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DEVICE)),
        fake: None,
        dump: None,
        socket: std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        fonts: None,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--device" => options.device = PathBuf::from(value("--device")?),
            "--fake" => {
                let text = value("--fake")?;
                let size =
                    parse_size(&text).ok_or_else(|| format!("invalid --fake value: {text}"))?;
                options.fake = Some(size);
            }
            "--dump" => options.dump = Some(PathBuf::from(value("--dump")?)),
            "--socket" => options.socket = Some(PathBuf::from(value("--socket")?)),
            "--fonts" => options.fonts = Some(PathBuf::from(value("--fonts")?)),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if options.dump.is_some() && options.fake.is_none() {
        return Err("--dump needs --fake".to_owned());
    }
    Ok(options)
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// コンポジタの状態。Smithayのハンドラは、この型に実装する。
struct Standalone {
    /// Waylandの受け口。
    frontend: Frontend<Standalone>,
    /// 共通の描画。
    painter: Painter,
    /// 裏側(fbdevの装置か、偽の画面)。
    screen: Screen,
    start: Instant,
    needs_redraw: bool,
    /// 描けない要素があった。切られたクライアントの後片付けが要る。
    reap_needed: bool,
    last_draw: Option<Instant>,
}

impl FrontendHost for Standalone {
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

seinas_frontend::delegate_frontend!(Standalone);

impl Standalone {
    /// 次に描いてよい時刻までの残り。もう描いてよければ0。
    fn until_next_frame(&self) -> Duration {
        match self.last_draw {
            Some(last) => FRAME_INTERVAL.saturating_sub(last.elapsed()),
            None => Duration::ZERO,
        }
    }

    /// クライアントの画面を合成して画面へ出し、クライアントへframeコールバックを返す。
    fn draw(&mut self) -> Result<(), Box<dyn Error>> {
        let elements = self.frontend.render_elements(self.painter.renderer());
        let view = self.painter.paint(&elements)?;
        if view.failed_elements() > 0 {
            // たとえば、クライアントが共有メモリーを縮めていた場合。Smithayがそのクライアントを切るので、
            // ここでは絵をそのまま出して、動き続ける。
            warn!("{} element(s) could not be drawn", view.failed_elements());
            self.reap_needed = true;
        }
        self.screen.show(&view)?;
        drop(elements);
        self.needs_redraw = false;
        self.last_draw = Some(Instant::now());
        self.frontend.send_frame_callbacks(self.start.elapsed());
        Ok(())
    }
}

fn bind_socket(path: Option<PathBuf>) -> Result<ListeningSocket, Box<dyn Error>> {
    match path {
        Some(path) => {
            let socket = ListeningSocket::bind_absolute(path.clone())
                .map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
            info!("listening on {}", path.display());
            Ok(socket)
        }
        None => {
            let socket = ListeningSocket::bind(DEFAULT_SOCKET_NAME).map_err(|e| {
                format!("cannot listen on {DEFAULT_SOCKET_NAME} in XDG_RUNTIME_DIR: {e} (use --socket PATH)")
            })?;
            info!("listening on {DEFAULT_SOCKET_NAME} in XDG_RUNTIME_DIR");
            Ok(socket)
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let screen = match options.fake {
        Some((width, height)) => Screen::fake(width, height, options.dump)?,
        None => Screen::open(&options.device)?,
    };
    let (width, height) = screen.size();
    info!("screen: {width}x{height} ({})", screen.describe());

    let mut event_loop: EventLoop<Standalone> = EventLoop::try_new()?;
    // displayは、クライアントの要求を処理する源と、下のループの両方から使う。
    let display: Rc<RefCell<Display<Standalone>>> = Rc::new(RefCell::new(Display::new()?));
    let frontend = Standalone::build_frontend(
        &display.borrow().handle(),
        Config {
            width,
            height,
            pointer: false,
            font_dir: font_dir(options.fonts),
        },
    );
    let painter = Painter::new(width, height)?;

    // 源その1: クライアントの接続の受け付け。
    let listener = bind_socket(options.socket)?;
    event_loop.handle().insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Standalone| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept(stream);
            }
            Ok(PostAction::Continue)
        },
    )?;

    // 源その2: クライアントの要求の処理。
    let display_fd = display
        .borrow_mut()
        .backend()
        .poll_fd()
        .try_clone_to_owned()?;
    let dispatcher = display.clone();
    event_loop.handle().insert_source(
        Generic::new(display_fd, Interest::READ, Mode::Level),
        move |_, _, state: &mut Standalone| {
            // クライアントとのやり取りの失敗で、コンポジタ全体を止めない。
            if let Err(e) = dispatcher.borrow_mut().dispatch_clients(state) {
                error!("failed to dispatch the clients: {e}");
            }
            Ok(PostAction::Continue)
        },
    )?;

    let mut state = Standalone {
        frontend,
        painter,
        screen,
        start: Instant::now(),
        // クライアントがいなくても、最初に1枚(背景)を出す。
        needs_redraw: true,
        reap_needed: false,
        last_draw: None,
    };

    loop {
        if state.needs_redraw && state.until_next_frame().is_zero() {
            state.draw()?;
        }
        if std::mem::take(&mut state.reap_needed) {
            reap_dead_clients(&mut display.borrow_mut(), &mut state);
        }
        if let Err(e) = state.frontend.display_handle.flush_clients() {
            error!("failed to flush the clients: {e}");
        }
        // 描き直しが残っていれば、次に描いてよい時刻まで待つ。無ければ、何か起きるまで待つ。
        let timeout = state.needs_redraw.then(|| state.until_next_frame());
        event_loop.dispatch(timeout, &mut state)?;
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let options = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("seinas-standalone: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seinas-standalone: {e}");
            ExitCode::FAILURE
        }
    }
}
