//! The harness itself: sensitivity, exceptions, control run and guard (INF-GRP-001). Test names
//! live under `repo_intact::` so the single CI gate selects them.

mod repo_intact {
    use std::path::Path;

    use gitraptor_testkit::exceptions::without_keys;
    use gitraptor_testkit::fixture::git_from_path;
    use gitraptor_testkit::guard::{self, GuardError};
    use gitraptor_testkit::{
        ChangeKind, Exception, Exceptions, Field, Fixture, Mode, Scenario, Step, check,
    };

    fn busy() -> Fixture {
        Fixture::busy(&git_from_path())
    }

    // --- Sensitivity: an injected write makes the harness fail -------------------------------

    #[test]
    fn sensitivity_lock_created_and_deleted_is_detected() {
        let f = busy();
        let report = check("lock in .git", &f, &Exceptions::none(), || {
            let lock = f.repo.join(".git/index.lock");
            std::fs::write(&lock, "").unwrap();
            std::fs::remove_file(&lock).unwrap();
        });
        assert!(!report.is_intact());
        assert!(
            report
                .imputable
                .iter()
                .any(|c| c.scope == "repo" && c.path == Path::new(".git")),
            "{report}"
        );
        let text = report.to_string();
        assert!(text.contains("lock in .git") && text.contains("[repo] .git: modified"));
    }

    #[test]
    fn sensitivity_write_outside_profile_is_detected() {
        let f = busy();
        let report = check(
            "write in home",
            &f,
            &Exceptions::engine_profile("profile"),
            || std::fs::write(f.home.join(".gnupg/trustdb.gpg"), "x").unwrap(),
        );
        assert!(
            report.imputable.iter().any(|c| c.scope == "home"
                && c.path == Path::new(".gnupg/trustdb.gpg")
                && c.kind == ChangeKind::Created),
            "{report}"
        );
    }

    #[test]
    fn sensitivity_touch_and_chmod_are_detected() {
        let f = busy();
        let report = check("touch", &f, &Exceptions::none(), || f.dirty_stat("a.txt"));
        assert!(
            report.imputable.iter().any(|c| matches!(&c.kind,
                ChangeKind::Modified(fields) if fields.contains(&Field::Mtime))),
            "{report}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let report = check("chmod", &f, &Exceptions::none(), || {
                std::fs::set_permissions(
                    f.repo.join("b.txt"),
                    std::fs::Permissions::from_mode(0o600),
                )
                .unwrap();
            });
            assert!(
                report.imputable.iter().any(|c| matches!(&c.kind,
                    ChangeKind::Modified(fields) if fields.contains(&Field::Mode))),
                "{report}"
            );
        }
    }

    #[test]
    fn sensitivity_system_config_is_fingerprinted_when_present() {
        let f = busy();
        if let Some(sys) = &f.system_config {
            let snap = f.fingerprint();
            assert!(
                snap.get("system-gitconfig", Path::new("")).is_some(),
                "{sys:?}"
            );
        }
    }

    #[test]
    fn untouched_fixture_is_intact() {
        let f = busy();
        check("nothing", &f, &Exceptions::none(), || {}).assert_intact();
    }

    // --- Exceptions per scenario ---------------------------------------------------------------

    #[test]
    fn engine_profile_allows_data_and_state_but_not_profile_config() {
        let f = busy();
        let ex = Exceptions::engine_profile("profile");
        check("engine data", &f, &ex, || {
            std::fs::write(f.profile.join("data/index.db"), "db").unwrap();
            std::fs::write(f.profile.join("state/engine.log"), "log").unwrap();
        })
        .assert_intact();
        let report = check("profile config", &f, &ex, || {
            std::fs::write(f.profile.join("config/config.toml"), "changed").unwrap();
        });
        assert_eq!(report.imputable.len(), 1, "{report}");
        assert_eq!(report.imputable[0].path, Path::new("config/config.toml"));
    }

    #[test]
    fn autostart_exception_allows_exactly_its_artifacts() {
        let f = busy();
        std::fs::create_dir_all(f.home.join("Library/LaunchAgents")).unwrap();
        let agent = Path::new("Library/LaunchAgents/dev.gitraptor.plist");
        let ex = Exceptions::autostart(&[("home", agent)]);
        check("enable", &f, &ex, || {
            std::fs::write(f.home.join(agent), "<plist/>").unwrap();
        })
        .assert_intact();
        let report = check("enable plus extra", &f, &ex, || {
            std::fs::write(f.home.join("Library/LaunchAgents/other.plist"), "x").unwrap();
        });
        assert_eq!(report.imputable.len(), 1, "{report}");
    }

    #[test]
    fn guardrails_install_allows_only_hookspath_and_its_folder() {
        let f = busy();
        let common = Path::new(".git");
        let ex = Exceptions::guardrails_install("repo", common);
        let install = || {
            f.git(&["config", "core.hooksPath", ".git/gitraptor/hooks"]);
            std::fs::create_dir_all(f.repo.join(".git/gitraptor/hooks")).unwrap();
            std::fs::write(f.repo.join(".git/gitraptor/hooks/pre-commit"), "x").unwrap();
        };
        let before = f.snapshot(&ex);
        install();
        let after = f.snapshot(&ex);
        let changes = gitraptor_testkit::diff(&before, &after);
        assert!(!changes.is_empty());
        assert_eq!(ex.filter(&changes, &before, &after), vec![]);

        // Any other key in the same file is a difference.
        let report = check("install plus another key", &f, &ex, || {
            f.git(&["config", "core.hooksPath", ".git/gitraptor/v2"]);
            f.git(&["config", "core.autocrlf", "true"]);
        });
        assert!(
            report
                .imputable
                .iter()
                .any(|c| c.path == common.join("config")),
            "{report}"
        );

        // After uninstall, compared with the snapshot before install: zero differences.
        let f = busy();
        let ex = Exceptions::guardrails_uninstalled("repo", common);
        let before = f.snapshot(&ex);
        f.git(&["config", "core.hooksPath", ".git/gitraptor/hooks"]);
        std::fs::create_dir_all(f.repo.join(".git/gitraptor")).unwrap();
        std::fs::remove_dir(f.repo.join(".git/gitraptor")).unwrap();
        f.git(&["config", "--unset", "core.hooksPath"]);
        // `git config --unset` may leave an empty `[core]`; it is not a key.
        let after = f.snapshot(&ex);
        let changes = gitraptor_testkit::diff(&before, &after);
        assert_eq!(ex.filter(&changes, &before, &after), vec![]);
    }

    #[test]
    fn config_comparison_ignores_only_listed_keys() {
        let a = b"[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n";
        let b = b"[core]\n\tbare = false\n\thooksPath = h\n[remote \"origin\"]\n\turl = x\n";
        let keys = vec!["core.hooksPath".to_string()];
        assert_eq!(without_keys(a, &keys), without_keys(b, &keys));
        assert_ne!(without_keys(a, &[]), without_keys(b, &[]));
        let c = b"[remote \"origin\"]\n\turl = y\n";
        assert_ne!(without_keys(a, &keys), without_keys(c, &keys));
    }

    #[test]
    fn exceptions_never_apply_outside_their_scope() {
        let f = busy();
        let ex = Exceptions::none().with(Exception::Subtree {
            scope: "profile".into(),
            prefix: "data".into(),
        });
        let report = check("data folder in home", &f, &ex, || {
            std::fs::create_dir_all(f.home.join("data")).unwrap();
        });
        assert!(!report.is_intact());
    }

    // --- Control run ---------------------------------------------------------------------------

    /// An agent commit triggers `gc --auto` (user config). The engine reads nothing here, so the
    /// whole difference is the control's: nothing is imputed. Recent Git runs a geometric
    /// maintenance strategy after a commit by default, which ignores `gc.auto`; a user who wants
    /// `gc` sets `maintenance.strategy=gc`, which older Git ignores. The fixture home turns
    /// automatic maintenance off; this repo turns it back on.
    #[test]
    fn control_gc_auto_by_agent_commit_is_not_imputed() {
        let git = git_from_path();
        let build = || {
            let f = Fixture::with_commit(&git);
            f.git(&["config", "maintenance.auto", "true"]);
            f.git(&["config", "gc.auto", "1"]);
            f.git(&["config", "gc.autoDetach", "false"]);
            f.git(&["config", "maintenance.strategy", "gc"]);
            f
        };
        // One commit with enough loose objects to cross `gc.auto` (Git samples `objects/17`).
        let agent_commit = |f: &Fixture| {
            for i in 0..1500 {
                f.write(&format!("gen/f{i}.txt"), &format!("file {i}\n"));
            }
            f.git(&["add", "."]);
            f.git(&["commit", "-q", "-m", "agent"]);
        };
        let report = Scenario::new("gc --auto by an agent commit", build)
            .step(Step::user(agent_commit))
            .step(Step::engine(|_| {}))
            .run();
        assert_eq!(report.mode, Mode::Subtracted);
        assert!(
            report
                .control
                .iter()
                .any(|c| c.path.starts_with(".git/objects/pack") && c.kind == ChangeKind::Created),
            "gc --auto did not run: {report}"
        );
        report.assert_intact();
    }

    /// No background maintenance of a fixture lands in a fingerprint (`maintenance.lock`).
    #[test]
    fn fixture_turns_automatic_maintenance_off() {
        let f = Fixture::with_commit(&git_from_path());
        let get = |k: &str| f.git(&["config", "--get", k]).trim().to_owned();
        assert_eq!(get("maintenance.auto"), "false");
        assert_eq!(get("maintenance.autoDetach"), "false");
        assert_eq!(get("gc.auto"), "0");
        assert_eq!(get("gc.autoDetach"), "false");
    }

    #[test]
    fn control_still_imputes_an_engine_write() {
        let git = git_from_path();
        let report = Scenario::new("engine writes beside a commit", || {
            Fixture::with_commit(&git)
        })
        .step(Step::user(|f: &Fixture| {
            f.write("n.txt", "n\n");
            f.git(&["add", "."]);
            f.git(&["commit", "-q", "-m", "agent"]);
        }))
        .step(Step::engine(|f: &Fixture| {
            std::fs::write(f.repo.join(".git/gitraptor-cache"), "x").unwrap();
        }))
        .run();
        assert_eq!(report.mode, Mode::Subtracted);
        assert!(
            report
                .imputable
                .iter()
                .any(|c| c.path == Path::new(".git/gitraptor-cache")),
            "{report}"
        );
    }

    #[test]
    fn control_is_strict_without_user_activity() {
        let git = git_from_path();
        let report = Scenario::new("engine only", || Fixture::busy(&git))
            .step(Step::engine(|f: &Fixture| {
                let lock = f.repo.join(".git/index.lock");
                std::fs::write(&lock, "").unwrap();
                std::fs::remove_file(lock).unwrap();
            }))
            .run();
        assert_eq!(report.mode, Mode::Strict);
        assert!(report.control.is_empty());
        assert!(!report.is_intact(), "{report}");
    }

    // --- Guard ---------------------------------------------------------------------------------

    #[test]
    fn guard_refuses_the_gitraptor_repo() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for p in [
            workspace.clone(),
            workspace.join("crates/git"),
            workspace.join(".git"),
        ] {
            assert!(
                matches!(guard::check(&p), Err(GuardError::InsideGitRaptorRepo(_))),
                "{p:?}"
            );
        }
        for root in guard::gitraptor_roots() {
            assert!(guard::check_not_forbidden(&root).is_err());
        }
    }

    #[test]
    fn guard_refuses_home_and_root_and_unmarked_dirs() {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
        assert!(matches!(
            guard::check_not_forbidden(Path::new(&home)),
            Err(GuardError::CoversHome(_)) | Err(GuardError::InsideGitRaptorRepo(_))
        ));
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        assert!(guard::check_not_forbidden(Path::new(root)).is_err());
        let unmarked = tempfile::tempdir().unwrap();
        assert!(matches!(
            guard::check(unmarked.path()),
            Err(GuardError::NotATestkitRoot(_))
        ));
    }

    #[test]
    #[should_panic(expected = "harness guard")]
    fn snapshot_of_the_gitraptor_repo_panics() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        gitraptor_testkit::Snapshot::of_dir(&workspace);
    }

    #[test]
    fn fixture_roots_pass_the_guard() {
        let f = busy();
        for scope in f.scopes().iter().filter(|s| !s.system) {
            guard::check(&scope.root).unwrap();
        }
    }
}
