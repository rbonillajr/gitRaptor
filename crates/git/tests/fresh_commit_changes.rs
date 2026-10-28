//! The commits a ref update brings, with the lines they change per file (DS-US-GRD-009 D6, D7,
//! D15, T004). `git diff --numstat -M` is the oracle: the reader must count what Git counts on a
//! corpus (add, delete, modify, CRLF, exact and partial rename, binary, submodule, mode change,
//! root commit, amend). Read-only, on temporary repos (NFR-01).
//!
//! Verified on the runner's Git (2.50.1, macOS) while writing this file (T001 Reglas):
//! - `diff.renameLimit` is unset by default (`git config --get diff.renameLimit` exits 1), so
//!   Git's own default applies. D7's ASSUMPTION ("hasta 1 000 candidatos") does NOT hold on this
//!   Git: 1 100 inexact renames (94 % similar) were still all detected by a plain
//!   `git diff --numstat -M`, so the compiled-in default is above 1 000 (7 000 in recent Gits).
//!   The corpus stays far below either number; the plan's 1 000 should read "the limit of the
//!   runner's Git" or be pinned by a test with more candidates.
//! - `git revert` writes `Revert "<subject>"` + a blank line + `This reverts commit <oid>.`, and
//!   reverting a revert writes `Reapply "<subject>"` with the same `This reverts commit <oid>.`
//!   line (D9 b). `git revert` has no `-q`.

mod common;

use std::collections::BTreeMap;

use common::Fixture;
use gitraptor_git::{
    CommitChange, Hide, LineLimits, NewCommitChanges, PathLimits, ReaderOptions, RepoReader,
};

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

fn bytes(f: &Fixture, rela: &str, content: &[u8]) {
    let path = f.repo.join(rela);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn numbered(prefix: &str, n: usize) -> String {
    (1..=n).map(|i| format!("{prefix} {i}\n")).collect()
}

const MAIN: &[&str] = &["refs/heads/main"];

fn lines_limits() -> LineLimits {
    LineLimits::default()
}

fn read(
    f: &Fixture,
    old: Option<&str>,
    new: &str,
    hide: Hide,
    limits: &PathLimits,
    lines: Option<&LineLimits>,
) -> NewCommitChanges {
    reader(f)
        .fresh_commit_changes(old, new, MAIN, hide, limits, lines)
        .unwrap()
}

/// What `git diff --numstat -M -z` says of `parent..commit`: new path → added + deleted, entries
/// with no changed line left out (a pure rename, a binary, a mode change count 0).
fn oracle(f: &Fixture, parent: Option<&str>, commit: &str) -> BTreeMap<String, u32> {
    let empty_tree = f.git(&["hash-object", "-t", "tree", "/dev/null"]);
    let parent = parent.map_or_else(|| empty_tree.trim().to_owned(), str::to_owned);
    let out = f
        .git_command(&f.repo, &["diff", "--numstat", "-M", "-z", &parent, commit])
        .output()
        .unwrap();
    assert!(out.status.success(), "the oracle itself failed");
    let mut tokens = out.stdout.split(|b| *b == 0).map(String::from_utf8_lossy);
    let mut found = BTreeMap::new();
    while let Some(head) = tokens.next() {
        if head.is_empty() {
            continue;
        }
        let mut parts = head.splitn(3, '\t');
        let counted = |s: Option<&str>| s.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let (added, deleted) = (counted(parts.next()), counted(parts.next()));
        let path = match parts.next() {
            Some("") => {
                tokens.next(); // the old path of a rename
                tokens
                    .next()
                    .expect("the new path of a rename")
                    .into_owned()
            }
            Some(path) => path.to_owned(),
            None => panic!("unexpected numstat record {head:?}"),
        };
        if added + deleted > 0 {
            found.insert(path, added + deleted);
        }
    }
    found
}

/// What the reader reports for one commit, in the same shape as [`oracle`].
fn reported(change: &CommitChange) -> BTreeMap<String, u32> {
    change
        .files
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|(_, lines)| *lines > 0)
        .map(|(path, lines)| (path.clone(), *lines))
        .collect()
}

fn change<'a>(found: &'a NewCommitChanges, oid: &str) -> &'a CommitChange {
    assert!(!found.unverifiable, "{found:?}");
    found
        .commits
        .iter()
        .find(|c| c.oid == oid)
        .unwrap_or_else(|| panic!("commit {oid} is not among the new ones: {found:?}"))
}

fn assert_matches_git(f: &Fixture, parent: Option<&str>, commit: &str, found: &NewCommitChanges) {
    let c = change(found, commit);
    assert!(!c.partial, "nothing is cut on a small corpus: {c:?}");
    assert!(c.files.is_some(), "the lines were asked for: {c:?}");
    assert_eq!(
        reported(c),
        oracle(f, parent, commit),
        "lines per file must equal `git diff --numstat -M`"
    );
}

/// The corpus: a root commit, then one commit with a bit of everything.
struct Corpus {
    f: Fixture,
    root: String,
    mixed: String,
}

fn corpus() -> Corpus {
    let f = Fixture::new();
    f.write("a.txt", "one\ntwo\nthree\n");
    f.write("crlf.txt", "l1\r\nl2\r\nl3\r\nl4\r\n");
    f.write("del.txt", &numbered("gone", 5));
    f.write("ren.txt", &numbered("exact", 100));
    f.write("part.txt", &numbered("partial", 40));
    bytes(&f, "bin.dat", b"\x00\x01\x02binary\x00");
    f.write("mode.sh", "#!/bin/sh\n");
    let root = commit(&f, "feat: root");

    f.write("a.txt", "one\nTWO\nthree\nfour\n");
    f.write("new.txt", &numbered("fresh", 7));
    std::fs::remove_file(f.repo.join("del.txt")).unwrap();
    // CRLF -> LF: every line changes (the line ending counts).
    f.write("crlf.txt", "l1\nl2\nl3\nl4\n");
    // Exact rename: 0 lines.
    std::fs::create_dir_all(f.repo.join("moved")).unwrap();
    std::fs::rename(f.repo.join("ren.txt"), f.repo.join("moved/ren.txt")).unwrap();
    // Partial rename: 4 of 40 lines change (similarity well over 50 %).
    std::fs::remove_file(f.repo.join("part.txt")).unwrap();
    let mut partial = numbered("partial", 40);
    partial = partial.replacen("partial 5\n", "changed 5\n", 1);
    partial = partial.replacen("partial 6\n", "changed 6\n", 1);
    partial = partial.replacen("partial 7\n", "changed 7\n", 1);
    partial = partial.replacen("partial 8\n", "changed 8\n", 1);
    f.write("part2.txt", &partial);
    // Binary: modified bytes count 0.
    bytes(&f, "bin.dat", b"\x00\x01\x02other binary\x00\x00");
    // Mode change alone: 0.
    f.git(&["update-index", "--chmod=+x", "mode.sh"]);
    // Submodule: a gitlink, one line per side as `--numstat` says.
    f.git(&["add", "-A"]);
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{root},vendor/sub"),
    ]);
    f.git(&["commit", "-q", "-m", "fix: everything at once"]);
    let mixed = rev(&f, "HEAD");
    Corpus { f, root, mixed }
}

#[test]
fn a_root_commit_counts_against_the_empty_tree() {
    let c = corpus();
    let found = read(
        &c.f,
        None,
        &c.root,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&lines_limits()),
    );
    assert_matches_git(&c.f, None, &c.root, &found);
    // a.txt 3 + crlf 4 + del 5 + ren 100 + part 40 + mode.sh 1 = 153 (the binary counts 0).
    let total: u32 = reported(change(&found, &c.root)).values().sum();
    assert_eq!(total, 153, "the root adds every line of the text files");
}

#[test]
fn lines_per_file_equal_git_numstat_over_the_corpus() {
    let c = corpus();
    let found = read(
        &c.f,
        Some(&c.root),
        &c.mixed,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&lines_limits()),
    );
    assert_matches_git(&c.f, Some(&c.root), &c.mixed, &found);
    let got = reported(change(&found, &c.mixed));
    // Spelled out, so a bug that git and the reader share cannot hide behind the oracle.
    assert_eq!(got.get("a.txt"), Some(&3), "modify: +2 -1 -> {got:?}");
    assert_eq!(got.get("new.txt"), Some(&7), "add -> {got:?}");
    assert_eq!(got.get("del.txt"), Some(&5), "delete -> {got:?}");
    assert_eq!(
        got.get("crlf.txt"),
        Some(&8),
        "CRLF -> LF counts -> {got:?}"
    );
    assert!(
        !got.contains_key("moved/ren.txt"),
        "an exact rename counts 0 -> {got:?}"
    );
    assert_eq!(got.get("part2.txt"), Some(&8), "partial rename -> {got:?}");
    assert!(
        !got.contains_key("bin.dat") && !got.contains_key("mode.sh"),
        "a binary and a mode change count 0 -> {got:?}"
    );
}

#[test]
fn an_amended_commit_counts_whole_against_the_parent_of_the_original() {
    let c = corpus();
    c.f.write("amended.txt", &numbered("amend", 9));
    c.f.git(&["add", "-A"]);
    c.f.git(&["commit", "-q", "--amend", "--no-edit"]);
    let amended = rev(&c.f, "HEAD");
    assert_ne!(amended, c.mixed);
    // The ref moves from the original to the amended commit: only the amended one is new.
    let found = read(
        &c.f,
        Some(&c.mixed),
        &amended,
        Hide::OldOnly,
        &PathLimits::default(),
        Some(&lines_limits()),
    );
    assert_eq!(found.commits.len(), 1, "{found:?}");
    assert_matches_git(&c.f, Some(&c.root), &amended, &found);
    assert_eq!(
        reported(change(&found, &amended)).get("amended.txt"),
        Some(&9)
    );
}

#[test]
fn a_merge_counts_no_lines() {
    let f = Fixture::with_commit();
    let base = rev(&f, "HEAD");
    f.git(&["switch", "-q", "-c", "side"]);
    f.write("side.txt", &numbered("side", 30));
    commit(&f, "feat: side");
    f.git(&["switch", "-q", "main"]);
    f.write("main.txt", &numbered("main", 20));
    commit(&f, "feat: main");
    f.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge branch 'side'",
        "side",
    ]);
    let merge = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&base),
        &merge,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&lines_limits()),
    );
    assert_eq!(
        found.commits.len(),
        2,
        "the main commit and the merge (`side` holds its own commit and is hidden): {found:?}"
    );
    let m = change(&found, &merge);
    assert_eq!(
        m.files
            .as_deref()
            .map_or(0, |files| files.iter().map(|f| f.1).sum()),
        0,
        "a merge counts no lines (D6): {m:?}"
    );
    assert_eq!(m.head, b"Merge branch 'side'");
}

#[test]
fn the_header_is_the_first_line_and_says_if_the_second_is_blank() {
    let f = Fixture::with_commit();
    let base = rev(&f, "HEAD");
    f.write("x.txt", "x\n");
    f.git(&["add", "-A"]);
    f.git(&[
        "commit",
        "-q",
        "-m",
        "feat(core)!: split it\n\nthe body\n\nCo-Authored-By: a",
    ]);
    let separated = rev(&f, "HEAD");
    f.write("y.txt", "y\n");
    f.git(&["add", "-A"]);
    f.git(&[
        "commit",
        "-q",
        "--cleanup=verbatim",
        "-m",
        "fix: glued\nthe body",
    ]);
    let glued = rev(&f, "HEAD");
    f.write("z.txt", "z\n");
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", &"x".repeat(5000)]);
    let long = rev(&f, "HEAD");
    let found = read(
        &f,
        Some(&base),
        &long,
        Hide::OtherBranches,
        &PathLimits::default(),
        None,
    );
    assert!(!found.unverifiable, "{found:?}");
    let a = change(&found, &separated);
    assert_eq!(a.head, b"feat(core)!: split it");
    assert!(a.body_separated, "a blank second line separates the body");
    let b = change(&found, &glued);
    assert_eq!(b.head, b"fix: glued");
    assert!(!b.body_separated, "a body glued to the first line");
    let c = change(&found, &long);
    assert!(
        c.head.len() <= 4096,
        "the header is bounded: {}",
        c.head.len()
    );
    // `lines = None`: only headers, no tree is read.
    assert!(
        found.commits.iter().all(|c| c.files.is_none()),
        "no line was asked for: {found:?}"
    );
}

#[test]
fn what_another_branch_holds_is_not_new() {
    let f = Fixture::with_commit();
    let base = rev(&f, "HEAD");
    f.write("one.txt", "1\n");
    let one = commit(&f, "feat: one");
    f.write("two.txt", "2\n");
    let two = commit(&f, "feat: two");
    f.git(&["branch", "keep", &one]);
    let found = read(
        &f,
        Some(&base),
        &two,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&lines_limits()),
    );
    assert_eq!(found.commits.len(), 1, "{found:?}");
    assert_eq!(found.commits[0].oid, two);
}

#[test]
fn bounds_are_reported_never_read_as_zero_lines() {
    let f = Fixture::with_commit();
    let base = rev(&f, "HEAD");
    // One blob over the per-blob bound.
    f.write("huge.txt", &numbered("huge", 400));
    let tip = commit(&f, "feat: huge");
    let small = LineLimits {
        blob_bytes: 512,
        ..LineLimits::default()
    };
    let found = read(
        &f,
        Some(&base),
        &tip,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&small),
    );
    assert!(
        found.unverifiable || found.commits.iter().any(|c| c.partial || c.files.is_none()),
        "a blob over the bound must be flagged, not counted as 0: {found:?}"
    );
    // Total bytes read over the bound.
    let total = LineLimits {
        total_bytes: 512,
        ..LineLimits::default()
    };
    let found = read(
        &f,
        Some(&base),
        &tip,
        Hide::OtherBranches,
        &PathLimits::default(),
        Some(&total),
    );
    assert!(
        found.unverifiable || found.commits.iter().any(|c| c.partial || c.files.is_none()),
        "the total bound must be flagged: {found:?}"
    );
    // More changed files than `PathLimits.paths`.
    f.write("p1.txt", "1\n");
    f.write("p2.txt", "2\n");
    f.write("p3.txt", "3\n");
    f.write("p4.txt", "4\n");
    let tip2 = commit(&f, "feat: many files");
    let few = PathLimits {
        paths: 2,
        ..PathLimits::default()
    };
    let found = read(
        &f,
        Some(&tip),
        &tip2,
        Hide::OtherBranches,
        &few,
        Some(&lines_limits()),
    );
    assert!(
        found.unverifiable || found.commits.iter().any(|c| c.partial || c.files.is_none()),
        "past the files bound the count is cut and says so: {found:?}"
    );
    // More new commits than the bound.
    f.write("q.txt", "q\n");
    let tip3 = commit(&f, "feat: q");
    let two_commits = PathLimits {
        commits: 1,
        ..PathLimits::default()
    };
    let found = read(
        &f,
        Some(&tip),
        &tip3,
        Hide::OtherBranches,
        &two_commits,
        Some(&lines_limits()),
    );
    assert!(found.unverifiable, "more commits than the bound: {found:?}");
}
