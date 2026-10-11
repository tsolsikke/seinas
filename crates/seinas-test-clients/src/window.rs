//! 試験用: ウィンドウを1つ出し、コンポジタから届くconfigureの中身を書き続けるクライアント。
//!
//! 行儀のよいクライアントで、止められるまで動き続ける。コンポジタが、どのウィンドウを「選ばれている
//! (activated)」として知らせているか、題名をどう表示するかを確かめるのに使う。
//!
//! 引数:
//!
//! - `--title TEXT`: 題名を付ける。無ければ、題名を付けない。
//! - `--app-id TEXT`: app_idを付ける。無ければ、付けない。
//! - `--decoration`: 飾り(題名の帯など)をどちらの側で描くかを、xdg-decorationで尋ねる。
//! - `--pointer`: 席のポインターを使い、届いた知らせを書く。
//! - `--move-on-press`: 中身の上でボタンが押されたら、ウィンドウを動かしてほしいと頼む(xdg_toplevelのmove)。
//!   `--pointer` も付けたことになる。
//! - `--ignore-close`: 閉じるように頼まれても、終わらない(応じないクライアントのまね)。
//! - `--size WxH`: コンテンツの大きさ(既定は 200x150)。
//! - `--color RRGGBB`: コンテンツの色(既定は ff8000、橙)。
//! - `--follow-size`: configure で届く大きさか、maximized の状態が変わったら、届いた大きさで再描画する(既定は、
//!   大きさを変えない)。最初の configure の大きさには従わない(`--size` の大きさで出す)。
//! - `--min-size WxH`、`--max-size WxH`: コンテンツの最小と最大の大きさを、コンポジタに伝える。
//! - `--resize-on-press EDGE`: コンテンツの上でボタンが押されたら、辺 `EDGE`(top・bottom・left・right・
//!   top-left・top-right・bottom-left・bottom-right)でサイズ変更してほしいと頼む(xdg_toplevel の resize)。
//!   `--pointer` も付けたことになる。
//!
//! 標準入力から `title TEXT` の行が届くと、タイトルを `TEXT` に変える。`maximize`、`unmaximize`、`minimize` の行が
//! 届くと、xdg_toplevel の set_maximized、unset_maximized、set_minimized を頼む。`sync` の行が届くと、それまでの
//! 頼みをコンポジタが処理し終えたときに `window: synced` を書く。
//!
//! 標準出力には、次の行を書く。
//!
//! - `window: configure 640x480 [activated]`: xdg_toplevel.configure が届くたびに1行。大きさと、状態の並び。
//!   状態が無ければ `[]`。
//! - `window: decoration server-side`: 飾りを描く側の知らせ(zxdg_toplevel_decoration_v1.configure)が
//!   届くたびに1行(`--decoration` を付けたとき)。クライアントの側なら `client-side`。
//! - `window: wm_capabilities []`: コンポジタができることの並び(xdg_toplevel.wm_capabilities)が届くたびに1行。
//! - `window: drawn`: 最初の絵をコンポジタが描いたとき(frameコールバックが届いたとき)に1回。
//! - `window: resized WxH`: `--follow-size` で、新しい大きさの絵を commit したとき。
//! - `window: pointer enter X Y`、`window: pointer leave`、`window: pointer motion X Y`、
//!   `window: pointer button 0x110 pressed`(または `released`): ポインターの知らせ(`--pointer` を付けたとき)。
//!   位置は、中身の左上を原点にした画素。
//! - `window: close`: 閉じるように頼まれたとき(xdg_toplevel.close)。`--ignore-close` が無ければ、
//!   これを書いて終わる。

use std::{
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::wire::{
    commit_buffer_with, connect, open_window_with, protocol_error, Polled, WindowSetup, XDG_SURFACE,
};

/// ウィンドウの大きさと色(橙)。
const WIDTH: i32 = 200;
const HEIGHT: i32 = 150;
const PIXEL: [u8; 4] = [0x00, 0x80, 0xff, 0xff]; // B, G, R, X

/// 起動のときの指定。
#[derive(Default)]
struct Options {
    setup: WindowSetup,
    pointer: bool,
    move_on_press: bool,
    ignore_close: bool,
    size: Option<(u32, u32)>,
    pixel: Option<[u8; 4]>,
    follow_size: bool,
    resize_on_press: Option<u32>,
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
            "--move-on-press" => (options.pointer, options.move_on_press) = (true, true),
            "--ignore-close" => options.ignore_close = true,
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
    let options = parse_options(args)?;
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
    let mut drawn = (width, height);
    // 最後に従うかどうかを決めた、configure の大きさと maximized の状態。
    let mut seen = (wire.toplevel_size, wire.toplevel_maximized);
    // `sync` の行で送った wl_display.sync の callback の番号。
    let mut sync = None;
    if options.pointer {
        wire.watch_pointer()?;
    }
    println!("window: drawn");
    let _ = std::io::stdout().flush();

    // 標準入力は、別のスレッドで1行ずつ読む(コンポジタからの知らせを待つのと、並べて行うため)。
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
                        let (buffer, pool) = wire.new_buffer(w, h, pixel)?;
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
            if command == "sync" {
                // wl_display.sync:それまでの頼みをコンポジタが処理し終えたら、callback が届く。
                let id = wire.new_id();
                wire.send(&crate::message(crate::WL_DISPLAY, 0, &id.to_le_bytes()))?;
                sync = Some(id);
            }
        }
    }
}
