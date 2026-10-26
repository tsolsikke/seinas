//! layer surface の keyboard_interactivity(wlr-layer-shell)を、テスト用のクライアントのキーボードのイベントで確かめる。
//!
//! 決まり(`docs/window-placement.md` の「layer surface のキーボード」):
//! - none(0):キーボードのフォーカスを渡さない。押しても渡さない。
//! - exclusive(1):top と overlay の層では、出ている間、キーを独り占めする(ウィンドウを押しても移らない)。
//!   bottom と background の層では、on_demand と同じに扱う。
//! - on_demand(2):押したときにフォーカスを渡し、ほかを押したら外す。

mod common;

use std::time::Duration;

use common::{Compositor, TestClient};

const KEY_A: u32 = 30;

/// フォーカスが移らないことを確かめるときに待つ時間。
const QUIET: Duration = Duration::from_millis(500);

/// フォーカスを持つウィンドウを出す。
fn window(seinas: &Compositor) -> TestClient {
    let client = seinas.connect_test_client(&["window", "--keyboard"]);
    client.wait_for_line("window: drawn");
    client.wait_for_line("window: keyboard enter");
    client
}

/// 画面の下の端(y 440〜480)に、層 `layer`、keyboard_interactivity `interactivity` の layer surface を出す。
fn layer(seinas: &Compositor, layer: &str, interactivity: &str) -> TestClient {
    let client = seinas.connect_shell_client(&[
        "layer",
        "--layer",
        layer,
        "--anchor",
        "bottom,left,right",
        "--size",
        "0x40",
        "--keyboard-interactivity",
        interactivity,
        "--keyboard",
    ]);
    client.wait_for_line("layer: drawn");
    client
}

fn click(seinas: &mut Compositor, x: i32, y: i32) {
    seinas.pointer(&format!("motion {x} {y}"));
    seinas.pointer("press");
    seinas.pointer("release");
}

/// キー A を押して離し、`to` に届くまで待つ。
fn type_a(seinas: &mut Compositor, to: &TestClient) {
    seinas.pointer(&format!("key {KEY_A} press"));
    seinas.pointer(&format!("key {KEY_A} release"));
    to.wait_for_line(&format!("window: key {KEY_A} pressed"));
}

/// しばらくの間に、`client` へ `line` が届かない。
fn never_gets(client: &TestClient, line: &str) {
    let lines = client.lines_within(QUIET);
    assert!(
        !lines.iter().any(|l| l == line),
        "{line:?} arrived: {lines:?}"
    );
}

#[test]
fn a_layer_without_keyboard_interactivity_never_takes_the_focus() {
    let mut seinas = Compositor::start_with_input("lka");
    let window = window(&seinas);
    let panel = layer(&seinas, "top", "0");
    click(&mut seinas, 300, 460);
    type_a(&mut seinas, &window);
    never_gets(&panel, "window: keyboard enter");
}

#[test]
fn an_exclusive_top_layer_keeps_the_keys_while_it_is_shown() {
    let mut seinas = Compositor::start_with_input("lkb");
    let window = window(&seinas);
    let panel = layer(&seinas, "top", "1");
    // 出たときに、フォーカスを取る。
    panel.wait_for_line("window: keyboard enter");
    window.wait_for_line("window: keyboard leave");
    // ウィンドウを押しても、移らない。
    click(&mut seinas, 100, 100);
    type_a(&mut seinas, &panel);
    never_gets(&window, "window: keyboard enter");
    // 消えたら、ウィンドウへ戻る。
    drop(panel);
    window.wait_for_line("window: keyboard enter");
    type_a(&mut seinas, &window);
}

/// 層 `layer` の、keyboard_interactivity `interactivity` の layer surface は、押したときにフォーカスを取り、ほかを
/// 押したら手放す。
fn takes_the_focus_on_press(name: &str, layer_name: &str, interactivity: &str) {
    let mut seinas = Compositor::start_with_input(name);
    let window = window(&seinas);
    let panel = layer(&seinas, layer_name, interactivity);
    // 出ただけでは、取らない。
    never_gets(&window, "window: keyboard leave");
    // 押すと、取る。
    click(&mut seinas, 300, 460);
    panel.wait_for_line("window: keyboard enter");
    window.wait_for_line("window: keyboard leave");
    type_a(&mut seinas, &panel);
    // ウィンドウを押すと、手放す。
    click(&mut seinas, 100, 100);
    panel.wait_for_line("window: keyboard leave");
    window.wait_for_line("window: keyboard enter");
    type_a(&mut seinas, &window);
}

#[test]
fn an_on_demand_layer_takes_the_focus_on_press() {
    takes_the_focus_on_press("lkc", "top", "2");
}

#[test]
fn an_exclusive_bottom_layer_is_treated_as_on_demand() {
    takes_the_focus_on_press("lkd", "bottom", "1");
}
