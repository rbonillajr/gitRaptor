// TS-CKP-001 T004/T005: the in-memory dry merge on temporary repos (NFR-01): which files conflict,
// of what kind, with which line ranges, and what it never does (no driver, no write, no load of a
// huge blob).

#[cfg(unix)]
use std::path::PathBuf;

use gitraptor_git::dry_merge::{
    ConflictFile, ConflictKind, DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome,
    LineRange, MergeBaseUsed, merge_in_memory,
};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Fixture, fingerprint};

fn id(f: &Fixture, rev: &str) -> String {
    f.git(&["rev-parse", rev]).trim().to_owned()
}

/// Commit on `branch` (reset to `from`) the changes `edit` makes; leaves `main` checked out.
fn commit_on(f: &Fixture, branch: &str, from: &str, edit: impl FnOnce(&Fixture)) -> String {
    f.git(&["checkout", "-q", "-B", branch, from]);
    edit(f);
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", branch]);
    let tip = id(f, "HEAD");
    f.git(&["checkout", "-q", "main"]);
    tip
}

fn input(ours: &str, theirs: &str) -> DryMergeInput {
    DryMergeInput {
        ours: ours.to_owned(),
        theirs: theirs.to_owned(),
    }
}

fn merge(f: &Fixture, ours: &str, theirs: &str) -> DryMergeOutcome {
    merge_with(f, ours, theirs, &DryMergeLimits::default()).unwrap()
}

fn merge_with(
    f: &Fixture,
    ours: &str,
    theirs: &str,
    limits: &DryMergeLimits,
) -> Result<DryMergeOutcome, DryMergeError> {
    merge_in_memory(&f.repo, &input(ours, theirs), limits)
}

fn file<'a>(o: &'a DryMergeOutcome, path: &str) -> &'a ConflictFile {
    o.files
        .iter()
        .find(|c| c.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed: {:?}", o.files))
}

fn lines(prefix: &str, n: u32) -> String {
    (1..=n).map(|i| format!("{prefix}{i}\n")).collect()
}

/// `main` with a 20-line `a.txt` and a `b.txt`, and two tips that edit `a.txt` on line 5.
fn content_conflict() -> (Fixture, String, String) {
    let f = Fixture::with_commit(&git_from_path());
    f.write("a.txt", &lines("l", 20));
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "a"]);
    let edit = |side: &'static str| {
        move |f: &Fixture| {
            let text = lines("l", 20).replace("l5\n", &format!("{side}\n"));
            f.write("a.txt", &text);
        }
    };
    let ours = commit_on(&f, "ours", "main", edit("ours-5"));
    let theirs = commit_on(&f, "theirs", "main", edit("theirs-5"));
    (f, ours, theirs)
}

mod kinds {
    use super::*;

    #[test]
    fn ckp001_kinds_content_has_ranges_per_side_and_needs_no_escalation() {
        let (f, ours, theirs) = content_conflict();
        let o = merge(&f, &ours, &theirs);
        assert_eq!(o.files.len(), 1);
        let a = file(&o, "a.txt");
        assert_eq!(a.kind, ConflictKind::Content);
        assert_eq!(a.ours, vec![LineRange { start: 5, len: 1 }]);
        assert_eq!(a.theirs, vec![LineRange { start: 5, len: 1 }]);
        assert!(!a.hunks_truncated);
        assert!(!a.has_merge_rule);
        assert!(!o.truncated);
        assert!(matches!(o.merge_base, MergeBaseUsed::Single(_)));
        assert!(!o.has_kind_needing_escalation());
    }

    #[test]
    fn ckp001_kinds_clean_merge_lists_nothing() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", |f| f.write("o.txt", "o\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("t.txt", "t\n"));
        let o = merge(&f, &ours, &theirs);
        assert!(o.files.is_empty());
        assert!(!o.truncated);
    }

    #[test]
    fn ckp001_kinds_add_add() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", |f| f.write("new.txt", "ours\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("new.txt", "theirs\n"));
        let o = merge(&f, &ours, &theirs);
        assert_eq!(file(&o, "new.txt").kind, ConflictKind::AddAdd);
        assert!(!o.has_kind_needing_escalation());
    }

    #[test]
    fn ckp001_kinds_modify_delete_needs_escalation() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", |f| {
            std::fs::remove_file(f.repo.join("b.txt")).unwrap();
        });
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("b.txt", "beta changed\n"));
        let o = merge(&f, &ours, &theirs);
        assert_eq!(file(&o, "b.txt").kind, ConflictKind::ModifyDelete);
        assert!(o.has_kind_needing_escalation());
    }

    #[test]
    fn ckp001_kinds_rename_needs_escalation() {
        let f = Fixture::with_commit(&git_from_path());
        f.write("r.txt", &lines("row", 30));
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "r"]);
        // Both sides rename the same file to different names.
        let ours = commit_on(&f, "ours", "main", |f| {
            f.git(&["mv", "r.txt", "r-ours.txt"]);
        });
        let theirs = commit_on(&f, "theirs", "main", |f| {
            f.git(&["mv", "r.txt", "r-theirs.txt"]);
        });
        let o = merge(&f, &ours, &theirs);
        assert!(
            o.files.iter().any(|c| c.kind == ConflictKind::Rename),
            "{:?}",
            o.files
        );
        assert!(o.has_kind_needing_escalation());
    }

    #[test]
    fn ckp001_kinds_directory_file_is_not_escalated() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", |f| f.write("d", "a file\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("d/x.txt", "in a dir\n"));
        let o = merge(&f, &ours, &theirs);
        assert!(
            o.files
                .iter()
                .any(|c| c.kind == ConflictKind::DirectoryFile),
            "{:?}",
            o.files
        );
        assert!(!o.has_kind_needing_escalation());
    }

    #[test]
    fn ckp001_kinds_past_max_files_it_cuts_and_marks_truncated() {
        let f = Fixture::with_commit(&git_from_path());
        for n in ["c1.txt", "c2.txt", "c3.txt"] {
            f.write(n, "base\n");
        }
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "three"]);
        let edit = |side: &'static str| {
            move |f: &Fixture| {
                for n in ["c1.txt", "c2.txt", "c3.txt"] {
                    f.write(n, &format!("{side}\n"));
                }
            }
        };
        let ours = commit_on(&f, "ours", "main", edit("ours"));
        let theirs = commit_on(&f, "theirs", "main", edit("theirs"));
        let limits = DryMergeLimits {
            max_files: 2,
            ..DryMergeLimits::default()
        };
        let o = merge_with(&f, &ours, &theirs, &limits).unwrap();
        assert_eq!(o.files.len(), 2);
        assert!(o.truncated);
        let all = merge(&f, &ours, &theirs);
        assert_eq!(all.files.len(), 3);
        assert!(!all.truncated);
    }
}

mod dir_rename {
    use super::*;

    #[test]
    fn a_directory_rename_conflict_is_listed() {
        let f = Fixture::with_commit(&git_from_path());
        f.write("old/f1.txt", &lines("one-", 12));
        f.write("old/f2.txt", &lines("two-", 12));
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "dir"]);
        // Ours renames the directory; theirs adds a file to the old one: Git moves it to `new/`
        // and reports a conflict (`merge.directoryRenames=conflict`).
        let ours = commit_on(&f, "ours", "main", |f| {
            f.git(&["mv", "old", "new"]);
        });
        let theirs = commit_on(&f, "theirs", "main", |f| {
            f.write("old/f3.txt", &lines("three-", 12));
        });
        let o = merge(&f, &ours, &theirs);
        assert!(
            o.files.iter().any(|c| c.path == "new/f3.txt"),
            "{:?}",
            o.files
        );
    }

    #[test]
    fn ckp001_dir_rename_a_directory_file_conflict_without_index_stage_is_listed() {
        let f = Fixture::with_commit(&git_from_path());
        // Ours has a file where theirs has a directory: no stage is left in the index.
        let ours = commit_on(&f, "ours", "main", |f| f.write("d", "a file\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("d/x.txt", "in a dir\n"));
        let o = merge(&f, &ours, &theirs);
        assert!(
            o.files
                .iter()
                .any(|c| c.path.starts_with('d') && c.kind == ConflictKind::DirectoryFile),
            "{:?}",
            o.files
        );
    }
}

/// A driver that leaves a marker file when Git (or anything) runs it.
#[cfg(unix)]
fn canary_driver(f: &Fixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let marker = f.root.join("driver-ran.marker");
    let script = f.root.join("driver.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.git(&[
        "config",
        "merge.canary.driver",
        &format!("{} %O %A %B", script.display()),
    ]);
    marker
}

#[cfg(unix)]
mod no_driver {
    use super::*;

    #[test]
    fn a_merge_attribute_runs_no_driver() {
        let f = Fixture::with_commit(&git_from_path());
        let marker = canary_driver(&f);
        // `.gitattributes` in the tree of both tips, on disk and in the index.
        f.write(".gitattributes", "a.txt merge=canary\n");
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "attrs"]);
        let ours = commit_on(&f, "ours", "main", |f| f.write("a.txt", "ours\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("a.txt", "theirs\n"));
        assert!(f.repo.join(".gitattributes").exists());
        let staged = f.git(&["ls-files", ".gitattributes"]);
        assert_eq!(staged.trim(), ".gitattributes");

        let o = merge(&f, &ours, &theirs);
        assert!(!marker.exists(), "a merge driver ran");
        assert_eq!(file(&o, "a.txt").kind, ConflictKind::Content);
    }

    #[test]
    fn ckp001_no_driver_attribute_only_on_disk_and_in_the_index_runs_none() {
        let f = Fixture::with_commit(&git_from_path());
        let marker = canary_driver(&f);
        let ours = commit_on(&f, "ours", "main", |f| f.write("a.txt", "ours\n"));
        let theirs = commit_on(&f, "theirs", "main", |f| f.write("a.txt", "theirs\n"));
        f.write(".gitattributes", "a.txt merge=canary\n");
        f.git(&["add", ".gitattributes"]);
        std::fs::write(f.repo.join(".git/info/attributes"), "a.txt merge=canary\n").unwrap();

        let o = merge(&f, &ours, &theirs);
        assert!(!marker.exists(), "a merge driver ran");
        assert!(o.files.iter().any(|c| c.path == "a.txt"));
    }
}

mod blob_cap {
    use super::*;

    fn big_edit(side: &'static str) -> impl FnOnce(&Fixture) {
        move |f| f.write("a.txt", &format!("{side}\n{}", lines(side, 400)))
    }

    #[test]
    fn a_blob_over_the_cap_is_binary_without_loading() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", big_edit("ours"));
        let theirs = commit_on(&f, "theirs", "main", big_edit("theirs"));
        let limits = DryMergeLimits {
            max_blob_bytes: 1024,
            ..DryMergeLimits::default()
        };
        let o = merge_with(&f, &ours, &theirs, &limits).expect("no error for a big blob");
        let a = file(&o, "a.txt");
        assert_eq!(a.kind, ConflictKind::Binary);
        assert!(a.ours.is_empty() && a.theirs.is_empty());
    }

    #[test]
    fn ckp001_blob_cap_a_blob_under_the_cap_gets_hunks() {
        let f = Fixture::with_commit(&git_from_path());
        let ours = commit_on(&f, "ours", "main", big_edit("ours"));
        let theirs = commit_on(&f, "theirs", "main", big_edit("theirs"));
        let limits = DryMergeLimits {
            max_blob_bytes: 1 << 20,
            ..DryMergeLimits::default()
        };
        let o = merge_with(&f, &ours, &theirs, &limits).unwrap();
        let a = file(&o, "a.txt");
        assert_eq!(a.kind, ConflictKind::Content);
        assert!(!a.ours.is_empty() && !a.theirs.is_empty());
    }

    #[test]
    fn ckp001_blob_cap_default_limit_is_eight_mebibytes() {
        assert_eq!(DryMergeLimits::default().max_blob_bytes, 8 * 1024 * 1024);
    }
}

mod merge_rule {
    use super::*;

    fn two_files(f: &Fixture) {
        f.write("a.txt", "base a\n");
        f.write("b.txt", "base b\n");
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "two"]);
    }

    fn edit(side: &'static str, rules: Option<&'static str>) -> impl FnOnce(&Fixture) {
        move |f| {
            f.write("a.txt", &format!("{side}\n"));
            f.write("b.txt", &format!("{side}\n"));
            if let Some(rules) = rules {
                f.write(".gitattributes", rules);
            }
        }
    }

    #[test]
    fn a_union_rule_in_either_tip_marks_the_file() {
        // In the tree of ours only.
        let f = Fixture::with_commit(&git_from_path());
        two_files(&f);
        let ours = commit_on(
            &f,
            "ours",
            "main",
            edit("ours", Some("a.txt merge=union\n")),
        );
        let theirs = commit_on(&f, "theirs", "main", edit("theirs", None));
        let o = merge(&f, &ours, &theirs);
        assert!(file(&o, "a.txt").has_merge_rule);
        assert!(
            !file(&o, "b.txt").has_merge_rule,
            "the rule names a.txt only"
        );
        // The rule does not resolve the conflict.
        assert_eq!(file(&o, "a.txt").kind, ConflictKind::Content);

        // In the tree of theirs only.
        let f = Fixture::with_commit(&git_from_path());
        two_files(&f);
        let ours = commit_on(&f, "ours", "main", edit("ours", None));
        let theirs = commit_on(
            &f,
            "theirs",
            "main",
            edit("theirs", Some("a.txt merge=union\n")),
        );
        let o = merge(&f, &ours, &theirs);
        assert!(file(&o, "a.txt").has_merge_rule);

        // The unset forms (`-merge`, `merge`) are also a value.
        for rule in ["a.txt -merge\n", "a.txt merge\n"] {
            let f = Fixture::with_commit(&git_from_path());
            two_files(&f);
            let ours = commit_on(&f, "ours", "main", edit("ours", None));
            let theirs = commit_on(&f, "theirs", "main", edit("theirs", Some(rule)));
            let o = merge(&f, &ours, &theirs);
            assert!(file(&o, "a.txt").has_merge_rule, "{rule:?}");
        }
    }

    #[test]
    fn ckp001_merge_rule_a_gitattributes_only_on_disk_or_in_the_index_does_not_mark() {
        let f = Fixture::with_commit(&git_from_path());
        two_files(&f);
        let ours = commit_on(&f, "ours", "main", edit("ours", None));
        let theirs = commit_on(&f, "theirs", "main", edit("theirs", None));
        f.write(".gitattributes", "a.txt merge=union\n");
        f.git(&["add", ".gitattributes"]);
        std::fs::write(f.repo.join(".git/info/attributes"), "a.txt merge=union\n").unwrap();
        let o = merge(&f, &ours, &theirs);
        assert!(!file(&o, "a.txt").has_merge_rule);
        assert!(!file(&o, "b.txt").has_merge_rule);
    }

    #[test]
    fn ckp001_merge_rule_a_rule_in_a_parent_directory_applies_to_the_file() {
        let f = Fixture::with_commit(&git_from_path());
        f.write("src/deep/c.txt", "base\n");
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "deep"]);
        let ours = commit_on(&f, "ours", "main", |f| {
            f.write("src/deep/c.txt", "ours\n");
            f.write("src/.gitattributes", "*.txt merge=union\n");
        });
        let theirs = commit_on(&f, "theirs", "main", |f| {
            f.write("src/deep/c.txt", "theirs\n")
        });
        let o = merge(&f, &ours, &theirs);
        assert!(file(&o, "src/deep/c.txt").has_merge_rule);
    }
}

mod errors {
    use super::*;

    #[test]
    fn ckp001_errors_a_missing_object_is_missing_object() {
        let (f, ours, theirs) = content_conflict();
        let blob = id(&f, &format!("{theirs}:a.txt"));
        let loose = f
            .repo
            .join(".git/objects")
            .join(&blob[..2])
            .join(&blob[2..]);
        assert!(loose.exists(), "the fixture writes loose objects");
        let mut perms = std::fs::metadata(&loose).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&loose, perms).unwrap();
        std::fs::remove_file(&loose).unwrap();
        assert_eq!(
            merge_with(&f, &ours, &theirs, &DryMergeLimits::default()),
            Err(DryMergeError::MissingObject)
        );
    }

    #[test]
    fn ckp001_errors_a_shallow_repo_without_the_merge_base_is_shallow() {
        let (f, ours, theirs) = content_conflict();
        let dst = f.root.join("shallow");
        f.git_in(
            &f.root,
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                "--no-single-branch",
                &format!("file://{}", f.repo.display()),
                dst.to_str().unwrap(),
            ],
        );
        let got = merge_in_memory(&dst, &input(&ours, &theirs), &DryMergeLimits::default());
        assert_eq!(got, Err(DryMergeError::Shallow));
    }

    #[test]
    fn ckp001_errors_a_tiny_written_bytes_cap_is_limit_exceeded() {
        let (f, ours, theirs) = content_conflict();
        let limits = DryMergeLimits {
            max_written_bytes: 1,
            ..DryMergeLimits::default()
        };
        assert_eq!(
            merge_with(&f, &ours, &theirs, &limits),
            Err(DryMergeError::LimitExceeded)
        );
    }
}

mod virtual_base {
    use super::*;

    /// A criss-cross where the two merge-bases change the same line differently, and each side
    /// resolved the other's way: the virtual base carries conflict markers.
    fn criss_cross() -> (Fixture, String, String, String, String) {
        let f = Fixture::with_commit(&git_from_path());
        f.write("f.txt", &lines("row", 9));
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "f"]);
        let edit = |side: &'static str| {
            move |f: &Fixture| {
                f.write(
                    "f.txt",
                    &lines("row", 9).replace("row5\n", &format!("{side}\n")),
                );
            }
        };
        let a1 = commit_on(&f, "a", "main", edit("a1-5"));
        let b1 = commit_on(&f, "b", "main", edit("b1-5"));
        for (branch, other, resolved) in [("a", &b1, "a-resolved"), ("b", &a1, "b-resolved")] {
            f.git(&["checkout", "-q", branch]);
            // Conflicts: that is the point.
            let _ = f
                .git_command(&f.repo, &["merge", "--no-edit", other])
                .output();
            f.write(
                "f.txt",
                &lines("row", 9).replace("row5\n", &format!("{resolved}\n")),
            );
            f.git(&["add", "f.txt"]);
            f.git(&["commit", "-q", "-m", &format!("{branch} merge")]);
        }
        f.git(&["checkout", "-q", "main"]);
        let (ma, mb) = (id(&f, "a"), id(&f, "b"));
        (f, ma, mb, a1, b1)
    }

    #[test]
    fn ckp001_virtual_base_two_merge_bases_make_a_virtual_base_with_both() {
        let (f, ma, mb, a1, b1) = criss_cross();
        let o = merge(&f, &ma, &mb);
        let MergeBaseUsed::Virtual { mut bases } = o.merge_base.clone() else {
            panic!("expected a virtual base, got {:?}", o.merge_base);
        };
        bases.sort();
        let mut want = vec![a1, b1];
        want.sort();
        assert_eq!(bases, want);
    }

    #[test]
    fn ckp001_virtual_base_markers_of_the_virtual_base_are_not_blocks() {
        let (f, ma, mb, _, _) = criss_cross();
        let o = merge(&f, &ma, &mb);
        let c = file(&o, "f.txt");
        assert_eq!(c.kind, ConflictKind::Content);
        assert_eq!(c.ours, vec![LineRange { start: 5, len: 1 }], "{c:?}");
        assert_eq!(c.theirs, vec![LineRange { start: 5, len: 1 }], "{c:?}");
    }
}

mod repo_intact {
    use super::*;

    /// NFR-01: the repo, its worktrees and everything around it are byte-for-byte and
    /// mtime-for-mtime the same after the merges, packs and object directories included.
    #[test]
    fn ckp001_intact_merge_in_memory_leaves_the_repo_as_it_was() {
        let (f, ours, theirs) = content_conflict();
        let dir_ours = commit_on(&f, "dir-ours", "main", |f| f.write("dd", "a file\n"));
        let dir_theirs = commit_on(&f, "dir-theirs", "main", |f| f.write("dd/x.txt", "x\n"));
        let big_ours = commit_on(&f, "big-ours", "main", |f| {
            f.write("a.txt", &lines("o", 400))
        });
        let big_theirs = commit_on(&f, "big-theirs", "main", |f| {
            f.write("a.txt", &lines("t", 400))
        });
        // Packs and loose objects both exist.
        f.git(&["gc", "-q"]);
        f.write("loose.txt", "loose\n");
        f.git(&["add", "loose.txt"]);
        f.git(&["commit", "-q", "-m", "loose"]);

        let before = f.fingerprint();
        fingerprint::wait_for_timestamp_tick();
        let tiny = DryMergeLimits {
            max_blob_bytes: 1024,
            ..DryMergeLimits::default()
        };
        for (o, t, limits) in [
            (&ours, &theirs, DryMergeLimits::default()),
            (&dir_ours, &dir_theirs, DryMergeLimits::default()),
            (&big_ours, &big_theirs, tiny),
        ] {
            let outcome = merge_with(&f, o, t, &limits).unwrap();
            assert!(!outcome.files.is_empty());
        }
        // The failure paths write nothing either.
        let _ = merge_with(
            &f,
            &ours,
            &theirs,
            &DryMergeLimits {
                max_written_bytes: 1,
                ..DryMergeLimits::default()
            },
        );
        let after = f.fingerprint();
        let changes = fingerprint::diff(&before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }
}
