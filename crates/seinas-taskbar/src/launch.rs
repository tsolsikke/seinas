//! トーチのメニューから、アプリを起こす所。
//!
//! 起こし方は [`Launcher`] で差し替える。Linux では [`CommandLauncher`](`std::process::Command`。fork と execve。
//! シェルは使わない)を使う。ZeikOS の spawn で起こす形は、ZeikOS の spawn を確かめてから足す(`docs/taskbar.md` の
//! 「トーチのメニュー」)。
//!
//! 子には、タスクバーのシェルの部品のための接続を渡さない:
//! - `WAYLAND_DISPLAY` は、アプリのための(ふだんの)ソケットにする([`app_display`])。`WAYLAND_SOCKET` は消す。
//! - 標準入力と標準出力は `/dev/null`(ZeikOS では、タスクバーの標準出力はコンポジタへのパイプのため)。標準エラーは、
//!   タスクバーと同じ所(ログ)。
//! - ほかの fd は渡さない。タスクバーが開く fd(Wayland の接続、共有メモリー、epoll など)は、どれも close-on-exec。

use std::{
    ffi::{OsStr, OsString},
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::apps::App;

/// アプリを起こすもの。
pub trait Launcher {
    /// `app` を起こす。起こす頼みが受け付けられたら、子の pid。その場で失敗したら、理由。
    fn launch(&mut self, app: &App) -> Result<u32, String>;

    /// 終わった子を片付ける(イベントを扱った後に、毎回呼ぶ)。
    fn reap(&mut self) {}
}

/// シェルの部品のためのソケットの名前 `shell`(`WAYLAND_DISPLAY`)から、アプリのためのソケットの名前を求める。
/// シェルの部品のためのソケットは、アプリのためのソケットの名前に `-shell` を入れたもの(名前に拡張子があれば、その
/// 前に入れる。`docs/standalone.md`)。`seinas-0-shell` → `seinas-0`、`/tmp/seinas/s-shell.sock` → `/tmp/seinas/s.sock`。
/// その形でなければNone。
pub fn app_display(shell: &OsStr) -> Option<OsString> {
    let path = Path::new(shell);
    let name = path.file_name()?.to_str()?;
    let app = match name.rsplit_once('.') {
        Some((stem, ext)) if stem.ends_with("-shell") && !stem.is_empty() => {
            format!("{}.{ext}", &stem[..stem.len() - "-shell".len()])
        }
        _ => name.strip_suffix("-shell")?.to_owned(),
    };
    if app.is_empty() || app.starts_with('.') {
        return None;
    }
    Some(path.with_file_name(app).into_os_string())
}

/// `std::process::Command` で起こす形(Linux)。
pub struct CommandLauncher {
    /// アプリのためのソケット(子の `WAYLAND_DISPLAY`)。分からなければNone(起こさずに理由を返す)。
    display: Option<OsString>,
    children: Vec<Child>,
}

impl CommandLauncher {
    pub fn new(display: Option<OsString>) -> Self {
        CommandLauncher {
            display,
            children: Vec::new(),
        }
    }
}

impl Launcher for CommandLauncher {
    fn launch(&mut self, app: &App) -> Result<u32, String> {
        let display = self
            .display
            .as_ref()
            .ok_or("the socket for applications is not known (use --app-display)")?;
        let (program, args) = app.argv.split_first().ok_or("no program")?;
        let mut command = Command::new(program);
        command
            .args(args)
            .env("WAYLAND_DISPLAY", display)
            .env_remove("WAYLAND_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(dir) = &app.working_dir {
            command.current_dir(dir);
        }
        let child = command.spawn().map_err(|e| format!("{program}: {e}"))?;
        let pid = child.id();
        self.children.push(child);
        Ok(pid)
    }

    fn reap(&mut self) {
        self.children
            .retain_mut(|child| !matches!(child.try_wait(), Ok(Some(_)) | Err(_)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_for_applications_drops_shell_from_the_name() {
        let app = |shell: &str| app_display(OsStr::new(shell)).map(|s| s.into_string().unwrap());
        assert_eq!(app("seinas-0-shell").as_deref(), Some("seinas-0"));
        assert_eq!(
            app("/tmp/seinas/s-shell.sock").as_deref(),
            Some("/tmp/seinas/s.sock")
        );
        assert_eq!(app("/run/x/wl-shell").as_deref(), Some("/run/x/wl"));
        assert_eq!(app("/a/b.c-shell.sock").as_deref(), Some("/a/b.c.sock"));
        assert_eq!(app("seinas-0"), None);
        assert_eq!(app("-shell"), None);
        assert_eq!(app("/tmp/-shell.sock"), None);
    }

    fn app(argv: &[&str]) -> App {
        App {
            id: "t.desktop".to_owned(),
            name: "T".to_owned(),
            icon: None,
            argv: argv.iter().map(|s| s.to_string()).collect(),
            working_dir: None,
        }
    }

    #[test]
    fn a_program_is_started_with_the_socket_for_applications() {
        let dir = std::env::temp_dir().join(format!("seinas-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("env");
        let mut launcher = CommandLauncher::new(Some(OsString::from("/tmp/app.sock")));
        let script = format!(
            "echo \"$WAYLAND_DISPLAY:${{WAYLAND_SOCKET-unset}}\" > {}",
            out.display()
        );
        launcher.launch(&app(&["/bin/sh", "-c", &script])).unwrap();
        // 子が終わって片付くまで待つ(5 秒まで)。
        for _ in 0..500 {
            launcher.reap();
            if launcher.children.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(launcher.children.is_empty());
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "/tmp/app.sock:unset\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
        // 無いプログラムと、ソケットが分からないときは、その場で理由を返す。
        assert!(launcher
            .launch(&app(&["/no/such/program"]))
            .unwrap_err()
            .contains("/no/such/program"));
        let mut unknown = CommandLauncher::new(None);
        assert!(unknown.launch(&app(&["/bin/true"])).is_err());
    }
}
