//! テスト用: ウィンドウを1つ出し、コンポジタから届くconfigureの中身を書き続けるクライアント。
//!
//! 行儀のよいクライアントで、止められるまで動き続ける。コンポジタが、どのウィンドウを
//! activated として知らせているか、タイトルをどう表示するかを確かめるのに使う。
//!
//! 引数:
//!
//! - `--title TEXT`: タイトルを付ける。無ければ、タイトルを付けない。
//! - `--app-id TEXT`: app_idを付ける。無ければ、付けない。
//! - `--decoration`: 飾り(タイトルバーなど)をどちらの側で描くかを、xdg-decorationで尋ねる。
//! - `--pointer`: seat のポインターを使い、届いたイベントを書く。
//! - `--keyboard`: seat のキーボードを使い、届いたイベントを書く(`window: keyboard enter`(押されているキーが
//!   あれば `window: keyboard enter keys 56`)、`window: keyboard leave`、
//!   `window: key 30 pressed`、`window: modifiers 1 0 0 0`(押されている・ラッチ・ロック・グループ)、
//!   `window: keymap 大きさ`)。
//! - `--move-on-press`: コンテンツの上でボタンが押されたら、ウィンドウを動かしてほしいと頼む(xdg_toplevelのmove)。
//!   `--pointer` も付けたことになる。
//! - `--ignore-close`: 閉じるように頼まれても、終わらない(閉じるのに応じないが、ping には pong を返すクライアントの
//!   まね。保存するかを尋ねているアプリなど)。
//! - `--ignore-ping`: xdg_wm_base の ping に pong を返さず、閉じるように頼まれても終わらない(固まったクライアントの
//!   まね)。ping が届くと `window: ping` を書く。標準入力の `pong` の行で、返していない ping に pong を返す。
//! - `--size WxH`: コンテンツの大きさ(既定は 200x150)。
//! - `--color RRGGBB`: コンテンツの色(既定は ff8000、橙)。
//! - `--follow-size`: configure で届く大きさか、maximized の状態が変わったら、届いた大きさで再描画する(既定は、
//!   大きさを変えない)。最初の configure の大きさには従わない(`--size` の大きさで出す)。
//! - `--min-size WxH`、`--max-size WxH`: コンテンツの最小と最大の大きさを、コンポジタに伝える。
//! - `--text-input`: text-input-v3 を使う(`--keyboard` も付けたことになる)。enter が届くたびに、有効にして
//!   カーソルの矩形を送り、commit する。届いたイベントを `window: text-input enter`、`window: text-input leave`、
//!   `window: preedit あい`、`window: commit あい`、`window: delete 1 0`、`window: text-input done` の形で書く。
//!   text-input-v3 のとおり、届いた状態は done で効かせ、done で確定した文字を効かせたら `window: applied commit あい`
//!   を書く。leave では、変換中の文字を消し(仕様の「The client should reset any preedit string previously set」)、
//!   消すものがあれば `window: reset preedit` を書く。
//! - `--show-text`: text-input で確定した文字と変換中の文字を、コンテンツに描く(`--text-input` も付けたことになる。
//!   スクリーンショットのため)。カーソルの矩形は、変換中の文字の終わりに合わせて送る。
//! - `--cursor X,Y,W,H`: text-input で送るカーソルの矩形(コンテンツの左上が原点。既定は 10,20,2,16)。
//! - `--resize-on-press EDGE`: コンテンツの上でボタンが押されたら、辺 `EDGE`(top・bottom・left・right・
//!   top-left・top-right・bottom-left・bottom-right)でサイズ変更してほしいと頼む(xdg_toplevel の resize)。
//!   `--pointer` も付けたことになる。
//!
//! 標準入力から `title TEXT` の行が届くと、タイトルを `TEXT` に変える。`maximize`、`unmaximize`、`minimize` の行が
//! 届くと、xdg_toplevel の set_maximized、unset_maximized、set_minimized を頼む。`cursor X Y W H` の行は、text-input の
//! カーソルの矩形を変えて commit する。`pong` の行は、`--ignore-ping` で
//! 返していない ping に pong を返し、`window: ponged` を書く。`sync` の行が届くと、それまでの
//! 頼みをコンポジタが処理し終えたときに `window: synced` を書く。
//!
//! 標準出力には、次の行を書く。
//!
//! - `window: configure 640x480 [activated]`: xdg_toplevel.configure が届くたびに1行。大きさと、状態の並び。
//!   状態が無ければ `[]`。
//! - `window: decoration server-side`: 飾りを描く側のイベント(zxdg_toplevel_decoration_v1.configure)が
//!   届くたびに1行(`--decoration` を付けたとき)。クライアントの側なら `client-side`。
//! - `window: wm_capabilities []`: コンポジタができることの並び(xdg_toplevel.wm_capabilities)が届くたびに1行。
//! - `window: drawn`: 最初の絵をコンポジタが描いたとき(frameコールバックが届いたとき)に1回。
//! - `window: resized WxH`: `--follow-size` で、新しい大きさの絵を commit したとき。
//! - `window: pointer enter X Y`、`window: pointer leave`、`window: pointer motion X Y`、
//!   `window: pointer button 0x110 pressed`(または `released`): ポインターのイベント(`--pointer` を付けたとき)。
//!   位置は、コンテンツの左上を原点にしたピクセル。
//! - `window: close`: 閉じるように頼まれたとき(xdg_toplevel.close)。`--ignore-close` が無ければ、
//!   これを書いて終わる。

use std::{
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::wire::{
    commit_buffer_with, connect, open_window_with, protocol_error, Event, Polled, WindowSetup,
    Wire, XDG_SURFACE,
};

/// ウィンドウの大きさと色(橙)。
const WIDTH: i32 = 200;
const HEIGHT: i32 = 150;
const PIXEL: [u8; 4] = [0x00, 0x80, 0xff, 0xff]; // B, G, R, X
/// text-input で送るカーソルの矩形の既定(x、y、幅、高さ)。
const CURSOR: [i32; 4] = [10, 20, 2, 16];

/// 起動のときの指定。
#[derive(Default)]
struct Options {
    setup: WindowSetup,
    pointer: bool,
    keyboard: bool,
    move_on_press: bool,
    ignore_close: bool,
    ignore_ping: bool,
    size: Option<(u32, u32)>,
    pixel: Option<[u8; 4]>,
    follow_size: bool,
    resize_on_press: Option<u32>,
    text_input: bool,
    cursor: [i32; 4],
    show_text: bool,
}

/// 引数を読む。
fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--title" => options.setup.title = Some(value()?),
            "--app-id" => options.setup.app_id = Some(value()?),
            "--decoration" => options.setup.decoration = true,
            "--pointer" => options.pointer = true,
            "--keyboard" => options.keyboard = true,
            "--text-input" => (options.keyboard, options.text_input) = (true, true),
            "--show-text" => {
                (options.keyboard, options.text_input, options.show_text) = (true, true, true)
            }
            "--cursor" => {
                let text = value()?;
                let numbers: Vec<i32> = text.split(',').filter_map(|n| n.parse().ok()).collect();
                options.cursor = numbers
                    .try_into()
                    .map_err(|_| format!("--cursor needs X,Y,W,H: {text}"))?;
            }
            "--move-on-press" => (options.pointer, options.move_on_press) = (true, true),
            "--ignore-close" => options.ignore_close = true,
            "--ignore-ping" => (options.ignore_close, options.ignore_ping) = (true, true),
            "--size" => options.size = Some(crate::layer::parse_size(&value()?)?),
            "--color" => options.pixel = Some(crate::layer::parse_color(&value()?)?),
            "--follow-size" => options.follow_size = true,
            "--min-size" => options.setup.min_size = Some(crate::layer::parse_size(&value()?)?),
            "--max-size" => options.setup.max_size = Some(crate::layer::parse_size(&value()?)?),
            "--resize-on-press" => {
                // xdg_toplevel の resize_edge の値。
                let edges = match value()?.as_str() {
                    "top" => 1,
                    "bottom" => 2,
                    "left" => 4,
                    "top-left" => 5,
                    "bottom-left" => 6,
                    "right" => 8,
                    "top-right" => 9,
                    "bottom-right" => 10,
                    other => return Err(format!("unknown edge: {other}")),
                };
                (options.pointer, options.resize_on_press) = (true, Some(edges));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(options)
}

/// コンポジタに切られるか、閉じるように頼まれるまで動く。そうなったらtrueを返す。
pub fn run(args: &[String]) -> Result<bool, String> {
    let mut options = parse_options(args)?;
    if options.cursor == [0; 4] {
        options.cursor = CURSOR;
    }
    let mut wire = connect()?;
    wire.print_configures = true;
    // プールは、ウィンドウを出している間、持ち続ける。
    let (width, height) = options
        .size
        .map(|(w, h)| (w as i32, h as i32))
        .unwrap_or((WIDTH, HEIGHT));
    let pixel = options.pixel.unwrap_or(PIXEL);
    let pool = open_window_with(&mut wire, width, height, pixel, &options.setup)?;
    // 再描画したときの pool も、持ち続ける(前のバッファを、コンポジタがまだ使っているかもしれないため)。
    let mut pools = vec![pool];
    // 文字を描くときの状態(`--show-text`)。
    let mut view = options.show_text.then(TextView::new);
    let mut text_state = TextState::default();
    if let Some(view) = &mut view {
        let (pixels, cursor) = view.paint(width, height, pixel);
        options.cursor = cursor;
        let (buffer, pool) = wire.new_buffer_from(crate::wire::SHM, width, height, &pixels)?;
        pools.push(pool);
        let callback = wire.new_id();
        commit_buffer_with(&mut wire, buffer, width, height, callback)?;
    }
    let mut drawn = (width, height);
    // 最後に従うかどうかを決めた、configure の大きさと maximized の状態。
    let mut seen = (wire.toplevel_size, wire.toplevel_maximized);
    // `sync` の行で送った wl_display.sync の callback の番号。
    let mut sync = None;
    if options.pointer || options.keyboard {
        wire.watch_seat(options.pointer, options.keyboard)?;
    }
    let mut text_input = None;
    if options.text_input {
        let manager = wire.bind("zwp_text_input_manager_v3", 1)?;
        let seat = wire.seat().ok_or("no seat")?;
        // zwp_text_input_manager_v3.get_text_input(新しい番号、seat)
        let id = wire.new_id();
        wire.send(&crate::message(
            manager,
            1,
            &[id.to_le_bytes(), seat.to_le_bytes()].concat(),
        ))?;
        text_input = Some(id);
    }
    wire.ignore_ping = options.ignore_ping;
    println!("window: drawn");
    let _ = std::io::stdout().flush();

    // 標準入力は、別のスレッドで1行ずつ読む(コンポジタからのイベントを待つのと、並べて行うため)。
    let (sender, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Event(event) => {
                if Some(event.object) == sync {
                    sync = None;
                    println!("window: synced");
                    let _ = std::io::stdout().flush();
                    continue;
                }
                if let Some(error) = protocol_error(&event) {
                    return Err(format!("protocol error: {error}"));
                }
                if Some(event.object) == text_input {
                    text_input_event(&mut wire, &event, options.cursor, &mut text_state)?;
                    if let Some(view) = &mut view {
                        if view.event(&event) {
                            // done で変わった文字を再描画し、カーソルの矩形を送り直す。
                            let (pixels, cursor) = view.paint(drawn.0, drawn.1, pixel);
                            options.cursor = cursor;
                            let (w, h) = drawn;
                            let (buffer, pool) =
                                wire.new_buffer_from(crate::wire::SHM, w, h, &pixels)?;
                            pools.push(pool);
                            let callback = wire.new_id();
                            commit_buffer_with(&mut wire, buffer, w, h, callback)?;
                            send_cursor(&mut wire, event.object, cursor)?;
                            wire.send(&crate::message(event.object, 7, &[]))?;
                        }
                    }
                    continue;
                }
                wire.answer(&event)?;
                if let (true, Some(serial)) =
                    (options.move_on_press, wire.button_press_serial(&event))
                {
                    wire.request_move(serial)?;
                }
                if let (Some(edges), Some(serial)) =
                    (options.resize_on_press, wire.button_press_serial(&event))
                {
                    wire.request_resize(serial, edges)?;
                }
                // xdg_surface.configure(configure のまとまりの終わり)で、届いた大きさに再描画する。
                let configured = (event.object, event.opcode) == (XDG_SURFACE, 0);
                let now = (wire.toplevel_size, wire.toplevel_maximized);
                let changed = configured && now != seen;
                if changed {
                    seen = now;
                }
                let wanted = seen.0.map(|(w, h)| (w as i32, h as i32));
                if let (true, true, Some((w, h))) = (options.follow_size, changed, wanted) {
                    if w > 0 && h > 0 && (w, h) != drawn {
                        let (buffer, pool) = match &mut view {
                            Some(view) => {
                                let (pixels, _) = view.paint(w, h, pixel);
                                wire.new_buffer_from(crate::wire::SHM, w, h, &pixels)?
                            }
                            None => wire.new_buffer(w, h, pixel)?,
                        };
                        pools.push(pool);
                        let callback = wire.new_id();
                        commit_buffer_with(&mut wire, buffer, w, h, callback)?;
                        drawn = (w, h);
                        println!("window: resized {w}x{h}");
                        let _ = std::io::stdout().flush();
                    }
                }
                if wire.close_requested && !options.ignore_close {
                    return Ok(true);
                }
            }
            Polled::Nothing => {}
        }
        for command in commands.try_iter() {
            if let Some(title) = command.strip_prefix("title ") {
                wire.set_title(title)?;
                wire.commit()?;
            }
            // xdg_toplevel.set_maximized(9)、unset_maximized(10)、set_minimized(13)。
            let opcode = match command.as_str() {
                "maximize" => Some(9),
                "unmaximize" => Some(10),
                "minimize" => Some(13),
                _ => None,
            };
            if let Some(opcode) = opcode {
                wire.send(&crate::message(crate::wire::TOPLEVEL, opcode, &[]))?;
            }
            if let (Some(id), Some(rest)) = (text_input, command.strip_prefix("cursor ")) {
                let numbers: Vec<i32> = rest
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                if let Ok(cursor) = <[i32; 4]>::try_from(numbers) {
                    options.cursor = cursor;
                    send_cursor(&mut wire, id, cursor)?;
                    wire.send(&crate::message(id, 7, &[]))?;
                }
            }
            if command == "pong" && wire.pong()? {
                println!("window: ponged");
                let _ = std::io::stdout().flush();
            }
            if command == "sync" {
                // wl_display.sync:それまでの頼みをコンポジタが処理し終えたら、callback が届く。
                let id = wire.new_id();
                wire.send(&crate::message(crate::WL_DISPLAY, 0, &id.to_le_bytes()))?;
                sync = Some(id);
            }
        }
    }
}

fn say(text: &str) {
    println!("window: {text}");
    let _ = std::io::stdout().flush();
}

/// zwp_text_input_v3.set_cursor_rectangle(x、y、幅、高さ)。
fn send_cursor(wire: &mut Wire, text_input: u32, cursor: [i32; 4]) -> Result<(), String> {
    let body: Vec<u8> = cursor.iter().flat_map(|n| n.to_le_bytes()).collect();
    wire.send(&crate::message(text_input, 6, &body))
}

/// 文字列の引数(null なら None)と、その次の位置。
fn string_at(event: &Event, at: usize) -> Option<(Option<String>, usize)> {
    let length = event.u32_at(at)? as usize;
    if length == 0 {
        return Some((None, at + 4));
    }
    let text = event.body.get(at + 4..at + 4 + length - 1)?;
    Some((
        Some(String::from_utf8_lossy(text).into_owned()),
        at + 4 + length.next_multiple_of(4),
    ))
}

/// text-input の状態(done で効かせる。変換中の文字は leave で消す)。
#[derive(Default)]
struct TextState {
    preedit: String,
    pending_preedit: String,
    pending_commit: String,
}

/// text-input のイベントを書き、enter なら、有効にしてカーソルの矩形を送る。状態 `text` を、仕様のとおりに変える。
fn text_input_event(
    wire: &mut Wire,
    event: &Event,
    cursor: [i32; 4],
    text: &mut TextState,
) -> Result<(), String> {
    match event.opcode {
        2 => {
            text.pending_preedit = string_at(event, 0)
                .and_then(|(text, _)| text)
                .unwrap_or_default();
        }
        3 => {
            let committed = string_at(event, 0).and_then(|(text, _)| text);
            text.pending_commit.push_str(&committed.unwrap_or_default());
        }
        _ => {}
    }
    match event.opcode {
        0 => {
            say("text-input enter");
            // enable → set_cursor_rectangle → commit
            wire.send(&crate::message(event.object, 1, &[]))?;
            send_cursor(wire, event.object, cursor)?;
            wire.send(&crate::message(event.object, 7, &[]))?;
        }
        1 => {
            say("text-input leave");
            if !text.preedit.is_empty() || !text.pending_preedit.is_empty() {
                text.preedit.clear();
                text.pending_preedit.clear();
                say("reset preedit");
            }
        }
        2 => {
            let text = string_at(event, 0).and_then(|(text, _)| text);
            say(&format!("preedit {}", text.unwrap_or_default()));
        }
        3 => {
            let text = string_at(event, 0).and_then(|(text, _)| text);
            say(&format!("commit {}", text.unwrap_or_default()));
        }
        4 => say(&format!(
            "delete {} {}",
            event.u32_at(0).unwrap_or(0),
            event.u32_at(4).unwrap_or(0)
        )),
        5 => {
            say("text-input done");
            let committed = std::mem::take(&mut text.pending_commit);
            if !committed.is_empty() {
                say(&format!("applied commit {committed}"));
            }
            text.preedit = std::mem::take(&mut text.pending_preedit);
        }
        _ => {}
    }
    Ok(())
}

/// `--show-text` のウィンドウに描く文字(確定した文字と、変換中の文字)。text-input の done で効かせる。
struct TextView {
    painter: seinas_text::TextPainter,
    committed: String,
    preedit: String,
    pending_commit: String,
    pending_preedit: String,
}

/// 文字の色と、変換中の文字の下線の色。
const TEXT: [u8; 3] = [0x18, 0x18, 0x18];
/// 行の高さと、2 行の上の端。
const TEXT_LINE: i32 = 24;
const COMMITTED_TOP: i32 = 8;
const PREEDIT_TOP: i32 = 40;
const TEXT_LEFT: i32 = 10;

impl TextView {
    fn new() -> Self {
        TextView {
            painter: crate::text::painter(),
            committed: String::new(),
            preedit: String::new(),
            pending_commit: String::new(),
            pending_preedit: String::new(),
        }
    }

    /// text-input のイベントを覚える。done で文字が変わったらtrue。
    fn event(&mut self, event: &Event) -> bool {
        match event.opcode {
            // leave:変換中の文字を消す(仕様)。
            1 => {
                self.preedit.clear();
                self.pending_preedit.clear();
                true
            }
            2 => {
                self.pending_preedit = string_at(event, 0)
                    .and_then(|(text, _)| text)
                    .unwrap_or_default();
                false
            }
            3 => {
                let text = string_at(event, 0).and_then(|(text, _)| text);
                self.pending_commit.push_str(&text.unwrap_or_default());
                false
            }
            5 => {
                self.committed
                    .push_str(&std::mem::take(&mut self.pending_commit));
                self.preedit = std::mem::take(&mut self.pending_preedit);
                true
            }
            _ => false,
        }
    }

    /// `width` × `height` の絵(地は `pixel` の色)と、変換中の文字の終わりのカーソルの矩形。
    fn paint(&mut self, width: i32, height: i32, pixel: [u8; 4]) -> (Vec<u8>, [i32; 4]) {
        let painter = &mut self.painter;
        let mut pixels = crate::text::canvas(width, height, [pixel[2], pixel[1], pixel[0]]);
        let committed = format!("確定:{}", self.committed);
        crate::text::draw(
            painter,
            &mut pixels,
            width,
            &committed,
            TEXT_LEFT,
            COMMITTED_TOP,
            TEXT_LINE,
            TEXT,
        );
        let label = crate::text::draw(
            painter,
            &mut pixels,
            width,
            "入力中:",
            TEXT_LEFT,
            PREEDIT_TOP,
            TEXT_LINE,
            TEXT,
        );
        let x = TEXT_LEFT + label;
        let drawn = crate::text::draw(
            painter,
            &mut pixels,
            width,
            &self.preedit,
            x,
            PREEDIT_TOP,
            TEXT_LINE,
            TEXT,
        );
        if drawn > 0 {
            crate::text::fill(
                &mut pixels,
                width,
                (x, PREEDIT_TOP + TEXT_LINE - 3, drawn, 1),
                TEXT,
            );
        }
        (pixels, [x + drawn, PREEDIT_TOP, 2, TEXT_LINE])
    }
}
