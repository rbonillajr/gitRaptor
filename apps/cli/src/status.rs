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
    BaseBranchView, BaseStatusView, ChangeAreaView, ChangeCounts, CommitCountView, DivergenceView,
    FileChangeView, HeadView, RepoStateView, RepoView, SessionStateView, SessionView, Snapshot,
    UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::timemachine::KeptTempsView;
use serde::Serialize;

use crate::i18n::t;
use crate::sessions::{SessionJson, session_json};

/// The agent sessions `raptor status` shows (US-GRP-007): the present ones
/// and the latest ended one of each worktree.
#[derive(Debug, Default)]
pub struct SessionsInfo {
    /// `None`: the engine does not offer `sessions.list` (an older daemon).
    pub available: Option<bool>,
    pub sessions: Vec<SessionView>,
}

impl SessionsInfo {
    fn of<'a>(
        &'a self,
        repo_id: &'a str,
        w: &'a WorktreeView,
    ) -> impl Iterator<Item = &'a SessionView> {
        self.sessions
            .iter()
            .filter(move |s| s.repo_id == repo_id && s.worktree.raw() == w.path.raw())
    }
}

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
    folder_of(repo.path.raw())
}

/// The folder shown for a repo whose Git common directory is `common`.
pub fn folder_of(common: &str) -> String {
    let path = Path::new(common);
    match (path.file_name(), path.parent()) {
        (Some(name), Some(parent)) if name == ".git" => parent.display().to_string(),
        _ => common.to_owned(),
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

/// The text output, one line per repo, worktree, session and listed
/// change. A worktree without sessions shows none.
pub fn text(snapshot: &Snapshot, sessions: &SessionsInfo) -> String {
    let mut out = String::new();
    if snapshot.repos.is_empty() {
        let _ = writeln!(out, "{}", t("status.none", &[]));
    }
    if sessions.available == Some(false) && !snapshot.repos.is_empty() {
        let _ = writeln!(out, "{}", t("sessions.unavailable", &[]));
    }
    for repo in &snapshot.repos {
        let folder = sanitize(&repo_folder(repo));
        let key = match repo.state {
            RepoStateView::Observed => "status.repo",
            RepoStateView::Unavailable => "status.repo-unavailable",
        };
        let _ = writeln!(out, "{}", t(key, &[("path", &folder)]));
        let _ = writeln!(out, "  {}", base_text(&repo.base));
        if let Some(kept) = &repo.kept_temps {
            kept_temps_text(&mut out, kept);
        }
        let base = base_name(&repo.base);
        for w in &repo.worktrees {
            worktree_text(&mut out, w, &base);
            for s in sessions.of(&repo.repo_id, w) {
                let _ = writeln!(out, "    {}", crate::sessions::line(s));
            }
        }
    }
    out
}

/// Temporary files the sweep after a crash kept (DS-TS-TMC-003, Enmienda T2): `raptor undo` is
/// offered only while the interrupted operation is still the last one.
fn kept_temps_text(out: &mut String, kept: &KeptTempsView) {
    let count = kept.count.to_string();
    let operation = sanitize(&kept.operation_id);
    let key = if kept.undo_next {
        "status.kept-temps"
    } else {
        "status.kept-temps-not-next"
    };
    let _ = writeln!(
        out,
        "  {}",
        t(key, &[("count", &count), ("operation", &operation)])
    );
    if kept.foreign > 0 {
        let foreign = kept.foreign.to_string();
        let _ = writeln!(
            out,
            "    {}",
            t("status.kept-temps-foreign", &[("foreign", &foreign)])
        );
    }
}

fn sanitize(text: &str) -> String {
    Untrusted::new(text).sanitized()
}

/// The base branch name, sanitized (empty if there is none).
fn base_name(base: &BaseBranchView) -> String {
    base.name
        .as_ref()
        .map(gitraptor_api::UntrustedName::sanitized)
        .unwrap_or_default()
}

fn base_text(base: &BaseBranchView) -> String {
    let name = base_name(base);
    match base.status {
        BaseStatusView::Confirmed => t("status.base", &[("name", &name)]),
        BaseStatusView::Unconfirmed => t("status.base-unconfirmed", &[("name", &name)]),
        BaseStatusView::Invalid => t("status.base-invalid", &[]),
    }
}

fn divergence_text(divergence: &DivergenceView, base: &str) -> String {
    let count = |c: &CommitCountView| {
        if c.exact {
            c.count.to_string()
        } else {
            t("status.at-least", &[("count", &c.count)])
        }
    };
    match divergence {
        // The base name last: it is repo text and must not fill the others.
        DivergenceView::Counted { ahead, behind } => t(
            "status.divergence",
            &[
                ("ahead", &count(ahead)),
                ("behind", &count(behind)),
                ("base", &base),
            ],
        ),
        DivergenceView::BaseMissing => t("status.no-divergence-base-missing", &[("base", &base)]),
        DivergenceView::NoBase => t("status.no-divergence-no-base", &[]),
        DivergenceView::NoCommits => t("status.no-divergence-no-commits", &[]),
        DivergenceView::Unreadable => t("status.no-divergence-unreadable", &[]),
    }
}

fn worktree_text(out: &mut String, w: &WorktreeView, base: &str) {
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
            divergence,
        } => {
            let _ = writeln!(out, "    {}", head_text(head));
            let _ = writeln!(out, "    {}", divergence_text(divergence, base));
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
    /// Whether agent sessions can be detected on this system; absent when
    /// the engine is older than this `raptor`.
    #[serde(skip_serializing_if = "Option::is_none")]
    session_detection: Option<bool>,
    repos: Vec<RepoJson>,
}

#[derive(Serialize)]
struct RepoJson {
    repo_id: String,
    path: String,
    git_dir: String,
    state: String,
    /// `None` if the repo has no base branch.
    base_branch: Option<String>,
    base_confirmed: bool,
    /// Temporary files kept by the sweep after a crash; absent when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    kept_temps: Option<KeptTempsView>,
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
    /// Against the repo's base branch; absent if the worktree is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead_behind: Option<AheadBehindJson>,
    /// Present sessions and the latest ended one (US-GRP-007).
    sessions: Vec<SessionJson>,
    /// More than one present session (BR-CONS-004): two agents share it.
    shared: bool,
}

#[derive(Serialize)]
struct AheadBehindJson {
    /// `counted`, `base-missing`, `no-base`, `no-commits` or `unreadable`.
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ahead: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    behind: Option<u64>,
    /// `false`: there are at least that many (the walk is bounded).
    #[serde(skip_serializing_if = "Option::is_none")]
    exact: Option<bool>,
}

fn ahead_behind_json(divergence: &DivergenceView) -> AheadBehindJson {
    let state = serde_json::to_value(divergence)
        .ok()
        .and_then(|v| v["state"].as_str().map(str::to_owned))
        .unwrap_or_default();
    match divergence {
        DivergenceView::Counted { ahead, behind } => AheadBehindJson {
            state,
            ahead: Some(ahead.count),
            behind: Some(behind.count),
            exact: Some(ahead.exact && behind.exact),
        },
        _ => AheadBehindJson {
            state,
            ahead: None,
            behind: None,
            exact: None,
        },
    }
}

#[derive(Serialize)]
struct ChangeJson {
    path: String,
    area: String,
    kind: String,
}

pub fn json(snapshot: &Snapshot, sessions: &SessionsInfo) -> StatusJson {
    StatusJson {
        engine: wire(&snapshot.engine.state),
        session_detection: sessions.available,
        repos: snapshot
            .repos
            .iter()
            .map(|repo| RepoJson {
                repo_id: repo.repo_id.clone(),
                path: repo_folder(repo),
                git_dir: repo.path.raw().to_owned(),
                state: wire(&repo.state),
                base_branch: repo.base.name.as_ref().map(|n| n.raw().to_owned()),
                base_confirmed: repo.base.status == BaseStatusView::Confirmed,
                kept_temps: repo.kept_temps.clone(),
                worktrees: repo
                    .worktrees
                    .iter()
                    .map(|w| {
                        let mut out = worktree_json(w);
                        out.sessions = sessions.of(&repo.repo_id, w).map(session_json).collect();
                        out.shared = sessions
                            .of(&repo.repo_id, w)
                            .filter(|s| s.state != SessionStateView::Ended)
                            .count()
                            > 1;
                        out
                    })
                    .collect(),
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
        ahead_behind: None,
        sessions: Vec::new(),
        shared: false,
    };
    match &w.status {
        WorktreeStatus::Unavailable { reason } => out.unavailable_reason = Some(wire(reason)),
        WorktreeStatus::Ready {
            head,
            counts,
            changes,
            divergence,
        } => {
            out.state = "ready";
            out.ahead_behind = Some(ahead_behind_json(divergence));
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
            last_activity_utc_ms: None,
            last_activity_in_gap: false,
            detached_at: None,
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
                protocol: 3,
                binary_version: "0".into(),
                started_wall_ms: 0,
            },
            repos: vec![RepoView {
                fetched_utc_ms: None,
                repo_id: "ab-01".into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/demo/.git"),
                base: BaseBranchView {
                    name: Some(gitraptor_api::UntrustedName::new("main")),
                    status: BaseStatusView::Unconfirmed,
                },
                worktrees: vec![
                    wt(
                        "/w/demo",
                        true,
                        WorktreeStatus::Ready {
                            head: HeadView::Branch {
                                name: gitraptor_api::UntrustedName::new("main"),
                            },
                            counts: ChangeCounts::default(),
                            changes: Vec::new(),
                            divergence: DivergenceView::Counted {
                                ahead: CommitCountView {
                                    count: 0,
                                    exact: true,
                                },
                                behind: CommitCountView {
                                    count: 0,
                                    exact: true,
                                },
                            },
                        },
                    ),
                    wt(
                        "/w/feat\u{1b}]52;c;eA==\u{7}",
                        false,
                        WorktreeStatus::Ready {
                            head: HeadView::Branch {
                                name: gitraptor_api::UntrustedName::new("feat-login"),
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
                            divergence: DivergenceView::Counted {
                                ahead: CommitCountView {
                                    count: 3,
                                    exact: true,
                                },
                                behind: CommitCountView {
                                    count: 10_000,
                                    exact: false,
                                },
                            },
                        },
                    ),
                ],
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
            }],
        }
    }

    #[test]
    fn text_lists_every_worktree_sanitized() {
        let out = text(&snapshot(), &SessionsInfo::default());
        assert!(out.contains("/w/demo"), "{out}");
        assert!(
            out.contains("feat-login") && out.contains("login.txt"),
            "{out}"
        );
        assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
    }

    #[test]
    fn text_shows_the_base_branch_and_each_ahead_behind() {
        let out = text(&snapshot(), &SessionsInfo::default());
        assert!(out.contains("main (unconfirmed)"), "{out}");
        assert!(out.contains("0 ahead and 0 behind main"), "{out}");
        assert!(
            out.contains("3 ahead and at least 10000 behind main"),
            "{out}"
        );
        let mut s = snapshot();
        s.repos[0].base.status = BaseStatusView::Confirmed;
        if let WorktreeStatus::Ready { divergence, .. } = &mut s.repos[0].worktrees[1].status {
            *divergence = DivergenceView::BaseMissing;
        }
        let out = text(&s, &SessionsInfo::default());
        assert!(out.contains("base branch: main\n"), "{out}");
        assert!(
            out.contains("base branch \"main\" does not exist in the repo"),
            "{out}"
        );
    }

    fn kept(count: u32, foreign: u32, undo_next: bool) -> KeptTempsView {
        KeptTempsView {
            count,
            foreign,
            operation_id: "op-1".into(),
            undo_next,
        }
    }

    #[test]
    fn kept_temporary_files_get_a_line_with_the_way_back() {
        let out = text(&snapshot(), &SessionsInfo::default());
        assert!(!out.contains("temporary files"), "{out}");
        let mut s = snapshot();
        s.repos[0].kept_temps = Some(kept(2, 0, true));
        let out = text(&s, &SessionsInfo::default());
        assert!(
            out.contains(
                "2 Time Machine temporary files kept after operation op-1 was interrupted: \
                 raptor undo takes it back"
            ),
            "{out}"
        );
        assert!(!out.contains("not GitRaptor's copies"), "{out}");
        // Someone else's files, and a later operation: no undo offered, a warning instead.
        s.repos[0].kept_temps = Some(kept(3, 1, false));
        let out = text(&s, &SessionsInfo::default());
        assert!(out.contains("later operations ran"), "{out}");
        assert!(!out.contains("raptor undo"), "{out}");
        assert!(
            out.contains("1 of them are not GitRaptor's copies"),
            "{out}"
        );
    }

    #[test]
    fn json_has_the_kept_temporary_files_only_when_there_are_some() {
        let value = serde_json::to_value(json(&snapshot(), &SessionsInfo::default())).unwrap();
        assert!(value["repos"][0].get("kept_temps").is_none(), "{value}");
        let mut s = snapshot();
        s.repos[0].kept_temps = Some(kept(2, 1, true));
        let value = serde_json::to_value(json(&s, &SessionsInfo::default())).unwrap();
        assert_eq!(
            value["repos"][0]["kept_temps"],
            serde_json::json!({"count": 2, "foreign": 1, "operation_id": "op-1", "undo_next": true})
        );
    }

    #[test]
    fn a_branch_name_cannot_fill_the_other_placeholders() {
        let d = DivergenceView::Counted {
            ahead: CommitCountView {
                count: 1,
                exact: true,
            },
            behind: CommitCountView {
                count: 2,
                exact: true,
            },
        };
        assert_eq!(
            divergence_text(&d, "{ahead}"),
            "1 ahead and 2 behind {ahead}"
        );
    }

    #[test]
    fn json_has_plain_strings_and_flags() {
        let value = serde_json::to_value(json(&snapshot(), &SessionsInfo::default())).unwrap();
        let wts = &value["repos"][0]["worktrees"];
        assert_eq!(value["repos"][0]["path"], "/w/demo");
        assert_eq!(wts[0]["clean"], true);
        assert_eq!(wts[1]["branch"], "feat-login");
        assert_eq!(wts[1]["changes"][0]["path"], "login.txt");
        assert_eq!(wts[1]["changes"][0]["area"], "unstaged");
        assert_eq!(value["repos"][0]["base_branch"], "main");
        assert_eq!(value["repos"][0]["base_confirmed"], false);
        assert_eq!(
            wts[1]["ahead_behind"],
            serde_json::json!({"state": "counted", "ahead": 3, "behind": 10_000, "exact": false})
        );
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
