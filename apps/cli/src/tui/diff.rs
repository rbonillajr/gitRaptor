//! The diff view's state (US-CKP-012): opened with `d` on the cursor's row, asked from the
//! engine once (a photo, D9) and closed with `Esc` or `d`. Pure: the asking and the answer
//! travel as [`Cmd`] and [`EngineMsg::Diff`].

use gitraptor_api::diff::WorktreeDiff;

use crate::model::{Cmd, DiffFailure, Model, Notice, WorktreeState};
use crate::present::SafeText;
use crate::present::diff::{DiffRow, rows};
use crate::tui::selection;

/// What the full-screen view holds while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffState {
    /// Asked, no answer yet.
    Loading { ticket: u64, worktree: SafeText },
    /// The engine's answer as rows of sanitized text (the response itself is not kept),
    /// scrolled to `offset` rows. `at_utc_ms` is when the engine computed it.
    Ready {
        worktree: SafeText,
        at_utc_ms: i64,
        lines: Vec<DiffRow>,
        offset: usize,
    },
    /// The answer did not come: the view says why.
    Failed {
        worktree: SafeText,
        failure: DiffFailure,
    },
}

/// `d` on the row under the cursor: asks for its diff, or says why it does not. The state of
/// the agent's session does not matter (BR-CKP-ELIG-006); an unreadable worktree, or an engine
/// without `diff.worktree`, asks nothing.
pub fn open(model: &mut Model) -> Vec<Cmd> {
    let Some(row) = selection::selected_row(model) else {
        model.ui.notice = Some(Notice::UnknownKey);
        return Vec::new();
    };
    if let WorktreeState::Unavailable(reason) = row.state {
        model.ui.notice = Some(Notice::RowActionDisabled(reason));
        return Vec::new();
    }
    if !model.engine.diff {
        model.ui.notice = Some(Notice::DiffUnsupported);
        return Vec::new();
    }
    let Some(repo_id) = model
        .engine
        .repo
        .as_ref()
        .and_then(|r| r.data.as_ref())
        .map(|r| r.repo_id.clone())
    else {
        model.ui.notice = Some(Notice::UnknownKey);
        return Vec::new();
    };
    let (worktree, root) = (row.name.clone(), row.root.clone());
    let mut cmds = close(model);
    let ticket = model.ui.next_ticket;
    model.ui.next_ticket += 1;
    model.ui.diff = Some(DiffState::Loading { ticket, worktree });
    cmds.push(Cmd::Diff {
        ticket,
        repo_id,
        root,
    });
    cmds
}

/// Closes the view; a query still running is cancelled.
pub fn close(model: &mut Model) -> Vec<Cmd> {
    match model.ui.diff.take() {
        Some(DiffState::Loading { ticket, .. }) => vec![Cmd::CancelQuery { ticket }],
        _ => Vec::new(),
    }
}

/// The answer of ticket `ticket`. One that is not the current query's (an older one) is
/// dropped. A cancelled one is not shown: the view was already closed.
pub fn apply_reply(
    model: &mut Model,
    ticket: u64,
    result: Result<Box<WorktreeDiff>, crate::model::DiffFailure>,
) {
    let Some(DiffState::Loading {
        ticket: current,
        worktree,
    }) = &model.ui.diff
    else {
        return;
    };
    if *current != ticket {
        return;
    }
    let worktree = worktree.clone();
    model.ui.diff = match result {
        Ok(diff) => Some(DiffState::Ready {
            worktree,
            at_utc_ms: diff.computed_at_utc_ms,
            lines: rows(&diff),
            offset: 0,
        }),
        Err(DiffFailure::Cancelled) => None,
        Err(failure) => Some(DiffState::Failed { worktree, failure }),
    };
    model.dirty = true;
}

/// Scrolls the open view one row, never past its last row.
pub fn scroll(model: &mut Model, down: bool) {
    if let Some(DiffState::Ready { lines, offset, .. }) = &mut model.ui.diff {
        let last = lines.len().saturating_sub(1);
        *offset = if down {
            (*offset + 1).min(last)
        } else {
            offset.saturating_sub(1)
        };
    }
}

#[cfg(test)]
mod tests {
    use gitraptor_api::diff::{CommittedDiff, CommittedGap, DiffSection};

    use super::*;
    use crate::model::{Head, WorktreeState};
    use crate::tui::selection::tests::{model_with, row};

    fn empty_diff() -> Box<WorktreeDiff> {
        Box::new(WorktreeDiff {
            computed_at_utc_ms: 0,
            committed: CommittedDiff::NotComputable {
                reason: CommittedGap::NoBase,
            },
            uncommitted: DiffSection {
                files: Vec::new(),
                files_total: 0,
                cut: None,
            },
        })
    }

    fn ready_model() -> Model {
        let mut w = row("a", 1, false);
        w.state = WorktreeState::Ready {
            head: Head::Detached(None),
            changes: 0,
            divergence: gitraptor_api::messages::DivergenceView::NoBase,
        };
        let mut model = model_with(vec![w]);
        model.engine.diff = true;
        model
    }

    fn asked(cmds: &[Cmd]) -> u64 {
        match cmds.last() {
            Some(Cmd::Diff { ticket, .. }) => *ticket,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_reply_for_an_old_ticket_is_dropped() {
        let mut model = ready_model();
        let first = asked(&open(&mut model));
        assert_eq!(close(&mut model), vec![Cmd::CancelQuery { ticket: first }]);
        let second = asked(&open(&mut model));
        assert_ne!(first, second);
        apply_reply(&mut model, first, Ok(empty_diff()));
        assert!(
            matches!(model.ui.diff, Some(DiffState::Loading { ticket, .. }) if ticket == second)
        );
        apply_reply(&mut model, second, Ok(empty_diff()));
        assert!(matches!(model.ui.diff, Some(DiffState::Ready { .. })));
    }

    #[test]
    fn closing_while_loading_cancels() {
        let mut model = ready_model();
        let ticket = asked(&open(&mut model));
        assert_eq!(close(&mut model), vec![Cmd::CancelQuery { ticket }]);
        assert!(model.ui.diff.is_none());
        // The cancelled answer arrives after: nothing to show.
        apply_reply(&mut model, ticket, Err(DiffFailure::Cancelled));
        assert!(model.ui.diff.is_none());
        // A ready view closes with nothing to cancel.
        let ticket = asked(&open(&mut model));
        apply_reply(&mut model, ticket, Ok(empty_diff()));
        assert!(close(&mut model).is_empty());
    }

    #[test]
    fn a_failure_leaves_the_view_with_its_reason_and_d_asks_nothing_when_it_cannot() {
        let mut model = ready_model();
        let ticket = asked(&open(&mut model));
        apply_reply(&mut model, ticket, Err(DiffFailure::Busy));
        assert!(matches!(
            model.ui.diff,
            Some(DiffState::Failed {
                failure: DiffFailure::Busy,
                ..
            })
        ));
        model.ui.diff = None;
        model.engine.diff = false;
        assert!(open(&mut model).is_empty());
        assert_eq!(model.ui.notice, Some(Notice::DiffUnsupported));
    }
}
