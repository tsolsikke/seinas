//! Waylandのフロントエンド(Smithay)。クライアントに見せる側。
//!
//! フロントエンドは、バックエンド(絵をどこへ出すか)を知らない。入れ子の構成(`seinas`)と、fbdevへ出す構成
//! (`seinas-standalone`)の両方が、このフロントエンドを使う。
//!
//! Smithayのハンドラは、コンポジタの状態の型そのものに実装する決まりになっている。そこで、
//! 使う側は次の2つを行う。
//!
//! 1. 状態の型に [`FrontendHost`] を実装する(フロントエンドの置き場所と、再描画の知らせ方を教える)。
//! 2. [`delegate_frontend!`] を呼ぶ(Smithayのハンドラと、フロントエンドの作り方をその型に実装する)。
//!
//! フロントエンドを持たない構成(共通のレンダラーだけ)では、このクレートごと使わない。

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub use smithay;

mod decoration;
mod foreign_toplevel;
mod frame;
pub mod geometry;
mod ime;
mod ime_popup;
mod interaction;
mod key_binding;
mod ping;
mod popup;
mod resize;
mod shortcuts;
mod stack;
mod switcher;
mod switcher_view;
mod title_bar;
pub use decoration::{
    bar_height, clamp_outer_location, content_location, content_size_for, frame_contains,
    in_cut_corner, load_fonts, outer_size, paint_title_bar, title_text, TitleButton, TitleButtons,
    TitleLook, ACTIVE_BAR, ACTIVE_TITLE, BORDER_WIDTH, CLOSE_BUTTON_SIZE, CORNER_RADIUS,
    FRAME_RADIUS, HOT_ACTIVE_BUTTON, HOT_CLOSE_BUTTON, HOT_CLOSE_MARK, HOT_INACTIVE_BUTTON,
    INACTIVE_BAR, INACTIVE_TITLE, MAX_BUTTONS_WIDTH, MIN_VISIBLE_BAR, NOT_RESPONDING,
    TITLE_BAR_HEIGHT, TITLE_BUTTON_SIZE, TITLE_ICON_SIZE, TITLE_PADDING, TITLE_SIZE, TITLE_TEXT_X,
    UNRESPONSIVE_CLOSE_BUTTON,
};
pub use foreign_toplevel::{ForeignToplevelState, HandleData, Listing};
pub use ime::{
    popup_hidden, popup_shown, release_ime, ImeState, RefusedInputMethod, RefusedVirtualKeyboard,
};
pub use ime_popup::popup_location;
pub use interaction::{
    part_at, Command, Configure, Cursor, Deliver, Interaction, Measure, Outcome, Part,
    DOUBLE_CLICK_MS, DRAG_SLACK, LEFT_BUTTON, RESTORE_DRAG_SLACK,
};
pub use key_binding::{
    default_config_file, describe, parse_shortcut, read_switch_keys, switch_keys_from, CONFIG_FILE,
};
pub use ping::{Pings, DEFAULT_PING_TIMEOUT};
pub use resize::{
    anchored_location, edges_at, resized_content, Edges, ResizeStart, SizeLimits, CORNER_LENGTH,
    MIN_CONTENT_SIZE, RESIZE_BORDER,
};
pub use seinas_theme::frame::{ACTIVE_BORDER, INACTIVE_BORDER};
pub use shortcuts::{
    is_modifier_key, keysym, Action, Decision, Modifiers, Shortcut, Shortcuts, SwitchKeys,
    SWITCH_NEXT, SWITCH_NEXT_NESTED, SWITCH_PREVIOUS, SWITCH_PREVIOUS_NESTED,
};
pub use stack::{
    next_slot, slot_count, slot_location, Placed, Restore, Stack, WindowId, CASCADE_STEP,
};
pub use switcher::Switcher;
pub use switcher_view::{
    paint_switcher, switcher_layout, Row as SwitcherRow, SwitcherEntry, SwitcherLayout, MAX_ROWS,
    PANEL_MARGIN, PANEL_PADDING, PANEL_WIDTH, ROW_HEIGHT, SWITCHER_BACKGROUND, SWITCHER_BORDER,
    SWITCHER_HIGHLIGHT, SWITCHER_SELECTED_BACKGROUND, UNTITLED,
};
pub use title_bar::{SwitcherImage, TitleBar};

use seinas_icons::Found;
use seinas_text::TextPainter;
use smithay::{
    backend::input::{ButtonState, KeyState},
    backend::renderer::{
        element::{
            memory::MemoryRenderBufferRenderElement,
            solid::{SolidColorBuffer, SolidColorRenderElement},
            render_elements,
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Id, Kind,
        },
        pixman::PixmanRenderer,
        Color32F,
        utils::with_renderer_surface_state,
    },
    desktop::{layer_map_for_output, LayerSurface as MappedLayer, PopupManager, WindowSurfaceType},
    input::{
        keyboard::{FilterResult, KeyboardHandle, XkbConfig},
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::Output,
    reexports::wayland_protocols::xdg::{
        decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
        shell::server::xdg_toplevel,
    },
    reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason, ObjectId},
        protocol::wl_surface::WlSurface,
        Client, Dispatch, Display, DisplayHandle, Resource,
    },
    utils::{
        Logical, Point as SmithayPoint, Rectangle, Serial, Size as SmithaySize, SERIAL_COUNTER,
    },
    wayland::{
        compositor::{
            with_states, with_surface_tree_downward, CompositorClientState, CompositorState,
            SurfaceAttributes, TraversalAction,
        },
        shell::{
            wlr_layer::{Layer, LayerSurface, LayerSurfaceData, WlrLayerShellState},
            xdg::{
                decoration::XdgDecorationState, ShellClient, SurfaceCachedState,
                ToplevelSurface, XdgShellState, XdgToplevelSurfaceData,
            },
        },
        foreign_toplevel_list::ForeignToplevelListHandler,
        shm::ShmState,
    },
};
use tracing::{error, info, warn};

use geometry::{Point, PointF, Rect, Size};

/// フォントの置き場を指定する環境変数。
pub const FONTS_ENV: &str = "SEINAS_FONTS";
/// フォントの置き場の指定が無いときの場所。`tools/fetch-fonts.sh` が置く、開発用の場所(作業中の
/// ディレクトリから見た位置)。
pub const DEFAULT_FONT_DIR: &str = "target/fonts";

/// フォントの置き場。引数での指定 `chosen` があればそれ、無ければ環境変数 [`FONTS_ENV`]、それも
/// 無ければ [`DEFAULT_FONT_DIR`]。
pub fn font_dir(chosen: Option<PathBuf>) -> PathBuf {
    chosen
        .or_else(|| std::env::var_os(FONTS_ENV).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_FONT_DIR))
}

/// フロントエンドの作り方の指定。
#[derive(Debug, Clone)]
pub struct Config {
    /// 画面の幅と高さ(ピクセル)。wl_outputで公開する。
    pub width: i32,
    pub height: i32,
    /// seat(wl_seat)とポインターを公開するか。入力を扱わない構成ではfalseにする。
    pub pointer: bool,
    /// seat にキーボードを足すときの keymap。キーボードを扱わない構成ではNone。
    pub keyboard: Option<KeyboardConfig>,
    /// フォントの置き場。タイトルを描くのに使う。読めなくても動く(タイトルの文字が出ないだけ)。
    pub font_dir: PathBuf,
    /// フォントの設定のファイル(`--font-config`)。Noneなら、既定の場所(ADR 0003)。
    pub font_config: Option<PathBuf>,
    /// 閉じるように頼んだクライアントの pong を待つ時間。過ぎたら応答なしにする(既定は
    /// [`DEFAULT_PING_TIMEOUT`])。
    pub ping_timeout: Duration,
    /// ウィンドウの切り替えのキー(既定は [`SwitchKeys::defaults`]。設定は [`read_switch_keys`])。
    pub switch_keys: SwitchKeys,
    /// 届いたキーを、修飾キーと keysym の名前でログに出すか(`--log-keys`。入れ子の構成で、どの組み合わせが
    /// Seinas まで届くかを確かめるため)。
    pub log_keys: bool,
    /// IME のためのソケットを開くか。開かない構成(`--no-ime-socket`)では、input-method と virtual-keyboard の
    /// global も作らない(つなげるクライアントがいないため。ADR 0004 の 1.6)。
    pub ime_socket: bool,
}

/// キーボードの keymap(xkb の名前)と、押し続けたときの繰り返しの設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardConfig {
    /// xkb の layout と model。空なら、xkbcommon の既定(環境変数か、us)。
    pub layout: String,
    pub model: String,
    /// 繰り返しが始まるまでの時間(ミリ秒)と、1 秒あたりの回数。
    pub repeat_delay: i32,
    pub repeat_rate: i32,
}

impl KeyboardConfig {
    /// 日本語の配列(jp106)。親のいない構成(seinas-standalone、fbdev)の既定。
    pub fn jp106() -> Self {
        KeyboardConfig {
            layout: "jp".to_owned(),
            model: "jp106".to_owned(),
            ..KeyboardConfig::default()
        }
    }
}

impl Default for KeyboardConfig {
    /// xkbcommon の既定の keymap。入れ子の構成で、親の keymap が届くまでに使う。
    fn default() -> Self {
        KeyboardConfig {
            layout: String::new(),
            model: String::new(),
            repeat_delay: 600,
            repeat_rate: 25,
        }
    }
}

pub use frame::{border_elements, BorderCorners, ClippedSurface, Shadow};
pub use seinas_icons::IconCache;

render_elements! {
    /// 画面に描く要素。クライアントの画面(ウィンドウと layer surface)と、Seinas が描くタイトルバー・枠・影。
    /// `Clipped` は、角を欠いて描くウィンドウのコンテンツ。
    pub WindowElement<=PixmanRenderer>;
    Surface=WaylandSurfaceRenderElement<PixmanRenderer>,
    Clipped=ClippedSurface,
    TitleBar=MemoryRenderBufferRenderElement<PixmanRenderer>,
    Mark=SolidColorRenderElement,
}

/// 最大化の予告の枠の太さ(ピクセル)。
pub const MAXIMIZE_MARK_WIDTH: i32 = seinas_theme::maximize_preview::BORDER_WIDTH;
/// 最大化の予告の枠の色(選んだものの色。オレンジ)。枠の内側には、同じ色の薄い面を重ねる。
pub const MAXIMIZE_MARK: [u8; 3] = seinas_theme::maximize_preview::COLOR;
/// 最大化の予告の、枠の内側の面の不透明度。
pub const MAXIMIZE_FACE_OPACITY: f32 = seinas_theme::maximize_preview::FACE_OPACITY;

/// フロントエンドの状態。`D` は、コンポジタの状態の型。
pub struct Frontend<D: SeatHandler> {
    pub display_handle: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    /// layer-shell。global は、シェルの部品(シェルのソケットからつないだクライアント)にだけ見せる。
    pub layer_shell_state: WlrLayerShellState,
    /// ポップアップ(xdg_popup)を追う。置き方と描く順に使う(`popup.rs`)。
    pub popups: PopupManager,
    /// grab したポップアップの連なり。grab の間は、いちばん上のポップアップにキーボードのフォーカスを付ける。
    pub popup_grab: Option<popup::PopupGrabChain>,
    /// 押したときにキーボードのフォーカスを受けた layer surface(keyboard_interactivity が on_demand のもの)。
    pub layer_focus: Option<WlSurface>,
    /// 直前の入力(ポインターの押下と、キーの押下)の serial。ポップアップの grab の serial を確かめる。
    pub input_serials: Vec<Serial>,
    /// ポップアップの連なりを閉じるのに使って飲み込んだ押下のボタン。その離す側も飲み込む。
    pub swallowed_button: Option<u32>,
    /// ウィンドウの一覧と操作(foreign-toplevel)。global は、シェルの部品にだけ見せる。
    pub foreign_toplevels: ForeignToplevelState,
    /// IME の道(text-input-v3、input-method-v2、virtual-keyboard)。input-method と virtual-keyboard の global は、
    /// シェルの部品にだけ見せる(ADR 0004)。
    pub ime: ImeState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<D>,
    /// seat は、持っている間だけクライアントに見える。入力を扱わない構成ではNone。
    pub seat: Option<Seat<D>>,
    pub pointer: Option<PointerHandle<D>>,
    /// キーボード。キーボードを扱わない構成か、keymap を作れなかったときはNone。
    pub keyboard: Option<KeyboardHandle<D>>,
    /// Seinas のショートカット。キーを IME とアプリへ渡す前に見る。
    pub shortcuts: Shortcuts,
    /// ウィンドウを動かす修飾キー(`seinas.conf` の `window-drag`。既定は Alt)。押しながらウィンドウのどこでも
    /// ドラッグすると、左のボタンで動かし、右のボタンで大きさを変える。
    pub window_drag: Modifiers,
    /// Alt+Tab での切り替えの最中なら、その状態。
    pub switcher: Option<Switcher>,
    /// 切り替えを始めたキーの修飾キー(Shift を除く)。どれも離したら、切り替えを決める。
    pub switch_hold: Modifiers,
    /// 切り替えの一覧の絵(切り替えの最中だけ。条件が変わったら作り直す)。
    pub switcher_image: Option<SwitcherImage>,
    /// 届いたキーをログに出すか([`Config::log_keys`])。
    pub log_keys: bool,
    /// 画面(wl_output)。
    pub output: Output,
    /// xdg-output の global(wl_output の論理の大きさと位置を、クライアントに伝える)。
    pub output_manager: smithay::wayland::output::OutputManagerState,
    /// 画面の大きさ。
    pub size: Size,
    /// 使える範囲(画面から、layer surface の exclusive zone を除いた矩形)。layer surface が変わったときに
    /// 求め直す。
    pub usable: Rect,
    /// ウィンドウのスタック。先頭がいちばん手前で、それだけが「activated」。
    /// スロットは、タイトルバーを含めた外枠の左上。
    pub windows: Stack<ToplevelSurface>,
    /// タイトルを描くもの。
    pub text: TextPainter,
    /// 作ったタイトルバーの絵。ウィンドウ(の画面)ごとに覚えておく。
    pub title_bars: HashMap<ObjectId, TitleBar>,
    /// アプリのアイコン(タイトルバーの左)。app_id ごとに 1 回だけ探して読む(`seinas-icons`。ADR 0007)。
    pub icons: IconCache,
    /// 枠の角の絵(activated のウィンドウと、そうでないもの)。
    pub frame_corners: (BorderCorners, BorderCorners),
    /// 作った影の絵。ウィンドウ(の画面)ごとに覚えておき、大きさが変わったときだけ作り直す。
    pub shadows: HashMap<ObjectId, Shadow>,
    /// 最初の絵を出した(開いた)ウィンドウ(の画面)。開いた大きさが使える範囲に収まるかを、1 回だけ見るのに使う。
    pub opened: HashSet<ObjectId>,
    /// ポインターでの操作(動かす、閉じる、サイズ変更)の状態。
    pub interaction: Interaction,
    /// 閉じるように頼んだクライアントへの ping の答えを待つ一覧と、応答なしの印。
    pub pings: Pings<ShellClient>,
    /// 接続を切ったクライアントのクリーンアップを、バックエンドに頼む([`reap_dead_clients`])。
    pub reap_requested: bool,
    /// 最大化の予告の枠(上・下・左・右の 4 本)と、内側の薄い面。
    pub maximize_marks: [SolidColorBuffer; 5],
    /// サイズ変更の最中か、その最後の大きさがまだ届いていないウィンドウ。左か上の辺を動かしたときに、
    /// 反対の辺の位置を保つのに使う。
    pub resize_anchors: HashMap<WindowId, ResizeStart>,
}

/// コンポジタの状態の型が、フロントエンドを使うために実装するもの。
///
/// `Dispatch` と `ForeignToplevelListHandler` は、[`delegate_frontend!`] が実装する。
pub trait FrontendHost:
    SeatHandler<KeyboardFocus = WlSurface, PointerFocus = WlSurface>
    + Dispatch<ZwlrForeignToplevelHandleV1, HandleData>
    + ForeignToplevelListHandler
    + Sized
    + 'static
{
    /// フロントエンドの置き場所。
    fn frontend(&self) -> &Frontend<Self>;
    fn frontend_mut(&mut self) -> &mut Frontend<Self>;
    /// クライアントの画面が変わり、再描画が要るようになったときに呼ばれる。
    fn redraw_needed(&mut self);
}

/// Seinas のショートカットの一覧([`Config::switch_keys`])。IME のためのソケットを開かない構成([`Config::ime_socket`]
/// が偽)では、IME の緊急解除のキーを予約せず、アプリへ渡す(IME がつなげないので、解除するものが無いため。
/// ADR 0004 の 1.5)。予約するかは、起動のときの設定だけで決まり、IME の接続と切断では変わらない。
pub fn shortcuts_for(config: &Config) -> Shortcuts {
    if config.ime_socket {
        return Shortcuts::switching(&config.switch_keys);
    }
    info!("the release key for the input method is not reserved and goes to the applications (the socket for the input method is not opened)");
    Shortcuts::switching(&SwitchKeys {
        release_input_method: Vec::new(),
        ..config.switch_keys.clone()
    })
}

/// フロントエンドを作る。[`delegate_frontend!`] が、状態の型に実装する。
pub trait BuildFrontend: SeatHandler + Sized {
    fn build_frontend(display_handle: &DisplayHandle, config: Config) -> Frontend<Self>;
}

impl<D: FrontendHost> Frontend<D> {
    /// ふだんのソケットからつないできたクライアントを受け入れる。
    pub fn accept(&mut self, stream: std::os::unix::net::UnixStream) {
        self.insert_client(stream, ClientRole::App);
    }

    /// シェルの部品のためのソケットからつないできたクライアントを受け入れる。このクライアントにだけ、
    /// layer-shell の global を見せる(ADR 0002)。
    pub fn accept_shell(&mut self, stream: std::os::unix::net::UnixStream) {
        self.insert_client(stream, ClientRole::Shell);
    }

    /// IME のためのソケットからつないできたクライアントを受け入れる。このクライアントにだけ、input-method と
    /// virtual-keyboard の global を見せる(ADR 0004 の 1.6)。
    pub fn accept_ime(&mut self, stream: std::os::unix::net::UnixStream) {
        self.insert_client(stream, ClientRole::Ime);
    }

    fn insert_client(&mut self, stream: std::os::unix::net::UnixStream, role: ClientRole) {
        let state = ClientState {
            role,
            ..ClientState::default()
        };
        if let Err(e) = self.display_handle.insert_client(stream, Arc::new(state)) {
            error!("failed to accept a client: {e}");
        }
    }

    /// 新しいウィンドウを、いちばん手前に加える。スロットは、使える範囲の中で、前のものからずらして決める。
    ///
    /// 新しいウィンドウが activated なものになり、前のものからは外れる。新しいウィンドウへの
    /// 最初のconfigureは、呼ぶ側が送る。
    pub fn add_window(&mut self, toplevel: ToplevelSurface) {
        self.windows.add(toplevel, self.usable);
        self.update_activation();
    }

    /// 新しい layer surface を、画面の layer の一覧に加える。最初の configure は、最初の commit で送る。
    pub fn map_layer(&mut self, surface: LayerSurface, namespace: String) {
        let layer = MappedLayer::new(surface, namespace);
        if let Err(e) = layer_map_for_output(&self.output).map_layer(&layer) {
            error!("failed to map a layer surface: {e}");
        }
        self.update_usable_area();
    }

    /// 去った layer surface を、一覧から外す。使える範囲が広がることがある。
    pub fn unmap_layer(&mut self, surface: &LayerSurface) {
        let mut map = layer_map_for_output(&self.output);
        let layer = map
            .layer_for_surface(surface.wl_surface(), WindowSurfaceType::TOPLEVEL)
            .cloned();
        if let Some(layer) = layer {
            map.unmap_layer(&layer);
        }
        drop(map);
        self.update_usable_area();
    }

    /// `surface` が layer surface なら、commit に合わせて並べ直し、まだなら最初の configure を送る。
    /// layer surface だったらtrue。
    pub fn commit_layer(&mut self, surface: &WlSurface) -> bool {
        let mut map = layer_map_for_output(&self.output);
        let Some(layer) = map
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .cloned()
        else {
            return false;
        };
        // anchor、exclusive zone、margin、大きさの変更は、commit で効く。
        map.arrange();
        drop(map);
        let initial_configure_sent = with_states(surface, |states| {
            states
                .data_map
                .get::<LayerSurfaceData>()
                .is_some_and(|data| data.lock().expect("the layer data").initial_configure_sent)
        });
        if !initial_configure_sent {
            layer.layer_surface().send_configure();
        }
        self.update_usable_area();
        true
    }

    /// 使える範囲を求め直す。変わったら、ウィンドウの位置を直して、trueを返す。
    ///
    /// スロットにあるウィンドウは新しい範囲の同じスロットへ、動かされたウィンドウはタイトルバーが範囲に
    /// 入るように動かす。configure で伝えた大きさは、送り直さない(新しいウィンドウから、新しい範囲で伝える)。
    pub fn update_usable_area(&mut self) -> bool {
        let zone = layer_map_for_output(&self.output).non_exclusive_zone();
        let usable = rect_from(zone);
        if usable == self.usable {
            return false;
        }
        info!(
            "usable area: {}x{} at ({}, {})",
            usable.size.w, usable.size.h, usable.loc.x, usable.loc.y
        );
        self.usable = usable;
        self.windows
            .rearrange(usable, window_size, clamp_outer_location);
        // 最大化しているウィンドウには、新しい範囲の大きさを伝え直す。
        for window in self.windows.iter().filter(|window| window.is_maximized()) {
            let size = content_size_for(usable.size, bar_height(decorated_pending(&window.item)));
            window.item.with_pending_state(|state| {
                state.size = Some((size.w, size.h).into());
            });
            if window.item.is_initial_configure_sent() {
                window.item.send_pending_configure();
            }
        }
        true
    }

    /// 出力(画面)の大きさを `size` に変える。変わったらtrue(呼ぶ側が、再描画と、描く先の大きさを合わせる)。
    ///
    /// wl_output と xdg-output のモード(大きさ)を送り直し、layer surface を並べ直す(大きさの変わった layer
    /// surface には configure が届く)。使える範囲を求め直し、ウィンドウを新しい範囲に合わせる(最大化している
    /// ウィンドウには新しい大きさを configure で伝え、スロットのウィンドウはそのスロットへ、動かされたウィンドウは
    /// タイトルバーが範囲に入るように戻す)。
    pub fn resize_output(&mut self, size: Size) -> bool {
        use smithay::output::Mode;
        if size == self.size || size.w <= 0 || size.h <= 0 {
            return false;
        }
        let mode = |size: Size| Mode {
            size: (size.w, size.h).into(),
            refresh: 60_000,
        };
        let old = mode(self.size);
        let new = mode(size);
        self.output
            .change_current_state(Some(new), None, None, None);
        self.output.set_preferred(new);
        // 古いモードは、これからつなぐクライアントに見せない(大きさを変え続けても、モードが溜まらない)。
        self.output.delete_mode(old);
        self.size = size;
        info!("output: {}x{}", size.w, size.h);
        layer_map_for_output(&self.output).arrange();
        self.update_usable_area();
        true
    }

    /// 去ったウィンドウを、スタックから外す。その下にあったものが見えるようになる。
    ///
    /// 去ったのがいちばん手前なら、その下にあったものが activated なものになる。
    pub fn remove_window(&mut self, toplevel: &ToplevelSurface) {
        if let Some(window) = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == toplevel.wl_surface())
        {
            self.resize_anchors.remove(&window.id);
        }
        self.windows
            .remove(|window| window.wl_surface() == toplevel.wl_surface());
        self.title_bars.remove(&toplevel.wl_surface().id());
        self.shadows.remove(&toplevel.wl_surface().id());
        self.opened.remove(&toplevel.wl_surface().id());
        self.forget_closed_candidates();
        self.pings.forget(|client| !client.alive());
        self.update_activation();
    }

    /// いちばん手前のウィンドウにだけ、activatedの状態を付ける。変わったウィンドウには、configureで知らせる。
    /// ウィンドウの一覧(foreign-toplevel)も合わせる。
    fn update_activation(&mut self) {
        // ポップアップの連なりの根元がウィンドウで、activated でなくなったら(別のウィンドウへフォーカスが移ったら)、
        // 連なりを閉じる。
        if let Some(chain) = &self.popup_grab {
            let root_is_window = self
                .windows
                .iter()
                .any(|w| w.item.wl_surface() == &chain.root);
            let root_is_active = self
                .windows
                .iter_with_activation()
                .any(|(w, active)| active && w.item.wl_surface() == &chain.root);
            if root_is_window && !root_is_active {
                self.dismiss_popup_grab();
            }
        }
        for (window, active) in self.windows.iter_with_activation() {
            let toplevel = &window.item;
            toplevel.with_pending_state(|state| {
                if active {
                    state.states.set(xdg_toplevel::State::Activated);
                } else {
                    state.states.unset(xdg_toplevel::State::Activated);
                }
            });
            // まだ最初のconfigureを送っていないウィンドウには、最初のconfigureと一緒に伝わる。
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        self.update_foreign_toplevels();
    }

    /// ウィンドウの一覧(foreign-toplevel)を、いまのウィンドウに合わせる。一覧に載せるのは、絵を出した
    /// ウィンドウだけ。タイトル・app_id・activated かどうかが変わったら、シェルの部品へ送る。
    pub fn update_foreign_toplevels(&mut self) {
        let now = self
            .windows
            .iter_with_activation()
            .filter(|(window, _)| window_size(&window.item).is_some())
            .map(|(window, activated)| {
                let (title, app_id) = toplevel_names(&window.item);
                Listing {
                    window: window.id,
                    title,
                    app_id,
                    activated,
                    maximized: window.is_maximized(),
                    minimized: window.minimized,
                }
            })
            .collect();
        self.foreign_toplevels
            .update::<D>(&self.display_handle, &self.output, now);
    }

    /// `surface` がウィンドウなら、commit に合わせる。サイズ変更で左か上の辺を動かしているときは、反対の辺の
    /// 位置を保つように動かす。絵を出した・外したことで一覧に載るかどうかが変わったら、一覧を合わせる。
    pub fn commit_window(&mut self, surface: &WlSurface) {
        self.send_initial_configure(surface);
        self.fit_opened_window(surface);
        self.follow_resize(surface);
        let changed = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == surface)
            .is_some_and(|window| {
                window_size(&window.item).is_some() != self.foreign_toplevels.is_listed(window.id)
            });
        if changed {
            self.update_foreign_toplevels();
        }
    }

    /// ウィンドウ `surface` の最初の commit で、最初の configure を送る(xdg-shell の決まり)。大きさは 0x0(クライアントに
    /// 任せる。重ねて使う画面では、アプリが自分の大きさで開くのがふつう)で、activated の状態を伝える。最大化・全画面を
    /// 頼まれていれば(最初の commit の前の set_maximized など)、今までどおり、使える範囲から取り決めた装飾のタイトル
    /// バーのぶんを引いた大きさを伝える。
    fn send_initial_configure(&mut self, surface: &WlSurface) {
        let Some(window) = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == surface)
        else {
            return;
        };
        let toplevel = &window.item;
        if toplevel.is_initial_configure_sent() {
            return;
        }
        let size = content_size_for(self.usable.size, bar_height(decorated_pending(toplevel)));
        toplevel.with_pending_state(|state| {
            let filled = state.states.contains(xdg_toplevel::State::Maximized)
                || state.states.contains(xdg_toplevel::State::Fullscreen);
            state.size = filled.then(|| (size.w, size.h).into());
        });
        toplevel.send_configure();
    }

    /// ウィンドウ `surface` が最初の絵を出した(開いた)とき、その大きさが使える範囲より大きければ、範囲に収まる大きさ
    /// (使える範囲から、タイトルバーのぶんを引いたもの。はみ出した向きだけ縮める)を configure で伝え直す。1 回だけ見る。
    /// 置く位置は、置き方の決まりのまま。最大化・全画面のウィンドウは見ない。
    fn fit_opened_window(&mut self, surface: &WlSurface) {
        let Some(window) = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == surface)
        else {
            return;
        };
        let toplevel = &window.item;
        let Some(shape) = shape(toplevel) else {
            return;
        };
        if !self.opened.insert(surface.id()) {
            return;
        }
        let states = toplevel.current_state().states;
        if states.contains(xdg_toplevel::State::Maximized)
            || states.contains(xdg_toplevel::State::Fullscreen)
        {
            return;
        }
        let room = content_size_for(self.usable.size, shape.bar);
        let fitted = Size::new(shape.content.w.min(room.w), shape.content.h.min(room.h));
        if fitted == shape.content {
            return;
        }
        info!(
            "a window opened at {}x{}, larger than the usable area; asking for {}x{}",
            shape.content.w, shape.content.h, fitted.w, fitted.h
        );
        toplevel.with_pending_state(|state| {
            state.size = Some((fitted.w, fitted.h).into());
        });
        toplevel.send_pending_configure();
    }

    /// サイズ変更の最中のウィンドウ `surface` の、新しい大きさに合わせて位置を直す。
    fn follow_resize(&mut self, surface: &WlSurface) {
        let Some((id, content, resizing)) = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == surface)
            .map(|window| {
                let resizing = window
                    .item
                    .current_state()
                    .states
                    .contains(xdg_toplevel::State::Resizing);
                (window.id, content_size(&window.item), resizing)
            })
        else {
            return;
        };
        let Some(start) = self.resize_anchors.get(&id).copied() else {
            return;
        };
        if let Some(content) = content {
            self.windows.move_to(id, anchored_location(start, content));
        }
        // 最後の configure(resizing の無いもの)に応じた commit で、終わりにする。
        let grabbed = self
            .interaction
            .resizing()
            .is_some_and(|(window, _)| window == id);
        if !grabbed && !resizing {
            self.resize_anchors.remove(&id);
        }
    }

    /// サイズ変更の大きさを、クライアントへ configure で伝える。
    fn send_resize_configure(&mut self, configure: Configure) {
        let Some(window) = self.windows.get(configure.window) else {
            return;
        };
        window.item.with_pending_state(|state| {
            state.size = Some((configure.size.w, configure.size.h).into());
            if configure.resizing {
                state.states.set(xdg_toplevel::State::Resizing);
            } else {
                state.states.unset(xdg_toplevel::State::Resizing);
            }
        });
        window.item.send_pending_configure();
    }

    /// ポインターの操作の結果のうち、サイズ変更のものを行う。
    fn apply_resize(&mut self, configure: Option<Configure>) {
        if let Some((window, start)) = self.interaction.resizing() {
            self.resize_anchors.entry(window).or_insert(start);
        }
        if let Some(configure) = configure {
            self.send_resize_configure(configure);
        }
    }

    /// いまのポインターの形(サイズ変更の境界の上か、サイズ変更の最中は、その向き)。バックエンドが、カーソルの
    /// 絵を選ぶのに使う。
    pub fn cursor(&self) -> Cursor {
        self.interaction.cursor()
    }

    /// ウィンドウ `id` を raise して activated にする(foreign-toplevel の activate)。スタックが変わったらtrue。
    ///
    /// 最小化しているウィンドウは、戻してから raise する。
    pub fn activate(&mut self, id: WindowId) -> bool {
        if self.windows.get(id).is_some_and(|window| window.minimized) {
            return self.unminimize(id);
        }
        let Some(index) = self.windows.position(id) else {
            return false;
        };
        if !self.windows.raise(index) {
            return false;
        }
        self.update_activation();
        true
    }

    /// Alt+Tab(`backward` なら Alt+Shift+Tab)で、次(前)のウィンドウを候補にする。切り替えの最中でなければ、
    /// 始める。候補は、いちばん手前に見せるだけで、raise・activated・キーボードのフォーカスは、決めるまで変えない。
    /// 見せるものが変わったらtrue。
    ///
    /// 候補は、始めたときの、絵を出しているウィンドウ(最小化しているものも)。ウィンドウが無ければ、始めない。
    pub fn switch_windows(&mut self, backward: bool) -> bool {
        // 切り替えを始めたら、ポップアップの連なりを閉じる(フォーカスは、切り替えを決めたときに移る)。
        self.dismiss_popup_grab();
        match &mut self.switcher {
            Some(switcher) => switcher.step(backward),
            None => {
                let candidates = self
                    .windows
                    .switch_order()
                    .into_iter()
                    .filter(|&id| {
                        self.windows
                            .get(id)
                            .is_some_and(|window| window_size(&window.item).is_some())
                    })
                    .collect();
                self.switcher = Switcher::start(candidates, backward);
            }
        }
        let candidate = self.switcher.as_ref().map(Switcher::current);
        self.windows.set_preview(candidate)
    }

    /// 切り替えを決める。候補を(最小化していれば戻して)raise し、activated にする。切り替えの最中でなければ、
    /// 何もしない。見せるものかスタックが変わったらtrue。
    pub fn finish_switch(&mut self) -> bool {
        let Some(switcher) = self.switcher.take() else {
            return false;
        };
        let shown = self.windows.set_preview(None);
        self.activate(switcher.current()) || shown
    }

    /// 切り替えを取り消す。重なる順とフォーカスは、始める前のまま(見せていた候補を外すだけ)。見せるものが
    /// 変わったらtrue。
    pub fn cancel_switch(&mut self) -> bool {
        self.switcher.take().is_some() && self.windows.set_preview(None)
    }

    /// 切り替えの候補から、去ったウィンドウを外す。見せていた候補が去ったら、次の候補を見せる。候補が
    /// 無くなったら、切り替えを終える。
    fn forget_closed_candidates(&mut self) {
        let Some(switcher) = &mut self.switcher else {
            return;
        };
        let windows = &self.windows;
        if switcher.remove(|id| windows.get(id).is_none()) {
            let candidate = switcher.current();
            self.windows.set_preview(Some(candidate));
        } else {
            self.switcher = None;
            self.windows.set_preview(None);
        }
    }

    /// キーボードのフォーカスを付ける画面。先に見るものから順に:grab したポップアップの連なりのいちばん上、
    /// キーを独り占めする layer surface(top か overlay の層で exclusive)、押したときにフォーカスを受けた layer surface
    /// (on_demand)、activated のウィンドウ(最小化していないうちの、いちばん手前)で、絵を出しているもの。無ければNone。
    pub fn keyboard_focus_target(&self) -> Option<WlSurface> {
        if let Some(popup) = self.popup_grab_focus() {
            return Some(popup);
        }
        if let Some(layer) = self.exclusive_layer() {
            return Some(layer);
        }
        if let Some(layer) = self.layer_focus_alive() {
            return Some(layer);
        }
        self.windows
            .iter_with_activation()
            .find(|(_, activated)| *activated)
            .map(|(window, _)| &window.item)
            .filter(|toplevel| window_size(toplevel).is_some())
            .map(|toplevel| toplevel.wl_surface().clone())
    }

    /// ウィンドウの測り方(操作の決まりが使う)。
    pub fn measure(&self) -> Toplevels {
        Toplevels {
            minimizable: self.foreign_toplevels.has_managers(),
        }
    }

    /// ウィンドウ `id` を最大化する(`maximized` がtrue)か、元に戻す。変わったらtrue。
    ///
    /// 最大化は、使える範囲の左上に置き、使える範囲からタイトルバーの高さを引いた大きさを、maximized の状態
    /// つきの configure で伝える。元に戻すと、最大化する前の位置とスロット番号に戻し、そのときのコンテンツの
    /// 大きさを伝える。大きさの決まったウィンドウ(最小と最大が同じ)は、最大化しない。
    pub fn set_maximized(&mut self, id: WindowId, maximized: bool) -> bool {
        let Some(toplevel) = self.windows.get(id).map(|window| window.item.clone()) else {
            return false;
        };
        if maximized {
            if !can_maximize(&toplevel) {
                return false;
            }
            // 元に戻すときの大きさは、いま描いている大きさ(まだ描いていなければ、伝えていた大きさ)。
            let content = content_size(&toplevel).or_else(|| {
                toplevel
                    .with_pending_state(|state| state.size)
                    .map(|size| Size::new(size.w, size.h))
            });
            if !self.windows.maximize(id, self.usable, content) {
                return false;
            }
            self.resize_anchors.remove(&id);
            let size = content_size_for(self.usable.size, bar_height(decorated_pending(&toplevel)));
            toplevel.with_pending_state(|state| {
                state.size = Some((size.w, size.h).into());
                state.states.set(xdg_toplevel::State::Maximized);
            });
        } else {
            let Some(restore) = self.windows.unmaximize(id, self.usable) else {
                return false;
            };
            // 動かされていたウィンドウは、タイトルバーがいまの使える範囲に入るようにする。
            if restore.slot.is_none() {
                let width = restore
                    .content
                    .or_else(|| content_size(&toplevel))
                    .map_or(1, |size| size.w);
                let location = clamp_outer_location(restore.location, width, self.usable);
                self.windows.move_to(id, location);
            }
            toplevel.with_pending_state(|state| {
                state.size = restore.content.map(|size| (size.w, size.h).into());
                state.states.unset(xdg_toplevel::State::Maximized);
            });
        }
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.update_foreign_toplevels();
        true
    }

    /// ウィンドウ `id` を最小化する。描かず、ポインターも当たらず、activated は次のウィンドウへ移る。変わったら
    /// true。戻す手段(foreign-toplevel の manager を bind しているシェルの部品)が無ければ、最小化しない。
    pub fn minimize(&mut self, id: WindowId) -> bool {
        if !self.foreign_toplevels.has_managers() || !self.windows.set_minimized(id, true) {
            return false;
        }
        self.interaction.forget_hover();
        self.update_activation();
        true
    }

    /// 最小化しているウィンドウ `id` を戻し、raise して activated にする。変わったらtrue。
    pub fn unminimize(&mut self, id: WindowId) -> bool {
        if !self.windows.set_minimized(id, false) {
            return false;
        }
        if let Some(index) = self.windows.position(id) {
            self.windows.raise(index);
        }
        self.update_activation();
        true
    }

    /// foreign-toplevel の manager を bind しているシェルの部品が、増えたか減ったときに呼ぶ。
    ///
    /// 最小化できるかどうか(ボタンと、クライアントに伝える wm_capabilities)を合わせる。シェルの部品が
    /// 1 つも無くなったら、戻す手段が無くなるので、最小化しているウィンドウを全部戻す。
    pub fn managers_changed(&mut self) {
        let minimizable = self.foreign_toplevels.has_managers();
        let mut capabilities = vec![xdg_toplevel::WmCapabilities::Maximize];
        if minimizable {
            capabilities.push(xdg_toplevel::WmCapabilities::Minimize);
        }
        self.xdg_shell_state.replace_capabilities(capabilities);
        if !minimizable {
            let minimized: Vec<WindowId> = self
                .windows
                .iter()
                .filter(|window| window.minimized)
                .map(|window| window.id)
                .collect();
            for id in minimized {
                self.windows.set_minimized(id, false);
            }
            self.update_activation();
        }
    }

    /// `toplevel` のウィンドウの番号。
    pub fn window_id(&self, toplevel: &ToplevelSurface) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|window| window.item.wl_surface() == toplevel.wl_surface())
            .map(|window| window.id)
    }

    /// ウィンドウ `id` のクライアントに、閉じるように頼む(閉じるボタンと、foreign-toplevel の close)。応じるか
    /// どうかは、クライアントしだい。
    ///
    /// あわせて、そのクライアントへ ping を送り、pong を待ち始める(すでに待っていれば、送り直さない)。決まった
    /// 時間の中に返らなければ、[`check_pings`] が応答なしの印を付ける。
    pub fn request_close(&mut self, id: WindowId) {
        let Some(window) = self.windows.get(id) else {
            return;
        };
        window.item.send_close();
        let client = window.item.client();
        if self.pings.is_waiting(&client) {
            return;
        }
        match client.send_ping(SERIAL_COUNTER.next_serial()) {
            Ok(()) => self.pings.sent(client, Instant::now()),
            Err(e) => warn!("cannot ping a client: {e:?}"),
        }
    }

    /// ウィンドウ `id` の閉じるボタンが押された。応答なしの印が付いたクライアントなら、Wayland の接続を切る
    /// (プロセスは kill しない)。そうでなければ、閉じるように頼む([`Frontend::request_close`])。
    ///
    /// 切ったクライアントのウィンドウは、接続のクリーンアップ(バックエンドが [`reap_dead_clients`] で行う)で、全部
    /// 消える。
    pub fn close_button(&mut self, id: WindowId) {
        let Some(window) = self.windows.get(id) else {
            return;
        };
        let client = window.item.client();
        if !self.pings.is_unresponsive(&client) {
            self.request_close(id);
            return;
        }
        // ふだんのログには、ウィンドウのタイトルを書かない(ADR 0004 の 1.6)。接続とオブジェクトの id と、切る理由を
        // 書く。
        let surface = window.item.wl_surface();
        let connection = surface.client().map(|client| client.id());
        warn!(
            "disconnecting a client that does not respond (connection {connection:?}, {}): the close button was pressed after it did not answer a ping; its process is not killed",
            window.item.xdg_toplevel().id()
        );
        // xdg_wm_base の unresponsive のエラーを送り、接続を切る。
        if let Err(e) = client.unresponsive() {
            warn!("the client was already gone: {e:?}");
        }
        self.pings.forget(|other| *other == client);
        self.reap_requested = true;
    }

    /// ウィンドウ `toplevel` のクライアントに、応答なしの印が付いているか。
    pub fn is_unresponsive(&self, toplevel: &ToplevelSurface) -> bool {
        self.pings.is_unresponsive(&toplevel.client())
    }

    /// ウィンドウの画面 `surface` のコンテンツの、画面の上の矩形。IME の候補ウィンドウを置くのに使う。ウィンドウで
    /// なければ、大きさ 0 の矩形。
    pub fn parent_geometry(&self, surface: &WlSurface) -> Rectangle<i32, Logical> {
        self.windows
            .iter()
            .find(|window| window.item.wl_surface() == surface)
            .and_then(|window| {
                let shape = shape(&window.item)?;
                let location = surface_origin(window.location, shape);
                let size = shape.content;
                Some(Rectangle::new(
                    (location.x, location.y).into(),
                    (size.w, size.h).into(),
                ))
            })
            .unwrap_or_default()
    }

    /// ポインターの操作の結果の `command` を、ウィンドウ `id` に行う。
    fn apply_command(&mut self, id: WindowId, command: Command) {
        match command {
            // 閉じるように頼むだけ。応じるかどうかは、クライアントしだい。
            // 応答なしのクライアントなら、接続を切る。
            Command::Close => self.close_button(id),
            Command::Minimize => {
                self.minimize(id);
            }
            Command::ToggleMaximize => {
                let maximized = self
                    .windows
                    .get(id)
                    .is_some_and(|window| window.is_maximized());
                self.set_maximized(id, !maximized);
            }
            Command::Maximize => {
                self.set_maximized(id, true);
            }
            // 最大化しているウィンドウを、タイトルバーか、クライアントから頼まれた移動でドラッグした。元の大きさに
            // 戻し、ポインターの下へ置く。
            Command::Restore { location } => {
                if self.set_maximized(id, false) {
                    self.windows.move_to(id, location);
                }
            }
        }
    }

    /// 見せている IME の候補ウィンドウと、画面の上の位置(左上)。親のウィンドウが見えていない(最小化している)ときと、
    /// 候補ウィンドウがまだ絵を出していないときはNone。
    ///
    /// 位置は、親のウィンドウのいまのコンテンツの位置に、アプリが伝えたカーソルの矩形を足し、[`popup_location`] で
    /// 決める(ウィンドウを動かしたときも、出力の大きさが変わったときも、描くたびに求め直す)。
    pub fn ime_popup(&self) -> Option<(WlSurface, Point)> {
        let popup = self.ime.popup.as_ref().filter(|popup| popup.alive())?;
        let parent = popup.get_parent()?;
        let window = self
            .windows
            .iter()
            .find(|window| !window.minimized && window.item.wl_surface() == &parent.surface)?;
        let content = surface_origin(window.location, shape(&window.item)?);
        let size: SmithaySize<i32, Logical> =
            with_renderer_surface_state(popup.wl_surface(), |state| state.surface_size())??;
        let cursor = popup.text_input_rectangle();
        let cursor = Rect::new(
            (content.x + cursor.loc.x, content.y + cursor.loc.y).into(),
            (cursor.size.w, cursor.size.h).into(),
        );
        let location = popup_location(self.size, cursor, Size::new(size.w, size.h));
        Some((popup.wl_surface().clone(), location))
    }

    /// 画面の上の点 `point` にある、いちばん手前のウィンドウか、そのポップアップ。ウィンドウは手前から順に見て、
    /// 各々のウィンドウのポップアップを、そのウィンドウより先に見る(ポップアップは、根元のウィンドウのすぐ手前)。
    fn window_hit(&self, point: PointF) -> Option<WindowHit> {
        for (entry, _) in self.windows.iter_shown() {
            let Some(shape) = shape(&entry.item) else {
                continue;
            };
            let content = content_location(entry.location, shape.bar);
            if let Some((surface, origin)) =
                self.popup_under(entry.item.wl_surface(), content, point)
            {
                return Some(WindowHit::Popup(surface, origin));
            }
            let outer = Rect::new(entry.location, outer_size(shape.content, shape.bar));
            if frame_contains(outer, shape.bar > 0 && !entry.is_maximized(), point) {
                return Some(if point.y >= f64::from(content.y) {
                    let origin = surface_origin(entry.location, shape).to_f64();
                    WindowHit::Content(entry.item.wl_surface().clone(), origin)
                } else {
                    WindowHit::Decoration
                });
            }
        }
        None
    }

    /// 点 `point` が、ウィンドウのポップアップの上か(ウィンドウの操作の前に、ポップアップへ渡す)。
    pub fn window_popup_at(&self, point: PointF) -> bool {
        matches!(self.window_hit(point), Some(WindowHit::Popup(..)))
    }

    /// 画面の上の点 `point` にある、いちばん手前のウィンドウのコンテンツか、そのポップアップ。その画面と、画面の左上の
    /// 位置を返す。
    ///
    /// 点がタイトルバーの上にあるときは、Noneを返す(タイトルバーはSeinasのもので、クライアントには渡さない)。
    pub fn window_under(&self, point: PointF) -> Option<(WlSurface, PointF)> {
        match self.window_hit(point)? {
            WindowHit::Popup(surface, origin) | WindowHit::Content(surface, origin) => {
                Some((surface, origin))
            }
            WindowHit::Decoration => None,
        }
    }

    /// 画面の上の点 `point` にある、層が `layers` のどれかの layer surface(手前から探す)。その画面と、
    /// 左上の位置を返す。
    pub fn layer_under(&self, point: PointF, layers: &[Layer]) -> Option<(WlSurface, PointF)> {
        let map = layer_map_for_output(&self.output);
        layers.iter().find_map(|&layer| {
            map.layers_on(layer).rev().find_map(|mapped| {
                let geometry = rect_from(map.layer_geometry(mapped)?);
                // layer surface のポップアップは、その layer surface のすぐ手前。
                if let Some(found) = self.popup_under(mapped.wl_surface(), geometry.loc, point) {
                    return Some(found);
                }
                geometry
                    .contains(point)
                    .then(|| (mapped.wl_surface().clone(), geometry.loc.to_f64()))
            })
        })
    }

    /// 画面の上の点 `point` で、ポインターのイベントを受け取る画面。手前から順に、overlay の layer surface、
    /// IME の候補ウィンドウ、top の layer surface、ウィンドウのコンテンツ、bottom と background の layer surface。
    pub fn surface_under(&self, point: PointF) -> Option<(WlSurface, PointF)> {
        if let Some(found) = self.layer_under(point, &[Layer::Overlay]) {
            return Some(found);
        }
        if let Some(found) = self.ime_popup_under(point) {
            return Some(found);
        }
        if let Some(found) = self.layer_under(point, &[Layer::Top]) {
            return Some(found);
        }
        if self.window_hit(point).is_some() {
            // ウィンドウの上なら、タイトルバーの上でも、その下の層には渡さない。
            return self.window_under(point);
        }
        self.layer_under(point, &[Layer::Bottom, Layer::Background])
    }

    /// 切り替えの最中なら、候補の一覧の描画の要素(`switcher_view.rs`)。条件が変わったときだけ絵を作り直す。
    fn switcher_element(
        &mut self,
        renderer: &mut PixmanRenderer,
    ) -> Option<MemoryRenderBufferRenderElement<PixmanRenderer>> {
        let Some(switcher) = &self.switcher else {
            self.switcher_image = None;
            return None;
        };
        let (candidates, current) = switcher.candidates();
        let entries: Vec<SwitcherEntry> = candidates
            .iter()
            .filter_map(|&id| self.windows.get(id))
            .map(|window| SwitcherEntry {
                title: toplevel_title(&window.item),
                minimized: window.minimized,
            })
            .collect();
        let fresh = self
            .switcher_image
            .as_ref()
            .is_some_and(|image| image.matches(&entries, current, self.size));
        if !fresh {
            self.switcher_image = SwitcherImage::new(&mut self.text, entries, current, self.size);
        }
        self.switcher_image.as_ref()?.element(renderer)
    }

    /// 点 `point` にある IME の候補ウィンドウと、その左上の位置。
    fn ime_popup_under(&self, point: PointF) -> Option<(WlSurface, PointF)> {
        let (surface, location) = self.ime_popup()?;
        let size = with_renderer_surface_state(&surface, |state| state.surface_size())??;
        Rect::new(location, Size::new(size.w, size.h))
            .contains(point)
            .then(|| (surface, location.to_f64()))
    }

    /// 画面に描くものを、描画の要素として並べる(手前から順)。手前から、overlay の layer surface、IME の候補ウィンドウ、
    /// top の layer surface、ウィンドウ(タイトルバーと、その下のコンテンツ)、bottom と background の layer surface。
    pub fn render_elements(&mut self, renderer: &mut PixmanRenderer) -> Vec<WindowElement> {
        // 切り替えの一覧は、全部より手前に描く。
        let mut elements: Vec<WindowElement> = self
            .switcher_element(renderer)
            .map(WindowElement::TitleBar)
            .into_iter()
            .collect();
        elements.extend(self.layer_elements(renderer, &[Layer::Overlay]));
        if let Some((surface, location)) = self.ime_popup() {
            elements.extend(
                render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                    renderer,
                    &surface,
                    (location.x, location.y),
                    1.0,
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(WindowElement::Surface),
            );
        }
        elements.extend(self.layer_elements(renderer, &[Layer::Top]));
        // 最大化の予告は、使える範囲の内側に沿った枠で、ウィンドウより手前に描く。
        if self.interaction.maximize_preview().is_some() {
            elements.extend(self.maximize_mark_elements().map(WindowElement::Mark));
        }
        let measure = self.measure();
        // 最小化しているウィンドウは描かない(Alt+Tab の候補として見せているものは描く)。
        for (window, active) in self.windows.iter_shown() {
            let surface = window.item.wl_surface();
            // まだ絵を出していないウィンドウには、タイトルバーも枠も影も付けない。
            let Some(shape) = shape(&window.item) else {
                continue;
            };
            let content = shape.content;
            let outer = Rect::new(window.location, outer_size(content, shape.bar));
            let location = surface_origin(window.location, shape);
            // ポップアップは、根元のウィンドウのすぐ手前に、飾りもクリップも付けずに描く。
            for (popup, at) in
                self.popup_origins(surface, content_location(window.location, shape.bar))
            {
                elements.extend(
                    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                        renderer,
                        &popup,
                        (at.x, at.y),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(WindowElement::Surface),
                );
            }
            let surfaces = render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                renderer,
                surface,
                // 拡大率は1なので、画面の上の位置とピクセルの位置は同じ。
                (location.x, location.y),
                1.0,
                1.0,
                Kind::Unspecified,
            );
            if shape.bar == 0 {
                // 装飾を取り決めていない(か、client_side の)ウィンドウには、タイトルバーも枠も角も影も付けない。
                // クライアントが自分で描く。
                elements.extend(surfaces.into_iter().map(WindowElement::Surface));
                continue;
            }
            let text = toplevel_title(&window.item);
            let (_, app_id) = toplevel_names(&window.item);
            let look = TitleLook {
                buttons: measure.buttons(&window.item),
                maximized: window.is_maximized(),
                hot: self.interaction.hot_button(window.id),
                unresponsive: self.pings.is_unresponsive(&window.item.client()),
            };
            // アイコンは、app_id ごとに初めてのときだけ探して読む。
            match self.icons.load(&app_id) {
                Some(Found::File(path)) => info!("the icon of {app_id}: {}", path.display()),
                Some(Found::Unreadable(path, reason)) => warn!(
                    "the icon of {app_id} ({}) is not used: {reason}; the default icon is used",
                    path.display()
                ),
                Some(Found::Missing) => {
                    info!("no icon for {app_id}; the default icon is used")
                }
                None => {}
            }
            let icon = self.icons.get(&app_id);
            let bar = self
                .title_bars
                .entry(surface.id())
                .and_modify(|bar| {
                    if !bar.matches(&text, content.w, active, look, &app_id) {
                        *bar = TitleBar::new(
                            &mut self.text,
                            &text,
                            content.w,
                            active,
                            look,
                            &app_id,
                            icon,
                        );
                    }
                })
                .or_insert_with(|| {
                    TitleBar::new(
                        &mut self.text,
                        &text,
                        content.w,
                        active,
                        look,
                        &app_id,
                        icon,
                    )
                });
            elements.extend(
                bar.element(renderer, window.location)
                    .map(WindowElement::TitleBar),
            );
            if window.is_maximized() {
                // 最大化しているときは、角も枠も影も付けない。
                elements.extend(surfaces.into_iter().map(WindowElement::Surface));
                continue;
            }
            elements.extend(
                surfaces
                    .into_iter()
                    .map(|element| WindowElement::Clipped(ClippedSurface::new(element, outer))),
            );
            // 枠(角の絵と、まっすぐな所の単色の矩形)。
            let (corners, color) = if active {
                (&self.frame_corners.0, ACTIVE_BORDER)
            } else {
                (&self.frame_corners.1, INACTIVE_BORDER)
            };
            let (images, lines) = border_elements(renderer, outer, corners);
            elements.extend(images.into_iter().map(WindowElement::TitleBar));
            let [r, g, b] = color.map(|c| f32::from(c) / 255.0);
            elements.extend(lines.into_iter().map(|[x, y, w, h]| {
                WindowElement::Mark(SolidColorRenderElement::new(
                    Id::new(),
                    Rectangle::new((x, y).into(), (w, h).into()),
                    0usize,
                    Color32F::from([r, g, b, 1.0]),
                    Kind::Unspecified,
                ))
            }));
            // 影(枠の外側の矩形の大きさで、作って覚える)。
            let (w, h) = (
                outer.size.w + 2 * BORDER_WIDTH,
                outer.size.h + 2 * BORDER_WIDTH,
            );
            let shadow = self
                .shadows
                .entry(surface.id())
                .and_modify(|shadow| {
                    if !shadow.matches(w, h) {
                        *shadow = Shadow::new(w, h);
                    }
                })
                .or_insert_with(|| Shadow::new(w, h));
            elements.extend(
                shadow
                    .elements(renderer, outer)
                    .into_iter()
                    .map(WindowElement::TitleBar),
            );
        }
        elements.extend(self.layer_elements(renderer, &[Layer::Bottom, Layer::Background]));
        elements
    }

    /// 最大化の予告(使える範囲の内側に沿った、太さ [`MAXIMIZE_MARK_WIDTH`] の 4 本の枠と、その内側の薄い面)を、
    /// 描画の要素にする。面は、不透明度 [`MAXIMIZE_FACE_OPACITY`] で後ろに重ねる(色は premultiplied で渡す)。
    fn maximize_mark_elements(&mut self) -> impl Iterator<Item = SolidColorRenderElement> + '_ {
        let area = self.usable;
        let w = MAXIMIZE_MARK_WIDTH.min(area.size.w).min(area.size.h);
        let (x, y, width, height) = (area.loc.x, area.loc.y, area.size.w, area.size.h);
        let [r, g, b] = MAXIMIZE_MARK.map(|c| c as f32 / 255.0);
        let a = MAXIMIZE_FACE_OPACITY;
        let (line, face) = ([r, g, b, 1.0], [r * a, g * a, b * a, a]);
        // 手前から、4 本の枠と、内側の面。
        let parts = [
            (x, y, width, w, line),
            (x, y + height - w, width, w, line),
            (x, y + w, w, height - 2 * w, line),
            (x + width - w, y + w, w, height - 2 * w, line),
            (x + w, y + w, width - 2 * w, height - 2 * w, face),
        ];
        self.maximize_marks
            .iter_mut()
            .zip(parts)
            .map(move |(buffer, (x, y, w, h, color))| {
                buffer.update((w.max(0), h.max(0)), color);
                SolidColorRenderElement::from_buffer(buffer, (x, y), 1.0, 1.0, Kind::Unspecified)
            })
    }

    /// 層が `layers` の layer surface を、描画の要素として並べる(`layers` の順に、同じ層の中は手前から)。
    fn layer_elements(
        &self,
        renderer: &mut PixmanRenderer,
        layers: &[Layer],
    ) -> Vec<WindowElement> {
        let map = layer_map_for_output(&self.output);
        let mut elements = Vec::new();
        for &layer in layers {
            // 同じ層の中では、後から加わったものが手前。
            for mapped in map.layers_on(layer).rev() {
                let Some(geometry) = map.layer_geometry(mapped) else {
                    continue;
                };
                // layer surface のポップアップ(メニューなど)は、その layer surface のすぐ手前。
                let origin = Point::new(geometry.loc.x, geometry.loc.y);
                for (popup, at) in self.popup_origins(mapped.wl_surface(), origin) {
                    elements.extend(
                        render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                            renderer,
                            &popup,
                            (at.x, at.y),
                            1.0,
                            1.0,
                            Kind::Unspecified,
                        )
                        .into_iter()
                        .map(WindowElement::Surface),
                    );
                }
                elements.extend(
                    render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<_>>(
                        renderer,
                        mapped.wl_surface(),
                        (geometry.loc.x, geometry.loc.y),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(WindowElement::Surface),
                );
            }
        }
        elements
    }

    /// クライアントが、装飾のオブジェクトを作った・モードを求めた(client_side でも)・求めを取り消したときに呼ぶ。
    /// いつも、サーバーの側(Seinas)で描くと答える(xdg-decoration の決まりで、コンポジタはクライアントの希望と
    /// 違うモードを選んでよい)。
    ///
    /// 答えは次の configure で伝え、Seinas が描き始めるのは、クライアントがそれに応じた commit から(`decorated`)。
    /// 最初の configure をもう送っていれば、すぐ送る。最大化しているウィンドウには、タイトルバーのぶんを引いた
    /// 大きさを、一緒に伝え直す。
    pub fn decorate_on_server_side(&self, toplevel: &ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
            if state.states.contains(xdg_toplevel::State::Maximized) {
                let size = content_size_for(self.usable.size, TITLE_BAR_HEIGHT);
                state.size = Some((size.w, size.h).into());
            }
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    /// クライアントへframeコールバックを返す(「次の絵を描いてよい」の通知)。
    pub fn send_frame_callbacks(&self, elapsed: Duration) {
        let time = elapsed.as_millis() as u32;
        let mut layers: Vec<WlSurface> = layer_map_for_output(&self.output)
            .layers()
            .map(|layer| layer.wl_surface().clone())
            .collect();
        // IME の候補ウィンドウは、見せているときだけ。
        layers.extend(self.ime_popup().map(|(surface, _)| surface));
        // 最小化しているウィンドウには返さない(描いていないので)。
        let windows = self
            .windows
            .iter()
            .filter(|window| !window.minimized)
            .map(|window| window.item.wl_surface());
        for surface in windows.chain(layers.iter()) {
            with_surface_tree_downward(
                surface,
                (),
                |_, _, &()| TraversalAction::DoChildren(()),
                |_, states, &()| {
                    for callback in states
                        .cached_state
                        .get::<SurfaceAttributes>()
                        .current()
                        .frame_callbacks
                        .drain(..)
                    {
                        callback.done(time);
                    }
                },
                |_, _, &()| true,
            );
        }
    }
}

/// ポインターのイベント。バックエンド(親のWayland、テスト用の入力インターフェース)が、フロントエンドへ渡す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerInput {
    /// 画面の上の点へ動いた(入ってきたときも、これ)。
    Motion(PointF),
    /// 画面の外へ出た。
    Leave,
    /// 点 `location` で、ボタン `button`(Linuxの入力の番号)が押された・離された。
    Button {
        location: PointF,
        button: u32,
        pressed: bool,
    },
}

/// ポインターのイベントを処理する。`time` は、起動からの時間(ミリ秒)。
///
/// 押した所のウィンドウを raise し、タイトルバーと閉じるボタンの上での操作(動かす、閉じる)はSeinasが
/// 自分で行い、それ以外は、ポインターの下にあるウィンドウのコンテンツへ渡す。seat(wl_seat)を公開して
/// いない構成では、何もしない。
pub fn pointer_input<D>(state: &mut D, input: PointerInput, time: u32)
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    // Alt+Tab での切り替えの最中にボタンを押したら、切り替えを取り消してから、ふだんどおりに扱う。
    if matches!(input, PointerInput::Button { pressed: true, .. })
        && state.frontend_mut().cancel_switch()
    {
        changed(state);
    }
    // ポップアップを閉じるのに使って飲み込んだ押下は、離す側も飲み込む(押下の無い離す側を、クライアントに渡さない)。
    if let PointerInput::Button {
        button,
        pressed: false,
        ..
    } = input
    {
        if state.frontend_mut().swallowed_button == Some(button) {
            state.frontend_mut().swallowed_button = None;
            return;
        }
    }
    if popup_grab_pointer(state, input, time) {
        return;
    }
    let frontend = state.frontend_mut();
    let Some(pointer) = frontend.pointer.clone() else {
        return;
    };
    let area = frontend.usable;
    let measure = frontend.measure();
    // ボタンを押していない間に、top か overlay の layer surface(タスクバーなど)か、ウィンドウのポップアップの上に
    // あるときは、ウィンドウの操作(raise、ドラッグ、close ボタン)をせず、そのまま渡す。
    let over_layer_above = |frontend: &Frontend<D>, point: PointF| {
        frontend.interaction.is_idle()
            && (frontend
                .layer_under(point, &[Layer::Overlay, Layer::Top])
                .is_some()
                || frontend.window_popup_at(point))
    };
    let (location, outcome) = match input {
        PointerInput::Motion(location) if over_layer_above(frontend, location) => {
            let mut outcome = frontend.interaction.leave();
            outcome.deliver = Deliver::ToClient;
            (location, outcome)
        }
        PointerInput::Button {
            location,
            pressed: true,
            ..
        } if over_layer_above(frontend, location) => (location, Outcome::to_client()),
        PointerInput::Motion(location) => (
            location,
            frontend
                .interaction
                .motion(&mut frontend.windows, area, location, &measure),
        ),
        PointerInput::Leave => (
            point_from(pointer.current_location()),
            frontend.interaction.leave(),
        ),
        PointerInput::Button {
            location,
            button,
            pressed: true,
        } => {
            let drag_modifier = drag_modifier_held(frontend);
            (
                location,
                frontend.interaction.press_with(
                    &mut frontend.windows,
                    location,
                    button,
                    time,
                    drag_modifier,
                    &measure,
                ),
            )
        }
        PointerInput::Button {
            location,
            button,
            pressed: false,
        } => (
            location,
            frontend
                .interaction
                .release(&mut frontend.windows, location, button, &measure),
        ),
    };
    frontend.apply_resize(outcome.configure);
    if outcome.raised {
        frontend.update_activation();
    }
    if let Some((id, command)) = outcome.command {
        frontend.apply_command(id, command);
    }
    let focus = match outcome.deliver {
        Deliver::ToClient => frontend
            .surface_under(location)
            .map(|(surface, origin)| (surface, to_smithay_point(origin))),
        Deliver::Nobody => None,
    };
    if outcome.redraw {
        state.redraw_needed();
    }
    // raise や最小化で activated のウィンドウが変わったら、キーボードのフォーカスも移す。
    sync_keyboard_focus(state);

    let serial = SERIAL_COUNTER.next_serial();
    let motion = MotionEvent {
        location: to_smithay_point(location),
        serial,
        time,
    };
    match input {
        PointerInput::Motion(_) => pointer.motion(state, focus, &motion),
        PointerInput::Leave => pointer.motion(state, None, &motion),
        PointerInput::Button {
            button, pressed, ..
        } => {
            let pressed_surface = focus.as_ref().map(|(surface, _)| surface.clone());
            if pressed {
                press_moves_layer_focus(state, pressed_surface.as_ref());
            }
            pointer.motion(state, focus, &motion);
            // Seinasが自分で使ったボタンは、クライアントには渡さない。離したイベントだけは、渡す相手を
            // 無しにしたうえで、Smithayに伝える(クライアントから頼まれた移動では、押したイベントを
            // Smithayが覚えているので、離したことも教えておく)。
            if outcome.deliver == Deliver::ToClient || !pressed {
                let state_of_button = if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                if pressed && outcome.deliver == Deliver::ToClient {
                    state.frontend_mut().remember_input_serial(serial);
                }
                pointer.button(
                    state,
                    &ButtonEvent {
                        serial,
                        time,
                        button,
                        state: state_of_button,
                    },
                );
            }
        }
    }
    pointer.frame(state);
}

/// クライアントが、xdg_toplevel で最大化・元に戻す・最小化を頼んできた。[`delegate_frontend!`] が使う。
///
/// `command` が [`Command::ToggleMaximize`] のときは、`maximized` が頼まれた状態。最大化の頼みには、応じても
/// 応じなくても configure を返す(xdg-shell の決まり)。最小化は、戻す手段が無ければ何もしない。
pub fn toplevel_request<D: FrontendHost>(
    state: &mut D,
    toplevel: &ToplevelSurface,
    command: Command,
    maximized: Option<bool>,
) {
    let frontend = state.frontend_mut();
    let Some(id) = frontend.window_id(toplevel) else {
        return;
    };
    let changed = match command {
        Command::ToggleMaximize => {
            let changed = frontend.set_maximized(id, maximized.unwrap_or(true));
            if !changed && toplevel.is_initial_configure_sent() {
                toplevel.send_configure();
            }
            changed
        }
        Command::Minimize => frontend.minimize(id),
        Command::Maximize => frontend.set_maximized(id, true),
        Command::Close | Command::Restore { .. } => false,
    };
    if changed {
        self::changed(state);
    }
}

/// クライアントが、自分のウィンドウを動かしてほしいと頼んできた(xdg_toplevelのmove)。
/// [`delegate_frontend!`] が使う。
///
/// 受けるのは、そのクライアントの上でボタンが押されている間の頼みだけ。`serial` が、いま押されている
/// ボタンのものでなければ、何もしない。
pub fn client_move_request<D>(state: &mut D, toplevel: &ToplevelSurface, serial: Serial)
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let frontend = state.frontend_mut();
    let Some((pointer, window, press)) = client_press(frontend, toplevel, serial) else {
        return;
    };
    let area = frontend.usable;
    let measure = frontend.measure();
    let Some(outcome) = frontend.interaction.begin_client_move(
        &mut frontend.windows,
        area,
        window,
        press,
        &measure,
    ) else {
        return;
    };
    // 最大化しているウィンドウで、頼みが届くまでにしきい値を越えて動いていれば、ここで元の大きさに戻す。
    if let Some((id, command)) = outcome.command {
        frontend.apply_command(id, command);
    }
    take_pointer_from_client(state, &pointer, serial);
}

/// クライアントが、自分のウィンドウの辺 `edges` でサイズ変更してほしいと頼んできた(xdg_toplevel の
/// resize)。[`delegate_frontend!`] が使う。
///
/// 受ける条件は [`client_move_request`] と同じ。サイズ変更できないウィンドウ(最大化しているものなど)と、
/// 辺の指定が無いものは、受けない。
pub fn client_resize_request<D>(
    state: &mut D,
    toplevel: &ToplevelSurface,
    serial: Serial,
    edges: xdg_toplevel::ResizeEdge,
) where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let frontend = state.frontend_mut();
    let Some((pointer, window, press)) = client_press(frontend, toplevel, serial) else {
        return;
    };
    let area = frontend.usable;
    let measure = frontend.measure();
    let Some(outcome) = frontend.interaction.begin_client_resize(
        &mut frontend.windows,
        area,
        window,
        edges_from(edges),
        press,
        &measure,
    ) else {
        return;
    };
    frontend.apply_resize(outcome.configure);
    take_pointer_from_client(state, &pointer, serial);
}

/// `serial` が、`toplevel` のクライアントの上でいま押されているボタンのものなら、ポインターと、その
/// ウィンドウと、押した点。
fn client_press<D>(
    frontend: &Frontend<D>,
    toplevel: &ToplevelSurface,
    serial: Serial,
) -> Option<(PointerHandle<D>, WindowId, PointF)>
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let pointer = frontend.pointer.clone()?;
    // 押されているボタンが、このウィンドウの上で押されたものか。押した点も、ここで分かる。
    let press = pointer
        .grab_start_data()
        .filter(|_| pointer.has_grab(serial))
        .filter(|start| {
            start.focus.as_ref().is_some_and(|(surface, _)| {
                surface.id().same_client_as(&toplevel.wl_surface().id())
            })
        })
        .map(|start| point_from(start.location))?;
    let window = frontend
        .windows
        .iter()
        .find(|window| window.item.wl_surface() == toplevel.wl_surface())?
        .id;
    Some((pointer, window, press))
}

/// ウィンドウのスタックや状態が変わった後に呼ぶ。キーボードのフォーカスを activated のウィンドウへ合わせ、
/// 再描画を頼む。
pub fn changed<D: FrontendHost>(state: &mut D) {
    sync_keyboard_focus(state);
    state.redraw_needed();
}

/// 閉じるように頼んだクライアントの pong の期限を、時刻 `now` で確かめる。期限を過ぎたクライアントに応答なしの
/// 印を付け、再描画を頼む。バックエンドが、イベントループを回るたびに呼ぶ(次の期限は
/// `frontend().pings.next_deadline()`)。
pub fn check_pings<D: FrontendHost>(state: &mut D, now: Instant) {
    let frontend = state.frontend_mut();
    if frontend.pings.expire(now) {
        let timeout = frontend.pings.timeout().as_millis();
        warn!("a client did not answer a ping in {timeout} ms; its windows are marked as not responding");
        state.redraw_needed();
    }
}

/// 応答なしかを見ていたクライアント `client` から、pong が返った。印を付けていたら外し、再描画を頼む。
/// [`delegate_frontend!`] が使う。
pub fn client_pong<D: FrontendHost>(state: &mut D, client: ShellClient) {
    if state.frontend_mut().pings.ponged(&client) {
        info!("a client that did not respond answered the ping; the mark is removed");
        state.redraw_needed();
    }
}

/// ウィンドウを動かす修飾キーが、ちょうど押されているか。
fn drag_modifier_held<D: SeatHandler + 'static>(frontend: &Frontend<D>) -> bool {
    frontend.keyboard.as_ref().is_some_and(|keyboard| {
        let now = keyboard.modifier_state();
        let held = Modifiers {
            ctrl: now.ctrl,
            alt: now.alt,
            shift: now.shift,
            logo: now.logo,
        };
        held == frontend.window_drag
    })
}

/// 押下で、layer surface のキーボードのフォーカスを合わせる。押した面 `surface`(クライアントへ渡さない押下では
/// None)が、押したときにフォーカスを受ける layer surface(on_demand)ならそれに付け、ほかを押したら外す。
fn press_moves_layer_focus<D: FrontendHost>(state: &mut D, surface: Option<&WlSurface>) {
    let frontend = state.frontend_mut();
    let takes = surface
        .filter(|surface| frontend.takes_focus_on_press(surface))
        .cloned();
    if takes != frontend.layer_focus {
        frontend.layer_focus = takes;
        sync_keyboard_focus(state);
    }
}

/// ポップアップの grab の間のポインターのイベントを扱う(xdg-shell の owner-events)。扱ったらtrue。
///
/// grab したクライアントの面にだけ、ポインターを渡す(ほかの面の上では、だれにも渡さない)。ほかの所(ほかの
/// クライアントの面、Seinas のタイトルバー、何も無い所)を押したら、連なりを閉じ、その押下は飲み込む。ウィンドウを
/// 動かす修飾キーでの押下は、閉じてから、ふだんどおりに扱う(falseを返す)。
fn popup_grab_pointer<D>(state: &mut D, input: PointerInput, time: u32) -> bool
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    let frontend = state.frontend_mut();
    if frontend.popup_grab.is_none() {
        return false;
    }
    frontend.forget_dead_popups();
    if frontend.popup_grab.is_none() {
        sync_keyboard_focus(state);
        return false;
    }
    let Some(pointer) = frontend.pointer.clone() else {
        return false;
    };
    let location = match input {
        PointerInput::Motion(location) | PointerInput::Button { location, .. } => location,
        PointerInput::Leave => return false,
    };
    let under = frontend.surface_under(location);
    let owned = frontend.popup_grab_owns(under.as_ref().map(|(surface, _)| surface)) == Some(true);
    let serial = SERIAL_COUNTER.next_serial();
    let motion = MotionEvent {
        location: to_smithay_point(location),
        serial,
        time,
    };
    if matches!(input, PointerInput::Button { pressed: true, .. }) && !owned
        || matches!(input, PointerInput::Button { pressed: true, .. })
            && drag_modifier_held(frontend)
    {
        let drag = drag_modifier_held(frontend);
        frontend.dismiss_popup_grab();
        sync_keyboard_focus(state);
        changed(state);
        if drag {
            return false;
        }
        if let PointerInput::Button { button, .. } = input {
            state.frontend_mut().swallowed_button = Some(button);
        }
        pointer.motion(state, None, &motion);
        pointer.frame(state);
        return true;
    }
    let focus = under
        .filter(|_| owned)
        .map(|(surface, origin)| (surface, to_smithay_point(origin)));
    pointer.motion(state, focus, &motion);
    if let PointerInput::Button {
        button, pressed, ..
    } = input
    {
        if pressed {
            state.frontend_mut().remember_input_serial(serial);
        }
        pointer.button(
            state,
            &ButtonEvent {
                serial,
                time,
                button,
                state: if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
            },
        );
    }
    pointer.frame(state);
    true
}

/// クライアントが、ポップアップ `surface` の grab を頼んだ(xdg_popup.grab)。seat が Seinas のもので、serial が直前の
/// 押下かキーのものなら受け、キーボードのフォーカスを、いちばん上のポップアップへ移す。[`delegate_frontend!`] が使う。
pub fn grab_popup<D>(
    state: &mut D,
    surface: smithay::wayland::shell::xdg::PopupSurface,
    seat: &smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
    serial: Serial,
) where
    D: FrontendHost + 'static,
{
    if Seat::<D>::from_resource(seat).is_none() {
        surface.send_popup_done();
        return;
    }
    match state.frontend_mut().grab_popup(surface, serial) {
        Ok(()) => info!("a popup took a grab"),
        Err(refusal) => info!("a popup grab was refused: {refusal:?}"),
    }
    sync_keyboard_focus(state);
    changed(state);
}

/// キーボードのフォーカスを、activated のウィンドウ(最小化していないうちの、いちばん手前で、絵を出したもの)に
/// 合わせる。変わったときだけ、leave と enter を送る。キーボードが無ければ、何もしない。
pub fn sync_keyboard_focus<D: FrontendHost>(state: &mut D) {
    let frontend = state.frontend();
    let Some(keyboard) = frontend.keyboard.clone() else {
        return;
    };
    let target = frontend.keyboard_focus_target();
    if keyboard.current_focus() != target {
        state.frontend_mut().ime.keyboard_focus_moved();
        keyboard.set_focus(state, target, SERIAL_COUNTER.next_serial());
    }
}

/// キーの押下か解放を受ける。`code` は evdev のキーの番号、`time` は起動からの時間(ミリ秒)。
///
/// 先に Seinas のショートカットを見て(IME とアプリへ渡す前)、使わなければ、IME が keyboard を grab していれば
/// IME へ、していなければキーボードのフォーカスのあるクライアントへ渡す(`ime.rs`)。行ったことを返す。最小化して
/// いるウィンドウには、フォーカスが無いので届かない。
///
/// ウィンドウの切り替え(`docs/window-placement.md` の「Alt+Tab」。キーは設定で変えられる):
/// - 次へ・前へのキー(既定は Alt+Tab・Alt+Shift+Tab。入れ子では Alt+F6・Alt+Shift+F6 も)は、切り替えを始めるか、
///   次・前の候補へ進める。そのキーはクライアントへ渡さない。
/// - 切り替えの最中は、修飾キーのほかのキーを渡さない。Esc は、取り消し。
/// - 切り替えを始めたキーの修飾キー(Shift を除く。既定では Alt)を離したら決める。離したことは、先にフォーカスの
///   あるクライアントへ渡し、その後でフォーカスを移す(どのクライアントにも、Alt が押されたまま残らない)。
///   Shift のほかに修飾キーの無いキーで始めたときは、その場で決める。
pub fn keyboard_input<D: FrontendHost>(
    state: &mut D,
    code: u32,
    pressed: bool,
    time: u32,
) -> Option<Action> {
    let keyboard = state.frontend().keyboard.clone()?;
    sync_keyboard_focus(state);
    let key_state = if pressed {
        KeyState::Pressed
    } else {
        KeyState::Released
    };
    let serial = SERIAL_COUNTER.next_serial();
    if pressed {
        // キーで開くメニューの grab のために覚える。
        state.frontend_mut().remember_input_serial(serial);
    }
    // Seinas が使ったショートカットのすること。
    let used = keyboard
        .input(
            state,
            (code + 8).into(),
            key_state,
            serial,
            time,
            |state, modifiers, keysym| {
                let modifiers = Modifiers {
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    logo: modifiers.logo,
                };
                let sym = keysym.modified_sym().raw();
                let raw_sym = keysym.raw_syms().first().map_or(sym, |raw| raw.raw());
                let frontend = state.frontend_mut();
                if frontend.log_keys {
                    let name = if is_modifier_key(sym) {
                        describe(Modifiers::default(), sym)
                    } else {
                        describe(modifiers, sym)
                    };
                    let what = if pressed { "pressed" } else { "released" };
                    info!("key: {name} {what}");
                }
                let switching = frontend.switcher.is_some();
                match frontend
                    .shortcuts
                    .filter(code, pressed, sym, raw_sym, modifiers)
                {
                    Decision::Use(shortcut, action) => {
                        let switch = matches!(action, Action::SwitchNext | Action::SwitchPrevious);
                        if switch && !switching {
                            frontend.switch_hold = Modifiers {
                                shift: false,
                                ..shortcut.modifiers
                            };
                        }
                        FilterResult::Intercept(Some(action))
                    }
                    Decision::Swallow => FilterResult::Intercept(None),
                    // 切り替えの最中は、修飾キーのほかのキーを渡さない(離したことも)。
                    Decision::Forward if switching && pressed && !is_modifier_key(sym) => {
                        frontend.shortcuts.hold(code);
                        FilterResult::Intercept(
                            (sym == keysym::ESCAPE).then_some(Action::CancelSwitch),
                        )
                    }
                    Decision::Forward => FilterResult::Forward,
                }
            },
        )
        .flatten();
    if used == Some(Action::ReleaseInputMethod) {
        release_ime(state);
    }
    let frontend = state.frontend_mut();
    let mut changed = match used {
        Some(Action::SwitchNext) => frontend.switch_windows(false),
        Some(Action::SwitchPrevious) => frontend.switch_windows(true),
        Some(Action::CancelSwitch) => frontend.cancel_switch(),
        Some(Action::ReleaseInputMethod) | None => false,
    };
    // 始めたキーの修飾キー(既定では Alt)を離した(離したことは、上で、いまのフォーカスへ渡してある)。修飾キーの
    // 無いキーで始めたときは、すぐ決める。
    let now = keyboard.modifier_state();
    let hold = frontend.switch_hold;
    let held = (hold.alt && now.alt) || (hold.ctrl && now.ctrl) || (hold.logo && now.logo);
    if frontend.switcher.is_some() && !held {
        changed |= frontend.finish_switch();
    }
    if changed {
        self::changed(state);
    }
    used
}

/// 親の keymap で `locked` の修飾キー(Caps Lock、Num Lock)を、子の側の状態に合わせる。入れ子の構成で、親から
/// 修飾キーの状態が届いたときに使う。`locked` は、wl_keyboard の modifiers の mods_locked(親と同じ keymap を
/// 使っているので、ビットの意味は同じ)。
///
/// 押されている修飾キー(Shift、Alt など)は、子の側でキーから求めたものを使い、親からは受けない(押したことが
/// 子に届いていないキーを押されたことにすると、離したことも届かず、押されたまま残るため)。合わせたときは、
/// フォーカスのあるクライアントへ、enter で伝え直す。変えたらtrue。
pub fn set_locked_modifiers<D: FrontendHost>(state: &mut D, locked: u32) -> bool {
    let Some(keyboard) = state.frontend().keyboard.clone() else {
        return false;
    };
    let (caps, num) = keyboard.with_xkb_state(state, |context| {
        let xkb = context.xkb().lock().expect("the xkb state");
        // SAFETY: keymap は、この中で番号を引くのに使うだけで、外へ持ち出さない。
        let keymap = unsafe { xkb.keymap() };
        let is_locked = |name: &str| {
            let index = keymap.mod_get_index(name);
            index < 32 && locked & (1 << index) != 0
        };
        (
            is_locked(smithay::input::keyboard::xkb::MOD_NAME_CAPS),
            is_locked(smithay::input::keyboard::xkb::MOD_NAME_NUM),
        )
    });
    let mut modifiers = keyboard.modifier_state();
    if (modifiers.caps_lock, modifiers.num_lock) == (caps, num) {
        return false;
    }
    modifiers.caps_lock = caps;
    modifiers.num_lock = num;
    keyboard.set_modifier_state(modifiers);
    // Smithay は、ここで変えた修飾キーの状態をクライアントへ送らないので、enter で送り直す。
    if let Some(focus) = keyboard.current_focus() {
        state.frontend_mut().ime.keyboard_focus_moved();
        keyboard.set_focus(state, None, SERIAL_COUNTER.next_serial());
        keyboard.set_focus(state, Some(focus), SERIAL_COUNTER.next_serial());
    }
    true
}

/// キーボードの keymap を、xkb の文字列(XKB_KEYMAP_FORMAT_TEXT_V1)で置き換え、クライアントへ配り直す。
/// 入れ子の構成で、親の keymap をそのまま使うのに使う。置き換えたらtrue。
pub fn set_keymap<D: FrontendHost>(state: &mut D, keymap: String) -> bool {
    let Some(keyboard) = state.frontend().keyboard.clone() else {
        return false;
    };
    match keyboard.set_keymap_from_string(state, keymap) {
        Ok(()) => true,
        Err(e) => {
            warn!("failed to use the keymap: {e:?}");
            false
        }
    }
}

/// 押されているキーを、全部離したことにする。入れ子の構成で、親のキーボードのフォーカスが外れたときに使う
/// (離したイベントが届かないまま、クライアントで繰り返しが続かないように)。
///
/// Alt+Tab での切り替えの最中なら、決めずに取り消す(Alt を離したのではないので)。
pub fn release_all_keys<D: FrontendHost>(state: &mut D, time: u32) {
    let Some(keyboard) = state.frontend().keyboard.clone() else {
        return;
    };
    if state.frontend_mut().cancel_switch() {
        changed(state);
    }
    for keycode in keyboard.pressed_keys() {
        keyboard_input(state, keycode.raw() - 8, false, time);
    }
}

/// クライアントから頼まれた移動・サイズ変更を受けた後、ボタンを離すまでのポインターのイベントを、
/// クライアントから外す(Seinas が使う)。
fn take_pointer_from_client<D>(state: &mut D, pointer: &PointerHandle<D>, serial: Serial)
where
    D: FrontendHost + SeatHandler<PointerFocus = WlSurface> + 'static,
{
    changed(state);
    let location = pointer.current_location();
    pointer.unset_grab(state, serial, 0);
    pointer.motion(
        state,
        None,
        &MotionEvent {
            location,
            serial: SERIAL_COUNTER.next_serial(),
            time: 0,
        },
    );
    pointer.frame(state);
}

/// 点の下にある、ウィンドウの部分。
enum WindowHit {
    /// ウィンドウのポップアップ(画面と、その左上)。
    Popup(WlSurface, PointF),
    /// ウィンドウのコンテンツ(画面と、その左上)。
    Content(WlSurface, PointF),
    /// Seinas が描く飾り(タイトルバー)。クライアントには渡さない。
    Decoration,
}

/// ウィンドウの形。Seinas が描くタイトルバーの高さと、クライアントが決めたウィンドウの幾何。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    /// Seinas が描くタイトルバーの高さ([`bar_height`]。飾りを描かないウィンドウは 0)。
    bar: i32,
    /// ウィンドウの幾何(xdg_surface.set_window_geometry)の、画面(surface)の中の左上。決めていなければ (0, 0)。
    /// クライアントが自分で描く影などは、この外にあり、ウィンドウの大きさに数えない。
    offset: Point,
    /// コンテンツの大きさ(ウィンドウの幾何の大きさ。決めていなければ、画面の大きさ)。
    content: Size,
}

/// 装飾の取り決めで、Seinas が飾り(タイトルバー・枠・角・影)を描くか。いまの状態(クライアントが応じた configure
/// の後の commit で決まるもの)の装飾のモードが server_side のとき。装飾のオブジェクトを作っていないウィンドウは、
/// 取り決めていないので描かない(manager を bind しただけでは、取り決めたことにしない)。
fn decorated(toplevel: &ToplevelSurface) -> bool {
    toplevel.current_state().decoration_mode == Some(DecorationMode::ServerSide)
}

/// 次の configure で伝える装飾のモードで、Seinas が飾りを描くことになるか。configure で伝える大きさを決めるのに使う。
fn decorated_pending(toplevel: &ToplevelSurface) -> bool {
    toplevel.with_pending_state(|state| state.decoration_mode) == Some(DecorationMode::ServerSide)
}

/// ウィンドウ `toplevel` の形。まだ絵を出していなければNone。
fn shape(toplevel: &ToplevelSurface) -> Option<Shape> {
    let size: SmithaySize<i32, Logical> =
        with_renderer_surface_state(toplevel.wl_surface(), |state| state.surface_size())??;
    let geometry = with_states(toplevel.wl_surface(), |states| {
        states
            .cached_state
            .get::<SurfaceCachedState>()
            .current()
            .geometry
    });
    let (offset, content) = match geometry {
        Some(rect) if rect.size.w > 0 && rect.size.h > 0 => (
            Point::new(rect.loc.x, rect.loc.y),
            Size::new(rect.size.w, rect.size.h),
        ),
        _ => (Point::new(0, 0), Size::new(size.w, size.h)),
    };
    Some(Shape {
        bar: bar_height(decorated(toplevel)),
        offset,
        content,
    })
}

/// 外枠の左上が `location`、形が `shape` のウィンドウの、画面(surface)の左上。ウィンドウの幾何の左上を、コンテンツの
/// 左上(タイトルバーの下)に合わせる。
fn surface_origin(location: Point, shape: Shape) -> Point {
    let content = content_location(location, shape.bar);
    Point::new(content.x - shape.offset.x, content.y - shape.offset.y)
}

/// ウィンドウのコンテンツ(クライアントが描く所。ウィンドウの幾何)の大きさ。まだ絵を出していなければNone。
fn content_size(toplevel: &ToplevelSurface) -> Option<Size> {
    shape(toplevel).map(|shape| shape.content)
}

/// クライアントが決めた最小と最大の大きさ。最大化・全画面のウィンドウは、サイズ変更できないのでNone。
fn window_limits(toplevel: &ToplevelSurface) -> Option<SizeLimits> {
    let states = toplevel.current_state().states;
    if states.contains(xdg_toplevel::State::Maximized)
        || states.contains(xdg_toplevel::State::Fullscreen)
    {
        return None;
    }
    let cached = with_states(toplevel.wl_surface(), |states| {
        *states.cached_state.get::<SurfaceCachedState>().current()
    });
    Some(SizeLimits {
        min: Size::new(cached.min_size.w, cached.min_size.h),
        max: Size::new(cached.max_size.w, cached.max_size.h),
    })
}

/// 大きさの決まったウィンドウ(クライアントの最小と最大が同じ)でなければ、最大化できる。
fn can_maximize(toplevel: &ToplevelSurface) -> bool {
    let cached = with_states(toplevel.wl_surface(), |states| {
        *states.cached_state.get::<SurfaceCachedState>().current()
    });
    let (min, max) = (cached.min_size, cached.max_size);
    !(max.w > 0 && max.h > 0 && max == min)
}

/// ウィンドウ(toplevel)の測り方。操作の決まり([`Interaction`])が使う。
pub struct Toplevels {
    /// 最小化のボタンを出すか(戻す手段のシェルの部品があるか)。
    pub minimizable: bool,
}

impl Measure<ToplevelSurface> for Toplevels {
    fn outer_size(&self, toplevel: &ToplevelSurface) -> Option<Size> {
        window_size(toplevel)
    }
    fn limits(&self, toplevel: &ToplevelSurface) -> Option<SizeLimits> {
        window_limits(toplevel)
    }
    fn buttons(&self, toplevel: &ToplevelSurface) -> TitleButtons {
        TitleButtons {
            minimize: self.minimizable,
            maximize: can_maximize(toplevel),
        }
    }
    fn bar_height(&self, toplevel: &ToplevelSurface) -> i32 {
        bar_height(decorated(toplevel))
    }
}

/// xdg_toplevel の辺の指定を、Seinas の型にする。
fn edges_from(edges: xdg_toplevel::ResizeEdge) -> Edges {
    let bits = u32::from(edges);
    Edges::new(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0)
}

/// タイトルバーを含めた、ウィンドウの外枠の大きさ。まだ絵を出していなければNone。
fn window_size(toplevel: &ToplevelSurface) -> Option<Size> {
    shape(toplevel).map(|shape| outer_size(shape.content, shape.bar))
}

// 土台(Smithay)の幾何の型と、ウィンドウ管理の決まりの幾何の型(`geometry`)の変換。ADR 0001 の決まりで、
// 変換はフロントエンドの中のここだけで行う。

fn point_from(point: SmithayPoint<f64, Logical>) -> PointF {
    PointF::new(point.x, point.y)
}

fn to_smithay_point(point: PointF) -> SmithayPoint<f64, Logical> {
    (point.x, point.y).into()
}

fn rect_from(rect: Rectangle<i32, Logical>) -> Rect {
    Rect::new(
        Point::new(rect.loc.x, rect.loc.y),
        Size::new(rect.size.w, rect.size.h),
    )
}

/// クライアントが付けたタイトルと app_id。付けていなければ空。
fn toplevel_names(toplevel: &ToplevelSurface) -> (String, String) {
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|data| data.lock().expect("the toplevel data"));
        match &data {
            Some(data) => (
                data.title.clone().unwrap_or_default(),
                data.app_id.clone().unwrap_or_default(),
            ),
            None => Default::default(),
        }
    })
}

/// タイトルバーに出す文字。クライアントが付けたタイトル、無ければapp_id、それも無ければ空。
fn toplevel_title(toplevel: &ToplevelSurface) -> String {
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|data| data.lock().expect("the toplevel data"));
        match &data {
            Some(data) => title_text(data.title.as_deref(), data.app_id.as_deref()).to_owned(),
            None => String::new(),
        }
    })
}

/// 切られたクライアントのクリーンアップを、いますぐ行わせる。
///
/// 描いている最中に切られたクライアント(共有メモリーを縮めていた場合など)は、そのままだと、ほかの
/// クライアントから次の要求が届くまで、ソケットもウィンドウも残り続ける。描けない要素があったときに、
/// これを呼んで片付ける。イベントループの処理の外(クライアントの要求を処理していないとき)で呼ぶこと。
pub fn reap_dead_clients<D: FrontendHost + 'static>(display: &mut Display<D>, state: &mut D) {
    // クリーンアップは、どれか1つのクライアントの要求を処理させると、あわせて行われる。緊急解除で切った IME があれば、
    // それを選ぶ(ウィンドウが 1 つも無くても、すぐ片付くように)。
    let client = state.frontend_mut().ime.to_reap.take().or_else(|| {
        state
            .frontend()
            .windows
            .iter()
            .find_map(|window| window.item.wl_surface().client())
    });
    if let Some(client) = client {
        // 切られたクライアントを選んだ場合は失敗が返るが、クリーンアップは行われる。
        let _ = display.backend().dispatch_single_client(state, client.id());
    }
}

/// クライアントがつないできたソケット(役割)。見せる global を絞るのに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientRole {
    /// ふだんのソケット(アプリ)。
    #[default]
    App,
    /// シェルの部品のためのソケット。layer-shell と foreign-toplevel を見せる(ADR 0002)。
    Shell,
    /// IME のためのソケット。input-method と virtual-keyboard を見せる(ADR 0004 の 1.6)。
    Ime,
}

/// クライアントごとの状態。
#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// つないできたソケット。
    pub role: ClientRole,
}

/// `client` の役割(つないできたソケット)。
pub fn client_role(client: &Client) -> ClientRole {
    client
        .get_data::<ClientState>()
        .map_or(ClientRole::App, |state| state.role)
}

/// `client` がシェルの部品か。layer-shell などの global を見せる相手を絞るのに使う([`delegate_frontend!`])。
pub fn is_shell_client(client: &Client) -> bool {
    client_role(client) == ClientRole::Shell
}

/// `client` が IME のためのソケットからつないだか。input-method と virtual-keyboard の global を見せる相手を
/// 絞るのに使う。
pub fn is_ime_client(client: &Client) -> bool {
    client_role(client) == ClientRole::Ime
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        info!("a client connected");
    }
    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        info!("a client disconnected: {reason:?}");
    }
}

/// クライアントごとのコンポジタの状態を取り出す。[`delegate_frontend!`] が使う。
pub fn client_compositor_state(client: &Client) -> &CompositorClientState {
    // 受け付けるときに必ずClientStateを付けている。
    &client
        .get_data::<ClientState>()
        .expect("every client carries a ClientState")
        .compositor_state
}

/// 状態の型 `$ty` に、Smithayのハンドラと、フロントエンドの作り方([`BuildFrontend`])を実装する。
///
/// `$ty` は [`FrontendHost`] を実装していること。
#[macro_export]
macro_rules! delegate_frontend {
    ($ty:ty) => {
        impl $crate::smithay::wayland::buffer::BufferHandler for $ty {
            fn buffer_destroyed(
                &mut self,
                _buffer: &$crate::smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
            ) {
            }
        }

        impl $crate::smithay::wayland::compositor::CompositorHandler for $ty {
            fn compositor_state(&mut self) -> &mut $crate::smithay::wayland::compositor::CompositorState {
                &mut $crate::FrontendHost::frontend_mut(self).compositor_state
            }
            fn client_compositor_state<'a>(
                &self,
                client: &'a $crate::smithay::reexports::wayland_server::Client,
            ) -> &'a $crate::smithay::wayland::compositor::CompositorClientState {
                $crate::client_compositor_state(client)
            }
            fn commit(
                &mut self,
                surface: &$crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
            ) {
                $crate::smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
                let frontend = $crate::FrontendHost::frontend_mut(self);
                if frontend.commit_popup(surface) {
                    // ポップアップの commit。
                } else if frontend.commit_layer(surface) {
                    // keyboard_interactivity の変更は、commit で効く。
                    $crate::sync_keyboard_focus(self);
                } else {
                    frontend.commit_window(surface);
                }
                $crate::changed(self);
            }
        }

        impl $crate::smithay::wayland::shm::ShmHandler for $ty {
            fn shm_state(&self) -> &$crate::smithay::wayland::shm::ShmState {
                &$crate::FrontendHost::frontend(self).shm_state
            }
        }

        impl $crate::smithay::wayland::shell::xdg::XdgShellHandler for $ty {
            fn xdg_shell_state(&mut self) -> &mut $crate::smithay::wayland::shell::xdg::XdgShellState {
                &mut $crate::FrontendHost::frontend_mut(self).xdg_shell_state
            }
            fn client_pong(&mut self, client: $crate::smithay::wayland::shell::xdg::ShellClient) {
                $crate::client_pong(self, client);
            }
            fn new_toplevel(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                let frontend = $crate::FrontendHost::frontend_mut(self);
                frontend.output.enter(surface.wl_surface());
                // スタックに加えると、このウィンドウが activated なものになる。その状態も、最初の
                // configureで一緒に伝える。最初の configure は、最初の commit のときに送る(装飾の取り決めは、
                // ふつう、その前に届く。`Frontend::commit_window`)。
                frontend.add_window(surface.clone());
                $crate::log_new_toplevel();
                // 前に手前だったウィンドウのタイトルバーの色が変わる。
                $crate::changed(self);
            }
            fn move_request(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface,
                _seat: $crate::smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
                serial: $crate::smithay::utils::Serial,
            ) {
                $crate::client_move_request(self, &surface, serial);
            }
            fn resize_request(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface,
                _seat: $crate::smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
                serial: $crate::smithay::utils::Serial,
                edges: $crate::smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge,
            ) {
                $crate::client_resize_request(self, &surface, serial, edges);
            }
            fn maximize_request(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::toplevel_request(self, &surface, $crate::Command::ToggleMaximize, Some(true));
            }
            fn unmaximize_request(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::toplevel_request(self, &surface, $crate::Command::ToggleMaximize, Some(false));
            }
            fn minimize_request(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::toplevel_request(self, &surface, $crate::Command::Minimize, None);
            }
            fn title_changed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend_mut(self).update_foreign_toplevels();
                $crate::changed(self);
            }
            fn app_id_changed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend_mut(self).update_foreign_toplevels();
                $crate::changed(self);
            }
            fn toplevel_destroyed(&mut self, surface: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend_mut(self).remove_window(&surface);
                $crate::changed(self);
            }
            fn new_popup(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                positioner: $crate::smithay::wayland::shell::xdg::PositionerState,
            ) {
                $crate::FrontendHost::frontend_mut(self).new_popup(surface, positioner);
            }
            fn grab(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                seat: $crate::smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
                serial: $crate::smithay::utils::Serial,
            ) {
                $crate::grab_popup(self, surface, &seat, serial);
            }
            fn reposition_request(
                &mut self,
                surface: $crate::smithay::wayland::shell::xdg::PopupSurface,
                positioner: $crate::smithay::wayland::shell::xdg::PositionerState,
                token: u32,
            ) {
                $crate::FrontendHost::frontend_mut(self).reposition_popup(surface, positioner, token);
                $crate::changed(self);
            }
            fn popup_destroyed(&mut self, _surface: $crate::smithay::wayland::shell::xdg::PopupSurface) {
                $crate::FrontendHost::frontend_mut(self).forget_dead_popups();
                $crate::sync_keyboard_focus(self);
                $crate::changed(self);
            }
        }

        impl $crate::smithay::wayland::shell::xdg::decoration::XdgDecorationHandler for $ty {
            fn new_decoration(&mut self, toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
            }
            fn request_mode(
                &mut self,
                toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface,
                _mode: $crate::smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
            ) {
                // クライアントが自分で描きたいと言っても、タイトルバーはSeinasが描く。
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
            }
            fn unset_mode(&mut self, toplevel: $crate::smithay::wayland::shell::xdg::ToplevelSurface) {
                $crate::FrontendHost::frontend(self).decorate_on_server_side(&toplevel);
            }
        }

        impl $crate::smithay::wayland::shell::wlr_layer::WlrLayerShellHandler for $ty {
            fn shell_state(&mut self) -> &mut $crate::smithay::wayland::shell::wlr_layer::WlrLayerShellState {
                &mut $crate::FrontendHost::frontend_mut(self).layer_shell_state
            }
            fn new_layer_surface(
                &mut self,
                surface: $crate::smithay::wayland::shell::wlr_layer::LayerSurface,
                _output: Option<$crate::smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
                _layer: $crate::smithay::wayland::shell::wlr_layer::Layer,
                namespace: String,
            ) {
                // 画面は 1 つなので、output の指定は見ない。
                $crate::FrontendHost::frontend_mut(self).map_layer(surface, namespace);
                $crate::changed(self);
            }
            fn layer_destroyed(&mut self, surface: $crate::smithay::wayland::shell::wlr_layer::LayerSurface) {
                $crate::FrontendHost::frontend_mut(self).unmap_layer(&surface);
                $crate::sync_keyboard_focus(self);
                $crate::changed(self);
            }
            fn new_popup(
                &mut self,
                _parent: $crate::smithay::wayland::shell::wlr_layer::LayerSurface,
                popup: $crate::smithay::wayland::shell::xdg::PopupSurface,
            ) {
                $crate::FrontendHost::frontend_mut(self).new_layer_popup(popup);
            }
        }

        impl $crate::smithay::input::SeatHandler for $ty {
            type KeyboardFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            type PointerFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            type TouchFocus = $crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
            fn seat_state(&mut self) -> &mut $crate::smithay::input::SeatState<Self> {
                &mut $crate::FrontendHost::frontend_mut(self).seat_state
            }
            fn focus_changed(
                &mut self,
                _seat: &$crate::smithay::input::Seat<Self>,
                _focused: Option<&$crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
            ) {
            }
            fn cursor_image(
                &mut self,
                _seat: &$crate::smithay::input::Seat<Self>,
                _image: $crate::smithay::input::pointer::CursorImageStatus,
            ) {
            }
        }

        impl $crate::smithay::wayland::output::OutputHandler for $ty {
            fn output_bound(
                &mut self,
                _output: $crate::smithay::output::Output,
                wl_output: $crate::smithay::reexports::wayland_server::protocol::wl_output::WlOutput,
            ) {
                $crate::FrontendHost::frontend(self)
                    .foreign_toplevels
                    .output_bound(&wl_output);
            }
        }

        impl $crate::smithay::wayland::foreign_toplevel_list::ForeignToplevelListHandler for $ty {
            fn foreign_toplevel_list_state(
                &mut self,
            ) -> &mut $crate::smithay::wayland::foreign_toplevel_list::ForeignToplevelListState {
                &mut $crate::FrontendHost::frontend_mut(self).foreign_toplevels.list
            }
        }

        impl $crate::smithay::wayland::input_method::InputMethodHandler for $ty {
            fn new_popup(&mut self, surface: $crate::smithay::wayland::input_method::PopupSurface) {
                $crate::popup_shown(self, surface);
            }
            fn dismiss_popup(&mut self, surface: $crate::smithay::wayland::input_method::PopupSurface) {
                $crate::popup_hidden(self, surface);
            }
            fn popup_repositioned(&mut self, _surface: $crate::smithay::wayland::input_method::PopupSurface) {
                $crate::FrontendHost::redraw_needed(self);
            }
            fn parent_geometry(
                &self,
                parent: &$crate::smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
            ) -> $crate::smithay::utils::Rectangle<i32, $crate::smithay::utils::Logical> {
                $crate::FrontendHost::frontend(self).parent_geometry(parent)
            }
        }

        impl $crate::BuildFrontend for $ty {
            fn build_frontend(
                display_handle: &$crate::smithay::reexports::wayland_server::DisplayHandle,
                config: $crate::Config,
            ) -> $crate::Frontend<Self> {
                use $crate::smithay::{
                    input::SeatState,
                    wayland::{
                        compositor::CompositorState,
                        shell::{
                            wlr_layer::WlrLayerShellState,
                            xdg::{decoration::XdgDecorationState, XdgShellState},
                        },
                        shm::ShmState,
                    },
                };

                let compositor_state = CompositorState::new::<Self>(display_handle);
                // できるのは最大化だけ(最小化は、戻す手段のシェルの部品がつないだときに加える。全画面はできない)。
                let xdg_shell_state = XdgShellState::new_with_capabilities::<Self>(
                    display_handle,
                    [$crate::smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities::Maximize],
                );
                let xdg_decoration_state = XdgDecorationState::new::<Self>(display_handle);
                // layer-shell は、シェルの部品にだけ見せる(ADR 0002)。
                let layer_shell_state =
                    WlrLayerShellState::new_with_filter::<Self, _>(display_handle, $crate::is_shell_client);
                let shm_state = ShmState::new::<Self>(display_handle, vec![]);
                let mut seat_state = SeatState::new();
                // seat は、ポインターかキーボードを扱う構成でだけ公開する。
                let mut seat = (config.pointer || config.keyboard.is_some())
                    .then(|| seat_state.new_wl_seat(display_handle, "seinas-seat"));
                let pointer = seat
                    .as_mut()
                    .filter(|_| config.pointer)
                    .map(|seat| seat.add_pointer());
                let keyboard = match (seat.as_mut(), &config.keyboard) {
                    (Some(seat), Some(keyboard)) => $crate::add_keyboard(seat, keyboard),
                    _ => None,
                };
                let output = $crate::new_output(config.width, config.height);
                output.create_global::<Self>(display_handle);
                let output_manager =
                    $crate::smithay::wayland::output::OutputManagerState::new_with_xdg_output::<Self>(
                        display_handle,
                    );
                // ウィンドウの一覧と操作も、シェルの部品にだけ見せる(ADR 0002)。
                let foreign_toplevels = $crate::ForeignToplevelState::new::<Self>(display_handle);
                // IME の道。input-method と virtual-keyboard は、キーボードがあり、IME のためのソケットを開くときだけ
                // (ADR 0004)。
                let ime = $crate::ImeState::new::<Self>(
                    display_handle,
                    keyboard.is_some() && config.ime_socket,
                );
                $crate::Frontend {
                    display_handle: display_handle.clone(),
                    compositor_state,
                    xdg_shell_state,
                    xdg_decoration_state,
                    layer_shell_state,
                    foreign_toplevels,
                    ime,
                    shm_state,
                    seat_state,
                    seat,
                    pointer,
                    keyboard,
                    shortcuts: $crate::shortcuts_for(&config),
                    window_drag: config.switch_keys.window_drag,
                    switcher: None,
                    switch_hold: Default::default(),
                    switcher_image: None,
                    log_keys: config.log_keys,
                    output,
                    output_manager,
                    size: $crate::geometry::Size::new(config.width, config.height),
                    usable: $crate::geometry::Rect::from_size($crate::geometry::Size::new(
                        config.width,
                        config.height,
                    )),
                    windows: $crate::Stack::default(),
                    text: $crate::load_fonts(&config.font_dir, config.font_config.as_deref()),
                    title_bars: Default::default(),
                    icons: $crate::IconCache::from_env($crate::TITLE_ICON_SIZE as u32),
                    frame_corners: (
                        $crate::BorderCorners::new($crate::ACTIVE_BORDER),
                        $crate::BorderCorners::new($crate::INACTIVE_BORDER),
                    ),
                    shadows: Default::default(),
                    popups: Default::default(),
                    popup_grab: None,
                    layer_focus: None,
                    input_serials: Vec::new(),
                    swallowed_button: None,
                    opened: Default::default(),
                    interaction: Default::default(),
                    pings: $crate::Pings::new(config.ping_timeout),
                    reap_requested: false,
                    maximize_marks: Default::default(),
                    resize_anchors: Default::default(),
                }
            }
        }

        $crate::smithay::delegate_compositor!($ty);
        $crate::smithay::delegate_shm!($ty);
        $crate::smithay::delegate_xdg_shell!($ty);
        $crate::smithay::delegate_xdg_decoration!($ty);
        $crate::smithay::delegate_layer_shell!($ty);
        $crate::smithay::delegate_foreign_toplevel_list!($ty);
        $crate::smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1: ()
        ] => $crate::ForeignToplevelState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1: ()
        ] => $crate::ForeignToplevelState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1: $crate::HandleData
        ] => $crate::ForeignToplevelState);
        $crate::smithay::delegate_seat!($ty);
        $crate::smithay::delegate_output!($ty);
        $crate::smithay::delegate_text_input_manager!($ty);
        // input-method と virtual-keyboard は、IME を 1 つに限り、IME の要求を見るために、Smithay の手前で
        // ImeState が受ける(`ime.rs`)。
        $crate::smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_manager_v2::ZwpInputMethodManagerV2:
            $crate::smithay::wayland::input_method::InputMethodManagerGlobalData
        ] => $crate::smithay::wayland::input_method::InputMethodManagerState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_manager_v2::ZwpInputMethodManagerV2: ()
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_v2::ZwpInputMethodV2:
            $crate::smithay::wayland::input_method::InputMethodUserData<Self>
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_v2::ZwpInputMethodV2: $crate::RefusedInputMethod
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2:
            $crate::smithay::wayland::input_method::InputMethodKeyboardUserData<Self>
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2: $crate::RefusedInputMethod
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2:
            $crate::smithay::wayland::input_method::InputMethodPopupSurfaceUserData
        ] => $crate::smithay::wayland::input_method::InputMethodManagerState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2: $crate::RefusedInputMethod
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1:
            $crate::smithay::wayland::virtual_keyboard::VirtualKeyboardManagerGlobalData
        ] => $crate::smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1: ()
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1: $crate::RefusedVirtualKeyboard
        ] => $crate::ImeState);
        $crate::smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            $crate::smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1:
            $crate::smithay::wayland::virtual_keyboard::VirtualKeyboardUserData<Self>
        ] => $crate::ImeState);
    };
}

/// seat `seat` に、`config` の keymap でキーボードを足す。[`delegate_frontend!`] が使う。
///
/// keymap を作れなければ(xkb のファイルが無いなど)、ログに出してNoneを返す(キーボードの無い構成として動く)。
/// xkbcommon の context そのものを作れないとき(既定の include のパスを 1 つも足せないとき)も同じ。Smithay 0.7.0
/// は、そのとき NULL の context で keymap を作ろうとして落ちる(xkbcommon 0.8.0 の `Context::new` が NULL を
/// 確かめない)ので、Smithay に渡す前に確かめる。
pub fn add_keyboard<D: SeatHandler + 'static>(
    seat: &mut Seat<D>,
    config: &KeyboardConfig,
) -> Option<KeyboardHandle<D>> {
    if !xkb_context_can_be_made() {
        warn!(
            "no keyboard: the keymap could not be made: the xkb context could not be made \
             (no xkb include path; see XKB_CONFIG_ROOT)"
        );
        return None;
    }
    let xkb = XkbConfig {
        layout: &config.layout,
        model: &config.model,
        ..XkbConfig::default()
    };
    match seat.add_keyboard(xkb, config.repeat_delay, config.repeat_rate) {
        Ok(keyboard) => {
            info!(
                "keyboard: layout {:?}, model {:?}",
                config.layout, config.model
            );
            Some(keyboard)
        }
        Err(e) => {
            warn!("no keyboard: the keymap could not be made: {e:?}");
            None
        }
    }
}

/// xkbcommon の context を、Smithay 0.7.0 と同じ flags(`CONTEXT_NO_FLAGS`)で作れるか。
fn xkb_context_can_be_made() -> bool {
    use smithay::input::keyboard::xkb::{ffi, CONTEXT_NO_FLAGS};
    // SAFETY: xkb_context_new は、引数の flags(ただの数)だけを読み、作れなければ NULL を返す。返った
    // ポインターは、この関数の中だけで持ち、ほかへ渡さない。NULL でなければ、作ったときに持った参照 1 つを、
    // xkb_context_unref で 1 度だけ放す(その後は使わない)。
    unsafe {
        let context = ffi::xkb_context_new(CONTEXT_NO_FLAGS);
        if context.is_null() {
            return false;
        }
        ffi::xkb_context_unref(context);
    }
    true
}

/// 画面(wl_output)を、`width` × `height` ピクセルの1つの表示モードで作る。[`delegate_frontend!`] が使う。
pub fn new_output(width: i32, height: i32) -> Output {
    use smithay::{
        output::{Mode, PhysicalProperties, Scale, Subpixel},
        utils::Transform,
    };
    let output = Output::new(
        "seinas-0".to_owned(),
        PhysicalProperties {
            // 実際の大きさ(ミリメートル)は分からないので、0にしておく。
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Seinas".to_owned(),
            model: "screen".to_owned(),
        },
    );
    let mode = Mode {
        size: (width, height).into(),
        // 垂直同期の周期は分からないので、60Hzと名乗る(単位はmHz)。
        refresh: 60_000,
    };
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output
}

/// [`delegate_frontend!`] が使う。
pub fn log_new_toplevel() {
    info!("new toplevel");
}
