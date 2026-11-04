//! トーチのメニュー(スタートメニュー)の状態と、ポップアップの出し入れ。
//!
//! アプリの一覧は `seinas_taskbar::apps`(`.desktop` を限って読む)、絵は `seinas_taskbar::paint_menu`、起こすのは
//! `seinas_taskbar::launch`(Linux と ZeikOS で差し替える)にある。ここは、それらをつなぎ、隠れたタスクの一覧と同じく、
//! layer surface のポップアップ(トーチを押した serial で grab)として出す。決まりの説明は `docs/taskbar.md` の
//! 「トーチのメニュー」にある。

use std::{collections::HashMap, path::PathBuf};

use seinas_icons::{find_icon_file, load_icon, Icon};
use seinas_taskbar::{
    apps::{read_apps, App, Locale},
    layout, menu_height, menu_rows_height, paint_menu, ListKey, HEIGHT, ICON_SIZE, MENU_GAP,
    MENU_ROWS, MENU_ROW_HEIGHT, MENU_WIDTH,
};
use smithay_client_toolkit::{
    reexports::protocols::xdg::shell::client::xdg_positioner,
    shell::{
        wlr_layer::KeyboardInteractivity,
        xdg::{popup::Popup, XdgPositioner},
    },
    shm::slot::Buffer,
};
use wayland_client::protocol::wl_shm;

use crate::{
    FrameCallbackData, Taskbar, KEY_DOWN, KEY_END, KEY_ENTER, KEY_ESC, KEY_HOME, KEY_KP_ENTER,
    KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP,
};

/// アプリが 1 つも無いときに出す行。
const NO_APPS: &str = "起動できるアプリがありません";

/// 開いているトーチのメニュー。
pub struct Menu {
    pub popup: Popup,
    /// コンポジタが決めた高さ。最初の configure までは 0。
    pub height: i32,
    pub scroll: i32,
    /// 選んでいる行(開いている間、アプリの一覧は変えないので、一覧の中の位置で持つ)。
    pub selected: Option<usize>,
    pub apps: Vec<App>,
    /// 各々のアプリのアイコン(`apps` と同じ順)。
    pub icons: Vec<Icon>,
    /// 行の下に出す 1 行(起動できなかった理由、アプリが無いこと)。
    pub message: Option<String>,
    pub buffer: Option<Buffer>,
    pub frame_pending: bool,
    pub dirty: bool,
}

impl Menu {
    /// 行の所の高さ(理由の行を除く)。
    fn rows_height(&self) -> i32 {
        menu_rows_height(self.height, self.message.is_some())
    }
}

impl Taskbar {
    /// メニューを置く positioner。トーチの上に、空き [`MENU_GAP`] を置いて出す。横は画面に収まるようずらし、縦は
    /// 画面に収まらなければ低くする(行の所がスクロールする)。
    fn menu_positioner(&self, rows: usize, message: bool) -> Option<XdgPositioner> {
        let positioner = match XdgPositioner::new(&self.xdg_shell) {
            Ok(positioner) => positioner,
            Err(e) => {
                eprintln!("seinas-taskbar: cannot place the torch menu: {e}");
                return None;
            }
        };
        let layout = layout(self.width, self.shown().len());
        let (x, _, w, _) = layout.highlight_rect(0);
        positioner.set_size(MENU_WIDTH, menu_height(rows, message));
        positioner.set_anchor_rect(x, 0, w, HEIGHT);
        positioner.set_anchor(xdg_positioner::Anchor::Top);
        positioner.set_gravity(xdg_positioner::Gravity::Top);
        positioner.set_offset(0, -MENU_GAP);
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::ResizeY,
        );
        Some(positioner)
    }

    /// アプリの一覧を読み直し、メニューを開く。`serial` はトーチを押したときのもの(ポップアップの grab に使う)。
    pub(crate) fn open_menu(&mut self, serial: u32) {
        self.close_list();
        let dirs: Vec<PathBuf> = seinas_icons::data_dirs_from_env();
        let path = std::env::var_os("PATH");
        let (apps, notes) = read_apps(&dirs, Locale::from_env().as_ref(), path.as_deref());
        for note in notes {
            // 同じ理由は、1 回だけ出す。
            if self.menu_notes.insert(note.clone()) {
                eprintln!("seinas-taskbar: {note}");
            }
        }
        let icons = apps
            .iter()
            .map(|app| self.app_icon(app.icon.as_deref(), &dirs))
            .collect();
        let message = apps.is_empty().then(|| NO_APPS.to_owned());
        let Some(positioner) = self.menu_positioner(apps.len(), message.is_some()) else {
            return;
        };
        let surface = self.compositor.create_surface(&self.qh);
        let popup = match Popup::from_surface(None, &positioner, &self.qh, surface, &self.xdg_shell)
        {
            Ok(popup) => popup,
            Err(e) => {
                eprintln!("seinas-taskbar: cannot open the torch menu: {e}");
                return;
            }
        };
        self.layer.get_popup(popup.xdg_popup());
        // 矢印キー・Enter・Esc を受けるため、開いている間は、押したときにキーボードのフォーカスを受ける形にする。
        self.layer
            .set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        match self.seat_state.seats().next() {
            Some(seat) => popup.xdg_popup().grab(&seat, serial),
            None => eprintln!("seinas-taskbar: no seat to open the torch menu with"),
        }
        popup.wl_surface().commit();
        eprintln!(
            "seinas-taskbar: opened the torch menu of {} applications",
            apps.len()
        );
        self.menu = Some(Menu {
            popup,
            height: 0,
            scroll: 0,
            selected: (!apps.is_empty()).then_some(0),
            apps,
            icons,
            message,
            buffer: None,
            frame_pending: false,
            dirty: true,
        });
        self.redraw();
    }

    /// アイコンの名前(か絶対パス)`name` のアイコン。読んだものは覚えて使い回す。無ければ既定のアイコン。
    fn app_icon(&mut self, name: Option<&str>, dirs: &[PathBuf]) -> Icon {
        let Some(name) = name else {
            return self.icons.default_icon().clone();
        };
        if let Some(icon) = self.app_icons.get(name) {
            return icon.clone();
        }
        let icon = find_icon_file(name, dirs, ICON_SIZE as u32)
            .and_then(|path| match load_icon(&path, ICON_SIZE as u32) {
                Ok(icon) => Some(icon),
                Err(reason) => {
                    eprintln!(
                        "seinas-taskbar: the icon {} is not used: {reason}",
                        path.display()
                    );
                    None
                }
            })
            .unwrap_or_else(|| self.icons.default_icon().clone());
        self.app_icons.insert(name.to_owned(), icon.clone());
        icon
    }

    /// メニューを閉じる(ポップアップを消す)。
    pub(crate) fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.layer
                .set_keyboard_interactivity(KeyboardInteractivity::None);
            eprintln!("seinas-taskbar: closed the torch menu");
            self.redraw();
        }
    }

    /// メニューの絵を描いて出す。最初の configure の前と、frame callback を待っている間は描かない。
    pub(crate) fn draw_menu(&mut self) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        if menu.height <= 0 || menu.frame_pending {
            return;
        }
        let rows_height = menu.rows_height();
        menu.scroll = menu
            .scroll
            .clamp(0, MENU_ROWS.max_scroll(menu.apps.len(), rows_height));
        let items: Vec<(&str, &Icon)> = menu
            .apps
            .iter()
            .zip(&menu.icons)
            .map(|(app, icon)| (app.name.as_str(), icon))
            .collect();
        let pixels = paint_menu(
            &mut self.text,
            menu.height,
            &items,
            menu.selected,
            menu.scroll,
            menu.message.as_deref(),
        );
        let (width, stride) = (MENU_WIDTH, MENU_WIDTH * 4);
        let buffer = match menu.buffer.take() {
            Some(buffer)
                if buffer.height() == menu.height
                    && buffer.stride() == stride
                    && self.pool.canvas(&buffer).is_some() =>
            {
                buffer
            }
            _ => match self
                .pool
                .create_buffer(width, menu.height, stride, wl_shm::Format::Xrgb8888)
            {
                Ok((buffer, _)) => buffer,
                Err(e) => {
                    eprintln!("seinas-taskbar: cannot create a buffer for the torch menu: {e}");
                    return;
                }
            },
        };
        let Some(canvas) = self.pool.canvas(&buffer) else {
            return;
        };
        canvas.copy_from_slice(&pixels);
        let surface = menu.popup.wl_surface().clone();
        surface.damage_buffer(0, 0, width, menu.height);
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        if let Err(e) = buffer.attach_to(&surface) {
            eprintln!("seinas-taskbar: cannot attach the buffer of the torch menu: {e}");
            return;
        }
        surface.commit();
        menu.buffer = Some(buffer);
        menu.frame_pending = true;
        menu.dirty = false;
    }

    /// 行 `row` を選び、見えるようにスクロールする。
    pub(crate) fn select_menu_row(&mut self, row: Option<usize>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        if row == menu.selected {
            return;
        }
        menu.selected = row;
        if let Some(row) = row {
            let rows_height = menu.rows_height();
            menu.scroll = MENU_ROWS.scroll_to(row, menu.apps.len(), rows_height, menu.scroll);
        }
        menu.dirty = true;
        self.draw_menu();
    }

    /// メニューの中の高さ `y` にある行。
    pub(crate) fn menu_row_at(&self, y: f64) -> Option<usize> {
        let menu = self.menu.as_ref()?;
        MENU_ROWS.row_at(y, menu.apps.len(), menu.rows_height(), menu.scroll)
    }

    /// メニューを `step` ピクセルだけスクロールする(ホイール)。
    pub(crate) fn scroll_menu(&mut self, step: i32) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let rows_height = menu.rows_height();
        menu.scroll =
            (menu.scroll + step).clamp(0, MENU_ROWS.max_scroll(menu.apps.len(), rows_height));
        menu.dirty = true;
        self.draw_menu();
    }

    /// 行 `row` のアプリを起こす。起こす頼みが受け付けられたら閉じる。その場で失敗したら、閉じずに理由を出す。
    pub(crate) fn start_app(&mut self, row: usize) {
        let Some(app) = self
            .menu
            .as_ref()
            .and_then(|menu| menu.apps.get(row).cloned())
        else {
            return;
        };
        match self.launcher.launch(&app) {
            Ok(pid) => {
                eprintln!(
                    "seinas-taskbar: started {} ({}) as pid {pid}",
                    app.name, app.id
                );
                self.close_menu();
            }
            Err(reason) => {
                eprintln!(
                    "seinas-taskbar: cannot start {} ({}): {reason}",
                    app.name, app.id
                );
                let rows = self.menu.as_ref().map_or(0, |menu| menu.apps.len());
                let grow = self
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.message.is_none());
                if grow {
                    // 理由の行の分だけ高くする(置き直しを頼む)。
                    if let Some(positioner) = self.menu_positioner(rows, true) {
                        if let Some(menu) = self.menu.as_ref() {
                            menu.popup.reposition(&positioner, 0);
                        }
                    }
                }
                if let Some(menu) = self.menu.as_mut() {
                    menu.message = Some(format!("起動できませんでした: {reason}"));
                    menu.dirty = true;
                }
                self.draw_menu();
            }
        }
    }

    /// メニューを開いている間のキー(Linux のキーの番号)。
    pub(crate) fn menu_key(&mut self, key: u32) {
        let Some(menu) = self.menu.as_ref() else {
            return;
        };
        let movement = match key {
            KEY_UP => Some(ListKey::Up),
            KEY_DOWN => Some(ListKey::Down),
            KEY_HOME => Some(ListKey::Home),
            KEY_END => Some(ListKey::End),
            KEY_PAGE_UP => Some(ListKey::PageUp),
            KEY_PAGE_DOWN => Some(ListKey::PageDown),
            _ => None,
        };
        if let Some(movement) = movement {
            let row = MENU_ROWS.move_selection(
                menu.selected,
                menu.apps.len(),
                menu.rows_height(),
                movement,
            );
            self.select_menu_row(row);
            return;
        }
        match key {
            KEY_ENTER | KEY_KP_ENTER => {
                if let Some(row) = menu.selected {
                    self.start_app(row);
                }
            }
            KEY_ESC => self.close_menu(),
            _ => {}
        }
    }

    /// メニューのポップアップに configure が届いた。
    pub(crate) fn configure_menu(&mut self, height: i32) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        if height != menu.height {
            menu.height = height;
            menu.buffer = None;
        }
        if let Some(row) = menu.selected {
            let rows_height = menu.rows_height();
            menu.scroll = MENU_ROWS.scroll_to(row, menu.apps.len(), rows_height, menu.scroll);
        }
        menu.dirty = true;
        self.draw_menu();
    }
}

/// メニューの 1 行の高さ(ホイールの 1 段で動かす量)。
pub const WHEEL_STEP: i32 = MENU_ROW_HEIGHT;

/// 覚えるアイコンの表(アイコンの名前 → 絵)。
pub type IconTable = HashMap<String, Icon>;
