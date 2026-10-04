//! 共通の描画だけで動かしたときに、余計なスレッドやシグナルの受け口が増えないことを確かめる。
//!
//! Smithayの共有メモリー(wl_shm)の実装は、プールを捨てるときにスレッドを1つ作り、バッファを読む
//! ときにSIGBUSの受け口を置き換える。共通の描画はその実装を通らないので、どちらも起きないはずである。
//! スレッドの数を数えるため、このテストはテストハーネスを使わずに、1本のスレッドで動く。

use seinas_render::Painter;
use smithay::backend::renderer::element::{solid::SolidColorRenderElement, Id, Kind};
use smithay::utils::Rectangle;

fn thread_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .expect("read /proc/self/task")
        .map(|entry| {
            let path = entry.expect("task entry").path().join("comm");
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .collect();
    names.sort();
    names
}

/// いまのSIGBUSの受け口(関数の番地と旗)。
fn sigbus_action() -> (usize, i32) {
    // SAFETY: 新しい設定を渡さず(null)、いまの設定を読むだけである。
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        assert_eq!(
            libc::sigaction(libc::SIGBUS, std::ptr::null(), &mut current),
            0
        );
        (current.sa_sigaction, current.sa_flags)
    }
}

fn main() {
    let threads_before = thread_names();
    let sigbus_before = sigbus_action();
    assert_eq!(
        threads_before.len(),
        1,
        "threads at start: {threads_before:?}"
    );

    let mut painter = Painter::new(800, 600).expect("painter");
    for i in 0..3 {
        let element = SolidColorRenderElement::new(
            Id::new(),
            Rectangle::new((10 * i, 10 * i).into(), (100, 100).into()),
            0usize,
            [1.0, 1.0, 1.0, 1.0],
            Kind::Unspecified,
        );
        let view = painter.paint(&[element]).expect("paint");
        assert_eq!((view.width(), view.height()), (800, 600));
    }
    drop(painter);

    let threads_after = thread_names();
    assert_eq!(
        threads_after, threads_before,
        "painting must not start a thread"
    );
    assert_eq!(
        sigbus_action(),
        sigbus_before,
        "painting must not replace the SIGBUS handler"
    );
    println!("render_only: ok (threads: {threads_after:?}, SIGBUS handler unchanged)");
}
