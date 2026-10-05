//! The Guardrails hook-layer harness itself (INF-GRD-001, núcleo): prior hooks, install footprint,
//! cut points and the interceptability matrix. Test names live under `repo_intact::` so the single
//! CI gate selects them.
//!
//! The product's hook layer does not exist yet (US-GRD-001). To prove the harness, these tests
//! drive a **reference installer** that only exists here: it follows the steps of ADR-GRD-001 § 4
//! with plain files and the Git CLI, in a child process (this same test binary re-executed), so it
//! can be killed at every cut point. Its broken variants are the negative tests.

mod repo_intact {
    pub mod hooks {
        use std::path::{Path, PathBuf};
        use std::process::Command;

        use gitraptor_testkit::cut::{
            self, Baseline, CutPoint, EndState, Exit, Outcome, Sweep, When,
        };
        use gitraptor_testkit::exceptions::without_keys;
        use gitraptor_testkit::fixture::git_from_path;
        use gitraptor_testkit::hooks::{CoverageBlocker, PriorHooks, write_script};
        use gitraptor_testkit::{ChangeKind, Exceptions, Fixture, check};

        const CHILD_OP: &str = "TESTKIT_REFINST_OP";
        const CHILD_VARIANT: &str = "TESTKIT_REFINST_VARIANT";
        const CHILD_ROOT: &str = "TESTKIT_REFINST_ROOT";
        const CHILD_GIT: &str = "TESTKIT_REFINST_GIT";

        const INSTALL_STEPS: &[&str] = &[
            "precheck",
            "journal",
            "folder-temp",
            "folder-rename",
            "key",
            "verify",
            "close",
        ];
        const UNINSTALL_STEPS: &[&str] = &["journal", "key", "folder", "close"];

        fn git() -> PathBuf {
            git_from_path()
        }

        fn install_exceptions() -> Exceptions {
            Exceptions::guardrails_install("repo", Path::new(".git"))
                .and(Exceptions::engine_profile("profile"))
        }

        fn uninstalled_exceptions() -> Exceptions {
            Exceptions::guardrails_uninstalled("repo", Path::new(".git"))
                .and(Exceptions::engine_profile("profile"))
        }

        // --- Reference installer (child process) ----------------------------------------------

        /// What the child process sees: the fixture's paths and Git, rebuilt from the environment.
        struct Machine {
            git: PathBuf,
            home: PathBuf,
            repo: PathBuf,
            common: PathBuf,
            journal: PathBuf,
        }

        impl Machine {
            fn from_env() -> Self {
                let root = PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
                let repo = root.join("repo");
                Self {
                    git: PathBuf::from(std::env::var_os(CHILD_GIT).unwrap()),
                    home: root.join("home"),
                    common: repo.join(".git"),
                    repo,
                    journal: root.join("profile/state/guardrails/journal.txt"),
                }
            }

            fn git_cmd(&self, dir: &Path, args: &[&str]) -> Command {
                let mut c = Command::new(&self.git);
                c.args(args).current_dir(dir);
                if cfg!(unix) {
                    c.env_clear().env("PATH", "/usr/bin:/bin");
                } else {
                    c.env("USERPROFILE", &self.home);
                }
                c.env("HOME", &self.home).env("GIT_CONFIG_NOSYSTEM", "1");
                c
            }

            fn git(&self, dir: &Path, args: &[&str]) -> Option<String> {
                let out = self.git_cmd(dir, args).output().unwrap();
                out.status
                    .success()
                    .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            }

            fn folder(&self) -> PathBuf {
                self.common.join("gitraptor")
            }

            fn hooks_value(&self) -> String {
                slash(&self.folder().join("hooks"))
            }

            fn local_value(&self) -> Option<String> {
                let config = slash(&self.common.join("config"));
                self.git(
                    &self.repo,
                    &["config", "--file", &config, "--get", "core.hooksPath"],
                )
            }

            fn read_journal(&self) -> Vec<(String, String)> {
                std::fs::read_to_string(&self.journal)
                    .unwrap_or_default()
                    .lines()
                    .filter_map(|l| l.split_once('='))
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect()
            }

            fn journal_get(&self, key: &str) -> Option<String> {
                self.read_journal()
                    .into_iter()
                    .rev()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v)
            }

            fn journal_set(&self, entries: &[(&str, &str)]) {
                std::fs::create_dir_all(self.journal.parent().unwrap()).unwrap();
                let mut all = self.read_journal();
                for (k, v) in entries {
                    all.retain(|(ek, _)| ek != k);
                    all.push(((*k).to_owned(), (*v).to_owned()));
                }
                let text: String = all.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
                std::fs::write(&self.journal, text).unwrap();
            }

            /// Remove the Guardrails folder (or a temp) file by file, never recursively.
            fn remove_folder(&self, dir: &Path) {
                let hooks = dir.join("hooks");
                if let Ok(entries) = std::fs::read_dir(&hooks) {
                    for e in entries.flatten() {
                        std::fs::remove_file(e.path()).unwrap();
                    }
                }
                let _ = std::fs::remove_dir(&hooks);
                let _ = std::fs::remove_dir(dir);
            }

            /// The previous hooks directory and its hook names.
            fn prior(&self) -> (String, Vec<String>) {
                let value = self.git(&self.repo, &["config", "--get", "core.hooksPath"]);
                let dir = match &value {
                    Some(v) if Path::new(v).is_absolute() => PathBuf::from(v),
                    Some(v) => self.repo.join(v),
                    None => self.common.join("hooks"),
                };
                let mut names: Vec<String> = std::fs::read_dir(&dir)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| !n.ends_with(".sample") && !n.starts_with('.') && n != "h")
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                let constant = value.unwrap_or_else(|| slash(&self.common.join("hooks")));
                (constant, names)
            }
        }

        fn slash(p: &Path) -> String {
            p.to_string_lossy().replace('\\', "/")
        }

        fn install(m: &Machine, variant: &str) {
            cut::trip("precheck", When::Before);
            let prior_local = m.local_value();
            let (prior_dir, prior_names) = m.prior();
            cut::trip("precheck", When::After);

            cut::trip("journal", When::Before);
            let tmp_name = format!("gitraptor.tmp-{}", std::process::id());
            m.journal_set(&[
                ("op", "install"),
                ("state", "started"),
                ("tmp", &tmp_name),
                ("prior_local", prior_local.as_deref().unwrap_or("")),
            ]);
            cut::trip("journal", When::After);

            cut::trip("folder-temp", When::Before);
            let tmp = m.common.join(&tmp_name);
            let mut names: Vec<String> = ["pre-push", "pre-rebase", "reference-transaction"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            for n in prior_names {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            for name in &names {
                write_script(
                    &tmp.join("hooks").join(name),
                    &format!(
                        "#!/bin/sh\np='{prior_dir}/{name}'\n[ -x \"$p\" ] || exit 0\nexec \"$p\" \"$@\"\n"
                    ),
                );
            }
            cut::trip("folder-temp", When::After);

            cut::trip("folder-rename", When::Before);
            std::fs::rename(&tmp, m.folder()).unwrap();
            cut::trip("folder-rename", When::After);

            if variant == "tamper-prior" {
                let p = m.common.join("hooks/pre-commit");
                let mut text = std::fs::read(&p).unwrap();
                text.push(b'\n');
                std::fs::write(p, text).unwrap();
            }
            if variant == "write-global" {
                let mut text = std::fs::read_to_string(m.home.join(".gitconfig")).unwrap();
                text.push_str("[gitraptor]\n\tinstalled = true\n");
                std::fs::write(m.home.join(".gitconfig"), text).unwrap();
            }

            cut::trip("key", When::Before);
            let config = slash(&m.common.join("config"));
            cut::trip_during("key", || {
                // What a `git config` killed mid-write leaves: its lock, not yet renamed.
                let mut text = std::fs::read(m.common.join("config")).unwrap();
                text.extend_from_slice(b"[core]\n\thooksPath = ");
                std::fs::write(m.common.join("config.lock"), text).unwrap();
            });
            m.git(
                &m.repo,
                &[
                    "config",
                    "--file",
                    &config,
                    "core.hooksPath",
                    &m.hooks_value(),
                ],
            )
            .expect("write core.hooksPath");
            cut::trip("key", When::After);

            cut::trip("verify", When::Before);
            assert!(verify(m), "verification failed");
            cut::trip("verify", When::After);

            cut::trip("close", When::Before);
            m.journal_set(&[("state", "confirmed")]);
            cut::trip("close", When::After);
        }

        /// Every worktree sees the Guardrails folder as its effective `core.hooksPath`.
        fn verify(m: &Machine) -> bool {
            let list = m
                .git(&m.repo, &["worktree", "list", "--porcelain"])
                .unwrap_or_default();
            list.lines()
                .filter_map(|l| l.strip_prefix("worktree "))
                .all(|wt| {
                    m.git(Path::new(wt), &["config", "--get", "core.hooksPath"])
                        .is_some_and(|v| v == m.hooks_value())
                })
        }

        fn uninstall(m: &Machine, variant: &str) {
            cut::trip("journal", When::Before);
            m.journal_set(&[("op", "uninstall"), ("state", "started")]);
            cut::trip("journal", When::After);

            cut::trip("key", When::Before);
            let config = slash(&m.common.join("config"));
            cut::trip_during("key", || {
                let text = std::fs::read(m.common.join("config")).unwrap();
                std::fs::write(m.common.join("config.lock"), &text[..text.len() / 2]).unwrap();
            });
            if m.local_value().as_deref() == Some(m.hooks_value().as_str()) {
                match m.journal_get("prior_local").filter(|v| !v.is_empty()) {
                    Some(prior) => m.git(
                        &m.repo,
                        &["config", "--file", &config, "core.hooksPath", &prior],
                    ),
                    None => m.git(
                        &m.repo,
                        &["config", "--file", &config, "--unset", "core.hooksPath"],
                    ),
                }
                .expect("restore core.hooksPath");
            }
            cut::trip("key", When::After);

            cut::trip("folder", When::Before);
            if variant != "leave-folder" {
                m.remove_folder(&m.folder());
            }
            cut::trip("folder", When::After);

            cut::trip("close", When::Before);
            m.journal_set(&[("state", "uninstalled")]);
            cut::trip("close", When::After);
        }

        /// Daemon start (ADR-GRD-001 § 4, Recuperación).
        fn recover(m: &Machine, variant: &str) {
            if variant == "no-recovery" || m.journal_get("state").as_deref() != Some("started") {
                return;
            }
            // The daemon is the only writer and it died: its lock is stale.
            let _ = std::fs::remove_file(m.common.join("config.lock"));
            let ours = m.local_value().as_deref() == Some(m.hooks_value().as_str());
            match m.journal_get("op").as_deref() {
                Some("install") if ours && verify(m) => m.journal_set(&[("state", "confirmed")]),
                Some("install") => {
                    if ours {
                        let config = slash(&m.common.join("config"));
                        m.git(
                            &m.repo,
                            &["config", "--file", &config, "--unset", "core.hooksPath"],
                        );
                    }
                    if let Some(tmp) = m.journal_get("tmp") {
                        m.remove_folder(&m.common.join(tmp));
                    }
                    m.remove_folder(&m.folder());
                    m.journal_set(&[("state", "rolled-back")]);
                }
                Some("uninstall") if ours => m.journal_set(&[("state", "not-completed")]),
                Some("uninstall") => {
                    m.remove_folder(&m.folder());
                    m.journal_set(&[("state", "uninstalled")]);
                }
                _ => {}
            }
        }

        /// Entry point of the child process; a no-op in a normal test run.
        #[test]
        fn child_entry() {
            let Ok(op) = std::env::var(CHILD_OP) else {
                return;
            };
            let variant = std::env::var(CHILD_VARIANT).unwrap_or_default();
            let m = Machine::from_env();
            match op.as_str() {
                "install" => install(&m, &variant),
                "uninstall" => uninstall(&m, &variant),
                "recover" => recover(&m, &variant),
                other => panic!("unknown op {other}"),
            }
        }

        /// Run the reference installer in a child process.
        fn spawn(
            f: &Fixture,
            op: &str,
            variant: &str,
            point: Option<&CutPoint>,
            trace: Option<&Path>,
        ) -> Exit {
            let mut c = Command::new(std::env::current_exe().unwrap());
            c.args([
                "--exact",
                "repo_intact::hooks::child_entry",
                "--test-threads=1",
                "-q",
            ])
            .env(CHILD_OP, op)
            .env(CHILD_VARIANT, variant)
            .env(CHILD_ROOT, &f.root)
            .env(CHILD_GIT, &f.git)
            .env_remove(cut::ENV_CUT)
            .env_remove(cut::ENV_TRACE);
            if let Some(p) = point {
                c.env(cut::ENV_CUT, p.to_string());
            }
            if let Some(t) = trace {
                c.env(cut::ENV_TRACE, t);
            }
            Exit::from(c.output().expect("spawn child").status)
        }

        fn installed(f: &Fixture) -> Result<(), String> {
            let ours = f.common_dir().join("gitraptor/hooks");
            for at in f.hooks_path_state() {
                let v = at
                    .value
                    .as_ref()
                    .map(|v| (v.scope.as_str(), v.value.as_str()));
                if v != Some(("local", ours.to_str().unwrap())) {
                    return Err(format!("{}: hooksPath {v:?}", at.worktree.display()));
                }
            }
            for h in ["pre-push", "pre-rebase", "reference-transaction"] {
                if !ours.join(h).is_file() {
                    return Err(format!("dispatcher {h} missing"));
                }
            }
            Ok(())
        }

        fn not_installed(f: &Fixture) -> Result<(), String> {
            let ours = f.common_dir().join("gitraptor/hooks");
            for at in f.hooks_path_state() {
                if at
                    .value
                    .as_ref()
                    .is_some_and(|v| Path::new(&v.value) == ours)
                {
                    return Err(format!(
                        "{}: still points to Guardrails",
                        at.worktree.display()
                    ));
                }
            }
            Ok(())
        }

        fn install_sweep<'a>(
            name: &str,
            build: impl Fn() -> Fixture + 'a,
            variant: &'static str,
        ) -> Sweep<'a> {
            Sweep {
                scenario: name.into(),
                build: Box::new(build),
                prepare: Box::new(|_| {}),
                run: Box::new(move |f, p, t| spawn(f, "install", variant, p, t)),
                recover: Box::new(move |f| {
                    spawn(f, "recover", variant, None, None);
                }),
                points: cut::points(INSTALL_STEPS, &["key"]),
                unchanged: EndState::new(
                    "identical",
                    Baseline::Pristine,
                    uninstalled_exceptions(),
                    not_installed,
                ),
                done: EndState::new(
                    "complete",
                    Baseline::Pristine,
                    install_exceptions(),
                    installed,
                ),
            }
        }

        fn uninstall_sweep<'a>(name: &str, build: impl Fn() -> Fixture + 'a) -> Sweep<'a> {
            Sweep {
                scenario: name.into(),
                build: Box::new(build),
                prepare: Box::new(|f| {
                    assert_eq!(spawn(f, "install", "", None, None), Exit::Finished);
                }),
                run: Box::new(|f, p, t| spawn(f, "uninstall", "", p, t)),
                recover: Box::new(|f| {
                    spawn(f, "recover", "", None, None);
                }),
                points: cut::points(UNINSTALL_STEPS, &["key"]),
                unchanged: EndState::new(
                    "still installed",
                    Baseline::Start,
                    Exceptions::engine_profile("profile").and(Exceptions::none().with(
                        gitraptor_testkit::Exception::DirTimes {
                            scope: "repo".into(),
                            path: ".git".into(),
                        },
                    )),
                    installed,
                ),
                done: EndState::new(
                    "uninstalled",
                    Baseline::Pristine,
                    uninstalled_exceptions(),
                    not_installed,
                ),
            }
        }

        // --- Footprint: install and uninstall with every kind of prior hooks ---------------------

        /// After install only the key and the folder change; after uninstall, zero differences
        /// with the repo before install, under the semantic config criterion (ADR-GRD-001
        /// Validación 1). Every prior hooks kind, with a linked worktree.
        #[test]
        fn footprint_install_uninstall_every_prior_hooks() {
            for prior in PriorHooks::ALL {
                let f = Fixture::with_prior_hooks(&git(), prior);
                f.add_worktree("linked", "HEAD");
                let state_before = f.hooks_path_state();
                let pristine = f.snapshot(&uninstalled_exceptions());

                let report = check(
                    &format!("install {prior:?}"),
                    &f,
                    &install_exceptions(),
                    || {
                        assert_eq!(spawn(&f, "install", "", None, None), Exit::Finished);
                    },
                );
                report.assert_intact();
                installed(&f).unwrap();

                assert_eq!(spawn(&f, "uninstall", "", None, None), Exit::Finished);
                let after = f.snapshot(&uninstalled_exceptions());
                let imputable = uninstalled_exceptions().filter(
                    &gitraptor_testkit::diff(&pristine, &after),
                    &pristine,
                    &after,
                );
                assert!(
                    imputable.is_empty(),
                    "{prior:?} after uninstall: {imputable:#?}"
                );
                assert_eq!(f.hooks_path_state(), state_before, "{prior:?}");
            }
        }

        /// The prior linter rejects the same commit before install, after install (chained) and
        /// after uninstall, in the main and in a linked worktree (ADR-GRD-001 Validación 2).
        #[test]
        fn chaining_prior_hook_rejects_the_same_before_and_after() {
            for prior in PriorHooks::ALL {
                let f = Fixture::with_prior_hooks(&git(), prior);
                let wt = f.add_worktree("linked", "HEAD");
                // husky does not run in linked worktrees, with or without Guardrails.
                let in_wt = prior.has_linter() && prior != PriorHooks::Husky;
                let rejects = |stage: &str| {
                    assert_eq!(
                        f.commit_with_marker_rejected(&f.repo),
                        prior.has_linter(),
                        "{prior:?} {stage}"
                    );
                    assert_eq!(
                        f.commit_with_marker_rejected(&wt),
                        in_wt,
                        "{prior:?} {stage}, linked worktree"
                    );
                };
                rejects("before install");
                assert_eq!(spawn(&f, "install", "", None, None), Exit::Finished);
                rejects("after install");
                assert_eq!(spawn(&f, "uninstall", "", None, None), Exit::Finished);
                rejects("after uninstall");
            }
        }

        #[test]
        fn footprint_prior_hooks_local_value_is_restored_at_its_level() {
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Husky);
            let before = f.hooks_path_state();
            assert_eq!(before[0].value.as_ref().unwrap().scope, "local");
            assert_eq!(before[0].value.as_ref().unwrap().value, ".husky/_");
            assert_eq!(spawn(&f, "install", "", None, None), Exit::Finished);
            assert_eq!(spawn(&f, "uninstall", "", None, None), Exit::Finished);
            assert_eq!(f.hooks_path_state(), before);

            // A global value is not restored locally: the local key is just removed.
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Global);
            let before = f.hooks_path_state();
            assert_eq!(before[0].value.as_ref().unwrap().scope, "global");
            assert_eq!(spawn(&f, "install", "", None, None), Exit::Finished);
            assert_eq!(spawn(&f, "uninstall", "", None, None), Exit::Finished);
            assert_eq!(f.hooks_path_state(), before);
        }

        // --- Sensitivity (negative tests) --------------------------------------------------------

        #[test]
        fn sensitivity_uninstall_leaving_a_trace_is_detected() {
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Own);
            let pristine = f.snapshot(&uninstalled_exceptions());
            assert_eq!(spawn(&f, "install", "", None, None), Exit::Finished);
            assert_eq!(
                spawn(&f, "uninstall", "leave-folder", None, None),
                Exit::Finished
            );
            let after = f.snapshot(&uninstalled_exceptions());
            let imputable = uninstalled_exceptions().filter(
                &gitraptor_testkit::diff(&pristine, &after),
                &pristine,
                &after,
            );
            assert!(
                imputable.iter().any(|c| c.path == Path::new(".git/gitraptor")
                    && c.kind == ChangeKind::Created),
                "{imputable:#?}"
            );
        }

        #[test]
        fn sensitivity_install_changing_a_prior_hook_byte_is_detected() {
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Own);
            let report = check("install tampers prior", &f, &install_exceptions(), || {
                assert_eq!(
                    spawn(&f, "install", "tamper-prior", None, None),
                    Exit::Finished
                );
            });
            assert!(
                report
                    .imputable
                    .iter()
                    .any(|c| c.path == Path::new(".git/hooks/pre-commit")),
                "{report}"
            );
        }

        #[test]
        fn sensitivity_install_writing_global_config_is_detected() {
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::None);
            let report = check("install writes global", &f, &install_exceptions(), || {
                assert_eq!(
                    spawn(&f, "install", "write-global", None, None),
                    Exit::Finished
                );
            });
            assert!(
                report
                    .imputable
                    .iter()
                    .any(|c| c.scope == "home" && c.path == Path::new(".gitconfig")),
                "{report}"
            );
        }

        #[test]
        fn sensitivity_install_without_recovery_fails_the_sweep() {
            let git = git();
            let report = install_sweep(
                "no recovery",
                || Fixture::with_prior_hooks(&git, PriorHooks::Own),
                "no-recovery",
            )
            .run();
            assert!(!report.is_ok());
            assert!(
                matches!(
                    report.outcome("folder-rename:after"),
                    Some(Outcome::Broken { .. })
                ),
                "{report}"
            );
            assert!(
                matches!(report.outcome("key:during"), Some(Outcome::Broken { .. })),
                "{report}"
            );
            // The report names the scenario, the cut point, the path and the kind of change.
            let text = report.to_string();
            assert!(text.contains("cut sweep 'no recovery'"), "{text}");
            assert!(
                text.contains("folder-rename:after: FAILED")
                    && text.contains("[repo] .git/gitraptor: created"),
                "{text}"
            );
            assert!(text.contains("[repo] .git/config.lock: created"), "{text}");
        }

        #[test]
        fn sensitivity_undeclared_or_unreached_cut_points_fail_the_sweep() {
            let git = git();
            let mut sweep = install_sweep(
                "coverage",
                || Fixture::with_prior_hooks(&git, PriorHooks::None),
                "",
            );
            sweep.points.retain(|p| p.step != "verify");
            sweep.points.push(CutPoint::new("phantom", When::Before));
            let report = sweep.run();
            assert!(!report.is_ok());
            assert!(
                report
                    .coverage
                    .contains(&"verify:before reached but not declared".to_owned()),
                "{report}"
            );
            assert!(
                report
                    .coverage
                    .contains(&"phantom:before declared but never reached".to_owned()),
                "{report}"
            );
            assert_eq!(
                report.outcome("phantom:before"),
                Some(&Outcome::NotCut(Exit::Finished))
            );

            // An empty list of points is never a pass.
            let mut empty = install_sweep(
                "empty",
                || Fixture::with_prior_hooks(&git, PriorHooks::None),
                "",
            );
            empty.points.clear();
            assert!(!empty.run().is_ok());
        }

        // --- Cuts (NFR-12) -----------------------------------------------------------------------

        #[test]
        fn cuts_install_is_complete_or_identical_at_every_point() {
            let git = git();
            for prior in [PriorHooks::None, PriorHooks::Husky] {
                let report = install_sweep(
                    &format!("install with {prior:?} and a linked worktree"),
                    || {
                        let f = Fixture::with_prior_hooks(&git, prior);
                        f.add_worktree("linked", "HEAD");
                        f
                    },
                    "",
                )
                .run();
                report.assert_ok();
                // Both end states are reached somewhere in the sweep.
                assert_eq!(
                    report.outcome("precheck:before"),
                    Some(&Outcome::Reached("identical"))
                );
                assert_eq!(
                    report.outcome("key:after"),
                    Some(&Outcome::Reached("complete"))
                );
            }
        }

        #[test]
        fn cuts_uninstall_is_complete_or_identical_at_every_point() {
            let git = git();
            let report = uninstall_sweep("uninstall with husky", || {
                Fixture::with_prior_hooks(&git, PriorHooks::Husky)
            })
            .run();
            report.assert_ok();
            assert_eq!(
                report.outcome("key:before"),
                Some(&Outcome::Reached("still installed"))
            );
            assert_eq!(
                report.outcome("key:after"),
                Some(&Outcome::Reached("uninstalled"))
            );
        }

        #[test]
        fn cut_point_protocol_round_trips() {
            for p in cut::points(INSTALL_STEPS, &["key"]) {
                assert_eq!(CutPoint::parse(&p.to_string()), Some(p));
            }
            assert_eq!(CutPoint::parse("key"), None);
            assert_eq!(CutPoint::parse(":before"), None);
            assert_eq!(CutPoint::parse("key:later"), None);
        }

        // --- Fixtures and config criterion -------------------------------------------------------

        #[test]
        fn fixtures_coverage_blockers_are_visible_per_worktree() {
            let f = Fixture::with_commit(&git());
            f.add_coverage_blocker(CoverageBlocker::WorktreeConfig);
            let state = f.hooks_path_state();
            assert!(
                state
                    .iter()
                    .any(|s| s.value.as_ref().is_some_and(|v| v.scope == "worktree")),
                "{state:?}"
            );

            let f = Fixture::with_commit(&git());
            f.add_coverage_blocker(CoverageBlocker::Include);
            let v = f.hooks_path_state()[0].value.clone().unwrap();
            assert_eq!(v.scope, "local");
            assert!(v.origin.ends_with("hooks.inc"), "{v:?}");

            let f = Fixture::with_commit(&git());
            f.add_coverage_blocker(CoverageBlocker::IncludeIfOnbranch);
            assert!(
                f.git(&[
                    "config",
                    "--local",
                    "--get-regexp",
                    "^includeif\\.onbranch:"
                ])
                .contains("onbranch:main")
            );
        }

        #[test]
        fn fixtures_managers_leave_what_the_spike_observed() {
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Husky);
            assert!(f.repo.join(".husky/_/pre-push").is_file());
            assert!(f.repo.join(".husky/pre-commit").is_file());
            assert!(
                f.git(&["status", "--porcelain"]).is_empty(),
                "husky's _ is ignored"
            );
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::Lefthook);
            assert!(f.repo.join("lefthook.yml").is_file());
            let f = Fixture::with_prior_hooks(&git(), PriorHooks::PreCommit);
            let hook = std::fs::read_to_string(f.repo.join(".git/hooks/pre-commit")).unwrap();
            assert!(hook.contains("File generated by pre-commit"));
        }

        #[test]
        fn config_criterion_is_semantic_but_ordered() {
            let none: &[String] = &[];
            // Formatting, comments, quoting and a missing final newline do not count.
            let a = b"[core]\n\thooksPath = .husky/_\n\tbare = false\n";
            let b = b"[core]\n  hooksPath = \".husky/_\"   ; by hand\n\tbare = false";
            assert_eq!(without_keys(a, none), without_keys(b, none));
            // An empty section left by `--unset` does not count either.
            let c = b"[core]\n\tbare = false\n[gitraptor]\n";
            let d = b"[core]\n\tbare = false\n";
            assert_eq!(without_keys(c, none), without_keys(d, none));
            // The value and the order do: a key after an include overrides it.
            let e = b"[core]\n\thooksPath = other\n\tbare = false\n";
            assert_ne!(without_keys(a, none), without_keys(e, none));
            let f = b"[include]\n\tpath = x\n[core]\n\tbare = false\n";
            let g = b"[core]\n\tbare = false\n[include]\n\tpath = x\n";
            assert_ne!(without_keys(f, none), without_keys(g, none));
            // Only the listed keys are ignored; an unparsable file never compares equal.
            let keys = vec!["core.hooksPath".to_owned()];
            assert_eq!(without_keys(a, &keys), without_keys(d, &keys));
            assert_eq!(without_keys(b"[core\n", none), None);
        }
    }
}
