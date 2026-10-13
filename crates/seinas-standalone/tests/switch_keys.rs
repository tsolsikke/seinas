//! ウィンドウの切り替えのキーの設定と既定を、テスト用のクライアントの出力とログで確かめる。
//!
//! 決まり(`docs/window-placement.md` の「Alt+Tab」、`docs/config.md`):
//! - 既定は Alt+Tab と Alt+Shift+Tab。入れ子の構成では、Alt+F6 と Alt+Shift+F6 も使う。
//! - `seinas.conf` の `switch-next`・`switch-prev` で変えられる。書いたキーは、既定と置き換わる。読めない値は、ログに
//!   出して既定のまま。
//! - 切り替えの動きは、どのキーでも Alt+Tab と同じ(押したまま続けて押すと進み、修飾キーを離すと決める)。
//! - `--log-keys` で、届いたキーを `key: Alt+F6 pressed` の形でログに出す。
//!
//! 入れ子の構成は、seinas-standalone を親にして `seinas` を動かし、親のテスト用の入力インターフェースからキーを送る。
//! 親も Alt+Tab を使う(入れ子の Seinas に届かない。WSLg での Windows と同じ)ので、親の切り替えのキーは設定で変える。
//! 3 つのウィンドウ a、b、c を出すと、c にフォーカスがある。次へのキーを、修飾キーを押したまま 2 回押して離すと、
//! a へ移る。

mod common;

use std::{fs, path::PathBuf};

use common::{wait_for, Compositor, TestClient};

/// evdev のキーの番号。
const TAB: u32 = 15;
const LEFT_CTRL: u32 = 29;
const ALT: u32 = 56;
const F6: u32 = 64;
const F7: u32 = 65;

/// キーボードの、次の行(キー、enter、leave)。修飾キーの状態と keymap の行は読み飛ばす。
fn next_key_line(client: &TestClient, what: &str) -> String {
    loop {
        let line = client.next_line(what);
        if line.starts_with("window: key ") || line.starts_with("window: keyboard ") {
            return line;
        }
    }
}

/// キーボードを使うウィンドウ a、b、c を `connect` で出す。c にフォーカスがあり、a・b の leave まで読んである。
fn three_windows(connect: impl Fn(&str) -> TestClient) -> [TestClient; 3] {
    let open = |title: &str| {
        let window = connect(title);
        window.wait_for_line("window: drawn");
        assert_eq!(next_key_line(&window, "enter"), "window: keyboard enter");
        window
    };
    let a = open("a");
    let b = open("b");
    assert_eq!(next_key_line(&a, "leave"), "window: keyboard leave");
    let c = open("c");
    assert_eq!(next_key_line(&b, "leave"), "window: keyboard leave");
    [a, b, c]
}

/// 修飾キー `modifier` を押したまま `key` を 2 回押して離し、修飾キーを離す。
fn switch_twice(seinas: &mut Compositor, modifier: u32, key: u32) {
    seinas.pointer(&format!("key {modifier} press"));
    for _ in 0..2 {
        seinas.pointer(&format!("key {key} press"));
        seinas.pointer(&format!("key {key} release"));
    }
    seinas.pointer(&format!("key {modifier} release"));
}

/// c から 2 つ進んで、a にフォーカスが移ったこと(c には修飾キーを押したことと leave、a には enter が届く)。
fn expect_switched_to_a(a: &TestClient, c: &TestClient, modifier: u32) {
    assert_eq!(
        next_key_line(c, "the modifier"),
        format!("window: key {modifier} pressed")
    );
    assert_eq!(
        next_key_line(c, "the modifier released"),
        format!("window: key {modifier} released")
    );
    assert_eq!(next_key_line(c, "leave"), "window: keyboard leave");
    assert_eq!(next_key_line(a, "enter"), "window: keyboard enter");
}

/// テストごとの設定のファイル(`--config` で渡す)。印を付けた作業用の場所に置く(片付けられずに残っても、
/// 次のテストか `tools/check-leftovers.py` が消す)。
struct Settings {
    dir: PathBuf,
    path: PathBuf,
}

impl Settings {
    fn new(name: &str, text: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("seinas-{name}cfg-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create the settings directory");
        fs::write(dir.join(common::DIR_MARK), "").expect("mark the settings directory");
        let path = dir.join("seinas.conf");
        fs::write(&path, text).expect("write the settings");
        Settings { dir, path }
    }

    fn arg(&self) -> &str {
        self.path.to_str().expect("a UTF-8 path")
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn alt_f6_switches_windows_in_the_nested_compositor() {
    // 親(seinas-standalone)も Alt+Tab を使うので、Windows と同じく、入れ子の Seinas には届かない。入れ子でも
    // Alt+Tab が残ることを確かめるため、親の切り替えのキーは、使わないものに変えておく。
    let parent_settings = Settings::new(
        "ka6",
        "switch-next = Ctrl+Super+F12\nswitch-prev = Ctrl+Super+Shift+F12\n",
    );
    let mut seinas = Compositor::start_with_input_and("ka6", &["--config", parent_settings.arg()]);
    let nested = seinas.start_nested(&["--log-keys"]);
    let socket = nested.socket.clone();
    let [a, _b, c] = three_windows(|title| {
        seinas.connect_test_client_at(&socket, &["window", "--keyboard", "--title", title])
    });
    // Alt を押したまま F6 を 2 回押すと、c → b → a と進み、Alt を離すと a に決まる。
    switch_twice(&mut seinas, ALT, F6);
    expect_switched_to_a(&a, &c, ALT);
    // 入れ子の Seinas に届いたキーが、修飾キーと keysym の名前でログに出る。
    wait_for("the logged keys", || {
        let log = nested.log();
        (log.contains("key: Alt_L pressed") && log.contains("key: Alt+F6 pressed")).then_some(())
    });
    let log = nested.log();
    assert!(
        log.contains("switch windows: next Alt+Tab, Alt+F6; previous Alt+Shift+Tab, Alt+Shift+F6")
    );
    // 入れ子でも Alt+Tab は残る(a から 2 つ進んで、b へ)。
    switch_twice(&mut seinas, ALT, TAB);
    assert_eq!(next_key_line(&a, "Alt"), "window: key 56 pressed");
    assert_eq!(next_key_line(&a, "Alt released"), "window: key 56 released");
    assert_eq!(next_key_line(&a, "leave"), "window: keyboard leave");
    drop(nested);
}

#[test]
fn alt_f6_is_not_used_without_the_nested_compositor() {
    let mut seinas = Compositor::start_with_input("kb6");
    let [_a, _b, c] = three_windows(|title| {
        seinas.connect_test_client(&["window", "--keyboard", "--title", title])
    });
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {F6} press"));
    seinas.pointer(&format!("key {F6} release"));
    seinas.pointer(&format!("key {ALT} release"));
    // 既定には Alt+F6 が無いので、F6 は c へ渡り、フォーカスも c のまま。
    for line in [
        "window: key 56 pressed",
        "window: key 64 pressed",
        "window: key 64 released",
        "window: key 56 released",
    ] {
        assert_eq!(next_key_line(&c, line), line);
    }
    assert!(seinas
        .log()
        .contains("switch windows: next Alt+Tab; previous Alt+Shift+Tab"));
}

#[test]
fn configured_keys_replace_the_defaults() {
    let settings = Settings::new(
        "kc",
        "# 切り替えのキー\nswitch-next = Ctrl+F7\nswitch-prev = Ctrl+Shift+F7\n",
    );
    let mut seinas = Compositor::start_with_input_and("kc", &["--config", settings.arg()]);
    let [a, _b, c] = three_windows(|title| {
        seinas.connect_test_client(&["window", "--keyboard", "--title", title])
    });
    switch_twice(&mut seinas, LEFT_CTRL, F7);
    expect_switched_to_a(&a, &c, LEFT_CTRL);
    // Alt+Tab は、置き換わったので使わない(Tab は a へ渡る)。
    seinas.pointer(&format!("key {ALT} press"));
    seinas.pointer(&format!("key {TAB} press"));
    seinas.pointer(&format!("key {TAB} release"));
    seinas.pointer(&format!("key {ALT} release"));
    for line in [
        "window: key 56 pressed",
        "window: key 15 pressed",
        "window: key 15 released",
        "window: key 56 released",
    ] {
        assert_eq!(next_key_line(&a, line), line);
    }
    assert!(seinas
        .log()
        .contains("switch windows: next Ctrl+F7; previous Ctrl+Shift+F7"));
}

#[test]
fn unreadable_values_are_logged_and_the_defaults_stay() {
    let settings = Settings::new(
        "ku",
        "switch-next = Alt+NoSuchKey\nswitch-prev = Hyper+Tab\n",
    );
    let mut seinas = Compositor::start_with_input_and("ku", &["--config", settings.arg()]);
    let [a, _b, c] = three_windows(|title| {
        seinas.connect_test_client(&["window", "--keyboard", "--title", title])
    });
    let log = seinas.log();
    assert!(
        log.contains("switch-next keeps the default: unknown key name \"NoSuchKey\""),
        "{log}"
    );
    assert!(
        log.contains("switch-prev keeps the default: unknown modifier \"hyper\""),
        "{log}"
    );
    switch_twice(&mut seinas, ALT, TAB);
    expect_switched_to_a(&a, &c, ALT);
}
