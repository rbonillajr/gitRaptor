//! SEC-09 canary repo: every program a user or a repository can configure points to a script
//! that leaves a marker. No read of the layer may run any of them (ADR-GRP-009, Validación 7).
//! The canary itself lives in `gitraptor-testkit` (INF-GRP-001), reusable by every crate.
#![cfg(unix)]

mod common;

use common::{invoker_for, read_everything_with, system_git};
use gitraptor_git::cli::GitCli;
use gitraptor_git::{ChangeKind, ReaderOptions, RefName, RepoReader};
use gitraptor_testkit::canary::{Canary, PROGRAMS};
use gitraptor_testkit::{Exceptions, Fixture, check};

fn canary() -> Canary {
    Canary::arm(Fixture::busy(&system_git().path))
}

mod repo_intact {
    use super::*;

    #[test]
    fn clean_filter_never_runs_and_file_reported_modified() {
        let c = canary();
        let r = RepoReader::open(&c.f.repo, &ReaderOptions::default()).unwrap();
        let status = r.status().unwrap();
        assert!(
            status
                .unstaged
                .iter()
                .any(|ch| ch.path == "a.txt" && ch.kind == ChangeKind::Modified),
            "a.txt must be reported modified without running the filter: {status:?}"
        );
        drop(r);
        assert!(!c.marker("filter-clean").exists());
        assert!(c.fired().is_empty(), "fired: {:?}", c.fired());
    }

    #[test]
    fn user_programs_never_run() {
        let c = canary();
        let git = system_git();
        let invoker = invoker_for(&c.f.home, "/usr/bin:/bin".as_ref());
        check(
            "reads of the canary repo",
            &c.f,
            &Exceptions::none(),
            || {
                read_everything_with(&git, &invoker, &c.f.repo);
                let cli = GitCli::new(&git, &invoker, &c.f.repo).unwrap();
                let log = cli.log(&RefName::new("main").unwrap(), 5).unwrap();
                assert_eq!(log[0].subject, "signed");
            },
        )
        .assert_intact();
        assert!(c.fired().is_empty(), "fired: {:?}", c.fired());
        for p in PROGRAMS {
            assert!(!c.marker(p).exists(), "{p} ran");
        }
    }

    /// Control run: the canary is armed. Plain `git status` runs the filter (`process` wins over
    /// `clean`, and its failure aborts the required filter) and `git log` with
    /// `log.showSignature` runs `gpg.program`.
    #[test]
    fn canary_is_armed_for_plain_git() {
        let c = canary();
        let _ =
            c.f.git_command(&c.f.repo, &["status", "--porcelain"])
                .output()
                .unwrap();
        assert!(
            c.marker("filter-process").exists(),
            "fired: {:?}",
            c.fired()
        );
        let _ = c.f.git_command(&c.f.repo, &["log", "-1"]).output().unwrap();
        assert!(c.marker("gpg").exists(), "fired: {:?}", c.fired());
    }
}
