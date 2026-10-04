//! 共有メモリーのプールを縮めてくるクライアントがいても、seinas-standaloneは落ちず、そのクライアント
//! だけが切られることを確かめる。
//!
//! 縮められたプールをコンポジタが読むと、Linuxでは SIGBUS になる。Smithayがそれを受け止めて、
//! 読み出しを失敗として返し、そのクライアントへプロトコルのエラーを送って切る。
//! 動かし方と期待する結果は、`docs/shrink-test.md` にまとめてある。

mod common;

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
};

use common::{
    shows_zeyes, wait_for, workspace_bin, Compositor, Running, BACKGROUND, SKIN, TIMEOUT,
};

/// 縮めるクライアントの、ウィンドウの色(紫)。大きさは400x300。
const SHRINK: [u8; 3] = [0xff, 0x00, 0xff];
/// 縮めるクライアントのウィンドウの中の点。後からつなぐので、(32, 32)にずれて、手前に置かれる。
const VISIBLE: [(usize, usize); 3] = [(34, 34), (200, 150), (430, 330)];

#[test]
fn a_client_that_shrinks_its_pool_is_cut_and_the_others_keep_running() {
    let mut seinas = Compositor::start("sh");

    // ふつうのクライアントを1つ、先につないでおく。
    let mut zeyes = seinas.connect_zeyes();
    seinas.wait_for_screen("the window of zeyes-min", shows_zeyes);

    // 縮めるクライアント。描かれたら「shrink: drawn」と書き、こちらが1行送るまで、縮めずに待つ。
    let mut shrink = Running(
        Command::new(workspace_bin("seinas-test-clients", "bad-client"))
            .args(["shrink", "--step"])
            .env("WAYLAND_DISPLAY", &seinas.socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the shrinking client"),
    );
    // 標準出力は、別のスレッドで1行ずつ読む(待つ時間に上限を置くため)。
    let (lines, output) = mpsc::channel();
    let stdout = shrink.0.stdout.take().expect("the client's output");
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let next_line = |what: &str| {
        output
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("timed out while waiting for {what}"))
    };

    // 1) 縮める前: frameコールバックが届き、画面にも描かれている。
    assert_eq!(next_line("the first frame"), "shrink: drawn");
    // zeyes-minは奥になり、左上の端だけが見えている。
    seinas.wait_for_screen("the window of the shrinking client", |shot| {
        VISIBLE.iter().all(|&(x, y)| shot.pixel(x, y) == SHRINK) && shot.pixel(2, 2) == SKIN
    });

    // 2) 縮めさせる。
    shrink
        .0
        .stdin
        .take()
        .expect("the client's input")
        .write_all(b"\n")
        .expect("tell the client to go on");
    assert_eq!(next_line("the shrink"), "shrink: shrunk");

    // 3) 縮めたクライアントだけが切られる。プロトコルのエラーが届いている。
    assert_eq!(
        next_line("the protocol error"),
        "shrink: protocol error: object 11, code 2, \"Bad pool size.\""
    );
    assert_eq!(
        next_line("the disconnection"),
        "shrink: the compositor closed the connection"
    );
    let status = wait_for("the shrinking client to exit", || {
        shrink.0.try_wait().expect("check the client")
    });
    assert!(
        status.success(),
        "the shrinking client must report that it was cut"
    );

    // 4) そのウィンドウは消え、zeyes-minは描かれ続ける。seinas-standaloneもzeyes-minも動いている。
    seinas.wait_for_screen("the screen without the shrinking client", |shot| {
        shot.pixel(430, 330) == BACKGROUND && shows_zeyes(shot)
    });
    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
    assert!(zeyes.is_running(), "zeyes-min must keep running");

    // 5) その後も、新しいクライアントを受け付ける。
    drop(zeyes);
    seinas.wait_for_screen("the background after zeyes-min left", |shot| {
        shot.pixel(2, 2) == BACKGROUND
    });
    let _again = seinas.connect_zeyes();
    seinas.wait_for_screen("the window of a new zeyes-min", shows_zeyes);
    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
}
