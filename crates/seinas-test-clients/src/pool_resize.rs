//! `pool-resize SIZE`:wl_shm_pool を 4096 バイトで作り、`wl_shm_pool.resize(SIZE)` を送る(SIZE は 0 や負の値も
//! そのまま送る)。コンポジタの答えを書く。
//!
//! - `pool-resize: protocol error …`:プロトコルのエラーが届いた。
//! - `pool-resize: synced`:エラーなしに、resize の後の往復が済んだ。
//! - `pool-resize: the compositor closed the connection`:エラーが届かずに、接続が切れた。

use std::os::fd::AsRawFd;

use crate::{
    message,
    wire::{connect, make_pool_from, protocol_error},
    WL_DISPLAY,
};

pub fn run(args: &[String]) -> Result<bool, String> {
    let size: i32 = args
        .first()
        .and_then(|text| text.parse().ok())
        .ok_or("pool-resize needs SIZE")?;
    let mut wire = connect()?;
    wire.list_globals()?;
    let shm = wire.bind("wl_shm", 1)?;
    let file = make_pool_from(&[0; 4096])?;
    let pool = wire.new_id();
    // wl_shm.create_pool(新しい番号、fd、大きさ)
    let body = [pool.to_le_bytes(), 4096i32.to_le_bytes()].concat();
    wire.send_with_fd(&message(shm, 0, &body), file.as_raw_fd())?;
    // wl_shm_pool.resize(大きさ)
    wire.send(&message(pool, 2, &size.to_le_bytes()))?;
    let callback = wire.new_id();
    wire.send(&message(WL_DISPLAY, 0, &callback.to_le_bytes()))?;
    loop {
        let Some(event) = wire.next_event()? else {
            println!("pool-resize: the compositor closed the connection");
            return Ok(false);
        };
        if let Some(error) = protocol_error(&event) {
            println!("pool-resize: protocol error {error}");
            return Ok(false);
        }
        if event.object == callback {
            println!("pool-resize: synced");
            return Ok(true);
        }
    }
}
