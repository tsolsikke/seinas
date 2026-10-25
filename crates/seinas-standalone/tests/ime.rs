//! IME の道(`docs/adr/0004-ime-keyboard-path.md`)を、テスト用の IME(`test-client ime`)とテスト用のクライアントの
//! 出力で確かめる。
//!
//! 決まり:
//! - input-method と virtual-keyboard の global は、シェルの部品のためのソケットからつないだクライアントにだけ見せる。
//! - Seinas のショートカット(Alt+Tab)は、いつも IME へ渡す前に見る。
//! - IME が keyboard を grab している間は、ショートカットのほかのキーを IME へ渡す。IME が virtual-keyboard で戻した
//!   キーは、フォーカスのあるクライアントへ届く。
//! - IME が去るか grab を手放したら、次のキーから直接の道(フォーカスのあるクライアントの wl_keyboard)にする。
//! - IME は同時に 1 つだけ。2 つ目には unavailable を送り、1 つ目はそのまま使える。
//!
//! キーは、テスト用の入力インターフェース(`--test-input`)の `key` の行で送る。

mod common;

use common::{Compositor, Shot, TestClient};

/// evdev のキーの番号。
const KEY_X: u32 = 45;
const KEY_Y: u32 = 21;
const KEY_TAB: u32 = 15;
const KEY_LEFT_ALT: u32 = 56;

/// ウィンドウのキーボードの、次の行(キー、enter、leave)。修飾キーの状態と keymap の行は読み飛ばす。
fn next_key_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        if line.starts_with("window: key ") || line.starts_with("window: keyboard ") {
            return line;
        }
    }
}

/// IME の、次のキーの行。
fn next_ime_key(ime: &TestClient, what: &str) -> String {
    loop {
        let line = ime.next_line(what);
        if line.starts_with("ime: key ") {
            return line;
        }
    }
}

/// キーボードを使うウィンドウを出し、フォーカスが来るまで待つ。
fn open_window(seinas: &Compositor, title: &str) -> TestClient {
    let window = seinas.connect_test_client(&["window", "--keyboard", "--title", title]);
    window.wait_for_line("window: drawn");
    assert_eq!(
        next_key_line(&window, "the focus"),
        "window: keyboard enter"
    );
    window
}

/// テスト用の IME を、IME のためのソケットへつなぎ、grab を取るまで待つ。
fn connect_ime(seinas: &Compositor, args: &[&str]) -> TestClient {
    let mut all = vec!["ime"];
    all.extend(args);
    let ime = seinas.connect_ime_client(&all);
    ime.wait_for_line("ime: ready");
    ime
}

/// キー `code` を押して、離す。
fn tap(seinas: &mut Compositor, code: u32) {
    seinas.pointer(&format!("key {code} press"));
    seinas.pointer(&format!("key {code} release"));
}

/// ログに `text` が出るまで待つ。
fn wait_for_log(seinas: &Compositor, text: &str) {
    common::wait_for(text, || seinas.log().contains(text).then_some(()));
}

#[test]
fn keys_go_through_the_ime_and_come_back_to_the_window() {
    let mut seinas = Compositor::start_with_input("ia");
    let window = open_window(&seinas, "ウィンドウ");
    let ime = connect_ime(&seinas, &[]);
    tap(&mut seinas, KEY_X);
    assert_eq!(next_ime_key(&ime, "x to the IME"), "ime: key 45 pressed");
    assert_eq!(next_ime_key(&ime, "x to the IME"), "ime: key 45 released");
    // IME が virtual-keyboard で戻したキーが、ウィンドウに届く。
    assert_eq!(
        next_key_line(&window, "x from the IME"),
        "window: key 45 pressed"
    );
    assert_eq!(
        next_key_line(&window, "x from the IME"),
        "window: key 45 released"
    );
    assert!(seinas.log().contains("an input method connected"));
}

/// `test-client globals` が書いた global の名前(`globals: done` まで)。
fn globals(client: &TestClient) -> Vec<String> {
    let mut found = Vec::new();
    loop {
        let line = client.next_line("the globals");
        if line == "globals: done" {
            return found;
        }
        if let Some(interface) = line.strip_prefix("globals: ") {
            found.push(interface.to_owned());
        }
    }
}

const IME_GLOBALS: [&str; 2] = [
    "zwp_input_method_manager_v2",
    "zwp_virtual_keyboard_manager_v1",
];

const SHELL_GLOBALS: [&str; 3] = [
    "zwlr_layer_shell_v1",
    "zwlr_foreign_toplevel_manager_v1",
    "ext_foreign_toplevel_list_v1",
];

#[test]
fn the_input_method_globals_are_only_on_the_ime_socket() {
    let seinas = Compositor::start_with_input("ib");
    // IME のソケットには、input-method と virtual-keyboard があり、シェルの部品のもの(layer-shell と
    // foreign-toplevel)は無い。候補ウィンドウに要る wl_compositor などは、ある。
    let on_ime = globals(&seinas.connect_ime_client(&["globals"]));
    for global in IME_GLOBALS
        .iter()
        .chain(&["wl_compositor", "wl_shm", "wl_seat"])
    {
        assert!(on_ime.iter().any(|g| g == global), "{global}: {on_ime:?}");
    }
    for global in SHELL_GLOBALS {
        assert!(!on_ime.iter().any(|g| g == global), "{global}: {on_ime:?}");
    }
    // シェルの部品のソケットとふだんのソケットには、input-method と virtual-keyboard が無い。
    let on_shell = globals(&seinas.connect_shell_client(&["globals"]));
    assert!(
        on_shell.iter().any(|g| g == "zwlr_layer_shell_v1"),
        "{on_shell:?}"
    );
    let on_app = globals(&seinas.connect_test_client(&["globals"]));
    for global in IME_GLOBALS {
        assert!(
            !on_shell.iter().any(|g| g == global),
            "{global}: {on_shell:?}"
        );
        assert!(!on_app.iter().any(|g| g == global), "{global}: {on_app:?}");
    }
    // タスクバー役(シェルの部品のソケット)の IME は、準備の段階で失敗する(exit status 2)。
    let mut taskbar_as_ime = seinas.connect_shell_client(&["ime"]);
    let status = taskbar_as_ime
        .process
        .0
        .wait()
        .expect("the test client ends");
    assert_eq!(status.code(), Some(2));
}

#[test]
fn a_virtual_keyboard_needs_the_accepted_input_method() {
    let mut seinas = Compositor::start_with_input("it");
    // get_input_method を受け入れていないクライアントは、IME のソケットからでも virtual-keyboard を作れない
    // (unauthorized のエラーで切られる)。
    let refused = seinas.connect_ime_client(&["ime", "--virtual-keyboard-only"]);
    let line = refused.next_line("the refusal");
    assert!(line.starts_with("ime: protocol error"), "{line}");
    assert!(line.contains("only the accepted input method"), "{line}");
    wait_for_log(&seinas, "a virtual keyboard was refused");
    assert!(seinas.process.is_running());

    // 受け入れた IME の virtual-keyboard は使える。input method を destroy すると認可が消え、同じ
    // virtual-keyboard から送ったキーは捨てる。
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line(&format!("vk-press {KEY_Y}"));
    ime.send_line(&format!("vk-release {KEY_Y}"));
    assert_eq!(
        next_key_line(&window, "y from the IME"),
        "window: key 21 pressed"
    );
    assert_eq!(
        next_key_line(&window, "y from the IME"),
        "window: key 21 released"
    );
    ime.send_line("destroy-input-method");
    ime.wait_for_line("ime: input method destroyed");
    ime.send_line(&format!("vk-press {KEY_X}"));
    ime.send_line(&format!("vk-release {KEY_X}"));
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
    let lines = window.lines_within(std::time::Duration::from_millis(300));
    assert!(
        !lines.iter().any(|line| line.starts_with("window: key 45")),
        "{lines:?}"
    );
    tap(&mut seinas, KEY_Y);
    assert!(seinas.process.is_running());
}

#[test]
fn a_second_input_method_on_the_same_connection_keeps_the_first() {
    let mut seinas = Compositor::start_with_input("iu");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("again");
    ime.wait_for_line("ime: second unavailable");
    // 1 つ目の grab と virtual-keyboard は、そのまま使える。
    tap(&mut seinas, KEY_X);
    assert_eq!(
        next_ime_key(&ime, "x on the first IME"),
        "ime: key 45 pressed"
    );
    assert_eq!(
        next_key_line(&window, "x from the IME"),
        "window: key 45 pressed"
    );
    ime.send_line(&format!("vk-press {KEY_Y}"));
    ime.send_line(&format!("vk-release {KEY_Y}"));
    loop {
        let line = next_key_line(&window, "y from the IME");
        if line == "window: key 21 pressed" {
            break;
        }
    }
}

#[test]
fn the_input_method_globals_need_a_keyboard() {
    // Smithay 0.7.0 は、キーボードの無い seat への get_input_method と、virtual-keyboard の key・modifiers で
    // panic する。xkb のファイルが読めず、seat にキーボードを足せなかった構成(ポインターだけの seat)では、
    // input-method と virtual-keyboard の global を作らない。
    //
    // xkb の置き場は、xkb のファイルの無い、在るディレクトリにする(xkbcommon の context は作れるが、keymap を
    // 作れない形。context そのものを作れない形は、下の the_compositor_starts_without_an_xkb_include_path)。
    let no_xkb = env!("CARGO_MANIFEST_DIR");
    let mut seinas = Compositor::start_with_input_env(
        "iq",
        &[
            ("XKB_CONFIG_ROOT", no_xkb),
            ("XKB_CONFIG_EXTRA_PATH", no_xkb),
            ("HOME", no_xkb),
        ],
    );
    wait_for_log(&seinas, "no keyboard: the keymap could not be made");
    let found = globals(&seinas.connect_ime_client(&["globals"]));
    assert!(found.iter().any(|g| g == "wl_seat"), "{found:?}");
    for global in IME_GLOBALS {
        assert!(!found.iter().any(|g| g == global), "{global}: {found:?}");
    }
    assert!(seinas.process.is_running());

    // キーボードのある seat では、同じソケットに両方とも見せる。
    let seinas = Compositor::start_with_input("ir");
    let found = globals(&seinas.connect_ime_client(&["globals"]));
    for global in IME_GLOBALS {
        assert!(found.iter().any(|g| g == global), "{global}: {found:?}");
    }
}

#[test]
fn the_compositor_starts_without_an_xkb_include_path() {
    // xkb の置き場が無いと、xkbcommon の context そのものを作れない。Smithay 0.7.0 の add_keyboard はそのとき
    // 落ちるので、Seinas はその前に確かめて、キーボードの無い構成として動く。
    let missing = "/nonexistent-seinas-xkb";
    let mut seinas = Compositor::start_with_input_env(
        "is",
        &[
            ("XKB_CONFIG_ROOT", missing),
            ("XKB_CONFIG_EXTRA_PATH", missing),
            ("HOME", missing),
        ],
    );
    wait_for_log(
        &seinas,
        "no keyboard: the keymap could not be made: the xkb context could not be made",
    );
    let found = globals(&seinas.connect_ime_client(&["globals"]));
    assert!(found.iter().any(|g| g == "wl_seat"), "{found:?}");
    for global in IME_GLOBALS {
        assert!(!found.iter().any(|g| g == global), "{global}: {found:?}");
    }
    assert!(seinas.process.is_running());
}

#[test]
fn alt_tab_works_while_the_ime_has_the_keyboard() {
    let mut seinas = Compositor::start_with_input("ic");
    let first = open_window(&seinas, "一つ目");
    let second = open_window(&seinas, "二つ目");
    assert_eq!(next_key_line(&first, "leave"), "window: keyboard leave");
    let ime = connect_ime(&seinas, &[]);
    // Alt+Tab は Seinas が使う。Alt は IME へ渡り(IME がウィンドウへ戻す)、Tab は IME にも渡らない。人が打つ
    // ときと同じく、Alt が戻ってから Tab を押す。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    assert_eq!(next_ime_key(&ime, "Alt"), "ime: key 56 pressed");
    assert_eq!(next_key_line(&second, "Alt"), "window: key 56 pressed");
    tap(&mut seinas, KEY_TAB);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    assert_eq!(next_ime_key(&ime, "Alt released"), "ime: key 56 released");
    assert_eq!(
        next_key_line(&second, "the leave"),
        "window: keyboard leave"
    );
    assert_eq!(next_key_line(&first, "the enter"), "window: keyboard enter");
    // 切り替えた先へ、次のキーが IME を通って届く。
    tap(&mut seinas, KEY_X);
    assert_eq!(next_ime_key(&ime, "x"), "ime: key 45 pressed");
    // IME が戻した「Alt を離した」は、フォーカスが移った後に届くが、Alt を押したことを知らない一つ目には渡さない。
    assert_eq!(
        next_key_line(&first, "x on the first window"),
        "window: key 45 pressed"
    );
}

#[test]
fn alt_through_the_ime_during_alt_tab_reaches_only_the_window_that_got_the_press() {
    let mut seinas = Compositor::start_with_input("ip");
    let first = open_window(&seinas, "一つ目");
    let second = open_window(&seinas, "二つ目");
    assert_eq!(next_key_line(&first, "leave"), "window: keyboard leave");
    let ime = connect_ime(&seinas, &[]);
    // 二つ目が、IME から戻った Alt の押下を受け、Alt+Tab で leave を受ける。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    assert_eq!(next_ime_key(&ime, "Alt"), "ime: key 56 pressed");
    assert_eq!(next_key_line(&second, "Alt"), "window: key 56 pressed");
    tap(&mut seinas, KEY_TAB);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    assert_eq!(next_ime_key(&ime, "Alt released"), "ime: key 56 released");
    assert_eq!(next_key_line(&second, "leave"), "window: keyboard leave");
    assert_eq!(next_key_line(&first, "enter"), "window: keyboard enter");
    // 一つ目には、Alt を離したことが届かない(次に届くのは x)。
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&first, "x"), "window: key 45 pressed");
    assert_eq!(next_key_line(&first, "x"), "window: key 45 released");
    // 一つ目で押して離した Alt は、両方とも IME から届く(ふつうの道は変わらない)。
    tap(&mut seinas, KEY_LEFT_ALT);
    assert_eq!(next_key_line(&first, "Alt"), "window: key 56 pressed");
    assert_eq!(next_key_line(&first, "Alt"), "window: key 56 released");
    // 戻す:一つ目が Alt の押下を受けて leave、二つ目に enter。二つ目にも、Alt を離したことは届かない。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    assert_eq!(next_key_line(&first, "Alt"), "window: key 56 pressed");
    tap(&mut seinas, KEY_TAB);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    assert_eq!(next_key_line(&first, "leave"), "window: keyboard leave");
    assert_eq!(next_key_line(&second, "enter"), "window: keyboard enter");
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&second, "y"), "window: key 21 pressed");
}

#[test]
fn keys_go_straight_to_the_window_after_the_ime_leaves() {
    let mut seinas = Compositor::start_with_input("id");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("quit");
    wait_for_log(&seinas, "the input method left");
    // IME が去ると、ウィンドウへ leave と enter を送り直す(押されたキーは無い)。
    assert_eq!(next_reentered_keys(&window), Vec::<u32>::new());
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&window, "y"), "window: key 21 pressed");
    assert_eq!(next_key_line(&window, "y"), "window: key 21 released");
}

#[test]
fn keys_go_straight_to_the_window_after_the_ime_releases_the_keyboard() {
    let mut seinas = Compositor::start_with_input("ie");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    wait_for_log(&seinas, "the input method released the keyboard");
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&window, "y"), "window: key 21 pressed");
    assert_eq!(next_key_line(&window, "y"), "window: key 21 released");
    // IME には届かない(届いていれば、sync より先に書く)。
    ime.send_line("sync");
    assert_eq!(ime.next_line("the sync"), "ime: synced");
}

#[test]
fn a_second_ime_is_refused_and_the_first_keeps_working() {
    let mut seinas = Compositor::start_with_input("if");
    let window = open_window(&seinas, "ウィンドウ");
    let first = connect_ime(&seinas, &[]);
    // 2 つ目は、get_input_method と grab_keyboard を送った後に unavailable を受けて終わる。
    let mut second = seinas.connect_ime_client(&["ime"]);
    second.wait_for_line("ime: unavailable");
    let _ = second.process.0.wait();
    assert!(seinas
        .log()
        .contains("an input method was refused (another input method is running)"));
    // 2 つ目が grab を取って去っても、1 つ目の grab は外れない。
    tap(&mut seinas, KEY_X);
    assert_eq!(
        next_ime_key(&first, "x on the first IME"),
        "ime: key 45 pressed"
    );
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    assert!(seinas.process.is_running());
    // 1 つ目が去れば、次の IME を受け入れる。
    drop(first);
    wait_for_log(&seinas, "the input method left");
    let third = connect_ime(&seinas, &[]);
    tap(&mut seinas, KEY_Y);
    assert_eq!(
        next_ime_key(&third, "y on the third IME"),
        "ime: key 21 pressed"
    );
}

/// evdev のキーの番号(IME が変換中の文字にするもの)。
const KEY_A: u32 = 30;
const KEY_I: u32 = 23;
const KEY_ENTER: u32 = 28;

/// text-input の次の行(enter、leave、preedit、commit、delete、done)。
fn next_text_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        let text = ["text-input ", "preedit ", "commit ", "delete "]
            .iter()
            .any(|prefix| line.starts_with(&format!("window: {prefix}")));
        if text {
            return line;
        }
    }
}

/// text-input の、done のほかの次の行。
fn next_text_change(client: &TestClient, what: &str) -> String {
    loop {
        let line = next_text_line(client, what);
        if line != "window: text-input done" {
            return line;
        }
    }
}

/// IME の、次の activate か deactivate の行。
fn next_activation(ime: &TestClient, what: &str) -> String {
    loop {
        let line = ime.next_line(what);
        if line == "ime: activate" || line == "ime: deactivate" {
            return line;
        }
    }
}

/// IME が、activate を受けて done で効かせるまで待つ(deactivate が先に届くことがある)。
fn wait_for_activate(ime: &TestClient, what: &str) {
    while next_activation(ime, what) != "ime: activate" {}
    ime.wait_for_line("ime: done");
}

/// テスト用の IME をつなぎ、フォーカスのある text-input のウィンドウで activate になるまで待つ(ウィンドウが
/// 先にあると、activate は ready より先に届くことがある。activate が届いたときには、IME は virtual-keyboard の
/// keymap を送り終えている)。
fn connect_ime_to_window(seinas: &Compositor) -> TestClient {
    connect_ime_to_window_with(seinas, &[])
}

/// [`connect_ime_to_window`] と同じだが、IME に引数 `args` を付ける。
fn connect_ime_to_window_with(seinas: &Compositor, args: &[&str]) -> TestClient {
    let mut all = vec!["ime"];
    all.extend(args);
    let ime = seinas.connect_ime_client(&all);
    wait_for_activate(&ime, "activate");
    ime
}

/// text-input を使うウィンドウを出し、描かれるまで待つ。
fn open_text_window(seinas: &Compositor, title: &str) -> TestClient {
    let window = seinas.connect_test_client(&["window", "--text-input", "--title", title]);
    window.wait_for_line("window: drawn");
    window
}

#[test]
fn preedit_and_commit_reach_the_window() {
    let mut seinas = Compositor::start_with_input("ig");
    let window = open_text_window(&seinas, "ウィンドウ");
    // IME がつながると、フォーカスのあるウィンドウの text-input に enter が届き、ウィンドウが有効にすると、
    // IME に activate が届く。
    let _ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    tap(&mut seinas, KEY_I);
    assert_eq!(next_text_line(&window, "あい"), "window: preedit あい");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    tap(&mut seinas, KEY_ENTER);
    assert_eq!(next_text_line(&window, "commit"), "window: commit あい");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    // IME が使ったキーは、ウィンドウの wl_keyboard には届かない。使わなかったキーは、IME から戻る。
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    // ふだんのログ(--log-keys を付けない)には、キー・keysym・変換中の文字・確定した文字が出ない
    // (ADR 0004 の 1.6)。
    let log = seinas.log();
    for text in [
        "あ",
        "い",
        "key:",
        "Return",
        "keysym",
        "preedit",
        "commit_string",
    ] {
        assert!(!log.contains(text), "{text:?} in the log:\n{log}");
    }
}

#[test]
fn a_window_opened_after_the_ime_gets_the_text_input() {
    let mut seinas = Compositor::start_with_input("ih");
    let ime = connect_ime(&seinas, &[]);
    let window = open_text_window(&seinas, "ウィンドウ");
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    wait_for_activate(&ime, "activate");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
}

#[test]
fn focus_changes_move_the_text_input_and_the_activation() {
    let mut seinas = Compositor::start_with_input("ii");
    // 最小化は、戻す手段(ウィンドウの一覧を受け取るシェルの部品)があるときだけできる。
    let _list = seinas.connect_shell_client(&["toplevels"]);
    let mut first = open_text_window(&seinas, "一つ目");
    let second = open_text_window(&seinas, "二つ目");
    let ime = connect_ime_to_window(&seinas);
    assert_eq!(
        next_text_change(&second, "enter"),
        "window: text-input enter"
    );

    // Alt+Tab:二つ目に leave、IME に deactivate、一つ目に enter、IME に activate。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    ime.wait_for_line("ime: key 56 pressed");
    tap(&mut seinas, KEY_TAB);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    assert_eq!(
        next_text_change(&second, "leave"),
        "window: text-input leave"
    );
    assert_eq!(next_activation(&ime, "deactivate"), "ime: deactivate");
    assert_eq!(
        next_text_change(&first, "enter"),
        "window: text-input enter"
    );
    wait_for_activate(&ime, "activate on the first");
    tap(&mut seinas, KEY_A);
    assert_eq!(
        next_text_change(&first, "あ on the first"),
        "window: preedit あ"
    );

    // 最小化:一つ目に leave、二つ目に enter。変換中の文字は、新しく打ち直す。
    first.send_line("minimize");
    assert_eq!(
        next_text_change(&first, "leave"),
        "window: text-input leave"
    );
    assert_eq!(next_activation(&ime, "deactivate"), "ime: deactivate");
    assert_eq!(
        next_text_change(&second, "enter"),
        "window: text-input enter"
    );
    wait_for_activate(&ime, "activate on the second again");
    tap(&mut seinas, KEY_I);
    assert_eq!(
        next_text_change(&second, "い on the second"),
        "window: preedit い"
    );

    // 閉じる:フォーカスのあるウィンドウが無くなる(一つ目は最小化している)ので、IME は deactivate になる。
    drop(second);
    assert_eq!(next_activation(&ime, "deactivate"), "ime: deactivate");
    ime.wait_for_line("ime: done");
}

/// 修飾キー(左の Shift)。
const KEY_LEFT_SHIFT: u32 = 42;

/// テスト用のウィンドウの色(橙)と、テスト用の IME の候補ウィンドウの色(紫)。
const ORANGE: [u8; 3] = [0xff, 0x80, 0x00];
const PURPLE: [u8; 3] = [0x80, 0x00, 0xff];

/// text-input を使うウィンドウを、引数 `args` を足して出し、描かれるまで待つ。
fn open_text_window_with(seinas: &Compositor, args: &[&str]) -> TestClient {
    let mut all = vec!["window", "--text-input"];
    all.extend(args);
    let window = seinas.connect_test_client(&all);
    window.wait_for_line("window: drawn");
    window
}

/// 候補ウィンドウ(40x30)が、左上 (x, y) に出ているか。
fn popup_at(shot: &Shot, x: usize, y: usize) -> bool {
    shot.pixel(x, y) == PURPLE
        && shot.pixel(x + 39, y + 29) == PURPLE
        && shot.pixel(x + 40, y) != PURPLE
        && shot.pixel(x, y + 30) != PURPLE
        && (x == 0 || shot.pixel(x - 1, y) != PURPLE)
        && (y == 0 || shot.pixel(x, y - 1) != PURPLE)
}

#[test]
fn the_candidate_window_follows_the_cursor_inside_the_output() {
    let seinas = Compositor::start_with_input("il");
    // 1 つ目のウィンドウの外枠は (0, 0)、コンテンツは (0, 24) から。
    let mut window = open_text_window_with(&seinas, &["--size", "640x150"]);
    let ime = connect_ime_to_window_with(&seinas, &["--popup", "40x30"]);
    // カーソルの矩形 (10, 20, 2, 16) の下:(10, 24 + 20 + 16)。
    seinas.wait_for_screen("the candidate window below the cursor", |shot| {
        popup_at(shot, 10, 60) && shot.pixel(10, 59) == ORANGE
    });
    // 右の端:カーソルは x = 620 だが、640 - 40 = 600 へ寄せる。
    window.send_line("cursor 620 20 2 16");
    ime.wait_for_line("ime: popup rectangle 620 20 2 16");
    seinas.wait_for_screen("the candidate window at the right edge", |shot| {
        popup_at(shot, 600, 60) && shot.pixel(599, 60) == ORANGE
    });
}

#[test]
fn the_candidate_window_goes_above_the_cursor_at_the_bottom_edge() {
    let seinas = Compositor::start_with_input("im");
    // コンテンツは y = 24 から 474 まで。カーソルの矩形の下の端は 24 + 440 + 8 = 472 で、下には 30 が入らない。
    let _window = open_text_window_with(&seinas, &["--size", "200x450", "--cursor", "10,440,2,8"]);
    let _ime = connect_ime_to_window_with(&seinas, &["--popup", "40x30"]);
    seinas.wait_for_screen("the candidate window above the cursor", |shot| {
        popup_at(shot, 10, 464 - 30) && shot.pixel(10, 464) == ORANGE
    });
}

#[test]
fn the_candidate_window_stays_inside_a_resized_output() {
    let mut seinas = Compositor::start_with_input("in");
    let _window = open_text_window_with(&seinas, &["--cursor", "10,100,2,16"]);
    let _ime = connect_ime_to_window_with(&seinas, &["--popup", "40x30"]);
    // カーソルの矩形は画面の上で (10, 124) から 16。下に置くと y = 140。
    seinas.wait_for_screen("the candidate window below the cursor", |shot| {
        popup_at(shot, 10, 140)
    });
    // 出力の高さを 160 にすると、下には入らない(140 + 30 > 160)ので、上(124 - 30)に置く。
    seinas.pointer("output 400x160");
    seinas.wait_for_screen("the candidate window above the cursor", |shot| {
        shot.height == 160 && popup_at(shot, 10, 94) && shot.pixel(10, 140) == ORANGE
    });
}

#[test]
fn the_candidate_window_hides_when_the_focus_leaves() {
    let seinas = Compositor::start_with_input("io");
    let first = open_text_window_with(&seinas, &[]);
    let ime = connect_ime_to_window_with(&seinas, &["--popup", "40x30"]);
    seinas.wait_for_screen("the candidate window", |shot| popup_at(shot, 10, 60));
    // 2 つ目のウィンドウ(外枠は (32, 32))にフォーカスが移ると、IME は deactivate になり、候補ウィンドウは消える。
    // (10, 60) は 2 つ目に隠れない所なので、1 つ目のコンテンツが見える。
    let _second = seinas.connect_test_client(&["window", "--keyboard"]);
    ime.wait_for_line("ime: deactivate");
    seinas.wait_for_screen("the candidate window hidden", |shot| {
        shot.pixel(10, 60) == ORANGE && shot.pixel(49, 89) != PURPLE
    });
    drop(first);
}

/// 緊急解除のキー(既定の Alt+Shift+F12)の F12。
const KEY_F12: u32 = 88;

/// キー `code` を押す(`pressed`)か、離す。
fn key(seinas: &mut Compositor, code: u32, pressed: bool) {
    let what = if pressed { "press" } else { "release" };
    seinas.pointer(&format!("key {code} {what}"));
}

/// 緊急解除のキー(Alt+Shift+F12)を押す。離すのは呼ぶ側。
fn press_release_key(seinas: &mut Compositor) {
    key(seinas, KEY_LEFT_ALT, true);
    key(seinas, KEY_LEFT_SHIFT, true);
    key(seinas, KEY_F12, true);
}

/// 緊急解除のキーを離す(F12 は Seinas が使ったので、どこへも届かない)。
fn lift_release_key(seinas: &mut Compositor) {
    key(seinas, KEY_F12, false);
    key(seinas, KEY_LEFT_SHIFT, false);
    key(seinas, KEY_LEFT_ALT, false);
}

/// ウィンドウの、`last` までの text-input の行(`last` を含む)。
fn text_lines_until(window: &TestClient, last: &str) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let line = window.next_line(last);
        let done = line == last;
        if line.starts_with("window: text-input ")
            || line.starts_with("window: preedit ")
            || line.starts_with("window: commit ")
            || line.starts_with("window: delete ")
            || line.starts_with("window: applied ")
            || line.starts_with("window: reset ")
        {
            lines.push(line);
        }
        if done {
            return lines;
        }
    }
}

/// `window: keyboard enter keys …` の行のキーの番号(順は問わない)。
fn enter_keys(line: &str) -> Vec<u32> {
    let mut keys: Vec<u32> = line
        .strip_prefix("window: keyboard enter")
        .unwrap_or_else(|| panic!("not an enter line: {line}"))
        .trim()
        .strip_prefix("keys")
        .unwrap_or("")
        .split_whitespace()
        .map(|key| key.parse().expect("a key number"))
        .collect();
    keys.sort_unstable();
    keys
}

/// IME が去った後に、ウィンドウへ送り直される leave と enter を待ち、enter のキーを返す。
fn next_reentered_keys(window: &TestClient) -> Vec<u32> {
    while next_key_line(window, "the keyboard leave") != "window: keyboard leave" {}
    enter_keys(&next_key_line(window, "the keyboard enter"))
}

#[test]
fn a_disconnected_ime_returns_the_keys_and_the_window_resets_its_preedit() {
    let mut seinas = Compositor::start_with_input("iv");
    let window = seinas.connect_test_client(&[
        "window",
        "--text-input",
        "--keyboard",
        "--title",
        "ウィンドウ",
    ]);
    window.wait_for_line("window: drawn");
    let mut ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    // commit_string を送り、commit の前に接続を切る。Seinas は、空の preedit も done も送らない。アプリには、
    // text-input の leave だけが届き、アプリが変換中の文字を消す(text-input-v3 の leave)。
    ime.send_line("commit-string x");
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
    ime.send_line("quit");
    let lines = text_lines_until(&window, "window: reset preedit");
    assert_eq!(
        lines,
        [
            "window: commit x",
            "window: text-input leave",
            "window: reset preedit"
        ]
    );
    wait_for_log(&seinas, "the input method left");
    // 次のキーから直接の道。押されたキーは残らない(送り直した enter に、押されているキーは無い)。
    assert_eq!(next_reentered_keys(&window), Vec::<u32>::new());
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    let lines = window.lines_within(std::time::Duration::from_millis(200));
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("text-input done") || line.contains("applied")),
        "{lines:?}"
    );
}

#[test]
fn the_release_key_closes_the_ime_and_keys_go_straight_to_the_window() {
    let mut seinas = Compositor::start_with_input("iw");
    let window = seinas.connect_test_client(&[
        "window",
        "--text-input",
        "--keyboard",
        "--title",
        "ウィンドウ",
    ]);
    window.wait_for_line("window: drawn");
    let mut ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    // delete_surrounding_text を送り、commit の前に緊急解除する。
    ime.send_line("delete-surrounding 1 0");
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
    // Alt と Shift は IME を通ってウィンドウに届く。IME が virtual-keyboard で送り返し終えてから F12 を押す
    // (送り返す途中で接続が切れると、IME の送信が EPIPE で失敗するため)。
    key(&mut seinas, KEY_LEFT_ALT, true);
    key(&mut seinas, KEY_LEFT_SHIFT, true);
    let mut before = Vec::new();
    loop {
        let line = window.next_line("alt and shift");
        let done = line == "window: key 42 pressed";
        before.push(line);
        if done {
            break;
        }
    }
    let keys: Vec<&String> = before
        .iter()
        .filter(|line| line.starts_with("window: key "))
        .collect();
    assert_eq!(
        keys,
        ["window: key 56 pressed", "window: key 42 pressed"],
        "{before:?}"
    );
    key(&mut seinas, KEY_F12, true);
    // IME の接続は切れて、IME は終わる。
    let status = ime.process.0.wait().expect("the IME ends");
    assert!(status.success(), "{status}");
    wait_for_log(
        &seinas,
        "the input method was released with the emergency key",
    );
    wait_for_log(&seinas, "the input method left");
    // delete_surrounding_text の行は、Seinas が IME の sync に答える前に送るので、Alt と Shift の行より先にある。
    let mut lines: Vec<String> = before
        .into_iter()
        .filter(|line| line.starts_with("window: delete "))
        .collect();
    lines.extend(text_lines_until(&window, "window: reset preedit"));
    assert_eq!(
        lines,
        [
            "window: delete 1 0",
            "window: text-input leave",
            "window: reset preedit"
        ]
    );
    // 送り直した enter には、物理のキーボードで押している Alt と Shift だけが入り、Seinas が使った F12 は
    // 入らない。離すと、直接ウィンドウに届く。
    assert_eq!(next_reentered_keys(&window), [KEY_LEFT_SHIFT, KEY_LEFT_ALT]);
    lift_release_key(&mut seinas);
    assert_eq!(next_key_line(&window, "shift"), "window: key 42 released");
    assert_eq!(next_key_line(&window, "alt"), "window: key 56 released");
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    assert_eq!(next_key_line(&window, "x"), "window: key 45 released");
    let lines = window.lines_within(std::time::Duration::from_millis(200));
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("key 88") || line.contains("applied")),
        "{lines:?}"
    );
}

#[test]
fn an_ime_that_swallows_a_key_is_not_bypassed() {
    // 半角/全角のように、文字を出さずにキーを使う IME。返事が無くても、直接の道にはしない。
    const KEY_ZENKAKU_HANKAKU: u32 = 41;
    let mut seinas = Compositor::start_with_input("ix");
    let window = seinas.connect_test_client(&[
        "window",
        "--text-input",
        "--keyboard",
        "--title",
        "ウィンドウ",
    ]);
    window.wait_for_line("window: drawn");
    let ime = connect_ime_to_window_with(&seinas, &["--swallow", "41"]);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    for _ in 0..3 {
        tap(&mut seinas, KEY_ZENKAKU_HANKAKU);
        assert_eq!(
            next_ime_key(&ime, "the swallowed key"),
            "ime: key 41 pressed"
        );
        assert_eq!(
            next_ime_key(&ime, "the swallowed key"),
            "ime: key 41 released"
        );
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    // IME を通る道のまま。
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    let lines = window.lines_within(std::time::Duration::from_millis(200));
    assert!(
        !lines.iter().any(|line| line.contains("key 41")),
        "{lines:?}"
    );
    let log = seinas.log();
    assert!(
        !log.contains("left") && !log.contains("released with"),
        "{log}"
    );
}

#[test]
fn a_key_held_on_the_keyboard_and_through_the_ime_is_not_left_pressed() {
    let mut seinas = Compositor::start_with_input("iy");
    let window = open_window(&seinas, "ウィンドウ");
    let _ime = connect_ime(&seinas, &[]);
    // x を押したまま。IME が virtual-keyboard で押したことを戻すので、ウィンドウには押されている。
    key(&mut seinas, KEY_X, true);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    press_release_key(&mut seinas);
    wait_for_log(&seinas, "the input method left");
    // 送り直した enter には、物理のキーボードで押している x・Shift・Alt が入る(IME が virtual-keyboard で押した
    // ことは、leave で消える)。離すと、直接ウィンドウに届く。
    assert_eq!(
        next_reentered_keys(&window),
        [KEY_LEFT_SHIFT, KEY_X, KEY_LEFT_ALT]
    );
    key(&mut seinas, KEY_X, false);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 released");
    lift_release_key(&mut seinas);
    assert_eq!(next_key_line(&window, "shift"), "window: key 42 released");
    assert_eq!(next_key_line(&window, "alt"), "window: key 56 released");
}

#[test]
fn a_key_pressed_only_through_the_ime_is_not_left_pressed() {
    let mut seinas = Compositor::start_with_input("iz");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // x を押して離す。IME は押したことを戻した後、離したことをためる(ウィンドウには押されたまま)。
    key(&mut seinas, KEY_X, true);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    ime.send_line("hold");
    ime.wait_for_line("ime: holding");
    key(&mut seinas, KEY_X, false);
    assert_eq!(next_ime_key(&ime, "x released"), "ime: key 45 released");
    press_release_key(&mut seinas);
    wait_for_log(&seinas, "the input method left");
    // 送り直した enter に x は入らない(物理のキーボードでは離している)。leave で、ウィンドウの x は離れる。
    assert_eq!(next_reentered_keys(&window), [KEY_LEFT_SHIFT, KEY_LEFT_ALT]);
    lift_release_key(&mut seinas);
    let lines = window.lines_within(std::time::Duration::from_millis(300));
    assert!(
        !lines.iter().any(|line| line.contains("key 45")),
        "{lines:?}"
    );
}

#[test]
fn the_release_key_works_once_while_it_is_held() {
    let mut seinas = Compositor::start_with_input("ja");
    let window = open_text_window(&seinas, "ウィンドウ");
    let mut first = connect_ime_to_window(&seinas);
    press_release_key(&mut seinas);
    let _ = first.process.0.wait();
    wait_for_log(&seinas, "the input method left");
    // 押し続けた F12 の繰り返しの押下では、新しくつないだ IME を切らない。
    let second = connect_ime_to_window(&seinas);
    key(&mut seinas, KEY_F12, true);
    key(&mut seinas, KEY_F12, true);
    lift_release_key(&mut seinas);
    tap(&mut seinas, KEY_A);
    second.wait_for_line("ime: key 30 pressed");
    while next_text_line(&window, "あ") != "window: preedit あ" {}
    assert_eq!(
        seinas
            .log()
            .matches("released with the emergency key; its connection is closed")
            .count(),
        1,
        "{}",
        seinas.log()
    );
    // 離した後に押せば、また解除する。
    press_release_key(&mut seinas);
    wait_for_log(
        &seinas,
        "released with the emergency key; its connection is closed",
    );
    common::wait_for("the second release", || {
        (seinas
            .log()
            .matches("released with the emergency key; its connection is closed")
            .count()
            == 2)
            .then_some(())
    });
    lift_release_key(&mut seinas);
}

#[test]
fn a_virtual_keyboard_key_does_not_release_the_ime() {
    let mut seinas = Compositor::start_with_input("jb");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // IME が virtual-keyboard で Alt+Shift(Shift = 1、Mod1 = 8)と F12 を送っても、Seinas のショートカットには
    // ならない(virtual-keyboard のキーは、フォーカスのあるクライアントへ直に届く)。
    ime.send_line("vk-mods 9");
    ime.send_line(&format!("vk-press {KEY_F12}"));
    ime.send_line(&format!("vk-release {KEY_F12}"));
    ime.send_line("vk-mods 0");
    loop {
        if next_key_line(&window, "F12 from the IME") == "window: key 88 released" {
            break;
        }
    }
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
    assert!(!seinas.log().contains("released with the emergency key"));
    tap(&mut seinas, KEY_X);
    assert_eq!(next_ime_key(&ime, "x on the IME"), "ime: key 45 pressed");
}

#[test]
fn a_new_ime_connection_continues_in_the_same_field() {
    let mut seinas = Compositor::start_with_input("jc");
    let window = open_text_window(&seinas, "ウィンドウ");
    let mut first = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    first.send_line("quit");
    let _ = first.process.0.wait();
    while next_text_line(&window, "leave") != "window: text-input leave" {}
    // 新しい接続の IME がつながると、同じウィンドウの同じ欄に、クリックし直さずに enter が届き、続けて打てる。
    let _second = connect_ime_to_window(&seinas);
    while next_text_line(&window, "enter") != "window: text-input enter" {}
    tap(&mut seinas, KEY_I);
    while next_text_line(&window, "い") != "window: preedit い" {}
}

/// ウィンドウの、`duration` の間の `window: keyboard leave` の行の数。
fn keyboard_leaves_within(window: &TestClient, duration: std::time::Duration) -> usize {
    window
        .lines_within(duration)
        .iter()
        .filter(|line| *line == "window: keyboard leave")
        .count()
}

#[test]
fn keys_reach_the_window_after_the_ime_destroys_only_its_input_method() {
    let mut seinas = Compositor::start_with_input("jd");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // input method だけを destroy し、grab は release しない。仕様では、子の grab も破棄されたことになる。
    ime.send_line("destroy-input-method");
    ime.wait_for_line("ime: input method destroyed");
    wait_for_log(
        &seinas,
        "the input method was destroyed; its keyboard grab is ignored",
    );
    assert_eq!(next_reentered_keys(&window), Vec::<u32>::new());
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    assert_eq!(next_key_line(&window, "x"), "window: key 45 released");
    // IME には届かない。
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
    let lines = ime.lines_within(std::time::Duration::from_millis(200));
    assert!(
        !lines.iter().any(|line| line.starts_with("ime: key")),
        "{lines:?}"
    );
}

#[test]
fn the_cleanup_runs_once_whatever_the_order() {
    // input method を先に destroy し、後で grab を release する。leave と enter の送り直しは 1 回だけ。
    let mut seinas = Compositor::start_with_input("je");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("destroy-input-method");
    ime.wait_for_line("ime: input method destroyed");
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    wait_for_log(&seinas, "the input method left");
    assert_eq!(
        keyboard_leaves_within(&window, std::time::Duration::from_millis(400)),
        1
    );
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");

    // grab を先に release し、後で input method を destroy する。
    let mut seinas = Compositor::start_with_input("jf");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&window, "y"), "window: key 21 pressed");
    ime.send_line("destroy-input-method");
    ime.wait_for_line("ime: input method destroyed");
    wait_for_log(&seinas, "the input method left");
    assert_eq!(
        keyboard_leaves_within(&window, std::time::Duration::from_millis(400)),
        1
    );
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
}

#[test]
fn the_old_cleanup_does_not_remove_the_grab_of_a_new_ime() {
    let mut seinas = Compositor::start_with_input("jg");
    let window = open_text_window(&seinas, "ウィンドウ");
    let mut old = connect_ime_to_window(&seinas);
    old.send_line("destroy-input-method");
    old.wait_for_line("ime: input method destroyed");
    // 古い世代の grab のオブジェクトが残っていても、別の接続の新しい IME を受け入れる。
    let new = connect_ime_to_window(&seinas);
    // 古い grab を release し、古い接続を切っても、新しい IME の grab は外れない。
    old.send_line("release");
    old.wait_for_line("ime: released");
    tap(&mut seinas, KEY_A);
    new.wait_for_line("ime: key 30 pressed");
    while next_text_line(&window, "あ") != "window: preedit あ" {}
    old.send_line("quit");
    let _ = old.process.0.wait();
    tap(&mut seinas, KEY_X);
    new.wait_for_line("ime: key 45 pressed");
    let log = seinas.log();
    assert!(
        !log.contains("the destroyed input method still has a keyboard grab"),
        "{log}"
    );
    assert!(
        !log.contains("the input method released the keyboard"),
        "{log}"
    );
}

#[test]
fn the_old_grab_on_the_same_connection_does_not_remove_the_new_grab() {
    let mut seinas = Compositor::start_with_input("jq");
    let _window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // input method だけを destroy し(grab は残す)、同じ接続で次の input method を受け入れて grab を取る。
    ime_lines(&mut ime, &["destroy-input-method", "again", "again-grab"]);
    common::wait_for("the second input method", || {
        (seinas.log().matches("an input method connected").count() == 2).then_some(())
    });
    // 古い grab を release しても、新しい grab は外れず、キーは新しい grab に届く。
    ime_lines(&mut ime, &["release"]);
    tap(&mut seinas, KEY_A);
    loop {
        let line = ime.next_line("a on the second grab");
        assert_ne!(line, "ime: second unavailable");
        if line == "ime: second key 30 pressed" {
            break;
        }
    }
    // 古い grab は今の grab ではないので、「手放した」とは書かない(grab のオブジェクトの同一性で見分ける)。
    let log = seinas.log();
    assert!(
        !log.contains("the input method released the keyboard"),
        "{log}"
    );
}

#[test]
fn a_second_release_before_the_cleanup_says_so() {
    let mut seinas = Compositor::start_with_input("jh");
    let _window = open_window(&seinas, "ウィンドウ");
    let _ime = connect_ime(&seinas, &[]);
    // 緊急解除を押し、F12 だけを離して、もう一度押す。1 回の書き込みで送るので、切った接続のクリーンアップ
    // (イベントループの 1 回の終わり)より先に、2 回目の押下が届く。
    seinas.pointer(&format!(
        "key {KEY_LEFT_ALT} press\nkey {KEY_LEFT_SHIFT} press\nkey {KEY_F12} press\nkey {KEY_F12} release\nkey {KEY_F12} press"
    ));
    wait_for_log(&seinas, "the input method left");
    let log = seinas.log();
    assert!(
        log.contains("the input method is already released and its connection is being cleaned up"),
        "{log}"
    );
    assert!(!log.contains("no input method is running"), "{log}");
    lift_release_key(&mut seinas);
}

/// IME に `lines` を送り、sync で、コンポジタが処理し終えるまで待つ。
fn ime_lines(ime: &mut TestClient, lines: &[&str]) {
    for line in lines {
        ime.send_line(line);
    }
    ime.send_line("sync");
    ime.wait_for_line("ime: synced");
}

/// ウィンドウに、`duration` の間にキー `code` の行が届いたか。
fn key_arrived(window: &TestClient, code: u32, duration: std::time::Duration) -> bool {
    window
        .lines_within(duration)
        .iter()
        .any(|line| line.starts_with(&format!("window: key {code} ")))
}

#[test]
fn virtual_keyboards_of_one_generation_work_in_any_order() {
    let seinas = Compositor::start_with_input("ji");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // A・B の順に作り、B から先に使う。
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-new b",
            "vk-keymap b",
            "vk-key b 45 press",
            "vk-key b 45 release",
        ],
    );
    assert_eq!(next_key_line(&window, "x from b"), "window: key 45 pressed");
    ime_lines(
        &mut ime,
        &["vk-keymap a", "vk-key a 21 press", "vk-key a 21 release"],
    );
    loop {
        if next_key_line(&window, "y from a") == "window: key 21 pressed" {
            break;
        }
    }
}

/// 前の世代の virtual-keyboard A(要求を送らない)と、同じ接続の次の世代の B を作る。A と B のどちらの要求か
/// 見分けられない形で、B の最初の要求を待つところまで進める。
fn two_generations(seinas: &Compositor) -> TestClient {
    let mut ime = connect_ime(seinas, &[]);
    // A を前の世代で作り(要求は送らない)、IME を去らせてから、同じ接続で次の世代を受け入れ、B を作る。
    ime_lines(&mut ime, &["vk-new a", "destroy-input-method", "release"]);
    wait_for_log(seinas, "the input method left");
    ime_lines(&mut ime, &["again", "vk-new b"]);
    common::wait_for("the second generation", || {
        (seinas.log().matches("an input method connected").count() == 2).then_some(())
    });
    ime
}

/// B の最初の要求 `line` を送り、見分けられないので、認めずに IME の接続を unauthorized のエラーで切ったことを
/// 確かめる(つなぎ直しを求める)。切られた後に書く要求があると、IME がエラーを読む前に、切られた接続への書き込み
/// (EPIPE)で終わることがあるので、要求は 1 つだけにする。
fn refused_as_ambiguous(seinas: &Compositor, ime: &mut TestClient, line: &str) {
    ime.send_line(line);
    let line = loop {
        let line = ime.next_line("the refusal");
        if line.starts_with("ime: protocol error") {
            break line;
        }
    };
    assert!(line.contains("cannot tell which input method"), "{line}");
    wait_for_log(
        seinas,
        "a virtual keyboard was refused (it cannot be told whether it was made for the past or the current input method); the input method is disconnected and has to reconnect",
    );
}

#[test]
fn virtual_keyboards_of_two_generations_are_not_mixed_up() {
    let seinas = Compositor::start_with_input("jj");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = two_generations(&seinas);
    // B(今の世代)が最初の要求(キーの押下)を送る。
    refused_as_ambiguous(&seinas, &mut ime, "vk-key b 45 press");
    assert!(!key_arrived(
        &window,
        45,
        std::time::Duration::from_millis(300)
    ));
    // つなぎ直した IME の virtual-keyboard は、今の世代として使える。
    wait_for_log(&seinas, "the input method left");
    let window = open_window(&seinas, "ウィンドウ 2");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new c",
            "vk-keymap c",
            "vk-key c 30 press",
            "vk-key c 30 release",
        ],
    );
    loop {
        if next_key_line(&window, "a from c") == "window: key 30 pressed" {
            break;
        }
    }
}

#[test]
fn a_keymap_from_a_virtual_keyboard_of_an_unknown_generation_is_refused() {
    // 最初の要求が keymap でも、キーと同じく、見分けられないので認めずに切る(要求の種類で、断る道が分かれない)。
    let seinas = Compositor::start_with_input("jp");
    let _window = open_window(&seinas, "ウィンドウ");
    let mut ime = two_generations(&seinas);
    refused_as_ambiguous(&seinas, &mut ime, "vk-keymap b");
    wait_for_log(&seinas, "the input method left");
}

#[test]
fn a_virtual_keyboard_destroyed_before_its_first_request_does_not_shift_the_queue() {
    let seinas = Compositor::start_with_input("jk");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // 要求を送らずに destroy したものは、待ち行列から外れる(上限の 8 を越えて、作って destroy をくり返せる)。
    for i in 0..10 {
        ime_lines(
            &mut ime,
            &[&format!("vk-new a{i}"), &format!("vk-destroy a{i}")],
        );
    }
    ime_lines(
        &mut ime,
        &[
            "vk-new b",
            "vk-keymap b",
            "vk-key b 45 press",
            "vk-key b 45 release",
        ],
    );
    assert_eq!(next_key_line(&window, "x from b"), "window: key 45 pressed");
}

#[test]
fn too_many_live_virtual_keyboards_are_refused() {
    let seinas = Compositor::start_with_input("jo");
    let _window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // 最初の要求(keymap)を送ったものも数える。テスト用の IME は、つないだときに 1 つ作るので、ここで作れる
    // のは 15 個まで(合わせて 16 個)。次の 1 つは unauthorized のエラーで切られる。
    for i in 0..15 {
        ime_lines(
            &mut ime,
            &[&format!("vk-new k{i}"), &format!("vk-keymap k{i}")],
        );
    }
    assert!(
        !seinas.log().contains("a virtual keyboard was refused"),
        "{}",
        seinas.log()
    );
    ime.send_line("vk-new k15");
    let line = loop {
        let line = ime.next_line("the refusal");
        if line.starts_with("ime: protocol error") {
            break line;
        }
    };
    assert!(line.contains("are alive on this connection"), "{line}");
    wait_for_log(
        &seinas,
        "a virtual keyboard was refused (too many are alive on the connection)",
    );
}

#[test]
fn too_many_waiting_virtual_keyboards_are_refused() {
    let seinas = Compositor::start_with_input("jl");
    let _window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // 最初の要求を送らずに 8 個まで作れて、9 個目は unauthorized のエラーで切られる。
    for i in 0..8 {
        ime.send_line(&format!("vk-new k{i}"));
    }
    ime_lines(&mut ime, &[]);
    ime.send_line("vk-new k8");
    let line = loop {
        let line = ime.next_line("the refusal");
        if line.starts_with("ime: protocol error") {
            break line;
        }
    };
    assert!(line.contains("too many virtual keyboards"), "{line}");
    wait_for_log(
        &seinas,
        "a virtual keyboard was refused (too many wait for their first request)",
    );
    // 切れた接続の後につないだ IME の virtual-keyboard は、ふつうに使える。
    wait_for_log(&seinas, "the input method left");
    let window = open_window(&seinas, "ウィンドウ 2");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            "vk-key a 45 press",
            "vk-key a 45 release",
        ],
    );
    loop {
        if next_key_line(&window, "x from a") == "window: key 45 pressed" {
            break;
        }
    }
}

#[test]
fn a_virtual_keyboard_key_released_after_the_grab_release_is_not_left_pressed() {
    // virtual-keyboard で押す → grab を release → virtual-keyboard で離す。
    let seinas = Compositor::start_with_input("jm");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(&mut ime, &[&format!("vk-press {KEY_X}")]);
    assert_eq!(
        next_key_line(&window, "x pressed"),
        "window: key 45 pressed"
    );
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    ime_lines(&mut ime, &[&format!("vk-release {KEY_X}")]);
    assert_eq!(
        next_key_line(&window, "x released"),
        "window: key 45 released"
    );
}

#[test]
fn a_virtual_keyboard_key_left_pressed_after_the_grab_release_is_released_with_the_virtual_keyboard(
) {
    // virtual-keyboard で押す → grab を release → 離したことは来ない。grab の release だけでは消さない(仕様は、
    // grab の release が virtual-keyboard の押下を消すとは書いていない。遅れて正しい「離した」が届く形もある)。
    let seinas = Compositor::start_with_input("jn");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            &format!("vk-key a {KEY_X} press"),
        ],
    );
    assert_eq!(
        next_key_line(&window, "x pressed"),
        "window: key 45 pressed"
    );
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    wait_for_log(&seinas, "the input method released the keyboard");
    let lines = window.lines_within(std::time::Duration::from_millis(300));
    assert!(
        !lines
            .iter()
            .any(|line| line == "window: key 45 released" || line == "window: keyboard leave"),
        "{lines:?}"
    );
    // virtual-keyboard が壊れたら(destroy)、押したままのキーの「離した」が届く。
    ime_lines(&mut ime, &["vk-destroy a"]);
    assert_eq!(
        next_key_line(&window, "x released"),
        "window: key 45 released"
    );
}

/// ウィンドウに、`duration` の間に届いたキーとキーボードの行。
fn key_lines(window: &TestClient, duration: std::time::Duration) -> Vec<String> {
    window
        .lines_within(duration)
        .into_iter()
        .filter(|line| {
            line.starts_with("window: key ")
                || line.starts_with("window: keyboard ")
                || line.starts_with("window: modifiers ")
        })
        .collect()
}

/// 次に届く「離した」か leave の行(どちらが先かを見る)。
fn next_release_or_leave(window: &TestClient, what: &str) -> String {
    loop {
        let line = window.next_line(what);
        if line.ends_with(" released") || line == "window: keyboard leave" {
            return line;
        }
    }
}

#[test]
fn a_virtual_keyboard_destroyed_with_a_key_pressed_releases_it() {
    let seinas = Compositor::start_with_input("ka");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            &format!("vk-key a {KEY_A} press"),
        ],
    );
    assert_eq!(
        next_key_line(&window, "a pressed"),
        "window: key 30 pressed"
    );
    ime_lines(&mut ime, &["vk-destroy a"]);
    assert_eq!(
        next_key_line(&window, "a released"),
        "window: key 30 released"
    );
    wait_for_log(
        &seinas,
        "released 1 key(s) left pressed by a virtual keyboard",
    );
}

#[test]
fn a_disconnected_ime_releases_the_keys_of_its_virtual_keyboard() {
    let seinas = Compositor::start_with_input("kb");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(&mut ime, &[&format!("vk-press {KEY_X}")]);
    assert_eq!(
        next_key_line(&window, "x pressed"),
        "window: key 45 pressed"
    );
    ime.process.0.kill().expect("stop the IME");
    // leave の送り直しより先に、キーごとの「離した」が届く。
    assert_eq!(
        next_release_or_leave(&window, "x released"),
        "window: key 45 released"
    );
}

#[test]
fn a_destroyed_input_method_releases_the_keys_of_its_virtual_keyboard() {
    // input method の destroy で、認可が消える(この後の virtual-keyboard の「離した」は捨てる)。
    let seinas = Compositor::start_with_input("kc");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(&mut ime, &[&format!("vk-press {KEY_X}")]);
    assert_eq!(
        next_key_line(&window, "x pressed"),
        "window: key 45 pressed"
    );
    ime_lines(&mut ime, &["destroy-input-method"]);
    assert_eq!(
        next_release_or_leave(&window, "x released"),
        "window: key 45 released"
    );
}

#[test]
fn the_release_key_releases_the_keys_of_the_virtual_keyboards() {
    let mut seinas = Compositor::start_with_input("kd");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(&mut ime, &[&format!("vk-press {KEY_A}")]);
    assert_eq!(
        next_key_line(&window, "a pressed"),
        "window: key 30 pressed"
    );
    // 緊急解除のキー。Alt と Shift は IME を通ってウィンドウに届き、物理のキーボードで押されたまま。
    key(&mut seinas, KEY_LEFT_ALT, true);
    key(&mut seinas, KEY_LEFT_SHIFT, true);
    assert_eq!(next_key_line(&window, "alt"), "window: key 56 pressed");
    loop {
        if next_key_line(&window, "shift") == "window: key 42 pressed" {
            break;
        }
    }
    key(&mut seinas, KEY_F12, true);
    // virtual-keyboard で押した a だけが離され、物理のキーボードで押している Alt と Shift は離されない。
    assert_eq!(
        next_release_or_leave(&window, "a released"),
        "window: key 30 released"
    );
    assert_eq!(
        next_release_or_leave(&window, "the leave"),
        "window: keyboard leave"
    );
    lift_release_key(&mut seinas);
}

#[test]
fn a_virtual_keyboard_key_is_released_once_when_the_destroy_and_the_disconnection_follow() {
    let seinas = Compositor::start_with_input("ke");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            &format!("vk-key a {KEY_A} press"),
        ],
    );
    assert_eq!(
        next_key_line(&window, "a pressed"),
        "window: key 30 pressed"
    );
    ime_lines(&mut ime, &["vk-destroy a"]);
    assert_eq!(
        next_key_line(&window, "a released"),
        "window: key 30 released"
    );
    // 続けて接続が切れても、もう一度「離した」を送らない(記録は、片付けたときに外している)。
    ime.process.0.kill().expect("stop the IME");
    wait_for_log(&seinas, "the input method left");
    let lines = key_lines(&window, std::time::Duration::from_millis(300));
    assert!(
        !lines.iter().any(|line| line == "window: key 30 released"),
        "{lines:?}"
    );
    assert_eq!(
        seinas.log().matches("released 1 key(s)").count(),
        1,
        "{}",
        seinas.log()
    );
}

#[test]
fn a_key_held_on_the_physical_keyboard_is_not_released_with_a_virtual_keyboard() {
    let mut seinas = Compositor::start_with_input("kf");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // IME が grab を手放すと、物理のキーボードのキーは、直接ウィンドウに届く(IME の virtual-keyboard を通らない)。
    ime.send_line("release");
    ime.wait_for_line("ime: released");
    wait_for_log(&seinas, "the input method released the keyboard");
    key(&mut seinas, KEY_X, true);
    assert_eq!(
        next_key_line(&window, "x pressed"),
        "window: key 45 pressed"
    );
    // 別の virtual-keyboard も x を押してから、壊れる。物理の x は押されたままなので、離さない。
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            &format!("vk-key a {KEY_X} press"),
            "vk-destroy a",
        ],
    );
    let lines = key_lines(&window, std::time::Duration::from_millis(300));
    assert!(
        !lines.iter().any(|line| line == "window: key 45 released"),
        "{lines:?}"
    );
    // 物理のキーボードで離すと、ふつうに届く。
    key(&mut seinas, KEY_X, false);
    assert_eq!(
        next_key_line(&window, "x released"),
        "window: key 45 released"
    );
}

#[test]
fn only_the_keys_of_the_broken_virtual_keyboard_are_released() {
    const KEY_S: u32 = 31;
    const KEY_D: u32 = 32;
    let seinas = Compositor::start_with_input("kg");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // a は a と d を、b は d と s を押す。a が壊れたら、a だけが押しているもの(a)だけを離す。
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            "vk-new b",
            "vk-keymap b",
            &format!("vk-key a {KEY_A} press"),
            &format!("vk-key a {KEY_D} press"),
            &format!("vk-key b {KEY_D} press"),
            &format!("vk-key b {KEY_S} press"),
        ],
    );
    loop {
        if next_key_line(&window, "s pressed") == "window: key 31 pressed" {
            break;
        }
    }
    ime_lines(&mut ime, &["vk-destroy a"]);
    let lines = key_lines(&window, std::time::Duration::from_millis(300));
    let released: Vec<&String> = lines
        .iter()
        .filter(|line| line.ends_with(" released"))
        .collect();
    assert_eq!(released, ["window: key 30 released"], "{lines:?}");
}

#[test]
fn keys_are_not_released_after_the_focus_moved() {
    let seinas = Compositor::start_with_input("kh");
    let first = open_window(&seinas, "ウィンドウ 1");
    let mut ime = connect_ime(&seinas, &[]);
    ime_lines(
        &mut ime,
        &[
            "vk-new a",
            "vk-keymap a",
            &format!("vk-key a {KEY_A} press"),
        ],
    );
    assert_eq!(next_key_line(&first, "a pressed"), "window: key 30 pressed");
    // 2 つ目のウィンドウにフォーカスが移る(1 つ目には leave が届き、押下はアプリの側で消える)。
    let second = open_window(&seinas, "ウィンドウ 2");
    loop {
        if next_key_line(&first, "the leave") == "window: keyboard leave" {
            break;
        }
    }
    ime_lines(&mut ime, &["vk-destroy a"]);
    let wait = std::time::Duration::from_millis(300);
    let lines = key_lines(&second, wait);
    assert!(
        !lines.iter().any(|line| line.ends_with(" released")),
        "{lines:?}"
    );
    let lines = key_lines(&first, wait);
    assert!(
        !lines.iter().any(|line| line.ends_with(" released")),
        "{lines:?}"
    );
}

#[test]
fn modifiers_set_by_a_virtual_keyboard_are_restored_when_it_breaks() {
    let seinas = Compositor::start_with_input("ki");
    let window = open_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime(&seinas, &[]);
    // キーは押さず、modifiers(Shift)だけを渡す。
    ime_lines(&mut ime, &["vk-new a", "vk-keymap a", "vk-mods a 1"]);
    common::wait_for("the shift modifier", || {
        key_lines(&window, std::time::Duration::from_millis(50))
            .iter()
            .any(|line| line == "window: modifiers 1 0 0 0")
            .then_some(())
    });
    ime_lines(&mut ime, &["vk-destroy a"]);
    common::wait_for("the restored modifiers", || {
        key_lines(&window, std::time::Duration::from_millis(50))
            .iter()
            .any(|line| line == "window: modifiers 0 0 0 0")
            .then_some(())
    });
}

#[test]
fn a_virtual_keyboard_request_refused_before_the_first_keymap_is_not_cleaned_up_later() {
    // seat で最初の keymap が届く前は、Smithay が virtual-keyboard の modifiers と key を NoKeymap のエラーで断り、
    // アプリへは何も渡さない。断られたものを記録すると、接続が切れた後の片付けで、渡していない modifiers や
    // 「離した」を送り、事実と違うログを出してしまう。
    let seinas = Compositor::start_with_input("kj");
    let _window = open_window(&seinas, "ウィンドウ");
    // built-in の virtual-keyboard に keymap を送らない IME は、grab の modifiers を送り直したところで、NoKeymap の
    // エラーで切られて終わる(終わりの値 2)。切られて終わったことを確かめてから先へ進み、終わった IME には何も
    // 書かない(いつ終わるかは決まっていないので、書くと、先に終わったときに EPIPE になる)。key の道は、modifiers
    // で先に切られるので、このテストでは通らない(コードの道は modifiers と同じ)。
    let mut ime = seinas.connect_ime_client(&["ime", "--no-builtin-keymap"]);
    let code = common::wait_for("the IME to be disconnected and end", || {
        ime.process.0.try_wait().expect("look at the IME")
    })
    .code();
    assert_eq!(code, Some(2), "{}", seinas.log());
    wait_for_log(&seinas, "`modifiers` sent before keymap.");
    // 別のクライアントで往復して、切れた接続の片付けが済んだ後にする(時間には頼らない)。
    let other = seinas.connect_test_client(&["window", "--title", "other"]);
    other.wait_for_line("window: drawn");
    let log = seinas.log();
    assert!(!log.contains("left pressed by a virtual keyboard"), "{log}");
}

/// 緊急解除のキーの組(Alt、Shift、F12)を、押してから逆の順に離す。
fn press_and_lift_release_key(seinas: &mut Compositor) {
    press_release_key(seinas);
    lift_release_key(seinas);
}

/// ウィンドウの、次の `count` 個のキーの行(keyboard の enter・leave は除く)。
fn key_events(window: &TestClient, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| loop {
            let line = next_key_line(window, "a key");
            if line.starts_with("window: key ") {
                break line;
            }
        })
        .collect()
}

#[test]
fn the_release_key_goes_to_the_window_without_the_ime_socket() {
    // IME のためのソケットを開かない構成では、緊急解除のキーを予約しない。
    let mut seinas = Compositor::start_with_input_and("kk", &["--no-ime-socket"]);
    let window = open_window(&seinas, "ウィンドウ");
    press_and_lift_release_key(&mut seinas);
    assert_eq!(
        key_events(&window, 6),
        [
            "window: key 56 pressed",
            "window: key 42 pressed",
            "window: key 88 pressed",
            "window: key 88 released",
            "window: key 42 released",
            "window: key 56 released",
        ]
    );
    let log = seinas.log();
    assert!(
        log.contains("the release key for the input method is not reserved"),
        "{log}"
    );
    assert!(
        !log.contains("the emergency release key was pressed"),
        "{log}"
    );
}

#[test]
fn the_release_key_is_reserved_while_no_ime_is_connected() {
    // IME のためのソケットを開く構成では、IME がつながっていなくても予約する(F12 の押下も解放も渡さない)。
    let mut seinas = Compositor::start_with_input("kl");
    let window = open_window(&seinas, "ウィンドウ");
    press_and_lift_release_key(&mut seinas);
    // 修飾キーは押下も解放も渡り、F12 は渡らない。続けて F12 だけを押すと、ふつうに渡る。
    tap(&mut seinas, KEY_F12);
    assert_eq!(
        key_events(&window, 6),
        [
            "window: key 56 pressed",
            "window: key 42 pressed",
            "window: key 42 released",
            "window: key 56 released",
            "window: key 88 pressed",
            "window: key 88 released",
        ]
    );
    wait_for_log(
        &seinas,
        "the emergency release key was pressed, but no input method is running",
    );
    assert!(
        !seinas
            .log()
            .contains("the release key for the input method is not reserved"),
        "{}",
        seinas.log()
    );
}

// ---- Kieli との統合の前の確かめ(ADR 0004 の 4)。標準の input-method-v2 と text-input-v3 の振る舞いを、テスト用の
// IME とアプリで確かめる。

const KEY_BACKSPACE: u32 = 14;

#[test]
fn the_activation_follows_the_application_enabling_and_disabling_the_text_input() {
    // 項目 1:アプリの enable・disable・commit が、IME の activate・deactivate・done に対応する。
    let mut seinas = Compositor::start_with_input("la");
    let mut window = open_text_window(&seinas, "ウィンドウ");
    let ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    // アプリが無効にして commit すると、IME に deactivate と done が届く。
    window.send_line("text-input disable");
    assert_eq!(next_activation(&ime, "deactivate"), "ime: deactivate");
    ime.wait_for_line("ime: done");
    // 無効の間のキーは、変換されずにアプリへ戻る。
    tap(&mut seinas, KEY_A);
    assert_eq!(next_key_line(&window, "a"), "window: key 30 pressed");
    // アプリが有効にし直して commit すると、IME に activate と done が届き、また変換する。
    window.send_line("text-input enable");
    wait_for_activate(&ime, "activate again");
    tap(&mut seinas, KEY_I);
    assert_eq!(next_text_change(&window, "い"), "window: preedit い");
}

#[test]
fn the_preedit_is_updated_cleared_and_committed_in_order() {
    // 項目 2:変換中の文字の更新・消去・確定が、送った順に、done ごとに届く。
    let mut seinas = Compositor::start_with_input("lb");
    let window = open_text_window(&seinas, "ウィンドウ");
    let _ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    let mut lines = Vec::new();
    for key in [KEY_A, KEY_I, KEY_BACKSPACE, KEY_BACKSPACE, KEY_A, KEY_ENTER] {
        tap(&mut seinas, key);
        loop {
            let line = next_text_line(&window, "the next text-input line");
            let done = line == "window: text-input done";
            lines.push(line);
            if done {
                break;
            }
        }
    }
    assert_eq!(
        lines,
        [
            "window: preedit あ",
            "window: text-input done",
            "window: preedit あい",
            "window: text-input done",
            "window: preedit あ",
            "window: text-input done",
            "window: preedit ",
            "window: text-input done",
            "window: preedit あ",
            "window: text-input done",
            "window: commit あ",
            "window: text-input done",
        ]
    );
}

#[test]
#[ignore = "直していない:Smithay 0.7.0 は、IME の要求を届いたその場で、そのときアクティブな text-input へ送る(ADR 0004 の 1.5 の未解決)"]
fn an_answer_sent_before_a_focus_change_does_not_reach_the_new_window() {
    // 項目 3:フォーカスを変える前の応答(古い入力欄への変換中の文字)が、新しいアプリへ入らない。
    let mut seinas = Compositor::start_with_input("lc");
    let first = open_text_window(&seinas, "一つ目");
    let second = open_text_window(&seinas, "二つ目");
    let mut ime = connect_ime_to_window(&seinas);
    assert_eq!(
        next_text_change(&second, "enter"),
        "window: text-input enter"
    );
    // 二つ目への応答を、送らずにためておく。
    ime.send_line("hold");
    ime.wait_for_line("ime: holding");
    tap(&mut seinas, KEY_A);
    ime.wait_for_line("ime: preedit あ");
    // Alt+Tab で一つ目へ移し、一つ目が有効になってから、ためた応答を送る。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    tap(&mut seinas, KEY_TAB);
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    assert_eq!(
        next_text_change(&first, "enter"),
        "window: text-input enter"
    );
    wait_for_activate(&ime, "activate on the first");
    ime.send_line("send");
    while !ime
        .next_line("the held answers sent")
        .starts_with("ime: sent ")
    {}
    let lines = first.lines_within(std::time::Duration::from_millis(500));
    assert!(
        !lines.iter().any(|line| line == "window: preedit あ"),
        "{lines:?}"
    );
}

#[test]
fn a_destroyed_input_method_leaves_no_preedit_in_the_window() {
    // 項目 5:input method の destroy で、アプリに変換中の文字が残らない(切断と緊急解除は、ほかのテストで見る)。
    let mut seinas = Compositor::start_with_input("ld");
    let window = open_text_window(&seinas, "ウィンドウ");
    let mut ime = connect_ime_to_window(&seinas);
    assert_eq!(next_text_line(&window, "enter"), "window: text-input enter");
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_line(&window, "あ"), "window: preedit あ");
    assert_eq!(next_text_line(&window, "done"), "window: text-input done");
    ime.send_line("destroy-input-method");
    ime.wait_for_line("ime: input method destroyed");
    assert_eq!(
        text_lines_until(&window, "window: reset preedit"),
        ["window: text-input leave", "window: reset preedit"]
    );
    // 次のキーは、直接ウィンドウへ届く(その前に、wl_keyboard の leave と enter の送り直しが届く)。
    tap(&mut seinas, KEY_X);
    while next_key_line(&window, "x") != "window: key 45 pressed" {}
}
