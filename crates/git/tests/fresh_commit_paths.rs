//! The commits a ref update brings and the paths they touch (DS-US-GRD-008 D5): what the
//! forbidden-path rule reads. Read-only, on temporary repos (NFR-01).

mod common;

use common::Fixture;
use gitraptor_git::{Hide, NewCommitPaths, PathLimits, ReaderOptions, RepoReader};

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(
        &f.repo,
        &ReaderOptions {
            ignore_ambient_config: true,
            ..ReaderOptions::default()
        },
    )
    .unwrap()
}

fn rev(f: &Fixture, what: &str) -> String {
    f.git(&["rev-parse", what]).trim().to_owned()
}

fn commit(f: &Fixture, message: &str) -> String {
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", message]);
    rev(f, "HEAD")
}

const MAIN: &[&str] = &["refs/heads/main"];

fn read(
    f: &Fixture,
    old: Option<&str>,
    new: &str,
    updated: &[&str],
    hide: Hide,
    limits: &PathLimits,
) -> NewCommitPaths {
    reader(f)
        .fresh_commit_paths(old, new, updated, hide, limits)
        .unwrap()
}

fn paths(found: &NewCommitPaths) -> Vec<&str> {
    assert!(!found.unverifiable, "{found:?}");
    found.paths.iter().map(String::as_str).collect()
}

#[test]
fn a_commit_touches_what_it_modifies_creates_and_deletes() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("a.txt", "changed\n");
    f.write("dir/deep/n.txt", "new\n");
    std::fs::remove_file(f.repo.join("b.txt")).unwrap();
    let c1 = commit(&f, "change");
    let found = read(
        &f,
        Some(&c0),
        &c1,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["a.txt", "b.txt", "dir/deep/n.txt"]);
    assert_eq!(found.commits, 1);

    // Replacing a directory with a file (and the other way round) touches both sides.
    f.git(&["rm", "-rq", "dir"]);
    f.write("dir", "now a file\n");
    let c2 = commit(&f, "dir becomes a file");
    let found = read(
        &f,
        Some(&c1),
        &c2,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["dir", "dir/deep/n.txt"]);

    // A mode change alone is a change.
    f.git(&["update-index", "--chmod=+x", "a.txt"]);
    f.git(&["commit", "-q", "-m", "chmod"]);
    let c3 = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&c2),
        &c3,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["a.txt"]);
}

#[test]
fn several_new_commits_are_all_read_and_what_a_branch_holds_is_not_new() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    f.write("x/y.txt", "two\n");
    commit(&f, "two");
    std::fs::remove_file(f.repo.join("b.txt")).unwrap();
    let c3 = commit(&f, "three");
    let found = read(
        &f,
        Some(&c0),
        &c3,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["a.txt", "b.txt", "x/y.txt"]);
    assert_eq!(found.commits, 3);

    // Another branch already reaches `one`: only `two` and `three` are new.
    f.git(&["branch", "keep", &c1]);
    let found = read(
        &f,
        Some(&c0),
        &c3,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["b.txt", "x/y.txt"]);
    assert_eq!(found.commits, 2);

    // A fast-forward to what a branch already holds brings nothing.
    f.git(&["branch", "all", &c3]);
    let found = read(
        &f,
        Some(&c0),
        &c3,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), Vec::<&str>::new());
    assert_eq!(found.commits, 0);

    // A tag does not hide anything: an agent writes tags without any evaluation.
    f.git(&["branch", "-D", "all"]);
    f.git(&["branch", "-D", "keep"]);
    f.git(&["tag", "parked", &c3]);
    let found = read(
        &f,
        Some(&c0),
        &c3,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(found.commits, 3);
}

#[test]
fn an_amend_is_the_whole_commit_against_the_parents_of_the_old_tip() {
    let f = Fixture::with_commit();
    f.write("a.txt", "one\n");
    let old = commit(&f, "one");
    f.write("secret.txt", "s\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "--amend", "-m", "one amended"]);
    let new = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&old),
        &new,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    // The original change of the commit counts too: the commit as a whole is new.
    assert_eq!(paths(&found), ["a.txt", "secret.txt"]);
}

#[test]
fn a_merge_counts_only_what_differs_from_every_parent() {
    let f = Fixture::with_commit();
    f.git(&["switch", "-q", "-c", "side"]);
    f.write("s.txt", "side\n");
    f.write("a.txt", "side\n");
    commit(&f, "side");
    f.git(&["switch", "-q", "main"]);
    f.write("m.txt", "main\n");
    f.write("a.txt", "main\n");
    let tip = commit(&f, "main");
    // A clean merge brings in what already exists: nothing is touched by the merge itself…
    f.write("a.txt", "main\n");
    let mut merge = f.git_command(&f.repo, &["merge", "-q", "--no-ff", "-m", "merge", "side"]);
    assert!(
        !merge.output().unwrap().status.success(),
        "conflict on a.txt"
    );
    // …and the one path it resolves is.
    f.write("a.txt", "resolved\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "merge"]);
    let new = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&tip),
        &new,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["a.txt"]);
    assert_eq!(found.commits, 1);
}

#[test]
fn a_root_commit_touches_everything_it_holds() {
    let f = Fixture::with_commit();
    f.git(&["checkout", "-q", "--orphan", "other"]);
    let root = commit(&f, "root");
    let found = read(
        &f,
        None,
        &root,
        &["refs/heads/other"],
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["a.txt", "b.txt"]);
}

#[test]
fn a_push_hides_only_the_remote_tracking_branches() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.git(&["update-ref", "refs/remotes/origin/main", &c0]);
    f.git(&["switch", "-q", "-c", "feat"]);
    f.write("secrets/a.txt", "s\n");
    let c1 = commit(&f, "secret");
    // The local branch `feat` holds it, but what leaves is judged whole.
    let found = read(
        &f,
        None,
        &c1,
        &[],
        Hide::RemoteTracking,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["secrets/a.txt"]);
    // Once the remote has it, it is not new.
    f.git(&["update-ref", "refs/remotes/origin/feat", &c1]);
    let found = read(
        &f,
        None,
        &c1,
        &[],
        Hide::RemoteTracking,
        &PathLimits::default(),
    );
    assert_eq!(found.commits, 0);
    assert!(!found.unverifiable);
}

#[test]
fn an_annotated_tag_is_read_as_its_commit_and_anything_else_cannot_be_verified() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("secrets/a.txt", "s\n");
    let c1 = commit(&f, "secret");
    f.git(&["tag", "-a", "-m", "release", "v1", &c1]);
    let tag = rev(&f, "v1");
    assert_ne!(tag, c1);
    // A push of the tag object to a ref that is not a branch brings the commit it peels to.
    let found = read(
        &f,
        Some(&c0),
        &tag,
        &[],
        Hide::OldOnly,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["secrets/a.txt"]);
    // A blob, or an object the repo does not have, cannot be verified, never "nothing".
    let blob = f.git(&["hash-object", "-w", "a.txt"]).trim().to_owned();
    for what in [blob, "e".repeat(40)] {
        let found = read(
            &f,
            None,
            &what,
            MAIN,
            Hide::OtherBranches,
            &PathLimits::default(),
        );
        assert!(found.unverifiable, "{what}");
    }
}

/// `git mktree` of `lines` (`<mode> <type> <id>\t<name>`).
fn mktree(f: &Fixture, lines: &str) -> String {
    use std::io::Write;
    let mut child = f
        .git_command(&f.repo, &["mktree"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A commit of `tree` on top of `parent`, reached by no ref.
fn commit_of(f: &Fixture, tree: &str, parent: &str) -> String {
    f.git(&["commit-tree", tree, "-p", parent, "-m", "x"])
        .trim()
        .to_owned()
}

#[test]
fn a_tree_too_deep_or_a_path_too_long_cannot_be_verified() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    let blob = f.git(&["hash-object", "-w", "a.txt"]).trim().to_owned();
    let mut tree = mktree(&f, &format!("100644 blob {blob}\tleaf\n"));
    for _ in 0..6 {
        tree = mktree(&f, &format!("040000 tree {tree}\td\n"));
    }
    let c1 = commit_of(&f, &tree, &c0);
    let default = PathLimits::default();
    let found = read(&f, Some(&c0), &c1, MAIN, Hide::OtherBranches, &default);
    assert_eq!(paths(&found), ["a.txt", "b.txt", "d/d/d/d/d/d/leaf"]);
    // The same commit past the depth bound, and past the path bound.
    let shallow = PathLimits {
        depth: 3,
        ..default
    };
    assert!(read(&f, Some(&c0), &c1, MAIN, Hide::OtherBranches, &shallow).unverifiable);
    let short = PathLimits {
        path_bytes: 8,
        ..default
    };
    assert!(read(&f, Some(&c0), &c1, MAIN, Hide::OtherBranches, &short).unverifiable);
}

#[test]
fn a_submodule_is_reported_as_a_directory_and_a_malformed_tree_cannot_be_verified() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    // A submodule pointer at `vendor/keys`.
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{c0},vendor/keys"),
    ]);
    f.git(&["commit", "-q", "-m", "submodule"]);
    let c1 = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&c0),
        &c1,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["vendor/keys/"]);

    // Two entries with one name: one could hide the other.
    let blob = f.git(&["hash-object", "-w", "a.txt"]).trim().to_owned();
    let other = f.git(&["hash-object", "-w", "b.txt"]).trim().to_owned();
    let mut raw = Vec::new();
    for id in [&blob, &other] {
        raw.extend_from_slice(b"100644 secrets\0");
        for pair in id.as_bytes().chunks(2) {
            raw.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
    }
    use std::io::Write;
    let mut child = f
        .git_command(
            &f.repo,
            &["hash-object", "-w", "-t", "tree", "--literally", "--stdin"],
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&raw).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let tree = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let c2 = commit_of(&f, &tree, &c1);
    let found = read(
        &f,
        Some(&c1),
        &c2,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert!(found.unverifiable, "{found:?}");
}

#[test]
fn a_symbolic_ref_hides_nothing() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("secrets/a.txt", "s\n");
    let c1 = commit(&f, "secret");
    // The commit is parked under a tag (not a hider) and a branch points at the tag.
    f.git(&["tag", "parked", &c1]);
    f.git(&["reset", "-q", "--hard", &c0]);
    f.git(&["symbolic-ref", "refs/heads/alias", "refs/tags/parked"]);
    let found = read(
        &f,
        Some(&c0),
        &c1,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["secrets/a.txt"]);
    assert_eq!(found.commits, 1);
}

#[test]
fn every_bound_ends_in_unverifiable_never_in_nothing() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    for n in 0..3 {
        f.write(&format!("f{n}.txt"), "x\n");
        commit(&f, &format!("c{n}"));
    }
    let c3 = rev(&f, "HEAD");
    let tight = |limits: PathLimits| read(&f, Some(&c0), &c3, MAIN, Hide::OtherBranches, &limits);
    let default = PathLimits::default();

    assert!(!tight(default).unverifiable);
    // More new commits than allowed.
    assert!(
        tight(PathLimits {
            commits: 2,
            ..default
        })
        .unverifiable
    );
    // More commits visited than allowed.
    assert!(
        tight(PathLimits {
            visited: 2,
            ..default
        })
        .unverifiable
    );
    // More changed paths than allowed.
    assert!(
        tight(PathLimits {
            paths: 2,
            ..default
        })
        .unverifiable
    );
    // More tree entries than allowed.
    assert!(
        tight(PathLimits {
            entries: 3,
            ..default
        })
        .unverifiable
    );
    // More branch tips than hidden: the rest hide nothing, so what they hold counts as new
    // (never as nothing): a superset, not a verdict.
    f.git(&["branch", "other", &c3]);
    let found = tight(PathLimits { tips: 0, ..default });
    assert!(!found.unverifiable);
    assert_eq!(found.commits, 3);
    assert_eq!(tight(default).commits, 0);
}

#[test]
fn an_object_missing_from_the_repo_cannot_be_verified() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write("a.txt", "one\n");
    let c1 = commit(&f, "one");
    f.write("a.txt", "two\n");
    let c2 = commit(&f, "two");
    // A partial or shallow clone lacks the parent of a new commit: delete its loose object.
    let object = f
        .repo
        .join(format!(".git/objects/{}/{}", &c1[..2], &c1[2..]));
    let mut perms = std::fs::metadata(&object).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&object, perms).unwrap();
    std::fs::remove_file(&object).unwrap();
    let found = read(
        &f,
        Some(&c0),
        &c2,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert!(found.unverifiable, "{found:?}");
}

// File names are bytes only on Unix.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_reported_lossy() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    let blob = f.git(&["hash-object", "-w", "a.txt"]).trim().to_owned();
    let name = std::ffi::OsStr::from_bytes(b"secrets/x\xffy.txt");
    // The path is part of the same argument: build it byte by byte.
    let mut arg = std::ffi::OsString::from(format!("100644,{blob},"));
    arg.push(name);
    let mut index = f.git_command(&f.repo, &["update-index", "--add", "--cacheinfo"]);
    index.arg(arg);
    assert!(index.output().unwrap().status.success());
    f.git(&["commit", "-q", "-m", "odd name"]);
    let new = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&c0),
        &new,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&found), ["secrets/x\u{fffd}y.txt"]);
}

#[test]
fn a_commit_parked_under_remote_tracking_is_still_new_for_local_branches_only() {
    let f = Fixture::with_commit();
    let c0 = rev(&f, "HEAD");
    f.write(".gitraptor/settings.json", "{}\n");
    let bad = commit(&f, "config");
    f.git(&["reset", "-q", "--hard", &c0]);
    f.git(&["update-ref", "refs/remotes/origin/x", &bad]);
    f.git(&["branch", "keeps", &bad]);
    // Remote-tracking branches hide it (a ref update before the change), local ones do not hide
    // what only a remote-tracking branch holds.
    let hidden = read(
        &f,
        Some(&c0),
        &bad,
        MAIN,
        Hide::OtherBranches,
        &PathLimits::default(),
    );
    assert_eq!(hidden.commits, 0);
    // `keeps` is a local branch outside the update: it hides the commit...
    let local = read(
        &f,
        Some(&c0),
        &bad,
        MAIN,
        Hide::LocalBranches,
        &PathLimits::default(),
    );
    assert_eq!(local.commits, 0);
    // ...but once only the remote-tracking branch holds it, it is new.
    f.git(&["branch", "-D", "keeps"]);
    let local = read(
        &f,
        Some(&c0),
        &bad,
        MAIN,
        Hide::LocalBranches,
        &PathLimits::default(),
    );
    assert_eq!(paths(&local), [".gitraptor/settings.json"]);
    assert_eq!(local.commits, 1);
}

#[test]
fn the_root_entries_carry_name_mode_and_id_and_are_bounded() {
    let f = Fixture::with_commit();
    f.write(".gitraptor/settings.json", "{}\n");
    f.write("run.sh", "#!/bin/sh\n");
    f.git(&["add", "-A"]);
    f.git(&["update-index", "--chmod=+x", "run.sh"]);
    f.git(&["commit", "-q", "-m", "more"]);
    let c = rev(&f, "HEAD");
    let entries = reader(&f).root_entries(&c, &PathLimits::default()).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, [".gitraptor", "a.txt", "b.txt", "run.sh"]);
    assert_eq!(entries[0].mode, 0o040000);
    assert_eq!(entries[3].mode, 0o100755);
    assert_eq!(entries[0].oid, rev(&f, "HEAD:.gitraptor"));
    // More entries than the bound, and an object that is not a commit, are errors.
    let tight = PathLimits {
        entries: 2,
        ..PathLimits::default()
    };
    assert!(reader(&f).root_entries(&c, &tight).is_err());
    assert!(
        reader(&f)
            .root_entries(&rev(&f, "HEAD:a.txt"), &PathLimits::default())
            .is_err()
    );
}
