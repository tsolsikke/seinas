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
//!
//! 標準入力から `title TEXT` の行が届くと、題名を `TEXT` に変える。
//!
//! 標準出力には、次の行を書く。
//!
//! - `window: configure 640x480 [activated]`: xdg_toplevel.configure が届くたびに1行。大きさと、状態の並び。
//!   状態が無ければ `[]`。
//! - `window: decoration server-side`: 飾りを描く側の知らせ(zxdg_toplevel_decoration_v1.configure)が
//!   届くたびに1行(`--decoration` を付けたとき)。クライアントの側なら `client-side`。
//! - `window: wm_capabilities []`: コンポジタができることの並び(xdg_toplevel.wm_capabilities)が届くたびに1行。
//! - `window: drawn`: 最初の絵をコンポジタが描いたとき(frameコールバックが届いたとき)に1回。
//! - `window: pointer enter X Y`、`window: pointer leave`、`window: pointer motion X Y`、
//!   `window: pointer button 0x110 pressed`(または `released`): ポインターの知らせ(`--pointer` を付けたとき)。
//!   位置は、中身の左上を原点にした画素。
//! - `window: close`: 閉じるように頼まれたとき(xdg_toplevel.close)。`--ignore-close` が無ければ、
//!   これを書いて終わる。

use std::{
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::wire::{connect, open_window_with, protocol_error, Polled, WindowSetup};

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
    let _pool = open_window_with(&mut wire, WIDTH, HEIGHT, PIXEL, &options.setup)?;
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
                if let Some(error) = protocol_error(&event) {
                    return Err(format!("protocol error: {error}"));
                }
                wire.answer(&event)?;
                if let (true, Some(serial)) =
                    (options.move_on_press, wire.button_press_serial(&event))
                {
                    wire.request_move(serial)?;
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
        }
    }
}
