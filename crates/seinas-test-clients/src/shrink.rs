//! 試験用: 共有メモリーのプールを、コンポジタに渡した後で縮めるクライアント。
//!
//! 流れ:
//!
//! 1. 封印の無いmemfdでプールを作り、単色のバッファを1枚作る。
//! 2. ウィンドウを作ってバッファを出し、frameコールバックが届くまで待つ(コンポジタが描いた合図)。
//!    届いたら、標準出力に `shrink: drawn` と書く。
//! 3. `--step` が付いていれば、標準入力から1行届くまで待つ(呼ぶ側が画面を確かめるための間)。
//! 4. `ftruncate` でプールを0バイトに縮め、標準出力に `shrink: shrunk` と書く。
//! 5. もう一度damageとcommitを送り、コンポジタにそのバッファを読ませる。
//! 6. コンポジタが切るまで読み、プロトコルのエラーが届いていれば中身を書く。
//!
//! ライブラリを通すと、縮められないように封印されたプールになるので、ソケットへ直接書いている。

use std::{
    fs::File,
    io::{BufRead, ErrorKind, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};

use crate::{message, socket_path, string, WL_DISPLAY};

/// ウィンドウの大きさと色。色は紫(赤と青が最大)で、ほかのウィンドウと見分けやすくしてある。
pub const WIDTH: i32 = 400;
pub const HEIGHT: i32 = 300;
const PIXEL: [u8; 4] = [0xff, 0x00, 0xff, 0xff]; // B, G, R, X
const STRIDE: i32 = WIDTH * 4;
const POOL_SIZE: i32 = STRIDE * HEIGHT;
/// wl_shmの形式の番号(xrgb8888)。
const FORMAT_XRGB8888: u32 = 1;

/// 1つの返事を待つ時間の上限。
const TIMEOUT: Duration = Duration::from_secs(30);

// こちらが決める、オブジェクトの番号。
const REGISTRY: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
const WM_BASE: u32 = 6;
const SURFACE: u32 = 7;
const XDG_SURFACE: u32 = 8;
const TOPLEVEL: u32 = 9;
const POOL: u32 = 10;
const BUFFER: u32 = 11;
const FIRST_FRAME: u32 = 12;
const SECOND_FRAME: u32 = 13;

/// コンポジタから届いた1つのイベント。
struct Event {
    object: u32,
    opcode: u32,
    body: Vec<u8>,
}

impl Event {
    fn u32_at(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.body.get(at..at + 4)?.try_into().ok()?,
        ))
    }
}

/// 接続と、読みかけのバイト。
struct Wire {
    stream: UnixStream,
    incoming: Vec<u8>,
}

impl Wire {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.stream
            .write_all(bytes)
            .map_err(|e| format!("cannot send: {e}"))
    }

    /// バイトと一緒に、fdを1つ送る(`SCM_RIGHTS`)。
    fn send_with_fd(&mut self, bytes: &[u8], fd: i32) -> Result<(), String> {
        // 制御メッセージ1つぶんの領域。8バイト境界にそろえるため、u64の配列で取る。
        let mut control = [0u64; 4];
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut libc::c_void,
            iov_len: bytes.len(),
        };
        // SAFETY: msghdrは0で始めてよい構造体で、指す先(iovとcontrol)は、sendmsgが戻るまで生きている。
        // 制御メッセージは、CMSG_SPACEで求めた大きさの中に、fdを1つだけ書く。
        let sent = unsafe {
            let space = libc::CMSG_SPACE(size_of::<i32>() as u32) as usize;
            assert!(space <= size_of_val(&control));
            let mut header: libc::msghdr = std::mem::zeroed();
            header.msg_iov = &mut iov;
            header.msg_iovlen = 1;
            header.msg_control = control.as_mut_ptr().cast();
            header.msg_controllen = space as _;
            let cmsg = libc::CMSG_FIRSTHDR(&header);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(size_of::<i32>() as u32) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<i32>(), fd);
            libc::sendmsg(self.stream.as_raw_fd(), &header, libc::MSG_NOSIGNAL)
        };
        if sent != bytes.len() as isize {
            return Err(format!(
                "cannot send the pool: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// 次のイベントを待つ。コンポジタが切ったらNone。
    fn next_event(&mut self) -> Result<Option<Event>, String> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.incoming.len() >= 8 {
                let object = u32::from_le_bytes(self.incoming[0..4].try_into().unwrap());
                let word = u32::from_le_bytes(self.incoming[4..8].try_into().unwrap());
                let size = (word >> 16) as usize;
                if size < 8 {
                    return Err("the compositor sent a broken message".to_owned());
                }
                if self.incoming.len() >= size {
                    let body = self.incoming[8..size].to_vec();
                    self.incoming.drain(..size);
                    return Ok(Some(Event {
                        object,
                        opcode: word & 0xffff,
                        body,
                    }));
                }
            }
            let mut buffer = [0u8; 4096];
            match self.stream.read(&mut buffer) {
                Ok(0) => return Ok(None),
                Ok(n) => self.incoming.extend(&buffer[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    if Instant::now() >= deadline {
                        return Err("timed out while waiting for the compositor".to_owned());
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                // 切られた後の読み込みは、接続のエラーになることがある。
                Err(_) => return Ok(None),
            }
        }
    }

    /// `wanted` が真を返すイベントが届くまで読む。途中のping、configureには返事をする。
    ///
    /// コンポジタが切ったら、それまでに届いたプロトコルのエラー(あれば)を添えて失敗にする。
    fn wait_for(&mut self, what: &str, wanted: impl Fn(&Event) -> bool) -> Result<Event, String> {
        loop {
            let Some(event) = self.next_event()? else {
                return Err(format!(
                    "the compositor closed the connection while waiting for {what}"
                ));
            };
            if let Some(error) = protocol_error(&event) {
                return Err(format!("protocol error while waiting for {what}: {error}"));
            }
            self.answer(&event)?;
            if wanted(&event) {
                return Ok(event);
            }
        }
    }

    /// 返事の要るイベント(ping、configure)に返事をする。
    fn answer(&mut self, event: &Event) -> Result<(), String> {
        match (event.object, event.opcode) {
            // xdg_wm_base.ping → pong
            (WM_BASE, 0) => self.send(&message(WM_BASE, 3, &event.body[..4])),
            // xdg_surface.configure → ack_configure
            (XDG_SURFACE, 0) => self.send(&message(XDG_SURFACE, 4, &event.body[..4])),
            _ => Ok(()),
        }
    }
}

/// wl_display.error なら、その中身。
fn protocol_error(event: &Event) -> Option<String> {
    if (event.object, event.opcode) != (WL_DISPLAY, 0) {
        return None;
    }
    let object = event.u32_at(0)?;
    let code = event.u32_at(4)?;
    let length = event.u32_at(8)? as usize;
    let text = event.body.get(12..12 + length.saturating_sub(1))?;
    Some(format!(
        "object {object}, code {code}, \"{}\"",
        String::from_utf8_lossy(text)
    ))
}

/// 封印の無いmemfdを作り、単色の絵で埋める。
fn make_pool() -> Result<File, String> {
    // SAFETY: 名前は終端のある文字列。返ってきたfdは、このFileだけが持つ。
    // MFD_ALLOW_SEALINGを付けないので、このmemfdは封印できず、後から縮められる。
    let fd = unsafe { libc::memfd_create(c"seinas-shrink".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let pixels: Vec<u8> = PIXEL.repeat((WIDTH * HEIGHT) as usize);
    file.write_all(&pixels)
        .map_err(|e| format!("cannot fill the pool: {e}"))?;
    Ok(file)
}

fn say(text: &str) {
    println!("shrink: {text}");
    let _ = std::io::stdout().flush();
}

/// 縮めるクライアントを動かす。コンポジタに切られたらtrue、切られなければfalse。
pub fn run(step: bool) -> Result<bool, String> {
    let path = socket_path()?;
    let stream = UnixStream::connect(&path)
        .map_err(|e| format!("cannot connect to {}: {e}", path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| e.to_string())?;
    let mut wire = Wire {
        stream,
        incoming: Vec::new(),
    };

    // グローバルの一覧を受け取る。syncの返事(wl_callback.done)が、一覧の終わりの合図。
    wire.send(&message(WL_DISPLAY, 1, &REGISTRY.to_le_bytes()))?;
    wire.send(&message(WL_DISPLAY, 0, &SYNC.to_le_bytes()))?;
    let mut globals: Vec<(u32, String)> = Vec::new();
    loop {
        let event = wire.wait_for("the globals", |e| matches!(e.object, REGISTRY | SYNC))?;
        if event.object == SYNC {
            break;
        }
        if event.opcode == 0 {
            // wl_registry.global(番号、インターフェースの名前、版)
            let name = event.u32_at(0).ok_or("a broken global")?;
            let length = event.u32_at(4).ok_or("a broken global")? as usize;
            let interface = event
                .body
                .get(8..8 + length.saturating_sub(1))
                .ok_or("a broken global")?;
            globals.push((name, String::from_utf8_lossy(interface).into_owned()));
        }
    }
    let bind = |interface: &str, id: u32| -> Result<Vec<u8>, String> {
        let (name, _) = globals
            .iter()
            .find(|(_, i)| i == interface)
            .ok_or_else(|| format!("the compositor does not offer {interface}"))?;
        let mut body = name.to_le_bytes().to_vec();
        body.extend(string(interface));
        body.extend(1u32.to_le_bytes());
        body.extend(id.to_le_bytes());
        Ok(message(REGISTRY, 0, &body))
    };
    wire.send(&bind("wl_compositor", COMPOSITOR)?)?;
    wire.send(&bind("wl_shm", SHM)?)?;
    wire.send(&bind("xdg_wm_base", WM_BASE)?)?;

    // ウィンドウを作り、最初のconfigureを待つ。
    wire.send(&message(COMPOSITOR, 0, &SURFACE.to_le_bytes()))?;
    wire.send(&message(
        WM_BASE,
        2,
        &[XDG_SURFACE.to_le_bytes(), SURFACE.to_le_bytes()].concat(),
    ))?;
    wire.send(&message(XDG_SURFACE, 1, &TOPLEVEL.to_le_bytes()))?;
    wire.send(&message(SURFACE, 6, &[]))?;
    wire.wait_for("the first configure", |e| {
        (e.object, e.opcode) == (XDG_SURFACE, 0)
    })?;

    // プールとバッファを作る。プールのfdは、要求と一緒に送る。
    let pool = make_pool()?;
    wire.send_with_fd(
        &message(
            SHM,
            0,
            &[POOL.to_le_bytes(), POOL_SIZE.to_le_bytes()].concat(),
        ),
        pool.as_raw_fd(),
    )?;
    let mut create_buffer = BUFFER.to_le_bytes().to_vec();
    for value in [0, WIDTH, HEIGHT, STRIDE] {
        create_buffer.extend(value.to_le_bytes());
    }
    create_buffer.extend(FORMAT_XRGB8888.to_le_bytes());
    wire.send(&message(POOL, 0, &create_buffer))?;

    // バッファを出し、frameコールバックを頼んでcommitする。
    let mut attach = BUFFER.to_le_bytes().to_vec();
    attach.extend([0i32.to_le_bytes(), 0i32.to_le_bytes()].concat());
    let mut damage = Vec::new();
    for value in [0, 0, WIDTH, HEIGHT] {
        damage.extend(value.to_le_bytes());
    }
    wire.send(&message(SURFACE, 1, &attach))?;
    wire.send(&message(SURFACE, 2, &damage))?;
    wire.send(&message(SURFACE, 3, &FIRST_FRAME.to_le_bytes()))?;
    wire.send(&message(SURFACE, 6, &[]))?;
    wire.wait_for("the first frame", |e| e.object == FIRST_FRAME)?;
    say("drawn");

    if step {
        // 呼ぶ側が画面を確かめ終えるまで待つ。
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("cannot read the go-ahead: {e}"))?;
    }

    // プールを縮める。コンポジタに伝えた大きさは元のままなので、コンポジタが読むと範囲の外になる。
    pool.set_len(0)
        .map_err(|e| format!("cannot shrink the pool: {e}"))?;
    say("shrunk");

    // もう一度commitして、コンポジタにバッファを読ませる。
    wire.send(&message(SURFACE, 1, &attach))?;
    wire.send(&message(SURFACE, 2, &damage))?;
    wire.send(&message(SURFACE, 3, &SECOND_FRAME.to_le_bytes()))?;
    wire.send(&message(SURFACE, 6, &[]))?;

    // コンポジタが切るまで読む。プロトコルのエラーが届けば、中身を書く。
    let deadline = Instant::now() + TIMEOUT;
    let mut error = None;
    let closed = loop {
        match wire.next_event() {
            Ok(None) => break true,
            Ok(Some(event)) => {
                if let Some(text) = protocol_error(&event) {
                    error = Some(text);
                } else if event.object == SECOND_FRAME {
                    say("the second frame was drawn (the compositor did not notice)");
                }
                // 切られた後は送れないので、返事の失敗は気にしない。
                let _ = wire.answer(&event);
            }
            Err(_) => break false,
        }
        if Instant::now() >= deadline {
            break false;
        }
    };
    match error {
        Some(text) => say(&format!("protocol error: {text}")),
        None => say("no protocol error was sent"),
    }
    say(if closed {
        "the compositor closed the connection"
    } else {
        "the connection is still open"
    });
    Ok(closed)
}
