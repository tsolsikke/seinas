//! テスト用: layer surface(wlr-layer-shell)を 1 つ出し、止められるまで動き続けるクライアント。
//!
//! シェルの部品(タスクバーなど)の代わりに、層の重なりと、exclusive zone による使える範囲の変化を
//! 確かめるのに使う。シェルの部品のためのソケットへつなぐこと(ふだんのソケットには layer-shell が無い)。
//!
//! 引数:
//!
//! - `--layer NAME`: background・bottom・top・overlay のどれか(既定は top)。
//! - `--anchor LIST`: top・bottom・left・right をコンマで並べる(既定は無し)。
//! - `--size WxH`: 頼む大きさ(既定は 0x0。コンポジタに任せる)。
//! - `--exclusive N`: exclusive zone(既定は 0)。
//! - `--margin T,R,B,L`: margin(既定は 0,0,0,0)。
//! - `--color RRGGBB`: 色(既定は 808080)。
//! - `--namespace NAME`: namespace(既定は test)。
//! - `--keyboard-interactivity N`: keyboard_interactivity(0 は none、1 は exclusive、2 は on_demand。既定は 0)。
//! - `--keyboard`: seat のキーボードを使い、届いたイベントを書く(`window: keyboard enter`、`window: keyboard leave`、
//!   `window: key 30 pressed` など。書き方は `test-client window` と同じ)。
//! - `--pointer`: seat のポインターを使い、届いたイベントを書く(書き方は `test-client window` と同じ)。
//!
//! 標準入力の `popup X Y W H [grab] [child] …` の行は、layer surface のポップアップを出す(zwlr_layer_surface_v1.get_popup。
//! child なら、いちばん新しいポップアップの子)。書き方と、標準出力に書く行(`window: popup 1 configure …` など)は、
//! `test-client window` と同じ。anchor rect は、layer surface の左上から (X, Y) の 1x1。
//!
//! 標準出力には、`layer: configure WxH`(configure が届くたび)と、`layer: drawn`(最初の絵が描かれたとき)を書く。

use std::{
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::wire::{connect, open_layer, protocol_error, LayerSetup, Polled};

/// `RRGGBB` を、B, G, R, X のピクセルにする。
pub fn parse_color(text: &str) -> Result<[u8; 4], String> {
    let value = u32::from_str_radix(text.trim_start_matches('#'), 16)
        .ok()
        .filter(|_| text.trim_start_matches('#').len() == 6)
        .ok_or_else(|| format!("invalid color: {text} (use RRGGBB)"))?;
    Ok([value as u8, (value >> 8) as u8, (value >> 16) as u8, 0xff])
}

/// `WxH` を読む。
pub fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("invalid size: {text} (use WxH)"))?;
    Ok((
        w.parse().map_err(|_| format!("invalid size: {text}"))?,
        h.parse().map_err(|_| format!("invalid size: {text}"))?,
    ))
}

/// 引数の読み取り。返すのは、layer surface の設定と、キーボードとポインターを使うか。
fn parse_setup(args: &[String]) -> Result<(LayerSetup, bool, bool), String> {
    let mut keyboard = false;
    let mut pointer = false;
    let mut setup = LayerSetup {
        layer: 2,
        anchor: 0,
        width: 0,
        height: 0,
        exclusive_zone: 0,
        margin: [0; 4],
        namespace: "test".to_owned(),
        pixel: [0x80, 0x80, 0x80, 0xff],
        keyboard_interactivity: 0,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--layer" => {
                setup.layer = match value()?.as_str() {
                    "background" => 0,
                    "bottom" => 1,
                    "top" => 2,
                    "overlay" => 3,
                    other => return Err(format!("unknown layer: {other}")),
                }
            }
            "--anchor" => {
                for edge in value()?.split(',').filter(|e| !e.is_empty()) {
                    setup.anchor |= match edge {
                        "top" => 1,
                        "bottom" => 2,
                        "left" => 4,
                        "right" => 8,
                        other => return Err(format!("unknown anchor: {other}")),
                    };
                }
            }
            "--size" => (setup.width, setup.height) = parse_size(&value()?)?,
            "--exclusive" => {
                setup.exclusive_zone = value()?.parse().map_err(|_| "invalid --exclusive")?
            }
            "--margin" => {
                let text = value()?;
                let parts: Vec<i32> = text
                    .split(',')
                    .map(|p| p.parse().map_err(|_| format!("invalid margin: {text}")))
                    .collect::<Result<_, _>>()?;
                setup.margin = parts
                    .try_into()
                    .map_err(|_| format!("invalid margin: {text} (use T,R,B,L)"))?;
            }
            "--color" => setup.pixel = parse_color(&value()?)?,
            "--namespace" => setup.namespace = value()?,
            "--keyboard-interactivity" => {
                setup.keyboard_interactivity = value()?
                    .parse()
                    .map_err(|_| "--keyboard-interactivity needs 0, 1 or 2".to_owned())?
            }
            "--keyboard" => keyboard = true,
            "--pointer" => pointer = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((setup, keyboard, pointer))
}

/// コンポジタに切られるか、closed を受け取るまで動く。そうなったらtrueを返す。
pub fn run(args: &[String]) -> Result<bool, String> {
    let (setup, keyboard, pointer) = parse_setup(args)?;
    let mut wire = connect()?;
    wire.print_configures = true;
    let _pool = open_layer(&mut wire, &setup)?;
    if keyboard || pointer {
        wire.watch_seat(pointer, keyboard)?;
    }
    println!("layer: drawn");
    let _ = std::io::stdout().flush();

    // 標準入力は、別のスレッドで1行ずつ読む(`test-client window` と同じ)。
    let (sender, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    loop {
        for command in commands.try_iter() {
            if let Some(rest) = command.strip_prefix("popup ") {
                match crate::window::parse_popup(rest) {
                    Ok(spec) => {
                        wire.open_popup(spec)?;
                    }
                    Err(e) => eprintln!("test-client: {e}"),
                }
            }
        }
        match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Event(event) => {
                if let Some(error) = protocol_error(&event) {
                    return Err(format!("protocol error: {error}"));
                }
                wire.answer(&event)?;
                if wire.close_requested {
                    return Ok(true);
                }
            }
            Polled::Nothing => {}
        }
    }
}
