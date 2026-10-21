// TS-CKP-001 T011: fidelity of the in-memory dry merge against `git merge-tree` on the R-SYN
// corpus (31 cases: the BRD demo and one criss-cross history). The reference runs in a
// disposable mirror clone with isolated config, never in the observed repo.
//
// The reference's line ranges come from running the engine's own `hunk_ranges` over the blob
// that `merge-tree` merged: the test compares the merge engines, not two extractors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use gitraptor_git::dry_merge::{
    DryMergeInput, DryMergeLimits, LineRange, hunk_ranges, merge_in_memory,
};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::syn_corpus::{self, SynCase};

struct Reference {
    git: PathBuf,
    dir: PathBuf,
}

impl Reference {
    /// A disposable mirror of `repo` with the marker size pinned by attribute.
    fn clone_of(repo: &Path, into: &Path) -> Self {
        let git = git_from_path();
        let dir = into.join("reference.git");
        let r = Self {
            git,
            dir: dir.clone(),
        };
        let out = r
            .command(into)
            .args(["clone", "-q", "--mirror", "--no-local"])
            .arg(repo)
            .arg(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::create_dir_all(dir.join("info")).unwrap();
        std::fs::write(dir.join("info/attributes"), "* conflict-marker-size=32\n").unwrap();
        r
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut c = Command::new(&self.git);
        c.current_dir(cwd)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(unix) { "/dev/null" } else { "NUL" },
            );
        c
    }

    fn git(&self, args: &[&str]) -> Option<Vec<u8>> {
        let out = self.command(&self.dir).args(args).output().unwrap();
        out.status.success().then_some(out.stdout)
    }

    /// Conflicted paths and the id of the merged tree, per `merge-tree --write-tree`.
    fn merge_tree(&self, ours: &str, theirs: &str) -> (String, BTreeSet<String>) {
        let out = self
            .command(&self.dir)
            .args([
                "-c",
                "merge.renameLimit=1000",
                "merge-tree",
                "--write-tree",
                "--name-only",
                "-z",
                ours,
                theirs,
            ])
            .output()
            .unwrap();
        let code = out.status.code().unwrap_or(-1);
        assert!(
            code == 0 || code == 1,
            "merge-tree: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut parts = out.stdout.split(|b| *b == 0);
        let tree = String::from_utf8_lossy(parts.next().unwrap()).into_owned();
        let mut paths = BTreeSet::new();
        for p in parts {
            if p.is_empty() {
                break;
            }
            paths.insert(String::from_utf8_lossy(p).into_owned());
        }
        (tree, paths)
    }

    fn blob(&self, rev: &str, path: &str) -> Option<Vec<u8>> {
        self.git(&["show", &format!("{rev}:{path}")])
    }
}

/// Git's unique name for a file/directory conflict, `x~side`, counts as `x`.
fn plain(path: &str) -> String {
    path.split('~').next().unwrap().to_owned()
}

struct Row {
    case: SynCase,
    engine: gitraptor_git::dry_merge::DryMergeOutcome,
    ref_paths: BTreeSet<String>,
    ref_ranges: BTreeMap<String, (Vec<LineRange>, Vec<LineRange>)>,
}

fn run_corpus() -> (tempfile::TempDir, Vec<Row>) {
    let tmp = tempfile::tempdir().unwrap();
    let cases = syn_corpus::generate(&tmp.path().join("syn"));
    assert_eq!(cases.len(), 31, "R-SYN has 31 cases");
    let repo = cases[0].repo.clone();
    assert!(cases.iter().all(|c| c.repo == repo), "one repo, many cases");
    let reference = Reference::clone_of(&repo, tmp.path());
    let limits = DryMergeLimits::default();
    let rows = cases
        .into_iter()
        .map(|case| {
            let input = DryMergeInput {
                ours: case.ours.clone(),
                theirs: case.theirs.clone(),
            };
            let engine = merge_in_memory(&case.repo, &input, &limits)
                .unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
            let (tree, listed) = reference.merge_tree(&case.ours, &case.theirs);
            let ref_paths: BTreeSet<String> = listed.iter().map(|p| plain(p)).collect();
            let mut ref_ranges = BTreeMap::new();
            for p in &listed {
                let base = plain(p);
                let (Some(merged), Some(o), Some(t)) = (
                    reference.blob(&tree, p),
                    reference.blob(&case.ours, &base),
                    reference.blob(&case.theirs, &base),
                ) else {
                    continue;
                };
                let (ro, rt, _) = hunk_ranges(&merged, &o, &t, &limits);
                ref_ranges.insert(base, (ro, rt));
            }
            Row {
                case,
                engine,
                ref_paths,
                ref_ranges,
            }
        })
        .collect();
    (tmp, rows)
}

mod r_syn {
    use super::*;

    #[test]
    fn ckp001_fidelity_conflicted_files_match_merge_tree_on_every_pair() {
        let (_tmp, rows) = run_corpus();
        let mut mismatches = Vec::new();
        let mut with_conflict = 0;
        for row in &rows {
            let engine: BTreeSet<String> =
                row.engine.files.iter().map(|f| plain(&f.path)).collect();
            if engine.is_empty() && row.ref_paths.is_empty() {
                continue;
            }
            with_conflict += 1;
            if engine != row.ref_paths {
                mismatches.push(format!(
                    "{}: engine {engine:?} vs merge-tree {:?}",
                    row.case.name, row.ref_paths
                ));
            }
        }
        assert!(with_conflict > 0, "the corpus has conflicts");
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }

    #[test]
    fn ckp001_fidelity_hunk_ranges_are_identical() {
        let (_tmp, rows) = run_corpus();
        let mut compared = 0;
        let mut mismatches = Vec::new();
        for row in &rows {
            for f in &row.engine.files {
                let Some((ro, rt)) = row.ref_ranges.get(&plain(&f.path)) else {
                    continue;
                };
                compared += 1;
                if (&f.ours, &f.theirs) != (ro, rt) {
                    mismatches.push(format!(
                        "{} {}: engine {:?}/{:?} vs reference {ro:?}/{rt:?}",
                        row.case.name, f.path, f.ours, f.theirs
                    ));
                }
            }
        }
        assert!(compared > 0, "no file with ranges to compare");
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }

    #[test]
    fn ckp001_fidelity_the_demo_has_no_false_negatives() {
        let (_tmp, rows) = run_corpus();
        let demo: Vec<&Row> = rows.iter().filter(|r| r.case.demo).collect();
        assert!(!demo.is_empty(), "the corpus carries the demo");
        assert!(
            demo.iter().any(|r| !r.ref_paths.is_empty()),
            "the demo has a real conflict"
        );
        for row in demo {
            let engine: BTreeSet<String> =
                row.engine.files.iter().map(|f| plain(&f.path)).collect();
            let missed: Vec<_> = row.ref_paths.difference(&engine).collect();
            assert!(missed.is_empty(), "{}: missed {missed:?}", row.case.name);
        }
    }

    #[test]
    fn ckp001_fidelity_the_crossed_history_case_uses_a_virtual_base() {
        let (_tmp, rows) = run_corpus();
        assert!(
            rows.iter().any(|r| matches!(
                r.engine.merge_base,
                gitraptor_git::dry_merge::MergeBaseUsed::Virtual { .. }
            )),
            "no case with two merge-bases"
        );
    }
}
