//! IME の道(`docs/adr/0004-ime-keyboard-path.md`)。アプリの text-input-v3 と、IME の input-method-v2 を
//! 取り次ぎ、IME が使わなかったキーを virtual-keyboard でアプリへ戻させる。
//!
//! 取り次ぎそのものは、Smithay 0.7.0 の部品(`wayland::text_input`、`wayland::input_method`、
//! `wayland::virtual_keyboard`)が行う。ここでは、その手前に Dispatch を 1 枚かぶせて、次のことを足す。
//!
//! - 見せる相手:input-method と virtual-keyboard の global は、IME のためのソケットからつないだクライアントにだけ
//!   見せ(シェルの部品とアプリには見せない。ADR 0004 の 1.6)、seat にキーボードがあるときだけ作る(Smithay 0.7.0 は、キーボードの無い seat で
//!   `get_input_method` や virtual-keyboard のキーを受けると、`unwrap` で止まるため)。text-input の global は、
//!   全部のクライアントに見せる。
//! - virtual-keyboard の認可:作れるのは、`get_input_method` を受け入れたクライアントだけ(ほかは `unauthorized`
//!   のエラー)。認可は、接続と、受け入れた IME の世代に結び付ける(seat は 1 つ)。受け入れた input method が
//!   無くなったら認可も消え、それまでに作った virtual-keyboard からの要求は捨てる(ADR 0004 の 1.6)。
//! - IME は同時に 1 つだけ:2 つ目からの `get_input_method` は、Smithay へ渡さずにここで受け、Seinas の側の
//!   入れ物([`RefusedInputMethod`])で作って、すぐ `unavailable` を送る。その後の要求は、何もせずに捨てる。
//! - IME が去ったこと、keyboard の grab を手放したことをログに出す。IME が去ったら、フォーカスのあるアプリへ
//!   `wl_keyboard` の leave と enter を送り直す(IME が virtual-keyboard で押したキーを、押されたまま残さない)。
//! - 緊急解除([`release_ime`]):Seinas のショートカットで、IME の認可を消し、grab を外し、候補の窓を隠して、
//!   接続を切る(ADR 0004 の 1.5)。切った接続のクリーンアップ(Smithay の部品の destroy)が済むまで、次の IME は
//!   受け入れない(Smithay 0.7.0 の destroy は、どの IME のものでも seat の instance と grab を空にするため)。
//! - virtual-keyboard で戻ってきたキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーの
//!   ときだけ渡す。Alt+Tab で Alt を離すと、Seinas はその場でフォーカスを移すが、IME が戻す「Alt を離した」は
//!   後から届くので、そのままでは、押したことを知らない新しいフォーカスのクライアントへ届いてしまうため。前の
//!   フォーカスのクライアントには leave が届いているので、押されたまま残るクライアントは無い。
//!
//! 2 つ目を Smithay へ渡さない理由(Smithay 0.7.0 の不具合を避けるため):
//!
//! - `InputMethodHandle::add_instance` は、2 つ目が来ると、前からある IME の方へ `unavailable` を送り、新しい方には
//!   何も送らない(プロトコルでは、新しい方へ送るもの)。
//! - 2 つ目の IME も、1 つ目と同じ keyboard の grab の入れ物を指す。2 つ目が grab を取ってから手放すと、1 つ目の
//!   grab まで外れる。1 つ目の IME は、知らされないまま grab を失う(キーが届かなくなる)。コンポジタは落ちない
//!   (Smithay は、入れ物の grab のオブジェクトと seat の keyboard の grab を、いつも一緒に付けて一緒に外すため)。
//!
//! 2 つ目の要求が Smithay の部品に一度も届かなければ、どちらも起きない。
//!
//! 候補の窓(input popup surface)は、見せるときと隠すときに Smithay が呼ぶ([`popup_shown`]、[`popup_hidden`])。置く
//! 位置は、描くときに、親のウィンドウのいまの位置と、カーソルの矩形と、画面の大きさから求める(`ime_popup.rs`)。

use std::collections::{HashMap, HashSet, VecDeque};

use smithay::utils::SERIAL_COUNTER;
use smithay::{
    input::{keyboard::KeyboardHandle, Seat},
    reexports::{
        wayland_protocols::wp::text_input::zv3::server::{
            zwp_text_input_manager_v3::ZwpTextInputManagerV3, zwp_text_input_v3::ZwpTextInputV3,
        },
        wayland_protocols_misc::{
            zwp_input_method_v2::server::{
                zwp_input_method_keyboard_grab_v2::{self, ZwpInputMethodKeyboardGrabV2},
                zwp_input_method_manager_v2::{self, ZwpInputMethodManagerV2},
                zwp_input_method_v2::{self, ZwpInputMethodV2},
                zwp_input_popup_surface_v2::{self, ZwpInputPopupSurfaceV2},
            },
            zwp_virtual_keyboard_v1::server::{
                zwp_virtual_keyboard_manager_v1::{self, ZwpVirtualKeyboardManagerV1},
                zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1},
            },
        },
        wayland_server::{
            backend::{protocol::ProtocolError, ClientId, ObjectId},
            protocol::wl_surface::WlSurface,
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, Resource,
        },
    },
    wayland::{
        input_method::{
            InputMethodHandler, InputMethodKeyboardGrab, InputMethodKeyboardUserData,
            InputMethodManagerGlobalData, InputMethodManagerState, InputMethodPopupSurfaceUserData,
            InputMethodUserData, PopupSurface,
        },
        text_input::{TextInputManagerState, TextInputUserData},
        virtual_keyboard::{
            VirtualKeyboardManagerGlobalData, VirtualKeyboardManagerState, VirtualKeyboardUserData,
        },
    },
};
use tracing::{info, warn};

use crate::FrontendHost;

/// IME の道の状態。
pub struct ImeState {
    /// text-input-v3 の global(全部のクライアントに見せる)。
    pub text_input: TextInputManagerState,
    /// input-method-v2 と virtual-keyboard の global。seat にキーボードがあるときだけ作る。
    pub managers: Option<(InputMethodManagerState, VirtualKeyboardManagerState)>,
    /// Smithay へ通した IME(同時に 1 つ)。
    connected: Option<Connected>,
    /// 緊急解除で接続を切った IME のクライアント。バックエンドが、すぐにクリーンアップさせる
    /// ([`crate::reap_dead_clients`])。
    pub(crate) to_reap: Option<Client>,
    /// 見せている候補の窓。IME が有効で、親(フォーカスのあるウィンドウ)があるときだけ。
    pub popup: Option<PopupSurface>,
    /// virtual-keyboard で「押した」を渡したキー(evdev の番号)と、そのときのキーボードのフォーカス。フォーカスが
    /// 変わったら空にする(前のクライアントには leave が届き、押したキーは離したことになる)。
    vk_pressed: HashSet<u32>,
    vk_focus: Option<WlSurface>,
    /// 受け入れた IME の世代(受け入れるたびに 1 つ進める)。
    generation: u64,
    /// virtual-keyboard を作ったときの世代。作ってから最初の要求が届くまでは、クライアントごとの待ち行列に
    /// 置く(Smithay が作るオブジェクトの番号は、作るときには分からないため)。
    vk_pending: HashMap<ClientId, VecDeque<u64>>,
    vk_generation: HashMap<ObjectId, u64>,
}

/// Smithay へ通した IME のクライアントと、そのオブジェクトが残っているか。
struct Connected {
    client: ClientId,
    /// 接続を切るのに使う。
    handle: Client,
    /// 受け入れたときの世代。
    generation: u64,
    /// 緊急解除で認可を消したか(接続を切った後、クリーンアップが済むまで残る)。
    revoked: bool,
    /// この世代の grab・virtual-keyboard の認可・候補ウィンドウを片付けたか([`retire`]。1 回だけ行う)。
    retired: bool,
    /// IME が去った後の `wl_keyboard` の leave と enter を送り直したか(1 回だけ行う)。
    refreshed: bool,
    /// `zwp_input_method_v2` が残っているか。
    input_method: bool,
    /// 残っている keyboard の grab(`zwp_input_method_keyboard_grab_v2`)の数。
    grabs: usize,
}

/// Smithay へ渡さなかった IME(2 つ目から)のオブジェクトに付けるもの。要求は、何もせずに捨てる。
#[derive(Debug)]
pub struct RefusedInputMethod;

/// 認可の無いクライアントが作ろうとした virtual-keyboard に付けるもの(作った直後に `unauthorized` のエラーで
/// 接続を切る)。要求は、何もせずに捨てる。
#[derive(Debug)]
pub struct RefusedVirtualKeyboard;

impl ImeState {
    /// global を作る。input-method と virtual-keyboard は、`keyboard` が真(seat にキーボードがある)ときだけ作り、
    /// IME のためのソケットからつないだクライアントにだけ見せる。
    ///
    pub fn new<D>(display_handle: &DisplayHandle, keyboard: bool) -> Self
    where
        D: GlobalDispatch<ZwpTextInputManagerV3, ()>
            + Dispatch<ZwpTextInputManagerV3, ()>
            + Dispatch<ZwpTextInputV3, TextInputUserData>
            + GlobalDispatch<ZwpInputMethodManagerV2, InputMethodManagerGlobalData>
            + Dispatch<ZwpInputMethodManagerV2, ()>
            + Dispatch<ZwpInputMethodV2, InputMethodUserData<D>>
            + GlobalDispatch<ZwpVirtualKeyboardManagerV1, VirtualKeyboardManagerGlobalData>
            + Dispatch<ZwpVirtualKeyboardManagerV1, ()>
            + Dispatch<ZwpVirtualKeyboardV1, VirtualKeyboardUserData<D>>
            + FrontendHost,
    {
        let managers = keyboard.then(|| {
            (
                InputMethodManagerState::new::<D, _>(display_handle, crate::is_ime_client),
                VirtualKeyboardManagerState::new::<D, _>(display_handle, crate::is_ime_client),
            )
        });
        ImeState {
            text_input: TextInputManagerState::new::<D>(display_handle),
            managers,
            connected: None,
            to_reap: None,
            popup: None,
            vk_pressed: HashSet::new(),
            vk_focus: None,
            generation: 0,
            vk_pending: HashMap::new(),
            vk_generation: HashMap::new(),
        }
    }

    /// `client` が、いま認められた IME なら、その世代。受け入れた input method が残っているときだけ。
    fn authorized(&self, client: &ClientId) -> Option<u64> {
        self.connected
            .as_ref()
            .filter(|connected| {
                connected.client == *client
                    && connected.input_method
                    && !connected.revoked
                    && !connected.retired
            })
            .map(|connected| connected.generation)
    }

    /// キーボードのフォーカスを動かしたときに呼ぶ(同じクライアントへの leave と enter のし直しも含む)。
    /// virtual-keyboard で「押した」を渡したキーの記録を空にする。
    pub fn keyboard_focus_moved(&mut self) {
        self.vk_pressed.clear();
        self.vk_focus = None;
    }

    /// IME がつないでいるか(Smithay へ通した `zwp_input_method_v2` か grab が残っているか)。
    pub fn is_connected(&self) -> bool {
        self.connected.is_some()
    }

    /// IME の `zwp_input_method_v2` か grab が 1 つ消えた後に呼ぶ。両方とも無くなったら、IME が去ったことにする。
    /// そのとき、`wl_keyboard` の leave と enter をまだ送り直していなければtrueを返す。
    fn forget_if_gone(&mut self) -> bool {
        if !self
            .connected
            .as_ref()
            .is_some_and(|connected| !connected.input_method && connected.grabs == 0)
        {
            return false;
        }
        let Some(connected) = self.connected.take() else {
            return false;
        };
        self.vk_pending.remove(&connected.client);
        info!("the input method left; keys go straight to the applications");
        !connected.refreshed
    }
}

/// いまの keyboard の grab が IME の grab なら、それ。
fn ime_grab<D: FrontendHost>(keyboard: &KeyboardHandle<D>) -> Option<InputMethodKeyboardGrab> {
    keyboard
        .with_grab(|_, grab| grab.downcast_ref::<InputMethodKeyboardGrab>().cloned())
        .flatten()
}

/// 緊急解除(ADR 0004 の 1.5)。IME がつないでいれば、次の順に行い、trueを返す。
///
/// 1. その接続の認可を消す(この後、その接続の input-method と virtual-keyboard の要求は捨てる)。
/// 2. keyboard の grab を外し(次のキーから直接の道)、候補の窓を隠し、接続を切る。
///
/// IME へ渡したキーは、アプリへ送り直さない。切った接続のクリーンアップ(Smithay の部品の destroy と、
/// IME が去った後の `wl_keyboard` の leave と enter)は、バックエンドが [`crate::reap_dead_clients`] で、すぐ行わせる。
pub fn release_ime<D: FrontendHost>(state: &mut D) -> bool {
    let frontend = state.frontend_mut();
    let Some(connected) = frontend.ime.connected.as_mut() else {
        info!("the emergency release key was pressed, but no input method is running");
        return false;
    };
    if connected.revoked {
        info!("the emergency release key was pressed, but no input method is running");
        return false;
    }
    // 1. 認可を消す。
    connected.revoked = true;
    let client = connected.handle.clone();
    // 2. grab を外し、候補ウィンドウを隠し、接続を切る。
    retire(state, &client.id());
    let frontend = state.frontend_mut();
    client.kill(
        &frontend.display_handle,
        ProtocolError {
            code: 0,
            object_id: 1,
            object_interface: "wl_display".to_owned(),
            message: "the input method was released with the emergency key".to_owned(),
        },
    );
    frontend.ime.to_reap = Some(client);
    frontend.reap_requested = true;
    warn!(
        "the input method was released with the emergency key; its connection is closed and keys go straight to the applications"
    );
    true
}

/// 受け入れた IME の今の世代の、grab・virtual-keyboard の認可・候補ウィンドウを片付ける(クライアント `client` の
/// ものだけ。1 回だけ行い、2 回目からは何もしない)。keyboard の grab は、それが IME の grab のときだけ外す(次の
/// キーから直接の道)。物理のキーボードの状態には触れない。片付けたらtrue。
///
/// 新しい IME は、この世代の input method と grab が両方とも無くなるまで受け入れない([`refusal`])ので、ここで
/// 外す grab は、いつもこの世代のもの。
fn retire<D: FrontendHost>(state: &mut D, client: &ClientId) -> bool {
    let frontend = state.frontend_mut();
    let Some(connected) = frontend
        .ime
        .connected
        .as_mut()
        .filter(|c| c.client == *client && !c.retired)
    else {
        return false;
    };
    connected.retired = true;
    frontend.ime.vk_pending.remove(client);
    frontend.ime.popup = None;
    if let Some(keyboard) = frontend.keyboard.clone() {
        if ime_grab(&keyboard).is_some() {
            keyboard.unset_grab(state);
        }
    }
    state.redraw_needed();
    true
}

/// IME が去った後に呼ぶ。フォーカスのあるアプリへ `wl_keyboard` の leave と enter を送り直す。IME が
/// virtual-keyboard で押したキー(アプリには押されたまま)は enter に入らず、enter に入るのは、物理のキーボードで
/// 今押されていて、IME かアプリへ渡したキーだけ(Smithay の `forwarded_pressed_keys`)。
fn refresh_keyboard_focus<D: FrontendHost>(state: &mut D) {
    let Some(keyboard) = state.frontend().keyboard.clone() else {
        return;
    };
    state.frontend_mut().ime.keyboard_focus_moved();
    if let Some(focus) = keyboard.current_focus() {
        keyboard.set_focus(state, None, SERIAL_COUNTER.next_serial());
        keyboard.set_focus(state, Some(focus), SERIAL_COUNTER.next_serial());
    }
}

/// 候補の窓を見せる(Smithay の `InputMethodHandler::new_popup`)。[`delegate_frontend!`] が使う。
///
/// [`delegate_frontend!`]: crate::delegate_frontend
pub fn popup_shown<D: FrontendHost>(state: &mut D, popup: PopupSurface) {
    state.frontend_mut().ime.popup = Some(popup);
    state.redraw_needed();
}

/// 候補の窓を隠す(Smithay の `InputMethodHandler::dismiss_popup`)。[`delegate_frontend!`] が使う。
///
/// [`delegate_frontend!`]: crate::delegate_frontend
pub fn popup_hidden<D: FrontendHost>(state: &mut D, popup: PopupSurface) {
    let ime = &mut state.frontend_mut().ime;
    if ime.popup.as_ref() == Some(&popup) {
        ime.popup = None;
    }
    state.redraw_needed();
}

/// 新しい IME を断る理由。断らなければNone。
fn refusal<D: FrontendHost>(state: &D, seat: &Seat<D>) -> Option<&'static str> {
    if seat.get_keyboard().is_none() {
        Some("the seat has no keyboard")
    } else if let Some(connected) = &state.frontend().ime.connected {
        Some(if connected.revoked {
            "the released input method is not cleaned up yet"
        } else if !connected.input_method {
            "the destroyed input method still has a keyboard grab"
        } else {
            "another input method is running"
        })
    } else {
        None
    }
}

impl<D> Dispatch<ZwpInputMethodManagerV2, (), D> for ImeState
where
    D: Dispatch<ZwpInputMethodManagerV2, ()>
        + Dispatch<ZwpInputMethodV2, InputMethodUserData<D>>
        + Dispatch<ZwpInputMethodV2, RefusedInputMethod>
        + InputMethodHandler
        + FrontendHost,
{
    fn request(
        state: &mut D,
        client: &Client,
        resource: &ZwpInputMethodManagerV2,
        request: zwp_input_method_manager_v2::Request,
        data: &(),
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        let request = match request {
            zwp_input_method_manager_v2::Request::GetInputMethod { seat, input_method } => {
                let reason = match Seat::<D>::from_resource(&seat) {
                    Some(seat) => refusal(state, &seat),
                    None => Some("the seat is gone"),
                };
                if let Some(reason) = reason {
                    let refused = data_init.init(input_method, RefusedInputMethod);
                    refused.unavailable();
                    warn!("an input method was refused ({reason}); it got unavailable");
                    return;
                }
                let ime = &mut state.frontend_mut().ime;
                ime.generation += 1;
                ime.connected = Some(Connected {
                    client: client.id(),
                    handle: client.clone(),
                    generation: ime.generation,
                    revoked: false,
                    retired: false,
                    refreshed: false,
                    input_method: true,
                    grabs: 0,
                });
                info!("an input method connected");
                zwp_input_method_manager_v2::Request::GetInputMethod { seat, input_method }
            }
            other => other,
        };
        <InputMethodManagerState as Dispatch<ZwpInputMethodManagerV2, (), D>>::request(
            state, client, resource, request, data, dhandle, data_init,
        );
    }
}

impl<D> Dispatch<ZwpInputMethodV2, InputMethodUserData<D>, D> for ImeState
where
    D: Dispatch<ZwpInputMethodV2, InputMethodUserData<D>>
        + Dispatch<ZwpInputPopupSurfaceV2, InputMethodPopupSurfaceUserData>
        + Dispatch<ZwpInputMethodKeyboardGrabV2, InputMethodKeyboardUserData<D>>
        + Dispatch<ZwpInputPopupSurfaceV2, RefusedInputMethod>
        + Dispatch<ZwpInputMethodKeyboardGrabV2, RefusedInputMethod>
        + InputMethodHandler
        + FrontendHost,
{
    fn request(
        state: &mut D,
        client: &Client,
        resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        data: &InputMethodUserData<D>,
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        let revoked = state
            .frontend()
            .ime
            .connected
            .as_ref()
            .is_some_and(|c| c.client == client.id() && c.revoked);
        if revoked {
            // 緊急解除で認可を消した接続の要求は、捨てる。新しいオブジェクトは作らないといけないので、断った
            // IME と同じ入れ物で作る。
            match request {
                zwp_input_method_v2::Request::GrabKeyboard { keyboard } => {
                    data_init.init(keyboard, RefusedInputMethod);
                }
                zwp_input_method_v2::Request::GetInputPopupSurface { id, .. } => {
                    data_init.init(id, RefusedInputMethod);
                }
                _ => {}
            }
            return;
        }
        let grab = matches!(request, zwp_input_method_v2::Request::GrabKeyboard { .. });
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<D>, D>>::request(
            state, client, resource, request, data, dhandle, data_init,
        );
        let ime = &mut state.frontend_mut().ime;
        if let (true, Some(connected)) = (grab, ime.connected.as_mut()) {
            connected.grabs += 1;
        }
    }

    fn destroyed(
        state: &mut D,
        client: ClientId,
        resource: &ZwpInputMethodV2,
        data: &InputMethodUserData<D>,
    ) {
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<D>, D>>::destroyed(
            state, client.clone(), resource, data,
        );
        let ime = &mut state.frontend_mut().ime;
        let Some(connected) = ime.connected.as_mut().filter(|c| c.client == client) else {
            return;
        };
        connected.input_method = false;
        // 仕様では、input method の destroy は、その子(候補ウィンドウと keyboard の grab)も破棄する
        // (input-method-unstable-v2.xml の destroy)。grab のオブジェクトが残っていても、この世代を片付けて、
        // 直接の道に戻す。Smithay の部品は、ここで text-input の leave を送り終えている。
        let grab_left = connected.grabs > 0;
        let retired = retire(state, &client);
        if state.frontend_mut().ime.forget_if_gone() {
            refresh_keyboard_focus(state);
        } else if retired && grab_left {
            info!(
                "the input method was destroyed; its keyboard grab is ignored and keys go straight to the applications"
            );
            refresh_keyboard_focus(state);
            if let Some(connected) = state.frontend_mut().ime.connected.as_mut() {
                connected.refreshed = true;
            }
        }
    }
}

impl<D> Dispatch<ZwpInputMethodKeyboardGrabV2, InputMethodKeyboardUserData<D>, D> for ImeState
where
    D: FrontendHost,
{
    fn request(
        state: &mut D,
        client: &Client,
        resource: &ZwpInputMethodKeyboardGrabV2,
        request: zwp_input_method_keyboard_grab_v2::Request,
        data: &InputMethodKeyboardUserData<D>,
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        <InputMethodManagerState as Dispatch<
            ZwpInputMethodKeyboardGrabV2,
            InputMethodKeyboardUserData<D>,
            D,
        >>::request(state, client, resource, request, data, dhandle, data_init);
    }

    fn destroyed(
        state: &mut D,
        client: ClientId,
        resource: &ZwpInputMethodKeyboardGrabV2,
        data: &InputMethodKeyboardUserData<D>,
    ) {
        // Smithay は、ここで keyboard の grab を外す。
        <InputMethodManagerState as Dispatch<
            ZwpInputMethodKeyboardGrabV2,
            InputMethodKeyboardUserData<D>,
            D,
        >>::destroyed(state, client.clone(), resource, data);
        let ime = &mut state.frontend_mut().ime;
        let Some(connected) = ime.connected.as_mut().filter(|c| c.client == client) else {
            return;
        };
        connected.grabs = connected.grabs.saturating_sub(1);
        if connected.grabs == 0 && connected.input_method {
            info!("the input method released the keyboard; keys go straight to the applications");
        }
        if ime.forget_if_gone() {
            refresh_keyboard_focus(state);
        }
    }
}

impl<D> Dispatch<ZwpVirtualKeyboardV1, VirtualKeyboardUserData<D>, D> for ImeState
where
    D: Dispatch<ZwpVirtualKeyboardV1, VirtualKeyboardUserData<D>> + FrontendHost,
{
    fn request(
        state: &mut D,
        client: &Client,
        resource: &ZwpVirtualKeyboardV1,
        request: zwp_virtual_keyboard_v1::Request,
        data: &VirtualKeyboardUserData<D>,
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        if !vk_authorized(state, &client.id(), &resource.id()) {
            // 認可の無い(消えた)virtual-keyboard からの要求は、捨てる。
            return;
        }
        if let zwp_virtual_keyboard_v1::Request::Key { key, state: 0, .. } = request {
            if !take_vk_press(state, key) {
                // 押したことを渡していないキーの「離した」は、渡さない。
                return;
            }
        }
        let press = match request {
            zwp_virtual_keyboard_v1::Request::Key { key, state: 1, .. } => Some(key),
            _ => None,
        };
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<D>,
            D,
        >>::request(state, client, resource, request, data, dhandle, data_init);
        if let Some(key) = press {
            note_vk_press(state, key);
        }
    }
    fn destroyed(
        state: &mut D,
        client: ClientId,
        resource: &ZwpVirtualKeyboardV1,
        data: &VirtualKeyboardUserData<D>,
    ) {
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<D>,
            D,
        >>::destroyed(state, client, resource, data);
        state
            .frontend_mut()
            .ime
            .vk_generation
            .remove(&resource.id());
    }
}

/// virtual-keyboard `object`(クライアント `client` のもの)が、いま認められているか。作ったときの世代が、いま
/// 認められた IME の世代と同じときだけ。最初の要求で、作ったときの世代を決める(待ち行列の先頭)。
fn vk_authorized<D: FrontendHost>(state: &mut D, client: &ClientId, object: &ObjectId) -> bool {
    let ime = &mut state.frontend_mut().ime;
    let generation = match ime.vk_generation.get(object) {
        Some(&generation) => Some(generation),
        None => {
            let pending = ime
                .vk_pending
                .get_mut(client)
                .and_then(|queue| queue.pop_front());
            if let Some(generation) = pending {
                ime.vk_generation.insert(object.clone(), generation);
            }
            pending
        }
    };
    generation.is_some() && generation == ime.authorized(client)
}

/// いまのキーボードのフォーカス。
fn current_focus<D: FrontendHost>(state: &D) -> Option<WlSurface> {
    state.frontend().keyboard.as_ref()?.current_focus()
}

/// virtual-keyboard で、キー `key` の「押した」をいまのフォーカスへ渡した後に呼ぶ。
fn note_vk_press<D: FrontendHost>(state: &mut D, key: u32) {
    let focus = current_focus(state);
    let ime = &mut state.frontend_mut().ime;
    if ime.vk_focus != focus {
        ime.vk_pressed.clear();
        ime.vk_focus = focus.clone();
    }
    if focus.is_some() {
        ime.vk_pressed.insert(key);
    }
}

/// virtual-keyboard で、キー `key` の「離した」が届いた。いまのフォーカスへ「押した」を渡したキーなら、記録から
/// 外してtrue(「離した」を渡す)。
fn take_vk_press<D: FrontendHost>(state: &mut D, key: u32) -> bool {
    let focus = current_focus(state);
    let ime = &mut state.frontend_mut().ime;
    if focus.is_none() || ime.vk_focus != focus {
        return false;
    }
    ime.vk_pressed.remove(&key)
}

impl<D> Dispatch<ZwpInputMethodV2, RefusedInputMethod, D> for ImeState
where
    D: Dispatch<ZwpInputMethodV2, RefusedInputMethod>
        + Dispatch<ZwpInputMethodKeyboardGrabV2, RefusedInputMethod>
        + Dispatch<ZwpInputPopupSurfaceV2, RefusedInputMethod>,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        _data: &RefusedInputMethod,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        // 新しいオブジェクトは作らないといけないので、同じ入れ物で作る(キーも候補の窓も、届かないし出ない)。
        match request {
            zwp_input_method_v2::Request::GrabKeyboard { keyboard } => {
                data_init.init(keyboard, RefusedInputMethod);
            }
            zwp_input_method_v2::Request::GetInputPopupSurface { id, .. } => {
                data_init.init(id, RefusedInputMethod);
            }
            _ => {}
        }
    }
}

impl<D> Dispatch<ZwpInputMethodKeyboardGrabV2, RefusedInputMethod, D> for ImeState {
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwpInputMethodKeyboardGrabV2,
        _request: zwp_input_method_keyboard_grab_v2::Request,
        _data: &RefusedInputMethod,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
    }
}

impl<D> Dispatch<ZwpInputPopupSurfaceV2, RefusedInputMethod, D> for ImeState {
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwpInputPopupSurfaceV2,
        _request: zwp_input_popup_surface_v2::Request,
        _data: &RefusedInputMethod,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
    }
}

impl<D> Dispatch<ZwpVirtualKeyboardManagerV1, (), D> for ImeState
where
    D: Dispatch<ZwpVirtualKeyboardManagerV1, ()>
        + Dispatch<ZwpVirtualKeyboardV1, VirtualKeyboardUserData<D>>
        + Dispatch<ZwpVirtualKeyboardV1, RefusedVirtualKeyboard>
        + FrontendHost,
{
    fn request(
        state: &mut D,
        client: &Client,
        resource: &ZwpVirtualKeyboardManagerV1,
        request: zwp_virtual_keyboard_manager_v1::Request,
        data: &(),
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        if let zwp_virtual_keyboard_manager_v1::Request::CreateVirtualKeyboard { seat, id } =
            request
        {
            let ime = &mut state.frontend_mut().ime;
            let Some(generation) = ime.authorized(&client.id()) else {
                // get_input_method を受け入れていないクライアント。新しいオブジェクトは作らないといけないので、
                // Seinas の側の入れ物で作り、unauthorized のエラーで接続を切る。
                data_init.init(id, RefusedVirtualKeyboard);
                resource.post_error(
                    zwp_virtual_keyboard_manager_v1::Error::Unauthorized,
                    "only the accepted input method may create a virtual keyboard",
                );
                warn!(
                    "a virtual keyboard was refused (the client is not the accepted input method)"
                );
                return;
            };
            ime.vk_pending
                .entry(client.id())
                .or_default()
                .push_back(generation);
            let request =
                zwp_virtual_keyboard_manager_v1::Request::CreateVirtualKeyboard { seat, id };
            <VirtualKeyboardManagerState as Dispatch<ZwpVirtualKeyboardManagerV1, (), D>>::request(
                state, client, resource, request, data, dhandle, data_init,
            );
        }
    }
}

impl<D> Dispatch<ZwpVirtualKeyboardV1, RefusedVirtualKeyboard, D> for ImeState {
    fn request(
        _state: &mut D,
        _client: &Client,
        _resource: &ZwpVirtualKeyboardV1,
        _request: zwp_virtual_keyboard_v1::Request,
        _data: &RefusedVirtualKeyboard,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
    }
}
