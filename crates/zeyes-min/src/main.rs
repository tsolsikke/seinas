//! zeyes-min: wl_shmで2つの目を描く、小さなWaylandクライアント(zeyesの前段。文字は使わない)。
//!
//! xeyesのように、目玉が最後に見たポインターの位置へ動く。ポインターが入った・出た・ボタンを押したことは、
//! 標準エラーへ書く。つなぐ先は `WAYLAND_DISPLAY`(Seinasの中で動かすなら `seinas-0`)。
//!
//! 目のまわり(肌)の色は、引数 `--skin RRGGBB` か、環境変数 `ZEYES_SKIN` で変えられる(16進の6けた。
//! 引数が優先)。既定は緑(`6ab05c`)。ウィンドウを見分けたいときに使う。
//!
//! ウィンドウの題名は「zeyes」。引数 `--title TEXT` で変えられる。

use std::{error::Error, process::ExitCode};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const WIDTH: i32 = 320;
const HEIGHT: i32 = 240;

struct Zeyes {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    buffer: Option<Buffer>,
    pointer: Option<wl_pointer::WlPointer>,
    look_at: (f64, f64),
    /// 目のまわり(肌)の色。
    skin: u32,
    configured: bool,
    /// frameコールバックを待っている間はtrue。待っている間は描かない。
    frame_pending: bool,
    /// 描き直しが要るときはtrue。
    dirty: bool,
    exit: bool,
}

impl CompositorHandler for Zeyes {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        // 前の絵が表示に使われた合図。描き直しが要るときだけ、次の絵を描く。
        self.frame_pending = false;
        if self.dirty {
            self.draw(conn, qh);
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Zeyes {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for Zeyes {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        eprintln!("zeyes: configured to {:?}", configure.new_size);
        if !self.configured {
            self.configured = true;
            self.draw(conn, qh);
        }
    }
}

impl SeatHandler for Zeyes {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = Some(self.seat_state.get_pointer(qh, &seat).expect("pointer"));
            eprintln!("zeyes: the seat has a pointer");
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Zeyes {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        let mut moved = false;
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            match event.kind {
                Enter { .. } => eprintln!("zeyes: pointer entered at {:?}", event.position),
                Leave { .. } => eprintln!("zeyes: pointer left"),
                Motion { .. } => {
                    self.look_at = event.position;
                    moved = true;
                }
                Press { button, .. } => {
                    eprintln!("zeyes: button {button:#x} pressed at {:?}", event.position)
                }
                Release { button, .. } => eprintln!("zeyes: button {button:#x} released"),
                Axis { .. } => {}
            }
        }
        if moved {
            self.dirty = true;
        }
        // frameコールバックを待っている間は、届いてからまとめて1回だけ描く。
        if self.dirty && self.configured && !self.frame_pending {
            self.draw(conn, qh);
        }
    }
}

impl ShmHandler for Zeyes {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Zeyes {
    fn draw(&mut self, _conn: &Connection, qh: &QueueHandle<Self>) {
        let stride = WIDTH * 4;
        let buffer = self.buffer.get_or_insert_with(|| {
            self.pool
                .create_buffer(WIDTH, HEIGHT, stride, wl_shm::Format::Xrgb8888)
                .expect("create buffer")
                .0
        });
        let canvas = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                let (second, canvas) = self
                    .pool
                    .create_buffer(WIDTH, HEIGHT, stride, wl_shm::Format::Xrgb8888)
                    .expect("create a second buffer");
                *buffer = second;
                canvas
            }
        };
        paint_eyes(canvas, self.look_at, self.skin);

        let surface = self.window.wl_surface();
        surface.damage_buffer(0, 0, WIDTH, HEIGHT);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("attach");
        self.window.commit();
        self.frame_pending = true;
        self.dirty = false;
    }
}

/// 目のまわり(肌)の、既定の色。
const DEFAULT_SKIN: u32 = 0x6a_b0_5c;
const OUTLINE: u32 = 0x10_10_10;
const SCLERA: u32 = 0xff_ff_ff;
const IRIS: u32 = 0x4a_2e_1c;
const PUPIL: u32 = 0x14_0e_0a;
const GLINT: u32 = 0xff_ff_ff;

/// 目の中心(2つ)。
const CENTERS: [(f64, f64); 2] = [(85.0, 120.0), (235.0, 120.0)];
/// 目(縦長の楕円)の半径と、縁の太さ。
const EYE: (f64, f64) = (62.0, 106.0);
const EYE_OUTLINE: f64 = 10.0;
/// 目玉(虹彩)、瞳、光の点の半径。
const IRIS_RADIUS: f64 = 21.0;
const PUPIL_RADIUS: f64 = 10.0;
const GLINT_RADIUS: f64 = 3.5;
/// 目玉の中心が動ける範囲(楕円)の半径。目玉が縁に重ならないように、白目より内側に取る。
const REACH: (f64, f64) = (
    EYE.0 - EYE_OUTLINE - IRIS_RADIUS - 2.0,
    EYE.1 - EYE_OUTLINE - IRIS_RADIUS - 2.0,
);

/// xeyesのように、2つの縦長の目を描く。目玉は見ている方向へ動く。
fn paint_eyes(canvas: &mut [u8], look_at: (f64, f64), skin: u32) {
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let color = shade_with(x as f64 + 0.5, y as f64 + 0.5, look_at, skin);
            let at = ((y * WIDTH + x) * 4) as usize;
            canvas[at..at + 4].copy_from_slice(&(0xff00_0000 | color).to_le_bytes());
        }
    }
}

/// 中心が `center` の目の、目玉の中心。
///
/// 見ている点が動ける範囲の中なら、その真上に来る。外なら、中心からその点へ向かう線の上で、
/// 範囲の端まで行く。
fn eyeball(center: (f64, f64), look_at: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (look_at.0 - center.0, look_at.1 - center.1);
    let k = ((dx / REACH.0).powi(2) + (dy / REACH.1).powi(2)).sqrt();
    if k <= 1.0 {
        look_at
    } else {
        (center.0 + dx / k, center.1 + dy / k)
    }
}

/// 点(x, y)の色(0xRRGGBB)。形の端は、境目からの距離で色を混ぜてなめらかにする。
fn shade_with(x: f64, y: f64, look_at: (f64, f64), skin: u32) -> u32 {
    let mut color = skin;
    for (cx, cy) in CENTERS {
        // 目: 縁、白目の順に重ねる。
        let eye = ellipse_distance((x - cx, y - cy), EYE);
        color = mix(color, OUTLINE, coverage(eye));
        color = mix(color, SCLERA, coverage(eye + EYE_OUTLINE));

        // 目玉: 虹彩、瞳、光の点の順に重ねる。
        let (ex, ey) = eyeball((cx, cy), look_at);
        let from_eyeball = ((x - ex).powi(2) + (y - ey).powi(2)).sqrt();
        color = mix(color, IRIS, coverage(from_eyeball - IRIS_RADIUS));
        color = mix(color, PUPIL, coverage(from_eyeball - PUPIL_RADIUS));
        let (gx, gy) = (ex + 6.0, ey - 6.0);
        let from_glint = ((x - gx).powi(2) + (y - gy).powi(2)).sqrt();
        color = mix(color, GLINT, coverage(from_glint - GLINT_RADIUS));
    }
    color
}

/// 形の境目からの距離(内側が負)を、塗る割合(0.0〜1.0)に直す。境目の1画素ぶんだけ中間になる。
fn coverage(distance: f64) -> f64 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// 中心から見た点pの、楕円の境目からのおよその距離(内側が負)。
fn ellipse_distance(p: (f64, f64), radius: (f64, f64)) -> f64 {
    let k = ((p.0 / radius.0).powi(2) + (p.1 / radius.1).powi(2)).sqrt();
    if k == 0.0 {
        return -radius.0.min(radius.1);
    }
    // 中心からpへ向かう線が楕円と交わる点までの長さを使って、距離に直す。
    let len = (p.0 * p.0 + p.1 * p.1).sqrt();
    len - len / k
}

/// 色aに色bを、割合tで混ぜる。
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let channel = |shift: u32| {
        let (ca, cb) = (((a >> shift) & 0xff) as f64, ((b >> shift) & 0xff) as f64);
        ((ca + (cb - ca) * t).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // 接続が切れたときなどは、panicせずに1行だけ出して終わる。
            eprintln!("zeyes: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 色の指定(16進の6けた。先頭の `#` は、あってもよい)を読む。
fn parse_color(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    (digits.len() == 6).then(|| u32::from_str_radix(digits, 16).ok())?
}

/// ウィンドウの題名の既定。
const DEFAULT_TITLE: &str = "zeyes";

/// 起動のときの指定。
#[derive(Debug, PartialEq)]
struct Options {
    /// 目のまわりの色。
    skin: u32,
    /// ウィンドウの題名。
    title: String,
}

/// 引数を読む。目のまわりの色は、引数 `--skin`、環境変数 `ZEYES_SKIN`(`skin_env`)、既定の順に決める。
fn parse_options(args: &[String], skin_env: Option<String>) -> Result<Options, String> {
    let mut skin = skin_env;
    let mut title = DEFAULT_TITLE.to_owned();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--skin" => skin = Some(args.next().ok_or("--skin needs a value")?.clone()),
            "--title" => title = args.next().ok_or("--title needs a value")?.clone(),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let skin = match skin {
        Some(text) => {
            parse_color(&text).ok_or_else(|| format!("invalid skin color: {text} (use RRGGBB)"))?
        }
        None => DEFAULT_SKIN,
    };
    Ok(Options { skin, title })
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Options { skin, title } = parse_options(&args, std::env::var("ZEYES_SKIN").ok())?;

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to the compositor (WAYLAND_DISPLAY): {e}"))?;
    let (globals, event_queue) = registry_queue_init::<Zeyes>(&conn)?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Zeyes> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| format!("cannot watch the connection: {e}"))?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title(title);
    window.set_app_id("zeyes-min");
    window.set_min_size(Some((WIDTH as u32, HEIGHT as u32)));
    window.commit();
    let pool = SlotPool::new((WIDTH * HEIGHT * 4 * 2) as usize, &shm)?;

    let mut state = Zeyes {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        buffer: None,
        pointer: None,
        look_at: (WIDTH as f64 / 2.0, HEIGHT as f64 / 2.0),
        skin,
        configured: false,
        frame_pending: false,
        dirty: false,
        exit: false,
    };
    // 描画は、configure・ポインターのイベント・frame callback をきっかけに行うので、時間で起きる理由は無い。
    // 何かが届くまで、timeout なしで待つ(何も起きていない間は、CPU を使わない)。
    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| format!("the connection to the compositor was lost: {e}"))?;
    }
    Ok(())
}

delegate_registry!(Zeyes);
impl ProvidesRegistryState for Zeyes {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}
smithay_client_toolkit::delegate_dispatch2!(Zeyes);

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: (f64, f64) = (WIDTH as f64 / 2.0, HEIGHT as f64 / 2.0);
    const BACKGROUND: u32 = DEFAULT_SKIN;

    fn shade(x: f64, y: f64, look_at: (f64, f64)) -> u32 {
        shade_with(x, y, look_at, DEFAULT_SKIN)
    }

    #[test]
    fn the_skin_color_comes_from_the_argument_then_the_environment() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let skin = |list: &[&str], env: Option<&str>| {
            parse_options(&args(list), env.map(str::to_owned)).map(|options| options.skin)
        };
        assert_eq!(skin(&[], None), Ok(DEFAULT_SKIN));
        assert_eq!(skin(&[], Some("4060d0")), Ok(0x4060d0));
        assert_eq!(skin(&["--skin", "#ff8800"], None), Ok(0xff8800));
        // 引数が、環境変数より優先される。
        assert_eq!(skin(&["--skin", "102030"], Some("4060d0")), Ok(0x102030));
        assert!(skin(&["--skin"], None).is_err());
        assert!(skin(&["--skin", "blue"], None).is_err());
        assert!(skin(&["--bogus"], None).is_err());
        assert!(skin(&[], Some("12345")).is_err());
    }

    #[test]
    fn the_title_can_be_changed_with_an_argument() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_options(&[], None).unwrap().title, "zeyes");
        let options = parse_options(&args(&["--title", "目玉", "--skin", "102030"]), None);
        assert_eq!(
            options,
            Ok(Options {
                skin: 0x102030,
                title: "目玉".to_owned()
            })
        );
        assert!(parse_options(&args(&["--title"]), None).is_err());
    }

    #[test]
    fn the_skin_color_is_used_around_the_eyes_only() {
        let (cx, cy) = CENTERS[0];
        let far_down = (cx, cy + 500.0);
        assert_eq!(shade_with(0.5, 0.5, far_down, 0x4060d0), 0x4060d0);
        assert_eq!(
            shade_with(cx, cy - EYE.1 + EYE_OUTLINE + 3.0, far_down, 0x4060d0),
            SCLERA
        );
    }

    #[test]
    fn the_eyes_are_tall_and_where_expected() {
        let (cx, cy) = CENTERS[0];
        assert!(EYE.1 > EYE.0, "the eyes must be taller than wide");
        let far_down = (cx, cy + 500.0);
        // 隅と、2つの目の間は背景。
        assert_eq!(shade(0.5, 0.5, far_down), BACKGROUND);
        assert_eq!(shade(WIDTH as f64 / 2.0, cy, far_down), BACKGROUND);
        // 上の端は縁の色、その内側は白目。
        assert_eq!(shade(cx, cy - EYE.1 + EYE_OUTLINE / 2.0, far_down), OUTLINE);
        assert_eq!(shade(cx, cy - EYE.1 + EYE_OUTLINE + 3.0, far_down), SCLERA);
        assert_eq!(shade(cx - EYE.0 + EYE_OUTLINE / 2.0, cy, far_down), OUTLINE);
    }

    #[test]
    fn the_eyeball_sits_under_a_pointer_inside_the_eye() {
        let (cx, cy) = CENTERS[0];
        let look_at = (cx + 10.0, cy - 40.0);
        assert_eq!(eyeball((cx, cy), look_at), look_at);
        assert_eq!(shade(look_at.0, look_at.1, look_at), PUPIL);
        assert_eq!(
            shade(look_at.0 - PUPIL_RADIUS - 4.0, look_at.1, look_at),
            IRIS
        );
    }

    #[test]
    fn the_eyeball_stays_inside_the_white_of_the_eye() {
        let (cx, cy) = CENTERS[0];
        for look_at in [
            (-1000.0, -1000.0),
            (1000.0, 1000.0),
            (cx, -1000.0),
            (1000.0, cy),
        ] {
            let (ex, ey) = eyeball((cx, cy), look_at);
            let k = (((ex - cx) / REACH.0).powi(2) + ((ey - cy) / REACH.1).powi(2)).sqrt();
            assert!(
                k <= 1.0 + 1e-9,
                "the eyeball left its range for {look_at:?}"
            );
        }
        // 真上の遠くを見ると、目玉は真上の端まで行く。縁には重ならない。
        let (ex, ey) = eyeball((cx, cy), (cx, -1000.0));
        assert_eq!(ex, cx);
        assert!((ey - (cy - REACH.1)).abs() < 1e-9);
        assert_eq!(shade(cx, ey - IRIS_RADIUS - 1.5, (cx, -1000.0)), SCLERA);
    }

    #[test]
    fn painting_fills_the_whole_canvas() {
        let mut canvas = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        paint_eyes(&mut canvas, CENTER, DEFAULT_SKIN);
        assert_eq!(canvas[..4], [0x5c, 0xb0, 0x6a, 0xff]);
        assert!(canvas.chunks_exact(4).all(|pixel| pixel[3] == 0xff));
    }
}
