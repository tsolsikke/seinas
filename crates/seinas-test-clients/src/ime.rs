//! テスト用: 小さな IME(input-method-v2 と virtual-keyboard のクライアント)。シェルの部品のためのソケットへ
//! つなぐこと(input-method と virtual-keyboard の global は、そこからつないだクライアントにだけ見える)。
//!
//! 動き:
//!
//! - keyboard を grab し、grab で届いた keymap を、そのまま virtual-keyboard の keymap にする。
//! - IME が有効(activate を受けた)なら、`a`・`i`・`u` を、変換中の文字(preedit)の「あ」「い」「う」に足して
//!   送る。変換中の文字があるときの Enter で、それを確定する(commit_string)。
//! - ほかのキー(IME が有効でないときは、全部のキー)と修飾キーの状態は、virtual-keyboard でアプリへ戻す。
//!
//! 引数:
//!
//! - `--popup WxH`: 大きさ WxH の候補の窓(input popup surface)を、紫(8000ff)の単色で出す。コンポジタから届いた
//!   カーソルの矩形を `ime: popup rectangle X Y W H` の形で書く。
//! - `--popup-text`: 候補の窓に、候補(変換中の文字と、そのカタカナ)を白い文字で書く(スクリーンショットのため)。
//! - `--ignore-keys`: キーを受けても、何も返さない(応答しない IME のまね)。標準入力の `answer` の行で、ふつうの
//!   動きに戻り、input-method の commit を 1 つ送る。
//!
//! 標準入力の行: `release`(keyboard の grab を手放す)、`quit`(接続を切って終わる)、`answer`(上)、
//! `sync`(それまでの頼みをコンポジタが処理し終えたら `ime: synced` を書く)。
//!
//! 標準出力には、次の行を書く。
//!
//! - `ime: ready`: grab を取り、virtual-keyboard の keymap を送り終えた。
//! - `ime: unavailable`: コンポジタに断られた(このときは、これを書いて終わる)。
//! - `ime: activate`、`ime: deactivate`、`ime: done`: input-method のイベントが届いた。
//! - `ime: key 30 pressed`(または `released`): grab でキーが届いた。
//! - `ime: preedit あい`、`ime: commit あい`: 送った変換中の文字と、確定した文字。
//! - `ime: released`、`ime: answered`、`ime: synced`: 標準入力の行を処理した。

use std::{
    collections::HashSet,
    io::{BufRead, Write},
    os::fd::AsRawFd,
    sync::mpsc,
};

use crate::{
    message, string,
    wire::{connect, protocol_error, Polled},
    WL_DISPLAY,
};

/// evdev のキーの番号。
const KEY_A: u32 = 30;
const KEY_I: u32 = 23;
const KEY_U: u32 = 22;
const KEY_ENTER: u32 = 28;

fn say(text: &str) {
    println!("ime: {text}");
    let _ = std::io::stdout().flush();
}

/// キー `key` で変換中の文字に足すかな。
fn kana(key: u32) -> Option<char> {
    match key {
        KEY_A => Some('あ'),
        KEY_I => Some('い'),
        KEY_U => Some('う'),
        _ => None,
    }
}

/// 接続を切られるか、`quit` の行が届くまで動く。断られたら(unavailable)falseを返す。
pub fn run(args: &[String]) -> Result<bool, String> {
    let mut ignore_keys = false;
    let mut popup_size = None;
    let mut popup_text = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ignore-keys" => ignore_keys = true,
            "--popup-text" => popup_text = true,
            "--popup" => {
                let size = args.next().ok_or("--popup needs WxH")?;
                popup_size = Some(crate::layer::parse_size(size)?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let mut wire = connect()?;
    wire.keep_fds = true;
    wire.list_globals()?;
    wire.watch_seat(false, false)?;
    let seat = wire.seat().ok_or("no seat")?;
    let manager = wire.bind("zwp_input_method_manager_v2", 1)?;
    let virtual_manager = wire.bind("zwp_virtual_keyboard_manager_v1", 1)?;
    // zwp_input_method_manager_v2.get_input_method(seat、新しい番号)
    let input_method = wire.new_id();
    wire.send(&message(
        manager,
        0,
        &[seat.to_le_bytes(), input_method.to_le_bytes()].concat(),
    ))?;
    // zwp_virtual_keyboard_manager_v1.create_virtual_keyboard(seat、新しい番号)
    let keyboard = wire.new_id();
    wire.send(&message(
        virtual_manager,
        0,
        &[seat.to_le_bytes(), keyboard.to_le_bytes()].concat(),
    ))?;
    // zwp_input_method_v2.grab_keyboard(新しい番号)
    let grab = wire.new_id();
    wire.send(&message(input_method, 5, &grab.to_le_bytes()))?;
    // 候補の窓の pool は、出している間、持ち続ける。
    let (popup, mut popup_view) = match popup_size {
        Some((width, height)) => {
            let (popup, view) = open_popup(
                &mut wire,
                input_method,
                width as i32,
                height as i32,
                popup_text,
            )?;
            (Some(popup), Some(view))
        }
        None => (None, None),
    };

    let (sender, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    // 届いた done の数(input-method の commit に添える serial)。
    let mut serial: u32 = 0;
    // activate・deactivate は、done で効く。
    let (mut active, mut pending_active) = (false, false);
    let mut preedit = String::new();
    // IME が使ったキー(離したことも、アプリへ戻さない)。
    let mut used: HashSet<u32> = HashSet::new();
    // ready を書くための sync の番号と、標準入力の `sync` の番号。
    let mut ready_sync = None;
    let mut sync = None;
    let mut released = false;

    loop {
        let event = match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Nothing => None,
            Polled::Event(event) => Some(event),
        };
        if let Some(event) = event {
            if let Some(error) = protocol_error(&event) {
                return Err(format!("protocol error: {error}"));
            }
            match (event.object, event.opcode) {
                (object, _) if Some(object) == ready_sync => {
                    ready_sync = None;
                    say("ready");
                }
                (object, _) if Some(object) == sync => {
                    sync = None;
                    say("synced");
                }
                // zwp_input_popup_surface_v2.text_input_rectangle(x、y、幅、高さ)
                (object, 0) if Some(object) == popup => {
                    let at = |index: usize| event.u32_at(index * 4).unwrap_or(0) as i32;
                    say(&format!(
                        "popup rectangle {} {} {} {}",
                        at(0),
                        at(1),
                        at(2),
                        at(3)
                    ));
                }
                (object, opcode) if object == input_method => match opcode {
                    0 => {
                        pending_active = true;
                        say("activate");
                    }
                    1 => {
                        pending_active = false;
                        say("deactivate");
                    }
                    5 => {
                        serial += 1;
                        if active && !pending_active {
                            preedit.clear();
                        }
                        active = pending_active;
                        say("done");
                    }
                    6 => {
                        say("unavailable");
                        return Ok(false);
                    }
                    _ => {}
                },
                // keymap(形式、fd、大きさ):同じものを virtual-keyboard の keymap にする。
                (object, 0) if object == grab => {
                    let format = event.u32_at(0).ok_or("a broken keymap")?;
                    let size = event.u32_at(4).ok_or("a broken keymap")?;
                    let fd = wire.fds.pop_front().ok_or("the keymap came without a fd")?;
                    wire.send_with_fd(
                        &message(
                            keyboard,
                            0,
                            &[format.to_le_bytes(), size.to_le_bytes()].concat(),
                        ),
                        fd.as_raw_fd(),
                    )?;
                    let id = wire.new_id();
                    wire.send(&message(WL_DISPLAY, 0, &id.to_le_bytes()))?;
                    ready_sync = Some(id);
                }
                // key(serial、時刻、キー、状態)
                (object, 1) if object == grab => {
                    let time = event.u32_at(4).unwrap_or(0);
                    let key = event.u32_at(8).unwrap_or(0);
                    let pressed = event.u32_at(12) == Some(1);
                    say(&format!(
                        "key {key} {}",
                        if pressed { "pressed" } else { "released" }
                    ));
                    if ignore_keys {
                        continue;
                    }
                    if !pressed && used.remove(&key) {
                        continue;
                    }
                    if pressed && active {
                        if let Some(kana) = kana(key) {
                            preedit.push(kana);
                            send_preedit(&mut wire, input_method, &preedit, serial)?;
                            if let Some(view) = &mut popup_view {
                                view.show(&mut wire, &preedit)?;
                            }
                            say(&format!("preedit {preedit}"));
                            used.insert(key);
                            continue;
                        }
                        if key == KEY_ENTER && !preedit.is_empty() {
                            // zwp_input_method_v2.commit_string → commit(serial)
                            wire.send(&message(input_method, 0, &string(&preedit)))?;
                            wire.send(&message(input_method, 3, &serial.to_le_bytes()))?;
                            say(&format!("commit {preedit}"));
                            preedit.clear();
                            if let Some(view) = &mut popup_view {
                                view.show(&mut wire, &preedit)?;
                            }
                            used.insert(key);
                            continue;
                        }
                    }
                    // zwp_virtual_keyboard_v1.key(時刻、キー、状態)
                    wire.send(&message(
                        keyboard,
                        1,
                        &[
                            time.to_le_bytes(),
                            key.to_le_bytes(),
                            (pressed as u32).to_le_bytes(),
                        ]
                        .concat(),
                    ))?;
                }
                // modifiers(serial、押されている、ラッチ、ロック、グループ)→ virtual-keyboard の modifiers
                (object, 2) if object == grab && !ignore_keys => {
                    wire.send(&message(keyboard, 2, &event.body[4..20]))?;
                }
                _ => {}
            }
        }
        for command in commands.try_iter() {
            match command.as_str() {
                "quit" => return Ok(true),
                "release" if !released => {
                    // zwp_input_method_keyboard_grab_v2.release
                    wire.send(&message(grab, 0, &[]))?;
                    released = true;
                    say("released");
                }
                "answer" => {
                    ignore_keys = false;
                    wire.send(&message(input_method, 3, &serial.to_le_bytes()))?;
                    say("answered");
                }
                "sync" => {
                    let id = wire.new_id();
                    wire.send(&message(WL_DISPLAY, 0, &id.to_le_bytes()))?;
                    sync = Some(id);
                }
                _ => {}
            }
        }
    }
}

/// 変換中の文字 `text` を送る(カーソルは終わり)。
fn send_preedit(
    wire: &mut crate::wire::Wire,
    input_method: u32,
    text: &str,
    serial: u32,
) -> Result<(), String> {
    let end = text.len() as i32;
    let mut body = string(text);
    body.extend(end.to_le_bytes());
    body.extend(end.to_le_bytes());
    // zwp_input_method_v2.set_preedit_string → commit(serial)
    wire.send(&message(input_method, 1, &body))?;
    wire.send(&message(input_method, 3, &serial.to_le_bytes()))
}

/// 候補の窓の色(B, G, R, X。紫)。
const POPUP_PIXEL: [u8; 4] = [0xff, 0x00, 0x80, 0xff];

/// 候補の窓の絵と、描き直すためのもの。
struct PopupView {
    surface: u32,
    shm: u32,
    width: i32,
    height: i32,
    /// 文字を描くときだけ(`--popup-text`)。
    painter: Option<seinas_text::TextPainter>,
    /// 出した絵の pool(コンポジタが使い終わるまで持つ)。
    pools: Vec<std::fs::File>,
}

impl PopupView {
    /// 候補(変換中の文字 `preedit` と、そのカタカナ)を書いた絵を出す。文字を描かないときは、何もしない。
    fn show(&mut self, wire: &mut crate::wire::Wire, preedit: &str) -> Result<(), String> {
        let Some(painter) = &mut self.painter else {
            return Ok(());
        };
        let purple = [POPUP_PIXEL[2], POPUP_PIXEL[1], POPUP_PIXEL[0]];
        let mut pixels = crate::text::canvas(self.width, self.height, purple);
        if !preedit.is_empty() {
            let katakana: String = preedit
                .chars()
                .map(|c| char::from_u32(c as u32 + 0x60).unwrap_or(c))
                .collect();
            for (i, candidate) in [preedit.to_owned(), katakana].iter().enumerate() {
                let line = format!("{} {candidate}", i + 1);
                let top = 4 + i as i32 * 24;
                crate::text::draw(
                    painter,
                    &mut pixels,
                    self.width,
                    &line,
                    8,
                    top,
                    24,
                    [0xff; 3],
                );
            }
        }
        let (buffer, pool) = wire.new_buffer_from(self.shm, self.width, self.height, &pixels)?;
        self.pools.push(pool);
        commit_surface(wire, self.surface, buffer, self.width, self.height)
    }
}

/// `surface` に `buffer` を付け、全体を damage にして commit する。
fn commit_surface(
    wire: &mut crate::wire::Wire,
    surface: u32,
    buffer: u32,
    width: i32,
    height: i32,
) -> Result<(), String> {
    // wl_surface.attach → damage → commit
    wire.send(&message(
        surface,
        1,
        &[buffer.to_le_bytes(), [0; 4], [0; 4]].concat(),
    ))?;
    let damage: Vec<u8> = [0, 0, width, height]
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    wire.send(&message(surface, 2, &damage))?;
    wire.send(&message(surface, 6, &[]))
}

/// 候補の窓(input popup surface)を作り、`width` × `height` の紫の単色の絵を出す。返すのは、popup surface の番号と、
/// 描き直すためのもの。`text` なら、候補の文字を描く(フォントを読む)。
fn open_popup(
    wire: &mut crate::wire::Wire,
    input_method: u32,
    width: i32,
    height: i32,
    text: bool,
) -> Result<(u32, PopupView), String> {
    let compositor = wire.bind("wl_compositor", 1)?;
    let shm = wire.bind("wl_shm", 1)?;
    // wl_compositor.create_surface → zwp_input_method_v2.get_input_popup_surface(新しい番号、surface)
    let surface = wire.new_id();
    wire.send(&message(compositor, 0, &surface.to_le_bytes()))?;
    let popup = wire.new_id();
    wire.send(&message(
        input_method,
        4,
        &[popup.to_le_bytes(), surface.to_le_bytes()].concat(),
    ))?;
    let purple = [POPUP_PIXEL[2], POPUP_PIXEL[1], POPUP_PIXEL[0]];
    let pixels = crate::text::canvas(width, height, purple);
    let (buffer, pool) = wire.new_buffer_from(shm, width, height, &pixels)?;
    commit_surface(wire, surface, buffer, width, height)?;
    Ok((
        popup,
        PopupView {
            surface,
            shm,
            width,
            height,
            painter: text.then(crate::text::painter),
            pools: vec![pool],
        },
    ))
}
