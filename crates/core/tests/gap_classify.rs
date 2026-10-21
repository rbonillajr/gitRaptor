//! US-GRP-005, D3 of its Dev Spec: `classify_gap` enumerates the commits of a moved branch only
//! when the reflog chain is continuous from the stored tip to the current one; otherwise one
//! `branch-update` and `collapsed += 1`. Temporary repos only (NFR-01).

mod common;

use std::path::{Path, PathBuf};

use common::*;
use gitraptor_api::messages::GitEventKind;
use gitraptor_core::watch::{GapClassification, RawEvent, RefsView, classify_gap};

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), "demo", true);
    let common = common_dir(&repo);
    (dir, repo, common)
}

fn commit(repo: &Path, msg: &str) -> String {
    git(repo, &["commit", "-q", "--allow-empty", "-m", msg]);
    git(repo, &["rev-parse", "HEAD"])
}

fn classify(common: &Path, before: &RefsView) -> GapClassification {
    let now = RefsView::read(common).unwrap();
    classify_gap(common, before, &now, (1_000, 0))
}

fn of_kind(c: &GapClassification, kind: GitEventKind) -> Vec<&RawEvent> {
    c.events.iter().filter(|e| e.kind == kind).collect()
}

fn branch_updates(c: &GapClassification, tip: &str, new_tip: &str) {
    assert_eq!(c.collapsed, 1, "{c:?}");
    assert!(of_kind(c, GitEventKind::Commit).is_empty(), "{c:?}");
    let updates = of_kind(c, GitEventKind::BranchUpdate);
    assert_eq!(updates.len(), 1, "{c:?}");
    assert_eq!(updates[0].details.old_commit.as_deref(), Some(tip));
    assert_eq!(updates[0].details.new_commit.as_deref(), Some(new_tip));
}

#[test]
fn a_continuous_chain_gives_one_event_per_reflog_entry() {
    let (_dir, repo, common) = setup();
    let before = RefsView::read(&common).unwrap();
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    let ids: Vec<String> = (1..=3).map(|n| commit(&repo, &format!("c{n}"))).collect();
    let c = classify(&common, &before);
    assert_eq!(c.collapsed, 0, "{c:?}");
    let commits = of_kind(&c, GitEventKind::Commit);
    assert_eq!(commits.len(), 3, "{c:?}");
    let mut prev = tip;
    for (e, id) in commits.iter().zip(&ids) {
        assert_eq!(e.details.old_commit.as_deref(), Some(prev.as_str()));
        assert_eq!(e.details.new_commit.as_deref(), Some(id.as_str()));
        prev = id.clone();
    }
}

#[test]
fn a_chain_that_does_not_reach_the_old_tip_collapses() {
    let (_dir, repo, common) = setup();
    let before = RefsView::read(&common).unwrap();
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    commit(&repo, "c1");
    let head = commit(&repo, "c2");
    git(&repo, &["reflog", "expire", "--expire=now", "--all"]);
    branch_updates(&classify(&common, &before), &tip, &head);
}

#[test]
fn a_chain_with_a_missing_middle_entry_collapses() {
    let (_dir, repo, common) = setup();
    let before = RefsView::read(&common).unwrap();
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    for n in 1..=2 {
        commit(&repo, &format!("c{n}"));
    }
    let head = commit(&repo, "c3");
    git(&repo, &["reflog", "delete", "refs/heads/main@{1}"]);
    branch_updates(&classify(&common, &before), &tip, &head);
}

#[test]
fn more_than_64_entries_collapse() {
    let (_dir, repo, common) = setup();
    let before = RefsView::read(&common).unwrap();
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    let mut head = tip.clone();
    for n in 1..=70 {
        head = commit(&repo, &format!("c{n}"));
    }
    branch_updates(&classify(&common, &before), &tip, &head);
}

#[test]
fn a_branch_created_in_the_gap_is_one_branch_create_with_its_tip() {
    let (_dir, repo, common) = setup();
    let before = RefsView::read(&common).unwrap();
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", "hotfix"]);
    let c = classify(&common, &before);
    assert_eq!(c.collapsed, 0, "{c:?}");
    assert_eq!(c.events.len(), 1, "{c:?}");
    let e = &c.events[0];
    assert_eq!(e.kind, GitEventKind::BranchCreate);
    assert_eq!(
        e.details.branch.as_ref().map(|b| b.raw().to_owned()),
        Some("hotfix".to_owned())
    );
    assert_eq!(e.details.new_commit.as_deref(), Some(tip.as_str()));
}

#[test]
fn a_deleted_branch_is_one_branch_delete() {
    let (_dir, repo, common) = setup();
    git(&repo, &["branch", "old-work"]);
    let tip = git(&repo, &["rev-parse", "old-work"]);
    let before = RefsView::read(&common).unwrap();
    git(&repo, &["branch", "-D", "old-work"]);
    let c = classify(&common, &before);
    assert_eq!(c.collapsed, 0, "{c:?}");
    assert_eq!(c.events.len(), 1, "{c:?}");
    let e = &c.events[0];
    assert_eq!(e.kind, GitEventKind::BranchDelete);
    assert_eq!(
        e.details.branch.as_ref().map(|b| b.raw().to_owned()),
        Some("old-work".to_owned())
    );
    assert_eq!(e.details.old_commit.as_deref(), Some(tip.as_str()));
}
