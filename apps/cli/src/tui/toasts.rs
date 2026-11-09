//! The stack of toasts the cockpit shows: at most three, each alive for five seconds. The
//! widget (`widgets::notification`) only paints; the expiry lives here, driven by `Msg::Tick`.
//!
//! A toast keeps ids, never text: the view composes the words when it paints, from the same
//! names the panel uses, so a toast cannot say what the panel does not.

use std::collections::VecDeque;

/// How long a toast stays, in milliseconds.
pub const TOAST_TTL_MS: i64 = 5_000;
/// The most toasts at once; the oldest leaves first.
pub const MAX_TOASTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastBody {
    /// A predicted conflict that just appeared; `file` says whether its first file is named.
    Conflict {
        repo_id: String,
        pair_id: String,
        file: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub body: ToastBody,
    /// `None` until the first tick when the clock was not set yet.
    pub until_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Toasts {
    stack: VecDeque<Toast>,
}

impl Toasts {
    /// Shows a toast, dropping the oldest when full. One already alive for the same body
    /// is not shown twice.
    pub fn push(&mut self, body: ToastBody, now_ms: i64) {
        let same = |t: &Toast| match (&t.body, &body) {
            (
                ToastBody::Conflict {
                    repo_id: r,
                    pair_id: p,
                    ..
                },
                ToastBody::Conflict {
                    repo_id: r2,
                    pair_id: p2,
                    ..
                },
            ) => r == r2 && p == p2,
        };
        if self.stack.iter().any(same) {
            return;
        }
        if self.stack.len() == MAX_TOASTS {
            self.stack.pop_front();
        }
        self.stack.push_back(Toast {
            body,
            until_ms: (now_ms > 0).then(|| now_ms.saturating_add(TOAST_TTL_MS)),
        });
    }

    /// Moves the clock: fixes the missing expiries and drops the expired toasts and those
    /// `alive` rejects. Returns whether the stack changed.
    pub fn tick(&mut self, now_ms: i64, alive: impl Fn(&ToastBody) -> bool) -> bool {
        for t in &mut self.stack {
            t.until_ms
                .get_or_insert(now_ms.saturating_add(TOAST_TTL_MS));
        }
        let before = self.stack.len();
        self.stack
            .retain(|t| t.until_ms.is_some_and(|u| now_ms < u) && alive(&t.body));
        self.stack.len() != before
    }

    /// Oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.stack.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: &str) -> ToastBody {
        ToastBody::Conflict {
            repo_id: "r".into(),
            pair_id: id.into(),
            file: true,
        }
    }

    #[test]
    fn the_oldest_leaves_first_and_a_live_one_is_not_repeated() {
        let mut t = Toasts::default();
        for id in ["a", "b", "c", "c", "d"] {
            t.push(body(id), 10);
        }
        let ids: Vec<_> = t
            .iter()
            .map(|x| match &x.body {
                ToastBody::Conflict { pair_id, .. } => pair_id.clone(),
            })
            .collect();
        assert_eq!(ids, ["b", "c", "d"]);
    }

    #[test]
    fn it_expires_at_five_seconds_and_fixes_a_missing_expiry_on_the_first_tick() {
        let mut t = Toasts::default();
        t.push(body("a"), 1_000);
        assert!(!t.tick(5_999, |_| true));
        assert!(t.tick(6_000, |_| true));
        t.push(body("b"), 0);
        assert_eq!(t.iter().next().unwrap().until_ms, None);
        assert!(!t.tick(100, |_| true));
        assert_eq!(t.iter().next().unwrap().until_ms, Some(5_100));
        assert!(t.tick(101, |_| false));
    }
}
