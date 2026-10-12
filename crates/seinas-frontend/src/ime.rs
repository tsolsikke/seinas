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
//!
//! 2 つ目を Smithay へ渡さない理由(Smithay 0.7.0 の不具合を避けるため):
//!
//! - `InputMethodHandle::add_instance` は、2 つ目が来ると、前からある IME の方へ `unavailable` を送り、新しい方には
//!   何も送らない(プロトコルでは、新しい方へ送るもの)。
//! - 2 つ目の IME も、1 つ目と同じ keyboard の grab の入れ物を指す。2 つ目が grab を取ってから手放すと、1 つ目の
//!   grab まで外れる(1 つ目のキーが届かなくなる)。場合によっては、keyboard の grab が残ったまま、Smithay が覚えて
//!   いる grab のオブジェクトだけが無くなり、次のキーで Smithay が `unwrap` で止まる。
//!
//! 2 つ目の要求が Smithay の部品に一度も届かなければ、どちらも起きない。

use smithay::{
    input::Seat,
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
            backend::ClientId, Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch,
        },
    },
    wayland::{
        input_method::{
            InputMethodHandler, InputMethodKeyboardUserData, InputMethodManagerGlobalData,
            InputMethodManagerState, InputMethodPopupSurfaceUserData, InputMethodUserData,
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
                InputMethodManagerState::new::<D, _>(display_handle, crate::is_shell_client),
                VirtualKeyboardManagerState::new::<D, _>(display_handle, crate::is_shell_client),
            )
        });
        ImeState {
            text_input: TextInputManagerState::new::<D>(display_handle),
            managers,
            connected: None,
        }
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
            info!("the input method left; keys go straight to the applications");
        }
    }
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
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<D>,
            D,
        >>::request(state, client, resource, request, data, dhandle, data_init);
    }
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
