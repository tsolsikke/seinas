//! 共有メモリーの pool の大きさを 0 以下に変える要求(`wl_shm_pool.resize`)で、コンポジタが止まらないことを
//! 確かめる。
//!
//! 決まり(`docs/smithay-patches.md`):大きさが 0 以下の resize は、プロトコルのエラーで、そのクライアントだけを
//! 切る。コンポジタは動き続け、ほかのクライアントは使える。Smithay 0.7.0 は、エラーを送った後も処理を続け、
//! 0 で panic していた(fork の直し `c7a79dc1`)。

mod common;

use common::{wait_for, Compositor};

/// テスト用のクライアントで `wl_shm_pool.resize(size)` を送り、エラーで切られることと、その後もコンポジタが
/// 動き、別のクライアントのウィンドウが出ることを確かめる。
fn resize_is_refused(name: &str, size: i32) {
    let mut seinas = Compositor::start(name);
    let size = size.to_string();
    let mut client = seinas.connect_test_client(&["pool-resize", &size]);
    let line = client.next_line("the answer to the resize");
    assert_eq!(
        line,
        "pool-resize: protocol error object 5, code 2, \"invalid wl_shm_pool size\"",
        "{}",
        seinas.log()
    );
    let code = wait_for("the client to end", || {
        client.process.0.try_wait().expect("look at the client")
    })
    .code();
    assert_eq!(code, Some(1));
    // コンポジタは動き続け、別のクライアントが使える。
    let window = seinas.connect_test_client(&["window", "--title", "after"]);
    window.wait_for_line("window: drawn");
    assert!(seinas.process.is_running(), "{}", seinas.log());
    assert!(!seinas.log().contains("panicked"), "{}", seinas.log());
}

#[test]
fn a_resize_to_zero_cuts_only_that_client() {
    resize_is_refused("za", 0);
}

#[test]
fn a_resize_to_a_negative_size_cuts_only_that_client() {
    resize_is_refused("zb", -1);
}

#[test]
fn a_pool_can_still_grow() {
    let seinas = Compositor::start("zc");
    let client = seinas.connect_test_client(&["pool-resize", "8192"]);
    assert_eq!(client.next_line("the answer"), "pool-resize: synced");
}
