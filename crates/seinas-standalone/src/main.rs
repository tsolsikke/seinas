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
//! - `--shell-socket PATH`: シェルの部品(タスクバーなど)のためのソケットの場所。無ければ、ふだんのソケットの
//!   パスの名前に `-shell` を入れたもの(拡張子があれば、その前。`/tmp/seinas.sock` なら `/tmp/seinas-shell.sock`。
//!   `--socket` が無ければ `XDG_RUNTIME_DIR` の下の `seinas-0-shell`)。
//!   ここからつないだクライアントにだけ、layer-shell を見せる(`docs/adr/0002-shell-components.md`)。
//! - `--ime-socket PATH`: IME のためのソケットの場所。無ければ、ふだんのソケットのパスの名前に `-ime` を入れたもの
//!   (`--shell-socket` と同じ形。`--socket` が無ければ `XDG_RUNTIME_DIR` の下の `seinas-0-ime`)。ここからつないだクライアントにだけ、
//!   input-method と virtual-keyboard を見せる(`docs/adr/0004-ime-keyboard-path.md` の 1.6)。
//! - `--fonts DIR`: フォントの置き場(タイトルを描くのに使う)。無ければ環境変数 `SEINAS_FONTS`、それも
//!   無ければ `target/fonts`(`tools/fetch-fonts.sh` が置く場所)。フォントが読めなくても動く
//!   (タイトルの文字が出ないだけ)。
//! - `--config PATH`: Seinas の設定のファイル(ウィンドウの切り替えのキーなど。`docs/config.md`)。無ければ
//!   `$XDG_CONFIG_HOME/seinas/seinas.conf`。
//! - `--log-keys`: 届いたキーを、修飾キーと keysym の名前でログに出す(`key: Alt+F6 pressed`)。
//! - `--test-input`: テスト用の入力インターフェースを開く。標準入力から、ポインターの動きとボタンを1行ずつ読む
//!   (下の「テスト用の入力インターフェース」)。これを付けたときだけ、席(wl_seat)とポインターを公開する。
//!
//! 描く時機: クライアントがcommitして描き直しが要るときにだけ描き、描いた後にframeコールバックを
//! 返す。fbdevには垂直同期の知らせが無いので、描く回数は1秒に60回までに抑える。何も起きていない間は、
//! 何もせずに待つ。
//!
//! 制限: 入力の装置は扱わない(ふだんは、wl_seatを公開しない)。そのため、ふだんの起動では、
//! ウィンドウを動かすことも、閉じることもできない。
//!
//! テスト用の入力インターフェース: `--test-input` を付けると、標準入力の1行を、1つのポインターの知らせとして扱う。
//! ピクセルのテストが、ドラッグや閉じるボタンを確かめるのに使う。ふだんの起動では、開かない。
//!
//! ```text
//! motion X Y        ポインターが、画面の上の(X, Y)へ動いた
//! press [BUTTON]    いまの位置で、ボタンが押された(BUTTON は left か right。無ければ left)
//! release [BUTTON]  いまの位置で、ボタンが離された
//! leave             ポインターが、画面の外へ出た
//! ```
//! 止めるにはシグナルで終わらせる。終わるときの後片付け(画面を元に戻す、ソケットのファイルを消す)は
//! していない。

mod screen;

use std::{
    cell::RefCell,
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    rc::Rc,
    time::{Duration, Instant},
};

use seinas_frontend::{
    check_pings, default_config_file, font_dir,
    geometry::{PointF, Size},
    keyboard_input, pointer_input, read_switch_keys, reap_dead_clients, set_locked_modifiers,
    BuildFrontend, Config, Frontend, FrontendHost, KeyboardConfig, PointerInput,
    DEFAULT_PING_TIMEOUT, LEFT_BUTTON,
};
use seinas_render::Painter;
use smithay::reexports::{
    calloop::{channel, generic::Generic, EventLoop, Interest, Mode, PostAction},
    wayland_server::{Display, ListeningSocket},
};
use tracing::{error, info, warn};

use screen::Screen;

const DEFAULT_DEVICE: &str = "/dev/fb0";
const DEVICE_ENV: &str = "SEINAS_FBDEV";
const SOCKET_ENV: &str = "SEINAS_SOCKET";
/// 場所の指定が無いときの、ソケットの名前(`XDG_RUNTIME_DIR` の下)。
const DEFAULT_SOCKET_NAME: &str = "seinas-0";
/// シェルの部品のためのソケットの名前に付けるもの。
const SHELL_SOCKET_SUFFIX: &str = "-shell";
/// IME のためのソケットの名前に付けるもの。
const IME_SOCKET_SUFFIX: &str = "-ime";
/// 描く間隔の下限。1秒に60回まで。
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// 右のボタンの番号(Linuxの入力の決まり)。
const RIGHT_BUTTON: u32 = 0x111;

const USAGE: &str = "usage: seinas-standalone [--device PATH] [--socket PATH] [--shell-socket PATH] [--ime-socket PATH] [--fonts DIR] [--font-config PATH]
       seinas-standalone --fake WIDTHxHEIGHT [--dump FILE] [--socket PATH] [--shell-socket PATH] [--ime-socket PATH] [--fonts DIR] [--font-config PATH] [--test-input] [--ping-timeout MS] [--config PATH] [--log-keys]";

struct Options {
    device: PathBuf,
    fake: Option<(u32, u32)>,
    dump: Option<PathBuf>,
    socket: Option<PathBuf>,
    shell_socket: Option<PathBuf>,
    ime_socket: Option<PathBuf>,
    fonts: Option<PathBuf>,
    font_config: Option<PathBuf>,
    test_input: bool,
    /// 閉じるように頼んだクライアントの pong を待つ時間(テストで短くするため)。
    ping_timeout: Duration,
    /// Seinas の設定のファイル(`--config`)。Noneなら既定の場所。
    config: Option<PathBuf>,
    /// 届いたキーをログに出すか(`--log-keys`)。
    log_keys: bool,
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        device: std::env::var_os(DEVICE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DEVICE)),
        fake: None,
        dump: None,
        socket: std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        shell_socket: None,
        ime_socket: None,
        fonts: None,
        font_config: None,
        test_input: false,
        ping_timeout: DEFAULT_PING_TIMEOUT,
        config: None,
        log_keys: false,
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
            "--shell-socket" => {
                options.shell_socket = Some(PathBuf::from(value("--shell-socket")?))
            }
            "--ime-socket" => options.ime_socket = Some(PathBuf::from(value("--ime-socket")?)),
            "--fonts" => options.fonts = Some(PathBuf::from(value("--fonts")?)),
            "--font-config" => options.font_config = Some(PathBuf::from(value("--font-config")?)),
            "--test-input" => options.test_input = true,
            "--config" => options.config = Some(PathBuf::from(value("--config")?)),
            "--log-keys" => options.log_keys = true,
            "--ping-timeout" => {
                let text = value("--ping-timeout")?;
                let ms = text
                    .parse()
                    .map_err(|_| format!("invalid --ping-timeout value: {text}"))?;
                options.ping_timeout = Duration::from_millis(ms);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if options.dump.is_some() && options.fake.is_none() {
        return Err("--dump needs --fake".to_owned());
    }
    Ok(options)
}

/// テスト用の入力インターフェースに届いた1行を、ポインターの知らせに直す。`at` は、ポインターのいまの位置で、
/// `motion` の行が届くたびに書き換える。
/// テスト用の入力インターフェースの 1 行。
#[derive(Debug, Clone, Copy, PartialEq)]
enum TestInput {
    Pointer(PointerInput),
    /// キー(evdev の番号)を押した・離した。
    Key {
        code: u32,
        pressed: bool,
    },
    /// 固定された修飾キー(入れ子の構成で、親から届く wl_keyboard の mods_locked と同じもの)。
    Locked(u32),
    /// 出力(画面)の大きさが変わった(入れ子の構成で、親のウィンドウの大きさが変わったときと同じもの)。fake screen だけ。
    Output(u32, u32),
}

fn parse_test_input(line: &str, at: &mut PointF) -> Result<TestInput, String> {
    if let Some(rest) = line.strip_prefix("key ") {
        let mut words = rest.split_whitespace();
        let code = words
            .next()
            .and_then(|code| code.parse().ok())
            .ok_or_else(|| format!("key needs a key code: {line}"))?;
        let pressed = match words.next() {
            Some("press") => true,
            Some("release") => false,
            _ => return Err(format!("key needs press or release: {line}")),
        };
        return Ok(TestInput::Key { code, pressed });
    }
    if let Some(rest) = line.strip_prefix("output ") {
        let (width, height) =
            parse_size(rest.trim()).ok_or_else(|| format!("output needs WIDTHxHEIGHT: {line}"))?;
        return Ok(TestInput::Output(width, height));
    }
    if let Some(rest) = line.strip_prefix("locked ") {
        let mask = rest
            .trim()
            .parse()
            .map_err(|_| format!("locked needs a number: {line}"))?;
        return Ok(TestInput::Locked(mask));
    }
    parse_pointer_input(line, at).map(TestInput::Pointer)
}

fn parse_pointer_input(line: &str, at: &mut PointF) -> Result<PointerInput, String> {
    let mut words = line.split_whitespace();
    let command = words.next().ok_or("an empty line")?;
    match command {
        "motion" => {
            let mut number = || -> Option<f64> { words.next()?.parse().ok() };
            let (x, y) = number()
                .zip(number())
                .ok_or_else(|| format!("motion needs two numbers: {line}"))?;
            *at = (x, y).into();
            Ok(PointerInput::Motion(*at))
        }
        "press" | "release" => {
            let button = match words.next() {
                None | Some("left") => LEFT_BUTTON,
                Some("right") => RIGHT_BUTTON,
                Some(other) => return Err(format!("unknown button: {other}")),
            };
            Ok(PointerInput::Button {
                location: *at,
                button,
                pressed: command == "press",
            })
        }
        "leave" => Ok(PointerInput::Leave),
        other => Err(format!("unknown command: {other}")),
    }
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
    /// 出力の大きさを `width` × `height` に変える(画面、共通のレンダラー、フロントエンド)。
    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
        if (width as i32, height as i32) == self.screen.size() {
            return Ok(());
        }
        if width == 0 || height == 0 {
            return Err(format!("invalid output size: {width}x{height}").into());
        }
        self.screen.resize(width, height)?;
        self.painter = Painter::new(width as i32, height as i32)?;
        self.frontend
            .resize_output(Size::new(width as i32, height as i32));
        self.needs_redraw = true;
        Ok(())
    }

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

/// ソケットを listen する。`path` があれば、その絶対パス。無ければ、`XDG_RUNTIME_DIR` の下の `name`。
fn bind_socket(path: Option<PathBuf>, name: &str) -> Result<ListeningSocket, Box<dyn Error>> {
    match path {
        Some(path) => {
            let socket = ListeningSocket::bind_absolute(path.clone())
                .map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
            info!("listening on {}", path.display());
            Ok(socket)
        }
        None => {
            let socket = ListeningSocket::bind(name).map_err(|e| {
                format!("cannot listen on {name} in XDG_RUNTIME_DIR: {e} (use --socket PATH)")
            })?;
            info!("listening on {name} in XDG_RUNTIME_DIR");
            Ok(socket)
        }
    }
}

/// シェルの部品や IME のためのソケットの場所。指定(`given`)が無ければ、ふだんのソケットのパスの名前に `suffix`
/// (`-shell`、`-ime`)を入れる。名前に拡張子があれば、その前に入れる(`/tmp/seinas.sock` なら
/// `/tmp/seinas-shell.sock`)。ロックファイルは、ソケットのパスの拡張子を `.lock` に替えた名前になる
/// (wayland-server の `ListeningSocket`)ので、終わりに付けると `/tmp/seinas.lock` を取り合うため。
fn derived_socket_path(
    socket: Option<&PathBuf>,
    given: Option<PathBuf>,
    suffix: &str,
) -> Option<PathBuf> {
    given.or_else(|| socket.map(|path| with_suffix(path, suffix)))
}

/// パス `path` の名前の拡張子の前(拡張子が無ければ終わり)に `suffix` を入れる。
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    match (path.file_stem(), path.extension()) {
        (Some(stem), Some(extension)) => {
            let mut name = stem.to_owned();
            name.push(suffix);
            name.push(".");
            name.push(extension);
            path.with_file_name(name)
        }
        _ => {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            PathBuf::from(name)
        }
    }
}

/// 3 つのソケットのロックファイル(ソケットのパスの拡張子を `.lock` に替えたもの)がぶつかるなら、その説明。
/// `--shell-socket` や `--ime-socket` で、拡張子の前が同じパスを選んだときに起きる。
fn lock_conflict(paths: &[&Path]) -> Option<String> {
    for (i, a) in paths.iter().enumerate() {
        for b in &paths[i + 1..] {
            let lock = a.with_extension("lock");
            if lock == b.with_extension("lock") {
                return Some(format!(
                    "the sockets {} and {} would share the lock file {}; choose paths whose names differ before the last dot",
                    a.display(),
                    b.display(),
                    lock.display()
                ));
            }
        }
    }
    None
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
            // seat とポインターとキーボードは、テスト用の入力インターフェースを開いたときだけ公開する。
            pointer: options.test_input,
            // 親のいない構成なので、keymap は日本語の配列(jp106)。
            keyboard: options.test_input.then(KeyboardConfig::jp106),
            font_dir: font_dir(options.fonts),
            font_config: options.font_config.clone(),
            ping_timeout: options.ping_timeout,
            // 入れ子でない構成なので、切り替えの既定は Alt+Tab と Alt+Shift+Tab。
            switch_keys: read_switch_keys(
                options
                    .config
                    .clone()
                    .or_else(default_config_file)
                    .as_deref(),
                false,
            ),
            log_keys: options.log_keys,
        },
    );
    let painter = Painter::new(width, height)?;

    // 源その1: クライアントの接続の受け付け。ふだんのソケットと、シェルの部品のためのソケットと、IME のための
    // ソケット。
    let shell_path = derived_socket_path(
        options.socket.as_ref(),
        options.shell_socket,
        SHELL_SOCKET_SUFFIX,
    );
    let ime_path = derived_socket_path(
        options.socket.as_ref(),
        options.ime_socket,
        IME_SOCKET_SUFFIX,
    );
    // パスの指定が無いソケットは、XDG_RUNTIME_DIR の下の既定の名前で作るので、それも比べる。
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let shell_name = format!("{DEFAULT_SOCKET_NAME}{SHELL_SOCKET_SUFFIX}");
    let ime_name = format!("{DEFAULT_SOCKET_NAME}{IME_SOCKET_SUFFIX}");
    let effective = |given: Option<&PathBuf>, name: &str| {
        given
            .cloned()
            .or_else(|| runtime_dir.as_ref().map(|dir| dir.join(name)))
    };
    let paths: Vec<PathBuf> = [
        effective(options.socket.as_ref(), DEFAULT_SOCKET_NAME),
        effective(shell_path.as_ref(), &shell_name),
        effective(ime_path.as_ref(), &ime_name),
    ]
    .into_iter()
    .flatten()
    .collect();
    let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    if let Some(conflict) = lock_conflict(&paths) {
        return Err(conflict.into());
    }
    let listener = bind_socket(options.socket, DEFAULT_SOCKET_NAME)?;
    event_loop.handle().insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Standalone| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept(stream);
            }
            Ok(PostAction::Continue)
        },
    )?;
    let shell_listener = bind_socket(shell_path, &shell_name)?;
    event_loop.handle().insert_source(
        Generic::new(shell_listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Standalone| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept_shell(stream);
            }
            Ok(PostAction::Continue)
        },
    )?;
    let ime_listener = bind_socket(ime_path, &ime_name)?;
    event_loop.handle().insert_source(
        Generic::new(ime_listener, Interest::READ, Mode::Level),
        |_, listener, state: &mut Standalone| {
            while let Some(stream) = listener.accept()? {
                state.frontend.accept_ime(stream);
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

    // 源その3(テストのときだけ): 標準入力から届く、ポインターの知らせ。
    if options.test_input {
        warn!("the test input is open; pointer and key events are read from the standard input");
        let (sender, lines) = channel::channel::<String>();
        // 標準入力は、別のスレッドで1行ずつ読む。閉じられたら、スレッドは終わる。
        std::thread::Builder::new()
            .name("test input".to_owned())
            .spawn(move || {
                for line in std::io::stdin().lines().map_while(Result::ok) {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            })?;
        let mut at = PointF::new(0.0, 0.0);
        event_loop
            .handle()
            .insert_source(lines, move |event, _, state: &mut Standalone| {
                let channel::Event::Msg(line) = event else {
                    return;
                };
                let time = state.start.elapsed().as_millis() as u32;
                match parse_test_input(&line, &mut at) {
                    Ok(TestInput::Pointer(input)) => pointer_input(state, input, time),
                    Ok(TestInput::Key { code, pressed }) => {
                        keyboard_input(state, code, pressed, time);
                    }
                    Ok(TestInput::Locked(mask)) => {
                        set_locked_modifiers(state, mask);
                    }
                    Ok(TestInput::Output(width, height)) => {
                        if let Err(e) = state.resize(width, height) {
                            warn!("test input: {e}");
                        }
                    }
                    Err(message) => warn!("test input: {message}"),
                }
            })?;
    }

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
        // 再描画が残っていれば、次に描いてよい時刻まで待つ。無ければ、何か起きるまで待つ。pong の期限があれば、
        // そこまでで起きる。
        let redraw = state.needs_redraw.then(|| state.until_next_frame());
        let deadline = state
            .frontend
            .pings
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let timeout = match (redraw, deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
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
