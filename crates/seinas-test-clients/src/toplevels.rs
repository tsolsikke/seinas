//! テスト用: ウィンドウの一覧(foreign-toplevel)を受け取って書き出し、標準入力から届いた頼みを送るクライアント。
//!
//! タスクバーの代わりに、一覧の内容と、activate・close などの頼みの扱いを確かめるのに使う。シェルの部品の
//! ためのソケットへつなぐこと(ふだんのソケットには、一覧の global が無い)。
//!
//! 引数:
//!
//! - `--ext`: `ext_foreign_toplevel_list_v1` も bind する。
//! - `--ext-only`: `ext_foreign_toplevel_list_v1` だけを bind する(wlr の manager は bind しない)。
//! - `--output-after`: wl_output を、manager の後に bind する(output_enter が後から届くことを確かめる)。
//!
//! 標準出力:
//!
//! - `toplevel N: title="…" app_id="…" state=[activated] outputs=1`:wlr の handle に done が届くたび。N は、
//!   届いた順に 1 から付ける番号。
//! - `toplevel N: closed`
//! - `list N: title="…" app_id="…" identifier=32`:ext の handle に done が届くたび(identifier は長さ)。
//! - `list N: closed`
//! - `toplevels: ready`(最初の一覧を受け取り終えた)、`toplevels: synced`(頼みをコンポジタが処理し終えた)、
//!   `toplevels: finished`(manager が終わった)。
//!
//! 標準入力の 1 行が、1 つの頼み:`activate N`、`close N`、`maximize N`、`unmaximize N`、`minimize N`、
//! `unminimize N`、`fullscreen N`、`unfullscreen N`、`rectangle N X Y W H`、`stop`。

use std::{
    collections::HashMap,
    io::{BufRead, Write},
    sync::mpsc,
};

use crate::{
    message,
    wire::{connect, protocol_error, Event, Polled},
};

/// wlr の handle に届いた内容。done で書き出す。
#[derive(Default)]
struct Shown {
    number: usize,
    title: String,
    app_id: String,
    states: Vec<u32>,
    outputs: usize,
    identifier: usize,
}

impl Shown {
    fn describe(&self) -> String {
        let states: Vec<&str> = self
            .states
            .iter()
            .map(|state| match state {
                0 => "maximized",
                1 => "minimized",
                2 => "activated",
                3 => "fullscreen",
                _ => "unknown",
            })
            .collect();
        format!(
            "title={:?} app_id={:?} state=[{}] outputs={}",
            self.title,
            self.app_id,
            states.join(","),
            self.outputs
        )
    }
}

/// 文字列の引数を読む。
fn string_at(event: &Event, at: usize) -> String {
    let length = event.u32_at(at).unwrap_or(0) as usize;
    let text = event
        .body
        .get(at + 4..at + 4 + length.saturating_sub(1))
        .unwrap_or_default();
    String::from_utf8_lossy(text).into_owned()
}

fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

/// コンポジタに切られるまで動く。切られたらtrueを返す。
pub fn run(args: &[String]) -> Result<bool, String> {
    let ext_only = args.iter().any(|arg| arg == "--ext-only");
    let ext = ext_only || args.iter().any(|arg| arg == "--ext");
    let output_after = args.iter().any(|arg| arg == "--output-after");
    if let Some(other) = args
        .iter()
        .find(|arg| !matches!(arg.as_str(), "--ext" | "--ext-only" | "--output-after"))
    {
        return Err(format!("unknown argument: {other}"));
    }

    let mut wire = connect()?;
    wire.list_globals()?;
    // seat は、入力を扱う構成にしか無い。activate を送るときだけ要る。
    let seat = wire.bind("wl_seat", 1).ok();
    let compositor = wire.bind("wl_compositor", 1)?;
    if !output_after {
        wire.bind("wl_output", 1)?;
    }
    let manager = if ext_only {
        None
    } else {
        Some(wire.bind("zwlr_foreign_toplevel_manager_v1", 3)?)
    };
    let list = if ext {
        Some(wire.bind("ext_foreign_toplevel_list_v1", 1)?)
    } else {
        None
    };
    if output_after {
        wire.bind("wl_output", 1)?;
    }
    let mut sync = wire.new_id();
    wire.send(&message(1, 0, &sync.to_le_bytes()))?;
    let mut ready = false;

    // 標準入力は、別のスレッドで 1 行ずつ読む。
    let (sender, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });

    // handle の番号(コンポジタが作ったもの)から、届いた内容へ。
    let mut handles: HashMap<u32, Shown> = HashMap::new();
    let mut ext_handles: HashMap<u32, Shown> = HashMap::new();
    let mut surface = None;
    loop {
        match wire.poll_event()? {
            Polled::Closed => return Ok(true),
            Polled::Nothing => {}
            Polled::Event(event) => {
                if let Some(error) = protocol_error(&event) {
                    say(&format!("toplevels: protocol error: {error}"));
                    continue;
                }
                match (event.object, event.opcode) {
                    (object, _) if object == sync => {
                        say(if ready {
                            "toplevels: synced"
                        } else {
                            "toplevels: ready"
                        });
                        ready = true;
                    }
                    // zwlr_foreign_toplevel_manager_v1.toplevel(新しい handle)
                    (object, 0) if Some(object) == manager => {
                        let id = event.u32_at(0).ok_or("a broken toplevel event")?;
                        let number = handles.len() + 1;
                        handles.insert(
                            id,
                            Shown {
                                number,
                                ..Shown::default()
                            },
                        );
                    }
                    // zwlr_foreign_toplevel_manager_v1.finished
                    (object, 1) if Some(object) == manager => say("toplevels: finished"),
                    // ext_foreign_toplevel_list_v1.toplevel(新しい handle)
                    (object, 0) if Some(object) == list => {
                        let id = event.u32_at(0).ok_or("a broken toplevel event")?;
                        let number = ext_handles.len() + 1;
                        ext_handles.insert(
                            id,
                            Shown {
                                number,
                                ..Shown::default()
                            },
                        );
                    }
                    (object, opcode) if handles.contains_key(&object) => {
                        let shown = handles.get_mut(&object).expect("the handle");
                        match opcode {
                            0 => shown.title = string_at(&event, 0),
                            1 => shown.app_id = string_at(&event, 0),
                            2 => shown.outputs += 1,
                            3 => shown.outputs = shown.outputs.saturating_sub(1),
                            4 => {
                                let count = event.u32_at(0).unwrap_or(0) as usize / 4;
                                shown.states =
                                    (0..count).filter_map(|i| event.u32_at(4 + i * 4)).collect();
                            }
                            5 => say(&format!("toplevel {}: {}", shown.number, shown.describe())),
                            6 => {
                                say(&format!("toplevel {}: closed", shown.number));
                                // zwlr_foreign_toplevel_handle_v1.destroy
                                wire.send(&message(object, 7, &[]))?;
                            }
                            _ => {}
                        }
                    }
                    (object, opcode) if ext_handles.contains_key(&object) => {
                        let shown = ext_handles.get_mut(&object).expect("the handle");
                        match opcode {
                            0 => {
                                say(&format!("list {}: closed", shown.number));
                                // ext_foreign_toplevel_handle_v1.destroy
                                wire.send(&message(object, 0, &[]))?;
                            }
                            1 => say(&format!(
                                "list {}: title={:?} app_id={:?} identifier={}",
                                shown.number, shown.title, shown.app_id, shown.identifier
                            )),
                            2 => shown.title = string_at(&event, 0),
                            3 => shown.app_id = string_at(&event, 0),
                            4 => shown.identifier = string_at(&event, 0).len(),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        while let Ok(line) = commands.try_recv() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let Some((&command, rest)) = words.split_first() else {
                continue;
            };
            if command == "stop" {
                // zwlr_foreign_toplevel_manager_v1.stop
                let manager = manager.ok_or("the manager is not bound")?;
                wire.send(&message(manager, 0, &[]))?;
            } else {
                let number: usize = rest
                    .first()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| format!("which toplevel? {line}"))?;
                let handle = handles
                    .iter()
                    .find(|(_, shown)| shown.number == number)
                    .map(|(id, _)| *id)
                    .ok_or_else(|| format!("no toplevel {number}"))?;
                let request = match command {
                    "maximize" => message(handle, 0, &[]),
                    "unmaximize" => message(handle, 1, &[]),
                    "minimize" => message(handle, 2, &[]),
                    "unminimize" => message(handle, 3, &[]),
                    "activate" => {
                        let seat = seat.ok_or("the compositor does not offer wl_seat")?;
                        message(handle, 4, &seat.to_le_bytes())
                    }
                    "close" => message(handle, 5, &[]),
                    "rectangle" => {
                        let values: Vec<i32> = rest[1..]
                            .iter()
                            .map(|v| v.parse().map_err(|_| format!("invalid rectangle: {line}")))
                            .collect::<Result<_, _>>()?;
                        let [x, y, w, h] = values[..] else {
                            return Err(format!("invalid rectangle: {line}"));
                        };
                        let surface = match surface {
                            Some(surface) => surface,
                            None => {
                                let id = wire.new_id();
                                // wl_compositor.create_surface
                                wire.send(&message(compositor, 0, &id.to_le_bytes()))?;
                                surface = Some(id);
                                id
                            }
                        };
                        let mut body = surface.to_le_bytes().to_vec();
                        for value in [x, y, w, h] {
                            body.extend(value.to_le_bytes());
                        }
                        message(handle, 6, &body)
                    }
                    // output は指定しない(null)。
                    "fullscreen" => message(handle, 8, &0u32.to_le_bytes()),
                    "unfullscreen" => message(handle, 9, &[]),
                    other => return Err(format!("unknown command: {other}")),
                };
                wire.send(&request)?;
            }
            sync = wire.new_id();
            wire.send(&message(1, 0, &sync.to_le_bytes()))?;
        }
    }
}
