//! バグ探しの実験の控え。アサートせず、起きたことを `H…` の行で書き出す。置き方と番号の対応は、
//! `docs/notes/bug-hunt-c2s2.md` の「再現の控え」。
mod common;
use std::{fs, path::Path, time::Duration};
use common::{wait_for, Compositor, TestClient, BAR, HEIGHT};

const TASKBAR: usize = 40;
const TOP: usize = HEIGHT - TASKBAR;
const Q: Duration = Duration::from_millis(600);

fn show(tag: &str, lines: Vec<String>) {
    let keep: Vec<_> = lines.into_iter().filter(|l| !l.contains("pointer motion") && !l.contains("modifiers") && !l.contains("keymap")).collect();
    println!("H {tag}: {keep:?}");
}
fn center(count: usize, index: usize) -> usize { (640 - (count + 1) * 48) / 2 + index * 48 + 24 }
fn taskbar_with(seinas: &Compositor, share: &Path) -> common::Running {
    let dir = seinas.config_dir();
    let log = fs::File::create(dir.join("taskbar.log")).unwrap();
    common::Running(common::child_command(common::workspace_bin("seinas-taskbar", "seinas-taskbar"))
        .args(["--utc-offset", "+00:00", "--fonts"]).arg(common::font_dir())
        .env("WAYLAND_DISPLAY", seinas.shell_socket()).env_remove("SEINAS_FONTS").env_remove("TZ")
        .env("XDG_CONFIG_HOME", dir).env("XDG_DATA_HOME", dir.join("no-data")).env("XDG_DATA_DIRS", share).env("HOME", dir)
        .stderr(log).spawn().unwrap())
}
fn grabs(s: &Compositor, n: usize) { wait_for("grab", || (s.log().matches("a popup took a grab").count() >= n).then_some(())); }
fn click(s: &mut Compositor, x: usize, y: usize) { s.pointer(&format!("motion {x} {y}")); s.pointer("press"); s.pointer("release"); }
fn win(s: &Compositor, args: &[&str]) -> TestClient {
    let c = s.connect_test_client(&[&["window"], args].concat());
    c.wait_for_line("window: drawn"); c
}

#[test]
fn h1_taskbar_press_while_menu_open() {
    let mut s = Compositor::start_with_input("h1");
    let dir = s.config_dir().to_owned();
    fs::create_dir_all(dir.join("share/applications")).unwrap();
    fs::write(dir.join("share/applications/a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=/bin/true\n").unwrap();
    let _t = taskbar_with(&s, &dir.join("share"));
    let w1 = win(&s, &["--keyboard"]);
    let w2 = win(&s, &["--keyboard"]);
    std::thread::sleep(Q);
    click(&mut s, center(2, 0), TOP + 20); grabs(&s, 1);
    std::thread::sleep(Q);
    // タスク 1(1 つ目)を押す。
    click(&mut s, center(2, 1), TOP + 20);
    std::thread::sleep(Q);
    println!("H1a after task press log: {:?}", s.taskbar_log().lines().filter(|l| l.contains("torch menu")).collect::<Vec<_>>());
    show("1a w1", w1.lines_within(Q)); show("1a w2", w2.lines_within(Q));
    // タスクバーの空いた所を押す。
    click(&mut s, 60, TOP + 20);
    std::thread::sleep(Q);
    println!("H1b after empty press log: {:?}", s.taskbar_log().lines().filter(|l| l.contains("torch menu")).collect::<Vec<_>>());
    let n = s.log().matches("popup grab was dismissed").count();
    println!("H1b dismissed by compositor: {n}");
}

#[test]
fn h2_child_env_fds_and_zombie() {
    let mut s = Compositor::start_with_input("h2");
    let dir = s.config_dir().to_owned();
    fs::create_dir_all(dir.join("share/applications")).unwrap();
    let out = dir.join("child.txt");
    fs::write(dir.join("share/applications/a.desktop"), format!("[Desktop Entry]\nType=Application\nName=A\nExec=/bin/sh -c \"ls -l /proc/self/fd > {o}; env >> {o}\"\n", o=out.display())).unwrap();
    let mut t = taskbar_with(&s, &dir.join("share"));
    std::thread::sleep(Q);
    click(&mut s, center(0, 0), TOP + 20); grabs(&s, 1);
    std::thread::sleep(Q);
    s.pointer("key 28 press"); s.pointer("key 28 release");
    std::thread::sleep(Duration::from_secs(2));
    let text = fs::read_to_string(&out).unwrap_or_default();
    println!("H2 child:\n{}", text.lines().filter(|l| l.contains("->") || l.starts_with("WAYLAND") || l.starts_with("XDG_")).collect::<Vec<_>>().join("\n"));
    let pid = t.0.id();
    let ps = std::process::Command::new("ps").args(["--ppid", &pid.to_string(), "-o", "pid,stat,comm"]).output().unwrap();
    println!("H2 children of the taskbar after 2 s idle:\n{}", String::from_utf8_lossy(&ps.stdout));
    let _ = t.is_running();
}

#[test]
fn h4_close_parent_popup_first() {
    let mut s = Compositor::start_with_input("h4");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 100 60 grab"); c.wait_for_line("window: popup 1 drawn 100x60");
    click(&mut s, 60, BAR + 60); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 10 10 50 30 grab child"); c.wait_for_line("window: popup 2 drawn 50x30");
    c.send_line("popup-close 1");
    let l = c.lines_within(Q); show("4 lines", l);
    println!("H4 client running: {}", c.process.is_running());
    println!("H4 compositor running: {}", s.process.is_running());
}

#[test]
fn h5_exclusive_layer_and_popup_grab() {
    let mut s = Compositor::start_with_input("h5");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    let panel = s.connect_shell_client(&["layer", "--layer", "top", "--anchor", "bottom,left,right", "--size", "0x40", "--keyboard-interactivity", "1", "--keyboard"]);
    panel.wait_for_line("layer: drawn");
    panel.wait_for_line("window: keyboard enter");
    c.send_line("popup 20 30 100 60 grab");
    std::thread::sleep(Q);
    s.pointer("key 30 press"); s.pointer("key 30 release");
    show("5 window", c.lines_within(Q)); show("5 panel", panel.lines_within(Q));
}

#[test]
fn h6_swallowed_button_after_leave() {
    let mut s = Compositor::start_with_input("h6");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 100 60 grab"); c.wait_for_line("window: popup 1 drawn 100x60");
    // 外を押し、離さずにポインターが出ていく(入れ子で親のウィンドウの外へ出たときの形)。
    s.pointer("motion 600 300"); s.pointer("press"); s.pointer("leave");
    c.wait_for_line("window: popup 1 done");
    std::thread::sleep(Q);
    // 次のふつうのクリック。
    click(&mut s, 100, 100);
    show("6 window", c.lines_within(Q));
    click(&mut s, 100, 120);
    show("6 window 2nd", c.lines_within(Q));
}

#[test]
fn h7_popup_of_a_window_with_a_margin() {
    let s = Compositor::start_with_input("h7");
    let mut c = win(&s, &["--no-decoration", "--margin", "20", "--size", "200x150", "--pointer"]);
    std::thread::sleep(Q);
    let shot = s.shot();
    let pix = |x, y| shot.pixel(x, y);
    println!("H7 before: (0,0)={:?} (5,5)={:?} (30,30)={:?}", pix(0,0), pix(5,5), pix(30,30));
    c.send_line("popup 0 0 60 40");
    c.wait_for_line("window: popup 1 drawn 60x40");
    std::thread::sleep(Q);
    let shot = s.shot();
    let row: Vec<usize> = (0..120).filter(|&x| shot.pixel(x, 20) == [0x80, 0, 0xff]).collect();
    let col: Vec<usize> = (0..120).filter(|&y| shot.pixel(30, y) == [0x80, 0, 0xff]).collect();
    println!("H7 purple x range at y20: {:?}..{:?}, y range at x30: {:?}..{:?}", row.first(), row.last(), col.first(), col.last());
}

#[test]
fn h8_kill_client_with_grab() {
    let mut s = Compositor::start_with_input("h8");
    let other = win(&s, &["--keyboard", "--pointer", "--size", "200x150"]);
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "200x150"]);
    click(&mut s, 150, 150); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 100 60 grab"); c.wait_for_line("window: popup 1 drawn 100x60");
    drop(c);
    std::thread::sleep(Q);
    click(&mut s, 150, 150);
    s.pointer("key 30 press"); s.pointer("key 30 release");
    show("8 other", other.lines_within(Q));
    println!("H8 compositor running {}", s.process.is_running());
}

#[test]
fn h9_dnd_clients_die() {
    let mut s = Compositor::start_with_input("h9");
    let mut a = win(&s, &["--data-device"]);
    let b = win(&s, &["--accept-drop"]);
    click(&mut s, 10, 100); a.wait_for_line("window: pointer button 0x110 released");
    s.pointer("motion 10 100"); s.pointer("press"); a.wait_for_line("window: pointer button 0x110 pressed");
    a.send_line("drag x"); a.send_line("sync"); a.wait_for_line("window: synced");
    s.pointer("motion 220 200");
    std::thread::sleep(Q);
    show("9 b enter", b.lines_within(Q));
    // ドラッグしている側が死ぬ。
    drop(a);
    std::thread::sleep(Q);
    s.pointer("motion 215 195");
    std::thread::sleep(Q);
    s.pointer("release");
    show("9 b after source died", b.lines_within(Q));
    println!("H9 log: {:?}", s.log().lines().filter(|l| l.contains("drag and drop")).map(|l| l.split_once("INFO").map(|x| x.1.to_owned()).unwrap_or_default()).collect::<Vec<_>>());
    click(&mut s, 220, 200);
    show("9 b click after", b.lines_within(Q));
    // Alt+ドラッグが働くか。
    println!("H9 compositor running {}", s.process.is_running());
}

#[test]
fn h12_selection_owner_dies() {
    let mut s = Compositor::start_with_input("h12");
    let mut a = win(&s, &["--data-device"]);
    let mut b = win(&s, &["--data-device"]);
    click(&mut s, 10, 100); a.wait_for_line("window: pointer button 0x110 released");
    a.send_line("copy hello"); a.send_line("sync"); a.wait_for_line("window: synced");
    click(&mut s, 220, 200); b.wait_for_line("window: pointer button 0x110 released");
    drop(a);
    std::thread::sleep(Q);
    show("12 b after owner died", b.lines_within(Q));
    b.send_line("paste");
    show("12 b paste", b.lines_within(Duration::from_secs(2)));
    println!("H12 b running {} compositor {}", b.process.is_running(), s.process.is_running());
}

#[test]
fn h14_second_grab_without_child() {
    let mut s = Compositor::start_with_input("h14");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 60 40 grab"); c.wait_for_line("window: popup 1 drawn 60x40");
    c.send_line("popup 100 30 60 40 grab");
    let l = c.lines_within(Q); show("14", l);
    println!("H14 client running {}", c.process.is_running());
}

#[test]
fn h15_child_grab_of_a_popup_without_grab() {
    let mut s = Compositor::start_with_input("h15");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 60 40"); c.wait_for_line("window: popup 1 drawn 60x40");
    c.send_line("popup 5 5 30 20 grab child");
    let l = c.lines_within(Q); show("15", l);
    println!("H15 client running {}", c.process.is_running());
}

#[test]
#[ignore = "クリックが対象のウィンドウへ届き、serial が古くならない。試し方の誤り"]
fn h16_stale_serial() {
    let mut s = Compositor::start_with_input("h16");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    let other = win(&s, &["--pointer", "--keyboard", "--size", "100x100"]);
    click(&mut s, 280, 100); c.wait_for_line("window: pointer button 0x110 released");
    // 別のウィンドウの入力を 10 回。c の serial は古くなる。
    for _ in 0..10 { click(&mut s, 50, 100); }
    std::thread::sleep(Q);
    c.send_line("popup 20 30 60 40 grab");
    for l in c.lines_within(Q) { println!("H16 {l}"); }
    println!("H16 client running {}", c.process.is_running());
    drop(other);
}

#[test]
fn h17_child_popup_after_parent_closed() {
    let mut s = Compositor::start_with_input("h17");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 100 60 grab"); c.wait_for_line("window: popup 1 drawn 100x60");
    click(&mut s, 60, BAR + 60); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 10 10 50 30 grab child color=00ff00"); c.wait_for_line("window: popup 2 drawn 50x30");
    std::thread::sleep(Q);
    let at = (21 + 11 + 5, BAR + 31 + 11 + 5);
    println!("H17 before: child pixel {:?}", s.shot().pixel(at.0, at.1));
    c.send_line("popup-close 1");
    std::thread::sleep(Q);
    println!("H17 after closing the parent: child pixel {:?}", s.shot().pixel(at.0, at.1));
    s.pointer("key 30 press"); s.pointer("key 30 release");
    click(&mut s, 600, 450);
    std::thread::sleep(Q);
    show("17 lines", c.lines_within(Q));
    println!("H17 after outside press: child pixel {:?} running {} {}", s.shot().pixel(at.0, at.1), c.process.is_running(), s.process.is_running());
}

#[test]
fn h18_zombie_of_a_child_that_ends_later() {
    let mut s = Compositor::start_with_input("h18");
    let dir = s.config_dir().to_owned();
    fs::create_dir_all(dir.join("share/applications")).unwrap();
    fs::write(dir.join("share/applications/a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=/bin/sleep 1\n").unwrap();
    let t = taskbar_with(&s, &dir.join("share"));
    std::thread::sleep(Q);
    click(&mut s, center(0, 0), TOP + 20); grabs(&s, 1);
    std::thread::sleep(Q);
    s.pointer("key 28 press"); s.pointer("key 28 release");
    std::thread::sleep(Duration::from_secs(3));
    let ps = std::process::Command::new("ps").args(["--ppid", &t.0.id().to_string(), "-o", "pid,stat,comm"]).output().unwrap();
    println!("H18 children 2 s after it ended, nothing happening: {}", String::from_utf8_lossy(&ps.stdout).replace('\n', " | "));
    s.pointer("motion 300 300");
    click(&mut s, center(0, 0), TOP + 20);
    std::thread::sleep(Q);
    let ps = std::process::Command::new("ps").args(["--ppid", &t.0.id().to_string(), "-o", "pid,stat,comm"]).output().unwrap();
    println!("H18 after the next event: {}", String::from_utf8_lossy(&ps.stdout).replace('\n', " | "));
}

#[test]
fn h23_dnd_and_alt_tab() {
    let mut s = Compositor::start_with_input("h23");
    let mut a = win(&s, &["--data-device"]);
    let b = win(&s, &["--accept-drop"]);
    click(&mut s, 10, 100); a.wait_for_line("window: pointer button 0x110 released");
    s.pointer("motion 10 100"); s.pointer("press"); a.wait_for_line("window: pointer button 0x110 pressed");
    a.send_line("drag x"); a.send_line("sync"); a.wait_for_line("window: synced");
    s.pointer("key 56 press"); s.pointer("key 15 press"); s.pointer("key 15 release");
    std::thread::sleep(Q);
    s.pointer("key 56 release");
    std::thread::sleep(Q);
    s.pointer("motion 220 200");
    std::thread::sleep(Q);
    s.pointer("release");
    show("23 a", a.lines_within(Q)); show("23 b", b.lines_within(Q));
    println!("H23 log {:?}", s.log().lines().filter(|l| l.contains("drag and drop")).map(|l| l.split_once("INFO").map(|x| x.1.to_owned()).unwrap_or_default()).collect::<Vec<_>>());
}

#[test]
fn h31_start_drag_after_release() {
    let mut s = Compositor::start_with_input("h31");
    let mut a = win(&s, &["--data-device"]);
    click(&mut s, 100, 100); a.wait_for_line("window: pointer button 0x110 released");
    a.send_line("drag late"); a.send_line("sync");
    show("31", a.lines_within(Q));
    println!("H31 log {:?}", s.log().lines().filter(|l| l.contains("drag and drop")).count());
}

#[test]
fn h19_popup_follows_a_moved_and_a_maximized_window() {
    let mut s = Compositor::start_with_input("h19");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200", "--follow-size"]);
    c.send_line("popup 250 150 120 80 adjust=12"); c.wait_for_line("window: popup 1 drawn 120x80");
    std::thread::sleep(Q);
    let find = |s: &Compositor| { let shot = s.shot(); let mut xs = vec![]; for y in (0..480).step_by(4) { for x in (0..640).step_by(4) { if shot.pixel(x, y) == [0x80,0,0xff] { xs.push((x,y)); } } } (xs.first().copied(), xs.last().copied()) };
    println!("H19 at first: {:?}", find(&s));
    // タイトルバーを動かして、画面の右下の外へはみ出させる。
    s.pointer("motion 100 10"); s.pointer("press");
    for i in 1..=10 { s.pointer(&format!("motion {} {}", 100 + i * 40, 10 + i * 35)); }
    s.pointer("release");
    std::thread::sleep(Q);
    println!("H19 moved: {:?}", find(&s));
    c.send_line("maximize");
    std::thread::sleep(Q);
    println!("H19 maximized: {:?}", find(&s));
    show("19", c.lines_within(Q));
}

/// コンポジタが答えるか(別のクライアントの sync が 3 秒で返るか)。
fn alive(probe: &mut TestClient) -> bool {
    probe.send_line("sync");
    probe.lines_within(Duration::from_secs(3)).iter().any(|l| l == "window: synced")
}

#[test]
fn h40_popup_with_itself_as_the_parent() {
    let mut s = Compositor::start_with_input("h40");
    let mut probe = win(&s, &[]);
    let mut c = win(&s, &["--size", "300x200"]);
    println!("H40 before: answers {}", alive(&mut probe));
    c.send_line("popup 20 30 60 40 color=030201");
    std::thread::sleep(Duration::from_secs(2));
    println!("H40 after: answers {} running {} client {}", alive(&mut probe), s.process.is_running(), c.process.is_running());
    println!("H40 log tail {:?}", s.log().lines().rev().take(2).collect::<Vec<_>>());
}

#[test]
#[ignore = "コンポジタが落ちると、手順の側が send_line で失敗する。h50 と h57 が代わり"]
fn h41_huge_window_geometry() {
    for (name, w, h) in [("h41a", 100000000, 10), ("h41b", 30000, 30000)] {
        let mut s = Compositor::start_with_input(name);
        let mut probe = win(&s, &[]);
        let mut c = win(&s, &["--size", "300x200"]);
        c.send_line(&format!("geometry 0 0 {w} {h}"));
        std::thread::sleep(Duration::from_secs(3));
        println!("H41 {w}x{h}: answers {} running {}", alive(&mut probe), s.process.is_running());
        println!("H41 log {:?}", s.log().lines().filter(|l| l.contains("panicked") || l.contains("overflow") || l.contains("alloc")).take(3).collect::<Vec<_>>());
    }
}

#[test]
fn h42_grab_during_a_title_bar_drag() {
    let mut s = Compositor::start_with_input("h42");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    s.pointer("key 30 press"); s.pointer("key 30 release");
    c.wait_for_line("window: key 30 pressed");
    s.pointer("motion 100 10"); s.pointer("press"); s.pointer("motion 120 30");
    std::thread::sleep(Q);
    c.send_line("popup 20 30 60 40 grab");
    c.wait_for_line("window: popup 1 drawn 60x40");
    s.pointer("release");
    std::thread::sleep(Q);
    s.pointer("motion 300 200");
    std::thread::sleep(Q);
    let shot = s.shot();
    let top = (0..480).find(|&y| shot.pixel(150, y) == [0xff, 0x80, 0]);
    let left = (0..640).find(|&x| shot.pixel(x, 150) == [0xff, 0x80, 0]);
    println!("H42 after the release, pointer moved far: left {left:?} top {top:?} (20 / 56 なら止まっている)");
}

#[test]
fn h43_on_demand_layer_and_alt_tab() {
    let mut s = Compositor::start_with_input("h43");
    let a = win(&s, &["--keyboard"]);
    let b = win(&s, &["--keyboard"]);
    let panel = s.connect_shell_client(&["layer", "--layer", "top", "--anchor", "bottom,left,right", "--size", "0x40", "--keyboard-interactivity", "2", "--keyboard"]);
    panel.wait_for_line("layer: drawn");
    click(&mut s, 300, 460);
    panel.wait_for_line("window: keyboard enter");
    s.pointer("key 56 press"); s.pointer("key 15 press"); s.pointer("key 15 release"); s.pointer("key 56 release");
    std::thread::sleep(Q);
    s.pointer("key 30 press"); s.pointer("key 30 release");
    show("43 a", a.lines_within(Q)); show("43 b", b.lines_within(Q)); show("43 panel", panel.lines_within(Q));
}

#[test]
fn h44_exclusive_layer_unmapped() {
    let mut s = Compositor::start_with_input("h44");
    let w = win(&s, &["--keyboard"]);
    w.wait_for_line("window: keyboard enter");
    let mut panel = s.connect_shell_client(&["layer", "--layer", "top", "--anchor", "bottom,left,right", "--size", "0x40", "--keyboard-interactivity", "1", "--keyboard"]);
    panel.wait_for_line("layer: drawn");
    panel.wait_for_line("window: keyboard enter");
    panel.send_line("unmap");
    std::thread::sleep(Q);
    s.pointer("key 30 press"); s.pointer("key 30 release");
    show("44 window", w.lines_within(Q)); show("44 panel", panel.lines_within(Q));
}

#[test]
fn h45_same_mode_twice() {
    let s = Compositor::start_with_input("h45");
    let mut c = win(&s, &["--decoration"]);
    c.send_line("decoration server-side");
    show("45 first", c.lines_within(Q));
    c.send_line("decoration server-side");
    show("45 second", c.lines_within(Q));
    drop(s);
}

#[test]
fn h46_press_drag_release_into_a_menu() {
    let mut s = Compositor::start_with_input("h46");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    s.pointer("motion 100 100"); s.pointer("press right");
    c.wait_for_line("window: pointer button 0x111 pressed");
    c.send_line("popup 100 64 100 60 grab");
    c.wait_for_line("window: popup 1 drawn 100x60");
    s.pointer("motion 150 130");
    std::thread::sleep(Q);
    s.pointer("release right");
    show("46", c.lines_within(Q));
}

#[test]
fn h47_second_start_drag() {
    let mut s = Compositor::start_with_input("h47");
    let mut a = win(&s, &["--data-device"]);
    s.pointer("motion 100 100"); s.pointer("press"); a.wait_for_line("window: pointer button 0x110 pressed");
    a.send_line("drag one"); a.send_line("drag two"); a.send_line("sync"); a.wait_for_line("window: synced");
    println!("H47 log {:?}", s.log().lines().filter(|l| l.contains("drag and drop")).map(|l| l.split_once("INFO").map(|x| x.1.to_owned()).unwrap_or_default()).collect::<Vec<_>>());
    show("47 a", a.lines_within(Q));
    s.pointer("key 56 press"); s.pointer("press right"); s.pointer("motion 150 150"); s.pointer("release right"); s.pointer("key 56 release"); s.pointer("release");
    println!("H47 running {}", s.process.is_running());
}

#[test]
fn h48_app_id_pointing_at_a_fifo() {
    let mut s = Compositor::start_with_input("h48");
    let dir = s.config_dir().to_owned();
    let fifo = dir.join("evil.desktop");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let mut probe = win(&s, &[]);
    println!("H48 before: answers {}", alive(&mut probe));
    let id = dir.join("evil");
    let c = s.connect_test_client(&["window", "--app-id", id.to_str().unwrap()]);
    std::thread::sleep(Duration::from_secs(2));
    println!("H48 after: answers {} running {}", alive(&mut probe), s.process.is_running());
    drop(c);
    // 塞がっていたら、書き手を開いて解く。
    let _ = fs::OpenOptions::new().write(true).custom_flags_nonblock().open(&fifo);
}
trait Nb { fn custom_flags_nonblock(&mut self) -> &mut Self; }
impl Nb for fs::OpenOptions { fn custom_flags_nonblock(&mut self) -> &mut Self { use std::os::unix::fs::OpenOptionsExt; self.custom_flags(libc::O_NONBLOCK) } }

#[test]
fn h49_menu_on_a_very_low_screen() {
    let mut s = Compositor::start_sized_with_input("h49", &common::font_dir(), 400, 60);
    let dir = s.config_dir().to_owned();
    fs::create_dir_all(dir.join("share/applications")).unwrap();
    fs::write(dir.join("share/applications/a.desktop"), "[Desktop Entry]\nType=Application\nName=A\nExec=/bin/true\n").unwrap();
    let mut t = taskbar_with(&s, &dir.join("share"));
    std::thread::sleep(Duration::from_secs(1));
    click(&mut s, 200, 40);
    std::thread::sleep(Duration::from_secs(1));
    println!("H49 taskbar running {}; log {:?}", t.is_running(), s.taskbar_log().lines().rev().take(3).collect::<Vec<_>>());
}

fn proc_state(s: &Compositor) -> String {
    let pid = s.process.0.id().to_string();
    let out = std::process::Command::new("ps").args(["-o", "pcpu=,stat=,wchan:20=,rss=", "-p", &pid]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn geometry(name: &str, w: i32, h: i32) {
    let mut s = Compositor::start_with_input(name);
    let mut c = win(&s, &["--size", "300x200"]);
    std::thread::sleep(Q);
    println!("H50 {w}x{h} before: {}", proc_state(&s));
    c.send_line(&format!("geometry 0 0 {w} {h}"));
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if !s.process.is_running() { break; }
    }
    println!("H50 {w}x{h} after: running {} state [{}]", s.process.is_running(), proc_state(&s));
    let log = s.log();
    let lines: Vec<&str> = log.lines().collect();
    let at = lines.iter().position(|l| l.contains("panicked") || l.contains("memory allocation"));
    println!("H50 {w}x{h} reason {:?}", at.map(|i| lines[i..(i + 2).min(lines.len())].join(" / ")));
    println!("H50 {w}x{h} frames {:?}", lines.iter().filter(|l| l.contains("./crates/seinas") || l.contains("crates/seinas-")).take(4).map(|l| l.trim()).collect::<Vec<_>>());
}
#[test] fn h50a_geometry_wide() { geometry("h50a", 100000000, 10); }
#[test] fn h50b_geometry_square() { geometry("h50b", 30000, 30000); }
#[test] fn h50c_geometry_large() { geometry("h50c", 8000, 8000); }
#[test] fn h50d_geometry_max() { geometry("h50d", i32::MAX, 10); }

#[test]
fn h51_set_mode_control() {
    let s = Compositor::start_with_input("h51");
    let mut c = win(&s, &["--decoration"]);
    std::thread::sleep(Q);
    println!();
    show("51 start", c.lines_within(Q));
    for m in ["client-side", "client-side", "server-side", "server-side"] {
        c.send_line(&format!("decoration {m}"));
        show(&format!("51 {m}"), c.lines_within(Q));
    }
    drop(s);
}

#[test]
fn h52_press_drag_release_control() {
    let mut s = Compositor::start_with_input("h52");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 100 64 100 60 grab");
    c.wait_for_line("window: popup 1 drawn 100x60");
    show("52 opened", c.lines_within(Q));
    s.pointer("motion 150 130"); std::thread::sleep(Q);
    show("52 moved in, nothing held", c.lines_within(Q));
    s.pointer("press"); s.pointer("release");
    show("52 clicked", c.lines_within(Q));
}

/// `HUNT_KEEP_POPUP=1` で走らせる。無いと、クライアントが popup_done の後に親を消すので、Unknown id で切れるだけになる。
#[test]
fn h53_child_grab_after_the_parent_was_dismissed() {
    let mut s = Compositor::start_with_input("h53");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "300x200"]);
    click(&mut s, 100, 100); c.wait_for_line("window: pointer button 0x110 released");
    c.send_line("popup 20 30 100 60 grab"); c.wait_for_line("window: popup 1 drawn 100x60");
    click(&mut s, 60, BAR + 60); c.wait_for_line("window: pointer button 0x110 released");
    click(&mut s, 600, 450);
    c.wait_for_line("window: popup 1 done");
    c.send_line("popup 10 10 50 30 grab child");
    std::thread::sleep(Q);
    show("53", c.lines_within(Q));
    println!("H53 client running {} status {:?}", c.process.is_running(), c.process.0.try_wait());
    println!("H53 log {:?}", s.log().lines().rev().take(3).map(|l| l.chars().take(420).collect::<String>()).collect::<Vec<_>>());
}

#[test]
fn h54_where_the_compositor_stops() {
    let mut s = Compositor::start_with_input("h54a");
    let mut c = win(&s, &["--size", "300x200"]);
    c.send_line("popup 20 30 60 40 color=030201");
    std::thread::sleep(Duration::from_secs(2));
    println!("\nH54 self parent: {}", proc_state(&s));
    let mut s = Compositor::start_with_input("h54b");
    let dir = s.config_dir().to_owned();
    let fifo = dir.join("evil.desktop");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let id = dir.join("evil");
    let c2 = s.connect_test_client(&["window", "--app-id", id.to_str().unwrap()]);
    std::thread::sleep(Duration::from_secs(2));
    println!("H54 fifo: {}", proc_state(&s));
    drop(c2);
    let _ = fs::OpenOptions::new().write(true).custom_flags_nonblock().open(&fifo);
}

#[test]
fn h55_exclusive_layer_really_unmapped() {
    let mut s = Compositor::start_with_input("h55");
    let w = win(&s, &["--keyboard"]);
    w.wait_for_line("window: keyboard enter");
    let mut panel = s.connect_shell_client(&["layer", "--layer", "top", "--anchor", "bottom,left,right", "--size", "0x40", "--keyboard-interactivity", "1", "--keyboard"]);
    panel.wait_for_line("layer: drawn");
    panel.wait_for_line("window: keyboard enter");
    std::thread::sleep(Q);
    println!("\nH55 before {:?}", s.shot().pixel(300, 460));
    panel.send_line("unmap");
    panel.wait_for_line("layer: unmapped");
    std::thread::sleep(Q);
    println!("H55 after {:?}", s.shot().pixel(300, 460));
    s.pointer("key 30 press"); s.pointer("key 30 release");
    show("55 window", w.lines_within(Q)); show("55 panel", panel.lines_within(Q));
}

#[test]
#[ignore = "判定できなかった。最初のクリックの待ちが時間切れになる"]
fn h56_really_stale_serial() {
    let mut s = Compositor::start_with_input("h56");
    let mut c = win(&s, &["--pointer", "--keyboard", "--size", "120x100"]);
    let other = win(&s, &["--pointer", "--keyboard", "--size", "120x100"]);
    click(&mut s, 60, 80); c.wait_for_line("window: pointer button 0x110 released");
    let mut at = None;
    for (x, y) in [(200, 80), (300, 80), (400, 80), (200, 250), (300, 250), (400, 300), (560, 400)] {
        click(&mut s, x, y);
        if other.lines_within(Q).iter().any(|l| l.contains("0x110 pressed")) { at = Some((x, y)); break; }
    }
    println!("\nH56 other at {at:?}");
    let (x, y) = at.expect("found the other window");
    for _ in 0..10 { click(&mut s, x, y); }
    std::thread::sleep(Q);
    let _ = c.lines_within(Q);
    c.send_line("popup 20 30 60 40 grab");
    show("56", c.lines_within(Q));
    println!("H56 grabs held by screen: client running {}", c.process.is_running());
}

#[test] fn h57a_wraps_negative() { geometry("h57a", 100000000, 10); }
#[test] fn h57b_wraps_small() { geometry("h57b", 120000000, 10); }
#[test] fn h57c_million() { geometry("h57c", 1000000, 10); }
#[test] fn h57d_max() { geometry("h57d", i32::MAX, 10); }

#[test]
fn h58_set_mode_with_sync() {
    let s = Compositor::start_with_input("h58");
    let mut c = win(&s, &["--decoration"]);
    std::thread::sleep(Q);
    let _ = c.lines_within(Q);
    for m in ["server-side", "client-side"] {
        c.send_line(&format!("decoration {m}"));
        c.send_line("sync");
        println!("\nH58 asked {m}: {:?}", c.lines_within(Q));
    }
    let mut d = win(&s, &["--decoration", "--no-decoration"]);
    let _ = d.lines_within(Q);
    d.send_line("decoration server-side");
    d.send_line("sync");
    println!("H58 control, first ask: {:?}", d.lines_within(Q));
}
