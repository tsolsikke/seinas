//! zeyes-min: wl_shmで2つの目を描く、小さなWaylandクライアント(zeyesの前段。文字は使わない)。
//!
//! 黒目は、最後に見たポインターの位置へ寄る。ポインターが入った・出た・ボタンを押したことは、
//! 標準エラーへ書く。つなぐ先は `WAYLAND_DISPLAY`(Seinasの中で動かすなら `seinas-0`)。

use std::time::Duration;

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
    configured: bool,
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
        self.draw(conn, qh);
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
        if moved && self.configured {
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
        paint_eyes(canvas, self.look_at);

        let surface = self.window.wl_surface();
        surface.damage_buffer(0, 0, WIDTH, HEIGHT);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("attach");
        self.window.commit();
    }
}

/// 2つの目を描く。白目は楕円、黒目は見ている方向へ寄る。
fn paint_eyes(canvas: &mut [u8], look_at: (f64, f64)) {
    let (w, h) = (WIDTH as f64, HEIGHT as f64);
    let eyes = [(w * 0.3, h * 0.5), (w * 0.7, h * 0.5)];
    let (rx, ry) = (w * 0.16, h * 0.36);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
            let mut color: u32 = 0xff_f4_e9_d8; // 背景
            for &(cx, cy) in &eyes {
                let dx = (fx - cx) / rx;
                let dy = (fy - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    color = 0xff_ff_ff_ff; // 白目
                                           // 黒目: 目の中心からlook_atの方向へ、半径の55%まで寄せる。
                    let (vx, vy) = (look_at.0 - cx, look_at.1 - cy);
                    let len = (vx * vx + vy * vy).sqrt().max(1.0);
                    let reach = ((len / 60.0).min(1.0)) * 0.55;
                    let (px, py) = (cx + vx / len * reach * rx, cy + vy / len * reach * ry);
                    let pr = rx.min(ry) * 0.33;
                    if (fx - px) * (fx - px) + (fy - py) * (fy - py) <= pr * pr {
                        color = 0xff_10_10_10;
                    }
                }
                if dx * dx + dy * dy > 1.0 && dx * dx + dy * dy <= 1.12 {
                    color = 0xff_20_20_20; // 縁
                }
            }
            let at = ((y * WIDTH + x) * 4) as usize;
            canvas[at..at + 4].copy_from_slice(&color.to_le_bytes());
        }
    }
}

delegate_registry!(Zeyes);
impl ProvidesRegistryState for Zeyes {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}
smithay_client_toolkit::delegate_dispatch2!(Zeyes);

fn main() {
    let conn = Connection::connect_to_env().expect("connect (WAYLAND_DISPLAY)");
    let (globals, event_queue) = registry_queue_init::<Zeyes>(&conn).expect("registry");
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Zeyes> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .expect("source");

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg_shell = XdgShell::bind(&globals, &qh).expect("xdg_wm_base");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("zeyes-min");
    window.set_app_id("zeyes-min");
    window.set_min_size(Some((WIDTH as u32, HEIGHT as u32)));
    window.commit();
    let pool = SlotPool::new((WIDTH * HEIGHT * 4 * 2) as usize, &shm).expect("pool");

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
        configured: false,
        exit: false,
    };
    while !state.exit {
        event_loop
            .dispatch(Duration::from_millis(16), &mut state)
            .expect("dispatch");
    }
}
