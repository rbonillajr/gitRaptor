//! Reads of committed objects: a file at a path of a commit, a blob by id and the refs that
//! locate the copy of the main branch (TS-GRD-001, ADR-GRD-004).
//!
//! Every read here ignores replacement objects (`refs/replace/*`) because the reader disables
//! them when it opens the repository, and walks trees from the commit, so grafts and the
//! commit-graph, which only change parents, never change what is read (SEC-GRD-17, H-05).
//! A blob is never loaded before its size is known from its header (L-03).

use gix::bstr::ByteSlice;

use crate::{ReadError, RefName, RepoReader};

/// What a committed path holds when it is not a regular file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotRegular {
    /// A symbolic link entry (mode `120000`).
    Symlink,
    /// A submodule entry (mode `160000`).
    Submodule,
    /// A directory where a file was expected, or a file where a directory was expected.
    WrongKind,
}

/// The committed content of one path. Never carries a partial read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommittedFile {
    /// The commit has no entry at that path.
    Absent,
    /// The entry exists but is not a regular blob; nothing was read.
    NotRegular(NotRegular),
    /// The blob is larger than the limit; it was not loaded.
    TooLarge { size: u64 },
    /// A regular blob, with its object id (lowercase hex).
    Blob { id: String, bytes: Vec<u8> },
}

/// A blob read by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobRead {
    /// The object database has no object with that id.
    Missing,
    /// The object exists but is not a blob.
    NotABlob,
    /// The blob is larger than the limit; it was not loaded.
    TooLarge {
        size: u64,
    },
    Blob {
        bytes: Vec<u8>,
    },
}

fn parse_id(id: &str) -> Result<gix::ObjectId, ReadError> {
    gix::ObjectId::from_hex(id.as_bytes())
        .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
}

impl RepoReader {
    /// The committed file at `path` (components, no separators inside) of `commit` (hex id).
    pub fn committed_file(
        &self,
        commit: &str,
        path: &[&str],
        max_size: u64,
    ) -> Result<CommittedFile, ReadError> {
        if path.is_empty()
            || path
                .iter()
                .any(|c| c.is_empty() || *c == "." || *c == ".." || c.contains(['/', '\\']))
        {
            return Err(ReadError::InvalidInput("invalid committed path".into()));
        }
        let repo = &self.repo;
        let commit = repo
            .find_object(parse_id(commit)?)
            .map_err(|e| ReadError::Unavailable(format!("commit: {e}")))?
            .try_into_commit()
            .map_err(|_| ReadError::InvalidInput("not a commit".into()))?;
        let mut tree = commit
            .tree()
            .map_err(|e| ReadError::Unavailable(format!("tree: {e}")))?;
        let (last, dirs) = path.split_last().expect("non-empty path");
        for dir in dirs {
            let Some(entry) = tree.find_entry(dir.as_bytes()) else {
                return Ok(CommittedFile::Absent);
            };
            if !entry.mode().is_tree() {
                return Ok(CommittedFile::NotRegular(not_regular(entry.mode())));
            }
            let id = entry.object_id();
            tree = repo
                .find_tree(id)
                .map_err(|e| ReadError::Unavailable(format!("tree: {e}")))?;
        }
        let Some(entry) = tree.find_entry(last.as_bytes()) else {
            return Ok(CommittedFile::Absent);
        };
        let mode = entry.mode();
        if !mode.is_blob() {
            return Ok(CommittedFile::NotRegular(not_regular(mode)));
        }
        let id = entry.object_id();
        Ok(match self.read_blob(id, max_size)? {
            BlobRead::Blob { bytes } => CommittedFile::Blob {
                id: id.to_string(),
                bytes,
            },
            BlobRead::TooLarge { size } => CommittedFile::TooLarge { size },
            BlobRead::Missing | BlobRead::NotABlob => {
                return Err(ReadError::Unavailable("tree entry without its blob".into()));
            }
        })
    }

    /// The blob with id `id` (hex), if the object database has it.
    pub fn blob_by_id(&self, id: &str, max_size: u64) -> Result<BlobRead, ReadError> {
        self.read_blob(parse_id(id)?, max_size)
    }

    fn read_blob(&self, id: gix::ObjectId, max_size: u64) -> Result<BlobRead, ReadError> {
        let Some(header) = self
            .repo
            .try_find_header(id)
            .map_err(|e| ReadError::Unavailable(format!("object header: {e}")))?
        else {
            return Ok(BlobRead::Missing);
        };
        if header.kind() != gix::object::Kind::Blob {
            return Ok(BlobRead::NotABlob);
        }
        if header.size() > max_size {
            return Ok(BlobRead::TooLarge {
                size: header.size(),
            });
        }
        let object = self
            .repo
            .find_object(id)
            .map_err(|e| ReadError::Unavailable(format!("blob: {e}")))?;
        Ok(BlobRead::Blob {
            bytes: object.detach().data,
        })
    }

    /// Names of the configured remotes that are valid ref name components, sorted.
    pub fn remote_names(&self) -> Vec<RefName> {
        let mut names: Vec<RefName> = self
            .repo
            .remote_names()
            .into_iter()
            .filter_map(|n| RefName::new(&n.to_str_lossy()).ok())
            .filter(|n| !n.as_str().contains('/'))
            .collect();
        names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        names
    }

    /// Full name of the ref a symbolic ref points to, such as `refs/remotes/origin/trunk` for
    /// `refs/remotes/origin/HEAD`. `None` if `name` does not exist or is not symbolic.
    pub fn symbolic_target(&self, name: &RefName) -> Result<Option<String>, ReadError> {
        let Some(r) = self
            .repo
            .try_find_reference(name.as_str())
            .map_err(|e| ReadError::Unavailable(format!("refs: {e}")))?
        else {
            return Ok(None);
        };
        Ok(match r.target() {
            gix::refs::TargetRef::Symbolic(target) => {
                Some(target.as_bstr().to_str_lossy().into_owned())
            }
            gix::refs::TargetRef::Object(_) => None,
        })
    }
}

fn not_regular(mode: gix::object::tree::EntryMode) -> NotRegular {
    if mode.is_link() {
        NotRegular::Symlink
    } else if mode.is_commit() {
        NotRegular::Submodule
    } else {
        NotRegular::WrongKind
    }
}
