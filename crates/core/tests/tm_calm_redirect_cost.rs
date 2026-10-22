//! An idle repo stays calm, and cheap, with a foreign `git` alive that redirects into another
//! repo (fix/tm-restore-calm-mark, D20 as decided). `#[ignore]`: it measures and prints.
//!
//! Testkit fixtures and the real process table; never this repo or the real profile (NFR-01).
//! The foreign `git` (`git -C <other repo> hash-object --stdin`) waits on its standard input, so
//! it lives until the test closes it; it runs as a grandchild of the test, because the engine
//! leaves out its process's own children. Run with
//! `cargo test -p gitraptor-core --release --test tm_calm_redirect_cost -- --ignored --nocapture`.
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitraptor_core::detect::{ProcLister, SystemProcLister};
use gitraptor_core::timemachine::engine::{RepoMarks, git_dir_of};
use gitraptor_core::watch::{RefsView, read_local_branches};
use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;

const REPO: &str = "r";

#[test]
#[ignore = "measurement: run with --release --ignored --nocapture"]
fn an_idle_repo_with_a_foreign_redirecting_git_is_calm_and_cheap() {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let wt = f.repo.clone();
    let common = gitraptor_git::paths::canonicalize(&f.repo.join(".git")).unwrap();
    let marks = RepoMarks::default();
    let reader = RepoReader::open(&common, &ReaderOptions::default()).unwrap();
    marks.set_persisted(REPO, 1, &common, read_local_branches(&reader).unwrap());
    marks.set_head_logs(REPO, &RefsView::read(&common).unwrap().head_logs());
    let git_dir = git_dir_of(&wt).unwrap();
    marks.set_heads(
        REPO,
        &[(wt.clone(), std::fs::read(git_dir.join("HEAD")).unwrap())],
    );
    marks.set_procs(Arc::new(SystemProcLister) as Arc<dyn ProcLister>);

    // The foreign `git`, alive in another repo through `-C`.
    let mut foreign = Command::new("/bin/sh")
        .args(["-c", "\"$0\" -C \"$1\" hash-object --stdin; exit $?"])
        .arg(&f.git)
        .arg(&f.other_repo)
        .current_dir(&f.home)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &f.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let mut samples: Vec<u128> = (0..200)
        .map(|_| {
            let t = Instant::now();
            assert!(
                marks
                    .settle(REPO, std::slice::from_ref(&wt), Duration::ZERO)
                    .is_some(),
                "a git redirected into another repo leaves this one calm"
            );
            t.elapsed().as_micros()
        })
        .collect();
    drop(foreign.stdin.take());
    assert!(foreign.wait().unwrap().success());
    samples.sort_unstable();
    println!(
        "calm pass p95 with a foreign redirecting git: {} us",
        samples[samples.len() * 95 / 100]
    );
}
