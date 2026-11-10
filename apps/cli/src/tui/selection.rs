//! The row cursor of the fleet (US-CKP-012, D10): it holds the worktree's key, not its
//! position, so it follows the worktree when the fleet reorders. Pure.

use gitraptor_api::messages::SessionStateView;

use crate::model::{Model, RepoView, WorktreeRow};
use crate::tui::fleet_order;

/// The worktrees of the selected repo in the order the fleet paints them. It repeats the rule
/// of the view (group, then name, path and key), so the cursor and the painted rows agree.
pub fn ordered_rows(model: &Model) -> Vec<&WorktreeRow> {
    let Some(repo) = model.engine.repo.as_ref().and_then(|r| r.data.as_ref()) else {
        return Vec::new();
    };
    ordered(repo, model.now_ms, model.ui.show_ended)
}

fn ordered(repo: &RepoView, now_ms: i64, show_ended: bool) -> Vec<&WorktreeRow> {
    let mut rows: Vec<(fleet_order::Group, &WorktreeRow)> = repo
        .worktrees
        .iter()
        .map(|w| {
            let shown = fleet_order::shown(&repo.sessions, w.key, now_ms, show_ended);
            let lead: Option<SessionStateView> = shown
                .sessions
                .first()
                .filter(|_| repo.detection == Some(true))
                .map(|s| s.state);
            let conflict = repo
                .prediction
                .as_ref()
                .is_some_and(|p| p.known_conflict(w.key));
            (fleet_order::group(w.main, conflict, lead), w)
        })
        .collect();
    rows.sort_by(|a, b| fleet_order::order_key(a.0, a.1).cmp(&fleet_order::order_key(b.0, b.1)));
    rows.into_iter().map(|(_, w)| w).collect()
}

/// The row the keys act on: the cursor's worktree, or the first row while the cursor has not
/// moved or its worktree is gone.
pub fn selected_row(model: &Model) -> Option<&WorktreeRow> {
    let rows = ordered_rows(model);
    model
        .ui
        .cursor
        .and_then(|key| rows.iter().copied().find(|w| w.key == key))
        .or_else(|| rows.first().copied())
}

/// Moves the cursor one row without wrapping; with no rows it does nothing.
pub fn move_cursor(model: &mut Model, down: bool) {
    let rows = ordered_rows(model);
    let Some(last) = rows.len().checked_sub(1) else {
        return;
    };
    let current = model
        .ui
        .cursor
        .and_then(|key| rows.iter().position(|w| w.key == key))
        .unwrap_or(0);
    let next = if down {
        (current + 1).min(last)
    } else {
        current.saturating_sub(1)
    };
    let key = rows[next].key;
    model.ui.cursor = Some(key);
}

#[cfg(test)]
pub(crate) mod tests {
    use gitraptor_api::messages::UnavailableReason;

    use super::*;
    use crate::model::{ScopeReplica, Size, WorktreeState};
    use crate::present::SafeText;
    use crate::present::i18n::Lang;

    pub(crate) fn row(name: &str, key: u64, main: bool) -> WorktreeRow {
        WorktreeRow {
            path: SafeText::text(&format!("/w/{name}")),
            name: SafeText::name(name),
            key,
            main,
            state: WorktreeState::Unavailable(UnavailableReason::Missing),
            last_activity_ms: None,
            temporary: false,
            root: std::path::PathBuf::from(format!("/w/{name}")),
        }
    }

    pub(crate) fn model_with(worktrees: Vec<WorktreeRow>) -> Model {
        let mut model = Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        );
        model.engine.repo = Some(ScopeReplica {
            data: Some(RepoView {
                repo_id: "r1".into(),
                path: SafeText::text("/w"),
                name: SafeText::name("w"),
                base: None,
                worktrees,
                sessions: Vec::new(),
                fetched_ms: None,
                detection: None,
                commits: Vec::new(),
                protection: None,
                prediction: None,
            }),
            ..ScopeReplica::default()
        });
        model
    }

    #[test]
    fn cursor_follows_the_worktree_across_reorders() {
        let mut model = model_with(vec![
            row("a", 1, false),
            row("b", 2, false),
            row("c", 3, false),
        ]);
        assert_eq!(selected_row(&model).map(|w| w.key), Some(1));
        move_cursor(&mut model, true);
        assert_eq!(selected_row(&model).map(|w| w.key), Some(2));
        // `c` becomes the main worktree and goes first: the cursor stays on `b`.
        if let Some(data) = model.engine.repo.as_mut().and_then(|r| r.data.as_mut()) {
            data.worktrees[2].main = true;
        }
        let order: Vec<u64> = ordered_rows(&model).iter().map(|w| w.key).collect();
        assert_eq!(order, [3, 1, 2]);
        assert_eq!(selected_row(&model).map(|w| w.key), Some(2));
        move_cursor(&mut model, true);
        assert_eq!(selected_row(&model).map(|w| w.key), Some(2), "last row");
        move_cursor(&mut model, false);
        assert_eq!(selected_row(&model).map(|w| w.key), Some(1));
    }

    #[test]
    fn a_vanished_worktree_leaves_the_cursor_on_the_first_row() {
        let mut model = model_with(vec![row("a", 1, false), row("b", 2, false)]);
        move_cursor(&mut model, true);
        if let Some(data) = model.engine.repo.as_mut().and_then(|r| r.data.as_mut()) {
            data.worktrees.retain(|w| w.key != 2);
        }
        assert_eq!(selected_row(&model).map(|w| w.key), Some(1));
        let mut empty = model_with(Vec::new());
        move_cursor(&mut empty, true);
        assert!(selected_row(&empty).is_none());
    }
}
