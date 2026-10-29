//! クリップボード(wl_data_device の selection)と、ドラッグ&ドロップ(テスト用)。
//!
//! 扱う mime type は `text/plain;charset=utf-8` だけ。貼り付け・落とされたものの受け取りは、パイプの読み口が閉じる
//! まで待って読む(書くのは、ほかのクライアント。同じクライアントの中でのコピーと貼り付けは、書く前に読み始めて
//! 止まるので、使わない)。
//!
//! 書く行(`window: ` に続けて):
//! - `data selection text/plain;charset=utf-8`(selection が届いた。無いなら `data selection none`)
//! - `data pasted TEXT`(貼り付けた中身)、`data paste nothing`(selection が無い)
//! - `data send text/plain;charset=utf-8`(自分の source から、中身を求められて書いた)
//! - `data dnd enter X Y`、`data dnd leave`、`data dnd drop`、`data dropped TEXT`(落とされたものの中身)
//! - `data target MIME`(相手が受け入れる型。受け入れないなら `none`)、`data action N`、`data drop performed`、
//!   `data finished`、`data cancelled`(自分の source の、ドラッグ&ドロップのイベント)

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

use super::{Event, Wire};
use crate::{message, string};

const TEXT: &str = "text/plain;charset=utf-8";
/// wl_data_device_manager の dnd_action の copy。
const COPY: u32 = 1;

/// data device の状態。
pub(super) struct DataState {
    /// wl_data_device_manager(source を作るのに使う)。
    manager: u32,
    device: u32,
    /// 落とされそうになったとき(enter)、受け入れるか。
    accept_drops: bool,
    /// 届いた data offer と、その mime type。
    offers: HashMap<u32, Vec<String>>,
    /// 今の selection の offer。
    selection: Option<u32>,
    /// ドラッグ&ドロップの最中の offer。
    dnd_offer: Option<u32>,
    /// 自分の source と、その中身。
    sources: HashMap<u32, String>,
}

/// 文字列の引数(null なら None)と、その次の位置。
fn string_at(event: &Event, at: usize) -> (Option<String>, usize) {
    let length = event.u32_at(at).unwrap_or(0) as usize;
    if length == 0 {
        return (None, at + 4);
    }
    let text = event.body.get(at + 4..at + 4 + length - 1).unwrap_or(&[]);
    (
        Some(String::from_utf8_lossy(text).into_owned()),
        at + 4 + length.next_multiple_of(4),
    )
}

/// パイプ(読み口、書き口)。
fn pipe() -> Result<(File, OwnedFd), String> {
    let mut fds = [0; 2];
    // SAFETY: fds は 2 つの i32 の配列で、pipe2 が書く。
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(format!("pipe2 failed: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: pipe2 が作ったばかりの fd で、ほかには持ち主がいない。
    unsafe { Ok((File::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
}

impl Wire {
    /// wl_data_device_manager(バージョン 3)を bind し、seat の data device を作る。`accept_drops` なら、落とされそうに
    /// なったものを受け入れる。届く fd(source への send)を受け取るため、fd を残すようにする。
    pub fn watch_data_device(&mut self, accept_drops: bool) -> Result<(), String> {
        let seat = self.seat.ok_or("the seat is not watched")?;
        let manager = self.bind("wl_data_device_manager", 3)?;
        let device = self.new_id();
        // wl_data_device_manager.get_data_device(新しい番号、seat)
        self.send(&message(
            manager,
            1,
            &[device.to_le_bytes(), seat.to_le_bytes()].concat(),
        ))?;
        self.keep_fds = true;
        self.data = Some(DataState {
            manager,
            device,
            accept_drops,
            offers: HashMap::new(),
            selection: None,
            dnd_offer: None,
            sources: HashMap::new(),
        });
        Ok(())
    }

    /// 中身が `text` の source を作る。
    fn new_source(&mut self, text: &str, actions: bool) -> Result<u32, String> {
        let manager = self.data.as_ref().ok_or("no data device")?.manager;
        let source = self.new_id();
        // wl_data_device_manager.create_data_source(新しい番号)、wl_data_source.offer(mime type)
        self.send(&message(manager, 0, &source.to_le_bytes()))?;
        self.send(&message(source, 0, &string(TEXT)))?;
        if actions {
            // wl_data_source.set_actions(copy)
            self.send(&message(source, 2, &COPY.to_le_bytes()))?;
        }
        if let Some(data) = self.data.as_mut() {
            data.sources.insert(source, text.to_owned());
        }
        Ok(source)
    }

    /// `text` をコピーする(source を作り、直前の入力の serial で set_selection)。
    pub fn copy(&mut self, text: &str) -> Result<(), String> {
        let source = self.new_source(text, false)?;
        let device = self.data.as_ref().ok_or("no data device")?.device;
        let serial = self.last_input_serial.unwrap_or(0);
        self.send(&message(
            device,
            1,
            &[source.to_le_bytes(), serial.to_le_bytes()].concat(),
        ))
    }

    /// 今の selection を貼り付ける(中身を受け取って書く)。
    pub fn paste(&mut self) -> Result<(), String> {
        let offer = self.data.as_ref().and_then(|data| data.selection);
        match offer {
            Some(offer) => {
                let text = self.receive_offer(offer)?;
                self.say(&format!("data pasted {text}"));
            }
            None => self.say("data paste nothing"),
        }
        Ok(())
    }

    /// `text` のドラッグ&ドロップを始める(直前の押下の serial で start_drag。ボタンを押している間に)。
    pub fn drag(&mut self, text: &str) -> Result<(), String> {
        let source = self.new_source(text, true)?;
        let device = self.data.as_ref().ok_or("no data device")?.device;
        let serial = self.last_input_serial.unwrap_or(0);
        // wl_data_device.start_drag(source、origin、icon(無し)、serial)
        self.send(&message(
            device,
            0,
            &[
                source.to_le_bytes(),
                super::SURFACE.to_le_bytes(),
                0u32.to_le_bytes(),
                serial.to_le_bytes(),
            ]
            .concat(),
        ))
    }

    /// offer の中身(テキスト)を受け取る。パイプの書き口を送り、読み口が閉じるまで読む。
    fn receive_offer(&mut self, offer: u32) -> Result<String, String> {
        let (mut reader, writer) = pipe()?;
        // wl_data_offer.receive(mime type、fd)
        self.send_with_fd(&message(offer, 1, &string(TEXT)), writer.as_raw_fd())?;
        drop(writer);
        let mut text = String::new();
        reader
            .read_to_string(&mut text)
            .map_err(|e| format!("cannot read the offer: {e}"))?;
        Ok(text)
    }

    /// data device・data offer・自分の source のイベントなら扱い、true を返す。
    pub(super) fn answer_data(&mut self, event: &Event) -> Result<bool, String> {
        let Some(data) = self.data.as_mut() else {
            return Ok(false);
        };
        let (object, opcode) = (event.object, event.opcode);
        if object == data.device {
            match opcode {
                // data_offer(新しい番号):この後に、offer の mime type が届く。
                0 => {
                    data.offers.insert(event.u32_at(0).unwrap_or(0), Vec::new());
                }
                // enter(serial、surface、x、y、offer)
                1 => {
                    let serial = event.u32_at(0).unwrap_or(0);
                    let fixed = |at: usize| event.u32_at(at).unwrap_or(0) as i32 >> 8;
                    let (x, y) = (fixed(8), fixed(12));
                    let offer = event.u32_at(16).filter(|&id| id != 0);
                    data.dnd_offer = offer;
                    let accept = data.accept_drops;
                    self.say(&format!("data dnd enter {x} {y}"));
                    if let (Some(offer), true) = (offer, accept) {
                        // wl_data_offer.accept(serial、mime type)、set_actions(copy、copy)
                        let mut body = serial.to_le_bytes().to_vec();
                        body.extend(string(TEXT));
                        self.send(&message(offer, 0, &body))?;
                        self.send(&message(
                            offer,
                            4,
                            &[COPY.to_le_bytes(), COPY.to_le_bytes()].concat(),
                        ))?;
                    }
                }
                // leave
                2 => {
                    if let Some(offer) = data.dnd_offer.take() {
                        data.offers.remove(&offer);
                        self.send(&message(offer, 2, &[]))?;
                    }
                    self.say("data dnd leave");
                }
                // drop:中身を受け取り、finish してから消す。
                4 => {
                    let offer = data.dnd_offer.take();
                    if let Some(offer) = offer {
                        data.offers.remove(&offer);
                    }
                    self.say("data dnd drop");
                    if let Some(offer) = offer {
                        let text = self.receive_offer(offer)?;
                        self.say(&format!("data dropped {text}"));
                        self.send(&message(offer, 3, &[]))?;
                        self.send(&message(offer, 2, &[]))?;
                    }
                }
                // selection(offer か null)。前の selection の offer は消す。
                5 => {
                    let offer = event.u32_at(0).filter(|&id| id != 0);
                    if let Some(old) = data.selection.take().filter(|&old| Some(old) != offer) {
                        data.offers.remove(&old);
                        self.send(&message(old, 2, &[]))?;
                    }
                    let data = self.data.as_mut().expect("the data device");
                    data.selection = offer;
                    let mimes = offer
                        .and_then(|offer| data.offers.get(&offer))
                        .map(|mimes| mimes.join(","))
                        .unwrap_or_else(|| "none".to_owned());
                    self.say(&format!("data selection {mimes}"));
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(mimes) = data.offers.get_mut(&object) {
            // wl_data_offer.offer(mime type)。source_actions と action は書かない。
            if opcode == 0 {
                mimes.extend(string_at(event, 0).0);
            }
            return Ok(true);
        }
        if let Some(text) = data.sources.get(&object).cloned() {
            match opcode {
                // target(mime type か null)
                0 => {
                    let mime = string_at(event, 0).0;
                    self.say(&format!(
                        "data target {}",
                        mime.as_deref().unwrap_or("none")
                    ));
                }
                // send(mime type、fd):中身を書いて閉じる。
                1 => {
                    let mime = string_at(event, 0).0.unwrap_or_default();
                    let fd = self.fds.pop_front().ok_or("send came without a fd")?;
                    let mut file = File::from(fd);
                    let _ = file.write_all(text.as_bytes());
                    drop(file);
                    self.say(&format!("data send {mime}"));
                }
                // cancelled:source を消す。
                2 => {
                    self.say("data cancelled");
                    self.forget_source(object)?;
                }
                3 => self.say("data drop performed"),
                // dnd_finished:source を消す。
                4 => {
                    self.say("data finished");
                    self.forget_source(object)?;
                }
                5 => self.say(&format!("data action {}", event.u32_at(0).unwrap_or(0))),
                _ => {}
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn forget_source(&mut self, source: u32) -> Result<(), String> {
        if let Some(data) = self.data.as_mut() {
            data.sources.remove(&source);
        }
        // wl_data_source.destroy
        self.send(&message(source, 1, &[]))
    }
}
