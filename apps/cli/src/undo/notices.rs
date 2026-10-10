//! The notices a worktree has not been told yet: an operation cut half-way when GitRaptor
//! stopped. Shown once, by the next command run from that worktree.

use std::path::Path;

use gitraptor_api::methods;
use gitraptor_api::timemachine::{NoticesResult, TimelineOperationKind};
use gitraptor_api::untrusted::sanitize;
use gitraptor_core::client::Client;
use serde_json::json;

use crate::i18n::t;
use crate::offers;

/// Shows the pending notices of `worktree` on stderr, prefixed by `command`. Silent when the
/// daemon does not offer `timemachine.notices` or the call fails: it never stops the command.
pub(crate) fn show_pending(client: &mut Client, worktree: &Path, command: &str) {
    if !offers(client, methods::TM_NOTICES) {
        return;
    }
    let answer: Result<NoticesResult, _> = client.call(
        methods::TM_NOTICES,
        json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" }),
    );
    let Ok(answer) = answer else {
        return;
    };
    for notice in answer.notices {
        // The id comes from the oplog: text of the repo's history, never written raw.
        let id = sanitize(notice.operation_id.as_deref().unwrap_or(&notice.notice_id));
        eprintln!(
            "{command}: {}",
            t(key(notice.operation_kind), &[("id", &id)])
        );
    }
}

fn key(kind: Option<TimelineOperationKind>) -> &'static str {
    match kind {
        Some(TimelineOperationKind::Undo) => "tm-notice.interrupted-undo",
        Some(TimelineOperationKind::Restore) => "tm-notice.interrupted-restore",
        Some(TimelineOperationKind::Redo) => "tm-notice.interrupted-redo",
        Some(TimelineOperationKind::Protected) | None => "tm-notice.interrupted-operation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;

    /// Every kind has its message in both languages, and each one names the way back.
    #[test]
    fn every_notice_has_its_message() {
        let kinds = [
            Some(TimelineOperationKind::Undo),
            Some(TimelineOperationKind::Restore),
            Some(TimelineOperationKind::Redo),
            Some(TimelineOperationKind::Protected),
            None,
        ];
        let keys: std::collections::HashSet<_> = kinds.into_iter().map(key).collect();
        assert_eq!(keys.len(), 4);
        for key in keys {
            for spanish in [false, true] {
                assert!(i18n::has_key(key), "{key}");
                let text = i18n::text_in(spanish, key).unwrap_or_default();
                assert!(text.contains("{id}"), "{key}: {text}");
                assert!(text.contains("raptor undo"), "{key}: {text}");
            }
        }
    }
}
