//! INF-GRP-001, core: the minimum scenarios of ADR-GRP-009 (Validación 4) that the read layer can
//! exercise without the observer, run through the "intact repo" harness of `gitraptor-testkit`:
//! fingerprint of the repo, its worktrees and everything outside it (home, other repo, profile,
//! system Git config), control run where the user or an agent acts, zero differences otherwise.
//!
//! The scenarios that need the observer (US-GRP-002), the daemon (TS-GRP-003), the channel
//! (TS-GRP-004), autostart (US-GRP-004) and `~/.claude` (US-GRP-007) are suites of their own
//! stories, on top of this core (see the Dev Spec).

mod common;

mod repo_intact {
    use std::path::Path;
    use std::sync::OnceLock;

    use super::common::{invoker_for, read_everything_with, system_git};
    use gitraptor_git::{Invoker, ReadError, ReaderOptions, RepoReader, SystemGit};
    use gitraptor_testkit::{Exceptions, Fixture, Mode, Scenario, Step, check};

    fn git() -> &'static SystemGit {
        static GIT: OnceLock<SystemGit> = OnceLock::new();
        GIT.get_or_init(system_git)
    }

    fn invoker(f: &Fixture) -> Invoker {
        invoker_for(&f.home, "/usr/bin:/bin".as_ref())
    }

    /// The engine of this core: every read of the layer, gitoxide and CLI.
    fn read_all(f: &Fixture, path: &Path) {
        read_everything_with(git(), &invoker(f), path);
    }

    fn profile() -> Exceptions {
        Exceptions::engine_profile("profile")
    }

    fn busy() -> Fixture {
        let f = Fixture::busy(&git().path);
        f.git(&[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ]);
        f
    }

    /// `main` and `feature` both change `a.txt`: merging or rebasing stops on a conflict.
    fn diverged() -> Fixture {
        let f = Fixture::with_commit(&git().path);
        f.git(&["remote", "add", "origin", "https://example.com/o/r.git"]);
        f.git(&["checkout", "-q", "-b", "feature"]);
        f.write("a.txt", "feature side\n");
        f.git(&["commit", "-q", "-am", "feature"]);
        f.git(&["checkout", "-q", "main"]);
        f.write("a.txt", "main side\n");
        f.git(&["commit", "-q", "-am", "main"]);
        f
    }

    /// Run a Git command that is expected to stop (conflict); its exit status is irrelevant.
    fn git_may_fail(f: &Fixture, args: &[&str]) {
        let _ = f.git_command(&f.repo, args).output().unwrap();
    }

    // --- Observable acceptance: the harness catches an injected write; the real layer passes --

    #[test]
    fn read_layer_leaves_everything_intact() {
        let f = busy();
        check("read layer on a busy repo", &f, &profile(), || {
            read_all(&f, &f.repo)
        })
        .assert_intact();
    }

    #[test]
    fn injected_write_during_reads_is_caught() {
        let f = busy();
        let report = check("read layer plus an injected lock", &f, &profile(), || {
            read_all(&f, &f.repo);
            let lock = f.repo.join(".git/index.lock");
            std::fs::write(&lock, "").unwrap();
            std::fs::remove_file(&lock).unwrap();
        });
        assert!(!report.is_intact(), "{report}");
        let report = check("read layer plus a write in home", &f, &profile(), || {
            read_all(&f, &f.repo);
            std::fs::write(f.home.join(".gitconfig"), "[core]\n").unwrap();
        });
        assert!(
            report
                .imputable
                .iter()
                .any(|c| c.scope == "home" && c.path == Path::new(".gitconfig")),
            "{report}"
        );
    }

    // --- Minimum scenarios (ADR-GRP-009, Validación 4) ----------------------------------------

    #[test]
    fn linked_worktrees() {
        let f = busy();
        let wt = f.add_worktree("feature", "feature");
        std::fs::write(wt.join("c.txt"), "gamma dirty\n").unwrap();
        check("linked worktrees", &f, &profile(), || {
            read_all(&f, &f.repo);
            read_all(&f, &wt);
        })
        .assert_intact();
    }

    #[test]
    fn merge_in_progress() {
        let f = diverged();
        git_may_fail(&f, &["merge", "feature"]);
        assert!(f.repo.join(".git/MERGE_HEAD").exists());
        check("merge in progress", &f, &profile(), || {
            read_all(&f, &f.repo)
        })
        .assert_intact();
    }

    #[test]
    fn rebase_in_progress_with_detached_head() {
        let f = diverged();
        f.git(&["checkout", "-q", "feature"]);
        git_may_fail(&f, &["rebase", "main"]);
        assert!(f.repo.join(".git/rebase-merge").exists());
        check("rebase in progress", &f, &profile(), || {
            let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
            assert!(r.head().unwrap().detached);
            drop(r);
            read_all(&f, &f.repo);
        })
        .assert_intact();
    }

    #[test]
    fn detached_head() {
        let f = busy();
        f.git(&["checkout", "-q", "--detach", "feature"]);
        check("detached HEAD", &f, &profile(), || read_all(&f, &f.repo)).assert_intact();
    }

    #[test]
    fn fsmonitor_untracked_cache_and_split_index() {
        let f = busy();
        f.git(&["config", "core.fsmonitor", "true"]);
        f.git(&["config", "core.untrackedCache", "true"]);
        f.git(&["config", "core.splitIndex", "true"]);
        f.git(&["update-index", "--untracked-cache", "--split-index"]);
        f.write("more-untracked.txt", "u\n");
        f.dirty_stat("a.txt");
        check(
            "fsmonitor (daemon stopped), untracked cache, split index",
            &f,
            &profile(),
            || read_all(&f, &f.repo),
        )
        .assert_intact();
    }

    /// Control: the user's fsmonitor daemon is running while the engine reads. Its own writes
    /// (cookies, socket) appear in the control run too and are not imputed. Git has no builtin
    /// fsmonitor daemon on Linux.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn fsmonitor_daemon_running_is_not_imputed() {
        let report = Scenario::new("fsmonitor daemon running", || {
            let f = busy();
            f.git(&["config", "core.fsmonitor", "true"]);
            f
        })
        .step(Step::user(|f: &Fixture| {
            f.git(&["fsmonitor--daemon", "start"]);
            f.git(&["status", "--porcelain"]);
        }))
        .step(Step::engine(|f: &Fixture| read_all(f, &f.repo)))
        .step(Step::user(|f: &Fixture| {
            git_may_fail(f, &["fsmonitor--daemon", "stop"]);
        }))
        .exceptions(profile())
        .run();
        assert_eq!(report.mode, Mode::Subtracted);
        report.assert_intact();
    }

    /// Hooks of every kind present (marker scripts, unix); none runs. The full program set is
    /// the SEC-09 canary (`canary.rs`).
    #[cfg(unix)]
    #[test]
    fn hooks_present() {
        let f = busy();
        let markers = f.root.join("markers");
        std::fs::create_dir_all(&markers).unwrap();
        for hook in [
            "post-index-change",
            "reference-transaction",
            "post-checkout",
            "pre-commit",
        ] {
            gitraptor_testkit::canary::script(
                &f.repo.join(".git/hooks").join(hook),
                &format!(": > '{}'", markers.join(hook).display()),
            );
        }
        check("hooks present", &f, &profile(), || read_all(&f, &f.repo)).assert_intact();
        assert!(std::fs::read_dir(&markers).unwrap().next().is_none());
    }

    /// LFS-style filters (`filter=lfs`, required) with a pointer file whose stat is dirty. The
    /// LFS binary is never needed: the layer runs no filter.
    #[test]
    fn lfs_filters() {
        let f = busy();
        f.write(
            ".gitattributes",
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        );
        f.write(
            "asset.bin",
            "version https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000\nsize 1024\n",
        );
        f.git(&["add", ".gitattributes", "asset.bin"]);
        f.git(&["commit", "-q", "-m", "lfs pointer"]);
        f.git(&["config", "filter.lfs.clean", "git-lfs clean -- %f"]);
        f.git(&["config", "filter.lfs.smudge", "git-lfs smudge -- %f"]);
        f.git(&["config", "filter.lfs.process", "git-lfs filter-process"]);
        f.git(&["config", "filter.lfs.required", "true"]);
        f.dirty_stat("asset.bin");
        check("LFS filters", &f, &profile(), || read_all(&f, &f.repo)).assert_intact();
    }

    /// Signed commits with `log.showSignature` and a trace2 target configured: no `gpg`, no
    /// trace file. The armed version (programs that leave markers) is the SEC-09 canary.
    #[test]
    fn signatures_and_trace2_target() {
        let f = busy();
        let trace = f.root.join("trace");
        std::fs::create_dir_all(&trace).unwrap();
        f.git(&["config", "log.showSignature", "true"]);
        f.git(&["config", "gpg.program", "/nonexistent/gpg"]);
        let event = trace.join("event").display().to_string();
        let normal = trace.join("normal").display().to_string();
        f.git(&["config", "trace2.eventTarget", &event]);
        f.git(&["config", "trace2.normalTarget", &normal]);
        check("signatures and trace2", &f, &profile(), || {
            read_all(&f, &f.repo)
        })
        .assert_intact();
        assert!(std::fs::read_dir(&trace).unwrap().next().is_none());
    }

    /// The user's `gc` runs while the engine reads. Reads may fail ("not available"), never
    /// write; what `gc` writes is in the control run too.
    #[test]
    fn concurrent_user_gc() {
        let report = Scenario::new("user gc concurrent with reads", busy)
            .step(Step::concurrently(
                |f: &Fixture| {
                    f.git(&["gc", "-q", "--prune=now"]);
                },
                |f: &Fixture| {
                    for _ in 0..10 {
                        if let Ok(r) = RepoReader::open(&f.repo, &ReaderOptions::default()) {
                            let _ = r.status();
                            let _ = r.local_branches();
                        }
                    }
                },
            ))
            .exceptions(profile())
            .run();
        assert_eq!(report.mode, Mode::Subtracted);
        report.assert_intact();
    }

    /// Repo rejected by the `safe.directory` ownership rules: "not available", nothing written
    /// (no `safe.directory` added to any config). A repo of another uid needs a second user; it
    /// is simulated with reduced trust (partial coverage of SEC-11, see the Dev Spec).
    #[test]
    fn rejected_by_safe_directory() {
        let f = busy();
        check("safe.directory", &f, &profile(), || {
            let err = RepoReader::open(
                &f.repo,
                &ReaderOptions {
                    force_reduced_trust: true,
                },
            )
            .unwrap_err();
            assert!(matches!(err, ReadError::Untrusted(_)), "{err:?}");
        })
        .assert_intact();
    }

    // --- SEC-11 without the channel -------------------------------------------------------------

    /// A linked worktree whose `gitdir` was rewritten to point at the home directory. The read
    /// layer touches neither; deciding not to watch it is the observer's (US-GRP-002 suite).
    #[test]
    fn manipulated_gitdir_towards_home() {
        let f = busy();
        let wt = f.add_worktree("feature", "feature");
        let admin = f.repo.join(".git/worktrees/wt-feature");
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", f.home.join(".git").display()),
        )
        .unwrap();
        check("gitdir towards home", &f, &profile(), || {
            let r = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
            let _ = r.worktrees();
            drop(r);
            let _ = RepoReader::open(&wt, &ReaderOptions::default()).map(|r| r.status());
        })
        .assert_intact();
        assert!(!f.home.join(".git").exists());
    }

    /// `.gitraptor/settings.json` as a symlink to a secret. The read layer never follows it; the
    /// settings reader (ADR-GRP-007/008) adds its diagnostic check to this scenario when it
    /// exists.
    #[cfg(unix)]
    #[test]
    fn settings_symlink_to_a_secret() {
        let f = busy();
        std::fs::create_dir_all(f.home.join(".ssh")).unwrap();
        std::fs::write(f.home.join(".ssh/id_rsa"), "-----BEGIN SECRET-----\n").unwrap();
        std::fs::create_dir_all(f.repo.join(".gitraptor")).unwrap();
        std::os::unix::fs::symlink(
            f.home.join(".ssh/id_rsa"),
            f.repo.join(".gitraptor/settings.json"),
        )
        .unwrap();
        check("settings symlink", &f, &profile(), || read_all(&f, &f.repo)).assert_intact();
    }
}
