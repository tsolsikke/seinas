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

/// テスト用の IME を、シェルの部品のためのソケットへつなぎ、grab を取るまで待つ。
fn connect_ime(seinas: &Compositor, args: &[&str]) -> TestClient {
    let mut all = vec!["ime"];
    all.extend(args);
    let ime = seinas.connect_shell_client(&all);
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
    let window = open_window(&seinas, "窓");
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

#[test]
fn the_input_method_globals_are_only_on_the_shell_socket() {
    let seinas = Compositor::start_with_input("ib");
    let mut ime = seinas.connect_test_client(&["ime"]);
    let status = ime.process.0.wait().expect("the test client ends");
    // ふだんのソケットには input-method の global が無いので、準備の段階で失敗する(exit status 2)。
    assert_eq!(status.code(), Some(2));
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
    loop {
        let line = next_key_line(&first, "x on the first window");
        // IME が戻した、Alt を離したことは、フォーカスが移った後に届く(前のウィンドウには leave が届いている
        // ので、押されたまま残るキーは無い)。
        if line != "window: key 56 released" {
            assert_eq!(line, "window: key 45 pressed");
            break;
        }
    }
}

#[test]
fn keys_go_straight_to_the_window_after_the_ime_leaves() {
    let mut seinas = Compositor::start_with_input("id");
    let window = open_window(&seinas, "窓");
    let mut ime = connect_ime(&seinas, &[]);
    ime.send_line("quit");
    wait_for_log(&seinas, "the input method left");
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&window, "y"), "window: key 21 pressed");
    assert_eq!(next_key_line(&window, "y"), "window: key 21 released");
}

#[test]
fn keys_go_straight_to_the_window_after_the_ime_releases_the_keyboard() {
    let mut seinas = Compositor::start_with_input("ie");
    let window = open_window(&seinas, "窓");
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
    let window = open_window(&seinas, "窓");
    let first = connect_ime(&seinas, &[]);
    // 2 つ目は、get_input_method と grab_keyboard を送った後に unavailable を受けて終わる。
    let mut second = seinas.connect_shell_client(&["ime"]);
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
    let ime = seinas.connect_shell_client(&all);
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
    let window = open_text_window(&seinas, "窓");
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
}

#[test]
fn a_window_opened_after_the_ime_gets_the_text_input() {
    let mut seinas = Compositor::start_with_input("ih");
    let ime = connect_ime(&seinas, &[]);
    let window = open_text_window(&seinas, "窓");
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

/// テストで使う、IME が応答しないと見なすまでの時間(ミリ秒)。
const IME_TIMEOUT: u64 = 300;

/// IME が応答しないと見なすまでの時間を短くして起動する。
fn start_with_ime_timeout(name: &str) -> Compositor {
    Compositor::start_with_input_and(name, &["--ime-timeout", &IME_TIMEOUT.to_string()])
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

#[test]
fn an_unresponsive_ime_is_bypassed_and_comes_back() {
    let mut seinas = start_with_ime_timeout("ij");
    let window = open_window(&seinas, "窓");
    let mut ime = connect_ime(&seinas, &["--ignore-keys"]);
    // 修飾キーだけの押下は、返事が無くても数えない。
    for _ in 0..3 {
        tap(&mut seinas, KEY_LEFT_SHIFT);
        assert_eq!(next_ime_key(&ime, "Shift"), "ime: key 42 pressed");
        assert_eq!(next_ime_key(&ime, "Shift"), "ime: key 42 released");
        sleep_ms(IME_TIMEOUT);
    }
    assert!(!seinas.log().contains("did not answer"));
    // ほかのキーに返事が無ければ、応答しないと見なす。
    tap(&mut seinas, KEY_X);
    assert_eq!(next_ime_key(&ime, "x"), "ime: key 45 pressed");
    wait_for_log(&seinas, "the input method did not answer a key in 300 ms");
    // 次のキーから、直接の道。返事の無かったキー(x)は、ウィンドウへ送り直さない。
    tap(&mut seinas, KEY_Y);
    assert_eq!(next_key_line(&window, "y"), "window: key 21 pressed");
    assert_eq!(next_key_line(&window, "y"), "window: key 21 released");
    ime.send_line("sync");
    loop {
        let line = ime.next_line("the sync");
        assert!(!line.starts_with("ime: key 21"), "y reached the IME");
        if line == "ime: synced" {
            break;
        }
    }
    // IME から要求が届いたら、次のキーから IME を通る道に戻る。
    ime.send_line("answer");
    ime.wait_for_line("ime: answered");
    wait_for_log(&seinas, "the input method answered again");
    tap(&mut seinas, KEY_X);
    assert_eq!(next_ime_key(&ime, "x again"), "ime: key 45 pressed");
    assert_eq!(
        next_key_line(&window, "x from the IME"),
        "window: key 45 pressed"
    );
}

#[test]
fn an_ime_that_answers_is_not_bypassed() {
    // 混んだ CI でも返事が間に合うよう、既定の時間(1,000 ms)で動かす。
    let mut seinas = Compositor::start_with_input("ik");
    let window = open_text_window(&seinas, "窓");
    let _ime = connect_ime_to_window(&seinas);
    assert_eq!(
        next_text_change(&window, "enter"),
        "window: text-input enter"
    );
    // 変換中の文字にしたキー(input-method の要求で答える)と、戻したキー(virtual-keyboard の要求で答える)。
    tap(&mut seinas, KEY_A);
    assert_eq!(next_text_change(&window, "あ"), "window: preedit あ");
    sleep_ms(1500);
    tap(&mut seinas, KEY_X);
    assert_eq!(next_key_line(&window, "x"), "window: key 45 pressed");
    sleep_ms(1500);
    tap(&mut seinas, KEY_I);
    assert_eq!(next_text_change(&window, "あい"), "window: preedit あい");
    assert!(!seinas.log().contains("did not answer"));
}

/// テスト用のウィンドウの色(橙)と、テスト用の IME の候補の窓の色(紫)。
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

/// 候補の窓(40x30)が、左上 (x, y) に出ているか。
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
    // 2 つ目のウィンドウ(外枠は (32, 32))にフォーカスが移ると、IME は deactivate になり、候補の窓は消える。
    // (10, 60) は 2 つ目に隠れない所なので、1 つ目のコンテンツが見える。
    let _second = seinas.connect_test_client(&["window", "--keyboard"]);
    ime.wait_for_line("ime: deactivate");
    seinas.wait_for_screen("the candidate window hidden", |shot| {
        shot.pixel(10, 60) == ORANGE && shot.pixel(49, 89) != PURPLE
    });
    drop(first);
}
