//! コンポジタが、最初の 1 枚を画面へ present したときに、INFO のログを 1 回だけ出すことを確かめる。
//!
//! 起動を見る側(ZeikOS の確かめなど)は、`presented the first frame (WxH)` の行で、最初の 1 枚が出たことを知る。
//! その後に何枚描いても、行は増えない。

mod common;

use std::time::Duration;

use common::Compositor;

const LINE: &str = "presented the first frame (";

fn count(log: &str) -> usize {
    log.matches(LINE).count()
}

#[test]
fn the_headless_compositor_logs_the_first_frame_once() {
    let seinas = Compositor::start_sized("fa", 320, 200);
    common::wait_for("the first frame", || {
        (count(&seinas.log()) > 0).then_some(())
    });
    assert!(
        seinas.log().contains("presented the first frame (320x200)"),
        "{}",
        seinas.log()
    );
    // クライアントが描くと、もっと描くが、行は増えない。
    let window = seinas.connect_test_client(&["window", "--title", "first"]);
    window.wait_for_line("window: drawn");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count(&seinas.log()), 1, "{}", seinas.log());
}

#[test]
fn the_nested_compositor_logs_the_first_frame_once() {
    let seinas = Compositor::start("fb");
    let nested = seinas.start_nested(&[]);
    common::wait_for("the first frame of the nested compositor", || {
        (count(&nested.log()) > 0).then_some(())
    });
    let window = seinas.connect_test_client_at(&nested.socket, &["window", "--title", "first"]);
    window.wait_for_line("window: drawn");
    std::thread::sleep(Duration::from_millis(300));
    let log = nested.log();
    assert_eq!(count(&log), 1, "{log}");
    let line = log.lines().find(|line| line.contains(LINE)).unwrap();
    assert!(line.contains(" INFO seinas: "), "{line}");
}
