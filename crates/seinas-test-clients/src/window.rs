//! 試験用: ウィンドウを1つ出し、コンポジタから届くconfigureの中身を書き続けるクライアント。
//!
//! 行儀のよいクライアントで、止められるまで動き続ける。コンポジタが、どのウィンドウを「選ばれている
//! (activated)」として知らせているかを確かめるのに使う。
//!
//! 標準出力には、次の行を書く。
//!
//! - `window: configure 640x480 [activated]`: xdg_toplevel.configure が届くたびに1行。大きさと、状態の並び。
//!   状態が無ければ `[]`。
//! - `window: drawn`: 最初の絵をコンポジタが描いたとき(frameコールバックが届いたとき)に1回。

use std::io::Write;

use crate::wire::{connect, open_window, protocol_error};

/// ウィンドウの大きさと色(橙)。
const WIDTH: i32 = 200;
const HEIGHT: i32 = 150;
const PIXEL: [u8; 4] = [0x00, 0x80, 0xff, 0xff]; // B, G, R, X

/// コンポジタに切られるまで動く。切られたらtrueを返す。
pub fn run() -> Result<bool, String> {
    let mut wire = connect()?;
    wire.print_configures = true;
    // プールは、ウィンドウを出している間、持ち続ける。
    let _pool = open_window(&mut wire, WIDTH, HEIGHT, PIXEL)?;
    println!("window: drawn");
    let _ = std::io::stdout().flush();

    loop {
        match wire.next_event() {
            Ok(None) => return Ok(true),
            Ok(Some(event)) => {
                if let Some(error) = protocol_error(&event) {
                    return Err(format!("protocol error: {error}"));
                }
                wire.answer(&event)?;
            }
            // 何も届かないだけなら、待ち続ける。
            Err(_) => {}
        }
    }
}
