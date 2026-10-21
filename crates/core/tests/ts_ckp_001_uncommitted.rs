// TS-CKP-001 T001: the observer keeps every uncommitted path of a worktree and tells the
// predictor when a worktree is recomputed and when a repo changes tier. Temporary repos only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::MAX_WORKTREE_CHANGES;
use gitraptor_core::observe::{
    self, MAX_UNCOMMITTED_FOR_OVERLAP, UncommittedPaths, read_worktree, reconcile,
};
use gitraptor_core::watch::{FanoutHooks, Observer, ObserverHooks, Tier, WatchConfig};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

fn read_of(f: &Fixture) -> gitraptor_core::observe::WorktreeRead {
    let common = observe::locate(&f.repo).unwrap();
    read_worktree(&common, &observe::canonical(&f.repo), true, None)
}

mod full {
    use super::*;

    /// CKP001-UNC-FULL: modified, staged and untracked, each once.
    #[test]
    fn ckp001_unc_full_keeps_modified_staged_and_untracked_once() {
        let f = Fixture::with_commit(&git_from_path());
        f.write("a.txt", "alpha changed\n");
        f.write("staged.txt", "s\n");
        f.git(&["add", "staged.txt"]);
        // Staged and then modified again: still one path.
        f.write("staged.txt", "s2\n");
        f.write("untracked.txt", "u\n");
        let read = read_of(&f);
        let want: BTreeSet<String> = ["a.txt", "staged.txt", "untracked.txt"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(*read.uncommitted.paths, want);
        assert!(!read.uncommitted.partial);
    }

    /// CKP001-UNC-FULL: above the published cap the full set is still retained.
    #[test]
    fn ckp001_unc_full_keeps_more_paths_than_the_published_cap() {
        let f = Fixture::with_commit(&git_from_path());
        let n = MAX_WORKTREE_CHANGES + 50;
        for i in 0..n {
            f.write(&format!("many/f{i:04}.txt"), "x\n");
        }
        let read = read_of(&f);
        assert_eq!(read.uncommitted.paths.len(), n);
        assert!(!read.uncommitted.partial);
    }

    /// CKP001-UNC-FULL: a clean worktree retains nothing.
    #[test]
    fn ckp001_unc_full_is_empty_when_clean() {
        let f = Fixture::with_commit(&git_from_path());
        let read = read_of(&f);
        assert!(read.uncommitted.paths.is_empty());
        assert!(!read.uncommitted.partial);
    }
}

mod cap {
    use super::*;

    #[test]
    fn ckp001_unc_cap_limit_is_ten_thousand() {
        assert_eq!(MAX_UNCOMMITTED_FOR_OVERLAP, 10_000);
    }

    #[test]
    fn ckp001_unc_cap_keeps_the_first_in_byte_order_and_marks_partial() {
        let input = ["d", "b", "a", "c", "b", "B"].map(String::from);
        let got = UncommittedPaths::from_paths(input, 3);
        let want: BTreeSet<String> = ["B", "a", "b"].into_iter().map(String::from).collect();
        assert_eq!(*got.paths, want);
        assert!(got.partial);
    }

    #[test]
    fn ckp001_unc_cap_at_the_limit_is_not_partial_and_has_no_repeats() {
        let input = ["b", "a", "b", "a"].map(String::from);
        let got = UncommittedPaths::from_paths(input, 2);
        assert_eq!(got.paths.len(), 2);
        assert!(!got.partial);
    }

    #[test]
    fn ckp001_unc_cap_over_the_real_limit() {
        let input = (0..MAX_UNCOMMITTED_FOR_OVERLAP + 5).map(|i| format!("p{i:06}"));
        let got = UncommittedPaths::from_paths(input, MAX_UNCOMMITTED_FOR_OVERLAP);
        assert_eq!(got.paths.len(), MAX_UNCOMMITTED_FOR_OVERLAP);
        assert!(got.partial);
        assert!(got.paths.contains("p000000"));
        assert!(
            !got.paths
                .contains(&format!("p{:06}", MAX_UNCOMMITTED_FOR_OVERLAP + 4))
        );
    }
}

mod hooks {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Recomputed(String, PathBuf, BTreeSet<String>, bool),
        Tier(String, Tier),
    }

    struct Recorder(Mutex<Sender<Call>>);

    impl ObserverHooks for Recorder {
        fn worktree_touched(&self, _: &str, _: &Path) {}
        fn git_dir_touched(&self, _: &str, _: u64) {}
        fn worktree_recomputed(&self, repo_id: &str, root: &Path, paths: &UncommittedPaths) {
            let _ = self.0.lock().unwrap().send(Call::Recomputed(
                repo_id.to_owned(),
                root.to_owned(),
                (*paths.paths).clone(),
                paths.partial,
            ));
        }
        fn repo_tier_changed(&self, repo_id: &str, tier: Tier) {
            let _ = self
                .0
                .lock()
                .unwrap()
                .send(Call::Tier(repo_id.to_owned(), tier));
        }
    }

    #[test]
    fn ckp001_unc_hook_fanout_forwards_both_calls_to_every_hook() {
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        let fan = FanoutHooks(vec![
            Arc::new(Recorder(Mutex::new(tx1))),
            Arc::new(Recorder(Mutex::new(tx2))),
        ]);
        let paths = UncommittedPaths::from_paths(["x.txt".to_owned()], 10);
        fan.worktree_recomputed("r", Path::new("/w"), &paths);
        fan.repo_tier_changed("r", Tier::Dormant);
        for rx in [rx1, rx2] {
            let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                first,
                Call::Recomputed(
                    "r".into(),
                    PathBuf::from("/w"),
                    ["x.txt".to_owned()].into_iter().collect(),
                    false
                )
            );
            let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(second, Call::Tier("r".into(), Tier::Dormant));
        }
    }

    #[test]
    fn ckp001_unc_hook_observer_reports_the_full_set_when_a_worktree_is_recomputed() {
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        let wt = observe::canonical(&f.add_worktree("feat-login", "feat-login"));

        let (tx, rx) = channel();
        let observer = Observer::start_with_hooks(
            WatchConfig::default(),
            Arc::new(|_| {}),
            Some(Arc::new(Recorder(Mutex::new(tx)))),
        );
        let common = observe::locate(&f.repo).unwrap();
        let read = reconcile(&common, &observe::base_branch(None)).unwrap();
        observer.watch_repo("r", &common, &read);

        std::fs::write(wt.join("login.txt"), "user\npassword\n").unwrap();
        std::fs::write(wt.join("new.txt"), "n\n").unwrap();
        let want: BTreeSet<String> = ["login.txt", "new.txt"]
            .into_iter()
            .map(String::from)
            .collect();

        let start = Instant::now();
        loop {
            let left = Duration::from_secs(15).saturating_sub(start.elapsed());
            match rx.recv_timeout(left) {
                Ok(Call::Recomputed(id, root, paths, partial)) if root == wt && paths == want => {
                    assert_eq!(id, "r");
                    assert!(!partial);
                    break;
                }
                Ok(_) => {}
                Err(_) => panic!("worktree_recomputed with the full set never arrived"),
            }
        }
    }
}
