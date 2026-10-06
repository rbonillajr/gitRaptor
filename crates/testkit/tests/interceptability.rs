//! Interceptability matrix and hook-process counts (INF-GRD-001; ADR-GRD-002 Validación 1, 2, 12
//! and 13), with raw Git on temporary repos. Test names live under `repo_intact::` so the single
//! CI gate selects them. Every report prints the Git version of the runner.

mod repo_intact {
    pub mod interceptability {
        use std::collections::BTreeMap;

        use gitraptor_testkit::fixture::git_from_path;
        use gitraptor_testkit::interceptability::{
            COST_HOOKS, COST_TABLE, Case, GitVersion, MINIMUM_HOOKS, Moment, Observation,
            Published, REFERENCE, RefFormat, catalog, compare, cost_commands, cost_rows,
            count_hooks, derive, observe, render, restrict,
        };

        fn run(cases: Vec<Case>) -> (GitVersion, Vec<String>) {
            let git = git_from_path();
            let v = GitVersion::of(&git);
            let mismatches = cases
                .iter()
                .filter_map(|c| compare(&observe(&git, c), v))
                .map(|m| m.to_string())
                .collect();
            (v, mismatches)
        }

        /// The published list matches what raw Git does, row by row (ADR-GRD-002 Validación 1
        /// and 2). A difference blocks the merge (US-GRD-004 owns the list in the binary).
        #[test]
        fn matrix_matches_the_published_list() {
            let v = GitVersion::of(&git_from_path());
            assert!(
                v >= GitVersion(2, 38, 0),
                "git {v} is older than the supported 2.38"
            );
            let (v, mismatches) = run(catalog(false));
            assert!(
                mismatches.is_empty(),
                "git {v}: {} mismatch(es) with the published list:\n{}",
                mismatches.len(),
                mismatches.join("\n")
            );
        }

        /// Renaming over or away from the base branch in a reftable repo runs no hook
        /// (SPIKE-GRD-001 D10, D11). If Git fixes it, this fails and the row is retired from the
        /// list (ADR-GRD-002 Validación 12). Git < 2.45 has no reftable: nothing to check.
        #[test]
        fn matrix_reftable_rename_rows_still_hold() {
            let v = GitVersion::of(&git_from_path());
            if !v.has_reftable() {
                println!("git {v}: no reftable backend, rows not applicable");
                return;
            }
            let cases = catalog(true)
                .into_iter()
                .filter(|c| c.refs == RefFormat::Reftable)
                .collect::<Vec<_>>();
            assert_eq!(cases.len(), 2);
            let (v, mismatches) = run(cases);
            assert!(mismatches.is_empty(), "git {v}:\n{}", mismatches.join("\n"));
        }

        /// Every row of the reference list has a case, and every case a row.
        #[test]
        fn matrix_catalog_covers_the_reference_list() {
            let cases = catalog(true);
            for r in REFERENCE.iter() {
                assert!(
                    cases.iter().any(|c| c.code == r.code && c.refs == r.refs),
                    "no case for {} ({:?})",
                    r.code,
                    r.refs
                );
            }
            for c in &cases {
                assert!(
                    REFERENCE
                        .iter()
                        .any(|r| r.code == c.code && r.refs == c.refs),
                    "no reference row for {} ({:?})",
                    c.code,
                    c.refs
                );
            }
        }

        /// The executor itself: a hook that never ran is C; a declared skip is only published as
        /// such when its hook did not run; A of an unrecognisable operation is not impedible.
        #[test]
        fn matrix_derivation_rules() {
            let obs = |code, moment| Observation {
                code,
                refs: RefFormat::Files,
                hook_ran: moment != Moment::C,
                exit: Some(1),
                residual: String::new(),
                moment,
                trace: Vec::new(),
            };
            assert_eq!(
                derive(&obs("commit", Moment::A), false),
                Published::Impedible
            );
            assert_eq!(
                derive(&obs("push-no-verify", Moment::C), true),
                Published::DeclaredSkip
            );
            assert_eq!(
                derive(&obs("push-no-verify", Moment::A), true),
                Published::Impedible
            );
            assert_eq!(
                derive(&obs("merge", Moment::B), false),
                Published::NotImpedible("B")
            );
            assert_eq!(
                derive(&obs("worktree-add-existing", Moment::A), false),
                Published::NotImpedible("no-reconocible")
            );
            // Sensitivity: a published row that Git contradicts is reported.
            let wrong = obs("reset-hard", Moment::A);
            let v = GitVersion(2, 50, 1);
            let m = compare(&wrong, v).expect("mismatch");
            assert!(m.to_string().contains("reset-hard"), "{m}");
            // A version no row covers is its own failure.
            let gap = obs("rename-over-base", Moment::B);
            let gap = Observation {
                refs: RefFormat::Reftable,
                ..gap
            };
            let m = compare(&gap, GitVersion(2, 44, 0)).expect("not in the list");
            assert!(m.to_string().contains("not in the reference list"), "{m}");
            assert!(compare(&gap, v).is_none());
        }

        /// Hook processes per command and Git version, against the reference table
        /// (ADR-GRD-002 § 5 and Validación 13): the full probe set (without `post-index-change`) and
        /// the minimum set.
        #[test]
        fn cost_hook_processes_per_command_match_the_table() {
            let git = git_from_path();
            let v = GitVersion::of(&git);
            let rows = cost_rows(COST_TABLE, v);
            let mut measured = Vec::new();
            let mut failures = Vec::new();
            for cmd in cost_commands() {
                let full = count_hooks(&git, &cmd, COST_HOOKS);
                let min = count_hooks(&git, &cmd, MINIMUM_HOOKS);
                measured.push(format!("{}: {}", cmd.code, render(&full)));
                if min != restrict(&full, MINIMUM_HOOKS) {
                    failures.push(format!(
                        "{}: minimum set {} is not the full count restricted ({})",
                        cmd.code,
                        render(&min),
                        render(&restrict(&full, MINIMUM_HOOKS))
                    ));
                }
                let Some(row) = rows.iter().find(|r| r.command == cmd.code) else {
                    continue;
                };
                if render(&full) != row.counts {
                    failures.push(format!(
                        "{}: measured {} expected {}",
                        cmd.code,
                        render(&full),
                        row.counts
                    ));
                }
            }
            println!("git {v} hook processes:\n{}", measured.join("\n"));
            assert!(
                !rows.is_empty(),
                "git {v} is not in the hook-process reference table; measured:\n{}",
                measured.join("\n")
            );
            assert!(failures.is_empty(), "git {v}:\n{}", failures.join("\n"));
        }

        #[test]
        fn cost_version_parsing_and_ranges() {
            let p = |s| GitVersion::parse(s).unwrap();
            assert_eq!(
                p("git version 2.50.1 (Apple Git-155)"),
                GitVersion(2, 50, 1)
            );
            assert_eq!(p("git version 2.47.1.windows.2"), GitVersion(2, 47, 1));
            assert_eq!(p("git version 2.45.0"), GitVersion(2, 45, 0));
            assert!(cost_rows(COST_TABLE, GitVersion(1, 0, 0)).is_empty());
            let mut counts = BTreeMap::new();
            counts.insert("reference-transaction:prepared".to_owned(), 2);
            counts.insert("post-commit".to_owned(), 1);
            assert_eq!(
                render(&restrict(&counts, MINIMUM_HOOKS)),
                "reference-transaction:preparedx2"
            );
        }
    }
}
