//! ZeikOS の起動の順序(zash のパイプで、クライアントとコンポジタを同時に起こす)と同じく、タスクバー・壁紙・
//! zeyes-min をコンポジタより先に起こしても、ソケットができるまで試し直してつながることを確かめる。
//!
//! 決まり(`docs/taskbar.md` の「起動の順序」、`docs/standalone.md` の「ZeikOS の上で起動する」):3 つとも、最初の接続
//! だけを、100 ms ごとに 30 秒まで試し直す(`seinas-connect`)。1 回目に失敗したら `<名前>: the compositor is not ready at
//! …` を、つないだら `<名前>: connected to the compositor after <回数> tries` を、標準エラーに書く。

mod common;

use std::{fs, path::Path};

use common::{
    child_command, compositor_dir, shows_zeyes_at, start_taskbar, wait_for, workspace_bin,
    Compositor, Running, SKIN,
};

/// 標準エラーを `log` に書く、`package` の実行ファイル `name` を、ソケット `socket` へつなぐように起こす。
fn start_client(dir: &Path, package: &str, name: &str, socket: &Path, log: &Path) -> Running {
    let nowhere = dir.join("no-data");
    Running(
        child_command(workspace_bin(package, name))
            .env("WAYLAND_DISPLAY", socket)
            .env("XDG_CONFIG_HOME", dir)
            .env("XDG_DATA_HOME", &nowhere)
            .env("XDG_DATA_DIRS", &nowhere)
            .env_remove("SEINAS_WALLPAPERS")
            .env_remove("ZEYES_SKIN")
            .stderr(fs::File::create(log).expect("create the log"))
            .spawn()
            .unwrap_or_else(|e| panic!("start {name}: {e}")),
    )
}

#[test]
fn clients_started_before_the_compositor_connect_when_the_sockets_appear() {
    let dir = compositor_dir("so");
    let mut clients = [
        (
            "taskbar.log",
            start_taskbar(&dir, &dir.join("wl-shell"), &["--utc-offset", "+00:00"]),
        ),
        (
            "wallpaper.log",
            start_client(
                &dir,
                "seinas-wallpaper",
                "seinas-wallpaper",
                &dir.join("wl-shell"),
                &dir.join("wallpaper.log"),
            ),
        ),
        (
            "zeyes.log",
            start_client(
                &dir,
                "zeyes-min",
                "zeyes-min",
                &dir.join("wl"),
                &dir.join("zeyes.log"),
            ),
        ),
    ];
    let log = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default();
    // 3 つとも、1 回目の接続に失敗したことをログで確かめてから、コンポジタを起こす(時間には頼らない)。試し直さずに
    // 終わったら、すぐに落とす。
    for (name, client) in clients.iter_mut() {
        wait_for(&format!("the first try of {name} to fail"), || {
            let text = log(name);
            let failed_once = text.contains("the compositor is not ready at");
            assert!(failed_once || client.is_running(), "{name}: {text}");
            failed_once.then_some(())
        });
        assert!(log(name).contains("(ENOENT: "), "{name}: {}", log(name));
    }
    let seinas = Compositor::start("so");
    // zeyes-min のウィンドウが左上に出て、3 つともつないだことをログに書く(右下はタスクバーなので、ウィンドウだけを見る)。
    seinas.wait_for_screen("the zeyes window", |shot| shows_zeyes_at(shot, 0, 0, SKIN));
    for (name, client) in clients.iter_mut() {
        wait_for(&format!("{name} to connect"), || {
            log(name)
                .contains("connected to the compositor after")
                .then_some(())
        });
        assert!(client.is_running(), "{name}: {}", log(name));
    }
}
