//! What the "no calm while a `git` runs" rule leaves out, what it costs and what it says
//! (fix/tm-restore-calm-mark, D17-D23): the hook's own `git` and call, a `git -C` from another
//! folder, long-lived `git` helpers, the order of the checks and the log of a blocked `settle`.
//!
//! Testkit fixtures and the synthetic process table of `calm_git_alive_tests`; never this repo or
//! the real profile (NFR-01). Nothing waits on time.

use std::sync::atomic::Ordering;

use super::calm_git_alive_tests::{GIT, REPO, rig};
use super::*;
use crate::daemon::{LOG_FILE, LogLimits, Logger};

/// C23: the hook's own `git` and its own call do not block its `settle`; without exclusions they
/// do, and another hook call in flight does too.
#[test]
fn excluded_pids_and_own_hook_do_not_block_their_own_settle() {
    let r = rig();
    r.procs.add(42, 1, GIT, Some(&r.wt), &["commit"]);
    let hook = r.marks.hook_begin(REPO, &r.wt);
    let wts = std::slice::from_ref(&r.wt);
    let own = CalmExclusions {
        pids: &[42],
        own_hook: Some(hook.token()),
    };
    assert_eq!(
        r.marks.settle_excluding(REPO, wts, Duration::ZERO, &own),
        Some(1),
        "the hook's own git and call"
    );
    assert_eq!(
        r.marks
            .settle_excluding(REPO, wts, Duration::ZERO, &CalmExclusions::default()),
        None,
        "without exclusions the git counts"
    );
    let other = r.marks.hook_begin(REPO, &r.wt);
    assert_eq!(
        r.marks.settle_excluding(REPO, wts, Duration::ZERO, &own),
        None,
        "another hook call in flight"
    );
    drop(other);
    r.procs.kill(42);
    assert_eq!(
        r.marks
            .settle_excluding(REPO, wts, Duration::ZERO, &CalmExclusions::default()),
        None,
        "the hook call itself, while in flight"
    );
    drop(hook);
    assert_eq!(r.settle_now(), Some(1), "nothing in flight");
}

/// C25: a `git` working elsewhere counts if it redirects to the repo (`-C`, `GIT_DIR`), or if
/// that cannot be told; one that does not redirect does not.
#[test]
fn a_git_dash_c_from_another_cwd_counts_in_scope() {
    let r = rig();
    r.procs
        .add(50, 1, GIT, Some(&r.f.home), &["-C", "repo", "reset"]);
    r.procs.redirect(50, Some(true));
    assert_eq!(r.settle_now(), None, "git -C from another folder");
    r.procs.redirect(50, None);
    assert_eq!(
        r.settle_now(),
        Some(1),
        "its arguments cannot be read, from a folder outside the repo"
    );
    r.procs.redirect(50, Some(false));
    assert_eq!(r.settle_now(), Some(1), "a git elsewhere, not redirected");
}

/// C26: long-lived `git` helpers that never change a worktree do not hold its calm. Each case has
/// its control: a plain `git` in the same place does.
mod long_lived_git_daemons_do_not_block_the_calm {
    use super::*;

    fn control(r: &super::super::calm_git_alive_tests::Rig) {
        r.procs.add(60, 1, GIT, Some(&r.wt), &["status"]);
        assert_eq!(r.settle_now(), None, "control: a plain git counts");
        r.procs.kill(60);
    }

    #[test]
    fn fsmonitor_daemon_is_excluded() {
        let r = rig();
        control(&r);
        r.procs
            .add(61, 1, GIT, Some(&r.wt), &["fsmonitor--daemon", "run"]);
        assert_eq!(r.settle_now(), Some(1));
    }

    #[test]
    fn cat_file_batch_is_excluded() {
        let r = rig();
        control(&r);
        for (pid, flag) in [
            (62, "--batch"),
            (63, "--batch-check"),
            (64, "--batch-command"),
        ] {
            r.procs.add(pid, 1, GIT, Some(&r.wt), &["cat-file", flag]);
        }
        assert_eq!(r.settle_now(), Some(1));
    }

    #[test]
    fn credential_cache_daemon_is_not_named_git() {
        let r = rig();
        control(&r);
        r.procs.add(
            65,
            1,
            "/usr/libexec/git-core/git-credential-cache--daemon",
            Some(&r.wt),
            &["/tmp/socket"],
        );
        assert_eq!(r.settle_now(), Some(1));
    }
}

/// C28: the process table is read only when every cheap check says calm.
#[test]
fn the_process_probe_runs_only_when_cheap_checks_say_calm() {
    let r = rig();
    assert_eq!(r.settle_now(), Some(1));
    assert!(
        r.procs.bare_calls.load(Ordering::SeqCst) >= 1,
        "a calm repo reads the processes"
    );
    let git_dir = git_dir_of(&r.wt).unwrap();
    std::fs::write(git_dir.join("index.lock"), "").unwrap();
    let before = r.procs.bare_calls.load(Ordering::SeqCst);
    assert_eq!(r.settle_now(), None);
    assert_eq!(
        r.procs.bare_calls.load(Ordering::SeqCst),
        before,
        "a held index answers before any process is read"
    );
}

/// C29: a `settle` that runs out its limit because of a `git` logs it once, with the pid and the
/// subcommand only.
#[test]
fn a_settle_blocked_by_git_logs_pid_and_subcommand_only() {
    let r = rig();
    let dirs = crate::profile::ProfileDirs::under_root(&r.f.profile);
    r.marks
        .set_logger(Logger::open(&dirs.state, LogLimits::default()).unwrap());
    r.procs.add(
        77,
        1,
        GIT,
        Some(&r.wt),
        &["status", "--porcelain=v2", "s3cr3t-arg"],
    );
    assert_eq!(
        r.marks
            .settle(REPO, std::slice::from_ref(&r.wt), Duration::from_millis(30)),
        None
    );
    let log = std::fs::read_to_string(dirs.state.join(LOG_FILE)).unwrap_or_default();
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("tm_calm_blocked_by_git"))
        .collect();
    assert_eq!(lines.len(), 1, "once per settle:\n{log}");
    assert!(
        lines[0].contains("77") && lines[0].contains("status"),
        "{}",
        lines[0]
    );
    assert!(
        !log.contains("s3cr3t-arg") && !log.contains("porcelain"),
        "nothing else of the command line:\n{log}"
    );
}
