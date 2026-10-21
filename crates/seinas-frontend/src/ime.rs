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
//! - 緊急解除([`release_ime`]):Seinas のショートカットで、IME の認可を消し、grab を外し、候補ウィンドウを隠して、
//!   接続を切る(ADR 0004 の 1.5)。切った接続のクリーンアップ(Smithay の部品の destroy)が済むまで、次の IME は
//!   受け入れない(Smithay 0.7.0 の destroy は、どの IME のものでも seat の instance と grab を空にするため)。
//! - virtual-keyboard で戻ってきたキーの「離した」は、いまのフォーカスのクライアントに「押した」を渡したキーの
//!   ときだけ渡す。Alt+Tab で Alt を離すと、Seinas はその場でフォーカスを移すが、IME が戻す「Alt を離した」は
//!   後から届くので、そのままでは、押したことを知らない新しいフォーカスのクライアントへ届いてしまうため。前の
//!   フォーカスのクライアントには leave が届いているので、押されたまま残るクライアントは無い。
//! - virtual-keyboard の押下の状態は、オブジェクトごとに持つ([`VkInput`])。virtual-keyboard が壊れたとき
//!   (destroy・接続断・認可の取り消し・緊急解除)に、押したままのキーの「離した」をキーごとに送り、modifiers を
//!   物理のキーボードの状態へ戻す([`release_vk_input`])。grab の release だけでは消さない(ADR 0004 の 1.9)。
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
//! 候補ウィンドウ(input popup surface)は、見せるときと隠すときに Smithay が呼ぶ([`popup_shown`]、[`popup_hidden`])。置く
//! 位置は、描くときに、親のウィンドウのいまの位置と、カーソルの矩形と、画面の大きさから求める(`ime_popup.rs`)。

use std::collections::{HashMap, HashSet, VecDeque};

use smithay::utils::SERIAL_COUNTER;
use smithay::{
    backend::input::Keycode,
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
            protocol::{wl_keyboard::KeyState as WlKeyState, wl_surface::WlSurface},
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
    /// 見せている候補ウィンドウ。IME が有効で、親(フォーカスのあるウィンドウ)があるときだけ。
    pub popup: Option<PopupSurface>,
    /// virtual-keyboard のオブジェクトごとの、アプリへ渡した押下の状態([`VkInput`])。オブジェクトが壊れたとき
    /// (destroy・接続断・認可の取り消し・緊急解除)に、[`release_vk_input`] で片付ける。
    vk_inputs: HashMap<ObjectId, VkInput>,
    /// キーボードのフォーカスの世代(フォーカスが動くたびに 1 つ進める。同じクライアントへの leave と enter の
    /// し直しも含む)。進んだ後は、前の世代で渡した押下は、アプリの側で leave により消えている。
    focus_generation: u64,
    /// 受け入れた IME の世代(受け入れるたびに 1 つ進める)。
    generation: u64,
    /// virtual-keyboard を作ったときの世代。作ってから最初の要求が届くまでは、クライアントごとの待ち行列
    /// ([`PendingKeyboards`])に置く(Smithay が作るオブジェクトの番号は、作るときには分からないため)。
    vk_pending: HashMap<ClientId, PendingKeyboards>,
    vk_generation: HashMap<ObjectId, u64>,
    /// クライアントごとの、生きている virtual-keyboard(Smithay へ通したもの)。
    vk_clients: HashMap<ClientId, ClientKeyboards>,
}

/// 1 つの virtual-keyboard が、アプリへ渡した押下の状態。
///
/// 記録するのは、フォーカスのあるアプリへ実際に渡したものだけ。フォーカスが動いた後(`focus_generation` が
/// 違う)の記録は、アプリの側で leave により消えているので、片付けでは何も送らない。
struct VkInput {
    client: ClientId,
    /// 押したまま、まだ「離した」を渡していないキー(evdev の番号)。
    keys: HashSet<u32>,
    /// `modifiers` の要求で渡したか(渡していれば、片付けで物理のキーボードの状態に戻す)。
    modifiers: bool,
    /// 渡した先(フォーカスのあった surface)と、そのときのフォーカスの世代。
    target: Option<(WlSurface, u64)>,
    /// 最後に届いたキーの時刻(片付けで送る「離した」に使う)。
    time: u32,
}

/// 1 つのクライアントの、生きている virtual-keyboard の数と、それを作った manager。
struct ClientKeyboards {
    live: usize,
    /// 世代を見分けられないときに、`unauthorized` のエラーを送る先(最後に作ったときの manager)。
    manager: ZwpVirtualKeyboardManagerV1,
}

/// 1 つのクライアントの、作ったが、まだ最初の要求が届いていない virtual-keyboard。
///
/// `queue` は、今の世代で作ったものの世代(どれも同じ世代。世代が変わるのは、前の IME が去った後だけで、その
/// ときに `queue` は `stale` に移すため)。`stale` は、前の世代で作ったものの数。どのオブジェクトがどれかは
/// 分からないので、`stale` と `queue` の両方があるときに最初の要求が届いたら、見分けられないとして、そのとき
/// 待っているものを全部認めない(安全側)。
#[derive(Debug, Default)]
struct PendingKeyboards {
    queue: VecDeque<u64>,
    stale: usize,
}

impl PendingKeyboards {
    fn len(&self) -> usize {
        self.queue.len() + self.stale
    }

    /// 前の IME が去った(認可が消えた)。待っているものは、どれも前の世代のものになる。
    fn retire(&mut self) {
        self.stale += self.queue.len();
        self.queue.clear();
    }

    /// 最初の要求が届いたオブジェクトの世代。
    fn take(&mut self) -> Taken {
        match (self.stale, self.queue.is_empty()) {
            (0, _) => self
                .queue
                .pop_front()
                .map_or(Taken::Refused, Taken::Generation),
            (_, true) => {
                self.stale -= 1;
                Taken::Refused
            }
            (_, false) => {
                // 見分けられない:このオブジェクトを認めず、ほかの待っているものも、前の世代として扱う。
                self.stale = self.len() - 1;
                self.queue.clear();
                Taken::Ambiguous
            }
        }
    }

    /// 最初の要求を送らずに destroy された。
    fn forget_one(&mut self) {
        match (self.stale, self.queue.is_empty()) {
            (0, _) => {
                self.queue.pop_front();
            }
            (_, true) => self.stale -= 1,
            (_, false) => {
                self.stale = self.len() - 1;
                self.queue.clear();
            }
        }
    }
}

/// [`PendingKeyboards::take`] の結果。
#[derive(Debug, PartialEq, Eq)]
enum Taken {
    /// 作ったときの世代。
    Generation(u64),
    /// 前の世代のもの(または待ち行列に無いもの)。認めない。
    Refused,
    /// 前の世代のものと今の世代のものが両方待っていて、どちらか見分けられない。認めず、IME の接続を切る。
    Ambiguous,
}

/// 1 つのクライアントが、最初の要求を送らずに持てる virtual-keyboard の数の上限。越えて作ろうとしたら、
/// `unauthorized` のエラーにする(待ち行列が増え続けないように)。
const MAX_PENDING_KEYBOARDS: usize = 8;

/// 1 つのクライアントが、同時に持てる virtual-keyboard(待っているものも、最初の要求を送ったものも)の数の
/// 上限。越えて作ろうとしたら、`unauthorized` のエラーにする。
///
/// 根拠:IME が同時に使う virtual-keyboard は、ふつう 1 つ(テスト用の IME も 1 つ)。keymap を変えるために
/// 作り直す IME もあるので、待ち行列の上限([`MAX_PENDING_KEYBOARDS`])の 2 倍まで余裕を見る。生きている
/// オブジェクトごとに、Smithay が keymap(xkb)を持ち、Seinas が世代の記録(`vk_generation`)を持つので、数に
/// 上限を置いて、メモリーが増え続けないようにする。世代の記録は、生きているオブジェクトのぶんだけ(destroy で
/// 消す)なので、この上限で一緒に抑えられる。
const MAX_KEYBOARDS: usize = 16;

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
            vk_inputs: HashMap::new(),
            focus_generation: 0,
            generation: 0,
            vk_pending: HashMap::new(),
            vk_generation: HashMap::new(),
            vk_clients: HashMap::new(),
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
    /// フォーカスの世代を進める(前の世代で virtual-keyboard が渡した押下は、leave でアプリの側から消えている)。
    pub fn keyboard_focus_moved(&mut self) {
        self.focus_generation += 1;
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
        if let Some(pending) = self.vk_pending.get_mut(&connected.client) {
            pending.retire();
        }
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
/// 2. keyboard の grab を外し(次のキーから直接の道)、候補ウィンドウを隠し、接続を切る。
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
        info!(
            "the emergency release key was pressed, but the input method is already released and its connection is being cleaned up"
        );
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
    if let Some(pending) = frontend.ime.vk_pending.get_mut(client) {
        pending.retire();
    }
    frontend.ime.popup = None;
    // 認可が消えると、この接続の virtual-keyboard からの「離した」は、もう届かない(捨てる)。押したままのものを片付ける。
    let objects: Vec<ObjectId> = frontend
        .ime
        .vk_inputs
        .iter()
        .filter(|(_, input)| input.client == *client)
        .map(|(object, _)| object.clone())
        .collect();
    let keyboard = frontend.keyboard.clone();
    if let Some(keyboard) = keyboard {
        if ime_grab(&keyboard).is_some() {
            keyboard.unset_grab(state);
        }
    }
    for object in objects {
        release_vk_input(state, &object);
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

/// 候補ウィンドウを見せる(Smithay の `InputMethodHandler::new_popup`)。[`delegate_frontend!`] が使う。
///
/// [`delegate_frontend!`]: crate::delegate_frontend
pub fn popup_shown<D: FrontendHost>(state: &mut D, popup: PopupSurface) {
    state.frontend_mut().ime.popup = Some(popup);
    state.redraw_needed();
}

/// 候補ウィンドウを隠す(Smithay の `InputMethodHandler::dismiss_popup`)。[`delegate_frontend!`] が使う。
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
        if !vk_authorized(state, client, &resource.id()) {
            // 認可の無い(消えた)virtual-keyboard からの要求は、捨てる。
            return;
        }
        if let zwp_virtual_keyboard_v1::Request::Key {
            key,
            state: 0,
            time,
        } = request
        {
            if !take_vk_press(state, &resource.id(), key, time) {
                // このオブジェクトが、いまのフォーカスへ「押した」を渡していないキーの「離した」は、渡さない。
                return;
            }
        }
        let note = match request {
            zwp_virtual_keyboard_v1::Request::Key {
                key,
                state: 1,
                time,
            } => Some((Some(key), time)),
            zwp_virtual_keyboard_v1::Request::Modifiers { .. } => Some((None, 0)),
            _ => None,
        };
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<D>,
            D,
        >>::request(state, client, resource, request, data, dhandle, data_init);
        if let Some((key, time)) = note {
            note_vk_input(state, client.id(), resource.id(), key, time);
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
        >>::destroyed(state, client.clone(), resource, data);
        // destroy(と接続断)で、このオブジェクトが押したままのものを片付ける。
        release_vk_input(state, &resource.id());
        let ime = &mut state.frontend_mut().ime;
        if ime.vk_generation.remove(&resource.id()).is_none() {
            // 最初の要求を送らずに destroy された:待ち行列から 1 つ外す。
            if let Some(pending) = ime.vk_pending.get_mut(&client) {
                pending.forget_one();
            }
        }
        // 待っているものが無くなったクライアントの記録は消す(接続が切れたら、全部の virtual-keyboard が
        // destroy されるので、記録は残らない)。
        if ime.vk_pending.get(&client).is_some_and(|p| p.len() == 0) {
            ime.vk_pending.remove(&client);
        }
        if let Some(keyboards) = ime.vk_clients.get_mut(&client) {
            keyboards.live = keyboards.live.saturating_sub(1);
            if keyboards.live == 0 {
                ime.vk_clients.remove(&client);
            }
        }
    }
}

/// virtual-keyboard `object`(クライアント `client` のもの)が、いま認められているか。作ったときの世代が、いま
/// 認められた IME の世代と同じときだけ。最初の要求で、作ったときの世代を決める(待ち行列の先頭)。
///
/// 世代を見分けられないとき(前の世代のものと今の世代のものが両方待っているとき)は、IME の接続を
/// `unauthorized` のエラーで切り、つなぎ直しを求める。
fn vk_authorized<D: FrontendHost>(state: &mut D, client: &Client, object: &ObjectId) -> bool {
    let id = client.id();
    let ime = &mut state.frontend_mut().ime;
    let generation = match ime.vk_generation.get(object) {
        Some(&generation) => Some(generation),
        None => {
            let taken = ime
                .vk_pending
                .get_mut(&id)
                .map_or(Taken::Refused, PendingKeyboards::take);
            let generation = match taken {
                Taken::Generation(generation) => Some(generation),
                Taken::Refused => None,
                Taken::Ambiguous => {
                    if let Some(keyboards) = ime.vk_clients.get(&id) {
                        keyboards.manager.post_error(
                            zwp_virtual_keyboard_manager_v1::Error::Unauthorized,
                            "cannot tell which input method a virtual keyboard was made for; reconnect",
                        );
                    }
                    warn!(
                        "a virtual keyboard was refused (it cannot be told whether it was made for the past or the current input method); the input method is disconnected and has to reconnect"
                    );
                    None
                }
            };
            // 認めないものも、0(どの世代とも違う)で覚え、次からは待ち行列を見ない。
            ime.vk_generation
                .insert(object.clone(), generation.unwrap_or(0));
            generation
        }
    };
    generation.is_some() && generation == ime.authorized(&id)
}

/// いまのキーボードのフォーカス。
fn current_focus<D: FrontendHost>(state: &D) -> Option<WlSurface> {
    state.frontend().keyboard.as_ref()?.current_focus()
}

/// virtual-keyboard `object` が、キー `key` の「押した」(`key` が None なら `modifiers`)を、いまのフォーカスへ
/// 渡した後に呼ぶ。フォーカスが無ければ、何も渡していないので記録しない。
fn note_vk_input<D: FrontendHost>(
    state: &mut D,
    client: ClientId,
    object: ObjectId,
    key: Option<u32>,
    time: u32,
) {
    let Some(focus) = current_focus(state) else {
        return;
    };
    let ime = &mut state.frontend_mut().ime;
    let target = Some((focus, ime.focus_generation));
    let input = ime.vk_inputs.entry(object).or_insert_with(|| VkInput {
        client,
        keys: HashSet::new(),
        modifiers: false,
        target: None,
        time: 0,
    });
    if input.target != target {
        // フォーカスが動いた後:前の先へ渡したものは、leave で消えている。
        input.keys.clear();
        input.modifiers = false;
        input.target = target;
    }
    match key {
        Some(key) => {
            input.keys.insert(key);
            input.time = time;
        }
        None => input.modifiers = true,
    }
}

/// virtual-keyboard `object` から、キー `key` の「離した」が届いた。このオブジェクトが、いまのフォーカスへ
/// 「押した」を渡したキーなら、記録から外してtrue(「離した」を渡す)。
fn take_vk_press<D: FrontendHost>(state: &mut D, object: &ObjectId, key: u32, time: u32) -> bool {
    let focus = current_focus(state);
    let ime = &mut state.frontend_mut().ime;
    let generation = ime.focus_generation;
    let Some(input) = ime.vk_inputs.get_mut(object) else {
        return false;
    };
    let current = matches!((&input.target, &focus), (Some((target, g)), Some(focus)) if target == focus && *g == generation);
    if !current || !input.keys.remove(&key) {
        return false;
    }
    input.time = time;
    true
}

/// virtual-keyboard `object` が押したままのもの(キーと modifiers)を片付ける(destroy・接続断・認可の取り消し・
/// 緊急解除のとき)。記録を取り出してから片付けるので、2 回呼ばれても 2 回目は何もしない。
///
/// 渡した先が、いまもフォーカスを持っている(フォーカスの世代も同じ)ときだけ、その先のクライアントの
/// `wl_keyboard` へ、キーごとの「離した」を直に送り、modifiers を物理のキーボードの状態に戻す。Seinas の
/// ショートカットの処理(キーの filter)は通らない。次のキーの「離した」は送らない:物理のキーボードが押して
/// いるキー、ほかの virtual-keyboard が同じ先へ押しているキー。フォーカスが動いていれば、アプリの側の押下は
/// leave で消えているので、何も送らない(新しいフォーカスのアプリへ、誤った「離した」を送らない)。
fn release_vk_input<D: FrontendHost>(state: &mut D, object: &ObjectId) {
    let Some(input) = state.frontend_mut().ime.vk_inputs.remove(object) else {
        return;
    };
    let Some(keyboard) = state.frontend().keyboard.clone() else {
        return;
    };
    let Some((target, generation)) = input.target else {
        return;
    };
    let ime = &state.frontend().ime;
    if generation != ime.focus_generation || keyboard.current_focus().as_ref() != Some(&target) {
        return;
    }
    let Some(client) = target.client() else {
        return;
    };
    let same_target =
        |other: &VkInput| other.target.as_ref() == Some(&(target.clone(), generation));
    let physical = keyboard.pressed_keys();
    let keys: Vec<u32> = input
        .keys
        .iter()
        .copied()
        .filter(|&key| !physical.contains(&Keycode::new(key + 8)))
        .filter(|key| {
            !ime.vk_inputs
                .values()
                .any(|other| same_target(other) && other.keys.contains(key))
        })
        .collect();
    // modifiers は、ほかの virtual-keyboard が渡していても、物理のキーボードの今の状態へ戻す(IME は、grab で
    // 届いた物理の modifiers を、自分の virtual-keyboard で戻すのがふつうで、その値も物理と同じ)。
    let restore_modifiers = input.modifiers;
    if keys.is_empty() && !restore_modifiers {
        return;
    }
    let modifiers = keyboard.modifier_state().serialized;
    for kbd in keyboard.client_keyboards(&client) {
        for &key in &keys {
            kbd.key(
                SERIAL_COUNTER.next_serial().into(),
                input.time,
                key,
                WlKeyState::Released,
            );
        }
        if restore_modifiers {
            kbd.modifiers(
                SERIAL_COUNTER.next_serial().into(),
                modifiers.depressed,
                modifiers.latched,
                modifiers.locked,
                modifiers.layout_effective,
            );
        }
    }
    info!(
        "released {} key(s){} left pressed by a virtual keyboard",
        keys.len(),
        if restore_modifiers {
            " and the modifiers"
        } else {
            ""
        }
    );
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
        // 新しいオブジェクトは作らないといけないので、同じ入れ物で作る(キーも候補ウィンドウも、届かないし出ない)。
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
            let pending = ime.vk_pending.entry(client.id()).or_default();
            if pending.len() >= MAX_PENDING_KEYBOARDS {
                data_init.init(id, RefusedVirtualKeyboard);
                resource.post_error(
                    zwp_virtual_keyboard_manager_v1::Error::Unauthorized,
                    "too many virtual keyboards wait for their first request",
                );
                warn!("a virtual keyboard was refused (too many wait for their first request)");
                return;
            }
            if ime
                .vk_clients
                .get(&client.id())
                .is_some_and(|keyboards| keyboards.live >= MAX_KEYBOARDS)
            {
                data_init.init(id, RefusedVirtualKeyboard);
                resource.post_error(
                    zwp_virtual_keyboard_manager_v1::Error::Unauthorized,
                    "too many virtual keyboards are alive on this connection",
                );
                warn!("a virtual keyboard was refused (too many are alive on the connection)");
                return;
            }
            let pending = ime.vk_pending.entry(client.id()).or_default();
            pending.queue.push_back(generation);
            let keyboards = ime
                .vk_clients
                .entry(client.id())
                .or_insert_with(|| ClientKeyboards {
                    live: 0,
                    manager: resource.clone(),
                });
            keyboards.live += 1;
            keyboards.manager = resource.clone();
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

#[cfg(test)]
mod tests {
    use super::{PendingKeyboards, Taken};

    fn pending(queue: &[u64], stale: usize) -> PendingKeyboards {
        PendingKeyboards {
            queue: queue.iter().copied().collect(),
            stale,
        }
    }

    #[test]
    fn keyboards_of_one_generation_get_it_in_any_order() {
        let mut p = pending(&[3, 3], 0);
        assert_eq!(p.take(), Taken::Generation(3));
        assert_eq!(p.take(), Taken::Generation(3));
        assert_eq!(p.take(), Taken::Refused);
    }

    #[test]
    fn a_keyboard_of_a_past_generation_is_not_taken_for_a_new_one() {
        // 前の世代で作ったもの(stale)だけが残っていれば、認めない。
        let mut p = pending(&[1], 0);
        p.retire();
        assert_eq!(p.take(), Taken::Refused);
        assert_eq!(p.len(), 0);
        // 前の世代と今の世代が混ざっていれば、見分けられない(IME の接続を切る)。残りも全部認めない。
        let mut p = pending(&[1], 0);
        p.retire();
        p.queue.push_back(2);
        assert_eq!(p.take(), Taken::Ambiguous);
        assert_eq!(p.take(), Taken::Refused);
        assert_eq!(p.len(), 0);
        // 混ざりが無くなった後に作ったものは、認める。
        p.queue.push_back(2);
        assert_eq!(p.take(), Taken::Generation(2));
    }

    #[test]
    fn a_keyboard_destroyed_before_its_first_request_leaves_the_queue() {
        let mut p = pending(&[4, 4], 0);
        p.forget_one();
        assert_eq!(p.len(), 1);
        assert_eq!(p.take(), Taken::Generation(4));
        let mut p = pending(&[], 2);
        p.forget_one();
        assert_eq!(p.len(), 1);
        let mut p = pending(&[5], 1);
        p.forget_one();
        assert_eq!((p.queue.len(), p.stale), (0, 1));
    }
}
