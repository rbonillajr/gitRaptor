//! Helpers of the snapshot store tests (TS-TMC-001). Every test runs on a testkit fixture: a
//! temporary repo, home and profile, never this repo or the real profile (NFR-01).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog::{Oplog, SnapshotLevel};
use gitraptor_core::timemachine::store::{
    CaptureOutcome, CaptureRequest, ChangeHint, SnapshotStore, WorktreeScope,
};
use gitraptor_testkit::Fixture;

pub const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

pub fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

pub struct Env {
    pub f: Fixture,
    pub dirs: ProfileDirs,
    pub store: SnapshotStore,
    pub oplog: Mutex<Oplog>,
}

impl Env {
    pub fn new(f: Fixture) -> Self {
        let dirs = ProfileDirs::under_root(&f.profile);
        let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
        let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
        Self {
            f,
            dirs,
            store,
            oplog: Mutex::new(oplog),
        }
    }

    pub fn busy() -> Self {
        Self::new(Fixture::busy(&git()))
    }

    pub fn tm_root(&self) -> PathBuf {
        self.dirs.data.join("tm")
    }

    pub fn store_path(&self) -> PathBuf {
        self.tm_root().join(REPO_ID).join("store.git")
    }

    pub fn request(&self, level: SnapshotLevel, hint: Option<ChangeHint>) -> CaptureRequest {
        CaptureRequest {
            level,
            repo: self.f.repo.clone(),
            worktrees: vec![WorktreeScope {
                key: "main".into(),
                path: self.f.repo.clone(),
                hint,
            }],
            engine_mark: None,
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        }
    }

    pub fn capture(&self, level: SnapshotLevel, hint: Option<ChangeHint>) -> CaptureOutcome {
        self.store
            .capture(&self.oplog, &self.request(level, hint))
            .unwrap()
    }

    /// A capture whose read state reaches engine mark `mark`.
    pub fn capture_at(
        &self,
        level: SnapshotLevel,
        mark: i64,
        hint: Option<ChangeHint>,
    ) -> CaptureOutcome {
        let mut req = self.request(level, hint);
        req.engine_mark = Some(mark);
        self.store.capture(&self.oplog, &req).unwrap()
    }

    pub fn prior(&self) -> CaptureOutcome {
        self.capture(SnapshotLevel::GuaranteedPrior, None)
    }

    /// `path → bytes` of the files of worktree `key` in a snapshot.
    pub fn files(&self, id: &str, key: &str) -> Vec<(String, Vec<u8>)> {
        self.store
            .files(id, key)
            .unwrap()
            .into_iter()
            .filter(|(_, k, _)| *k != gitraptor_git::tm_write::store::TreeEntryKind::Gitlink)
            .map(|(p, _, oid)| (p, self.store.read_blob(oid).unwrap()))
            .collect()
    }

    pub fn file(&self, id: &str, path: &str) -> Option<Vec<u8>> {
        self.files(id, "main")
            .into_iter()
            .find(|(p, _)| p == path)
            .map(|(_, b)| b)
    }

    pub fn paths(&self, id: &str) -> Vec<String> {
        self.files(id, "main").into_iter().map(|(p, _)| p).collect()
    }

    /// Git in the store, for the tests' own checks.
    pub fn store_git(&self, args: &[&str]) -> String {
        let store = self.store_path();
        let mut full = vec!["--git-dir", store.to_str().unwrap()];
        full.extend_from_slice(args);
        self.f.git_in(&self.f.root, &full)
    }
}

/// A hint from the "engine": the paths changed since `since`.
pub fn hint(since: i64, mark: i64, paths: &[&str]) -> ChangeHint {
    ChangeHint {
        since,
        mark,
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        continuous: true,
    }
}

/// Writes `content` at `rela` under `root`, creating folders.
pub fn write(root: &Path, rela: &str, content: &[u8]) {
    let p = root.join(rela);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
