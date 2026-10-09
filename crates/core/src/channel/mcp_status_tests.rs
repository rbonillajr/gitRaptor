//! The pure parts of the full `mcp.status`: what makes a worktree unavailable and in which order
//! it is refused, where a caller whose folder was deleted still is, the pages of paths and the
//! table of cursors. Dedicated file (wired from `mcp_status.rs` by one line): the criteria's
//! tests must not live in the production file. Real folders only under a temporary directory
//! (NFR-01).

use std::path::{Path, PathBuf};

use gitraptor_api::mcp_view::{MCP_CURSOR_PLACEHOLDER, MCP_MAX_CURSORS, MCP_PATHS_PAGE};
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, ChangeAreaView, ChangeCounts, ChangeKindView, DivergenceView,
    FileChangeView, HeadView, RepoStateView, RepoView, UnavailableReason, WorktreeStatus,
    WorktreeView,
};
use gitraptor_api::methods::{McpUnavailable, valid_cursor};
use gitraptor_api::{Untrusted, UntrustedName};

use super::*;

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

fn worktree(root: &Path, main: bool, admin: Option<&str>, status: WorktreeStatus) -> WorktreeView {
    WorktreeView {
        path: Untrusted::from_os(root.as_os_str()),
        main,
        admin_name: admin.map(UntrustedName::new),
        status,
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
    }
}

fn ready() -> WorktreeStatus {
    WorktreeStatus::Ready {
        head: HeadView::Branch {
            name: UntrustedName::new("main"),
        },
        counts: ChangeCounts::default(),
        changes: Vec::new(),
        divergence: DivergenceView::BaseMissing,
    }
}

fn repo(id: &str, common: &Path, worktrees: Vec<WorktreeView>) -> RepoView {
    RepoView {
        repo_id: id.to_owned(),
        state: RepoStateView::Observed,
        path: Untrusted::from_os(common.as_os_str()),
        base: BaseBranchView {
            name: Some(UntrustedName::new("main")),
            status: BaseStatusView::Confirmed,
        },
        worktrees,
        fetched_utc_ms: None,
        tier: None,
        checked_utc_ms: None,
        kept_temps: None,
    }
}

fn facts_of(owned_by_me: Option<bool>, exists: bool, trusted_link: bool) -> WorktreeFacts {
    WorktreeFacts {
        exists,
        owned_by_me,
        trusted_link,
    }
}

/// A repo of one main worktree under a temporary directory, as the engine would list it.
fn real_repo(tmp: &Path) -> RepoView {
    let (common, root) = (tmp.join("shop.git"), tmp.join("shop"));
    std::fs::create_dir_all(&common).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    repo(
        "r1",
        &canonical(&common),
        vec![worktree(&canonical(&root), true, None, ready())],
    )
}

/// BR-MCP-EDGE-005: a repo of another system user is unavailable, said by its reason and nothing
/// else; the owner is read from the OS, and where it has none the engine's verdict rules.
#[test]
fn another_owner_makes_the_repo_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = real_repo(&canonical(tmp.path()));
    assert_eq!(
        availability(&repo, 0, &facts_of(Some(false), true, true)),
        Err(McpUnavailable::OtherOwner)
    );
    assert_eq!(
        availability(&repo, 0, &facts_of(Some(true), true, true)),
        Ok(())
    );
    assert_eq!(availability(&repo, 0, &facts_of(None, true, true)), Ok(()));

    // What is read from the OS: this user's folder is its own, where an owner is told.
    let read = facts(&repo, 0, None);
    assert!(read.exists);
    assert!(read.trusted_link, "a main worktree has no link to distrust");
    assert_eq!(read.owned_by_me, cfg!(unix).then_some(true));
}

/// SEC-11: a linked worktree rooted at `$HOME` is unavailable, even with its `.git` pointing back
/// at the repo as it should; the same worktree elsewhere is not.
#[test]
fn a_worktree_rooted_at_home_is_unavailable() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let common = base.join("shop.git");
    let admin = common.join("worktrees").join("wt1");
    let (main_root, home) = (base.join("shop"), base.join("home"));
    for dir in [&admin, &main_root, &home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(home.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    let repo = repo(
        "r1",
        &canonical(&common),
        vec![
            worktree(&canonical(&main_root), true, None, ready()),
            worktree(&canonical(&home), false, Some("wt1"), ready()),
        ],
    );

    let at_home = facts(&repo, 1, Some(&canonical(&home)));
    assert!(at_home.exists);
    assert!(!at_home.trusted_link, "{at_home:?}");
    assert_eq!(
        availability(&repo, 1, &at_home),
        Err(McpUnavailable::WorktreeUntrusted)
    );

    let elsewhere = facts(&repo, 1, Some(&base.join("not-home")));
    assert!(elsewhere.trusted_link, "{elsewhere:?}");
    assert_eq!(availability(&repo, 1, &elsewhere), Ok(()));

    // What the engine published counts too: an untrusted or missing link, or a repo whose
    // store cannot be opened.
    let published = |reason| {
        let mut r = repo.clone();
        r.worktrees[1].status = WorktreeStatus::Unavailable { reason };
        r
    };
    let sound = facts_of(Some(true), true, true);
    for (reason, expected) in [
        (
            UnavailableReason::Untrusted,
            McpUnavailable::WorktreeUntrusted,
        ),
        (UnavailableReason::Missing, McpUnavailable::WorktreeMissing),
        (
            UnavailableReason::Unreadable,
            McpUnavailable::RepoUnreadable,
        ),
    ] {
        assert_eq!(availability(&published(reason), 1, &sound), Err(expected));
    }
    let mut broken = repo.clone();
    broken.state = RepoStateView::Unavailable;
    assert_eq!(
        availability(&broken, 0, &sound),
        Err(McpUnavailable::RepoUnreadable)
    );
}

/// BR-MCP-EDGE-005: "not enabled" is the answer for a repo outside the allowlist, whatever else
/// is wrong with it: nothing of an unavailable repo is told to a caller it was not enabled for.
#[test]
fn the_allowlist_is_checked_before_availability() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = real_repo(&canonical(tmp.path()));
    let broken = [
        facts_of(Some(true), false, true),
        facts_of(Some(false), true, true),
        facts_of(Some(true), true, false),
    ];
    for facts in &broken {
        assert_eq!(admit(false, &repo, 0, facts), Err(Refusal::NotAllowlisted));
    }
    assert_eq!(
        admit(true, &repo, 0, &broken[0]),
        Err(Refusal::Unavailable(McpUnavailable::WorktreeMissing))
    );
    assert_eq!(
        admit(true, &repo, 0, &broken[1]),
        Err(Refusal::Unavailable(McpUnavailable::OtherOwner))
    );
    assert_eq!(
        admit(true, &repo, 0, &broken[2]),
        Err(Refusal::Unavailable(McpUnavailable::WorktreeUntrusted))
    );
    let mut unreadable = repo.clone();
    unreadable.state = RepoStateView::Unavailable;
    assert_eq!(
        admit(false, &unreadable, 0, &facts_of(Some(true), true, true)),
        Err(Refusal::NotAllowlisted)
    );
    assert_eq!(
        admit(true, &repo, 0, &facts_of(Some(true), true, true)),
        Ok(())
    );
}

/// D9: a caller whose worktree was deleted during the session still has its repo: the last scope
/// served stands in while that root is gone, and only then. Without a previous scope, or while
/// the root is still there, the caller is outside, as before.
#[test]
fn a_deleted_cwd_with_a_previous_scope_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let base = canonical(tmp.path());
    let (main_root, linked_root) = (base.join("shop"), base.join("shop-feat"));
    std::fs::create_dir_all(&main_root).unwrap();
    std::fs::create_dir_all(&linked_root).unwrap();
    let (main_root, linked_root) = (canonical(&main_root), canonical(&linked_root));
    let other_root = base.join("other");
    std::fs::create_dir_all(&other_root).unwrap();
    let repos = [
        repo(
            "r1",
            &base.join("shop.git"),
            vec![
                worktree(&main_root, true, None, ready()),
                worktree(&linked_root, false, Some("feat"), ready()),
            ],
        ),
        repo(
            "r2",
            &base.join("other.git"),
            vec![worktree(&canonical(&other_root), true, None, ready())],
        ),
    ];
    let last = |id: &str, root: &Path| Some((id.to_owned(), root.to_owned()));

    // Where the caller is, it is: the current folder wins over the memory.
    assert_eq!(
        locate_with_last(
            Some(&main_root),
            &repos,
            last("r2", &canonical(&other_root)).as_ref()
        ),
        Located::Worktree {
            repo: 0,
            worktree: 0
        }
    );

    // The worktree is deleted: its folder is gone and the working folder cannot be read.
    std::fs::remove_dir_all(&linked_root).unwrap();
    assert_eq!(
        locate_with_last(None, &repos, last("r1", &linked_root).as_ref()),
        Located::Missing { repo: 0 }
    );
    // A previous scope of another repo leads to that one: the memory decides, not the position.
    std::fs::remove_dir_all(&other_root).unwrap();
    assert_eq!(
        locate_with_last(None, &repos, last("r2", &other_root).as_ref()),
        Located::Missing { repo: 1 }
    );

    // No previous scope: nothing says where the caller was.
    assert_eq!(locate_with_last(None, &repos, None), Located::Outside);
    // A previous scope whose root still exists: the working folder is merely unreadable, which
    // is not a deleted worktree.
    assert_eq!(
        locate_with_last(None, &repos, last("r1", &main_root).as_ref()),
        Located::Outside
    );
    // A previous scope of a repo that is not observed any more.
    assert_eq!(
        locate_with_last(None, &repos, last("gone", &linked_root).as_ref()),
        Located::Outside
    );
}

fn change(path: &str, area: ChangeAreaView) -> FileChangeView {
    FileChangeView {
        path: Untrusted::new(path),
        area,
        kind: ChangeKindView::Modified,
    }
}

/// `n` changed paths, every fifth one in two areas, sorted as the engine sorts them: by path,
/// then by area.
fn sorted_changes(n: usize) -> (ChangeCounts, Vec<FileChangeView>, Vec<String>) {
    let mut changes = Vec::new();
    let mut paths = Vec::new();
    for i in 0..n {
        let path = format!("dir_{}/file_{i:04}.txt", i % 7);
        changes.push((path.clone(), ChangeAreaView::Unstaged));
        if i % 5 == 0 {
            changes.push((path.clone(), ChangeAreaView::Staged));
        }
        paths.push(path);
    }
    changes.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    paths.sort();
    let mut counts = ChangeCounts::default();
    for (_, area) in &changes {
        match area {
            ChangeAreaView::Staged => counts.staged += 1,
            ChangeAreaView::Unstaged => counts.unstaged += 1,
            ChangeAreaView::Untracked => counts.untracked += 1,
        }
    }
    let list = changes.iter().map(|(p, a)| change(p, *a)).collect();
    (counts, list, paths)
}

/// D3 and D4: a page of paths goes on after the last key served, with unique paths and no more
/// than its cap; walking the pages gives every path exactly once, in order, whatever the areas of
/// each one.
#[test]
fn a_paths_page_continues_after_its_key_with_unique_paths() {
    let of = UntrustedName::new("shop-feat-a");
    let (counts, changes, unique) = sorted_changes(100);
    assert!(changes.len() > unique.len(), "the list repeats some paths");

    let mut after: Option<String> = None;
    let mut seen: Vec<String> = Vec::new();
    let mut pages = 0;
    loop {
        let page = paths_page(of.clone(), counts, &changes, after.as_deref());
        pages += 1;
        assert!(pages <= 10, "the walk never ended");
        let paths: Vec<String> = page.paths.iter().map(|p| p.raw().to_owned()).collect();
        assert_eq!(page.of.as_ref(), Some(&of));
        assert_eq!(page.total, counts.total());
        assert!(page.worktrees.is_empty());
        assert!(
            !paths.is_empty() && paths.len() <= MCP_PATHS_PAGE,
            "{paths:?}"
        );
        let mut distinct = paths.clone();
        distinct.dedup();
        assert_eq!(distinct, paths, "a path twice in one page");
        if page.truncated {
            assert_eq!(paths.len(), MCP_PATHS_PAGE);
            assert_eq!(page.cursor.as_deref(), Some(MCP_CURSOR_PLACEHOLDER));
        } else {
            assert_eq!(page.cursor, None);
        }
        let last = paths.last().cloned();
        seen.extend(paths);
        if !page.truncated {
            break;
        }
        after = last;
    }
    assert_eq!(seen, unique, "every path once, in order");
    assert_eq!(pages, unique.len().div_ceil(MCP_PATHS_PAGE));

    // A page that ends exactly at the end of the list does not promise another one.
    for (n, truncated) in [(MCP_PATHS_PAGE, false), (MCP_PATHS_PAGE + 1, true)] {
        let (counts, changes, unique) = sorted_changes(n);
        let page = paths_page(of.clone(), counts, &changes, None);
        assert_eq!(page.truncated, truncated, "{n} paths");
        assert_eq!(page.paths.len(), n.min(MCP_PATHS_PAGE));
        assert_eq!(page.paths[0].raw(), unique[0]);
    }

    // The key is a position, not a member: a path that vanished between two pages does not
    // make the next one start over or skip.
    let (counts, changes, unique) = sorted_changes(100);
    let gone = format!("{}~", unique[39]);
    let page = paths_page(of.clone(), counts, &changes, Some(&gone));
    assert_eq!(page.paths[0].raw(), unique[40]);
    let end = paths_page(of, counts, &changes, unique.last().map(String::as_str));
    assert!(end.paths.is_empty() && !end.truncated && end.cursor.is_none());
}

/// D4: a cursor belongs to the table that minted it, has the form the parameter validates, can be
/// used again, and the table keeps no more than its bound, the oldest going first.
#[test]
fn cursors_belong_to_their_table_and_are_bounded() {
    let worktrees = |n: usize| CursorEntry::Worktrees {
        repo_id: "r1".into(),
        after: Some(PathBuf::from(format!("/w/{n}"))),
    };
    let mut table = McpCursors::default();
    let mut other = McpCursors::default();

    let first = table.mint(worktrees(0)).unwrap();
    assert!(valid_cursor(&first), "{first}");
    assert_eq!(table.get(&first), Some(&worktrees(0)));
    // Reusable: a retry gets the same list.
    assert_eq!(table.get(&first), Some(&worktrees(0)));
    // Another connection's table never heard of it, and neither of what was never minted.
    assert_eq!(other.get(&first), None);
    assert_eq!(table.get(MCP_CURSOR_PLACEHOLDER), None);
    assert_eq!(table.get("not-a-cursor"), None);
    // The entry says which repo it is of, so a caller of another repo can be told apart.
    let paths = CursorEntry::Paths {
        repo_id: "r2".into(),
        root: PathBuf::from("/w/r2"),
        after: Some("src/a.rs".into()),
    };
    let minted = other.mint(paths.clone()).unwrap();
    assert_eq!(other.get(&minted), Some(&paths));
    assert_eq!(table.get(&minted), None);

    // Bounded: past 64 the oldest are dropped first and the newest all stay.
    let minted: Vec<String> = (1..=MCP_MAX_CURSORS + 6)
        .map(|n| table.mint(worktrees(n)).unwrap())
        .collect();
    let mut all = vec![first];
    all.extend(minted);
    let distinct: std::collections::BTreeSet<_> = all.iter().collect();
    assert_eq!(distinct.len(), all.len(), "a cursor was minted twice");
    let kept = all.iter().filter(|id| table.get(id).is_some()).count();
    assert_eq!(kept, MCP_MAX_CURSORS);
    let dropped = all.len() - MCP_MAX_CURSORS;
    assert!(all[..dropped].iter().all(|id| table.get(id).is_none()));
    assert!(all[dropped..].iter().all(|id| table.get(id).is_some()));
}
