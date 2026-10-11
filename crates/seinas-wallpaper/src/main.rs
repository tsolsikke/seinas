//! seinas-wallpaper: Seinas の壁紙。
//!
//! コンポジタとは別のプログラムで、layer-shell の background の層に、出力いっぱいの面を出し、色か画像(PNG)を
//! 描く。シェルの部品のためのソケットへつなぐこと(`WAYLAND_DISPLAY=seinas-0-shell`。ADR 0002)。
//!
//! 引数:
//!
//! - `--config PATH`:設定のファイル(無ければ `$XDG_CONFIG_HOME/seinas/wallpaper.conf`)。
//!
//! 決まりの説明は `docs/wallpaper.md` にある。

use std::{error::Error, path::PathBuf, process::ExitCode};

use seinas_wallpaper::{load_png, paint, read_config, Config, CONFIG_FILE};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
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
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

struct Wallpaper {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    config: Config,
    /// いま出している絵の大きさと、そのバッファ。
    size: (u32, u32),
    buffer: Option<Buffer>,
    exit: bool,
}

impl Wallpaper {
    /// 大きさ `(width, height)` の絵を作って出す。画像は、ここで読んで縮め、縮めた後は持たない。
    fn draw(&mut self, width: u32, height: u32) {
        let picture = self
            .config
            .image
            .as_ref()
            .and_then(|path| match load_png(path) {
                Ok(picture) => Some(picture),
                Err(e) => {
                    eprintln!("seinas-wallpaper: {e}; only the color is shown");
                    None
                }
            });
        let pixels = paint(
            picture.as_ref(),
            self.config.mode,
            self.config.color,
            width,
            height,
        );
        drop(picture);
        let (w, h) = (width as i32, height as i32);
        let (buffer, canvas) = match self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Xrgb8888)
        {
            Ok(created) => created,
            Err(e) => {
                eprintln!("seinas-wallpaper: cannot create a buffer: {e}");
                return;
            }
        };
        canvas.copy_from_slice(&pixels);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w, h);
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("seinas-wallpaper: cannot attach the buffer: {e}");
            return;
        }
        self.layer.commit();
        // 前のバッファは、ここで手放す(コンポジタが離したら、pool の場所が空く)。
        self.buffer = Some(buffer);
        self.size = (width, height);
        eprintln!("seinas-wallpaper: drew {width}x{height}");
    }
}

impl LayerShellHandler for Wallpaper {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        eprintln!("seinas-wallpaper: the compositor closed the wallpaper");
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (width, height) = configure.new_size;
        if width == 0 || height == 0 {
            eprintln!("seinas-wallpaper: the compositor gave no size");
            return;
        }
        // 大きさが変わったときだけ、再描画する(画像を縮め直す)。
        if (width, height) != self.size {
            self.draw(width, height);
        } else {
            self.layer.commit();
        }
    }
}

impl CompositorHandler for Wallpaper {
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
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

impl OutputHandler for Wallpaper {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Wallpaper {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Wallpaper);
smithay_client_toolkit::delegate_dispatch2!(Wallpaper);

impl ProvidesRegistryState for Wallpaper {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

/// 引数から、設定のファイルの場所を決める。`--config PATH` が無ければ、既定の場所。
fn config_path(args: &[String]) -> Result<Option<PathBuf>, String> {
    let mut args = args.iter();
    let mut path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => path = Some(PathBuf::from(args.next().ok_or("--config needs a value")?)),
            other => {
                return Err(format!(
                    "unknown argument: {other} (usage: seinas-wallpaper [--config PATH])"
                ))
            }
        }
    }
    Ok(path.or_else(|| seinas_config::default_file(CONFIG_FILE)))
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match config_path(&args)? {
        Some(path) => {
            let (config, problems) = read_config(&path);
            for problem in problems {
                match problem.line {
                    0 => eprintln!("seinas-wallpaper: {}", problem.reason),
                    line => eprintln!(
                        "seinas-wallpaper: {}, line {line}: {}",
                        path.display(),
                        problem.reason
                    ),
                }
            }
            config
        }
        None => Config::default(),
    };
    match &config.image {
        Some(image) => eprintln!(
            "seinas-wallpaper: {} ({:?}), color {:02x}{:02x}{:02x}",
            image.display(),
            config.mode,
            config.color[0],
            config.color[1],
            config.color[2]
        ),
        None => eprintln!(
            "seinas-wallpaper: color {:02x}{:02x}{:02x}",
            config.color[0], config.color[1], config.color[2]
        ),
    }

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("cannot connect to the compositor (WAYLAND_DISPLAY): {e}"))?;
    let (globals, event_queue) = registry_queue_init::<Wallpaper>(&conn)?;
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<Wallpaper> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| format!("cannot watch the connection: {e}"))?;

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| {
        format!(
            "the compositor does not offer zwlr_layer_shell_v1 (connect to the shell socket): {e}"
        )
    })?;
    let shm = Shm::bind(&globals, &qh)?;

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Background,
        Some("seinas-wallpaper"),
        None,
    );
    // 出力いっぱい。大きさ 0 は「コンポジタに任せる」。exclusive zone -1 で、ほかの面(タスクバー)の
    // exclusive zone にかかわらず、出力の全体に置いてもらう。
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_size(0, 0);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();
    // 大きさは configure まで分からない。pool は、足りなければ伸びる。
    let pool = SlotPool::new(800 * 600 * 4, &shm)?;

    let mut state = Wallpaper {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        config,
        size: (0, 0),
        buffer: None,
        exit: false,
    };
    while !state.exit {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| format!("the connection to the compositor was lost: {e}"))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("seinas-wallpaper: {e}");
            ExitCode::FAILURE
        }
    }
}
