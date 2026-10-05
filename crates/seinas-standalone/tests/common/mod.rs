//! seinas-standaloneの試験で共通に使うもの。
//!
//! seinas-standaloneを偽の画面で動かし、その写し(PPM)の画素を見て確かめる。親のWaylandも、画面の
//! 装置も要らない。

#![allow(dead_code)]

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

/// 待つ時間の上限。遅い環境でも落ちないように長めに取り、条件がそろい次第すぐ先へ進む。
pub const TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(20);

/// 偽の画面の大きさ。
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
/// 題名の帯。選ばれているウィンドウのものと、そうでないもの。
pub const ACTIVE_BAR: [u8; 3] = [0x2f, 0x6f, 0xb5];
pub const INACTIVE_BAR: [u8; 3] = [0x4a, 0x50, 0x5c];
/// 題名の帯の高さ。ウィンドウの中身は、この下に置かれる。
pub const BAR: usize = 24;
/// 帯の左右の端から、題名までの空き。
pub const TITLE_PADDING: usize = 8;

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

/// 試験用のクライアント(`test-client`)と、その標準出力。
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

/// 偽の画面の写し。
pub struct Shot {
    /// PPMの画素の部分(赤, 緑, 青の順)。
    pixels: Vec<u8>,
}

impl Shot {
    /// 偽の画面の写し(PPM)を読む。まだ無いか、大きさが合わなければNone。
    pub fn read(path: &Path) -> Option<Self> {
        let image = fs::read(path).ok()?;
        let header = format!("P6 {WIDTH} {HEIGHT} 255\n");
        let pixels = image.strip_prefix(header.as_bytes())?;
        (pixels.len() == WIDTH * HEIGHT * 3).then(|| Shot {
            pixels: pixels.to_vec(),
        })
    }

    /// (x, y)の色。
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * WIDTH + x) * 3;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }
}

/// zeyes-minのウィンドウの大きさ。
pub const ZEYES_WIDTH: usize = 320;
pub const ZEYES_HEIGHT: usize = 240;

/// 外形の左上が(`x0`, `y0`)にあるzeyes-minのウィンドウの中身が、全体が見える形で描かれているか。
/// 中身は、題名の帯の下にある。
///
/// 目のまわりの色は `skin`。ポインターが無いとき、目玉は窓の中央を見る。左の目玉は中身の中の
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

/// 外形の左上が(`x0`, `y0`)、幅が `width` のウィンドウの、題名の帯の色。
///
/// 帯の右の端(題名の文字が来ない所)の、上と下の画素を見る。色がそろっていなければNone。
pub fn bar_color(shot: &Shot, x0: usize, y0: usize, width: usize) -> Option<[u8; 3]> {
    let top = shot.pixel(x0 + width - 2, y0);
    (top == shot.pixel(x0 + width - 2, y0 + BAR - 1)).then_some(top)
}

/// 外形の左上が(`x0`, `y0`)、幅が `width` のウィンドウの帯の中で、帯の色でない画素(題名の文字)の位置。
/// 位置は、帯の左上を原点にした(x, y)。
pub fn title_ink(shot: &Shot, x0: usize, y0: usize, width: usize) -> Vec<(usize, usize)> {
    let Some(bar) = bar_color(shot, x0, y0, width) else {
        return Vec::new();
    };
    (0..BAR)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
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

/// 偽の画面で動かしたseinas-standaloneと、その作業用の場所。
pub struct Compositor {
    pub process: Running,
    pub socket: PathBuf,
    pub dump: PathBuf,
    dir: PathBuf,
}

impl Compositor {
    /// 偽の画面で起動し、最初の1枚(背景)が出て、ソケットで待ち受けるまで待つ。
    ///
    /// `name` は、作業用の場所を試験ごとに分けるための短い名前。
    pub fn start(name: &str) -> Self {
        Self::start_with_fonts(name, &font_dir())
    }

    /// [`Compositor::start`] と同じだが、フォントの置き場を `fonts` にする。
    pub fn start_with_fonts(name: &str, fonts: &Path) -> Self {
        // ソケットのパスには長さの上限(約100文字)があるので、短い場所に置く。
        let dir = std::env::temp_dir().join(format!("seinas-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create the working directory");
        let socket = dir.join("wl");
        let dump = dir.join("screen.ppm");
        let log = fs::File::create(dir.join("log")).expect("create the log file");
        let process = Running(
            Command::new(env!("CARGO_BIN_EXE_seinas-standalone"))
                .args(["--fake", &format!("{WIDTH}x{HEIGHT}")])
                .arg("--dump")
                .arg(&dump)
                .arg("--socket")
                .arg(&socket)
                .arg("--fonts")
                .arg(fonts)
                // 外から入った指定に左右されないようにする。
                .env_remove("SEINAS_FONTS")
                .stderr(log)
                .spawn()
                .expect("start seinas-standalone"),
        );
        let compositor = Compositor {
            process,
            socket,
            dump,
            dir,
        };
        compositor.wait_for_screen("the first frame (background only)", shows_only_background);
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
            Command::new(workspace_bin("zeyes-min", "zeyes-min"))
                .args(args)
                .env("WAYLAND_DISPLAY", &self.socket)
                // 外から入った色の指定に左右されないようにする。
                .env_remove("ZEYES_SKIN")
                .stderr(Stdio::null())
                .spawn()
                .expect("start zeyes-min"),
        )
    }

    /// 試験用のクライアント(`test-client`)を、引数 `args` でつなぐ。標準入力と標準出力は、こちらで持つ。
    pub fn connect_test_client(&self, args: &[&str]) -> TestClient {
        let mut process = Running(
            Command::new(workspace_bin("seinas-test-clients", "test-client"))
                .args(args)
                .env("WAYLAND_DISPLAY", &self.socket)
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

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.process.0.kill();
        let _ = self.process.0.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
