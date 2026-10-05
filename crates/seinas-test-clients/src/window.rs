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
//!
//! 標準入力から `title TEXT` の行が届くと、題名を `TEXT` に変える。
//!
//! 標準出力には、次の行を書く。
//!
//! - `window: configure 640x480 [activated]`: xdg_toplevel.configure が届くたびに1行。大きさと、状態の並び。
//!   状態が無ければ `[]`。
//! - `window: decoration server-side`: 飾りを描く側の知らせ(zxdg_toplevel_decoration_v1.configure)が
//!   届くたびに1行(`--decoration` を付けたとき)。クライアントの側なら `client-side`。
//! - `window: drawn`: 最初の絵をコンポジタが描いたとき(frameコールバックが届いたとき)に1回。

use std::{
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::wire::{connect, open_window_with, protocol_error, Polled, WindowSetup};

/// ウィンドウの大きさと色(橙)。
const WIDTH: i32 = 200;
const HEIGHT: i32 = 150;
const PIXEL: [u8; 4] = [0x00, 0x80, 0xff, 0xff]; // B, G, R, X

/// 引数を読む。
fn parse_setup(args: &[String]) -> Result<WindowSetup, String> {
    let mut setup = WindowSetup::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--title" => setup.title = Some(value()?),
            "--app-id" => setup.app_id = Some(value()?),
            "--decoration" => setup.decoration = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(setup)
}

/// コンポジタに切られるまで動く。切られたらtrueを返す。
pub fn run(args: &[String]) -> Result<bool, String> {
    let setup = parse_setup(args)?;
    let mut wire = connect()?;
    wire.print_configures = true;
    // プールは、ウィンドウを出している間、持ち続ける。
    let _pool = open_window_with(&mut wire, WIDTH, HEIGHT, PIXEL, &setup)?;
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
