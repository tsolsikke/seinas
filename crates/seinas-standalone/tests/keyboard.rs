//! キーボードの入力とフォーカスを、テスト用のクライアントの出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「キーボード」):
//! - キーボードのフォーカスは、activated のウィンドウ(最小化していないうちの、いちばん手前)に付く。
//!   raise・activate・最小化・閉じたときに移る。
//! - 押した・離したキーと、修飾キーの状態が、フォーカスのあるクライアントへ届く。最小化したウィンドウには届かない。
//! - keymap は、日本語の配列(jp106)。
//!
//! キーとポインターのイベントは、テスト用の入力インターフェース(`--test-input`)から送る。

mod common;

use common::{Compositor, TestClient};

/// キーボードのイベントの、次の行。ほかの行(configure、keymap など)は読み飛ばす。
fn next_keyboard_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        let keyboard = ["window: key ", "window: keyboard ", "window: modifiers "]
            .iter()
            .any(|prefix| line.starts_with(prefix));
        if keyboard {
            return line;
        }
    }
}

/// フォーカスが来たこと(enter と、そのときの修飾キー)。
fn expect_enter(client: &TestClient, what: &str) {
    assert_eq!(next_keyboard_line(client, what), "window: keyboard enter");
    assert_eq!(
        next_keyboard_line(client, what),
        "window: modifiers 0 0 0 0"
    );
}

/// キーボードを使うウィンドウを出し、描かれるまで待つ。
fn open_window(seinas: &Compositor, title: &str) -> TestClient {
    let window = seinas.connect_test_client(&["window", "--keyboard", "--title", title]);
    window.wait_for_line("window: drawn");
    window
}

/// キー `code` を押して、離す。
fn tap(seinas: &mut Compositor, code: u32) {
    seinas.pointer(&format!("key {code} press"));
    seinas.pointer(&format!("key {code} release"));
}

#[test]
fn the_focus_follows_the_activated_window() {
    let mut seinas = Compositor::start_with_input("ka");
    let first = open_window(&seinas, "一つ目");
    expect_enter(&first, "the focus on the first window");
    // 新しいウィンドウが出ると、フォーカスはそちらへ移る。
    let second = open_window(&seinas, "二つ目");
    assert_eq!(
        next_keyboard_line(&first, "leave"),
        "window: keyboard leave"
    );
    expect_enter(&second, "the focus on the second window");

    // キーは、フォーカスのあるウィンドウだけに届く(30 は A)。
    tap(&mut seinas, 30);
    assert_eq!(
        next_keyboard_line(&second, "press"),
        "window: key 30 pressed"
    );
    assert_eq!(
        next_keyboard_line(&second, "release"),
        "window: key 30 released"
    );

    // 修飾キー(42 は左の Shift)。押している間は、depressed に Shift(1)が立つ。
    seinas.pointer("key 42 press");
    assert_eq!(
        next_keyboard_line(&second, "shift"),
        "window: key 42 pressed"
    );
    assert_eq!(
        next_keyboard_line(&second, "modifiers"),
        "window: modifiers 1 0 0 0"
    );
    seinas.pointer("key 42 release");
    assert_eq!(
        next_keyboard_line(&second, "shift released"),
        "window: key 42 released"
    );
    assert_eq!(
        next_keyboard_line(&second, "modifiers"),
        "window: modifiers 0 0 0 0"
    );

    // 奥のウィンドウ(左上)のタイトルバーを押して raise すると、フォーカスも移る。
    seinas.pointer("motion 20 12");
    seinas.pointer("press");
    seinas.pointer("release");
    assert_eq!(
        next_keyboard_line(&second, "leave"),
        "window: keyboard leave"
    );
    expect_enter(&first, "the focus back on the first window");
    tap(&mut seinas, 31);
    assert_eq!(
        next_keyboard_line(&first, "press"),
        "window: key 31 pressed"
    );

    // 閉じると、その下のウィンドウへ移る。
    drop(first);
    expect_enter(&second, "the focus after closing");
    tap(&mut seinas, 32);
    assert_eq!(
        next_keyboard_line(&second, "press"),
        "window: key 32 pressed"
    );
}

#[test]
fn a_minimized_window_gets_no_keys() {
    let mut seinas = Compositor::start_with_input("kb");
    let first = open_window(&seinas, "一つ目");
    expect_enter(&first, "the focus on the first window");
    let second = open_window(&seinas, "二つ目");
    expect_enter(&second, "the focus on the second window");
    // 最小化を戻す手段(シェルの部品)をつなぐ。
    let mut list = seinas.connect_shell_client(&["toplevels"]);
    list.wait_for_line("toplevels: ready");

    // 2 つ目を最小化すると、フォーカスは 1 つ目へ移り、キーは 1 つ目へ届く。
    list.send_line("minimize 2");
    assert_eq!(
        next_keyboard_line(&second, "leave"),
        "window: keyboard leave"
    );
    assert_eq!(
        next_keyboard_line(&first, "leave while the second was in front"),
        "window: keyboard leave"
    );
    expect_enter(&first, "the focus on the first window again");
    tap(&mut seinas, 30);
    assert_eq!(
        next_keyboard_line(&first, "press"),
        "window: key 30 pressed"
    );
    assert_eq!(
        next_keyboard_line(&first, "release"),
        "window: key 30 released"
    );

    // 戻すと、フォーカスも戻る。最小化している間のキーは、2 つ目に届いていない(enter が次の行)。
    list.send_line("activate 2");
    expect_enter(&second, "the focus on the restored window");
    tap(&mut seinas, 31);
    assert_eq!(
        next_keyboard_line(&second, "press"),
        "window: key 31 pressed"
    );
    assert_eq!(
        next_keyboard_line(&second, "release"),
        "window: key 31 released"
    );
    assert_eq!(
        next_keyboard_line(&first, "leave when the second came back"),
        "window: keyboard leave"
    );

    // foreign-toplevel の activate でも、フォーカスが移る。
    list.send_line("activate 1");
    expect_enter(&first, "the focus by activate");
    assert_eq!(
        next_keyboard_line(&second, "leave by activate"),
        "window: keyboard leave"
    );
}

#[test]
fn keys_without_a_window_go_nowhere() {
    let mut seinas = Compositor::start_with_input("kc");
    tap(&mut seinas, 30);
    seinas.pointer("key 999 press");
    seinas.pointer("key 30");
    // 動き続けていて、後から出たウィンドウにフォーカスが付く。
    let window = open_window(&seinas, "一つ目");
    expect_enter(&window, "the focus on the first window");
    assert!(seinas.process.is_running());
    assert!(seinas
        .log()
        .contains("keyboard: layout \"jp\", model \"jp106\""));
}
