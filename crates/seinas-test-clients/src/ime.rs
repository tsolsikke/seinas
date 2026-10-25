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
//! - `--popup WxH`: 大きさ WxH の候補ウィンドウ(input popup surface)を、紫(8000ff)の単色で出す。コンポジタから届いた
//!   カーソルの矩形を `ime: popup rectangle X Y W H` の形で書く。
//! - `--popup-text`: 候補ウィンドウに、候補(変換中の文字と、そのカタカナ)を白い文字で書く(スクリーンショットのため)。
//! - `--virtual-keyboard-only`: get_input_method を頼まずに virtual-keyboard を作る(認可の無いクライアントのまね)。
//! - `--no-builtin-keymap`: built-in の virtual-keyboard に keymap を送らない(seat に keymap がまだ無いときの確かめ)。
//!   断られたら `ime: protocol error …` を書いて終わる。
//! - `--swallow KEY`: evdev の番号 KEY のキーを、文字を出さずに使う(押下も解放も、何も返さない。半角/全角の
//!   ように、IME の状態だけを変えるキーのまね)。何度でも付けられる。
//!
//! 標準入力の行: `release`(keyboard の grab を手放す)、`quit`(接続を切って終わる)、
//! `sync`(それまでの頼みをコンポジタが処理し終えたら `ime: synced` を書く)、`again`(同じ接続から 2 つ目の
//! get_input_method を頼む)、`again-grab`(2 つ目の input method で keyboard を grab する)、`destroy-input-method`(input method だけを destroy する。grab と virtual-keyboard は
//! 残す)、`vk-press CODE`・`vk-release CODE`・`vk-mods DEPRESSED`(受けたキーと関係なく、virtual-keyboard で
//! キーと修飾キーの状態を送る)、`hold`(この後、キーを受けて送る要求(変換中の文字、確定、virtual-keyboard の
//! キーと修飾キー)を送らずにためる)、`send`(ためた要求を、ためた順に送り、ためるのをやめる)、
//! `commit-string TEXT`(commit_string だけを送り、commit は送らない)、`delete-surrounding BEFORE AFTER`
//! (delete_surrounding_text だけを送り、commit は送らない)、`vk-new NAME`(名前 NAME の virtual-keyboard を
//! もう 1 つ作る)、`vk-keymap NAME`(grab で届いた keymap を送る)、`vk-key NAME CODE press|release`、`vk-mods NAME DEPRESSED`、
//! `vk-destroy NAME`。
//!
//! 標準出力には、次の行を書く。
//!
//! - `ime: ready`: grab を取り、virtual-keyboard の keymap を送り終えた。
//! - `ime: unavailable`: コンポジタに断られた(このときは、これを書いて終わる)。
//! - `ime: activate`、`ime: deactivate`、`ime: done`: input-method のイベントが届いた。
//! - `ime: key 30 pressed`(または `released`): grab でキーが届いた。
//! - `ime: preedit あい`、`ime: commit あい`: 送った変換中の文字と、確定した文字。
//! - `ime: released`、`ime: synced`、`ime: holding`、`ime: sent N`: 標準入力の行を処理した(N は送った要求の数)。
//! - `ime: second unavailable`: `again` の 2 つ目が断られた。`ime: second grabbed`: `again-grab` で、2 つ目の input method
//!   で grab を頼んだ。`ime: second key 30 pressed`(または `released`): 2 つ目の grab でキーが届いた。`ime: input method destroyed`: `destroy-input-method` を送った。
//! - `ime: protocol error …`: コンポジタからプロトコルのエラーが届いた(この後、終わる)。

use std::{
    collections::{HashMap, HashSet},
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
    let mut swallow: HashSet<u32> = HashSet::new();
    let mut popup_size = None;
    let mut popup_text = false;
    let mut virtual_keyboard_only = false;
    // built-in の virtual-keyboard に keymap を送らない(seat に keymap がまだ無いときの確かめ)。
    let mut no_builtin_keymap = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--swallow" => {
                let key = args.next().ok_or("--swallow needs KEY")?;
                swallow.insert(
                    key.parse()
                        .map_err(|_| format!("invalid --swallow value: {key}"))?,
                );
            }
            "--popup-text" => popup_text = true,
            "--virtual-keyboard-only" => virtual_keyboard_only = true,
            "--no-builtin-keymap" => no_builtin_keymap = true,
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
    if virtual_keyboard_only {
        return virtual_keyboard_alone(&mut wire, virtual_manager, seat);
    }
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
    // 候補ウィンドウの pool は、出している間、持ち続ける。
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
    // 標準入力の `again` で、同じ接続から頼んだ 2 つ目の input method と、`again-grab` で取ったその grab。
    let mut second = None;
    let mut second_grab = None;
    // 標準入力の `hold` の後、キーを受けて送る要求をためる所。
    let mut held: Option<Vec<Vec<u8>>> = None;
    // grab で届いた keymap の形式とコンテンツ(`vk-keymap` で、ほかの virtual-keyboard にも送るため)。
    let mut keymap_copy: Option<(u32, Vec<u8>)> = None;
    // `vk-new NAME` で作った virtual-keyboard。
    let mut named: HashMap<String, u32> = HashMap::new();

    loop {
        let event = match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Nothing => None,
            Polled::Event(event) => Some(event),
        };
        if let Some(event) = event {
            if let Some(error) = protocol_error(&event) {
                say(&format!("protocol error {error}"));
                return Err(format!("protocol error: {error}"));
            }
            match (event.object, event.opcode) {
                // 同じ接続から頼んだ 2 つ目の unavailable。
                (object, 6) if Some(object) == second => say("second unavailable"),
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
                    let mut copy = vec![0; size as usize];
                    let file = std::fs::File::from(fd.try_clone().map_err(|e| e.to_string())?);
                    std::os::unix::fs::FileExt::read_exact_at(&file, &mut copy, 0)
                        .map_err(|e| format!("cannot read the keymap: {e}"))?;
                    keymap_copy = Some((format, copy));
                    if !no_builtin_keymap {
                        wire.send_with_fd(
                            &message(
                                keyboard,
                                0,
                                &[format.to_le_bytes(), size.to_le_bytes()].concat(),
                            ),
                            fd.as_raw_fd(),
                        )?;
                    }
                    let id = wire.new_id();
                    wire.send(&message(WL_DISPLAY, 0, &id.to_le_bytes()))?;
                    ready_sync = Some(id);
                }
                // key(serial、時刻、キー、状態)
                // 2 つ目の input method の grab に届いた key(serial、時刻、キー、状態)
                (object, 1) if Some(object) == second_grab => {
                    let key = event.u32_at(8).unwrap_or(0);
                    let pressed = event.u32_at(12) == Some(1);
                    say(&format!(
                        "second key {key} {}",
                        if pressed { "pressed" } else { "released" }
                    ));
                }
                (object, 1) if object == grab => {
                    let time = event.u32_at(4).unwrap_or(0);
                    let key = event.u32_at(8).unwrap_or(0);
                    let pressed = event.u32_at(12) == Some(1);
                    say(&format!(
                        "key {key} {}",
                        if pressed { "pressed" } else { "released" }
                    ));
                    if swallow.contains(&key) {
                        continue;
                    }
                    if !pressed && used.remove(&key) {
                        continue;
                    }
                    if pressed && active {
                        if let Some(kana) = kana(key) {
                            preedit.push(kana);
                            for request in preedit_requests(input_method, &preedit, serial) {
                                out(&mut wire, &mut held, request)?;
                            }
                            if let Some(view) = &mut popup_view {
                                view.show(&mut wire, &preedit)?;
                            }
                            say(&format!("preedit {preedit}"));
                            used.insert(key);
                            continue;
                        }
                        if key == KEY_ENTER && !preedit.is_empty() {
                            // zwp_input_method_v2.commit_string → commit(serial)
                            out(
                                &mut wire,
                                &mut held,
                                message(input_method, 0, &string(&preedit)),
                            )?;
                            out(
                                &mut wire,
                                &mut held,
                                message(input_method, 3, &serial.to_le_bytes()),
                            )?;
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
                    out(
                        &mut wire,
                        &mut held,
                        message(
                            keyboard,
                            1,
                            &[
                                time.to_le_bytes(),
                                key.to_le_bytes(),
                                (pressed as u32).to_le_bytes(),
                            ]
                            .concat(),
                        ),
                    )?;
                }
                // modifiers(serial、押されている、ラッチ、ロック、グループ)→ virtual-keyboard の modifiers
                (object, 2) if object == grab => {
                    out(
                        &mut wire,
                        &mut held,
                        message(keyboard, 2, &event.body[4..20]),
                    )?;
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
                "hold" => {
                    held = Some(Vec::new());
                    say("holding");
                }
                "send" => {
                    let requests = held.take().unwrap_or_default();
                    for request in &requests {
                        wire.send(request)?;
                    }
                    say(&format!("sent {}", requests.len()));
                }
                "sync" => {
                    let id = wire.new_id();
                    wire.send(&message(WL_DISPLAY, 0, &id.to_le_bytes()))?;
                    sync = Some(id);
                }
                "again" => {
                    // 同じ接続から、もう 1 つ get_input_method を頼む。
                    let id = wire.new_id();
                    wire.send(&message(
                        manager,
                        0,
                        &[seat.to_le_bytes(), id.to_le_bytes()].concat(),
                    ))?;
                    second = Some(id);
                }
                "again-grab" => {
                    // 2 つ目の input method で、keyboard を grab する(zwp_input_method_v2.grab_keyboard)。
                    if let Some(second) = second {
                        let id = wire.new_id();
                        wire.send(&message(second, 5, &id.to_le_bytes()))?;
                        second_grab = Some(id);
                        say("second grabbed");
                    }
                }
                "destroy-input-method" => {
                    // zwp_input_method_v2.destroy(grab と virtual-keyboard は残す)
                    wire.send(&message(input_method, 6, &[]))?;
                    say("input method destroyed");
                }
                other => {
                    let words: Vec<&str> = other.split_whitespace().collect();
                    if let ["vk-new", name] = words[..] {
                        // zwp_virtual_keyboard_manager_v1.create_virtual_keyboard(seat、新しい番号)
                        let id = wire.new_id();
                        wire.send(&message(
                            virtual_manager,
                            0,
                            &[seat.to_le_bytes(), id.to_le_bytes()].concat(),
                        ))?;
                        named.insert(name.to_owned(), id);
                    } else if let ["vk-keymap", name] = words[..] {
                        let (Some(&id), Some((format, bytes))) = (named.get(name), &keymap_copy)
                        else {
                            continue;
                        };
                        let file = crate::wire::make_pool_from(bytes)?;
                        let body = [format.to_le_bytes(), (bytes.len() as u32).to_le_bytes()];
                        wire.send_with_fd(&message(id, 0, &body.concat()), file.as_raw_fd())?;
                    } else if let ["vk-key", name, code, what] = words[..] {
                        let (Some(&id), Ok(code)) = (named.get(name), code.parse::<u32>()) else {
                            continue;
                        };
                        let pressed = (what == "press") as u32;
                        let body = [
                            0u32.to_le_bytes(),
                            code.to_le_bytes(),
                            pressed.to_le_bytes(),
                        ];
                        wire.send(&message(id, 1, &body.concat()))?;
                    } else if let ["vk-mods", name, depressed] = words[..] {
                        // zwp_virtual_keyboard_v1.modifiers(押されている、ラッチ、ロック、グループ)
                        let (Some(&id), Ok(depressed)) =
                            (named.get(name), depressed.parse::<u32>())
                        else {
                            continue;
                        };
                        let body: Vec<u8> = [depressed, 0, 0, 0]
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect();
                        wire.send(&message(id, 2, &body))?;
                    } else if let ["vk-destroy", name] = words[..] {
                        if let Some(id) = named.remove(name) {
                            wire.send(&message(id, 3, &[]))?;
                        }
                    } else if let Some(text) = other.strip_prefix("commit-string ") {
                        // zwp_input_method_v2.commit_string(commit は送らない)
                        wire.send(&message(input_method, 0, &string(text)))?;
                    } else if let Some(lengths) = other.strip_prefix("delete-surrounding ") {
                        // zwp_input_method_v2.delete_surrounding_text(commit は送らない)
                        let body: Vec<u8> = lengths
                            .split_whitespace()
                            .filter_map(|n| n.parse::<u32>().ok())
                            .flat_map(|n| n.to_le_bytes())
                            .collect();
                        wire.send(&message(input_method, 2, &body))?;
                    } else if let Some(body) = virtual_keyboard_line(other) {
                        wire.send(&message(keyboard, body.0, &body.1))?;
                    }
                }
            }
        }
    }
}

/// 標準入力の `vk-press CODE`・`vk-release CODE`・`vk-mods DEPRESSED` を、virtual-keyboard の要求(番号とコンテンツ)
/// にする。IME が受けたキーと関係なく、virtual-keyboard でキーを送るため。
fn virtual_keyboard_line(line: &str) -> Option<(u16, Vec<u8>)> {
    let (command, value) = line.split_once(' ')?;
    let value: u32 = value.trim().parse().ok()?;
    match command {
        // zwp_virtual_keyboard_v1.key(時刻、キー、状態)
        "vk-press" | "vk-release" => Some((
            1,
            [0u32, value, (command == "vk-press") as u32]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        )),
        // zwp_virtual_keyboard_v1.modifiers(押されている、ラッチ、ロック、グループ)
        "vk-mods" => Some((
            2,
            [value, 0, 0, 0]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        )),
        _ => None,
    }
}

/// `--virtual-keyboard-only`:get_input_method を頼まずに virtual-keyboard を作り、コンポジタの返事を書く。
/// 断られたら(プロトコルのエラー)`ime: protocol error …` を書いて終わる。
fn virtual_keyboard_alone(
    wire: &mut crate::wire::Wire,
    virtual_manager: u32,
    seat: u32,
) -> Result<bool, String> {
    let keyboard = wire.new_id();
    wire.send(&message(
        virtual_manager,
        0,
        &[seat.to_le_bytes(), keyboard.to_le_bytes()].concat(),
    ))?;
    let id = wire.new_id();
    wire.send(&message(WL_DISPLAY, 0, &id.to_le_bytes()))?;
    loop {
        match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Nothing => {}
            Polled::Event(event) => {
                if let Some(error) = protocol_error(&event) {
                    say(&format!("protocol error {error}"));
                    return Ok(true);
                }
                if event.object == id {
                    say("virtual keyboard accepted");
                    return Ok(false);
                }
            }
        }
    }
}

/// 要求 `request` を送る。`hold` でためているときは、送らずにためる。
fn out(
    wire: &mut crate::wire::Wire,
    held: &mut Option<Vec<Vec<u8>>>,
    request: Vec<u8>,
) -> Result<(), String> {
    match held {
        Some(queue) => {
            queue.push(request);
            Ok(())
        }
        None => wire.send(&request),
    }
}

/// 変換中の文字 `text` を送る要求(カーソルは終わり)。set_preedit_string と commit(serial)。
fn preedit_requests(input_method: u32, text: &str, serial: u32) -> [Vec<u8>; 2] {
    let end = text.len() as i32;
    let mut body = string(text);
    body.extend(end.to_le_bytes());
    body.extend(end.to_le_bytes());
    [
        message(input_method, 1, &body),
        message(input_method, 3, &serial.to_le_bytes()),
    ]
}

/// 候補ウィンドウの色(B, G, R, X。紫)。
const POPUP_PIXEL: [u8; 4] = [0xff, 0x00, 0x80, 0xff];

/// 候補ウィンドウの絵と、描き直すためのもの。
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

/// 候補ウィンドウ(input popup surface)を作り、`width` × `height` の紫の単色の絵を出す。返すのは、popup surface の番号と、
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
