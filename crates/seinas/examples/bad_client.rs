//! 試験用: わざと不正な要求を送るWaylandクライアント。
//!
//! コンポジタが、不正な要求を送ったクライアントだけを切り、自分は動き続けることを確かめるために使う。
//! ライブラリを通すと不正な要求は作れないので、ソケットへ直接バイトを書く。
//!
//! 使い方(Seinasを動かした状態で):
//!
//! ```text
//! WAYLAND_DISPLAY=seinas-0 cargo run -p seinas --example bad_client -- <種類>
//! ```
//!
//! 種類:
//! - `unknown-opcode`: wl_displayに、無い番号の要求を送る。
//! - `unknown-object`: 作っていないオブジェクトへ要求を送る。
//! - `short-message`: 長さがヘッダーより短い、壊れたメッセージを送る。
//! - `bad-global`: wl_registryで、無いグローバルを束ねようとする(プロトコルのエラーが返る)。
//!
//! コンポジタに切られたら終了コード0、2秒待っても切られなければ1で終わる。

use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

const WL_DISPLAY: u32 = 1;

/// 1つのメッセージを作る。ヘッダーは、オブジェクトの番号と、(長さ << 16 | 要求の番号)。
fn message(object: u32, opcode: u16, body: &[u8]) -> Vec<u8> {
    let size = (8 + body.len()) as u32;
    let mut bytes = Vec::new();
    bytes.extend(object.to_le_bytes());
    bytes.extend(((size << 16) | opcode as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

/// 文字列の引数(終端を含む長さ、中身、4バイト境界までの詰め物)。
fn string(text: &str) -> Vec<u8> {
    let mut bytes = ((text.len() + 1) as u32).to_le_bytes().to_vec();
    bytes.extend(text.as_bytes());
    bytes.push(0);
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}

fn requests(kind: &str) -> Option<Vec<u8>> {
    Some(match kind {
        "unknown-opcode" => message(WL_DISPLAY, 99, &[]),
        "unknown-object" => message(4242, 0, &[]),
        // 長さの欄が4(ヘッダーの8より短い)。
        "short-message" => [WL_DISPLAY.to_le_bytes(), (4u32 << 16).to_le_bytes()].concat(),
        "bad-global" => {
            // wl_display.get_registry(新しい番号2) の後、wl_registry.bind で無いグローバルを指す。
            let mut bytes = message(WL_DISPLAY, 1, &2u32.to_le_bytes());
            let mut bind = 0xffff_u32.to_le_bytes().to_vec();
            bind.extend(string("wl_compositor"));
            bind.extend(1u32.to_le_bytes());
            bind.extend(3u32.to_le_bytes());
            bytes.extend(message(2, 0, &bind));
            bytes
        }
        _ => return None,
    })
}

/// 届いたバイトの中から、wl_display.error(プロトコルのエラー)を探して、中身を返す。
fn find_protocol_error(mut bytes: &[u8]) -> Option<String> {
    while bytes.len() >= 8 {
        let object = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        let word = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let (size, opcode) = ((word >> 16) as usize, word & 0xffff);
        if size < 8 || size > bytes.len() {
            return None;
        }
        if object == WL_DISPLAY && opcode == 0 && size >= 20 {
            let target = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
            let code = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
            let length = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
            let text = bytes.get(20..20 + length.saturating_sub(1))?;
            return Some(format!(
                "object {target}, code {code}, \"{}\"",
                String::from_utf8_lossy(text)
            ));
        }
        bytes = &bytes[size..];
    }
    None
}

fn socket_path() -> Result<PathBuf, String> {
    let display = std::env::var("WAYLAND_DISPLAY").map_err(|_| "WAYLAND_DISPLAY is not set")?;
    let display = PathBuf::from(display);
    if display.is_absolute() {
        return Ok(display);
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime).join(display))
}

fn run(kind: &str) -> Result<bool, String> {
    let bytes = requests(kind).ok_or_else(|| format!("unknown kind: {kind}"))?;
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("cannot connect to {}: {e}", path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|e| format!("cannot send: {e}"))?;

    // コンポジタが切るまで読む。切られれば、読み込みが0バイトか、接続のエラーで終わる。
    let mut received = Vec::new();
    let mut buffer = [0u8; 4096];
    let closed = loop {
        match stream.read(&mut buffer) {
            Ok(0) => break true,
            Ok(n) => received.extend(&buffer[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break false
            }
            Err(_) => break true,
        }
    };
    match find_protocol_error(&received) {
        Some(error) => println!("bad_client: {kind}: protocol error: {error}"),
        None => println!("bad_client: {kind}: no protocol error was sent"),
    }
    println!(
        "bad_client: {kind}: {}",
        if closed {
            "the compositor closed the connection"
        } else {
            "the connection is still open"
        }
    );
    Ok(closed)
}

fn main() -> ExitCode {
    let Some(kind) = std::env::args().nth(1) else {
        eprintln!("usage: bad_client <unknown-opcode|unknown-object|short-message|bad-global>");
        return ExitCode::from(2);
    };
    match run(&kind) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("bad_client: {message}");
            ExitCode::from(2)
        }
    }
}
