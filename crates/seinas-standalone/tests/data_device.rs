//! クリップボード(wl_data_device の selection)と、クライアントどうしのドラッグ&ドロップを、テスト用のクライアントの
//! 出力で確かめる。
//!
//! 決まり(`docs/window-placement.md` の「クリップボードとドラッグ&ドロップ」):
//! - selection は、キーボードのフォーカスのあるクライアントにだけ見せる(フォーカスが移るたびに見せ直す)。フォーカスの
//!   無いクライアントは、selection を受け取れず、置くこともできない。
//! - ドラッグ&ドロップは、クライアントへ渡した押下を押している間にだけ始められる。最中のポインターのイベントは、
//!   Seinas の操作(タイトルバー、修飾キーとドラッグ)にも、ポップアップの grab にも使わない(ポップアップから始めた
//!   ドラッグも、ほかのクライアントへ落とせる。連なりは閉じない)。
//!
//! テスト用のクライアント(`test-client window --data-device`)は、標準入力の `copy TEXT`・`paste`・`drag TEXT` で
//! 頼み、`window: data …` の行で、届いたものを書く。

mod common;

use std::time::Duration;

use common::{Compositor, TestClient, BAR};

const TEXT: &str = "text/plain;charset=utf-8";
const KEY_A: u32 = 30;
const KEY_LEFT_ALT: u32 = 56;
/// 届かないことを確かめるときに待つ時間。
const QUIET: Duration = Duration::from_millis(500);

/// 1 つ目(コンテンツは (0, BAR)〜(200, BAR + 150))と 2 つ目((32, 32 + BAR)〜(232, 32 + BAR + 150))のウィンドウの、
/// ほかのウィンドウに覆われない所。
const A_AT: (i32, i32) = (10, 100);
const B_AT: (i32, i32) = (220, 200);

/// data device を使うウィンドウを出し、描かれてフォーカスが来るまで待つ。
fn window(seinas: &Compositor, args: &[&str]) -> TestClient {
    let client = seinas.connect_test_client(&[&["window", "--data-device"], args].concat());
    client.wait_for_line("window: drawn");
    client.wait_for_line("window: keyboard enter");
    client
}

/// `line` が届くまで読み、それまでに届いた行(`line` を含む)を返す。
fn read_until(client: &TestClient, line: &str) -> Vec<String> {
    let mut lines = Vec::new();
    while lines.last().map(String::as_str) != Some(line) {
        lines.push(client.next_line(line));
    }
    lines
}

/// (`x`, `y`)を押して離し、`client` に届くまで待つ(フォーカスを移し、押下の serial を作る)。それまでに届いた行を返す。
fn click(seinas: &mut Compositor, client: &TestClient, (x, y): (i32, i32)) -> Vec<String> {
    seinas.pointer(&format!("motion {x} {y}"));
    seinas.pointer("press");
    seinas.pointer("release");
    read_until(client, "window: pointer button 0x110 released")
}

/// 標準入力の行 `line` を送り、クライアントの頼みをコンポジタが処理し終えるまで待つ。それまでに届いた行を返す。
fn request(client: &mut TestClient, line: &str) -> Vec<String> {
    client.send_line(line);
    client.send_line("sync");
    read_until(client, "window: synced")
}

/// `lines` に `line` があるか。
fn has(lines: &[String], line: &str) -> bool {
    lines.iter().any(|l| l == line)
}

/// しばらくの間に、`client` へ `prefix` で始まる行が届かない。
fn never_gets(client: &TestClient, prefix: &str) {
    let lines = client.lines_within(QUIET);
    assert!(
        !lines.iter().any(|l| l.starts_with(prefix)),
        "{prefix:?} arrived: {lines:?}"
    );
}

/// (`at`)で押したまま、`source` にドラッグ&ドロップを始めさせる。
fn start_drag(seinas: &mut Compositor, source: &mut TestClient, at: (i32, i32), text: &str) {
    seinas.pointer(&format!("motion {} {}", at.0, at.1));
    seinas.pointer("press");
    source.wait_for_line("window: pointer button 0x110 pressed");
    request(source, &format!("drag {text}"));
}

#[test]
fn a_copy_reaches_the_focused_client() {
    let mut seinas = Compositor::start_with_input("dda");
    let mut a = window(&seinas, &[]);
    let mut b = window(&seinas, &[]);
    let selection = format!("window: data selection {TEXT}");
    // A を押してフォーカスを移すと、今の selection(無し)が A に届く。コピーすると、A に届く。
    let lines = click(&mut seinas, &a, A_AT);
    assert!(has(&lines, "window: data selection none"), "{lines:?}");
    let lines = request(&mut a, "copy hello");
    assert!(has(&lines, &selection), "{lines:?}");
    // フォーカスの無い B には、届かない。B からのコピーは、断られる(selection は変わらない)。
    never_gets(&b, "window: data selection");
    request(&mut b, "copy other");
    never_gets(&a, "window: data selection");
    // B を押してフォーカスを移すと、B に届き、貼り付けると A の中身が届く。
    let lines = click(&mut seinas, &b, B_AT);
    assert!(has(&lines, &selection), "{lines:?}");
    b.send_line("paste");
    a.wait_for_line(&format!("window: data send {TEXT}"));
    b.wait_for_line("window: data pasted hello");
    // キーの serial でのコピーも働く(B が置き直し、A に移ると A に届く)。
    seinas.pointer(&format!("key {KEY_A} press"));
    seinas.pointer(&format!("key {KEY_A} release"));
    b.wait_for_line(&format!("window: key {KEY_A} pressed"));
    request(&mut b, "copy from b");
    let lines = click(&mut seinas, &a, A_AT);
    assert!(has(&lines, &selection), "{lines:?}");
    a.send_line("paste");
    a.wait_for_line("window: data pasted from b");
}

#[test]
fn a_drag_enters_and_drops_on_another_client() {
    let mut seinas = Compositor::start_with_input("ddb");
    let mut a = window(&seinas, &[]);
    let b = window(&seinas, &["--accept-drop"]);
    click(&mut seinas, &a, A_AT);
    start_drag(&mut seinas, &mut a, A_AT, "dragged");
    // B の上へ動かすと、B に enter が届き、B が受け入れる。
    seinas.pointer(&format!("motion {} {}", B_AT.0, B_AT.1));
    b.wait_for_line(&format!(
        "window: data dnd enter {} {}",
        B_AT.0 - 32,
        B_AT.1 - 32 - BAR as i32
    ));
    // B が受け入れた動作(copy)が、A に届く。Smithay 0.7.0 は、受け入れた型(wl_data_source.target)を送らない。
    a.wait_for_line("window: data action 1");
    // 離すと、B に落とされ、中身が届く。
    seinas.pointer("release");
    b.wait_for_line("window: data dnd drop");
    b.wait_for_line("window: data dropped dragged");
    a.wait_for_line("window: data drop performed");
    a.wait_for_line("window: data finished");
    // 終わった後は、ふだんどおり押下がクライアントへ届く。
    click(&mut seinas, &b, B_AT);
}

#[test]
fn a_drag_dropped_on_nothing_or_left_is_cancelled() {
    let mut seinas = Compositor::start_with_input("ddc");
    let mut a = window(&seinas, &[]);
    let b = window(&seinas, &["--accept-drop"]);
    click(&mut seinas, &a, A_AT);
    start_drag(&mut seinas, &mut a, A_AT, "dragged");
    // B に入ってから出ると、leave が届く。何も無い所で離すと、取り消される。
    seinas.pointer(&format!("motion {} {}", B_AT.0, B_AT.1));
    b.wait_for_line(&format!(
        "window: data dnd enter {} {}",
        B_AT.0 - 32,
        B_AT.1 - 32 - BAR as i32
    ));
    seinas.pointer("motion 600 450");
    b.wait_for_line("window: data dnd leave");
    seinas.pointer("release");
    a.wait_for_line("window: data cancelled");
    never_gets(&b, "window: data dnd drop");
    // タイトルバー(Seinas のもの)の上で離しても、取り消される。ウィンドウは動かない。
    click(&mut seinas, &a, A_AT);
    start_drag(&mut seinas, &mut a, A_AT, "again");
    seinas.pointer("motion 100 10");
    seinas.pointer("motion 300 300");
    seinas.pointer("motion 100 10");
    seinas.pointer("release");
    a.wait_for_line("window: data cancelled");
    click(&mut seinas, &a, A_AT);
}

#[test]
fn seinas_operations_do_not_interfere_with_a_drag() {
    let mut seinas = Compositor::start_with_input("ddd");
    let mut a = window(&seinas, &[]);
    let b = window(&seinas, &["--accept-drop", "--follow-size"]);
    // ポップアップの grab の最中に、ポップアップの中の押下から始める。連なりは閉じない。
    click(&mut seinas, &a, A_AT);
    a.send_line("popup 5 30 100 60 grab");
    a.wait_for_line("window: popup 1 drawn 100x60");
    let inside = (20, BAR as i32 + 50);
    seinas.pointer(&format!("motion {} {}", inside.0, inside.1));
    seinas.pointer("press");
    a.wait_for_line("window: pointer button 0x110 pressed");
    let lines = request(&mut a, "drag from popup");
    assert!(!has(&lines, "window: popup 1 done"), "{lines:?}");
    // 最中に、修飾キーと右のボタンで B を押しても、Seinas のサイズ変更は始まらない。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    seinas.pointer(&format!("motion {} {}", B_AT.0, B_AT.1));
    b.wait_for_line(&format!(
        "window: data dnd enter {} {}",
        B_AT.0 - 32,
        B_AT.1 - 32 - BAR as i32
    ));
    seinas.pointer("press right");
    seinas.pointer(&format!("motion {} {}", B_AT.0 + 5, B_AT.1 + 5));
    seinas.pointer("release right");
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    never_gets(&b, "window: resized");
    // 左のボタンを離すと、B に落ちる。ポップアップの連なりは、外の押下で、今までどおり閉じる。
    seinas.pointer("release");
    b.wait_for_line("window: data dropped from popup");
    seinas.pointer("motion 600 450");
    seinas.pointer("press");
    seinas.pointer("release");
    a.wait_for_line("window: popup 1 done");
    // 終わった後は、修飾キーとドラッグがふつうに働く(B が右下へ動き、大きさは変わらない)。
    seinas.pointer(&format!("key {KEY_LEFT_ALT} press"));
    seinas.pointer(&format!("motion {} {}", B_AT.0, B_AT.1));
    seinas.pointer("press");
    seinas.pointer(&format!("motion {} {}", B_AT.0 + 40, B_AT.1 + 40));
    seinas.pointer("release");
    seinas.pointer(&format!("key {KEY_LEFT_ALT} release"));
    // B のコンテンツは (72, 108 + BAR)〜(272, 258 + BAR) になる。前にあった所の左下の角の近くは、背景になる。
    seinas.wait_for_screen("B moved by Alt and drag", |shot| {
        shot.pixel(265, 32 + 40 + BAR + 145) == [0xff, 0x80, 0x00]
            && shot.pixel(35, 32 + BAR + 145) != [0xff, 0x80, 0x00]
    });
}

#[test]
fn a_drag_works_while_an_input_method_holds_the_keyboard() {
    let mut seinas = Compositor::start_with_input("dde");
    let mut a = window(&seinas, &[]);
    let b = window(&seinas, &["--accept-drop"]);
    let ime = seinas.connect_ime_client(&["ime"]);
    ime.wait_for_line("ime: ready");
    click(&mut seinas, &a, A_AT);
    start_drag(&mut seinas, &mut a, A_AT, "with ime");
    seinas.pointer(&format!("motion {} {}", B_AT.0, B_AT.1));
    // B が受け入れるまで待ってから離す(受け入れる前に離すと、取り消しになる)。
    a.wait_for_line("window: data action 1");
    seinas.pointer("release");
    b.wait_for_line("window: data dropped with ime");
    // キーは、今までどおり IME に届く。
    seinas.pointer(&format!("key {KEY_A} press"));
    seinas.pointer(&format!("key {KEY_A} release"));
    ime.wait_for_line(&format!("ime: key {KEY_A} pressed"));
}
