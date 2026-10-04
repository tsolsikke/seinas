//! seinas-standaloneを偽の画面で動かし、zeyes-minをつないで、そのウィンドウが画面に描かれることを
//! 画素で確かめる。親のWaylandも、画面の装置も要らない。
//!
//! あわせて、zeyes-minを強制終了してからもう一度つなぎ、2回目も描かれることを確かめる。
//! クライアントが去ると、Smithayが共有メモリーの後始末のスレッドを作るので、その経路も通る。

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// 待つ時間の上限。遅い環境でも落ちないように長めに取り、条件がそろい次第すぐ先へ進む。
const TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(20);

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

// 色は (赤, 緑, 青)。
/// seinasの背景。
const BACKGROUND: [u8; 3] = [0x19, 0x1e, 0x28];
/// zeyes-minの、目のまわり(肌)。
const SKIN: [u8; 3] = [0x6a, 0xb0, 0x5c];
/// zeyes-minの白目、目の縁、瞳。
const SCLERA: [u8; 3] = [0xff, 0xff, 0xff];
const OUTLINE: [u8; 3] = [0x10, 0x10, 0x10];
const PUPIL: [u8; 3] = [0x14, 0x0e, 0x0a];

/// 終わるときに、動かしたプロセスを必ず止める。
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 条件がそろうまで待つ。上限を過ぎたら、何を待っていたかを添えて失敗する。
fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
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
struct Shot {
    /// PPMの画素の部分(赤, 緑, 青の順)。
    pixels: Vec<u8>,
}

impl Shot {
    /// 偽の画面の写し(PPM)を読む。まだ無いか、大きさが合わなければNone。
    fn read(path: &Path) -> Option<Self> {
        let image = fs::read(path).ok()?;
        let header = format!("P6 {WIDTH} {HEIGHT} 255\n");
        let pixels = image.strip_prefix(header.as_bytes())?;
        (pixels.len() == WIDTH * HEIGHT * 3).then(|| Shot {
            pixels: pixels.to_vec(),
        })
    }

    /// (x, y)の色。
    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let at = (y * WIDTH + x) * 3;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }
}

/// zeyes-minのウィンドウ(320x240、左上)が、決まった印のとおりに描かれているか。
///
/// ポインターが無いとき、目玉は窓の中央を見る。左の目玉は(114, 120)、右の目玉は(206, 120)に来る。
fn shows_zeyes(shot: &Shot) -> bool {
    shot.pixel(2, 2) == SKIN
        && shot.pixel(317, 237) == SKIN
        && shot.pixel(85, 60) == SCLERA
        && shot.pixel(235, 60) == SCLERA
        && shot.pixel(85, 19) == OUTLINE
        && shot.pixel(114, 120) == PUPIL
        && shot.pixel(206, 120) == PUPIL
        // ウィンドウの外は、seinasの背景のまま。
        && shot.pixel(330, 120) == BACKGROUND
        && shot.pixel(160, 250) == BACKGROUND
        && shot.pixel(WIDTH - 1, HEIGHT - 1) == BACKGROUND
}

fn shows_only_background(shot: &Shot) -> bool {
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

/// zeyes-minの実行ファイルの場所。このテストと同じ置き場(target/…/debug など)にある。
///
/// Cargoは、ほかのパッケージの実行ファイルをテストのためには作らない。無ければ、ここで作る。
fn zeyes_min() -> PathBuf {
    let test_exe = std::env::current_exe().expect("the path of this test");
    // <置き場>/deps/<このテスト>
    let profile_dir = test_exe
        .parent()
        .and_then(Path::parent)
        .expect("the profile directory");
    let zeyes = profile_dir.join("zeyes-min");
    if !zeyes.exists() {
        let mut build = Command::new(env!("CARGO"));
        build.args(["build", "--locked", "-p", "zeyes-min"]);
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
        let status = build.status().expect("run cargo to build zeyes-min");
        assert!(status.success(), "building zeyes-min failed");
    }
    assert!(zeyes.exists(), "{} was not built", zeyes.display());
    zeyes
}

fn thread_count(process: &Child) -> usize {
    fs::read_dir(format!("/proc/{}/task", process.id()))
        .map(|tasks| tasks.count())
        .unwrap_or(0)
}

#[test]
fn a_client_window_is_drawn_on_the_fake_screen_again_after_reconnecting() {
    let zeyes = zeyes_min();
    // ソケットのパスには長さの上限(約100文字)があるので、短い場所に置く。
    let dir = std::env::temp_dir().join(format!("seinas-sa-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the working directory");
    let socket = dir.join("wl");
    let dump = dir.join("screen.ppm");

    let mut seinas = Running(
        Command::new(env!("CARGO_BIN_EXE_seinas-standalone"))
            .args(["--fake", &format!("{WIDTH}x{HEIGHT}")])
            .arg("--dump")
            .arg(&dump)
            .arg("--socket")
            .arg(&socket)
            .stderr(Stdio::null())
            .spawn()
            .expect("start seinas-standalone"),
    );
    let connect = |name: &str| {
        Running(
            Command::new(&zeyes)
                .env("WAYLAND_DISPLAY", &socket)
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|e| panic!("start the {name} zeyes-min: {e}")),
        )
    };
    let screen_is = |what: &str, check: fn(&Shot) -> bool| {
        wait_for(what, || Shot::read(&dump).filter(check).map(|_| ()));
    };

    // 起動すると、クライアントがいなくても背景が1枚出て、ソケットで待ち受ける。
    screen_is("the first frame (background only)", shows_only_background);
    wait_for("the listening socket", || socket.exists().then_some(()));
    assert_eq!(
        thread_count(&seinas.0),
        1,
        "no thread before any client leaves"
    );

    // 1回目: つなぐと、ウィンドウが画面に描かれる(ポインターは無い)。
    let mut first = connect("first");
    screen_is("the first client's window", shows_zeyes);
    assert!(
        first.0.try_wait().unwrap().is_none(),
        "the first client must keep running"
    );

    // 強制終了すると、ウィンドウが消えて背景に戻る。後始末のスレッドが1本できる。
    drop(first);
    screen_is(
        "the background after the first client was killed",
        shows_only_background,
    );
    wait_for("the cleanup thread", || {
        (thread_count(&seinas.0) == 2).then_some(())
    });

    // 2回目: もう一度つないでも、同じように描かれる。
    let mut second = connect("second");
    screen_is("the second client's window", shows_zeyes);
    assert!(
        second.0.try_wait().unwrap().is_none(),
        "the second client must keep running"
    );
    assert!(
        seinas.0.try_wait().unwrap().is_none(),
        "seinas-standalone must keep running"
    );
    assert_eq!(
        thread_count(&seinas.0),
        2,
        "the cleanup thread is created only once"
    );

    drop(second);
    drop(seinas);
    let _ = fs::remove_dir_all(&dir);
}
