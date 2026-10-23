//! ソケットの場所の決まり(`docs/standalone.md` の `--socket`・`--shell-socket`・`--ime-socket`)を確かめる。
//!
//! - シェルの部品と IME のためのソケットは、指定が無ければ、ふだんのソケットのパスの名前に `-shell`・`-ime` を
//!   入れたもの。名前に拡張子があれば、その前に入れる(ロックファイルは、拡張子を `.lock` に替えた名前になるので、
//!   終わりに付けると取り合う)。
//! - 指定したパスどうしでロックファイルがぶつかるときは、分かりやすいエラーで終わる。
//! - `--no-ime-socket` を付けると、IME のためのソケットとロックファイルを作らない(ほかの 2 つは作る)。

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use common::{
    child_command, font_dir, sweep_stale_dirs, wait_for, workspace_bin, Running, DIR_MARK,
};

/// 作業用の場所(印のファイルつき)。テストが落ちても、Drop で消す。
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn work_dir(name: &str) -> WorkDir {
    sweep_stale_dirs();
    let dir = std::env::temp_dir().join(format!("seinas-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the working directory");
    fs::write(dir.join(DIR_MARK), "").expect("mark the working directory");
    WorkDir(dir)
}

/// seinas-standalone を、fake screen と引数 `args` で起こす(標準エラーは `dir/log`)。
fn start(dir: &Path, args: &[&str]) -> Running {
    start_with_env(dir, args, &[])
}

/// [`start`] と同じだが、環境変数 `env` も付ける。
fn start_with_env(dir: &Path, args: &[&str], env: &[(&str, &Path)]) -> Running {
    let log = fs::File::create(dir.join("log")).expect("create the log file");
    Running(
        child_command(env!("CARGO_BIN_EXE_seinas-standalone"))
            // テスト用の入力インターフェースを開く(seat にキーボードを足し、IME の global を作るため)。
            .args(["--fake", "320x240", "--test-input", "--fonts"])
            .arg(font_dir())
            .args(args)
            .env_remove("SEINAS_FONTS")
            .env("XDG_CONFIG_HOME", dir)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stderr(log)
            .spawn()
            .expect("start seinas-standalone"),
    )
}

/// ソケット `socket` の global の名前(`test-client globals`)。
fn globals(socket: &Path) -> Vec<String> {
    let output = child_command(workspace_bin("seinas-test-clients", "test-client"))
        .arg("globals")
        .env("WAYLAND_DISPLAY", socket)
        .stderr(Stdio::null())
        .output()
        .expect("run the test client");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("globals: "))
        .filter(|name| *name != "done")
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_socket_path_with_a_dot_gets_three_sockets_and_three_lock_files() {
    let work = work_dir("ka");
    let dir = &work.0;
    let mut seinas = start(dir, &["--socket", &dir.join("s.sock").to_string_lossy()]);
    let names = [
        "s.sock",
        "s-shell.sock",
        "s-ime.sock",
        "s.lock",
        "s-shell.lock",
        "s-ime.lock",
    ];
    wait_for("the three sockets", || {
        names
            .iter()
            .all(|name| dir.join(name).exists())
            .then_some(())
    });
    assert!(
        seinas.is_running(),
        "{}",
        fs::read_to_string(dir.join("log")).unwrap_or_default()
    );
    // それぞれのソケットに、決まった global がある。
    assert!(globals(&dir.join("s.sock"))
        .iter()
        .any(|g| g == "xdg_wm_base"));
    assert!(globals(&dir.join("s-shell.sock"))
        .iter()
        .any(|g| g == "zwlr_layer_shell_v1"));
    let on_ime = globals(&dir.join("s-ime.sock"));
    assert!(
        on_ime.iter().any(|g| g == "zwp_input_method_manager_v2"),
        "{on_ime:?}"
    );
    drop(seinas);
}

#[test]
fn socket_paths_that_would_share_a_lock_file_are_refused() {
    let work = work_dir("kb");
    let dir = &work.0;
    let socket = dir.join("a.sock");
    let shell = dir.join("a.other");
    let mut seinas = start(
        dir,
        &[
            "--socket",
            &socket.to_string_lossy(),
            "--shell-socket",
            &shell.to_string_lossy(),
        ],
    );
    let status = seinas.0.wait().expect("seinas-standalone ends");
    assert_eq!(status.code(), Some(1));
    let log = fs::read_to_string(dir.join("log")).unwrap_or_default();
    assert!(log.contains("would share the lock file"), "{log}");
    assert!(!socket.exists() && !shell.exists());
}

#[test]
fn a_given_path_that_would_share_the_lock_file_of_a_default_socket_is_refused() {
    // --socket が無いと、ふだんのソケットは XDG_RUNTIME_DIR の下の seinas-0(ロックファイルは seinas-0.lock)。
    // --shell-socket に seinas-0.x を選ぶと、同じロックファイルになる。
    let work = work_dir("kc");
    let dir = &work.0;
    let shell = dir.join("seinas-0.x");
    let mut seinas = start_with_env(
        dir,
        &["--shell-socket", &shell.to_string_lossy()],
        &[("XDG_RUNTIME_DIR", dir)],
    );
    let status = seinas.0.wait().expect("seinas-standalone ends");
    assert_eq!(status.code(), Some(1));
    let log = fs::read_to_string(dir.join("log")).unwrap_or_default();
    assert!(log.contains("would share the lock file"), "{log}");
    assert!(log.contains("seinas-0.lock"), "{log}");
    assert!(!dir.join("seinas-0").exists() && !shell.exists());
}

/// IME の global(どのソケットにも出てはいけない)。
const IME_GLOBALS: [&str; 2] = [
    "zwp_input_method_manager_v2",
    "zwp_virtual_keyboard_manager_v1",
];

/// ログに `line` が出るまで待つ。
fn wait_for_log(dir: &Path, line: &str) {
    wait_for(line, || {
        fs::read_to_string(dir.join("log"))
            .unwrap_or_default()
            .contains(line)
            .then_some(())
    });
}

/// IME のソケットを開かないと決めたときにログへ出る行(この行の後には、もうソケットを作らない)。
const NO_IME_LINE: &str = "the socket for the input method is not opened";

#[test]
fn no_ime_socket_opens_only_the_other_two_sockets_and_lock_files() {
    let work = work_dir("kd");
    let dir = &work.0;
    let mut seinas = start(
        dir,
        &[
            "--socket",
            &dir.join("s.sock").to_string_lossy(),
            "--no-ime-socket",
        ],
    );
    let names = ["s.sock", "s-shell.sock", "s.lock", "s-shell.lock"];
    wait_for("the two sockets", || {
        names
            .iter()
            .all(|name| dir.join(name).exists())
            .then_some(())
    });
    wait_for_log(dir, NO_IME_LINE);
    assert!(
        seinas.is_running(),
        "{}",
        fs::read_to_string(dir.join("log")).unwrap_or_default()
    );
    for name in ["s-ime.sock", "s-ime.lock"] {
        assert!(!dir.join(name).exists(), "{name}");
    }
    // ふだんのソケットとシェルの部品のソケットは、いつもどおり使え、IME の global はどちらにも無い。
    let on_app = globals(&dir.join("s.sock"));
    assert!(on_app.iter().any(|g| g == "xdg_wm_base"), "{on_app:?}");
    let on_shell = globals(&dir.join("s-shell.sock"));
    assert!(
        on_shell.iter().any(|g| g == "zwlr_layer_shell_v1"),
        "{on_shell:?}"
    );
    for global in IME_GLOBALS {
        assert!(!on_app.iter().any(|g| g == global), "{global}: {on_app:?}");
        assert!(
            !on_shell.iter().any(|g| g == global),
            "{global}: {on_shell:?}"
        );
    }
    drop(seinas);
}

#[test]
fn no_ime_socket_does_not_open_the_default_ime_socket() {
    // --socket が無いと、ソケットは XDG_RUNTIME_DIR の下の既定の名前で作る。
    let work = work_dir("ke");
    let dir = &work.0;
    let mut seinas = start_with_env(dir, &["--no-ime-socket"], &[("XDG_RUNTIME_DIR", dir)]);
    let names = [
        "seinas-0",
        "seinas-0-shell",
        "seinas-0.lock",
        "seinas-0-shell.lock",
    ];
    wait_for("the two sockets", || {
        names
            .iter()
            .all(|name| dir.join(name).exists())
            .then_some(())
    });
    wait_for_log(dir, NO_IME_LINE);
    assert!(seinas.is_running());
    for name in ["seinas-0-ime", "seinas-0-ime.lock"] {
        assert!(!dir.join(name).exists(), "{name}");
    }
    drop(seinas);
}

#[test]
fn the_ime_socket_and_no_ime_socket_are_refused_together() {
    let work = work_dir("kf");
    let dir = &work.0;
    let socket = dir.join("s.sock");
    let ime = dir.join("i.sock");
    let mut seinas = start(
        dir,
        &[
            "--socket",
            &socket.to_string_lossy(),
            "--ime-socket",
            &ime.to_string_lossy(),
            "--no-ime-socket",
        ],
    );
    let status = seinas.0.wait().expect("seinas-standalone ends");
    assert_eq!(status.code(), Some(2));
    let log = fs::read_to_string(dir.join("log")).unwrap_or_default();
    assert!(log.contains("cannot be used together"), "{log}");
    assert!(!socket.exists() && !ime.exists());
}
