//! いちばん手前のウィンドウにだけ、activatedの状態がconfigureで知らされることを確かめる。
//!
//! 試験用のクライアント(`test-client window`)は、届いたconfigureの中身を標準出力に書く。それを読んで、
//! つないだとき、手前のウィンドウが去ったとき、奥のウィンドウが去ったときの知らせを確かめる。
//!
//! この構成はポインターを扱わないので、クリックで手前に出す操作は、ここでは確かめられない
//! (並びの決まりは、`seinas-frontend` の単体試験で確かめている)。

mod common;

use common::Compositor;

const ACTIVATED: &str = "[activated]";
const NOT_ACTIVATED: &str = "[]";

#[test]
fn only_the_front_window_is_told_that_it_is_activated() {
    let mut seinas = Compositor::start("ac");

    // 1つ目(a): つないだときの最初のconfigureで、選ばれていると知らされる。
    let a = seinas.connect_test_client(&["window"]);
    assert_eq!(
        a.next_configure_states("the first configure of a"),
        ACTIVATED
    );

    // 2つ目(b)をつなぐ: bが手前に来て、選ばれる。aからは外れる。
    let b = seinas.connect_test_client(&["window"]);
    assert_eq!(
        b.next_configure_states("the first configure of b"),
        ACTIVATED
    );
    assert_eq!(
        a.next_configure_states("a losing the activation to b"),
        NOT_ACTIVATED
    );

    // 3つ目(c)をつなぐ: cが選ばれ、bからは外れる。aは、もともと外れているので、何も届かない。
    let c = seinas.connect_test_client(&["window"]);
    assert_eq!(
        c.next_configure_states("the first configure of c"),
        ACTIVATED
    );
    assert_eq!(
        b.next_configure_states("b losing the activation to c"),
        NOT_ACTIVATED
    );

    // 奥のaが去っても、手前のcは選ばれたままで、bも変わらない(どちらにも、何も届かない)。
    // その後、手前のcが去ると、その下にあったbが選ばれる。bに次に届くのは、その知らせである。
    drop(a);
    drop(c);
    assert_eq!(
        b.next_configure_states("b becoming the front window"),
        ACTIVATED
    );

    // 新しいウィンドウ(d)をつなぐと、また入れ替わる。
    let d = seinas.connect_test_client(&["window"]);
    assert_eq!(
        d.next_configure_states("the first configure of d"),
        ACTIVATED
    );
    assert_eq!(
        b.next_configure_states("b losing the activation to d"),
        NOT_ACTIVATED
    );

    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
}
