//! 試験用のクライアントが共通に使う、Waylandのやり取りの土台。
//!
//! Waylandのライブラリは使わず、ソケットへ直接バイトを書く。ウィンドウを1つ作って、単色の絵を出す
//! ところまでを用意している。

use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    time::{Duration, Instant},
};

use crate::{message, socket_path, string, WL_DISPLAY};

/// wl_shmの形式の番号(xrgb8888)。
const FORMAT_XRGB8888: u32 = 1;

/// 1つの返事を待つ時間の上限。
pub const TIMEOUT: Duration = Duration::from_secs(30);

// こちらが決める、オブジェクトの番号。新しい番号は、小さいほうから順に使う決まりになっている。
const REGISTRY: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
pub const WM_BASE: u32 = 6;
pub const SURFACE: u32 = 7;
pub const XDG_SURFACE: u32 = 8;
pub const TOPLEVEL: u32 = 9;
const POOL: u32 = 10;
pub const BUFFER: u32 = 11;
/// ここから先の番号は、[`Wire::new_id`] が順に配る。
const FIRST_FREE: u32 = 12;

/// ウィンドウを作るときの指定。
#[derive(Default)]
pub struct WindowSetup {
    /// 題名(xdg_toplevel.set_title)。Noneなら付けない。
    pub title: Option<String>,
    /// app_id(xdg_toplevel.set_app_id)。Noneなら付けない。
    pub app_id: Option<String>,
    /// 飾りの描き方を、xdg-decoration(zxdg_decoration_manager_v1)で尋ねるか。
    pub decoration: bool,
}

/// 次のイベントを少しだけ待った結果。
pub enum Polled {
    Event(Event),
    /// コンポジタが切った。
    Closed,
    /// 待つ間に、何も届かなかった。
    Nothing,
}

/// コンポジタから届いた1つのイベント。
pub struct Event {
    pub object: u32,
    pub opcode: u32,
    pub body: Vec<u8>,
}

impl Event {
    pub fn u32_at(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.body.get(at..at + 4)?.try_into().ok()?,
        ))
    }
}

/// 接続と、読みかけのバイト。
pub struct Wire {
    stream: UnixStream,
    incoming: Vec<u8>,
    /// xdg_toplevel.configure が届くたびに、中身を標準出力に書くか。
    pub print_configures: bool,
    /// 次に配る、オブジェクトの番号。
    next_id: u32,
    /// 飾りの描き方を尋ねるオブジェクト(zxdg_toplevel_decoration_v1)の番号。作っていなければNone。
    decoration: Option<u32>,
    /// コンポジタが公開しているグローバル(番号と、インターフェースの名前)。
    globals: Vec<(u32, String)>,
    /// 席(wl_seat)とポインター(wl_pointer)の番号。[`Wire::watch_pointer`] を呼ぶまではNone。
    seat: Option<u32>,
    pointer: Option<u32>,
    /// コンポジタから、閉じるように頼まれたか(xdg_toplevel.close)。
    pub close_requested: bool,
}

impl Wire {
    /// まだ使っていない、オブジェクトの番号を1つ取る。
    pub fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
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

    /// 次のイベントを待つ。コンポジタが切ったらNone。[`TIMEOUT`] の間に何も届かなければ失敗。
    pub fn next_event(&mut self) -> Result<Option<Event>, String> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.poll_event()? {
                Polled::Event(event) => return Ok(Some(event)),
                Polled::Closed => return Ok(None),
                Polled::Nothing if Instant::now() >= deadline => {
                    return Err("timed out while waiting for the compositor".to_owned());
                }
                Polled::Nothing => {}
            }
        }
    }

    /// 次のイベントを、少しの間(接続の読み込みの待ち時間)だけ待つ。
    pub fn poll_event(&mut self) -> Result<Polled, String> {
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
                    return Ok(Polled::Event(Event {
                        object,
                        opcode: word & 0xffff,
                        body,
                    }));
                }
            }
            let mut buffer = [0u8; 4096];
            match self.stream.read(&mut buffer) {
                Ok(0) => return Ok(Polled::Closed),
                Ok(n) => self.incoming.extend(&buffer[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Ok(Polled::Nothing);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                // 切られた後の読み込みは、接続のエラーになることがある。
                Err(_) => return Ok(Polled::Closed),
            }
        }
    }

    /// 席のポインターを使い始める。この後、ポインターの知らせが届くようになる。
    ///
    /// コンポジタが席を公開していなければ、失敗する。
    pub fn watch_pointer(&mut self) -> Result<(), String> {
        let name = self
            .globals
            .iter()
            .find(|(_, interface)| interface == "wl_seat")
            .map(|(name, _)| *name)
            .ok_or("the compositor does not offer wl_seat")?;
        let (seat, pointer) = (self.new_id(), self.new_id());
        let mut bind = name.to_le_bytes().to_vec();
        bind.extend(string("wl_seat"));
        bind.extend(1u32.to_le_bytes());
        bind.extend(seat.to_le_bytes());
        self.send(&message(REGISTRY, 0, &bind))?;
        // wl_seat.get_pointer(新しい番号)
        self.send(&message(seat, 0, &pointer.to_le_bytes()))?;
        self.seat = Some(seat);
        self.pointer = Some(pointer);
        Ok(())
    }

    /// `event` が、ポインターのボタンが押された知らせなら、その通し番号(serial)。
    pub fn button_press_serial(&self, event: &Event) -> Option<u32> {
        // wl_pointer.button(通し番号、時刻、ボタン、状態)。状態の1が「押された」。
        (Some(event.object) == self.pointer && event.opcode == 3 && event.u32_at(12) == Some(1))
            .then(|| event.u32_at(0))?
    }

    /// 通し番号が `serial` のボタンを押したまま、ウィンドウを動かしてほしいと頼む(xdg_toplevel.move)。
    pub fn request_move(&mut self, serial: u32) -> Result<(), String> {
        let seat = self.seat.ok_or("the pointer is not watched")?;
        self.send(&message(
            TOPLEVEL,
            5,
            &[seat.to_le_bytes(), serial.to_le_bytes()].concat(),
        ))
    }

    /// 題名を付ける(xdg_toplevel.set_title)。commitすると、コンポジタに伝わる。
    pub fn set_title(&mut self, title: &str) -> Result<(), String> {
        self.send(&message(TOPLEVEL, 2, &string(title)))
    }

    /// 画面(サーフェス)をcommitする。
    pub fn commit(&mut self) -> Result<(), String> {
        self.send(&message(SURFACE, 6, &[]))
    }

    /// `wanted` が真を返すイベントが届くまで読む。途中のping、configureには返事をする。
    ///
    /// コンポジタが切ったら、それまでに届いたプロトコルのエラー(あれば)を添えて失敗にする。
    pub fn wait_for(
        &mut self,
        what: &str,
        wanted: impl Fn(&Event) -> bool,
    ) -> Result<Event, String> {
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

    /// `print_configures` なら、標準出力に「window: …」の1行を書く。
    fn say(&self, text: &str) {
        if self.print_configures {
            println!("window: {text}");
            let _ = std::io::stdout().flush();
        }
    }

    /// 返事の要るイベント(ping、configure)に返事をする。届いた知らせの中身も、求められていれば書く。
    pub fn answer(&mut self, event: &Event) -> Result<(), String> {
        match (event.object, event.opcode) {
            // xdg_wm_base.ping → pong
            (WM_BASE, 0) => self.send(&message(WM_BASE, 3, &event.body[..4])),
            // xdg_surface.configure → ack_configure
            (XDG_SURFACE, 0) => self.send(&message(XDG_SURFACE, 4, &event.body[..4])),
            // xdg_toplevel.configure(幅、高さ、状態の並び)
            (TOPLEVEL, 0) => {
                if self.print_configures {
                    println!("window: configure {}", describe_configure(event));
                    let _ = std::io::stdout().flush();
                }
                Ok(())
            }
            // xdg_toplevel.close(閉じるように頼まれた)
            (TOPLEVEL, 1) => {
                self.close_requested = true;
                self.say("close");
                Ok(())
            }
            // xdg_toplevel.wm_capabilities(コンポジタができることの並び)
            (TOPLEVEL, 3) => {
                let count = event.u32_at(0).unwrap_or(0) as usize / 4;
                let names: Vec<String> = (0..count)
                    .filter_map(|i| event.u32_at(4 + i * 4))
                    .map(|capability| match capability {
                        1 => "window_menu".to_owned(),
                        2 => "maximize".to_owned(),
                        3 => "fullscreen".to_owned(),
                        4 => "minimize".to_owned(),
                        other => format!("capability-{other}"),
                    })
                    .collect();
                self.say(&format!("wm_capabilities [{}]", names.join(",")));
                Ok(())
            }
            // wl_pointer の知らせ。位置は、中身の左上を原点にした画素(端数は切り捨て)。
            (object, opcode) if Some(object) == self.pointer => {
                let fixed = |at: usize| event.u32_at(at).unwrap_or(0) as i32 >> 8;
                match opcode {
                    0 => self.say(&format!("pointer enter {} {}", fixed(8), fixed(12))),
                    1 => self.say("pointer leave"),
                    2 => self.say(&format!("pointer motion {} {}", fixed(4), fixed(8))),
                    3 => {
                        let state = match event.u32_at(12) {
                            Some(1) => "pressed",
                            _ => "released",
                        };
                        let button = event.u32_at(8).unwrap_or(0);
                        self.say(&format!("pointer button {button:#x} {state}"));
                    }
                    _ => {}
                }
                Ok(())
            }
            // zxdg_toplevel_decoration_v1.configure(飾りを、どちらの側で描くか)
            (object, 0) if Some(object) == self.decoration => {
                if self.print_configures {
                    let mode = match event.u32_at(0) {
                        Some(1) => "client-side".to_owned(),
                        Some(2) => "server-side".to_owned(),
                        other => format!("{other:?}"),
                    };
                    println!("window: decoration {mode}");
                    let _ = std::io::stdout().flush();
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// xdg_toplevel.configure の中身を、`640x480 [activated]` の形に直す。
fn describe_configure(event: &Event) -> String {
    let width = event.u32_at(0).unwrap_or(0);
    let height = event.u32_at(4).unwrap_or(0);
    let count = event.u32_at(8).unwrap_or(0) as usize / 4;
    let states: Vec<String> = (0..count)
        .filter_map(|i| event.u32_at(12 + i * 4))
        .map(|state| match state {
            1 => "maximized".to_owned(),
            2 => "fullscreen".to_owned(),
            3 => "resizing".to_owned(),
            4 => "activated".to_owned(),
            other => format!("state-{other}"),
        })
        .collect();
    format!("{width}x{height} [{}]", states.join(","))
}

/// wl_display.error なら、その中身。
pub fn protocol_error(event: &Event) -> Option<String> {
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

/// memfdを作り、単色の絵で埋める。
///
/// MFD_ALLOW_SEALINGを付けないので、このmemfdは封印できず、後から縮められる。
fn make_pool(width: i32, height: i32, pixel: [u8; 4]) -> Result<File, String> {
    // SAFETY: 名前は終端のある文字列。返ってきたfdは、このFileだけが持つ。
    let fd = unsafe { libc::memfd_create(c"seinas-test".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let pixels: Vec<u8> = pixel.repeat((width * height) as usize);
    file.write_all(&pixels)
        .map_err(|e| format!("cannot fill the pool: {e}"))?;
    Ok(file)
}

/// コンポジタにつなぐ。
pub fn connect() -> Result<Wire, String> {
    let path = socket_path()?;
    let stream = UnixStream::connect(&path)
        .map_err(|e| format!("cannot connect to {}: {e}", path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| e.to_string())?;
    Ok(Wire {
        stream,
        incoming: Vec::new(),
        print_configures: false,
        next_id: FIRST_FREE,
        decoration: None,
        globals: Vec::new(),
        seat: None,
        pointer: None,
        close_requested: false,
    })
}

/// 画面(サーフェス)にバッファを付け、全面をdamageにして、frameコールバック `callback` を頼んでcommitする。
pub fn commit_buffer(
    wire: &mut Wire,
    width: i32,
    height: i32,
    callback: u32,
) -> Result<(), String> {
    let mut attach = BUFFER.to_le_bytes().to_vec();
    attach.extend([0i32.to_le_bytes(), 0i32.to_le_bytes()].concat());
    let mut damage = Vec::new();
    for value in [0, 0, width, height] {
        damage.extend(value.to_le_bytes());
    }
    wire.send(&message(SURFACE, 1, &attach))?;
    wire.send(&message(SURFACE, 2, &damage))?;
    wire.send(&message(SURFACE, 3, &callback.to_le_bytes()))?;
    wire.send(&message(SURFACE, 6, &[]))
}

/// `width` × `height` の、色が `pixel`(B, G, R, X)のウィンドウを1つ出し、コンポジタが描くまで待つ。
///
/// 返すのは、絵を置いた共有メモリーのプール(memfd)。捨てると閉じる。
pub fn open_window(
    wire: &mut Wire,
    width: i32,
    height: i32,
    pixel: [u8; 4],
) -> Result<File, String> {
    open_window_with(wire, width, height, pixel, &WindowSetup::default())
}

/// [`open_window`] と同じだが、題名などを `setup` で指定する。
///
/// 飾りの描き方を尋ねるときは、どちらの側で描くとも頼まず、コンポジタの答えを聞くだけにする。
pub fn open_window_with(
    wire: &mut Wire,
    width: i32,
    height: i32,
    pixel: [u8; 4],
    setup: &WindowSetup,
) -> Result<File, String> {
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
    let bind = |interface: &str, version: u32, id: u32| -> Result<Vec<u8>, String> {
        let (name, _) = globals
            .iter()
            .find(|(_, i)| i == interface)
            .ok_or_else(|| format!("the compositor does not offer {interface}"))?;
        let mut body = name.to_le_bytes().to_vec();
        body.extend(string(interface));
        body.extend(version.to_le_bytes());
        body.extend(id.to_le_bytes());
        Ok(message(REGISTRY, 0, &body))
    };
    wire.send(&bind("wl_compositor", 1, COMPOSITOR)?)?;
    wire.send(&bind("wl_shm", 1, SHM)?)?;
    // 版5から、コンポジタができることの並び(wm_capabilities)が届く。
    wire.send(&bind("xdg_wm_base", 5, WM_BASE)?)?;

    // ウィンドウを作り、最初のconfigureを待つ。
    wire.send(&message(COMPOSITOR, 0, &SURFACE.to_le_bytes()))?;
    wire.send(&message(
        WM_BASE,
        2,
        &[XDG_SURFACE.to_le_bytes(), SURFACE.to_le_bytes()].concat(),
    ))?;
    wire.send(&message(XDG_SURFACE, 1, &TOPLEVEL.to_le_bytes()))?;
    if let Some(title) = &setup.title {
        wire.set_title(title)?;
    }
    if let Some(app_id) = &setup.app_id {
        wire.send(&message(TOPLEVEL, 3, &string(app_id)))?;
    }
    wire.commit()?;
    wire.wait_for("the first configure", |e| {
        (e.object, e.opcode) == (XDG_SURFACE, 0)
    })?;

    // プールとバッファを作る。プールのfdは、要求と一緒に送る。
    let stride = width * 4;
    let pool_size = stride * height;
    let pool = make_pool(width, height, pixel)?;
    wire.send_with_fd(
        &message(
            SHM,
            0,
            &[POOL.to_le_bytes(), pool_size.to_le_bytes()].concat(),
        ),
        pool.as_raw_fd(),
    )?;
    let mut create_buffer = BUFFER.to_le_bytes().to_vec();
    for value in [0, width, height, stride] {
        create_buffer.extend(value.to_le_bytes());
    }
    create_buffer.extend(FORMAT_XRGB8888.to_le_bytes());
    wire.send(&message(POOL, 0, &create_buffer))?;

    if setup.decoration {
        // 番号を順に使うため、バッファを作った後に作る。バッファを画面に付ける前なので、決まりには合っている。
        let (manager, decoration) = (wire.new_id(), wire.new_id());
        wire.send(&bind("zxdg_decoration_manager_v1", 1, manager)?)?;
        // zxdg_decoration_manager_v1.get_toplevel_decoration(新しい番号、toplevel)
        wire.send(&message(
            manager,
            1,
            &[decoration.to_le_bytes(), TOPLEVEL.to_le_bytes()].concat(),
        ))?;
        wire.decoration = Some(decoration);
    }

    // バッファを出し、frameコールバックが届くまで待つ(コンポジタが描いた合図)。
    let first_frame = wire.new_id();
    commit_buffer(wire, width, height, first_frame)?;
    wire.wait_for("the first frame", |e| e.object == first_frame)?;
    wire.globals = globals;
    Ok(pool)
}
