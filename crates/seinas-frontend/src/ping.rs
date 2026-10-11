//! 閉じるように頼んだクライアントへの ping と、応答なしの見分け。
//!
//! 閉じるように頼む(xdg_toplevel の close)ときに、そのクライアントへ xdg_wm_base の ping も送り、決まった時間
//! (既定は [`DEFAULT_PING_TIMEOUT`])の中に pong が返らなければ、応答なしとする。close に応じなくても、pong を
//! 返すクライアント(保存するかを尋ねているアプリなど)は、応答なしにしない。応答なしにした後でも、pong が
//! 返れば外す。決まりの説明は `docs/window-placement.md` の「閉じる」にある。
//!
//! Wayland のオブジェクトに触れずに確かめられるよう、クライアントの型 `C` を決めずに書いてある。

use std::time::{Duration, Instant};

/// pong を待つ時間の既定。Windows が、メッセージを処理しないウィンドウを「応答なし」とするまでの時間(5 秒)に
/// 倣う。
pub const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(5);

/// pong を待っているクライアント。
#[derive(Debug)]
struct Waiting<C> {
    client: C,
    /// この時刻までに pong が返らなければ、応答なしにする。
    deadline: Instant,
    /// 応答なしにしたか。
    unresponsive: bool,
}

/// pong を待っているクライアントと、応答なしにしたクライアントの一覧。
#[derive(Debug)]
pub struct Pings<C> {
    timeout: Duration,
    waiting: Vec<Waiting<C>>,
}

impl<C: PartialEq> Default for Pings<C> {
    fn default() -> Self {
        Self::new(DEFAULT_PING_TIMEOUT)
    }
}

impl<C: PartialEq> Pings<C> {
    /// pong を `timeout` だけ待つ一覧。
    pub fn new(timeout: Duration) -> Self {
        Pings {
            timeout,
            waiting: Vec::new(),
        }
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// `client` の答えを、もう待っているか(ping を送り直さなくてよいか)。
    pub fn is_waiting(&self, client: &C) -> bool {
        self.waiting.iter().any(|waiting| waiting.client == *client)
    }

    /// 時刻 `now` に `client` へ ping を送った。すでに待っていれば、何もしない(期限も印も、そのまま)。
    pub fn sent(&mut self, client: C, now: Instant) {
        if self.is_waiting(&client) {
            return;
        }
        self.waiting.push(Waiting {
            client,
            deadline: now + self.timeout,
            unresponsive: false,
        });
    }

    /// `client` から pong が返った。待つのを止める。応答なしの印を外したらtrue。
    pub fn ponged(&mut self, client: &C) -> bool {
        let Some(index) = self.waiting.iter().position(|w| w.client == *client) else {
            return false;
        };
        self.waiting.remove(index).unresponsive
    }

    /// 時刻 `now` で期限を過ぎたクライアントに、応答なしの印を付ける。新しく付けたらtrue。
    pub fn expire(&mut self, now: Instant) -> bool {
        let mut marked = false;
        for waiting in &mut self.waiting {
            if !waiting.unresponsive && waiting.deadline <= now {
                waiting.unresponsive = true;
                marked = true;
            }
        }
        marked
    }

    /// 次に期限が来る時刻(まだ印を付けていないものの中で)。待っているものが無ければNone。
    pub fn next_deadline(&self) -> Option<Instant> {
        self.waiting
            .iter()
            .filter(|waiting| !waiting.unresponsive)
            .map(|waiting| waiting.deadline)
            .min()
    }

    /// `client` に応答なしの印が付いているか。
    pub fn is_unresponsive(&self, client: &C) -> bool {
        self.waiting
            .iter()
            .any(|waiting| waiting.unresponsive && waiting.client == *client)
    }

    /// `is_gone` が真のクライアント(切れたものなど)を、一覧から外す。
    pub fn forget(&mut self, is_gone: impl Fn(&C) -> bool) {
        self.waiting.retain(|waiting| !is_gone(&waiting.client));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_without_a_pong_in_time_is_marked_and_a_late_pong_clears_it() {
        let start = Instant::now();
        let mut pings = Pings::new(Duration::from_secs(5));
        pings.sent(1, start);
        assert_eq!(pings.next_deadline(), Some(start + Duration::from_secs(5)));
        // 期限の前には、印を付けない。
        assert!(!pings.expire(start + Duration::from_millis(4999)));
        assert!(!pings.is_unresponsive(&1));
        // 期限が来たら、付ける。付けた後は、次の期限に入れない。
        assert!(pings.expire(start + Duration::from_secs(5)));
        assert!(pings.is_unresponsive(&1));
        assert!(
            !pings.expire(start + Duration::from_secs(6)),
            "already marked"
        );
        assert_eq!(pings.next_deadline(), None);
        // 送り直しても、印はそのまま。
        pings.sent(1, start + Duration::from_secs(7));
        assert!(pings.is_unresponsive(&1));
        // 遅れて pong が返れば、外す。
        assert!(pings.ponged(&1));
        assert!(!pings.is_unresponsive(&1));
        assert!(!pings.is_waiting(&1));
    }

    #[test]
    fn a_pong_in_time_leaves_no_mark() {
        let start = Instant::now();
        let mut pings = Pings::new(Duration::from_secs(5));
        pings.sent(1, start);
        pings.sent(2, start + Duration::from_secs(1));
        assert!(!pings.ponged(&1), "it was not marked");
        assert!(!pings.expire(start + Duration::from_secs(5)));
        assert!(!pings.is_unresponsive(&1));
        assert_eq!(pings.next_deadline(), Some(start + Duration::from_secs(6)));
        // 知らない pong と、切れたクライアントは、何も起こさない。
        assert!(!pings.ponged(&3));
        pings.forget(|client| *client == 2);
        assert_eq!(pings.next_deadline(), None);
    }
}
