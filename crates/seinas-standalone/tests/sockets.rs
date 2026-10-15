//! ソケットの場所の決まり(`docs/standalone.md` の `--socket`・`--shell-socket`・`--ime-socket`)を確かめる。
//!
//! - シェルの部品と IME のためのソケットは、指定が無ければ、ふだんのソケットのパスの名前に `-shell`・`-ime` を
//!   入れたもの。名前に拡張子があれば、その前に入れる(ロックファイルは、拡張子を `.lock` に替えた名前になるので、
//!   終わりに付けると取り合う)。
//! - 指定したパスどうしでロックファイルがぶつかるときは、分かりやすいエラーで終わる。

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
    let log = fs::File::create(dir.join("log")).expect("create the log file");
    Running(
        child_command(env!("CARGO_BIN_EXE_seinas-standalone"))
            // テスト用の入力インターフェースを開く(seat にキーボードを足し、IME の global を作るため)。
            .args(["--fake", "320x240", "--test-input", "--fonts"])
            .arg(font_dir())
            .args(args)
            .env_remove("SEINAS_FONTS")
            .env("XDG_CONFIG_HOME", dir)
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
