//! US-CKP-022 T002: `RepoReader::lane` lists the commits of a branch over the confirmed base,
//! newest first, with an exact total up to a walk limit, and writes nothing (NFR-01, D4, D6, D12).
//! The types (`gitraptor_git::lane`) are the ones of DS-US-CKP-022 § Contratos compartidos.

use std::cell::Cell;

use gitraptor_git::lane::{GitLaneGap, LaneLimits};
use gitraptor_git::{Count, ReaderOptions, RepoReader};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

/// Seconds since the epoch of the first commit made by these tests (after the fixture's own).
const T0: i64 = 1_800_000_000;

fn rev(f: &Fixture, name: &str) -> String {
    f.git(&["rev-parse", name]).trim().to_owned()
}

/// A commit with a file of its own and a committer date of `secs`.
fn commit_at(f: &Fixture, file: &str, message: &str, secs: i64) -> String {
    f.write(file, &format!("{file} {secs}\n"));
    f.git(&["add", file]);
    let date = format!("{secs} +0000");
    let out = f
        .git_command(&f.repo, &["commit", "-q", "-m", message])
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_AUTHOR_NAME", "Zed Author")
        .env("GIT_AUTHOR_EMAIL", "zed.author@secret.example")
        .output()
        .expect("run git commit");
    assert!(
        out.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    rev(f, "HEAD")
}

fn limits(max_commits: usize, max_walk: u64) -> LaneLimits {
    LaneLimits {
        max_commits,
        max_walk,
        max_summary_bytes: 256,
    }
}

fn reader(f: &Fixture) -> RepoReader {
    RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
}

fn never() -> bool {
    false
}

/// `feature` forks from the initial commit of `main`; `main` then moves on one commit.
/// Returns (fork point, tip of main).
fn fork(f: &Fixture) -> (String, String) {
    let fork = rev(f, "HEAD");
    f.git(&["checkout", "-q", "-b", "feature"]);
    f.git(&["checkout", "-q", "main"]);
    let main_tip = commit_at(f, "m1.txt", "main moves on", T0);
    f.git(&["checkout", "-q", "feature"]);
    (fork, main_tip)
}

#[test]
fn lane_walks_head_minus_base_newest_first() {
    let f = Fixture::with_commit(&git_from_path());
    let (fork_point, main_tip) = fork(&f);

    // f1 (t+10), f2 (t+20), then a side branch from the fork with a LATER commit (t+30) that
    // `feature` merges at t+40. By commit time: merge, s1, f2, f1. A breadth-first walk would
    // give merge, f2, s1, f1.
    let f1 = commit_at(&f, "f1.txt", "feature one", T0 + 10);
    let f2 = commit_at(&f, "f2.txt", "feature two", T0 + 20);
    f.git(&["checkout", "-q", "-b", "side", &fork_point]);
    let s1 = commit_at(&f, "s1.txt", "side one", T0 + 30);
    f.git(&["checkout", "-q", "feature"]);
    let date = format!("{} +0000", T0 + 40);
    let out = f
        .git_command(
            &f.repo,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "merge side into feature",
                "side",
            ],
        )
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_AUTHOR_DATE", &date)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let merge = rev(&f, "HEAD");

    let lane = reader(&f)
        .lane(&merge, &main_tip, &limits(50, 10_000), &never)
        .unwrap()
        .unwrap_or_else(|_| panic!("a branch with a merge-base must be a lane"));

    assert_eq!(
        lane.merge_base, fork_point,
        "the merge-base shown is the fork point with the base (D4)"
    );
    let ids: Vec<&str> = lane.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![merge.as_str(), s1.as_str(), f2.as_str(), f1.as_str()],
        "HEAD minus base, newest commit first by commit time, and the base's own commit left out"
    );
    assert!(
        !ids.contains(&main_tip.as_str()),
        "a base-only commit is not in the lane"
    );
    let times: Vec<i64> = lane.commits.iter().map(|c| c.committed_s).collect();
    assert_eq!(times, vec![T0 + 40, T0 + 30, T0 + 20, T0 + 10]);
    let merges: Vec<bool> = lane.commits.iter().map(|c| c.merge).collect();
    assert_eq!(
        merges,
        vec![true, false, false, false],
        "only the commit with two parents is flagged merge"
    );
    assert_eq!(lane.commits[0].summary, "merge side into feature");
    assert_eq!(lane.commits[3].summary, "feature one");
    assert!(
        matches!(lane.total, Count::Exact(4)),
        "total is the four commits"
    );
}

#[test]
fn lane_total_is_exact_until_the_walk_limit() {
    let f = Fixture::with_commit(&git_from_path());
    let (_, main_tip) = fork(&f);
    let mut ids = Vec::new();
    for n in 0..12 {
        ids.push(commit_at(
            &f,
            &format!("c{n}.txt"),
            &format!("c{n}"),
            T0 + 100 + n,
        ));
    }
    let head = rev(&f, "HEAD");
    let r = reader(&f);

    let get = |max_commits: usize, max_walk: u64| {
        r.lane(&head, &main_tip, &limits(max_commits, max_walk), &never)
            .unwrap()
            .unwrap_or_else(|_| panic!("expected a lane"))
    };

    let lane = get(5, 100);
    assert_eq!(lane.commits.len(), 5, "only max_commits are kept");
    let newest: Vec<&str> = ids.iter().rev().take(5).map(String::as_str).collect();
    let got: Vec<&str> = lane.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(got, newest, "the kept commits are the newest ones");
    assert!(
        matches!(lane.total, Count::Exact(12)),
        "the rest is still counted: 12 commits, exact below the walk limit"
    );

    assert!(
        matches!(get(5, 12).total, Count::Exact(12)),
        "a walk limit equal to the total is still exact"
    );
    assert!(
        matches!(get(5, 11).total, Count::AtLeast(11)),
        "one past the walk limit becomes AtLeast(limit)"
    );
    let capped = get(5, 10);
    assert!(matches!(capped.total, Count::AtLeast(10)));
    assert_eq!(
        capped.commits.len(),
        5,
        "a capped total does not shrink the window"
    );
}

#[test]
fn lane_without_merge_base_is_a_gap() {
    let f = Fixture::with_commit(&git_from_path());
    let main_tip = rev(&f, "HEAD");
    f.git(&["checkout", "-q", "--orphan", "island"]);
    f.git(&["rm", "-rfq", "."]);
    let island = commit_at(&f, "z.txt", "unrelated history", T0);

    let stops = Cell::new(0u32);
    let stop = || {
        stops.set(stops.get() + 1);
        false
    };
    let got = reader(&f)
        .lane(&island, &main_tip, &limits(50, 10_000), &stop)
        .unwrap();
    assert!(
        matches!(got, Err(GitLaneGap::NoMergeBase)),
        "histories without a common ancestor are a NoMergeBase gap, not a lane or an error"
    );
    assert_eq!(stops.get(), 0, "with no merge-base nothing is walked");
}

#[test]
fn summary_is_the_first_line_cut_on_a_char_boundary() {
    let f = Fixture::with_commit(&git_from_path());
    let (_, main_tip) = fork(&f);
    commit_at(
        &f,
        "a1.txt",
        "first line only\n\nbody that must not appear",
        T0 + 1,
    );
    commit_at(&f, "a2.txt", "abcdefghij", T0 + 2);
    commit_at(&f, "a3.txt", "ééééé", T0 + 3); // 2 bytes each
    commit_at(&f, "a4.txt", "😀😀😀", T0 + 4); // 4 bytes each
    let head = rev(&f, "HEAD");

    let summaries = |max_summary_bytes: usize| -> Vec<String> {
        let l = LaneLimits {
            max_commits: 50,
            max_walk: 10_000,
            max_summary_bytes,
        };
        reader(&f)
            .lane(&head, &main_tip, &l, &never)
            .unwrap()
            .unwrap_or_else(|_| panic!("expected a lane"))
            .commits
            .into_iter()
            .map(|c| c.summary)
            .collect()
    };

    // Newest first: 😀, é, abc, first line.
    assert_eq!(
        summaries(256),
        vec!["😀😀😀", "ééééé", "abcdefghij", "first line only"],
        "the summary is the first line of the message, never its body"
    );
    assert_eq!(
        summaries(6),
        vec!["😀", "ééé", "abcdef", "first "],
        "cut at max_summary_bytes without splitting a UTF-8 character (6 bytes: one emoji, three é)"
    );
    assert_eq!(
        summaries(5),
        vec!["😀", "éé", "abcde", "first"],
        "a limit inside a character falls back to the previous boundary"
    );
    for s in summaries(7) {
        assert!(s.len() <= 7, "{s:?} is longer than max_summary_bytes");
        assert!(
            !s.contains('@') && !s.contains("Zed"),
            "no author or email in the summary"
        );
    }
}

#[test]
fn lane_total_equals_ahead_behind_commits() {
    let f = Fixture::with_commit(&git_from_path());
    let (fork_point, main_tip) = fork(&f);
    for n in 0..6 {
        commit_at(&f, &format!("c{n}.txt"), &format!("c{n}"), T0 + 100 + n);
    }
    f.git(&["checkout", "-q", "-b", "side", &fork_point]);
    commit_at(&f, "s.txt", "side", T0 + 200);
    f.git(&["checkout", "-q", "feature"]);
    f.git(&["merge", "-q", "--no-ff", "-m", "merge", "side"]);
    let head = rev(&f, "HEAD");
    let r = reader(&f);

    for walk in [3u64, 7, 8, 9, 10_000] {
        let lane = r
            .lane(&head, &main_tip, &limits(50, walk), &never)
            .unwrap()
            .unwrap_or_else(|_| panic!("expected a lane"));
        let (ahead, _behind) = r.ahead_behind_commits(&head, &main_tip, walk).unwrap();
        assert_eq!(
            lane.total, ahead,
            "the total of the lane is the ↑ of the stream (same set, same walk limit {walk}): D4"
        );
    }
}

#[test]
fn lane_writes_nothing() {
    let f = Fixture::with_commit(&git_from_path());
    let (_, main_tip) = fork(&f);
    for n in 0..5 {
        commit_at(&f, &format!("c{n}.txt"), &format!("c{n}"), T0 + 100 + n);
    }
    let head = rev(&f, "HEAD");
    let report = check(
        "lane on a repo with history",
        &f,
        &Exceptions::engine_profile("profile"),
        || {
            let lane = reader(&f)
                .lane(&head, &main_tip, &limits(50, 10_000), &never)
                .unwrap()
                .unwrap_or_else(|_| panic!("expected a lane"));
            assert_eq!(lane.commits.len(), 5, "the lane was really read");
        },
    );
    report.assert_intact();
}

#[test]
fn a_stop_mid_walk_is_too_slow_never_a_partial_window() {
    let f = Fixture::with_commit(&git_from_path());
    let (_, main_tip) = fork(&f);
    for n in 0..150 {
        f.git(&["commit", "-q", "--allow-empty", "-m", &format!("e{n}")]);
    }
    let head = rev(&f, "HEAD");
    let r = reader(&f);

    // The token is looked at every 64 commits: it says "go on" once and then "stop".
    let calls = Cell::new(0u32);
    let stop = || {
        calls.set(calls.get() + 1);
        calls.get() >= 2
    };
    let got = r
        .lane(&head, &main_tip, &limits(50, 10_000), &stop)
        .unwrap();
    assert!(
        matches!(got, Err(GitLaneGap::TooSlow)),
        "a stop in the middle of the walk is TooSlow, never a window cut short"
    );
    assert!(calls.get() >= 2, "stop is consulted again during the walk");

    // Control: without a stop the same lane is complete.
    let lane = r
        .lane(&head, &main_tip, &limits(50, 10_000), &never)
        .unwrap()
        .unwrap_or_else(|_| panic!("expected a lane"));
    assert_eq!(lane.commits.len(), 50);
    assert!(matches!(lane.total, Count::Exact(150)));
}
