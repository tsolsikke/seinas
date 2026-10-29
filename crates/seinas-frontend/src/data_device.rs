//! クリップボード(wl_data_device の selection)と、クライアントどうしのドラッグ&ドロップ。
//!
//! 仕組みは Smithay の `DataDeviceState` に任せる(selection の受け渡し、data offer、ドラッグ&ドロップの grab)。
//! Seinas が行うのは、次のこと。
//! - selection の相手:キーボードのフォーカスが移るたびに、`set_data_device_focus` で、フォーカスのあるクライアントに
//!   今の selection を見せる。フォーカスの無いクライアントは、selection を受け取れず、置くこともできない(Smithay が断る)。
//! - ドラッグ&ドロップの始まり:start_drag の serial は、Smithay が、押しているボタンの暗黙の grab(クライアントへ渡した
//!   直前の押下)のものかを確かめる。
//! - ドラッグ&ドロップの最中:ポインターのイベントは、Seinas の操作(タイトルバー、修飾キーとドラッグ)にも、ポップアップの
//!   grab(ほかのクライアントの面に渡さない決まり)にも使わず、Smithay の grab へ渡す(`pointer_input`)。ポップアップの
//!   連なりは閉じない(Smithay は、始まりの押下の面が消えると grab を外すので、ポップアップから始めたドラッグが切れる)。
//!   アイコン(dnd_icon の役割の面)は、ポインターの位置に、いちばん手前に描く。
//!
//! primary selection(中ボタンの貼り付け)は、まだ扱わない。決まりの説明は `docs/window-placement.md` の
//! 「クリップボードとドラッグ&ドロップ」にある。

use smithay::{
    input::SeatHandler,
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::IsAlive,
    wayland::compositor::{with_states, SurfaceAttributes},
};
use tracing::info;

use crate::{geometry::Point, Frontend};

/// ドラッグ&ドロップのアイコンの面と、ポインターの位置からのずれ(クライアントが attach の offset で動かす)。
#[derive(Debug, Clone)]
pub struct DndIcon {
    pub surface: WlSurface,
    pub offset: Point,
}

impl<D: SeatHandler + 'static> Frontend<D> {
    /// クライアントがドラッグ&ドロップを始めた。アイコンがあれば、覚える。
    pub fn dnd_started(&mut self, icon: Option<WlSurface>) {
        info!("a drag and drop started");
        self.dnd_active = true;
        self.dnd_icon = icon.map(|surface| DndIcon {
            surface,
            offset: Point::new(0, 0),
        });
    }

    /// ドラッグ&ドロップが終わった(落とした・取り消した)。
    pub fn dnd_dropped(&mut self, validated: bool) {
        info!(
            "a drag and drop ended ({})",
            if validated { "dropped" } else { "cancelled" }
        );
        self.dnd_active = false;
        self.dnd_icon = None;
    }

    /// アイコンの面の commit なら、attach の offset をずれに足して、true を返す。
    pub fn commit_dnd_icon(&mut self, surface: &WlSurface) -> bool {
        let Some(icon) = self
            .dnd_icon
            .as_mut()
            .filter(|icon| &icon.surface == surface)
        else {
            return false;
        };
        let delta = with_states(surface, |states| {
            states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .buffer_delta
                .take()
                .unwrap_or_default()
        });
        icon.offset = Point::new(icon.offset.x + delta.x, icon.offset.y + delta.y);
        true
    }

    /// 描くアイコンの面と、画面の上の左上の位置(ポインターの位置にずれを足したもの)。
    pub fn dnd_icon_at(&self) -> Option<(WlSurface, Point)> {
        let icon = self.dnd_icon.as_ref().filter(|icon| icon.surface.alive())?;
        let pointer = self.pointer.as_ref()?.current_location();
        Some((
            icon.surface.clone(),
            Point::new(
                pointer.x.round() as i32 + icon.offset.x,
                pointer.y.round() as i32 + icon.offset.y,
            ),
        ))
    }
}

/// キーボードのフォーカスが `focused` に移った(Smithay の `SeatHandler::focus_changed`)。selection を見せる相手を、
/// フォーカスのあるクライアントにする。[`delegate_frontend!`](crate::delegate_frontend) が使う。
pub fn keyboard_focus_changed<D: crate::FrontendHost>(state: &mut D, focused: Option<&WlSurface>) {
    let frontend = state.frontend();
    let display = frontend.display_handle.clone();
    let Some(seat) = frontend.seat.clone() else {
        return;
    };
    let client = focused.and_then(|surface| display.get_client(surface.id()).ok());
    smithay::wayland::selection::data_device::set_data_device_focus(&display, &seat, client);
}

/// クライアントがドラッグ&ドロップを始めた(Smithay の `ClientDndGrabHandler::started`)。[`delegate_frontend!`](crate::delegate_frontend) が使う。
pub fn drag_started<D: crate::FrontendHost>(state: &mut D, icon: Option<WlSurface>) {
    state.frontend_mut().dnd_started(icon);
    crate::changed(state);
}

/// ドラッグ&ドロップが終わった(Smithay の `ClientDndGrabHandler::dropped`)。[`delegate_frontend!`](crate::delegate_frontend) が使う。
pub fn drag_ended<D: crate::FrontendHost>(state: &mut D, validated: bool) {
    state.frontend_mut().dnd_dropped(validated);
    crate::changed(state);
}
