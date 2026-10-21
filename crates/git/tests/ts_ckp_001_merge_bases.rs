// TS-CKP-001 T002: `RepoReader::merge_bases` returns every merge-base of two commits (hex ids).

use gitraptor_git::{ReaderOptions, RepoReader};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

fn id(f: &Fixture, rev: &str) -> String {
    f.git(&["rev-parse", rev]).trim().to_owned()
}

fn commit_file(f: &Fixture, file: &str, content: &str) {
    f.write(file, content);
    f.git(&["add", file]);
    f.git(&["commit", "-q", "-m", file]);
}

fn merge_bases(f: &Fixture, a: &str, b: &str) -> Vec<String> {
    let reader = RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap();
    let mut got = reader.merge_bases(a, b).unwrap();
    got.sort();
    got
}

mod merge_bases {
    use super::*;

    #[test]
    fn ckp001_merge_bases_simple_history_has_one() {
        let f = Fixture::with_commit(&git_from_path());
        let fork = id(&f, "HEAD");
        f.git(&["checkout", "-q", "-b", "ours"]);
        commit_file(&f, "o.txt", "o\n");
        f.git(&["checkout", "-q", "-b", "theirs", "main"]);
        commit_file(&f, "t.txt", "t\n");
        let (ours, theirs) = (id(&f, "ours"), id(&f, "theirs"));
        assert_eq!(merge_bases(&f, &ours, &theirs), vec![fork.clone()]);
        assert_eq!(merge_bases(&f, &theirs, &ours), vec![fork]);
    }

    #[test]
    fn ckp001_merge_bases_crossed_history_has_two() {
        let f = Fixture::with_commit(&git_from_path());
        f.git(&["checkout", "-q", "-b", "a"]);
        commit_file(&f, "fa.txt", "a\n");
        let a1 = id(&f, "HEAD");
        f.git(&["checkout", "-q", "-b", "b", "main"]);
        commit_file(&f, "fb.txt", "b\n");
        let b1 = id(&f, "HEAD");
        // Each branch merges the other's first commit: a criss-cross.
        f.git(&["checkout", "-q", "a"]);
        f.git(&["merge", "-q", "--no-edit", &b1]);
        f.git(&["checkout", "-q", "b"]);
        f.git(&["merge", "-q", "--no-edit", &a1]);
        let (ma, mb) = (id(&f, "a"), id(&f, "b"));
        let mut want = vec![a1, b1];
        want.sort();
        assert_eq!(merge_bases(&f, &ma, &mb), want);
    }

    #[test]
    fn ckp001_merge_bases_without_common_history_is_empty() {
        let f = Fixture::with_commit(&git_from_path());
        f.git(&["checkout", "-q", "--orphan", "other"]);
        f.git(&["rm", "-rfq", "."]);
        commit_file(&f, "z.txt", "z\n");
        let (main, other) = (id(&f, "main"), id(&f, "other"));
        assert!(merge_bases(&f, &main, &other).is_empty());
    }

    #[test]
    fn ckp001_merge_bases_of_a_commit_with_itself_is_itself() {
        let f = Fixture::with_commit(&git_from_path());
        let head = id(&f, "HEAD");
        assert_eq!(merge_bases(&f, &head, &head), vec![head]);
    }
}
