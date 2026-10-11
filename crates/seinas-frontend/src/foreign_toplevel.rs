//! シェルの部品(タスクバーなど)へ、ウィンドウの一覧と操作のプロトコルを見せる(ADR 0002)。
//!
//! - `zwlr_foreign_toplevel_manager_v1`(wlr-foreign-toplevel-management、バージョン 3):一覧(title、
//!   app_id、state、output)と、操作の頼み(activate、close など)。Smithay に handler が無いので、ここで
//!   Dispatch を書く。
//! - `ext_foreign_toplevel_list_v1`:読むだけの一覧。Smithay の実装を使い、ここでは内容を合わせるだけ。
//!
//! どちらの global も、シェルのソケットからつないだクライアントにだけ見せる。一覧に載るのは、絵を出した
//! (map した)ウィンドウだけで、順は map した順。

use std::sync::Mutex;

use smithay::{
    output::Output,
    reexports::{
        wayland_protocols_wlr::foreign_toplevel::v1::server::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
        wayland_server::{
            backend::ClientId,
            protocol::{wl_output::WlOutput, wl_surface::WlSurface},
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
        },
    },
    wayland::foreign_toplevel_list::{
        ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
    },
};
use tracing::info;

use crate::{
    geometry::{Point, Rect, Size},
    FrontendHost, WindowId,
};

/// 公開する wlr-foreign-toplevel-management のバージョン。
const WLR_VERSION: u32 = 3;

/// 一覧に載せる、1 つのウィンドウの見え方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub window: WindowId,
    /// クライアントが付けたタイトルと app_id。付けていなければ空。
    pub title: String,
    pub app_id: String,
    /// いちばん手前の(activated の)ウィンドウか。
    pub activated: bool,
    pub maximized: bool,
    pub minimized: bool,
}

/// wlr の handle(`zwlr_foreign_toplevel_handle_v1`)ごとのデータ。
#[derive(Debug)]
pub struct HandleData {
    window: WindowId,
    /// シェルの部品が、このウィンドウを表している場所(set_rectangle)。最小化のアニメーションの行き先に
    /// 使えるが、今は覚えるだけ。
    rectangle: Mutex<Option<(WlSurface, Rect)>>,
}

/// 一覧に載っている 1 つのウィンドウ。
struct Listed {
    shown: Listing,
    /// シェルの部品(manager を bind したクライアント)ごとの handle。
    handles: Vec<ZwlrForeignToplevelHandleV1>,
    /// ext の一覧での handle。
    ext: ForeignToplevelHandle,
}

/// 一覧と操作のプロトコルの状態。
pub struct ForeignToplevelState {
    /// bind されている wlr の manager。
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    /// 一覧に載っているウィンドウ(map した順)。
    listed: Vec<Listed>,
    /// ext の一覧(Smithay)。
    pub list: ForeignToplevelListState,
}

impl ForeignToplevelState {
    /// 2 つの global を作る。どちらも、シェルの部品にだけ見せる。
    pub fn new<D>(display_handle: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> + ForeignToplevelListHandler,
    {
        display_handle.create_global::<D, ZwlrForeignToplevelManagerV1, ()>(WLR_VERSION, ());
        ForeignToplevelState {
            managers: Vec::new(),
            listed: Vec::new(),
            list: ForeignToplevelListState::new_with_filter::<D>(
                display_handle,
                crate::is_shell_client,
            ),
        }
    }

    /// manager を bind しているシェルの部品があるか(最小化したウィンドウを戻す手段があるか)。
    pub fn has_managers(&self) -> bool {
        !self.managers.is_empty()
    }

    /// `window` が一覧に載っているか。
    pub fn is_listed(&self, window: WindowId) -> bool {
        self.listed
            .iter()
            .any(|listed| listed.shown.window == window)
    }

    /// 一覧を `now`(いま一覧に載せるウィンドウ)に合わせ、変わったことをシェルの部品へ送る。
    ///
    /// `now` に無くなったウィンドウには closed を送り、新しいウィンドウは一覧の終わりに加えて送り、
    /// タイトル・app_id・状態が変わったウィンドウには、変わったものと done を送る。
    pub fn update<D: FrontendHost>(
        &mut self,
        display_handle: &DisplayHandle,
        output: &Output,
        now: Vec<Listing>,
    ) {
        let before = self.listed.len();
        self.listed.retain(|listed| {
            let stays = now.iter().any(|l| l.window == listed.shown.window);
            if !stays {
                for handle in &listed.handles {
                    handle.closed();
                }
                // Smithay の remove_toplevel は使わない(一覧の中で死んだ弱い参照があると、別の項目を
                // 外してしまう)。閉じたものは、cleanup_closed_handles で外れる。
                listed.ext.send_closed();
            }
            stays
        });
        if self.listed.len() != before {
            self.list.cleanup_closed_handles();
        }
        for listing in now {
            match self
                .listed
                .iter_mut()
                .find(|listed| listed.shown.window == listing.window)
            {
                Some(listed) => listed.change(listing),
                None => {
                    let ext = self
                        .list
                        .new_toplevel::<D>(listing.title.clone(), listing.app_id.clone());
                    let mut listed = Listed {
                        shown: listing,
                        handles: Vec::new(),
                        ext,
                    };
                    for manager in &self.managers {
                        listed.announce::<D>(display_handle, manager, output);
                    }
                    self.listed.push(listed);
                }
            }
        }
    }

    /// クライアントが wl_output を bind した。そのクライアントの handle に、output_enter を送る。
    pub fn output_bound(&self, wl_output: &WlOutput) {
        for listed in &self.listed {
            for handle in &listed.handles {
                if handle.id().same_client_as(&wl_output.id()) {
                    handle.output_enter(wl_output);
                    handle.done();
                }
            }
        }
    }
}

impl Listed {
    /// manager を bind したクライアントに、このウィンドウの handle を作って、内容を全部送る。
    fn announce<D: FrontendHost>(
        &mut self,
        display_handle: &DisplayHandle,
        manager: &ZwlrForeignToplevelManagerV1,
        output: &Output,
    ) {
        let Some(client) = manager.client() else {
            return;
        };
        let data = HandleData {
            window: self.shown.window,
            rectangle: Mutex::new(None),
        };
        let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, _, D>(
            display_handle,
            manager.version(),
            data,
        ) else {
            return;
        };
        manager.toplevel(&handle);
        // タイトルと app_id は、付いているときだけ送る。
        if !self.shown.title.is_empty() {
            handle.title(self.shown.title.clone());
        }
        if !self.shown.app_id.is_empty() {
            handle.app_id(self.shown.app_id.clone());
        }
        // 画面は 1 つで、ウィンドウはいつもその上にある。まだ wl_output を bind していなければ、bind した
        // ときに送る(output_bound)。
        for wl_output in output.client_outputs(&client) {
            handle.output_enter(&wl_output);
        }
        handle.state(states(&self.shown));
        handle.done();
        self.handles.push(handle);
    }

    /// 見え方を `new` に変え、変わったものと done を送る。
    fn change(&mut self, new: Listing) {
        if self.shown == new {
            return;
        }
        let title = self.shown.title != new.title;
        let app_id = self.shown.app_id != new.app_id;
        let state = (
            self.shown.activated,
            self.shown.maximized,
            self.shown.minimized,
        ) != (new.activated, new.maximized, new.minimized);
        for handle in &self.handles {
            if title {
                handle.title(new.title.clone());
            }
            if app_id {
                handle.app_id(new.app_id.clone());
            }
            if state {
                handle.state(states(&new));
            }
            handle.done();
        }
        if title || app_id {
            self.ext.send_title(&new.title);
            self.ext.send_app_id(&new.app_id);
            self.ext.send_done();
        }
        self.shown = new;
    }
}

/// state の配列(u32 の配列)。全画面は、まだ持っていない。
fn states(listing: &Listing) -> Vec<u8> {
    use zwlr_foreign_toplevel_handle_v1::State;
    [
        (listing.maximized, State::Maximized),
        (listing.minimized, State::Minimized),
        (listing.activated, State::Activated),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .flat_map(|(_, state)| (state as u32).to_ne_bytes())
    .collect()
}

impl<D> GlobalDispatch<ZwlrForeignToplevelManagerV1, (), D> for ForeignToplevelState
where
    D: FrontendHost + Dispatch<ZwlrForeignToplevelManagerV1, ()>,
{
    fn bind(
        state: &mut D,
        display_handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        let manager = data_init.init(resource, ());
        let frontend = state.frontend_mut();
        let foreign = &mut frontend.foreign_toplevels;
        // 今あるウィンドウを、全部送る。
        for listed in &mut foreign.listed {
            listed.announce::<D>(display_handle, &manager, &frontend.output);
        }
        foreign.managers.push(manager);
        // 最小化のボタンを出せるようになったかもしれない。
        frontend.managers_changed();
        crate::changed(state);
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        crate::is_shell_client(&client)
    }
}

impl<D: FrontendHost> Dispatch<ZwlrForeignToplevelManagerV1, (), D> for ForeignToplevelState {
    fn request(
        state: &mut D,
        _client: &Client,
        manager: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _display_handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            // finished を送ると、manager は消える。この後は、新しいウィンドウを送らない。
            forget_manager(state, manager);
            manager.finished();
        }
    }

    fn destroyed(
        state: &mut D,
        _client: ClientId,
        manager: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        forget_manager(state, manager);
    }
}

fn forget_manager<D: FrontendHost>(state: &mut D, manager: &ZwlrForeignToplevelManagerV1) {
    let frontend = state.frontend_mut();
    let before = frontend.foreign_toplevels.managers.len();
    frontend.foreign_toplevels.managers.retain(|m| m != manager);
    if frontend.foreign_toplevels.managers.len() != before {
        // 最後のシェルの部品が去ったら、最小化のボタンを消し、最小化しているウィンドウを戻す。
        frontend.managers_changed();
        crate::changed(state);
    }
}

impl<D: FrontendHost> Dispatch<ZwlrForeignToplevelHandleV1, HandleData, D>
    for ForeignToplevelState
{
    fn request(
        state: &mut D,
        _client: &Client,
        handle: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        data: &HandleData,
        _display_handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Request;
        // 閉じたウィンドウの handle への頼みは、何もしない(window が見つからない)。
        match request {
            Request::Activate { seat: _ } => {
                // seat は 1 つだけなので、指定は見ない。
                if state.frontend_mut().activate(data.window) {
                    crate::changed(state);
                }
            }
            Request::Close => state.frontend_mut().request_close(data.window),
            Request::SetRectangle {
                surface,
                x,
                y,
                width,
                height,
            } => {
                if width < 0 || height < 0 {
                    handle.post_error(
                        zwlr_foreign_toplevel_handle_v1::Error::InvalidRectangle,
                        "the width and the height must not be negative",
                    );
                    return;
                }
                let mut rectangle = data.rectangle.lock().expect("the rectangle");
                // 幅と高さが 0 なら、前の場所を消す。
                *rectangle = (width > 0 || height > 0).then(|| {
                    (
                        surface,
                        Rect::new(Point::new(x, y), Size::new(width, height)),
                    )
                });
            }
            Request::SetMaximized | Request::UnsetMaximized => {
                let maximized = matches!(request, Request::SetMaximized);
                if state.frontend_mut().set_maximized(data.window, maximized) {
                    crate::changed(state);
                }
            }
            Request::SetMinimized => {
                if state.frontend_mut().minimize(data.window) {
                    crate::changed(state);
                }
            }
            // 戻すと、raise して activated にする(Windows のタスクバーと同じ)。
            Request::UnsetMinimized => {
                if state.frontend_mut().unminimize(data.window) {
                    crate::changed(state);
                }
            }
            // 全画面は、まだ持っていない。頼みは受けるが、今は何もしない(状態も変えない)。
            Request::SetFullscreen { .. } | Request::UnsetFullscreen => {
                info!("foreign toplevel: {request:?} is not supported yet; ignored");
            }
            Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut D,
        _client: ClientId,
        handle: &ZwlrForeignToplevelHandleV1,
        data: &HandleData,
    ) {
        let foreign = &mut state.frontend_mut().foreign_toplevels;
        if let Some(listed) = foreign
            .listed
            .iter_mut()
            .find(|listed| listed.shown.window == data.window)
        {
            listed.handles.retain(|h| h != handle);
        }
    }
}
