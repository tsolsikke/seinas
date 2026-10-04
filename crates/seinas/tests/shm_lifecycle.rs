//! Smithay 0.7.0 の共有メモリー(wl_shm)の実装が、どの操作でスレッドを作り、どの操作でSIGBUSの
//! 受け口を置き換えるかを、段階ごとに確かめる。
//!
//! 見るのは `smithay/src/wayland/shm/pool.rs` の2つの仕掛けである。
//!
//! - プールを捨てるとき、初回だけ「Shm dropping thread」という名前のスレッドを作り、以後はそこへ
//!   後始末を送る。スレッドを作れないと `unwrap` で落ちる。
//! - バッファの中身を読むとき、初回だけSIGBUSの受け口を置き換える。クライアントが共有メモリーの
//!   ファイルを縮めていた場合に、読み出しを失敗として返すためである。
//!
//! サーバーとクライアントを1本のスレッドの中で交互に動かす。ソケットは `socketpair` で作るので、
//! 名前の付いたソケットは開かない。

use std::{
    fs::File,
    os::{
        fd::{AsFd, FromRawFd},
        unix::net::UnixStream,
    },
    process::Command,
    sync::Arc,
};

use smithay::{
    backend::renderer::{pixman::PixmanRenderer, ImportMemWl},
    delegate_compositor, delegate_shm,
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer, wl_surface::WlSurface},
        Client, Display,
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_states, BufferAssignment, CompositorClientState, CompositorHandler,
            CompositorState, SurfaceAttributes,
        },
        shm::{with_buffer_contents, BufferAccessError, ShmHandler, ShmState},
    },
};
use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer as c_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, EventQueue, QueueHandle,
};

const WIDTH: i32 = 64;
const HEIGHT: i32 = 64;
const STRIDE: i32 = WIDTH * 4;
const POOL_SIZE: i32 = STRIDE * HEIGHT;
/// Linuxのスレッド名は15文字までなので、「Shm dropping thread」はこう見える。
const DROP_THREAD: &str = "Shm dropping th";
/// スレッドを作れない状況を再現する子プロセスの合図。
const CHILD_ENV: &str = "SEINAS_TEST_NO_THREADS";

// ---------- サーバー側 ----------

struct Server {
    compositor_state: CompositorState,
    shm_state: ShmState,
    /// クライアントがcommitしたバッファ。
    committed: Option<wl_buffer::WlBuffer>,
}

#[derive(Default)]
struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

impl BufferHandler for Server {
    fn buffer_destroyed(&mut self, _: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Server {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }
    fn commit(&mut self, surface: &WlSurface) {
        with_states(surface, |states| {
            let mut attributes = states.cached_state.get::<SurfaceAttributes>();
            if let Some(BufferAssignment::NewBuffer(buffer)) = attributes.current().buffer.take() {
                self.committed = Some(buffer);
            }
        });
    }
}

impl ShmHandler for Server {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_compositor!(Server);
delegate_shm!(Server);

// ---------- クライアント側 ----------

#[derive(Default)]
struct App {
    globals: Vec<(u32, String, u32)>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            state.globals.push((name, interface, version));
        }
    }
}

delegate_noop!(App: wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: wl_shm_pool::WlShmPool);
delegate_noop!(App: ignore c_buffer::WlBuffer);
delegate_noop!(App: ignore wl_surface::WlSurface);

// ---------- 観測 ----------

fn thread_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .expect("read /proc/self/task")
        .map(|entry| {
            let path = entry.expect("task entry").path().join("comm");
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .collect();
    names.sort();
    names
}

fn has_drop_thread() -> bool {
    thread_names().iter().any(|name| name == DROP_THREAD)
}

/// いまのSIGBUSの受け口(関数の番地と旗)。
fn sigbus_action() -> (usize, i32) {
    // SAFETY: 新しい設定を渡さず(null)、いまの設定を読むだけである。
    unsafe {
        let mut current: libc::sigaction = std::mem::zeroed();
        assert_eq!(
            libc::sigaction(libc::SIGBUS, std::ptr::null(), &mut current),
            0
        );
        (current.sa_sigaction, current.sa_flags)
    }
}

fn memfd(size: i32) -> File {
    // SAFETY: 名前は終端のある文字列で、返ってきたfdはこのFileだけが持つ。
    let fd = unsafe { libc::memfd_create(c"seinas-test".as_ptr(), libc::MFD_CLOEXEC) };
    assert!(fd >= 0, "memfd_create failed");
    let file = unsafe { File::from_raw_fd(fd) };
    file.set_len(size as u64).expect("set the memfd size");
    file
}

struct Harness {
    display: Display<Server>,
    server: Server,
    conn: Connection,
    queue: EventQueue<App>,
    app: App,
}

impl Harness {
    fn new() -> Self {
        let display: Display<Server> = Display::new().expect("display");
        let dh = display.handle();
        let server = Server {
            compositor_state: CompositorState::new::<Server>(&dh),
            shm_state: ShmState::new::<Server>(&dh, vec![]),
            committed: None,
        };
        let (server_end, client_end) = UnixStream::pair().expect("socketpair");
        display
            .handle()
            .insert_client(server_end, Arc::new(ClientState::default()))
            .expect("insert the client");
        let conn = Connection::from_socket(client_end).expect("client connection");
        let queue = conn.new_event_queue();
        Harness {
            display,
            server,
            conn,
            queue,
            app: App::default(),
        }
    }

    /// クライアントの要求をサーバーに処理させ、サーバーの返事をクライアントに読ませる。
    fn pump(&mut self) {
        for _ in 0..3 {
            self.conn.flush().expect("client flush");
            self.display
                .dispatch_clients(&mut self.server)
                .expect("server dispatch");
            self.display.flush_clients().expect("server flush");
            if let Some(guard) = self.conn.prepare_read() {
                // 読むものが無いとき(WouldBlock)は、そのまま進む。
                let _ = guard.read();
            }
            self.queue
                .dispatch_pending(&mut self.app)
                .expect("client dispatch");
        }
    }

    fn bind<I>(&self, registry: &wl_registry::WlRegistry, interface: &str) -> I
    where
        I: wayland_client::Proxy + 'static,
        App: Dispatch<I, ()>,
    {
        let (name, _, _) = self
            .app
            .globals
            .iter()
            .find(|(_, i, _)| i == interface)
            .unwrap_or_else(|| panic!("the server does not offer {interface}"));
        registry.bind::<I, _, _>(*name, 1, &self.queue.handle(), ())
    }
}

/// 段階を順に進め、段階ごとに `observe` を呼ぶ。
fn run(mut observe: impl FnMut(&str)) {
    observe("start");

    let mut h = Harness::new();
    let qh = h.queue.handle();
    let registry = h.conn.display().get_registry(&qh, ());
    h.pump();
    observe("the server offers wl_shm (ShmState::new)");

    let compositor: wl_compositor::WlCompositor = h.bind(&registry, "wl_compositor");
    let shm: wl_shm::WlShm = h.bind(&registry, "wl_shm");
    let surface = compositor.create_surface(&qh, ());

    let file = memfd(POOL_SIZE);
    let pool = shm.create_pool(file.as_fd(), POOL_SIZE, &qh, ());
    h.pump();
    observe("wl_shm.create_pool (the server maps the pool)");

    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, STRIDE, wl_shm::Format::Xrgb8888, &qh, ());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    h.pump();
    let server_buffer = h
        .server
        .committed
        .take()
        .expect("the server saw the commit");
    observe("wl_surface.commit with a wl_shm buffer");

    // 描画のために取り込む。PixmanRendererは、ここで初めてバッファの中身に触れる。
    let mut renderer = PixmanRenderer::new().expect("pixman renderer");
    let texture = renderer
        .import_shm_buffer(&server_buffer, None, &[])
        .expect("import the buffer");
    observe("the first read of the buffer (import for rendering)");

    // バッファとプールを壊す。サーバー側の参照が全部消えたときに、プールが捨てられる。
    drop(texture);
    drop(server_buffer);
    buffer.destroy();
    pool.destroy();
    h.pump();
    observe("the last reference to the pool is gone (the pool is dropped)");

    // 2つ目のプール。クライアントがファイルを縮める。プールの大きさの申告は元のままなので、
    // サーバーが読むとSIGBUSになる。
    let file = memfd(POOL_SIZE);
    let pool = shm.create_pool(file.as_fd(), POOL_SIZE, &qh, ());
    let buffer = pool.create_buffer(0, WIDTH, HEIGHT, STRIDE, wl_shm::Format::Xrgb8888, &qh, ());
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    h.pump();
    let server_buffer = h
        .server
        .committed
        .take()
        .expect("the server saw the second commit");
    file.set_len(0).expect("shrink the memfd");
    let read = with_buffer_contents(&server_buffer, |ptr, len, _| {
        // SAFETY: Smithayが、プールの対応づけ(`len` バイト)の先頭を渡してくる。ファイルが縮んで
        // いても、SIGBUSの受け口がその範囲を0で埋めた領域に差し替えるので、読み出しは完了する。
        unsafe { std::ptr::read_volatile(ptr.add(len - 1)) }
    });
    // 読み出しは失敗として返り、サーバーは落ちない。Smithayは、そのクライアントへプロトコルの
    // エラー(Bad pool size)を送って切る。
    assert!(
        matches!(read, Err(BufferAccessError::BadMap)),
        "reading a shrunk pool must fail without killing the process"
    );
    observe("reading a pool whose file was shrunk (SIGBUS caught)");
}

/// 子プロセス: スレッドを作れない状況でプールを捨てると、どうなるか。
fn child_without_threads() {
    run(|stage| {
        if stage.starts_with("the first read") {
            // これ以降、このプロセス(の利用者)は新しいスレッドを作れない。
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: 正しく作ったrlimitを渡すだけである。
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &limit) }, 0);
        }
    });
    println!("child: survived");
}

fn main() {
    if std::env::var_os(CHILD_ENV).is_some() {
        child_without_threads();
        return;
    }

    let threads_at_start = thread_names();
    assert_eq!(
        threads_at_start.len(),
        1,
        "threads at start: {threads_at_start:?}"
    );
    let sigbus_at_start = sigbus_action();

    let mut log = Vec::new();
    run(|stage| {
        let drop_thread = has_drop_thread();
        let sigbus_replaced = sigbus_action() != sigbus_at_start;
        println!(
            "shm_lifecycle: {stage}: threads={} drop-thread={drop_thread} sigbus-replaced={sigbus_replaced}",
            thread_names().len()
        );
        log.push((stage.to_owned(), drop_thread, sigbus_replaced));
    });

    let at = |prefix: &str| {
        log.iter()
            .find(|(stage, _, _)| stage.starts_with(prefix))
            .map(|(_, drop_thread, sigbus_replaced)| (*drop_thread, *sigbus_replaced))
            .unwrap_or_else(|| panic!("no stage starts with {prefix:?}"))
    };
    // プールを作っても、バッファをcommitしても、まだどちらも起きない。
    assert_eq!(at("the server offers"), (false, false));
    assert_eq!(at("wl_shm.create_pool"), (false, false));
    assert_eq!(at("wl_surface.commit"), (false, false));
    // バッファの中身を最初に読んだときに、SIGBUSの受け口が置き換わる。
    assert_eq!(at("the first read"), (false, true));
    // プールを最初に捨てたときに、スレッドができる。
    assert_eq!(at("the last reference"), (true, true));
    // 縮んだプールを読んでも、プロセスは落ちない(ここまで来ていることが、その証拠である)。
    assert_eq!(at("reading a pool"), (true, true));
    // 受け口は SA_NODEFER 付きで、代わりのスタックは使わない(SA_ONSTACK無し)。
    let (_, flags) = sigbus_action();
    assert_ne!(flags & libc::SA_NODEFER, 0);
    assert_eq!(flags & libc::SA_ONSTACK, 0);

    // スレッドを作れないと、プールを捨てるところで落ちる。
    // SAFETY: 引数も副作用も無い。
    if unsafe { libc::geteuid() } == 0 {
        // rootにはRLIMIT_NPROCが効かないので、再現できない。
        println!("shm_lifecycle: ok (running as root; skipped the case without threads)");
        return;
    }
    let exe = std::env::current_exe().expect("current exe");
    let output = Command::new(exe)
        .env(CHILD_ENV, "1")
        .output()
        .expect("run the child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let panic_line = stderr
        .lines()
        .find(|line| line.contains("panicked"))
        .unwrap_or("");
    println!(
        "shm_lifecycle: without threads: status={:?}, {panic_line}",
        output.status
    );
    assert!(!output.status.success(), "the child must not survive");
    assert!(
        stderr.contains("wayland/shm/pool.rs"),
        "the panic must come from the shm pool: {stderr}"
    );

    println!("shm_lifecycle: ok");
}
