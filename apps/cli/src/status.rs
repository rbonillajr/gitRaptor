//! `raptor status` output, text and JSON (US-GRP-001). The Cockpit TUI
//! (F-001-02) owns the final presentation; this one is plain and complete.
//!
//! Every text that comes from the engine is untrusted (SEC-12): the text
//! output prints it sanitized; the JSON output carries it as JSON strings,
//! whose escaping keeps terminal controls inert.

use std::fmt::Write as _;
use std::path::Path;

use gitraptor_api::Untrusted;
use gitraptor_api::messages::{
    ChangeAreaView, ChangeCounts, FileChangeView, HeadView, RepoStateView, RepoView, Snapshot,
    UnavailableReason, WorktreeStatus, WorktreeView,
};
use serde::Serialize;

use crate::i18n::t;

/// The kebab-case wire text of a contract enum.
pub fn wire(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Folder a repo is shown by: the main worktree root when the common
/// directory is its `.git`, else the common directory (a bare repo).
fn repo_folder(repo: &RepoView) -> String {
    let common = Path::new(repo.path.raw());
    match (common.file_name(), common.parent()) {
        (Some(name), Some(parent)) if name == ".git" => parent.display().to_string(),
        _ => repo.path.raw().to_owned(),
    }
}

/// The observed repo that `path` names: its Git directory, the folder
/// shown for it or any of its worktrees.
pub fn repo_at(snapshot: &Snapshot, path: &Path) -> Option<String> {
    snapshot
        .repos
        .iter()
        .find(|repo| {
            Path::new(repo.path.raw()) == path
                || Path::new(&repo_folder(repo)) == path
                || repo
                    .worktrees
                    .iter()
                    .any(|w| Path::new(w.path.raw()) == path)
        })
        .map(|repo| repo.repo_id.clone())
}

/// The text output, one line per repo, worktree and listed change.
pub fn text(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    if snapshot.repos.is_empty() {
        let _ = writeln!(out, "{}", t("status.none", &[]));
    }
    for repo in &snapshot.repos {
        let folder = sanitize(&repo_folder(repo));
        let key = match repo.state {
            RepoStateView::Observed => "status.repo",
            RepoStateView::Unavailable => "status.repo-unavailable",
        };
        let _ = writeln!(out, "{}", t(key, &[("path", &folder)]));
        for w in &repo.worktrees {
            worktree_text(&mut out, w);
        }
    }
    out
}

fn sanitize(text: &str) -> String {
    Untrusted::new(text).sanitized()
}

fn worktree_text(out: &mut String, w: &WorktreeView) {
    let key = if w.main {
        "status.worktree-main"
    } else {
        "status.worktree-linked"
    };
    let _ = writeln!(out, "  {}", t(key, &[("path", &w.path.sanitized())]));
    match &w.status {
        WorktreeStatus::Unavailable { reason } => {
            let key = match reason {
                UnavailableReason::Missing => "status.wt-missing",
                UnavailableReason::Untrusted => "status.wt-untrusted",
                UnavailableReason::Unreadable => "status.wt-unreadable",
            };
            let _ = writeln!(out, "    {}", t(key, &[]));
        }
        WorktreeStatus::Ready {
            head,
            counts,
            changes,
        } => {
            let _ = writeln!(out, "    {}", head_text(head));
            if counts.is_clean() {
                let _ = writeln!(out, "    {}", t("status.clean", &[]));
                return;
            }
            let _ = writeln!(out, "    {}", counts_text(counts));
            for c in changes {
                let _ = writeln!(out, "      {}", change_text(c));
            }
            let more = counts.total().saturating_sub(changes.len() as u64);
            if changes.is_empty() {
                let _ = writeln!(out, "      {}", t("status.lists-dropped", &[]));
            } else if more > 0 {
                let _ = writeln!(out, "      {}", t("status.truncated", &[("more", &more)]));
            }
        }
    }
}

fn head_text(head: &HeadView) -> String {
    match head {
        HeadView::Branch { name } => t("status.branch", &[("name", &name.sanitized())]),
        HeadView::Unborn { name } => t("status.unborn", &[("name", &name.sanitized())]),
        HeadView::Detached => t("status.detached", &[]),
    }
}

fn counts_text(counts: &ChangeCounts) -> String {
    t(
        "status.changes",
        &[
            ("total", &counts.total()),
            ("staged", &counts.staged),
            ("unstaged", &counts.unstaged),
            ("untracked", &counts.untracked),
        ],
    )
}

fn change_text(c: &FileChangeView) -> String {
    let path = c.path.sanitized();
    let kind = t(&format!("kind.{}", wire(&c.kind)), &[]);
    match c.area {
        ChangeAreaView::Staged => t("status.change-staged", &[("kind", &kind), ("path", &path)]),
        ChangeAreaView::Unstaged => t(
            "status.change-unstaged",
            &[("kind", &kind), ("path", &path)],
        ),
        ChangeAreaView::Untracked => t("status.change-untracked", &[("path", &path)]),
    }
}

/// The JSON output: a stable shape for scripts, plain strings instead of
/// the channel's `{"untrusted": …}` wrappers.
#[derive(Serialize)]
pub struct StatusJson {
    engine: String,
    repos: Vec<RepoJson>,
}

#[derive(Serialize)]
struct RepoJson {
    repo_id: String,
    path: String,
    git_dir: String,
    state: String,
    worktrees: Vec<WorktreeJson>,
}

#[derive(Serialize)]
struct WorktreeJson {
    path: String,
    main: bool,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<String>,
    /// `branch`, `unborn` or `detached`.
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    counts: Option<ChangeCounts>,
    changes: Vec<ChangeJson>,
    changes_truncated: bool,
}

#[derive(Serialize)]
struct ChangeJson {
    path: String,
    area: String,
    kind: String,
}

pub fn json(snapshot: &Snapshot) -> StatusJson {
    StatusJson {
        engine: wire(&snapshot.engine.state),
        repos: snapshot
            .repos
            .iter()
            .map(|repo| RepoJson {
                repo_id: repo.repo_id.clone(),
                path: repo_folder(repo),
                git_dir: repo.path.raw().to_owned(),
                state: wire(&repo.state),
                worktrees: repo.worktrees.iter().map(worktree_json).collect(),
            })
            .collect(),
    }
}

fn worktree_json(w: &WorktreeView) -> WorktreeJson {
    let mut out = WorktreeJson {
        path: w.path.raw().to_owned(),
        main: w.main,
        state: "unavailable",
        unavailable_reason: None,
        head: None,
        branch: None,
        clean: None,
        counts: None,
        changes: Vec::new(),
        changes_truncated: w.changes_truncated(),
    };
    match &w.status {
        WorktreeStatus::Unavailable { reason } => out.unavailable_reason = Some(wire(reason)),
        WorktreeStatus::Ready {
            head,
            counts,
            changes,
        } => {
            out.state = "ready";
            let (kind, name) = match head {
                HeadView::Branch { name } => ("branch", Some(name)),
                HeadView::Unborn { name } => ("unborn", Some(name)),
                HeadView::Detached => ("detached", None),
            };
            out.head = Some(kind.to_owned());
            out.branch = name.map(|n| n.raw().to_owned());
            out.clean = Some(counts.is_clean());
            out.counts = Some(*counts);
            out.changes = changes
                .iter()
                .map(|c| ChangeJson {
                    path: c.path.raw().to_owned(),
                    area: wire(&c.area),
                    kind: wire(&c.kind),
                })
                .collect();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::messages::{ChangeKindView, DaemonView, EngineStateView, EngineView};

    fn snapshot() -> Snapshot {
        let wt = |path: &str, main, status| WorktreeView {
            path: Untrusted::new(path),
            main,
            admin_name: None,
            status,
        };
        Snapshot {
            run_id: "r".into(),
            seq: 1,
            engine: EngineView {
                state: EngineStateView::Observing,
                git_version: Some("2.50.0".into()),
            },
            daemon: DaemonView {
                pid: 1,
                protocol: 2,
                binary_version: "0".into(),
                started_wall_ms: 0,
            },
            repos: vec![RepoView {
                repo_id: "ab-01".into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/demo/.git"),
                worktrees: vec![
                    wt(
                        "/w/demo",
                        true,
                        WorktreeStatus::Ready {
                            head: HeadView::Branch {
                                name: Untrusted::new("main"),
                            },
                            counts: ChangeCounts::default(),
                            changes: Vec::new(),
                        },
                    ),
                    wt(
                        "/w/feat\u{1b}]52;c;eA==\u{7}",
                        false,
                        WorktreeStatus::Ready {
                            head: HeadView::Branch {
                                name: Untrusted::new("feat-login"),
                            },
                            counts: ChangeCounts {
                                staged: 0,
                                unstaged: 1,
                                untracked: 0,
                            },
                            changes: vec![FileChangeView {
                                path: Untrusted::new("login.txt"),
                                area: ChangeAreaView::Unstaged,
                                kind: ChangeKindView::Modified,
                            }],
                        },
                    ),
                ],
            }],
        }
    }

    #[test]
    fn text_lists_every_worktree_sanitized() {
        let out = text(&snapshot());
        assert!(out.contains("/w/demo"), "{out}");
        assert!(
            out.contains("feat-login") && out.contains("login.txt"),
            "{out}"
        );
        assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
    }

    #[test]
    fn json_has_plain_strings_and_flags() {
        let value = serde_json::to_value(json(&snapshot())).unwrap();
        let wts = &value["repos"][0]["worktrees"];
        assert_eq!(value["repos"][0]["path"], "/w/demo");
        assert_eq!(wts[0]["clean"], true);
        assert_eq!(wts[1]["branch"], "feat-login");
        assert_eq!(wts[1]["changes"][0]["path"], "login.txt");
        assert_eq!(wts[1]["changes"][0]["area"], "unstaged");
    }

    #[test]
    fn a_repo_is_found_by_any_of_its_folders() {
        let s = snapshot();
        for p in ["/w/demo/.git", "/w/demo"] {
            assert_eq!(repo_at(&s, Path::new(p)).as_deref(), Some("ab-01"), "{p}");
        }
        assert_eq!(repo_at(&s, Path::new("/w/other")), None);
    }
}
