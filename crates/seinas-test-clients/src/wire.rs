//! テスト用のクライアントが共通に使う、Waylandのやり取りの土台。
//!
//! Waylandのライブラリは使わず、ソケットへ直接バイトを書く。ウィンドウを1つ作って、単色の絵を出す
//! ところまでを用意している。

use std::{
    collections::VecDeque,
    fs::File,
    io::{ErrorKind, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
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
pub const SHM: u32 = 5;
pub const WM_BASE: u32 = 6;
pub const SURFACE: u32 = 7;
pub const XDG_SURFACE: u32 = 8;
pub const TOPLEVEL: u32 = 9;
const POOL: u32 = 10;
pub const BUFFER: u32 = 11;
/// ここから先の番号は、[`Wire::new_id`] が順に配る。
const FIRST_FREE: u32 = 12;
/// `WindowSetup::margin` の縁の色(B, G, R, X。灰色)。
pub const MARGIN_PIXEL: [u8; 4] = [0x50, 0x50, 0x50, 0xff];

/// ウィンドウを作るときの指定。
#[derive(Default)]
pub struct WindowSetup {
    /// タイトル(xdg_toplevel.set_title)。Noneなら付けない。
    pub title: Option<String>,
    /// app_id(xdg_toplevel.set_app_id)。Noneなら付けない。
    pub app_id: Option<String>,
    /// xdg-decoration(zxdg_decoration_manager_v1)で、装飾をどう取り決めるか。既定は、server_side を求める。
    pub decoration: Decoration,
    /// 装飾の取り決めの答え(zxdg_toplevel_decoration_v1.configure)を、標準出力に書くか。
    pub print_decoration: bool,
    /// コンテンツのまわりに描く縁の幅(自分で描く影のまね。色は [`MARGIN_PIXEL`])。0 より大きければ、縁を除いた
    /// コンテンツの矩形を、ウィンドウの幾何(xdg_surface.set_window_geometry)として伝える。
    pub margin: i32,
    /// コンテンツの最小と最大の大きさ(xdg_toplevel.set_min_size、set_max_size)。Noneなら送らない。
    pub min_size: Option<(u32, u32)>,
    pub max_size: Option<(u32, u32)>,
}

/// ポップアップ(xdg_popup)を出すときの指定。位置と大きさは、親のウィンドウの幾何の左上からのピクセル。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupSpec {
    /// anchor rect の左上(大きさは 1x1)と、ポップアップの大きさ。
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub width: i32,
    pub height: i32,
    /// xdg_positioner の anchor と gravity(8 は bottom_right)、constraint_adjustment(slide_x 1、slide_y 2、
    /// flip_x 4、flip_y 8、resize_x 16、resize_y 32 の和)。
    pub anchor: u32,
    pub gravity: u32,
    pub adjust: u32,
    /// xdg_popup.grab を、直前の押下(かキー)の serial で頼むか。
    pub grab: bool,
    /// 親を、いちばん新しいポップアップにするか(入れ子)。しなければ、ウィンドウが親。
    pub child: bool,
    /// 色(B, G, R, X)。
    pub pixel: [u8; 4],
}

/// 出したポップアップの番号と状態。
struct Popup {
    surface: u32,
    xdg_surface: u32,
    popup: u32,
    spec: PopupSpec,
    /// いちばん新しい xdg_popup.configure の大きさ(届いていなければNone)。
    size: Option<(i32, i32)>,
    /// 絵を置いた pool(捨てると閉じる)。
    pools: Vec<File>,
    done: bool,
    /// 絵を出したときに頼んだ frame コールバックと、その絵の大きさ。届いたら `popup N drawn WxH` を書く。
    frame: Option<(u32, i32, i32)>,
}

/// xdg-decoration での、装飾の取り決め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Decoration {
    /// 装飾のオブジェクトを作らない(取り決めない。クライアントが自分で描くことになる)。
    None,
    /// 装飾のオブジェクトを作り、server_side を求める(既定)。
    #[default]
    ServerSide,
    /// 装飾のオブジェクトを作り、client_side を求める。
    ClientSide,
}

impl Decoration {
    /// zxdg_toplevel_decoration_v1 のモードの値。作らないならNone。
    fn mode(self) -> Option<u32> {
        match self {
            Decoration::None => None,
            Decoration::ClientSide => Some(1),
            Decoration::ServerSide => Some(2),
        }
    }
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
    /// 飾りの描き方を尋ねるオブジェクト(zxdg_toplevel_decoration_v1)と、その manager の番号。作っていなければNone。
    decoration: Option<u32>,
    decoration_manager: Option<u32>,
    /// 装飾の取り決めの答えを、標準出力に書くか。
    pub print_decoration: bool,
    /// コンポジタが公開しているグローバル(番号と、インターフェースの名前)。
    globals: Vec<(u32, String)>,
    /// seat(wl_seat)とポインター(wl_pointer)の番号。[`Wire::watch_seat`] を呼ぶまではNone。
    seat: Option<u32>,
    pointer: Option<u32>,
    /// キーボード(wl_keyboard)の番号。[`Wire::watch_seat`] で使い始めるまではNone。
    keyboard: Option<u32>,
    /// コンポジタから、閉じるように頼まれたか(xdg_toplevel.close)。
    pub close_requested: bool,
    /// xdg_wm_base の ping に、自分では pong を返さないか(固まったクライアントのまね)。返さなかった ping の
    /// serial は `unanswered_ping` に残し、[`Wire::pong`] で返す。
    pub ignore_ping: bool,
    pub unanswered_ping: Option<u32>,
    /// layer surface(zwlr_layer_surface_v1)の番号。layer surface を作ったときだけ。
    layer_surface: Option<u32>,
    /// layer surface に届いた、いちばん新しい configure の大きさ。
    pub layer_size: Option<(u32, u32)>,
    /// xdg_toplevel.configure で届いた、いちばん新しい大きさ(0x0 は「クライアントに任せる」)と、maximized の
    /// 状態が付いていたか。
    pub toplevel_size: Option<(u32, u32)>,
    pub toplevel_maximized: bool,
    /// コンポジタから届いた fd(keymap など)を、届いた順に残すか。残さなければ、すぐ閉じる。
    pub keep_fds: bool,
    pub fds: VecDeque<OwnedFd>,
    /// 出したポップアップ(番号は、出した順に 1 から)。
    popups: Vec<Popup>,
    /// 直前の入力(ポインターの押下か、キーの押下)の serial。ポップアップの grab に使う。
    last_input_serial: Option<u32>,
    /// layer surface のポップアップのために bind した xdg_wm_base(ウィンドウの xdg_wm_base は `WM_BASE`)。
    layer_wm_base: Option<u32>,
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

    /// [`Wire::list_globals`] で受け取った global の interface の名前(届いた順)。
    pub fn interfaces(&self) -> impl Iterator<Item = &str> {
        self.globals.iter().map(|(_, interface)| interface.as_str())
    }

    /// seat(wl_seat)の番号。[`Wire::watch_seat`] を呼ぶまではNone。
    pub fn seat(&self) -> Option<u32> {
        self.seat
    }

    /// バイトと一緒に、fdを1つ送る(`SCM_RIGHTS`)。
    pub fn send_with_fd(&mut self, bytes: &[u8], fd: i32) -> Result<(), String> {
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
            match self.receive(&mut buffer) {
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

    /// バイトを読む。一緒に届いた fd は、`keep_fds` なら `fds` に残し、そうでなければ閉じる。
    fn receive(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        // 制御メッセージの領域(fd を 16 個まで)。8バイト境界にそろえるため、u64の配列で取る。
        let mut control = [0u64; 10];
        let mut iov = libc::iovec {
            iov_base: buffer.as_mut_ptr().cast(),
            iov_len: buffer.len(),
        };
        // SAFETY: msghdrは0で始めてよい構造体で、指す先(iovとcontrol)は、recvmsgが戻るまで生きている。
        // 届いた fd は、制御メッセージの中に書かれた数だけ読み、それぞれを1つの OwnedFd だけが持つ。
        unsafe {
            let mut header: libc::msghdr = std::mem::zeroed();
            header.msg_iov = &mut iov;
            header.msg_iovlen = 1;
            header.msg_control = control.as_mut_ptr().cast();
            header.msg_controllen = size_of_val(&control) as _;
            let received =
                libc::recvmsg(self.stream.as_raw_fd(), &mut header, libc::MSG_CMSG_CLOEXEC);
            if received < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut cmsg = libc::CMSG_FIRSTHDR(&header);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data = libc::CMSG_DATA(cmsg).cast::<i32>();
                    let count =
                        ((*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / size_of::<i32>();
                    for i in 0..count {
                        let fd = OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i)));
                        if self.keep_fds {
                            self.fds.push_back(fd);
                        }
                    }
                }
                cmsg = libc::CMSG_NXTHDR(&header, cmsg);
            }
            Ok(received as usize)
        }
    }

    /// グローバルの一覧を受け取って覚える。この後の新しい番号は、4 から順に [`Wire::bind`] などで配る。
    ///
    /// ウィンドウも layer surface も作らないクライアント(一覧を受け取るだけのもの)が使う。
    pub fn list_globals(&mut self) -> Result<(), String> {
        self.send(&message(WL_DISPLAY, 1, &REGISTRY.to_le_bytes()))?;
        self.send(&message(WL_DISPLAY, 0, &SYNC.to_le_bytes()))?;
        let mut globals: Vec<(u32, String)> = Vec::new();
        loop {
            let Some(event) = self.next_event()? else {
                return Err("the compositor closed the connection".to_owned());
            };
            if let Some(error) = protocol_error(&event) {
                return Err(format!("protocol error while listing the globals: {error}"));
            }
            match (event.object, event.opcode) {
                (SYNC, _) => break,
                (REGISTRY, 0) => {
                    let name = event.u32_at(0).ok_or("a broken global")?;
                    let length = event.u32_at(4).ok_or("a broken global")? as usize;
                    let interface = event
                        .body
                        .get(8..8 + length.saturating_sub(1))
                        .ok_or("a broken global")?;
                    globals.push((name, String::from_utf8_lossy(interface).into_owned()));
                }
                _ => {}
            }
        }
        self.globals = globals;
        self.next_id = SYNC + 1;
        Ok(())
    }

    /// グローバル `interface` を、バージョン `version` で bind する。返すのは、新しいオブジェクトの番号。
    pub fn bind(&mut self, interface: &str, version: u32) -> Result<u32, String> {
        let name = self
            .globals
            .iter()
            .find(|(_, i)| i == interface)
            .map(|(name, _)| *name)
            .ok_or_else(|| format!("the compositor does not offer {interface}"))?;
        let id = self.new_id();
        let mut body = name.to_le_bytes().to_vec();
        body.extend(string(interface));
        body.extend(version.to_le_bytes());
        body.extend(id.to_le_bytes());
        self.send(&message(REGISTRY, 0, &body))?;
        Ok(id)
    }

    /// 装飾のモード `mode`(1 が client_side、2 が server_side)を求める(zxdg_toplevel_decoration_v1.set_mode)。
    /// 装飾のオブジェクトがまだ無ければ作る(manager も、まだなら bind する)。commit はしない。
    pub fn ask_decoration(&mut self, mode: u32) -> Result<(), String> {
        let manager = match self.decoration_manager {
            Some(manager) => manager,
            None => {
                let manager = self.bind("zxdg_decoration_manager_v1", 1)?;
                self.decoration_manager = Some(manager);
                manager
            }
        };
        let decoration = match self.decoration {
            Some(decoration) => decoration,
            None => {
                // zxdg_decoration_manager_v1.get_toplevel_decoration(新しい番号、toplevel)
                let decoration = self.new_id();
                self.send(&message(
                    manager,
                    1,
                    &[decoration.to_le_bytes(), TOPLEVEL.to_le_bytes()].concat(),
                ))?;
                self.decoration = Some(decoration);
                decoration
            }
        };
        // zxdg_toplevel_decoration_v1.set_mode(モード)
        self.send(&message(decoration, 1, &mode.to_le_bytes()))
    }

    /// seat のポインター(`pointer`)とキーボード(`keyboard`)を使い始める。この後、それぞれのイベントが届く
    /// ようになる。コンポジタが seat を公開していなければ、失敗する。
    pub fn watch_seat(&mut self, pointer: bool, keyboard: bool) -> Result<(), String> {
        let name = self
            .globals
            .iter()
            .find(|(_, interface)| interface == "wl_seat")
            .map(|(name, _)| *name)
            .ok_or("the compositor does not offer wl_seat")?;
        let seat = self.new_id();
        let mut bind = name.to_le_bytes().to_vec();
        bind.extend(string("wl_seat"));
        bind.extend(1u32.to_le_bytes());
        bind.extend(seat.to_le_bytes());
        self.send(&message(REGISTRY, 0, &bind))?;
        self.seat = Some(seat);
        if pointer {
            // wl_seat.get_pointer(新しい番号)
            let id = self.new_id();
            self.send(&message(seat, 0, &id.to_le_bytes()))?;
            self.pointer = Some(id);
        }
        if keyboard {
            // wl_seat.get_keyboard(新しい番号)
            let id = self.new_id();
            self.send(&message(seat, 1, &id.to_le_bytes()))?;
            self.keyboard = Some(id);
        }
        Ok(())
    }

    /// ポップアップを 1 つ出す(新しい wl_surface と xdg_surface と positioner を作り、xdg_surface.get_popup)。最初の
    /// commit までする。configure が届いたら、その大きさの絵を出す([`Wire::answer`])。返すのは、ポップアップの番号。
    pub fn open_popup(&mut self, spec: PopupSpec) -> Result<usize, String> {
        // layer surface のポップアップは、親を null にして作り、zwlr_layer_surface_v1.get_popup で親を決める。
        let on_layer = self.layer_surface.filter(|_| !spec.child);
        let parent = if spec.child {
            self.popups
                .last()
                .ok_or("no popup for the parent")?
                .xdg_surface
        } else if on_layer.is_some() {
            0
        } else {
            XDG_SURFACE
        };
        let wm_base = self.wm_base()?;
        let surface = self.new_id();
        self.send(&message(COMPOSITOR, 0, &surface.to_le_bytes()))?;
        let xdg_surface = self.new_id();
        self.send(&message(
            wm_base,
            2,
            &[xdg_surface.to_le_bytes(), surface.to_le_bytes()].concat(),
        ))?;
        let positioner = self.positioner(spec, spec.anchor_x, spec.anchor_y)?;
        let popup = self.new_id();
        // xdg_surface.get_popup(新しい番号、親の xdg_surface、positioner)
        self.send(&message(
            xdg_surface,
            2,
            &[
                popup.to_le_bytes(),
                parent.to_le_bytes(),
                positioner.to_le_bytes(),
            ]
            .concat(),
        ))?;
        self.send(&message(positioner, 0, &[]))?;
        if let Some(layer) = on_layer {
            // zwlr_layer_surface_v1.get_popup(xdg_popup)
            self.send(&message(layer, 5, &popup.to_le_bytes()))?;
        }
        if spec.grab {
            let seat = self.seat.ok_or("the seat is not watched")?;
            let serial = self.last_input_serial.ok_or("no input to grab with")?;
            // xdg_popup.grab(seat、serial)
            self.send(&message(
                popup,
                1,
                &[seat.to_le_bytes(), serial.to_le_bytes()].concat(),
            ))?;
        }
        self.send(&message(surface, 6, &[]))?;
        self.popups.push(Popup {
            surface,
            xdg_surface,
            popup,
            spec,
            size: None,
            pools: Vec::new(),
            done: false,
            frame: None,
        });
        Ok(self.popups.len())
    }

    /// `spec` の大きさと向きで、anchor rect の左上が (`x`, `y`) の positioner を作る。
    fn positioner(&mut self, spec: PopupSpec, x: i32, y: i32) -> Result<u32, String> {
        let wm_base = self.wm_base()?;
        let positioner = self.new_id();
        self.send(&message(wm_base, 1, &positioner.to_le_bytes()))?;
        let values = |values: &[i32]| {
            values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>()
        };
        self.send(&message(positioner, 1, &values(&[spec.width, spec.height])))?;
        self.send(&message(positioner, 2, &values(&[x, y, 1, 1])))?;
        self.send(&message(positioner, 3, &spec.anchor.to_le_bytes()))?;
        self.send(&message(positioner, 4, &spec.gravity.to_le_bytes()))?;
        self.send(&message(positioner, 5, &spec.adjust.to_le_bytes()))?;
        Ok(positioner)
    }

    /// ポップアップを作るのに使う xdg_wm_base。layer surface のクライアントでは、初めて使うときに bind する
    /// (layer surface の手順は、xdg_wm_base を bind しない)。
    fn wm_base(&mut self) -> Result<u32, String> {
        if self.layer_surface.is_none() {
            return Ok(WM_BASE);
        }
        match self.layer_wm_base {
            Some(id) => Ok(id),
            None => {
                let id = self.bind("xdg_wm_base", 3)?;
                self.layer_wm_base = Some(id);
                Ok(id)
            }
        }
    }

    /// ポップアップ `index`(1 から)の anchor rect の左上を (`x`, `y`) へ変えるよう頼む(xdg_popup.reposition)。
    pub fn reposition_popup(
        &mut self,
        index: usize,
        x: i32,
        y: i32,
        token: u32,
    ) -> Result<(), String> {
        let (popup, spec) = {
            let popup = self
                .popups
                .get(index.wrapping_sub(1))
                .ok_or("no such popup")?;
            (popup.popup, popup.spec)
        };
        let positioner = self.positioner(spec, x, y)?;
        self.send(&message(
            popup,
            2,
            &[positioner.to_le_bytes(), token.to_le_bytes()].concat(),
        ))?;
        self.send(&message(positioner, 0, &[]))
    }

    /// ポップアップ `index`(1 から)を消す(xdg_popup・xdg_surface・wl_surface の destroy)。
    pub fn close_popup(&mut self, index: usize) -> Result<(), String> {
        let (popup, xdg_surface, surface) = {
            let popup = self
                .popups
                .get(index.wrapping_sub(1))
                .ok_or("no such popup")?;
            (popup.popup, popup.xdg_surface, popup.surface)
        };
        self.send(&message(popup, 0, &[]))?;
        self.send(&message(xdg_surface, 0, &[]))?;
        self.send(&message(surface, 0, &[]))
    }

    /// 面 `surface` を、書き出すときの名前にする(ウィンドウなら空、ポップアップなら ` popup N`)。
    fn surface_name(&self, surface: u32) -> String {
        self.popups
            .iter()
            .position(|popup| popup.surface == surface)
            .map(|index| format!(" popup {}", index + 1))
            .unwrap_or_default()
    }

    /// ポップアップのイベントを扱う。扱ったらtrue。
    fn answer_popup(&mut self, event: &Event) -> Result<bool, String> {
        let Some(index) = self.popups.iter().position(|popup| {
            event.object == popup.popup
                || event.object == popup.xdg_surface
                || popup
                    .frame
                    .is_some_and(|(callback, ..)| callback == event.object)
        }) else {
            return Ok(false);
        };
        let n = index + 1;
        let int = |at: usize| event.u32_at(at).unwrap_or(0) as i32;
        // wl_callback.done:コンポジタが、その絵を描いた。
        if let Some((callback, w, h)) = self.popups[index].frame {
            if event.object == callback {
                self.popups[index].frame = None;
                self.say(&format!("popup {n} drawn {w}x{h}"));
                return Ok(true);
            }
        }
        if event.object == self.popups[index].popup {
            match event.opcode {
                // xdg_popup.configure(x、y、幅、高さ)
                0 => {
                    self.popups[index].size = Some((int(8), int(12)));
                    self.say(&format!(
                        "popup {n} configure {} {} {} {}",
                        int(0),
                        int(4),
                        int(8),
                        int(12)
                    ));
                }
                // xdg_popup.popup_done:書いてから、ふつうのクライアントと同じく、すぐ消す。
                1 => {
                    self.popups[index].done = true;
                    self.say(&format!("popup {n} done"));
                    self.close_popup(n)?;
                }
                // xdg_popup.repositioned(token)
                2 => self.say(&format!("popup {n} repositioned {}", int(0))),
                _ => {}
            }
            return Ok(true);
        }
        // xdg_surface.configure(serial):応じて、届いた大きさの絵を出す。
        if event.opcode == 0 {
            let xdg_surface = self.popups[index].xdg_surface;
            self.send(&message(xdg_surface, 4, &event.body[..4]))?;
            if let (Some((w, h)), false) = (self.popups[index].size, self.popups[index].done) {
                if w > 0 && h > 0 {
                    let (buffer, pool) = self.new_buffer(w, h, self.popups[index].spec.pixel)?;
                    let surface = self.popups[index].surface;
                    self.popups[index].pools.push(pool);
                    let mut attach = buffer.to_le_bytes().to_vec();
                    attach.extend([0i32.to_le_bytes(), 0i32.to_le_bytes()].concat());
                    let damage: Vec<u8> =
                        [0, 0, w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
                    self.send(&message(surface, 1, &attach))?;
                    self.send(&message(surface, 2, &damage))?;
                    // wl_surface.frame(新しい番号)
                    let callback = self.new_id();
                    self.send(&message(surface, 3, &callback.to_le_bytes()))?;
                    self.popups[index].frame = Some((callback, w, h));
                    self.send(&message(surface, 6, &[]))?;
                }
            }
        }
        Ok(true)
    }

    /// `event` が、ポインターのボタンが押されたイベントなら、その通し番号(serial)。
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

    /// 通し番号が `serial` のボタンを押したまま、辺 `edges`(xdg_toplevel の resize_edge の値)で
    /// サイズ変更してほしいと頼む(xdg_toplevel.resize)。
    pub fn request_resize(&mut self, serial: u32, edges: u32) -> Result<(), String> {
        let seat = self.seat.ok_or("the pointer is not watched")?;
        self.send(&message(
            TOPLEVEL,
            6,
            &[
                seat.to_le_bytes(),
                serial.to_le_bytes(),
                edges.to_le_bytes(),
            ]
            .concat(),
        ))
    }

    /// `width` × `height` の、色が `pixel` の新しいバッファ(と、そのための pool)を作る。返すのは、
    /// バッファの番号と pool(memfd。捨てると閉じる)。
    pub fn new_buffer(
        &mut self,
        width: i32,
        height: i32,
        pixel: [u8; 4],
    ) -> Result<(u32, File), String> {
        let stride = width * 4;
        let pool_file = make_pool(width, height, pixel)?;
        let pool = self.new_id();
        self.send_with_fd(
            &message(
                SHM,
                0,
                &[pool.to_le_bytes(), (stride * height).to_le_bytes()].concat(),
            ),
            pool_file.as_raw_fd(),
        )?;
        let buffer = self.new_id();
        let mut create_buffer = buffer.to_le_bytes().to_vec();
        for value in [0, width, height, stride] {
            create_buffer.extend(value.to_le_bytes());
        }
        create_buffer.extend(FORMAT_XRGB8888.to_le_bytes());
        self.send(&message(pool, 0, &create_buffer))?;
        Ok((buffer, pool_file))
    }

    /// `width` × `height` の、ピクセルが `pixels`(B, G, R, X)の新しいバッファ(と、そのための pool)を、shm の
    /// オブジェクト `shm` で作る。返すのは、バッファの番号と pool(memfd。捨てると閉じる)。
    pub fn new_buffer_from(
        &mut self,
        shm: u32,
        width: i32,
        height: i32,
        pixels: &[u8],
    ) -> Result<(u32, File), String> {
        let stride = width * 4;
        let pool_file = make_pool_from(pixels)?;
        let pool = self.new_id();
        self.send_with_fd(
            &message(
                shm,
                0,
                &[pool.to_le_bytes(), (stride * height).to_le_bytes()].concat(),
            ),
            pool_file.as_raw_fd(),
        )?;
        let buffer = self.new_id();
        let mut create_buffer = buffer.to_le_bytes().to_vec();
        for value in [0, width, height, stride] {
            create_buffer.extend(value.to_le_bytes());
        }
        create_buffer.extend(FORMAT_XRGB8888.to_le_bytes());
        self.send(&message(pool, 0, &create_buffer))?;
        Ok((buffer, pool_file))
    }

    /// タイトルを付ける(xdg_toplevel.set_title)。commitすると、コンポジタに伝わる。
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

    /// 返さなかった ping に、いま pong を返す。返したらtrue(返していない ping が無ければfalse)。
    pub fn pong(&mut self) -> Result<bool, String> {
        let Some(serial) = self.unanswered_ping.take() else {
            return Ok(false);
        };
        self.send(&message(WM_BASE, 3, &serial.to_le_bytes()))?;
        Ok(true)
    }

    /// 返事の要るイベント(ping、configure)に返事をする。届いたイベントの中身も、求められていれば書く。
    pub fn answer(&mut self, event: &Event) -> Result<(), String> {
        // layer surface を作ったときは、xdg のオブジェクトは無い(番号が重なるので、先に見る)。キーボードと
        // ポインターのイベントだけは、ウィンドウと同じに扱う。
        let input = Some(event.object) == self.keyboard || Some(event.object) == self.pointer;
        if let (Some(layer), false) = (self.layer_surface, input) {
            if self.answer_popup(event)? {
                return Ok(());
            }
            return match (event.object, event.opcode) {
                // zwlr_layer_surface_v1.configure(serial、幅、高さ)→ ack_configure
                (object, 0) if object == layer => {
                    let (width, height) =
                        (event.u32_at(4).unwrap_or(0), event.u32_at(8).unwrap_or(0));
                    self.layer_size = Some((width, height));
                    if self.print_configures {
                        println!("layer: configure {width}x{height}");
                        let _ = std::io::stdout().flush();
                    }
                    self.send(&message(layer, 6, &event.body[..4]))
                }
                // zwlr_layer_surface_v1.closed
                (object, 1) if object == layer => {
                    self.close_requested = true;
                    if self.print_configures {
                        println!("layer: closed");
                        let _ = std::io::stdout().flush();
                    }
                    Ok(())
                }
                // xdg_wm_base.ping → pong(ポップアップのために bind した xdg_wm_base)
                (object, 0) if Some(object) == self.layer_wm_base => {
                    self.send(&message(object, 3, &event.body[..4]))
                }
                _ => Ok(()),
            };
        }
        if self.answer_popup(event)? {
            return Ok(());
        }
        match (event.object, event.opcode) {
            // xdg_wm_base.ping → pong(返さない形では、`window: ping` を書いて、serial を残す)
            (WM_BASE, 0) if self.ignore_ping => {
                self.unanswered_ping = event.u32_at(0);
                self.say("ping");
                Ok(())
            }
            (WM_BASE, 0) => self.send(&message(WM_BASE, 3, &event.body[..4])),
            // xdg_surface.configure → ack_configure
            (XDG_SURFACE, 0) => self.send(&message(XDG_SURFACE, 4, &event.body[..4])),
            // xdg_toplevel.configure(幅、高さ、状態の並び)
            (TOPLEVEL, 0) => {
                self.toplevel_size =
                    Some((event.u32_at(0).unwrap_or(0), event.u32_at(4).unwrap_or(0)));
                // 状態の配列に maximized(1)があるか。
                let count = event.u32_at(8).unwrap_or(0) as usize / 4;
                self.toplevel_maximized = (0..count).any(|i| event.u32_at(12 + i * 4) == Some(1));
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
            // wl_pointer のイベント。位置は、コンテンツの左上を原点にしたピクセル(端数は切り捨て)。
            (object, opcode) if Some(object) == self.pointer => {
                let fixed = |at: usize| event.u32_at(at).unwrap_or(0) as i32 >> 8;
                match opcode {
                    0 => {
                        let name = self.surface_name(event.u32_at(4).unwrap_or(0));
                        self.say(&format!("pointer enter{name} {} {}", fixed(8), fixed(12)))
                    }
                    1 => self.say("pointer leave"),
                    2 => self.say(&format!("pointer motion {} {}", fixed(4), fixed(8))),
                    3 => {
                        let state = match event.u32_at(12) {
                            Some(1) => "pressed",
                            _ => "released",
                        };
                        let button = event.u32_at(8).unwrap_or(0);
                        if state == "pressed" {
                            self.last_input_serial = event.u32_at(0);
                        }
                        self.say(&format!("pointer button {button:#x} {state}"));
                    }
                    _ => {}
                }
                Ok(())
            }
            // wl_keyboard のイベント。keymap の fd は読まない(大きさだけを書く)。
            (object, opcode) if Some(object) == self.keyboard => {
                let at = |index: usize| event.u32_at(index * 4).unwrap_or(0);
                match opcode {
                    0 => self.say(&format!("keymap {}", at(1))),
                    // 押されているキーがあれば、その番号を添える(`keyboard enter keys 42`)。
                    1 => {
                        let keys: Vec<String> = (0..at(2) as usize / 4)
                            .map(|i| at(3 + i).to_string())
                            .collect();
                        let name = self.surface_name(at(1));
                        if keys.is_empty() {
                            self.say(&format!("keyboard enter{name}"));
                        } else {
                            self.say(&format!("keyboard enter{name} keys {}", keys.join(" ")));
                        }
                    }
                    2 => self.say("keyboard leave"),
                    3 => {
                        let state = if at(3) == 1 { "pressed" } else { "released" };
                        if at(3) == 1 {
                            self.last_input_serial = Some(at(0));
                        }
                        self.say(&format!("key {} {state}", at(2)));
                    }
                    4 => self.say(&format!(
                        "modifiers {} {} {} {}",
                        at(1),
                        at(2),
                        at(3),
                        at(4)
                    )),
                    _ => {}
                }
                Ok(())
            }
            // zxdg_toplevel_decoration_v1.configure(飾りを、どちらの側で描くか)
            (object, 0) if Some(object) == self.decoration => {
                if self.print_decoration {
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

/// memfd を作り、バイト `bytes` を書く。
pub fn make_pool_from(bytes: &[u8]) -> Result<File, String> {
    // SAFETY: 名前は終端のある文字列。返ってきたfdは、このFileだけが持つ。
    let fd = unsafe { libc::memfd_create(c"seinas-test".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)
        .map_err(|e| format!("cannot fill the pool: {e}"))?;
    Ok(file)
}

/// memfdを作り、単色の絵で埋める。
///
/// MFD_ALLOW_SEALINGを付けないので、このmemfdは封印できず、後から縮められる。
pub fn make_pool(width: i32, height: i32, pixel: [u8; 4]) -> Result<File, String> {
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
        decoration_manager: None,
        print_decoration: false,
        globals: Vec::new(),
        seat: None,
        pointer: None,
        keyboard: None,
        close_requested: false,
        ignore_ping: false,
        unanswered_ping: None,
        layer_surface: None,
        layer_size: None,
        toplevel_size: None,
        toplevel_maximized: false,
        keep_fds: false,
        fds: VecDeque::new(),
        popups: Vec::new(),
        last_input_serial: None,
        layer_wm_base: None,
    })
}

/// 画面(サーフェス)にバッファを付け、全面をdamageにして、frameコールバック `callback` を頼んでcommitする。
pub fn commit_buffer(
    wire: &mut Wire,
    width: i32,
    height: i32,
    callback: u32,
) -> Result<(), String> {
    commit_buffer_with(wire, BUFFER, width, height, callback)
}

/// [`commit_buffer`] と同じだが、付けるバッファの番号を `buffer` にする。
pub fn commit_buffer_with(
    wire: &mut Wire,
    buffer: u32,
    width: i32,
    height: i32,
    callback: u32,
) -> Result<(), String> {
    let mut attach = buffer.to_le_bytes().to_vec();
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

/// [`open_window`] と同じだが、タイトルなどを `setup` で指定する。
///
/// 飾りの描き方を尋ねるときは、どちらの側で描くとも頼まず、コンポジタの答えを聞くだけにする。
pub fn open_window_with(
    wire: &mut Wire,
    width: i32,
    height: i32,
    pixel: [u8; 4],
    setup: &WindowSetup,
) -> Result<File, String> {
    // グローバルの一覧を受け取る。syncの返事(wl_callback.done)が、一覧の終わりの通知。
    wire.send(&message(WL_DISPLAY, 1, &REGISTRY.to_le_bytes()))?;
    wire.send(&message(WL_DISPLAY, 0, &SYNC.to_le_bytes()))?;
    let mut globals: Vec<(u32, String)> = Vec::new();
    loop {
        let event = wire.wait_for("the globals", |e| matches!(e.object, REGISTRY | SYNC))?;
        if event.object == SYNC {
            break;
        }
        if event.opcode == 0 {
            // wl_registry.global(番号、インターフェースの名前、バージョン)
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
    // バージョン5から、コンポジタができることの並び(wm_capabilities)が届く。
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
    for (size, opcode) in [(setup.max_size, 7), (setup.min_size, 8)] {
        if let Some((w, h)) = size {
            wire.send(&message(
                TOPLEVEL,
                opcode,
                &[w.to_le_bytes(), h.to_le_bytes()].concat(),
            ))?;
        }
    }
    // プールとバッファを作る。プールのfdは、要求と一緒に送る。番号を順に使うため、装飾のオブジェクトより先に作る
    // (バッファを画面に付けるのは、最初の configure の後)。縁があれば、縁を含めた大きさの絵にする。
    let (content_w, content_h, margin) = (width, height, setup.margin.max(0));
    let (width, height) = (content_w + 2 * margin, content_h + 2 * margin);
    let stride = width * 4;
    let pool_size = stride * height;
    let pool = if margin > 0 {
        let pixels: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let inside = (margin..margin + content_w).contains(&x)
                    && (margin..margin + content_h).contains(&y);
                if inside {
                    pixel
                } else {
                    MARGIN_PIXEL
                }
            })
            .collect();
        make_pool_from(&pixels)?
    } else {
        make_pool(width, height, pixel)?
    };
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

    if margin > 0 {
        // xdg_surface.set_window_geometry(x, y, 幅, 高さ):縁を除いた所がウィンドウ。
        wire.send(&message(
            XDG_SURFACE,
            3,
            &[margin, margin, content_w, content_h]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>(),
        ))?;
    }
    // 装飾の取り決め(xdg-decoration)は、最初の commit の前に始める(決まりで勧められている形)。
    wire.print_decoration = setup.print_decoration;
    if let Some(mode) = setup.decoration.mode() {
        let manager = wire.new_id();
        wire.send(&bind("zxdg_decoration_manager_v1", 1, manager)?)?;
        wire.decoration_manager = Some(manager);
        wire.ask_decoration(mode)?;
    }
    wire.commit()?;
    wire.wait_for("the first configure", |e| {
        (e.object, e.opcode) == (XDG_SURFACE, 0)
    })?;

    // バッファを出し、frameコールバックが届くまで待つ(コンポジタが描いた通知)。
    let first_frame = wire.new_id();
    commit_buffer(wire, width, height, first_frame)?;
    wire.wait_for("the first frame", |e| e.object == first_frame)?;
    wire.globals = globals;
    Ok(pool)
}

/// layer surface の指定(wlr-layer-shell)。
pub struct LayerSetup {
    /// 層。0 が background、1 が bottom、2 が top、3 が overlay。
    pub layer: u32,
    /// anchor のビット(上 1、下 2、左 4、右 8)。
    pub anchor: u32,
    /// 頼む大きさ。0 は「コンポジタに任せる」(anchor した向きいっぱい)。
    pub width: u32,
    pub height: u32,
    pub exclusive_zone: i32,
    /// margin(上、右、下、左)。
    pub margin: [i32; 4],
    pub namespace: String,
    /// 色(B, G, R, X)。
    pub pixel: [u8; 4],
    /// keyboard_interactivity(0 は none、1 は exclusive、2 は on_demand)。
    pub keyboard_interactivity: u32,
}

/// layer surface を 1 つ出し、コンポジタが描くまで待つ。返すのは、絵を置いた pool(memfd)。
///
/// コンポジタが zwlr_layer_shell_v1 を見せていなければ、失敗する(シェルのソケットからつないでいない場合など)。
pub fn open_layer(wire: &mut Wire, setup: &LayerSetup) -> Result<File, String> {
    // 新しいオブジェクトの番号は、間を空けずに順に使う決まりなので、この手順だけの番号を使う。
    const LAYER_SHELL: u32 = 6;
    const LAYER_SURFACE: u32 = 8;
    const LAYER_POOL: u32 = 9;
    const LAYER_BUFFER: u32 = 10;

    wire.send(&message(WL_DISPLAY, 1, &REGISTRY.to_le_bytes()))?;
    wire.send(&message(WL_DISPLAY, 0, &SYNC.to_le_bytes()))?;
    let mut globals: Vec<(u32, String)> = Vec::new();
    loop {
        let event = wire.wait_for("the globals", |e| matches!(e.object, REGISTRY | SYNC))?;
        if event.object == SYNC {
            break;
        }
        if event.opcode == 0 {
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
    wire.send(&bind("zwlr_layer_shell_v1", 4, LAYER_SHELL)?)?;

    // layer surface を作り、最初の configure を待つ。
    wire.send(&message(COMPOSITOR, 0, &SURFACE.to_le_bytes()))?;
    let mut get = LAYER_SURFACE.to_le_bytes().to_vec();
    get.extend(SURFACE.to_le_bytes());
    // output は指定しない(null)。
    get.extend(0u32.to_le_bytes());
    get.extend(setup.layer.to_le_bytes());
    get.extend(string(&setup.namespace));
    wire.send(&message(LAYER_SHELL, 0, &get))?;
    wire.layer_surface = Some(LAYER_SURFACE);
    wire.send(&message(
        LAYER_SURFACE,
        0,
        &[setup.width.to_le_bytes(), setup.height.to_le_bytes()].concat(),
    ))?;
    wire.send(&message(LAYER_SURFACE, 1, &setup.anchor.to_le_bytes()))?;
    wire.send(&message(
        LAYER_SURFACE,
        2,
        &setup.exclusive_zone.to_le_bytes(),
    ))?;
    let margin: Vec<u8> = setup.margin.iter().flat_map(|m| m.to_le_bytes()).collect();
    wire.send(&message(LAYER_SURFACE, 3, &margin))?;
    // zwlr_layer_surface_v1.set_keyboard_interactivity
    wire.send(&message(
        LAYER_SURFACE,
        4,
        &setup.keyboard_interactivity.to_le_bytes(),
    ))?;
    wire.commit()?;
    wire.wait_for("the first configure of the layer surface", |e| {
        (e.object, e.opcode) == (LAYER_SURFACE, 0)
    })?;
    // コンポジタが決めた大きさ。0 なら、頼んだ大きさ。
    let (configured_w, configured_h) = wire.layer_size.unwrap_or((0, 0));
    let width = if configured_w > 0 {
        configured_w
    } else {
        setup.width
    } as i32;
    let height = if configured_h > 0 {
        configured_h
    } else {
        setup.height
    } as i32;
    if width <= 0 || height <= 0 {
        return Err(format!("no size for the layer surface ({width}x{height})"));
    }

    let stride = width * 4;
    let pool = make_pool(width, height, setup.pixel)?;
    wire.send_with_fd(
        &message(
            SHM,
            0,
            &[LAYER_POOL.to_le_bytes(), (stride * height).to_le_bytes()].concat(),
        ),
        pool.as_raw_fd(),
    )?;
    let mut create_buffer = LAYER_BUFFER.to_le_bytes().to_vec();
    for value in [0, width, height, stride] {
        create_buffer.extend(value.to_le_bytes());
    }
    create_buffer.extend(FORMAT_XRGB8888.to_le_bytes());
    wire.send(&message(LAYER_POOL, 0, &create_buffer))?;

    wire.next_id = LAYER_BUFFER + 1;
    let first_frame = wire.new_id();
    commit_buffer_with(wire, LAYER_BUFFER, width, height, first_frame)?;
    wire.wait_for("the first frame", |e| e.object == first_frame)?;
    wire.globals = globals;
    Ok(pool)
}
