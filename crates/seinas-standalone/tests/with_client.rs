//! seinas-standaloneを偽の画面で動かし、zeyes-minをつないで、そのウィンドウが画面に描かれることを
//! 画素で確かめる。
//!
//! あわせて、zeyes-minを強制終了してからもう一度つなぎ、2回目も描かれることを確かめる。
//! クライアントが去ると、Smithayが共有メモリーの後始末のスレッドを作るので、その経路も通る。

mod common;

use common::{shows_only_background, shows_zeyes, wait_for, Compositor, BACKGROUND};

#[test]
fn a_client_window_is_drawn_on_the_fake_screen_again_after_reconnecting() {
    // 起動すると、クライアントがいなくても背景が1枚出て、ソケットで待ち受ける。
    let mut seinas = Compositor::start("sa");
    assert_eq!(
        seinas.thread_count(),
        1,
        "no thread before any client leaves"
    );

    // 1回目: つなぐと、ウィンドウが画面に描かれる(ポインターは無い)。
    let mut first = seinas.connect_zeyes();
    seinas.wait_for_screen("the first client's window", shows_zeyes);
    assert!(first.is_running(), "the first client must keep running");
    // ウィンドウの外は、seinasの背景のまま。
    let shot = seinas.shot();
    assert_eq!(shot.pixel(330, 120), BACKGROUND);
    assert_eq!(shot.pixel(160, 250), BACKGROUND);

    // 強制終了すると、ウィンドウが消えて背景に戻る。後始末のスレッドが1本できる。
    drop(first);
    seinas.wait_for_screen(
        "the background after the first client was killed",
        shows_only_background,
    );
    wait_for("the cleanup thread", || {
        (seinas.thread_count() == 2).then_some(())
    });

    // 2回目: もう一度つないでも、同じように描かれる。
    let mut second = seinas.connect_zeyes();
    seinas.wait_for_screen("the second client's window", shows_zeyes);
    assert!(second.is_running(), "the second client must keep running");
    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
    assert_eq!(
        seinas.thread_count(),
        2,
        "the cleanup thread is created only once"
    );
}
