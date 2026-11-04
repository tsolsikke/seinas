//! シェルの部品とクライアント(タスクバー、壁紙、zeyes-min)が、コンポジタへつなぐ手順(Wayland の接続の最初の段)。
//!
//! コンポジタより先に起こされても、コンポジタのソケットができるまで試し直してつなぐ。ZeikOS の上では、コンポジタと
//! クライアントを zash のパイプで同時に起こすため(`docs/standalone.md` の「ZeikOS の上で起動する」)。決まりの説明は
//! `docs/taskbar.md` の「起動の順序」にある。

use std::{
    ffi::OsStr,
    io,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use wayland_client::Connection;

/// コンポジタへの最初の接続を試し直す間隔と、試し直す期限の既定(最初の試しから)。期限は単調増加の時計で
/// 判定する(`docs/taskbar.md` の「起動の順序」)。
pub const CONNECT_INTERVAL: Duration = Duration::from_millis(100);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// つなぐソケットのパス。`display`(`WAYLAND_DISPLAY`)が絶対パスならそのまま、そうでなければ `runtime_dir`
/// (`XDG_RUNTIME_DIR`。絶対パスに限る)の下(wayland-client の `Connection::connect_to_env` と同じ決まり)。
pub fn socket_path(
    display: Option<&OsStr>,
    runtime_dir: Option<&OsStr>,
) -> Result<PathBuf, String> {
    let display = Path::new(display.ok_or("WAYLAND_DISPLAY is not set")?);
    if display.is_absolute() {
        return Ok(display.to_owned());
    }
    match runtime_dir.map(Path::new) {
        Some(dir) if dir.is_absolute() => Ok(dir.join(display)),
        Some(_) => Err("XDG_RUNTIME_DIR is not an absolute path".to_owned()),
        None => Err(format!(
            "XDG_RUNTIME_DIR is not set (needed for WAYLAND_DISPLAY={})",
            display.display()
        )),
    }
}

/// 接続の失敗が、コンポジタがまだ受けられないためなら、その errno の名前。このときだけ、試し直す。
///
/// - `ENOENT`:ソケットがまだ無い。
/// - `ECONNREFUSED`:ソケットはあるが、listen する相手がいない。
/// - `EAGAIN`:ZeikOS で、接続のテーブルが満杯(Linux の blocking connect は、相手の backlog が満杯なら待つ)。
pub fn not_ready_yet(error: &io::Error) -> Option<&'static str> {
    match error.raw_os_error()? {
        libc::ENOENT => Some("ENOENT"),
        libc::ECONNREFUSED => Some("ECONNREFUSED"),
        libc::EAGAIN => Some("EAGAIN"),
        _ => None,
    }
}

/// コンポジタへつなぐ。コンポジタがまだ受けられないとき([`not_ready_yet`])は、[`CONNECT_INTERVAL`] ずつ眠って
/// (`std::thread::sleep`。musl では clock_nanosleep)、最初の試しから `timeout` の間、試し直す(単調増加の時計で
/// 判定する)。起動の順序で、コンポジタより先に起こされることがあるため(`docs/taskbar.md` の「起動の順序」)。
/// 試し直すのは、この最初の接続だけ(つないだ後に切れたら、終わる)。ログ(標準エラー)の行の頭は `program`。
pub fn connect(program: &str, timeout: Duration) -> Result<Connection, String> {
    // 親が接続を渡してきたときは、試し直す相手がいない。
    if std::env::var_os("WAYLAND_SOCKET").is_some() {
        return Connection::connect_to_env()
            .map_err(|e| format!("cannot connect to the compositor (WAYLAND_SOCKET): {e}"));
    }
    let path = socket_path(
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
    )
    .map_err(|e| format!("cannot connect to the compositor: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut tried = 0;
    let stream = loop {
        tried += 1;
        let e = match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e) => e,
        };
        let Some(reason) = not_ready_yet(&e) else {
            return Err(format!(
                "cannot connect to the compositor at {}: {e}",
                path.display()
            ));
        };
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "the compositor did not appear at {} within {} ms ({reason}: {e}; tried {tried} times)",
                path.display(),
                timeout.as_millis()
            ));
        }
        if tried == 1 {
            eprintln!(
                "{program}: the compositor is not ready at {} ({reason}: {e}); trying again every {} ms for up to {} ms",
                path.display(),
                CONNECT_INTERVAL.as_millis(),
                timeout.as_millis()
            );
        }
        std::thread::sleep(CONNECT_INTERVAL.min(deadline - now));
    };
    if tried > 1 {
        eprintln!("{program}: connected to the compositor after {tried} tries");
    }
    Connection::from_socket(stream).map_err(|e| {
        format!(
            "cannot connect to the compositor at {}: {e}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_path_follows_wayland_display_and_xdg_runtime_dir() {
        let path = |display: Option<&str>, dir: Option<&str>| {
            socket_path(display.map(OsStr::new), dir.map(OsStr::new))
        };
        assert_eq!(
            path(Some("/tmp/s-shell.sock"), None),
            Ok(PathBuf::from("/tmp/s-shell.sock"))
        );
        assert_eq!(
            path(Some("seinas-0-shell"), Some("/run/user/1000")),
            Ok(PathBuf::from("/run/user/1000/seinas-0-shell"))
        );
        assert!(path(None, Some("/run/user/1000")).is_err());
        assert!(path(Some("seinas-0-shell"), None).is_err());
        assert!(path(Some("seinas-0-shell"), Some("run")).is_err());
    }

    #[test]
    fn only_a_missing_socket_no_listener_or_a_full_table_is_tried_again() {
        let reason = |errno| not_ready_yet(&io::Error::from_raw_os_error(errno));
        assert_eq!(reason(libc::ENOENT), Some("ENOENT"));
        assert_eq!(reason(libc::ECONNREFUSED), Some("ECONNREFUSED"));
        assert_eq!(reason(libc::EAGAIN), Some("EAGAIN"));
        assert_eq!(reason(libc::EACCES), None);
        assert_eq!(reason(libc::ENAMETOOLONG), None);
        assert_eq!(
            not_ready_yet(&io::Error::from(io::ErrorKind::NotFound)),
            None
        );
    }
}
