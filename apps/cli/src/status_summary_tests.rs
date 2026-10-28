//! Contract tests of the untracked summary of `raptor status` (US-GRP-023): the literal E1 and E2
//! examples of the story, rendered with an explicit `Tr`. The line `base branch: ...` the repo
//! prints under its path is left out of the comparison: the examples do not show it and the story
//! says the existing lines keep their text.

use gitraptor_api::UntrustedName;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeAreaView, ChangeCounts, ChangeKindView, CommitCountView,
    DaemonView, DivergenceView, EngineStateView, EngineView, FileChangeView, HeadView,
    RepoStateView, RepoView, Snapshot, WorktreeStatus, WorktreeView,
};

use super::*;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };
const ROOT: &str = "/Users/rene/code/gitRaptor";

fn change(path: &str, area: ChangeAreaView, kind: ChangeKindView) -> FileChangeView {
    FileChangeView {
        path: Untrusted::new(path),
        area,
        kind,
    }
}

fn untracked(path: String) -> FileChangeView {
    change(&path, ChangeAreaView::Untracked, ChangeKindView::Added)
}

fn snapshot_with(counts: ChangeCounts, changes: Vec<FileChangeView>) -> Snapshot {
    let zero = CommitCountView {
        count: 0,
        exact: true,
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
            path: Untrusted::new(format!("{ROOT}/.git")),
            base: BaseBranchView {
                name: Some(UntrustedName::new("main")),
                status: BaseStatusView::Confirmed,
            },
            worktrees: vec![WorktreeView {
                last_activity_utc_ms: None,
                last_activity_in_gap: false,
                detached_at: None,
                operation: None,
                unmerged_paths: 0,
                path: Untrusted::new(ROOT),
                main: true,
                admin_name: None,
                status: WorktreeStatus::Ready {
                    head: HeadView::Branch {
                        name: UntrustedName::new("main"),
                    },
                    counts,
                    changes,
                    divergence: DivergenceView::Counted {
                        ahead: zero,
                        behind: zero,
                    },
                },
            }],
            tier: None,
            checked_utc_ms: None,
            kept_temps: None,
            prediction: None,
        }],
    }
}

/// E1/E2 worktree: 1 staged, 2 not staged, 220 untracked (214 in `bitacora/`, 4 in `docs/`, 2 in
/// the root). The engine sends the unstaged ones first, to show the summary orders them.
fn e1() -> Snapshot {
    let mut changes = vec![
        change(
            "README.md",
            ChangeAreaView::Unstaged,
            ChangeKindView::Modified,
        ),
        change(
            "docs/backlog.md",
            ChangeAreaView::Staged,
            ChangeKindView::Modified,
        ),
        change(
            "notes.txt",
            ChangeAreaView::Unstaged,
            ChangeKindView::Deleted,
        ),
    ];
    changes.extend((0..214).map(|i| untracked(format!("bitacora/f{i}.md"))));
    changes.extend((0..4).map(|i| untracked(format!("docs/d{i}.md"))));
    changes.extend((0..2).map(|i| untracked(format!("root{i}.txt"))));
    snapshot_with(
        ChangeCounts {
            staged: 1,
            unstaged: 2,
            untracked: 220,
        },
        changes,
    )
}

/// The text without the repo's `base branch` line.
fn shown(snapshot: &Snapshot, verbose: bool, tr: Tr) -> String {
    let out = text(snapshot, &SessionsInfo::default(), verbose, tr);
    out.lines()
        .filter(|l| !l.starts_with("  base branch") && !l.starts_with("  rama base"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn joined(lines: &[&str]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// GRP023-SUMMARY
#[test]
fn many_untracked_files_are_summarized_by_folder_in_english() {
    assert_eq!(
        shown(&e1(), false, EN),
        joined(&[
            "/Users/rene/code/gitRaptor",
            "  main worktree /Users/rene/code/gitRaptor",
            "    branch main",
            "    0 ahead and 0 behind main",
            "    223 uncommitted changes: 1 staged, 2 not staged, 220 untracked",
            "      staged modified docs/backlog.md",
            "      not staged modified README.md",
            "      not staged deleted notes.txt",
            "      220 untracked: bitacora/ 214, docs/ 4, (root) 2",
            "",
            "untracked files one by one: raptor status --verbose",
        ])
    );
}

/// GRP023-SUMMARY-ES
#[test]
fn many_untracked_files_are_summarized_by_folder_in_spanish() {
    assert_eq!(
        shown(&e1(), false, ES),
        joined(&[
            "/Users/rene/code/gitRaptor",
            "  worktree principal /Users/rene/code/gitRaptor",
            "    rama main",
            "    0 por delante y 0 por detrás de main",
            "    223 cambios sin commitear: 1 preparados, 2 sin preparar, 220 sin rastrear",
            "      preparado modificado docs/backlog.md",
            "      sin preparar modificado README.md",
            "      sin preparar borrado notes.txt",
            "      220 sin rastrear: bitacora/ 214, docs/ 4, (raíz) 2",
            "",
            "archivos sin rastrear uno por uno: raptor status --verbose",
        ])
    );
}

/// GRP023-FEW: 3 untracked files stay one by one, with no summary and no hint.
#[test]
fn few_untracked_files_are_still_listed() {
    let few = snapshot_with(
        ChangeCounts {
            staged: 0,
            unstaged: 0,
            untracked: 3,
        },
        ["a.txt", "b.txt", "c.txt"]
            .map(|p| untracked(p.into()))
            .to_vec(),
    );

    assert_eq!(
        shown(&few, false, EN),
        joined(&[
            "/Users/rene/code/gitRaptor",
            "  main worktree /Users/rene/code/gitRaptor",
            "    branch main",
            "    0 ahead and 0 behind main",
            "    3 uncommitted changes: 0 staged, 0 not staged, 3 untracked",
            "      untracked a.txt",
            "      untracked b.txt",
            "      untracked c.txt",
        ])
    );
    assert_eq!(
        shown(&few, false, ES),
        joined(&[
            "/Users/rene/code/gitRaptor",
            "  worktree principal /Users/rene/code/gitRaptor",
            "    rama main",
            "    0 por delante y 0 por detrás de main",
            "    3 cambios sin commitear: 0 preparados, 0 sin preparar, 3 sin rastrear",
            "      sin rastrear a.txt",
            "      sin rastrear b.txt",
            "      sin rastrear c.txt",
        ])
    );
}

/// GRP023-VERBOSE: every untracked file the engine sent, on its own line, and no hint.
#[test]
fn verbose_lists_every_untracked_file_sent() {
    let many = snapshot_with(
        ChangeCounts {
            staged: 0,
            unstaged: 0,
            untracked: 12,
        },
        (0..12)
            .map(|i| untracked(format!("bitacora/f{i:02}.md")))
            .collect(),
    );
    let listed: Vec<String> = (0..12)
        .map(|i| format!("      untracked bitacora/f{i:02}.md"))
        .collect();
    let listed_es: Vec<String> = (0..12)
        .map(|i| format!("      sin rastrear bitacora/f{i:02}.md"))
        .collect();
    let with = |head: &[&str], list: &[String]| {
        let mut lines: Vec<&str> = head.to_vec();
        lines.extend(list.iter().map(String::as_str));
        joined(&lines)
    };

    assert_eq!(
        shown(&many, true, EN),
        with(
            &[
                "/Users/rene/code/gitRaptor",
                "  main worktree /Users/rene/code/gitRaptor",
                "    branch main",
                "    0 ahead and 0 behind main",
                "    12 uncommitted changes: 0 staged, 0 not staged, 12 untracked",
            ],
            &listed
        )
    );
    assert_eq!(
        shown(&many, true, ES),
        with(
            &[
                "/Users/rene/code/gitRaptor",
                "  worktree principal /Users/rene/code/gitRaptor",
                "    rama main",
                "    0 por delante y 0 por detrás de main",
                "    12 cambios sin commitear: 0 preparados, 0 sin preparar, 12 sin rastrear",
            ],
            &listed_es
        )
    );
}

/// GRP023-PARTIAL: 5000 untracked, 200 sent. The sum of the line is the real total.
#[test]
fn a_partial_list_never_looks_like_the_total() {
    let mut sent: Vec<FileChangeView> = (0..150)
        .map(|i| untracked(format!("bitacora/f{i}.md")))
        .collect();
    sent.extend((0..50).map(|i| untracked(format!("docs/d{i}.md"))));
    let partial = snapshot_with(
        ChangeCounts {
            staged: 0,
            unstaged: 0,
            untracked: 5000,
        },
        sent,
    );

    assert_eq!(
        shown(&partial, false, EN),
        joined(&[
            "/Users/rene/code/gitRaptor",
            "  main worktree /Users/rene/code/gitRaptor",
            "    branch main",
            "    0 ahead and 0 behind main",
            "    5000 uncommitted changes: 0 staged, 0 not staged, 5000 untracked",
            "      5000 untracked: bitacora/ 150, docs/ 50, 4800 not listed",
            "",
            "untracked files one by one: raptor status --verbose",
        ])
    );
    let es = shown(&partial, false, ES);
    assert!(
        es.contains("      5000 sin rastrear: bitacora/ 150, docs/ 50, 4800 sin listar\n"),
        "{es}"
    );
}

/// GRP023-JSON-UNCHANGED: `--json` keeps listing every change the engine sent and gains no field.
#[test]
fn json_output_is_unchanged_by_verbose_and_summary() {
    let snapshot = e1();
    // The scenario's precondition: the text of this worktree is summarized.
    assert!(
        shown(&snapshot, false, EN).contains("220 untracked: bitacora/ 214"),
        "the text is not summarized"
    );

    let value = serde_json::to_value(json(&snapshot, &SessionsInfo::default())).unwrap();

    let worktree = &value["repos"][0]["worktrees"][0];
    assert_eq!(worktree["changes"].as_array().unwrap().len(), 223);
    assert_eq!(worktree["counts"]["untracked"], 220);
    assert_eq!(worktree["changes_truncated"], false);
    assert_eq!(worktree["changes"][0]["path"], "README.md");
    let mut keys: Vec<&str> = worktree
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "ahead_behind",
            "branch",
            "changes",
            "changes_truncated",
            "clean",
            "counts",
            "head",
            "main",
            "path",
            "sessions",
            "shared",
            "state"
        ]
    );
}
