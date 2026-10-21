//! What one calm pass costs with the real process table (fix/tm-restore-calm-mark, D21, C27).
//! `#[ignore]`: it measures and prints; past 5 ms the process list is cached for one poll.
//!
//! A testkit fixture; never this repo or the real profile (NFR-01). Run with
//! `cargo test -p gitraptor-core --release --test tm_calm_probe_cost -- --ignored --nocapture`.

use std::path::PathBuf;
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
fn one_calm_pass_with_800_processes_is_cheap() {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let wt: PathBuf = f.repo.clone();
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
    let lister = Arc::new(SystemProcLister);
    let processes = lister.list_bare().map_or(0, |l| l.len());
    marks.set_procs(lister as Arc<dyn ProcLister>);
    let mut samples: Vec<u128> = (0..200)
        .map(|_| {
            let t = Instant::now();
            assert!(
                marks
                    .settle(REPO, std::slice::from_ref(&wt), Duration::ZERO)
                    .is_some(),
                "the idle repo is calm"
            );
            t.elapsed().as_micros()
        })
        .collect();
    samples.sort_unstable();
    let p95 = samples[samples.len() * 95 / 100];
    println!("calm pass p95 with {processes} processes: {p95} us");
}
