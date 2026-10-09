//! The decisions of the default answer and of the page of worktrees, from a hand-built
//! [`McpContext`]: which sessions count, how many are shown, which gaps touch the window, what
//! the guard and the engine tier say and how the other worktrees are paged. No disk, no daemon.

use std::collections::BTreeSet;
use std::path::Path;

use gitraptor_api::guard::{
    Diagnostic, GuardStatus, HooksLayer, HooksStatus, LossCause, Permission, ProtectionState,
};
use gitraptor_api::mcp_view::{MCP_MAX_GAPS, MCP_MAX_SESSIONS, MCP_WORKTREES_PAGE};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeCounts, DivergenceView, HeadView, RepoStateView,
    RepoTier, RepoView, SessionStateView, SessionView, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::{McpEngineState, McpProtectionState};
use gitraptor_api::{Actor, Untrusted, UntrustedName, actor::AgentOrigin};

use super::*;
use crate::profile::{Gap, GapCause};

const NOW_MS: i64 = 1_800_000_000_000;
const HOUR_MS: i64 = 3_600_000;

fn view(root: &str, main: bool) -> WorktreeView {
    WorktreeView {
        path: Untrusted::new(root),
        main,
        admin_name: (!main).then(|| UntrustedName::new("w")),
        status: WorktreeStatus::Ready {
            head: HeadView::Branch {
                name: UntrustedName::new("main"),
            },
            counts: ChangeCounts::default(),
            changes: Vec::new(),
            divergence: DivergenceView::NoBase,
        },
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
    }
}

fn repo_of(worktrees: Vec<WorktreeView>, tier: Option<RepoTier>) -> RepoView {
    RepoView {
        repo_id: "r1".to_owned(),
        state: RepoStateView::Observed,
        path: Untrusted::new("/t/shop.git"),
        base: BaseBranchView {
            name: Some(UntrustedName::new("main")),
            status: BaseStatusView::Confirmed,
        },
        worktrees,
        fetched_utc_ms: None,
        tier,
        checked_utc_ms: None,
        kept_temps: None,
    }
}

fn guard(state: ProtectionState) -> GuardStatus {
    GuardStatus {
        repo_id: "r1".to_owned(),
        state,
        permission: Permission::NotAsked,
        offer: false,
        protected_bases: Vec::new(),
        base_confirmed: true,
        not_preventable: Vec::new(),
        last_refusal: Vec::new(),
        misnamed_settings: Vec::new(),
        pending: None,
        hooks: None,
        diagnostics: Vec::new(),
        minimum_set: None,
    }
}

fn context() -> McpContext {
    McpContext {
        detection_available: true,
        sessions: Vec::new(),
        gaps: Vec::new(),
        guard: guard(ProtectionState::Unprotected),
    }
}

fn session(id: &str, root: &str, state: SessionStateView) -> SessionView {
    SessionView {
        repo_id: "r1".to_owned(),
        session_id: id.to_owned(),
        worktree: Untrusted::new(root),
        actor: Actor::Agent {
            kind: gitraptor_api::actor::AgentKind::Other,
            name: Some(UntrustedName::new(id)),
            origin: AgentOrigin::Registered,
        },
        state,
        started_utc_ms: NOW_MS - HOUR_MS,
        state_since_utc_ms: NOW_MS - HOUR_MS,
        utc_offset_s: 0,
        ended_utc_ms: None,
        end_cause: None,
    }
}

fn gap(id: &str, started_ms: i64, ended_ms: Option<i64>) -> Gap {
    Gap {
        gap_id: id.to_owned(),
        started_ms,
        ended_ms,
        cause: GapCause::DaemonDown,
        requested_by: None,
    }
}

#[test]
fn the_own_session_and_the_ended_ones_are_not_listed_and_the_rest_is_capped() {
    let repo = repo_of(vec![view("/t/shop", true)], None);
    let mut ctx = context();
    ctx.sessions
        .push(session("own", "/t/shop", SessionStateView::Active));
    ctx.sessions
        .push(session("gone", "/t/shop", SessionStateView::Ended));
    ctx.sessions
        .push(session("elsewhere", "/t/other", SessionStateView::Active));
    for n in 0..10 {
        ctx.sessions.push(session(
            &format!("s{n}"),
            "/t/shop",
            SessionStateView::Active,
        ));
    }
    let (here, _) = default_status(&repo, 0, &ctx, Some("own"), NOW_MS);
    let here = here.expect("there are sessions to tell");
    assert_eq!(here.sessions.len(), MCP_MAX_SESSIONS);
    assert_eq!(here.sessions_total, Some(10));
    let names: Vec<_> = here
        .sessions
        .iter()
        .filter_map(|s| match &s.actor {
            Actor::Agent { name, .. } => name.as_ref().map(|n| n.raw().to_owned()),
            Actor::Unattributed => None,
        })
        .collect();
    assert!(names.iter().all(|n| n.starts_with('s')), "{names:?}");

    // Up to the cap there is no total: the list is the whole.
    let mut few = context();
    few.sessions
        .push(session("own", "/t/shop", SessionStateView::Active));
    few.sessions
        .push(session("a", "/t/shop", SessionStateView::Inactive));
    let (here, _) = default_status(&repo, 0, &few, Some("own"), NOW_MS);
    let here = here.unwrap();
    assert_eq!(here.sessions.len(), 1);
    assert_eq!(here.sessions_total, None);
}

#[test]
fn a_worktree_with_nothing_to_say_has_no_situation() {
    let mut quiet = view("/t/shop", true);
    if let WorktreeStatus::Ready { divergence, .. } = &mut quiet.status {
        *divergence = DivergenceView::Counted {
            ahead: gitraptor_api::messages::CommitCountView {
                count: 0,
                exact: true,
            },
            behind: gitraptor_api::messages::CommitCountView {
                count: 0,
                exact: true,
            },
        };
    }
    let repo = repo_of(vec![quiet], None);
    let (here, _) = default_status(&repo, 0, &context(), None, NOW_MS);
    assert_eq!(here, None);
}

#[test]
fn only_the_gaps_that_touch_the_last_day_count_and_three_are_shown() {
    let repo = repo_of(vec![view("/t/shop", true)], None);
    let mut ctx = context();
    // Ended before the window: out.
    ctx.gaps.push(gap(
        "old",
        NOW_MS - 30 * HOUR_MS,
        Some(NOW_MS - 25 * HOUR_MS),
    ));
    // Started before the window and ended inside it, and one still open: both touch it.
    ctx.gaps.push(gap(
        "edge",
        NOW_MS - 26 * HOUR_MS,
        Some(NOW_MS - 23 * HOUR_MS),
    ));
    ctx.gaps.push(gap("open", NOW_MS - 2 * HOUR_MS, None));
    for n in 0..3 {
        ctx.gaps.push(gap(
            &format!("g{n}"),
            NOW_MS - (10 + n) * HOUR_MS,
            Some(NOW_MS - (9 + n) * HOUR_MS),
        ));
    }
    let (_, repo_part) = default_status(&repo, 0, &ctx, None, NOW_MS);
    assert_eq!(repo_part.gaps_total, Some(5), "{:?}", repo_part.gaps);
    assert_eq!(repo_part.gaps.len(), MCP_MAX_GAPS);
    // The most recent first: the open one, then the ones of 10 and 11 hours ago.
    assert_eq!(repo_part.gaps[0].from_s_ago, 2 * 3600);
    assert_eq!(repo_part.gaps[0].to_s_ago, None);
    assert_eq!(repo_part.gaps[1].from_s_ago, 10 * 3600);
    assert_eq!(repo_part.gaps[2].from_s_ago, 11 * 3600);

    // Three or fewer: no total.
    let mut few = context();
    few.gaps.push(gap(
        "old",
        NOW_MS - 30 * HOUR_MS,
        Some(NOW_MS - 25 * HOUR_MS),
    ));
    few.gaps.push(gap("open", NOW_MS - HOUR_MS, None));
    let (_, repo_part) = default_status(&repo, 0, &few, None, NOW_MS);
    assert_eq!(repo_part.gaps.len(), 1);
    assert_eq!(repo_part.gaps_total, None);
}

#[test]
fn the_guard_decides_the_protection_and_carries_its_diagnostics() {
    let repo = repo_of(vec![view("/t/shop", true)], None);
    let mut ctx = context();
    let (_, part) = default_status(&repo, 0, &ctx, None, NOW_MS);
    assert_eq!(part.protection, McpProtectionState::McpOnly);
    assert!(part.diagnostics.is_empty());
    assert_eq!(part.protection_lost, None);

    ctx.guard = guard(ProtectionState::HooksOnly);
    ctx.guard.diagnostics.push(Diagnostic::TemplateOutdated);
    let (_, part) = default_status(&repo, 0, &ctx, None, NOW_MS);
    assert_eq!(part.protection, McpProtectionState::Full);
    assert_eq!(part.diagnostics, ctx.guard.diagnostics);
    assert_eq!(part.protection_lost, None);

    // Hooks that were installed and stopped working say why they were lost.
    ctx.guard.hooks = Some(HooksLayer {
        status: HooksStatus::Inactive,
        cause: Some(LossCause::FolderMissing),
        worktree: None,
    });
    let (_, part) = default_status(&repo, 0, &ctx, None, NOW_MS);
    assert_eq!(part.protection_lost, Some(LossCause::FolderMissing));
}

#[test]
fn the_engine_tier_maps_to_the_state_the_agent_is_told() {
    let engine = |tier| {
        let repo = repo_of(vec![view("/t/shop", true)], tier);
        default_status(&repo, 0, &context(), None, NOW_MS).1.engine
    };
    assert_eq!(
        engine(Some(RepoTier::Waking)),
        Some(McpEngineState::Reconciling)
    );
    assert_eq!(
        engine(Some(RepoTier::Dormant)),
        Some(McpEngineState::Dormant)
    );
    assert_eq!(engine(Some(RepoTier::Active)), None);
    assert_eq!(engine(None), None);
    // `WaitingForGit` is the engine's own state, not the repo's tier: the connection adds it
    // from the snapshot, so the pure answer never says it.
    assert_ne!(engine(None), Some(McpEngineState::WaitingForGit));
}

#[test]
fn more_than_a_page_of_worktrees_is_walked_without_repeats_and_main_first() {
    // The main one is not the first listed, nor the first by root.
    let mut worktrees: Vec<WorktreeView> = (0..20)
        .map(|n| view(&format!("/t/wt-{n:02}"), false))
        .collect();
    worktrees.push(view("/t/zz-main", true));
    let repo = repo_of(worktrees, None);
    let caller = 5;
    let ctx = context();

    let mut seen: Vec<String> = Vec::new();
    let mut after: Option<std::path::PathBuf> = None;
    let mut pages = 0;
    loop {
        let page = worktrees_page(&repo, caller, &ctx, after.as_deref());
        assert_eq!(page.total, 20);
        assert!(page.worktrees.len() <= MCP_WORKTREES_PAGE);
        seen.extend(page.worktrees.iter().map(|w| w.name.raw().to_owned()));
        pages += 1;
        assert!(pages <= 3, "it never ends: {seen:?}");
        // The indexes of the page are the ones the connection uses for the cursor.
        let indexes = page_indexes(&repo, caller, after.as_deref());
        assert_eq!(indexes.len(), page.worktrees.len());
        if !page.truncated {
            assert_eq!(page.cursor, None);
            break;
        }
        assert!(page.cursor.is_some());
        let last = *indexes.last().expect("a cut page has items");
        after = Some(Path::new(repo.worktrees[last].path.raw()).to_path_buf());
    }
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 20);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 20, "{seen:?}");
    assert_eq!(seen[0], "zz-main");
    assert!(
        !seen.contains(&"wt-05".to_owned()),
        "the caller is not listed"
    );
    let rest: Vec<&String> = seen[1..].iter().collect();
    assert!(rest.windows(2).all(|w| w[0] < w[1]), "{rest:?}");
}

#[test]
fn a_page_cursor_to_a_worktree_removed_since_goes_on_after_its_place() {
    let worktrees: Vec<WorktreeView> = (0..12)
        .map(|n| view(&format!("/t/wt-{n:02}"), n == 0))
        .collect();
    let repo = repo_of(worktrees, None);
    // `wt-04b` does not exist any more; the page goes on with what sorts after it.
    let page = worktrees_page(&repo, 1, &context(), Some(Path::new("/t/wt-04b")));
    let names: Vec<&str> = page.worktrees.iter().map(|w| w.name.raw()).collect();
    assert_eq!(names.first().copied(), Some("wt-05"), "{names:?}");
    assert!(
        !names.contains(&"wt-00"),
        "the main one came on the first page"
    );
}

/// A repo whose main root is a symlink to a real folder, as the engine would list it.
#[cfg(unix)]
#[test]
fn a_root_that_is_a_symlink_is_untrusted() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (common, real, link) = (base.join("shop.git"), base.join("real"), base.join("link"));
    std::fs::create_dir_all(&common).unwrap();
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut repo = repo_of(vec![view(&link.to_string_lossy(), true)], None);
    repo.path = Untrusted::from_os(common.as_os_str());

    let read = facts(&repo, 0, None);
    assert!(read.exists, "the link is there");
    assert!(!read.trusted_link);
    assert_eq!(
        availability(&repo, 0, &read),
        Err(McpUnavailable::WorktreeUntrusted)
    );
}

/// The `.git` of a linked worktree is a regular file of at most 4 KiB: a symlink to a good
/// one, or a huge one, is not trusted.
#[cfg(unix)]
#[test]
fn a_linked_git_file_that_is_a_symlink_or_huge_is_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let common = base.join("shop.git");
    let admin = common.join("worktrees").join("w");
    let root = base.join("wt");
    for dir in [&admin, &root] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let good = format!("gitdir: {}\n", admin.display());
    let trusted = |root: &Path| crate::observe::linked_is_trusted_in(&common, "w", root, None);

    std::fs::write(root.join(".git"), &good).unwrap();
    assert!(trusted(&root));

    // The same text behind a symlink.
    std::fs::remove_file(root.join(".git")).unwrap();
    let elsewhere = base.join("elsewhere");
    std::fs::write(&elsewhere, &good).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join(".git")).unwrap();
    assert!(!trusted(&root));

    // The same text followed by more than 4 KiB.
    std::fs::remove_file(root.join(".git")).unwrap();
    std::fs::write(root.join(".git"), format!("{good}{}", "#".repeat(5000))).unwrap();
    assert!(!trusted(&root));
}
