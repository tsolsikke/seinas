//! 試験用: 共有メモリーのプールを、コンポジタに渡した後で縮めるクライアント。
//!
//! 流れ:
//!
//! 1. 封印の無いmemfdでプールを作り、単色のバッファを1枚作る。
//! 2. ウィンドウを作ってバッファを出し、frameコールバックが届くまで待つ(コンポジタが描いた合図)。
//!    届いたら、標準出力に `shrink: drawn` と書く。
//! 3. `--step` が付いていれば、標準入力から1行届くまで待つ(呼ぶ側が画面を確かめるための間)。
//! 4. `ftruncate` でプールを0バイトに縮め、標準出力に `shrink: shrunk` と書く。
//! 5. もう一度damageとcommitを送り、コンポジタにそのバッファを読ませる。
//! 6. コンポジタが切るまで読み、プロトコルのエラーが届いていれば中身を書く。
//!
//! ライブラリを通すと、縮められないように封印されたプールになるので、ソケットへ直接書いている。

use std::{
    io::{BufRead, Write},
    time::Instant,
};

use crate::wire::{commit_buffer, connect, open_window, protocol_error, NEXT_FREE, TIMEOUT};

/// ウィンドウの大きさと色。色は紫(赤と青が最大)で、ほかのウィンドウと見分けやすくしてある。
const WIDTH: i32 = 400;
const HEIGHT: i32 = 300;
const PIXEL: [u8; 4] = [0xff, 0x00, 0xff, 0xff]; // B, G, R, X
/// 縮めた後のcommitで頼む、frameコールバックの番号。
const SECOND_FRAME: u32 = NEXT_FREE;

fn say(text: &str) {
    println!("shrink: {text}");
    let _ = std::io::stdout().flush();
}

/// 縮めるクライアントを動かす。コンポジタに切られたらtrue、切られなければfalse。
pub fn run(step: bool) -> Result<bool, String> {
    let mut wire = connect()?;
    let pool = open_window(&mut wire, WIDTH, HEIGHT, PIXEL)?;
    say("drawn");

    if step {
        // 呼ぶ側が画面を確かめ終えるまで待つ。
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("cannot read the go-ahead: {e}"))?;
    }

    // プールを縮める。コンポジタに伝えた大きさは元のままなので、コンポジタが読むと範囲の外になる。
    pool.set_len(0)
        .map_err(|e| format!("cannot shrink the pool: {e}"))?;
    say("shrunk");

    // もう一度commitして、コンポジタにバッファを読ませる。
    commit_buffer(&mut wire, WIDTH, HEIGHT, SECOND_FRAME)?;

    // コンポジタが切るまで読む。プロトコルのエラーが届けば、中身を書く。
    let deadline = Instant::now() + TIMEOUT;
    let mut error = None;
    let closed = loop {
        match wire.next_event() {
            Ok(None) => break true,
            Ok(Some(event)) => {
                if let Some(text) = protocol_error(&event) {
                    error = Some(text);
                } else if event.object == SECOND_FRAME {
                    say("the second frame was drawn (the compositor did not notice)");
                }
                // 切られた後は送れないので、返事の失敗は気にしない。
                let _ = wire.answer(&event);
            }
            Err(_) => break false,
        }
        if Instant::now() >= deadline {
            break false;
        }
    };
    match error {
        Some(text) => say(&format!("protocol error: {text}")),
        None => say("no protocol error was sent"),
    }
    say(if closed {
        "the compositor closed the connection"
    } else {
        "the connection is still open"
    });
    Ok(closed)
}
