//! TD-GRP-007 end to end (GRP007-WIRE): the daemon's own writes leave a write window over the
//! repo in `DaemonWrites::global()`. A real daemon in this process, real Git, temporary repos and
//! profile (NFR-01), the harness of `us_tmc_013_common`. Debug builds only (confirmation seam).
//!
//! Windows are counted by the temporary folder of each test (repo and worktrees live under the
//! fixture root), so tests of this binary that run in parallel never see each other's.
#![cfg(all(debug_assertions, any(target_os = "macos", target_os = "linux")))]

mod common;
mod us_tmc_013_common;

use std::path::Path;

use gitraptor_api::methods;
use gitraptor_api::timemachine::UndoResult;
use gitraptor_core::detect::daemon_writes::DaemonWrites;
use serde_json::Value;
use us_tmc_013_common::{Running, params, repo_with_login, start};

fn windows_under(root: &Path) -> usize {
    DaemonWrites::global().count(&|p| p.starts_with(root))
}

/// Scenario 1 of `us_tmc_003.rs`: an unattributed operation deletes the staged `b.rs`.
fn running_with_staged_b() -> Running {
    let (fx, wt) = repo_with_login();
    std::fs::write(wt.join("b.rs"), "fn b() {}\n").unwrap();
    fx.git_in(&wt, &["add", "b.rs"]);
    start(fx, wt, None, None)
}

#[test]
fn a_developers_undo_leaves_a_write_window_of_the_daemon_over_the_repo() {
    let r = running_with_staged_b();
    r.reset_hard();
    assert!(
        !r.worktree.join("b.rs").exists(),
        "the operation deleted b.rs"
    );
    let mut c = r.client();
    let before = windows_under(&r.fx.root);

    let value: Value = c
        .call(methods::TM_UNDO, params(&r.worktree, None))
        .unwrap_or_else(|e| panic!("the undo failed: {e:?}"));
    let _undo: UndoResult = serde_json::from_value(value).expect("an UndoResult");

    assert!(r.worktree.join("b.rs").exists(), "the undo recovered b.rs");
    let after = windows_under(&r.fx.root);
    assert!(
        after > before,
        "GRP007-WIRE: the Time Machine's applier must open a write window of the daemon over the repo (before {before}, after {after})"
    );
}

#[test]
fn an_executor_operation_that_runs_git_leaves_a_write_window_of_the_daemon_over_the_repo() {
    let r = running_with_staged_b();
    let before = windows_under(&r.fx.root);

    r.reset_hard();

    assert!(
        !r.worktree.join("b.rs").exists(),
        "the operation ran its git reset --hard"
    );
    let after = windows_under(&r.fx.root);
    assert!(
        after > before,
        "GRP007-WIRE: run_git_with must open a write window of the daemon over the worktree (before {before}, after {after})"
    );
}
