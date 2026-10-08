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
    /// `new` is a merge and `old` is its first parent: the paths are against its first parent.
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

/// Deepest directory nesting the walk follows (H-01): a real tree is far shallower.
pub const MAX_TREE_DEPTH: usize = 1024;
/// Longest path kept whole (M-01); a longer one is cut and ends with [`PATH_CUT_MARK`].
pub const MAX_PATH_BYTES: usize = 1024;
/// What a cut path ends with: the path is incomplete, never silently shortened.
pub const PATH_CUT_MARK: &str = "\u{2026}";

fn cap_path(path: &[u8]) -> String {
    let text = path.to_str_lossy();
    if text.len() <= MAX_PATH_BYTES {
        return text.into_owned();
    }
    let mut end = MAX_PATH_BYTES - PATH_CUT_MARK.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{PATH_CUT_MARK}", &text[..end])
}

fn parse_id(id: &str) -> Result<gix::ObjectId, ReadError> {
    gix::ObjectId::from_hex(id.as_bytes())
        .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
}

impl RepoReader {
    /// The paths that differ between the trees of `old` (absent: the empty tree, for a first
    /// commit) and `new` (hex ids): at most `max`, sorted, and how many differ in all. If `new`
    /// is a merge and `old` is its first parent, the result says so (`first_parent`).
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
        // A merge was reached from its first parent: then `old..new` is the first-parent diff
        // and says so. From anywhere else (a reset or a checkout to a merge) it is plain `old..new`.
        let first_parent = parents.len() > 1 && old == Some(parents[0]);
        let base = old;
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

    /// The entries of `old` and `new` that differ, in Git's order (a directory sorts as `name/`).
    fn changed_items(
        &self,
        old: Option<gix::ObjectId>,
        new: Option<gix::ObjectId>,
        deadline: Instant,
    ) -> Result<Vec<Item>, ReadError> {
        if old == new {
            return Ok(Vec::new());
        }
        let old = self.entries(old, deadline)?;
        let new = self.entries(new, deadline)?;
        let mut keys: Vec<&BString> = old.keys().chain(new.keys()).collect();
        keys.sort();
        keys.dedup();
        Ok(keys
            .into_iter()
            .filter_map(|key| {
                let (before, after) = (old.get(key), new.get(key));
                (before != after).then(|| Item {
                    key: key.clone(),
                    before: before.map(|b| b.1),
                    after: after.map(|a| a.1),
                })
            })
            .collect())
    }

    /// Compares two trees without recursion: an explicit stack of one frame per directory level,
    /// so a hostile tree of any depth cannot exhaust the thread's stack (H-01). Past
    /// [`MAX_TREE_DEPTH`] levels it fails: never a partial total.
    fn diff_trees(
        &self,
        old: Option<gix::ObjectId>,
        new: Option<gix::ObjectId>,
        prefix: &mut Vec<u8>,
        walk: &mut Walk,
    ) -> Result<(), ReadError> {
        let root = self.changed_items(old, new, walk.deadline)?;
        let mut stack = vec![Frame {
            items: root.into_iter(),
            mark: prefix.len(),
        }];
        while let Some(frame) = stack.last_mut() {
            let mark = frame.mark;
            let Some(item) = frame.items.next() else {
                stack.pop();
                continue;
            };
            prefix.truncate(mark);
            prefix.extend_from_slice(&item.key);
            if item.key.last() == Some(&b'/') {
                if stack.len() >= MAX_TREE_DEPTH {
                    return Err(ReadError::Unavailable("changed paths: tree too deep".into()));
                }
                let items = self.changed_items(item.before, item.after, walk.deadline)?;
                stack.push(Frame {
                    items: items.into_iter(),
                    mark: prefix.len(),
                });
            } else {
                walk.leaf(prefix);
            }
        }
        Ok(())
    }
}

/// One entry that differs between two trees.
struct Item {
    key: BString,
    before: Option<gix::ObjectId>,
    after: Option<gix::ObjectId>,
}

/// A directory level being walked: what is left to visit and the prefix that is its own.
struct Frame {
    items: std::vec::IntoIter<Item>,
    mark: usize,
}

impl Walk {
    fn leaf(&mut self, path: &[u8]) {
        self.total += 1;
        if self.paths.len() < self.max {
            self.paths.push(cap_path(path));
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
    fn a_merge_reached_from_its_first_parent_says_so() {
        let f = fixture();
        let base = head(&f);
        f.git(&["checkout", "-q", "-b", "side"]);
        commit(&f, &[("side.rs", "1")], "side");
        f.git(&["checkout", "-q", "-"]);
        let own = commit(&f, &[("own.rs", "1")], "own");
        f.git(&["merge", "--no-ff", "-q", "-m", "merge", "side"]);
        let merge = head(&f);
        let got = reader(&f)
            .changed_paths(Some(&own), &merge, 20, soon())
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
    fn a_move_to_a_merge_from_elsewhere_compares_old_with_new() {
        let f = fixture();
        let base = head(&f);
        f.git(&["checkout", "-q", "-b", "side"]);
        commit(&f, &[("side.rs", "1")], "side");
        f.git(&["checkout", "-q", "-"]);
        commit(&f, &[("own.rs", "1")], "own");
        f.git(&["merge", "--no-ff", "-q", "-m", "merge", "side"]);
        let merge = head(&f);
        // A reset or checkout from the fork point: everything the merge brings, both sides.
        let got = reader(&f)
            .changed_paths(Some(&base), &merge, 20, soon())
            .unwrap();
        assert_eq!(got.paths, ["own.rs", "side.rs"]);
        assert!(!got.first_parent);
    }

    #[test]
    fn a_very_deep_tree_is_unavailable_and_does_not_crash() {
        use gix::objs::tree::{Entry, EntryKind};
        let f = fixture();
        let old = head(&f);
        let repo = gix::open(&f.repo).unwrap();
        let mut id: gix::ObjectId = repo.write_blob(b"x").unwrap().detach();
        let mut kind = EntryKind::Blob;
        for level in 0..6000 {
            let name = if level == 0 { "leaf" } else { "d" };
            let tree = gix::objs::Tree {
                entries: vec![Entry {
                    mode: kind.into(),
                    filename: name.into(),
                    oid: id,
                }],
            };
            id = repo.write_object(&tree).unwrap().detach();
            kind = EntryKind::Tree;
        }
        let commit = f.git(&["commit-tree", &id.to_string(), "-m", "deep"]);
        let r = reader(&f);
        assert!(matches!(
            r.changed_paths(Some(&old), commit.trim(), 20, soon()),
            Err(ReadError::Unavailable(_))
        ));
    }

    #[test]
    fn a_long_path_is_cut_with_a_mark_on_a_char_boundary() {
        let long = format!("{}.rs", "n".repeat(5_000));
        let got = cap_path(long.as_bytes());
        assert_eq!(got.len(), MAX_PATH_BYTES);
        assert!(got.ends_with(PATH_CUT_MARK));
        // Multi-byte characters are never split.
        let wide = "\u{e9}".repeat(2_000);
        let got = cap_path(wide.as_bytes());
        assert!(got.len() <= MAX_PATH_BYTES && got.ends_with(PATH_CUT_MARK));
        assert_eq!(cap_path(b"a/b.rs"), "a/b.rs");
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
