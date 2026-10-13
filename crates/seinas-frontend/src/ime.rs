//! IME の道(`docs/adr/0004-ime-keyboard-path.md`)。アプリの text-input-v3 と、IME の input-method-v2 を
//! 取り次ぎ、IME が使わなかったキーを virtual-keyboard でアプリへ戻させる。
//!
//! 取り次ぎそのものは、Smithay 0.7.0 の部品(`wayland::text_input`、`wayland::input_method`、
//! `wayland::virtual_keyboard`)が行う。ここでは、その手前に Dispatch を 1 枚かぶせて、次のことを足す。
//!
//! - 見せる相手:input-method と virtual-keyboard の global は、シェルの部品のためのソケットからつないだ
//!   クライアントにだけ見せ、seat にキーボードがあるときだけ作る(Smithay 0.7.0 は、キーボードの無い seat で
//!   `get_input_method` や virtual-keyboard のキーを受けると、`unwrap` で止まるため)。text-input の global は、
//!   全部のクライアントに見せる。
//! - IME は同時に 1 つだけ:2 つ目からの `get_input_method` は、Smithay へ渡さずにここで受け、Seinas の側の
//!   入れ物([`RefusedInputMethod`])で作って、すぐ `unavailable` を送る。その後の要求は、何もせずに捨てる。
//! - IME が去ったこと、keyboard の grab を手放したことをログに出す。
//! - virtual-keyboard で戻ってきたキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーの
//!   ときだけ渡す。Alt+Tab で Alt を離すと、Seinas はその場でフォーカスを移すが、IME が戻す「Alt を離した」は
//!   後から届くので、そのままでは、押したことを知らない新しいフォーカスのクライアントへ届いてしまうため。前の
//!   フォーカスのクライアントには leave が届いているので、押されたまま残るクライアントは無い。
//! - 応答しない IME:キーの押下(修飾キーだけのものは数えない)を IME へ送ってから、決めた時間(既定は
//!   [`DEFAULT_IME_TIMEOUT`])の間に、その IME のクライアントから要求(input-method と virtual-keyboard のどちらでも
//!   よい)が 1 つも届かなければ、応答しないと見なして keyboard の grab を外す(次のキーから直接の道)。その後に
//!   要求が届いたら、grab を戻す(次のキーから IME を通る道)。どちらもログに出す。
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

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

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
                zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
                zwp_virtual_keyboard_v1::{self, ZwpVirtualKeyboardV1},
            },
        },
        wayland_server::{
            backend::ClientId, protocol::wl_surface::WlSurface, Client, DataInit, Dispatch,
            DisplayHandle, GlobalDispatch,
        },
    },
    wayland::{
        input_method::{
            InputMethodHandler, InputMethodKeyboardGrab, InputMethodKeyboardUserData,
            InputMethodManagerGlobalData, InputMethodManagerState, InputMethodPopupSurfaceUserData,
            InputMethodSeat, InputMethodUserData, PopupSurface,
        },
        text_input::{TextInputManagerState, TextInputUserData},
        virtual_keyboard::{
            VirtualKeyboardManagerGlobalData, VirtualKeyboardManagerState, VirtualKeyboardUserData,
        },
    },
};
use tracing::{info, warn};

use crate::FrontendHost;

/// IME が応答しないと見なすまでの時間の既定(ADR 0004。仮の値)。
pub const DEFAULT_IME_TIMEOUT: Duration = Duration::from_millis(1000);

/// IME の道の状態。
pub struct ImeState {
    /// text-input-v3 の global(全部のクライアントに見せる)。
    pub text_input: TextInputManagerState,
    /// input-method-v2 と virtual-keyboard の global。seat にキーボードがあるときだけ作る。
    pub managers: Option<(InputMethodManagerState, VirtualKeyboardManagerState)>,
    /// Smithay へ通した IME(同時に 1 つ)。
    connected: Option<Connected>,
    /// 応答しないと見なすまでの時間。
    timeout: Duration,
    /// IME へ送ったキーの押下の、返事の期限。返事を待っていなければNone。
    deadline: Option<Instant>,
    /// 応答しないと見なして、keyboard の grab を外しているか。外した grab は `suspended` に残す。
    unresponsive: bool,
    suspended: Option<InputMethodKeyboardGrab>,
    /// 見せている候補の窓。IME が有効で、親(フォーカスのあるウィンドウ)があるときだけ。
    pub popup: Option<PopupSurface>,
    /// virtual-keyboard で「押した」を渡したキー(evdev の番号)と、そのときのキーボードのフォーカス。フォーカスが
    /// 変わったら空にする(前のクライアントには leave が届き、押したキーは離したことになる)。
    vk_pressed: HashSet<u32>,
    vk_focus: Option<WlSurface>,
}

/// Smithay へ通した IME のクライアントと、そのオブジェクトが残っているか。
struct Connected {
    client: ClientId,
    /// `zwp_input_method_v2` が残っているか。
    input_method: bool,
    /// 残っている keyboard の grab(`zwp_input_method_keyboard_grab_v2`)の数。
    grabs: usize,
}

/// Smithay へ渡さなかった IME(2 つ目から)のオブジェクトに付けるもの。要求は、何もせずに捨てる。
#[derive(Debug)]
pub struct RefusedInputMethod;

impl ImeState {
    /// global を作る。input-method と virtual-keyboard は、`keyboard` が真(seat にキーボードがある)ときだけ作り、
    /// シェルの部品にだけ見せる。
    ///
    /// `timeout` は、IME が応答しないと見なすまでの時間。
    pub fn new<D>(display_handle: &DisplayHandle, keyboard: bool, timeout: Duration) -> Self
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
                InputMethodManagerState::new::<D, _>(display_handle, crate::is_shell_client),
                VirtualKeyboardManagerState::new::<D, _>(display_handle, crate::is_shell_client),
            )
        });
        ImeState {
            text_input: TextInputManagerState::new::<D>(display_handle),
            managers,
            connected: None,
            timeout,
            deadline: None,
            unresponsive: false,
            suspended: None,
            popup: None,
            vk_pressed: HashSet::new(),
            vk_focus: None,
        }
    }

    /// キーボードのフォーカスを動かしたときに呼ぶ(同じクライアントへの leave と enter のし直しも含む)。
    /// virtual-keyboard で「押した」を渡したキーの記録を空にする。
    pub fn keyboard_focus_moved(&mut self) {
        self.vk_pressed.clear();
        self.vk_focus = None;
    }

    /// 次に [`check_ime`] で確かめる時刻(IME へ送ったキーの返事の期限)。
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// IME が応答しないと見なして、直接の道にしているか。
    pub fn is_unresponsive(&self) -> bool {
        self.unresponsive
    }

    /// `client` が、Smithay へ通した IME か。
    fn is_ime(&self, client: &ClientId) -> bool {
        self.connected
            .as_ref()
            .is_some_and(|connected| connected.client == *client)
    }

    /// 返事の待ちと、応答しないと見なしたことを忘れる(IME が去ったとき、grab を手放したとき)。
    fn reset_watch(&mut self) {
        self.deadline = None;
        self.unresponsive = false;
        self.suspended = None;
    }

    /// IME がつないでいるか(Smithay へ通した `zwp_input_method_v2` か grab が残っているか)。
    pub fn is_connected(&self) -> bool {
        self.connected.is_some()
    }

    /// IME の `zwp_input_method_v2` か grab が 1 つ消えた後に呼ぶ。両方とも無くなったら、IME が去ったことにする。
    fn forget_if_gone(&mut self) {
        if self
            .connected
            .as_ref()
            .is_some_and(|connected| !connected.input_method && connected.grabs == 0)
        {
            self.connected = None;
            self.reset_watch();
            info!("the input method left; keys go straight to the applications");
        }
    }
}

/// いまの keyboard の grab が IME の grab なら、それ。
fn ime_grab<D: FrontendHost>(keyboard: &KeyboardHandle<D>) -> Option<InputMethodKeyboardGrab> {
    keyboard
        .with_grab(|_, grab| grab.downcast_ref::<InputMethodKeyboardGrab>().cloned())
        .flatten()
}

/// キーの押下(修飾キーだけのものではない)を、keyboard の grab へ渡した後に呼ぶ。IME の grab なら、返事を待ち
/// 始める(もう待っているときは、最初の期限のまま)。
pub(crate) fn key_sent<D: FrontendHost>(state: &mut D, keyboard: &KeyboardHandle<D>, now: Instant) {
    let ime = &mut state.frontend_mut().ime;
    if ime.connected.is_none() || ime.unresponsive || ime.deadline.is_some() {
        return;
    }
    if ime_grab(keyboard).is_some() {
        ime.deadline = Some(now + ime.timeout);
    }
}

/// IME へ送ったキーの返事の期限を、時刻 `now` で確かめる。過ぎていたら、応答しないと見なして keyboard の grab を
/// 外し、次のキーから直接の道にする。バックエンドが、イベントループを回るたびに呼ぶ(次の期限は
/// `frontend().ime.next_deadline()`)。
pub fn check_ime<D: FrontendHost>(state: &mut D, now: Instant) {
    let frontend = state.frontend_mut();
    if frontend.ime.deadline.is_none_or(|deadline| now < deadline) {
        return;
    }
    frontend.ime.deadline = None;
    frontend.ime.unresponsive = true;
    let timeout = frontend.ime.timeout.as_millis();
    if let Some(keyboard) = frontend.keyboard.clone() {
        if let Some(grab) = ime_grab(&keyboard) {
            frontend.ime.suspended = Some(grab);
            keyboard.unset_grab(state);
        }
    }
    warn!("the input method did not answer a key in {timeout} ms; keys go straight to the applications");
}

/// `client` から、input-method か virtual-keyboard の要求が届いた後に呼ぶ。IME なら返事の待ちを終え、応答しないと
/// 見なしていたら、keyboard の grab を戻して、次のキーから IME を通る道にする。
fn answered<D: FrontendHost>(state: &mut D, client: &ClientId) {
    let frontend = state.frontend_mut();
    if !frontend.ime.is_ime(client) {
        return;
    }
    frontend.ime.deadline = None;
    if !frontend.ime.unresponsive {
        return;
    }
    frontend.ime.unresponsive = false;
    let suspended = frontend.ime.suspended.take();
    // IME が、外している間に grab を手放していれば、戻さない。Smithay は grab を手放すと入れ物の grab の
    // オブジェクトを空にするので、空の入れ物を付け直すと、次のキーで `InputMethodKeyboardGrab::input` が
    // `unwrap` で止まる(中が空の grab になるのは、このように Smithay の外で付け直したときだけ)。
    let grabbed = frontend
        .seat
        .as_ref()
        .is_some_and(|seat| seat.input_method().keyboard_grabbed());
    if let (Some(grab), Some(keyboard), true) = (suspended, frontend.keyboard.clone(), grabbed) {
        if !keyboard.is_grabbed() {
            keyboard.set_grab(state, grab, SERIAL_COUNTER.next_serial());
        }
    }
    info!("the input method answered again; keys go through it again");
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
    } else if state.frontend().ime.is_connected() {
        Some("another input method is running")
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
                state.frontend_mut().ime.connected = Some(Connected {
                    client: client.id(),
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
        let grab = matches!(request, zwp_input_method_v2::Request::GrabKeyboard { .. });
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<D>, D>>::request(
            state, client, resource, request, data, dhandle, data_init,
        );
        let ime = &mut state.frontend_mut().ime;
        if let (true, Some(connected)) = (grab, ime.connected.as_mut()) {
            connected.grabs += 1;
        }
        answered(state, &client.id());
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
        if let Some(connected) = ime.connected.as_mut().filter(|c| c.client == client) {
            connected.input_method = false;
        }
        ime.forget_if_gone();
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
            ime.reset_watch();
            info!("the input method released the keyboard; keys go straight to the applications");
        }
        ime.forget_if_gone();
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
        if let zwp_virtual_keyboard_v1::Request::Key { key, state: 0, .. } = request {
            if !take_vk_press(state, key) {
                // 押したことを渡していないキーの「離した」は、渡さない(返事としては数える)。
                answered(state, &client.id());
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
        answered(state, &client.id());
    }
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
