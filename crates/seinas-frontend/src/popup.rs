//! ポップアップ(xdg_popup)と、layer surface のキーボードのフォーカス。
//!
//! - 置き方:positioner に従う。はみ出すときは、constraint_adjustment(反転・ずらし・縮め)を、Smithay の
//!   `PositionerState::get_unconstrained_geometry` で扱う。枠にするのは、ウィンドウのポップアップでは出力の使える範囲、
//!   layer surface のポップアップでは出力の全体(タスクバーは使える範囲の外にあるため)。
//! - 追い方と描く順:Smithay の `PopupManager`。ポップアップは、根元(ウィンドウか layer surface)のすぐ手前に描き、
//!   Seinas の飾り(枠・影・角丸)は付けない。根元のウィンドウの角丸のクリップにも掛けない。
//! - grab:xdg_popup.grab を、seat と serial(直前のポインターの押下か、キーの押下)を確かめてから受け、連なりを
//!   ここで持つ。grab の間は、grab したクライアントの面にだけポインターを渡し(xdg-shell の「owner-events」)、ほかの
//!   所を押したら連なりを閉じる(popup_done)。キーボードのフォーカスは、いちばん上のポップアップへ移す。Smithay の
//!   `PopupKeyboardGrab` は使わない(IME がキーボードを grab しているので、置き換えると IME の grab が戻らない)。
//! - layer surface の keyboard_interactivity:none はフォーカスを渡さない。exclusive は、top と overlay の層では
//!   キーを独り占めし、bottom と background の層では on_demand と同じに扱う(wlr-layer-shell の決まり)。on_demand は、
//!   押したときにフォーカスを渡し、ほかを押したら外す。
//!
//! 決まりの説明は `docs/window-placement.md` の「ポップアップ」と「layer surface と使える範囲」にある。

use smithay::{
    desktop::{
        find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
        utils::under_from_surface_tree, PopupKind, PopupManager, WindowSurfaceType,
    },
    input::SeatHandler,
    reexports::{
        wayland_protocols::xdg::shell::server::{xdg_popup, xdg_wm_base},
        wayland_server::{protocol::wl_surface::WlSurface, Resource},
    },
    utils::{IsAlive, Logical, Rectangle, Serial},
    wayland::{
        compositor::{get_role, with_states},
        shell::{
            wlr_layer::{KeyboardInteractivity, Layer, LayerSurfaceCachedState},
            xdg::{PopupSurface, PositionerState, XdgPopupSurfaceData, XDG_POPUP_ROLE},
        },
    },
};
use tracing::{info, warn};

use crate::{
    decoration::content_location,
    geometry::{Point, PointF, Rect},
    shape, Frontend,
};

/// 覚えておく、直前の入力の serial の数(grab の serial を確かめるのに使う)。
const RECENT_SERIALS: usize = 8;

/// grab したポップアップの連なり。`popups` は、下(根元に近いもの)から順。
#[derive(Debug, Clone)]
pub struct PopupGrabChain {
    pub root: WlSurface,
    pub popups: Vec<PopupSurface>,
}

impl PopupGrabChain {
    /// 生きている中で、いちばん上のポップアップ。
    fn topmost(&self) -> Option<&PopupSurface> {
        self.popups.iter().rev().find(|popup| popup.alive())
    }
}

/// grab を受けなかった理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabRefusal {
    /// serial が、直前の押下かキーのものでない(ポップアップは、すぐ閉じる)。
    Serial,
    /// もう描いたポップアップへの grab(xdg_popup.invalid_grab のエラー)。
    AlreadyMapped,
    /// 連なりのいちばん上でないポップアップの子への grab(xdg_wm_base.not_the_topmost_popup のエラー)。
    NotTopmost,
    /// 根元が見つからない(もう去った)。
    NoRoot,
}

impl<D: SeatHandler + 'static> Frontend<D> {
    /// 入力の serial を覚える(ポインターの押下と、キーの押下)。
    pub fn remember_input_serial(&mut self, serial: Serial) {
        if self.input_serials.len() == RECENT_SERIALS {
            self.input_serials.remove(0);
        }
        self.input_serials.push(serial);
    }

    /// 根元の面 `root` の、ウィンドウの幾何の左上(画面の上)と、ポップアップを収める枠。
    fn popup_root(&self, root: &WlSurface) -> Option<(Point, Rect)> {
        if let Some(window) = self
            .windows
            .iter()
            .find(|window| window.item.wl_surface() == root)
        {
            let shape = shape(&window.item)?;
            return Some((content_location(window.location, shape.bar), self.usable));
        }
        let map = layer_map_for_output(&self.output);
        let layer = map.layer_for_surface(root, WindowSurfaceType::TOPLEVEL)?;
        let geometry = map.layer_geometry(layer)?;
        Some((
            Point::new(geometry.loc.x, geometry.loc.y),
            Rect::from_size(self.size),
        ))
    }

    /// ポップアップ `popup` の置き方を、positioner と、収める枠で決める(まだ configure は送らない)。
    pub fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Some((origin, area)) = self.popup_root(&root) else {
            return;
        };
        // 枠は、親のポップアップの幾何(根元からの位置)と同じ座標で渡す。
        let parent = get_popup_toplevel_coords(&kind);
        let target = Rectangle::<i32, Logical>::new(
            (
                area.loc.x - origin.x - parent.x,
                area.loc.y - origin.y - parent.y,
            )
                .into(),
            (area.size.w, area.size.h).into(),
        );
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    /// 新しいポップアップ(xdg_surface.get_popup)。親がまだ無い(layer surface の get_popup を待つ)ときは、置き方を
    /// 後で決める。
    pub fn new_popup(&mut self, popup: PopupSurface, positioner: PositionerState) {
        popup.with_pending_state(|state| {
            state.positioner = positioner;
            state.geometry = positioner.get_geometry();
        });
        self.unconstrain_popup(&popup);
        if let Err(e) = self.popups.track_popup(PopupKind::Xdg(popup)) {
            warn!("a popup could not be tracked: {e:?}");
        }
    }

    /// layer surface を親にしたポップアップ(zwlr_layer_surface_v1.get_popup)。
    pub fn new_layer_popup(&mut self, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
    }

    /// クライアントが、ポップアップの置き方を変えたいと頼んだ(xdg_popup.reposition。xdg_wm_base のバージョン 3)。
    pub fn reposition_popup(
        &mut self,
        popup: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        popup.with_pending_state(|state| {
            state.positioner = positioner;
            state.geometry = positioner.get_geometry();
        });
        self.unconstrain_popup(&popup);
        popup.send_repositioned(token);
    }

    /// 面 `surface` の commit が、ポップアップのものなら、追いかけを進め、最初の commit なら configure を送る。
    /// ポップアップならtrue。
    pub fn commit_popup(&mut self, surface: &WlSurface) -> bool {
        self.popups.commit(surface);
        let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) else {
            return false;
        };
        if !popup.is_initial_configure_sent() {
            if let Err(e) = popup.send_configure() {
                warn!("a popup could not be configured: {e:?}");
            }
        }
        true
    }

    /// ポップアップ `popup` の grab(xdg_popup.grab)を受けるか決め、受けたら連なりに足す。
    pub fn grab_popup(&mut self, popup: PopupSurface, serial: Serial) -> Result<(), GrabRefusal> {
        if !self.input_serials.contains(&serial) {
            // 仕様:コンポジタが grab を断ったら、ポップアップはすぐ閉じる。
            popup.send_popup_done();
            return Err(GrabRefusal::Serial);
        }
        let committed = with_states(popup.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgPopupSurfaceData>()
                .is_some_and(|data| data.lock().expect("the popup data").committed)
        });
        if committed {
            popup
                .wl_surface()
                .post_error(xdg_popup::Error::InvalidGrab, "xdg_popup already is mapped");
            return Err(GrabRefusal::AlreadyMapped);
        }
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return Err(GrabRefusal::NoRoot);
        };
        let parent = popup.get_parent_surface();
        let parent_is_popup = parent
            .as_ref()
            .is_some_and(|parent| get_role(parent) == Some(XDG_POPUP_ROLE));
        let continues = self.popup_grab.as_ref().is_some_and(|chain| {
            chain.root == root && chain.topmost().map(PopupSurface::wl_surface) == parent.as_ref()
        });
        if continues {
            self.popup_grab
                .as_mut()
                .expect("the chain")
                .popups
                .push(popup);
            return Ok(());
        }
        if parent_is_popup {
            // grab していないポップアップか、連なりのいちばん上でないポップアップの子。
            popup.wl_surface().post_error(
                xdg_wm_base::Error::NotTheTopmostPopup,
                "xdg_popup was not created on the topmost popup",
            );
            return Err(GrabRefusal::NotTopmost);
        }
        // 新しい根元からの連なり。前の連なりは閉じる。
        self.dismiss_popup_grab();
        self.popup_grab = Some(PopupGrabChain {
            root,
            popups: vec![popup],
        });
        Ok(())
    }

    /// grab したポップアップの連なりを閉じる(下のポップアップから上へ、子から先に popup_done を送る)。閉じたらtrue。
    pub fn dismiss_popup_grab(&mut self) -> bool {
        let Some(chain) = self.popup_grab.take() else {
            return false;
        };
        if let Some(bottom) = chain.popups.iter().find(|popup| popup.alive()) {
            let _ = PopupManager::dismiss_popup(&chain.root, &PopupKind::Xdg(bottom.clone()));
            info!("the popup grab was dismissed");
        }
        true
    }

    /// 消えたポップアップを、連なりから外す。連なりが空になったら、grab を終える。
    pub fn forget_dead_popups(&mut self) {
        if let Some(chain) = &mut self.popup_grab {
            chain.popups.retain(|popup| popup.alive());
            if chain.popups.is_empty() || !chain.root.alive() {
                self.popup_grab = None;
            }
        }
        self.popups.cleanup();
    }

    /// grab している連なりのいちばん上のポップアップ(キーボードのフォーカスを付ける所)。
    pub fn popup_grab_focus(&self) -> Option<WlSurface> {
        self.popup_grab
            .as_ref()
            .filter(|chain| chain.root.alive())
            .and_then(PopupGrabChain::topmost)
            .map(|popup| popup.wl_surface().clone())
    }

    /// grab しているクライアントの面か。grab が無ければNone。
    pub fn popup_grab_owns(&self, surface: Option<&WlSurface>) -> Option<bool> {
        let chain = self
            .popup_grab
            .as_ref()
            .filter(|chain| chain.root.alive())?;
        Some(surface.is_some_and(|surface| surface.id().same_client_as(&chain.root.id())))
    }

    /// 根元 `root`(ウィンドウの幾何の左上が `origin`)のポップアップのうち、点 `point` で入力を受けるいちばん手前の面
    /// (ポップアップのサブサーフェスも含む)。その画面と、画面の左上の位置。
    ///
    /// 面の範囲と input region で決める(Smithay の `under_from_surface_tree`)。ポップアップの幾何
    /// (`PopupKind::geometry`)は、クライアントが set_window_geometry を送らないと 0x0 になるため、使わない。
    pub fn popup_under(
        &self,
        root: &WlSurface,
        origin: Point,
        point: PointF,
    ) -> Option<(WlSurface, PointF)> {
        self.popup_origins(root, origin)
            .into_iter()
            .find_map(|(surface, location)| {
                under_from_surface_tree(
                    &surface,
                    (point.x, point.y).into(),
                    (location.x, location.y),
                    WindowSurfaceType::ALL,
                )
                .map(|(surface, at)| (surface, Point::new(at.x, at.y).to_f64()))
            })
    }

    /// 根元 `root`(ウィンドウの幾何の左上が `origin`)のポップアップの、画面の左上の位置。手前のものから順。
    pub fn popup_origins(&self, root: &WlSurface, origin: Point) -> Vec<(WlSurface, Point)> {
        PopupManager::popups_for_surface(root)
            .map(|(popup, offset)| {
                let geometry = popup.geometry();
                (
                    popup.wl_surface().clone(),
                    Point::new(
                        origin.x + offset.x - geometry.loc.x,
                        origin.y + offset.y - geometry.loc.y,
                    ),
                )
            })
            .collect()
    }

    /// キーボードのフォーカスを、ほかに渡さずに独り占めする layer surface(top か overlay の層で、exclusive のもの)。
    /// 手前のものから探す。
    pub fn exclusive_layer(&self) -> Option<WlSurface> {
        let map = layer_map_for_output(&self.output);
        [Layer::Overlay, Layer::Top].into_iter().find_map(|layer| {
            map.layers_on(layer).rev().find_map(|mapped| {
                (keyboard_interactivity(mapped.wl_surface()) == KeyboardInteractivity::Exclusive)
                    .then(|| mapped.wl_surface().clone())
            })
        })
    }

    /// 押した layer surface `surface` が、押したときにフォーカスを受けるもの(on_demand か、bottom・background の
    /// 層の exclusive)か。
    pub fn takes_focus_on_press(&self, surface: &WlSurface) -> bool {
        let map = layer_map_for_output(&self.output);
        let Some(mapped) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) else {
            return false;
        };
        match keyboard_interactivity(surface) {
            KeyboardInteractivity::OnDemand => true,
            KeyboardInteractivity::Exclusive => {
                matches!(mapped.layer(), Layer::Bottom | Layer::Background)
            }
            KeyboardInteractivity::None => false,
        }
    }

    /// 押したときにフォーカスを受けた layer surface が、今も受けられるか(生きていて、まだ受ける設定か)。
    pub fn layer_focus_alive(&self) -> Option<WlSurface> {
        self.layer_focus
            .as_ref()
            .filter(|surface| surface.alive() && self.takes_focus_on_press(surface))
            .cloned()
    }
}

/// layer surface `surface` の、いまの keyboard_interactivity。
fn keyboard_interactivity(surface: &WlSurface) -> KeyboardInteractivity {
    with_states(surface, |states| {
        states
            .cached_state
            .get::<LayerSurfaceCachedState>()
            .current()
            .keyboard_interactivity
    })
}
