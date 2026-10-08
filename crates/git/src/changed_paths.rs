//! The paths that differ between two commits (DS-US-TMC-006 T002, ADR-GRP-009).
//!
//! Only trees are read: no blob is loaded, no message or content is ever returned, and nothing
//! runs a filter, a driver or the Git CLI. Trees with the same id on both sides are skipped
//! whole. There is no rename detection: a rename is two paths.

use std::collections::BTreeMap;
use std::time::Instant;

use gix::bstr::{BString, ByteSlice};

use crate::{ReadError, RepoReader};

/// The paths of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPaths {
    /// At most `max`, in byte order.
    pub paths: Vec<String>,
    /// The real number of paths that differ.
    pub total: u32,
    /// `new` is a merge: it was compared with its first parent, whatever `old` said.
    pub first_parent: bool,
}

/// One entry of a tree, keyed so a directory sorts as Git sorts it (`name/`).
type Entries = BTreeMap<BString, (gix::objs::tree::EntryMode, gix::ObjectId)>;

struct Walk {
    max: usize,
    deadline: Instant,
    paths: Vec<String>,
    total: u64,
}

fn parse_id(id: &str) -> Result<gix::ObjectId, ReadError> {
    gix::ObjectId::from_hex(id.as_bytes())
        .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
}

impl RepoReader {
    /// The paths that differ between the trees of `old` (absent: the empty tree, for a first
    /// commit) and `new` (hex ids): at most `max`, sorted, and how many differ in all. If `new`
    /// is a merge it is compared with its first parent instead of `old`.
    ///
    /// If `deadline` passes or an object is missing it fails: never a partial total, never a
    /// made-up zero.
    pub fn changed_paths(
        &self,
        old: Option<&str>,
        new: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<ChangedPaths, ReadError> {
        let old = old.map(parse_id).transpose()?;
        let new = parse_id(new)?;
        let new_commit = self.commit(new)?;
        let parents: Vec<gix::ObjectId> = new_commit.parent_ids().map(|p| p.detach()).collect();
        let first_parent = parents.len() > 1;
        let base = if first_parent { Some(parents[0]) } else { old };
        let old_tree = base
            .map(|id| self.commit(id).and_then(|c| self.commit_tree(&c)))
            .transpose()?;
        let new_tree = self.commit_tree(&new_commit)?;
        let mut walk = Walk {
            max,
            deadline,
            paths: Vec::new(),
            total: 0,
        };
        self.diff_trees(old_tree, Some(new_tree), &mut Vec::new(), &mut walk)?;
        Ok(ChangedPaths {
            paths: walk.paths,
            total: u32::try_from(walk.total).unwrap_or(u32::MAX),
            first_parent,
        })
    }

    fn commit(&self, id: gix::ObjectId) -> Result<gix::Commit<'_>, ReadError> {
        self.repo
            .find_object(id)
            .map_err(|e| ReadError::Unavailable(format!("commit: {e}")))?
            .try_into_commit()
            .map_err(|_| ReadError::InvalidInput("not a commit".into()))
    }

    fn commit_tree(&self, commit: &gix::Commit<'_>) -> Result<gix::ObjectId, ReadError> {
        commit
            .tree_id()
            .map(|id| id.detach())
            .map_err(|e| ReadError::Unavailable(format!("tree: {e}")))
    }

    fn entries(
        &self,
        tree: Option<gix::ObjectId>,
        deadline: Instant,
    ) -> Result<Entries, ReadError> {
        let Some(id) = tree else {
            return Ok(Entries::new());
        };
        if Instant::now() >= deadline {
            return Err(ReadError::TemporarilyUnavailable(
                "changed paths: deadline".into(),
            ));
        }
        let tree = self
            .repo
            .find_tree(id)
            .map_err(|e| ReadError::Unavailable(format!("tree: {e}")))?;
        let decoded = tree
            .decode()
            .map_err(|e| ReadError::Unavailable(format!("tree: {e}")))?;
        Ok(decoded
            .entries
            .iter()
            .map(|e| {
                let mut key = BString::from(e.filename.as_bytes());
                if e.mode.is_tree() {
                    key.push(b'/');
                }
                (key, (e.mode, e.oid.to_owned()))
            })
            .collect())
    }

    /// Compares two trees; `prefix` is the directory they are, with its trailing `/`.
    fn diff_trees(
        &self,
        old: Option<gix::ObjectId>,
        new: Option<gix::ObjectId>,
        prefix: &mut Vec<u8>,
        walk: &mut Walk,
    ) -> Result<(), ReadError> {
        if old == new {
            return Ok(());
        }
        let old = self.entries(old, walk.deadline)?;
        let new = self.entries(new, walk.deadline)?;
        let mut keys: Vec<&BString> = old.keys().chain(new.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let (before, after) = (old.get(key), new.get(key));
            if before == after {
                continue;
            }
            let is_tree = key.last() == Some(&b'/');
            let mark = prefix.len();
            prefix.extend_from_slice(key);
            if is_tree {
                self.diff_trees(before.map(|b| b.1), after.map(|a| a.1), prefix, walk)?;
            } else {
                walk.leaf(prefix);
            }
            prefix.truncate(mark);
        }
        Ok(())
    }
}

impl Walk {
    fn leaf(&mut self, path: &[u8]) {
        self.total += 1;
        if self.paths.len() < self.max {
            self.paths.push(path.to_str_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gitraptor_testkit::Fixture;

    use super::*;
    use crate::ReaderOptions;

    fn fixture() -> Fixture {
        Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path())
    }

    fn head(f: &Fixture) -> String {
        f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn commit(f: &Fixture, files: &[(&str, &str)], message: &str) -> String {
        for (path, content) in files {
            f.write(path, content);
        }
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", message]);
        head(f)
    }

    fn reader(f: &Fixture) -> RepoReader {
        RepoReader::open(&f.repo, &ReaderOptions::default()).unwrap()
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(20)
    }

    #[test]
    fn changed_paths_between_two_commits() {
        let f = fixture();
        let a = commit(
            &f,
            &[("a.rs", "1"), ("dir/b.rs", "1"), ("dir/c.rs", "1")],
            "a",
        );
        let b = commit(
            &f,
            &[
                ("a.rs", "2"),
                ("dir/b.rs", "1"),
                ("dir/d.rs", "1"),
                ("a.b", "1"),
            ],
            "SECRET-MESSAGE",
        );
        let got = reader(&f).changed_paths(Some(&a), &b, 20, soon()).unwrap();
        // Byte order of the full paths: `a.b` before `a.rs`, then the directory.
        assert_eq!(got.paths, ["a.b", "a.rs", "dir/d.rs"]);
        assert_eq!(got.total, 3);
        assert!(!got.first_parent);
        f.git(&["rm", "-q", "dir/c.rs"]);
        f.git(&["commit", "-q", "-m", "rm"]);
        let c = head(&f);
        let got = reader(&f).changed_paths(Some(&b), &c, 20, soon()).unwrap();
        assert_eq!(got.paths, ["dir/c.rs"]);
    }

    #[test]
    fn a_root_commit_is_compared_with_the_empty_tree() {
        let f = fixture();
        let first = f
            .git(&["rev-list", "--max-parents=0", "HEAD"])
            .trim()
            .to_owned();
        let all = f.git(&["ls-tree", "-r", "--name-only", &first]);
        let got = reader(&f).changed_paths(None, &first, 20, soon()).unwrap();
        assert_eq!(got.total as usize, all.lines().count());
        assert!(got.total > 0);
        assert_eq!(got.paths.len(), got.total as usize);
    }

    #[test]
    fn the_total_is_real_beyond_max() {
        let f = fixture();
        let a = head(&f);
        let files: Vec<(String, String)> = (0..25)
            .map(|i| (format!("f{i:02}.txt"), "x".into()))
            .collect();
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let b = commit(&f, &refs, "many");
        let got = reader(&f).changed_paths(Some(&a), &b, 20, soon()).unwrap();
        assert_eq!(got.total, 25);
        assert_eq!(got.paths.len(), 20);
        assert_eq!(got.paths[19], "f19.txt");
        let none = reader(&f).changed_paths(Some(&a), &b, 0, soon()).unwrap();
        assert!(none.paths.is_empty());
        assert_eq!(none.total, 25);
    }

    #[test]
    fn a_merge_is_compared_with_its_first_parent() {
        let f = fixture();
        let base = head(&f);
        f.git(&["checkout", "-q", "-b", "side"]);
        commit(&f, &[("side.rs", "1")], "side");
        f.git(&["checkout", "-q", "-"]);
        let own = commit(&f, &[("own.rs", "1")], "own");
        f.git(&["merge", "--no-ff", "-q", "-m", "merge", "side"]);
        let merge = head(&f);
        // `old` is the fork point: a merge ignores it for its first parent.
        let got = reader(&f)
            .changed_paths(Some(&base), &merge, 20, soon())
            .unwrap();
        assert_eq!(got.paths, ["side.rs"]);
        assert!(got.first_parent);
        let got = reader(&f)
            .changed_paths(Some(&base), &own, 20, soon())
            .unwrap();
        assert_eq!(got.paths, ["own.rs"]);
        assert!(!got.first_parent);
    }

    #[test]
    fn a_type_change_lists_both_sides() {
        let f = fixture();
        let a = commit(&f, &[("x", "file")], "file");
        f.git(&["rm", "-q", "x"]);
        let b = commit(&f, &[("x/y.rs", "1")], "dir");
        let got = reader(&f).changed_paths(Some(&a), &b, 20, soon()).unwrap();
        assert_eq!(got.paths, ["x", "x/y.rs"]);
    }

    #[test]
    fn a_missing_object_is_an_error_not_zero() {
        let f = fixture();
        let head = head(&f);
        let ghost = "1".repeat(40);
        let r = reader(&f);
        assert!(matches!(
            r.changed_paths(Some(&ghost), &head, 20, soon()),
            Err(ReadError::Unavailable(_))
        ));
        assert!(matches!(
            r.changed_paths(None, &ghost, 20, soon()),
            Err(ReadError::Unavailable(_))
        ));
        assert!(matches!(
            r.changed_paths(None, "not-an-id", 20, soon()),
            Err(ReadError::InvalidInput(_))
        ));
    }

    #[test]
    fn an_expired_deadline_is_an_error() {
        let f = fixture();
        let a = head(&f);
        let b = commit(&f, &[("n.rs", "1")], "n");
        let past = Instant::now();
        assert!(matches!(
            reader(&f).changed_paths(Some(&a), &b, 20, past),
            Err(ReadError::TemporarilyUnavailable(_))
        ));
    }
}
