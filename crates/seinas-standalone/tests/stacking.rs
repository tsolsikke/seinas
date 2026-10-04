//! ウィンドウの重ね方と置き方を、色の違う2つのzeyes-minで確かめる。
//!
//! 決まり(`docs/window-placement.md`):
//! - 新しいウィンドウは、いちばん手前に来る。手前のウィンドウが去ると、その下のものが見える。
//! - 新しいウィンドウは、空いている中でいちばん小さい番号の置き場所に入る。置き場所は、番号が1つ
//!   増えるごとに、右と下へ32画素ずつずれる。

mod common;

use common::{shows_zeyes_at, Compositor, Shot, BACKGROUND, SKIN, ZEYES_HEIGHT, ZEYES_WIDTH};

/// 2つ目のzeyes-minの、目のまわりの色(青)。
const BLUE: [u8; 3] = [0x40, 0x60, 0xd0];
/// 置き場所を1つずらす量。
const STEP: usize = 32;

/// 置き場所0(左上)にある緑のウィンドウが、全体が見える形で描かれているか。
fn green_in_front(shot: &Shot) -> bool {
    shows_zeyes_at(shot, 0, 0, SKIN)
}

/// 置き場所1(右下へ32画素)にある青のウィンドウが、全体が見える形で描かれているか。
fn blue_in_front(shot: &Shot) -> bool {
    shows_zeyes_at(shot, STEP, STEP, BLUE)
}

/// 置き場所1の青のウィンドウの下から、置き場所0の緑のウィンドウの左と上の端がのぞいているか。
fn green_peeks_out_behind_blue(shot: &Shot) -> bool {
    shot.pixel(2, 2) == SKIN
        && shot.pixel(2, ZEYES_HEIGHT - 3) == SKIN
        && shot.pixel(ZEYES_WIDTH - 3, 2) == SKIN
}

#[test]
fn a_new_window_goes_in_front_and_is_placed_with_an_offset() {
    let mut seinas = Compositor::start("st");

    // 1つ目(緑)は、置き場所0(左上)に入る。
    let green = seinas.connect_zeyes();
    seinas.wait_for_screen("the green window at the top left", |shot| {
        green_in_front(shot) && shot.pixel(ZEYES_WIDTH + 2, 100) == BACKGROUND
    });

    // 2つ目(青)は、置き場所1に入り、緑の手前に重なる。緑は、左と上の端だけが見える。
    let blue = seinas.connect_zeyes_with_skin("4060d0");
    seinas.wait_for_screen("the blue window in front, offset by 32", |shot| {
        blue_in_front(shot)
            && green_peeks_out_behind_blue(shot)
            // 青のウィンドウの右下の端と、その外。
            && shot.pixel(STEP + ZEYES_WIDTH - 1, STEP + ZEYES_HEIGHT - 1) == BLUE
            && shot.pixel(STEP + ZEYES_WIDTH, STEP + ZEYES_HEIGHT) == BACKGROUND
    });

    // 手前の青が去ると、その下の緑が見える。
    drop(blue);
    seinas.wait_for_screen("the green window after the blue one left", |shot| {
        green_in_front(shot)
            && shot.pixel(STEP + ZEYES_WIDTH - 1, STEP + ZEYES_HEIGHT - 1) == BACKGROUND
    });

    // もう一度つないだ青は、空いている置き場所1に入り、また手前に来る。
    let blue = seinas.connect_zeyes_with_skin("4060d0");
    seinas.wait_for_screen("the blue window in front again", |shot| {
        blue_in_front(shot) && green_peeks_out_behind_blue(shot)
    });

    // 奥の緑が去っても、青はそのままの位置に残る。緑がのぞいていた所は、背景になる。
    drop(green);
    seinas.wait_for_screen("the blue window alone", |shot| {
        blue_in_front(shot) && shot.pixel(2, 2) == BACKGROUND
    });

    // 次につないだ緑は、空いた置き場所0に入り、青の手前に来る。
    let _green = seinas.connect_zeyes();
    seinas.wait_for_screen("a new green window in front of the blue one", |shot| {
        green_in_front(shot)
            // 青は奥になり、右と下の端だけが見える。
            && shot.pixel(STEP + ZEYES_WIDTH - 1, STEP + ZEYES_HEIGHT - 1) == BLUE
            && shot.pixel(STEP + ZEYES_WIDTH - 3, STEP + 100) == BLUE
    });

    assert!(
        seinas.process.is_running(),
        "seinas-standalone must keep running"
    );
    drop(blue);
}
