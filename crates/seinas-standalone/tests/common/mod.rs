//! seinas-standaloneのテストで共通に使うもの。
//!
//! seinas-standaloneを fake screen で動かし、その写し(PPM)のピクセルを見て確かめる。親のWaylandも、画面の
//! 装置も要らない。

#![allow(dead_code)]

use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Once},
    time::{Duration, Instant},
};

/// 待つ時間の上限。遅い環境でも落ちないように長めに取り、条件がそろい次第すぐ先へ進む。
pub const TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(20);

/// fake screen の大きさ。
pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 480;

// 色は (赤, 緑, 青)。
/// seinasの背景。
pub const BACKGROUND: [u8; 3] = [0x19, 0x1e, 0x28];
/// zeyes-minの、目のまわり(肌)。
pub const SKIN: [u8; 3] = [0x6a, 0xb0, 0x5c];
/// zeyes-minの白目、目の縁、瞳。
pub const SCLERA: [u8; 3] = [0xff, 0xff, 0xff];
pub const OUTLINE: [u8; 3] = [0x10, 0x10, 0x10];
pub const PUPIL: [u8; 3] = [0x14, 0x0e, 0x0a];
/// タイトルバー。activated なウィンドウのものと、そうでないもの。
pub const ACTIVE_BAR: [u8; 3] = [0x2f, 0x6f, 0xb5];
pub const INACTIVE_BAR: [u8; 3] = [0x4a, 0x50, 0x5c];
/// タイトルバーの高さ。ウィンドウのコンテンツは、この下に置かれる。
pub const BAR: usize = 24;
/// タイトルバーの左右の端から、タイトルまでの空き。
pub const TITLE_PADDING: usize = 8;
/// 閉じるボタンの大きさ。タイトルバーの右の端に、正方形で置かれる。
pub const CLOSE_BUTTON: usize = 24;
/// タイトルバーの右の端のボタンの幅(シェルの部品が無いときの、最大化と閉じるの 2 つ)。
pub const BUTTONS: usize = 2 * CLOSE_BUTTON;
/// ポインターを乗せたときの、閉じるボタンの地の色。
pub const HOT_CLOSE_BUTTON: [u8; 3] = [0xc4, 0x2b, 0x1c];

/// テストが起こした子のプロセスに付ける環境変数。値は、テストのプロセスの PID。残ったプロセスを
/// `tools/check-leftovers.sh` が見分けるのに使う。
pub const CHILD_MARK_ENV: &str = "SEINAS_TEST_CHILD";

/// テストの作業用の場所に置く印のファイル。`tools/check-leftovers.sh` と [`sweep_stale_dirs`] は、これが
/// ある場所だけを片付けの対象にする。
pub const DIR_MARK: &str = ".seinas-test";

/// テストの子のプロセスを起こすための Command。
///
/// どの終わり方でも、子が残らないようにする:
/// - ふつうの終わり方(成功、失敗、panic、timeout の panic)では、[`Running`] の Drop が kill と wait をする。
/// - Drop が走らない終わり方(`std::process::exit`、SIGINT・SIGKILL での中断、テストのプロセスの abort)では、
///   子に付けた `PR_SET_PDEATHSIG`(SIGKILL)で、カーネルが子を止める。親が先に終わっていた場合に備え、
///   付けた後で親の PID を確かめる。
///
/// `PR_SET_PDEATHSIG` は、子を起こしたスレッドが終わったときにも働く。テストの子は、テストの関数を動かす
/// スレッドで起こし、そのテストの中でだけ使うので、困らない。
pub fn child_command(program: impl AsRef<OsStr>) -> Command {
    let parent = std::process::id();
    let mut command = Command::new(program);
    command.env(CHILD_MARK_ENV, parent.to_string());
    // SAFETY: fork の後、exec の前に、async-signal-safe な prctl・getppid・_exit だけを呼ぶ。
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() as u32 != parent {
                libc::_exit(1);
            }
            Ok(())
        });
    }
    command
}

/// 前のテストのプロセスが SIGKILL などで終わり、片付けられずに残った作業用の場所を消す(印のファイルがあり、
/// 名前の終わりの PID のプロセスがもういないものだけ)。テストのプロセスごとに 1 回だけ行う。
pub fn sweep_stale_dirs() {
    static SWEEP: Once = Once::new();
    SWEEP.call_once(|| {
        let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str().filter(|name| name.starts_with("seinas-")) else {
                continue;
            };
            let Some(pid) = name
                .rsplit('-')
                .next()
                .and_then(|pid| pid.parse::<u32>().ok())
            else {
                continue;
            };
            let path = entry.path();
            if path.join(DIR_MARK).exists() && !Path::new(&format!("/proc/{pid}")).exists() {
                let _ = fs::remove_dir_all(&path);
            }
        }
    });
}

/// 終わるときに、動かしたプロセスを必ず止める。
pub struct Running(pub Child);

impl Running {
    pub fn is_running(&mut self) -> bool {
        self.0.try_wait().expect("check the process").is_none()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// テスト用のクライアント(`test-client`)と、その標準出力。
pub struct TestClient {
    pub process: Running,
    lines: mpsc::Receiver<String>,
}

impl TestClient {
    /// 標準出力の次の1行。上限まで待っても届かなければ、何を待っていたかを添えて失敗する。
    pub fn next_line(&self, what: &str) -> String {
        self.lines
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("timed out while waiting for {what}"))
    }

    /// `line` と同じ行が届くまで読む。ほかの行は読み飛ばす。
    pub fn wait_for_line(&self, line: &str) {
        while self.next_line(line) != line {}
    }

    /// `duration` の間に届いた行(届いていないことを確かめるのに使う)。
    pub fn lines_within(&self, duration: std::time::Duration) -> Vec<String> {
        let deadline = Instant::now() + duration;
        let mut lines = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.lines.recv_timeout(left) {
                Ok(line) => lines.push(line),
                Err(_) => break,
            }
        }
        lines
    }

    /// 標準入力へ、1行を送る。
    pub fn send_line(&mut self, line: &str) {
        use std::io::Write;
        let stdin = self.process.0.stdin.as_mut().expect("the client's input");
        writeln!(stdin, "{line}").expect("send a line to the client");
    }

    /// 次に届く「window: configure …」の行の、状態の並びの部分(`[activated]` など)。ほかの行は読み飛ばす。
    pub fn next_configure_states(&self, what: &str) -> String {
        loop {
            let line = self.next_line(what);
            if let Some(rest) = line.strip_prefix("window: configure ") {
                let (_, states) = rest.split_once(' ').expect("the size and the states");
                return states.to_owned();
            }
        }
    }
}

/// 条件がそろうまで待つ。上限を過ぎたら、何を待っていたかを添えて失敗する。
pub fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "timed out while waiting for {what}"
        );
        std::thread::sleep(POLL);
    }
}

/// fake screen の写し。
pub struct Shot {
    pub width: usize,
    pub height: usize,
    /// PPMのピクセルの部分(赤, 緑, 青の順)。
    pub pixels: Vec<u8>,
}

impl Shot {
    /// fake screen の写し(PPM)を読む。まだ無いか、書きかけならNone。
    pub fn read(path: &Path) -> Option<Self> {
        let image = fs::read(path).ok()?;
        // ヘッダーは「P6 幅 高さ 255」と改行。
        let end = image.iter().position(|&b| b == b'\n')?;
        let header = std::str::from_utf8(&image[..end]).ok()?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        let [magic, width, height, max] = fields[..] else {
            return None;
        };
        let (width, height): (usize, usize) = (width.parse().ok()?, height.parse().ok()?);
        let pixels = &image[end + 1..];
        (magic == "P6" && max == "255" && pixels.len() == width * height * 3).then(|| Shot {
            width,
            height,
            pixels: pixels.to_vec(),
        })
    }

    /// (x, y)の色。
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * self.width + x) * 3;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }
}

/// zeyes-minのウィンドウの大きさ。
pub const ZEYES_WIDTH: usize = 320;
pub const ZEYES_HEIGHT: usize = 240;

/// 外枠の左上が(`x0`, `y0`)にあるzeyes-minのウィンドウのコンテンツが、全体が見える形で描かれているか。
/// コンテンツは、タイトルバーの下にある。
///
/// 目のまわりの色は `skin`。ポインターが無いとき、目玉はウィンドウの中央を見る。左の目玉はコンテンツの中の
/// (114, 120)、右の目玉は(206, 120)に来る。
pub fn shows_zeyes_at(shot: &Shot, x0: usize, y0: usize, skin: [u8; 3]) -> bool {
    let at = |x: usize, y: usize| shot.pixel(x0 + x, y0 + BAR + y);
    at(2, 2) == skin
        && at(317, 237) == skin
        && at(85, 60) == SCLERA
        && at(235, 60) == SCLERA
        && at(85, 19) == OUTLINE
        && at(114, 120) == PUPIL
        && at(206, 120) == PUPIL
}

/// 既定の色のzeyes-minが、左上に、全体が見える形で描かれているか。
pub fn shows_zeyes(shot: &Shot) -> bool {
    shows_zeyes_at(shot, 0, 0, SKIN) && shot.pixel(WIDTH - 1, HEIGHT - 1) == BACKGROUND
}

/// 外枠の左上が(`x0`, `y0`)、幅が `width` のウィンドウの、タイトルバーの色。
///
/// タイトルバーの右の端(閉じるボタンの隅。タイトルの文字も「×」の印も来ない所)の、上と下のピクセルを見る。
/// 色がそろっていなければNone。ポインターが閉じるボタンの上にあるときは、ボタンの地の色になる。
pub fn bar_color(shot: &Shot, x0: usize, y0: usize, width: usize) -> Option<[u8; 3]> {
    let top = shot.pixel(x0 + width - 2, y0);
    (top == shot.pixel(x0 + width - 2, y0 + BAR - 1)).then_some(top)
}

/// 外枠の左上が(`x0`, `y0`)、幅が `width` のウィンドウのタイトルバーの中で、タイトルバーの色でないピクセル(タイトルの文字)の位置。
/// 位置は、タイトルバーの左上を原点にした(x, y)。右の端のボタン(最大化と閉じる)は、見ない。
pub fn title_ink(shot: &Shot, x0: usize, y0: usize, width: usize) -> Vec<(usize, usize)> {
    let Some(bar) = bar_color(shot, x0, y0, width) else {
        return Vec::new();
    };
    (0..BAR)
        .flat_map(|y| (0..width - BUTTONS).map(move |x| (x, y)))
        .filter(|&(x, y)| shot.pixel(x0 + x, y0 + y) != bar)
        .collect()
}

/// `tools/fetch-fonts.sh` がフォントを置く場所。
pub fn font_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts");
    assert!(
        dir.join("BIZUDPGothic-Regular.ttf").exists()
            && dir.join("unifont_jp-18.0.01.otf").exists(),
        "the fonts are missing; run tools/fetch-fonts.sh first"
    );
    dir
}

pub fn shows_only_background(shot: &Shot) -> bool {
    [
        (2, 2),
        (114, 120),
        (206, 120),
        (317, 237),
        (WIDTH - 1, HEIGHT - 1),
    ]
    .into_iter()
    .all(|(x, y)| shot.pixel(x, y) == BACKGROUND)
}

/// ワークスペースの、ほかのパッケージの実行ファイルの場所。このテストと同じ置き場にある。
///
/// Cargoは、ほかのパッケージの実行ファイルをテストのためには作らない。古いものが残っていることも
/// あるので、毎回ここでCargoを呼んで、いまのソースのものにする(変わっていなければ、すぐ終わる)。
pub fn workspace_bin(package: &str, name: &str) -> PathBuf {
    let test_exe = std::env::current_exe().expect("the path of this test");
    // <置き場>/deps/<このテスト>
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("the profile directory");
    let mut build = Command::new(env!("CARGO"));
    build.args(["build", "--quiet", "--locked", "-p", package]);
    if profile_dir
        .file_name()
        .is_some_and(|name| name == "release")
    {
        build.arg("--release");
    }
    // 置き場が target/<ターゲット名>/<debugなど> の形なら、同じターゲット向けに作る。
    let target = profile_dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| name.contains("-linux-"));
    if let Some(target) = target {
        build.args(["--target", target]);
    }
    let status = build.status().expect("run cargo");
    assert!(status.success(), "building {package} failed");
    let bin = profile_dir.join(name);
    assert!(bin.exists(), "{} was not built", bin.display());
    bin
}

/// fake screen で動かしたseinas-standaloneと、その作業用の場所。
pub struct Compositor {
    pub process: Running,
    pub socket: PathBuf,
    pub dump: PathBuf,
    dir: PathBuf,
}

impl Compositor {
    /// fake screen で起動し、最初の1枚(背景)が出て、ソケットで待ち受けるまで待つ。
    ///
    /// `name` は、作業用の場所をテストごとに分けるための短い名前。
    pub fn start(name: &str) -> Self {
        Self::start_with_fonts(name, &font_dir())
    }

    /// [`Compositor::start`] と同じだが、フォントの置き場を `fonts` にする。
    pub fn start_with_fonts(name: &str, fonts: &Path) -> Self {
        Self::start_with(name, fonts, false)
    }

    /// [`Compositor::start`] と同じだが、テスト用の入力インターフェースを開く。ポインターの知らせは、
    /// [`Compositor::pointer`] で送る。
    pub fn start_with_input(name: &str) -> Self {
        Self::start_with(name, &font_dir(), true)
    }

    /// [`Compositor::start`] と同じだが、fake screen の大きさを `width` × `height` にする。
    pub fn start_sized(name: &str, width: usize, height: usize) -> Self {
        Self::start_full(name, &font_dir(), false, (width, height), &[], &[])
    }

    /// [`Compositor::start_with_input`] と同じだが、引数 `args` も付ける(`--ping-timeout 300` など)。
    pub fn start_with_input_and(name: &str, args: &[&str]) -> Self {
        Self::start_full(name, &font_dir(), true, (WIDTH, HEIGHT), args, &[])
    }

    /// [`Compositor::start_with_input`] と同じだが、環境変数 `env` も付ける(`XKB_CONFIG_ROOT` など)。
    pub fn start_with_input_env(name: &str, env: &[(&str, &str)]) -> Self {
        Self::start_full(name, &font_dir(), true, (WIDTH, HEIGHT), &[], env)
    }

    fn start_with(name: &str, fonts: &Path, test_input: bool) -> Self {
        Self::start_full(name, fonts, test_input, (WIDTH, HEIGHT), &[], &[])
    }

    fn start_full(
        name: &str,
        fonts: &Path,
        test_input: bool,
        (width, height): (usize, usize),
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
        // ソケットのパスには長さの上限(約100文字)があるので、短い場所に置く。
        sweep_stale_dirs();
        let dir = std::env::temp_dir().join(format!("seinas-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create the working directory");
        fs::write(dir.join(DIR_MARK), "").expect("mark the working directory");
        let socket = dir.join("wl");
        let dump = dir.join("screen.ppm");
        let log = fs::File::create(dir.join("log")).expect("create the log file");
        let mut command = child_command(env!("CARGO_BIN_EXE_seinas-standalone"));
        if test_input {
            command.arg("--test-input").stdin(Stdio::piped());
        }
        let process = Running(
            command
                .args(["--fake", &format!("{width}x{height}")])
                .arg("--dump")
                .arg(&dump)
                .arg("--socket")
                .arg(&socket)
                .arg("--fonts")
                .arg(fonts)
                .args(args)
                // 外から入った指定に左右されないようにする(利用者のフォントの設定のファイルも読まない)。
                .env_remove("SEINAS_FONTS")
                .env("XDG_CONFIG_HOME", &dir)
                .envs(env.iter().copied())
                .stderr(log)
                .spawn()
                .expect("start seinas-standalone"),
        );
        let mut compositor = Compositor {
            process,
            socket,
            dump,
            dir,
        };
        // 起動の途中で終わったら(落ちたら)、待ち続けずに、終わり方とログを添えて失敗する。
        wait_for("the first frame (background only)", || {
            if let Some(status) = compositor.process.0.try_wait().expect("check the process") {
                panic!(
                    "seinas-standalone ended while starting: {status}\n{}",
                    compositor.log()
                );
            }
            Shot::read(&compositor.dump)
                .filter(|shot| {
                    shot.width == width
                        && shot.height == height
                        && if (width, height) == (WIDTH, HEIGHT) {
                            shows_only_background(shot)
                        } else {
                            shot.pixel(0, 0) == BACKGROUND
                                && shot.pixel(width - 1, height - 1) == BACKGROUND
                        }
                })
                .map(|_| ())
        });
        wait_for("the listening socket", || {
            compositor.socket.exists().then_some(())
        });
        compositor
    }

    /// 画面が `check` を満たすまで待つ。
    pub fn wait_for_screen(&self, what: &str, check: impl Fn(&Shot) -> bool) {
        wait_for(what, || {
            Shot::read(&self.dump)
                .filter(|shot| check(shot))
                .map(|_| ())
        });
    }

    /// テスト用の入力インターフェースへ、1行を送る(`motion 100 10`、`press`、`release` など)。
    pub fn pointer(&mut self, line: &str) {
        use std::io::Write;
        let stdin = self.process.0.stdin.as_mut().expect("the test input");
        writeln!(stdin, "{line}").expect("send a pointer event");
    }

    /// いまの画面。
    pub fn shot(&self) -> Shot {
        wait_for("the screen", || Shot::read(&self.dump))
    }

    /// 画面が `check` を満たすまで待ち、満たしたときの画面を返す。
    pub fn wait_for_shot(&self, what: &str, check: impl Fn(&Shot) -> bool) -> Shot {
        wait_for(what, || Shot::read(&self.dump).filter(|shot| check(shot)))
    }

    /// これまでに標準エラーへ書かれたログ。
    pub fn log(&self) -> String {
        fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }

    /// zeyes-minをつなぐ。
    pub fn connect_zeyes(&self) -> Running {
        self.connect_zeyes_with(&[])
    }

    /// 目のまわりの色を `skin`(16進の6けた)にしたzeyes-minをつなぐ。
    pub fn connect_zeyes_with_skin(&self, skin: &str) -> Running {
        self.connect_zeyes_with(&["--skin", skin])
    }

    fn connect_zeyes_with(&self, args: &[&str]) -> Running {
        Running(
            child_command(workspace_bin("zeyes-min", "zeyes-min"))
                .args(args)
                .env("WAYLAND_DISPLAY", &self.socket)
                // 外から入った色の指定に左右されないようにする。
                .env_remove("ZEYES_SKIN")
                .stderr(Stdio::null())
                .spawn()
                .expect("start zeyes-min"),
        )
    }

    /// タスクバー(seinas-taskbar)を、引数 `args` で `socket` へつなぐ。標準エラーは、作業用の場所の
    /// `taskbar.log` に書く。
    pub fn connect_taskbar_to(&self, socket: &Path, args: &[&str]) -> Running {
        let log = fs::File::create(self.dir.join("taskbar.log")).expect("create the taskbar log");
        Running(
            child_command(workspace_bin("seinas-taskbar", "seinas-taskbar"))
                .args(args)
                .arg("--fonts")
                .arg(font_dir())
                .env("WAYLAND_DISPLAY", socket)
                .env_remove("SEINAS_FONTS")
                // 時計の時差は、引数と設定のファイルで決める(外の TZ に左右されない)。
                .env_remove("TZ")
                .env("XDG_CONFIG_HOME", &self.dir)
                .stderr(log)
                .spawn()
                .expect("start seinas-taskbar"),
        )
    }

    /// タスクバーを、シェルの部品のためのソケットへつなぐ。時計は UTC。
    pub fn connect_taskbar(&self) -> Running {
        self.connect_taskbar_to(&self.shell_socket(), &["--utc-offset", "+00:00"])
    }

    /// これまでにタスクバーが標準エラーへ書いたもの。
    pub fn taskbar_log(&self) -> String {
        fs::read_to_string(self.dir.join("taskbar.log")).unwrap_or_default()
    }

    /// 壁紙(seinas-wallpaper)を、シェルの部品のためのソケットへつなぐ。設定のファイルは
    /// [`Compositor::config_dir`] の下の `seinas/wallpaper.conf`(無ければ、色だけ)。
    pub fn connect_wallpaper(&self) -> Running {
        let log =
            fs::File::create(self.dir.join("wallpaper.log")).expect("create the wallpaper log");
        Running(
            child_command(workspace_bin("seinas-wallpaper", "seinas-wallpaper"))
                .env("WAYLAND_DISPLAY", self.shell_socket())
                .env("XDG_CONFIG_HOME", &self.dir)
                .stderr(log)
                .spawn()
                .expect("start seinas-wallpaper"),
        )
    }

    /// これまでに壁紙が標準エラーへ書いたもの。
    pub fn wallpaper_log(&self) -> String {
        fs::read_to_string(self.dir.join("wallpaper.log")).unwrap_or_default()
    }

    /// シェルの部品とコンポジタが見る `XDG_CONFIG_HOME`(テストごとの作業用の場所)。
    pub fn config_dir(&self) -> &Path {
        &self.dir
    }

    /// シェルの部品のためのソケット(ふだんのソケットのパスに `-shell` を付けたもの)。
    pub fn shell_socket(&self) -> PathBuf {
        let mut name = self.socket.as_os_str().to_owned();
        name.push("-shell");
        PathBuf::from(name)
    }

    /// テスト用のクライアント(`test-client`)を、引数 `args` でつなぐ。標準入力と標準出力は、こちらで持つ。
    pub fn connect_test_client(&self, args: &[&str]) -> TestClient {
        self.connect_test_client_to(&self.socket, args)
    }

    /// テスト用のクライアントを、シェルの部品のためのソケットへつなぐ。
    pub fn connect_shell_client(&self, args: &[&str]) -> TestClient {
        self.connect_test_client_to(&self.shell_socket(), args)
    }

    /// IME のためのソケット(ふだんのソケットのパスに `-ime` を付けたもの)。
    pub fn ime_socket(&self) -> PathBuf {
        let mut name = self.socket.as_os_str().to_owned();
        name.push("-ime");
        PathBuf::from(name)
    }

    /// テスト用のクライアントを、IME のためのソケットへつなぐ。
    pub fn connect_ime_client(&self, args: &[&str]) -> TestClient {
        self.connect_test_client_to(&self.ime_socket(), args)
    }

    /// 入れ子の構成の Seinas(`seinas`)を、このコンポジタを親にして起こし、子のソケットができるまで待つ。
    /// 引数 `args` を付ける。子のソケットは、作業用の場所の `seinas-0`(`XDG_RUNTIME_DIR` をそこにする)。
    /// 標準エラーは、作業用の場所の `nested.log` に書く。親は 1 つのウィンドウとして入れ子の Seinas を出し、
    /// キーボードのフォーカスも渡すので、テスト用の入力インターフェースのキーは、入れ子の Seinas に届く。
    pub fn start_nested(&self, args: &[&str]) -> Nested {
        let log = fs::File::create(self.dir.join("nested.log")).expect("create the nested log");
        let process = Running(
            child_command(workspace_bin("seinas", "seinas"))
                .args(args)
                .arg("--fonts")
                .arg(font_dir())
                .env("WAYLAND_DISPLAY", &self.socket)
                .env("XDG_RUNTIME_DIR", &self.dir)
                .env("XDG_CONFIG_HOME", &self.dir)
                .env_remove("SEINAS_FONTS")
                .stderr(log)
                .spawn()
                .expect("start the nested seinas"),
        );
        let socket = self.dir.join("seinas-0");
        wait_for("the nested socket", || socket.exists().then_some(()));
        Nested {
            process,
            socket,
            log: self.dir.join("nested.log"),
        }
    }

    /// テスト用のクライアントを、ソケット `socket` へつなぐ(入れ子の Seinas の子など)。
    pub fn connect_test_client_at(&self, socket: &Path, args: &[&str]) -> TestClient {
        self.connect_test_client_to(socket, args)
    }

    fn connect_test_client_to(&self, socket: &Path, args: &[&str]) -> TestClient {
        let mut process = Running(
            child_command(workspace_bin("seinas-test-clients", "test-client"))
                .args(args)
                .env("WAYLAND_DISPLAY", socket)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("start the test client"),
        );
        // 標準出力は、別のスレッドで1行ずつ読む(待つ時間に上限を置くため)。
        let (sender, lines) = mpsc::channel();
        let stdout = process.0.stdout.take().expect("the client's output");
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        TestClient { process, lines }
    }

    /// スレッドの数。
    pub fn thread_count(&self) -> usize {
        fs::read_dir(format!("/proc/{}/task", self.process.0.id()))
            .map(|tasks| tasks.count())
            .unwrap_or(0)
    }
}

/// 入れ子の構成で動かした Seinas。親(このテストの [`Compositor`])より先に止める。
pub struct Nested {
    pub process: Running,
    /// 子のソケット。
    pub socket: PathBuf,
    log: PathBuf,
}

impl Nested {
    /// これまでに標準エラーへ書かれたログ。
    pub fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.process.0.kill();
        let _ = self.process.0.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
