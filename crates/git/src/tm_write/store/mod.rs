//! The snapshot store writer, with gitoxide in process (ADR-TMC-001 § 3, ADR-TMC-002 § 1,
//! ADR-TMC-006 § 5, escalón 3).
//!
//! This submodule is the **only** place of the crate where gitoxide writes (checked by
//! `tests/static_check.rs`). It can only open a validated store: `<tm>/<repo-id>/store.git`, a
//! real bare folder of the current user with private permissions, inside the `tm/` folder of the
//! profile and with the store mark in its configuration. The access type to a user repository
//! ([`crate::RepoReader`]) offers no write.
//!
//! gitoxide is opened isolated: no system, global or environment configuration, never a `git`
//! process. Durability follows the batch scheme of ADR-TMC-001 § 4: a plain `fsync` of every
//! loose object, one full barrier before a ref is created, then the ref and its folder.

mod anchor;
mod durable;
mod seed;
mod verify;

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use gix::bstr::{BStr, ByteSlice};

use crate::{EntryKind, Oid};

pub use anchor::CopyReport;
pub use seed::{SeedLimits, SeedReport, SkippedPack};
pub use verify::VerifyReport;

/// Folder of the store inside `<tm>/<repo-id>/`.
pub const STORE_DIR: &str = "store.git";
/// Empty hooks folder the store's `core.hooksPath` points to.
pub const NOHOOKS_DIR: &str = "nohooks";
/// Prefix of the snapshot refs (ADR-TMC-001 § 1).
pub const SNAPSHOT_REF_PREFIX: &str = "refs/tm/snap/";
/// Configuration key that marks a folder as a GitRaptor store.
#[cfg(unix)]
const STORE_MARK: &str = "gitraptor.store";

/// Why the store could not do what was asked. Nothing is ever repaired in place.
#[derive(Debug)]
pub enum StoreError {
    /// Rejected before touching the file system.
    InvalidInput(String),
    /// The folder is not a store GitRaptor can trust (owner, permissions, symlink, outside
    /// `tm/`, no store mark). The caller sets it aside and starts a new one (ADR-TMC-001 § 4).
    Untrusted(String),
    /// An object is missing or does not hash to its id (SEC-TMC-09).
    Corrupt(String),
    /// A write gave way to a guaranteed prior snapshot (ADR-TMC-004 § 2).
    Yielded,
    /// Not available on this OS yet (Pendiente: etapa de validación multiplataforma).
    Unsupported(&'static str),
    Io(io::Error),
    Git(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::Untrusted(m) => write!(f, "store not trusted: {m}"),
            Self::Corrupt(m) => write!(f, "store corrupt: {m}"),
            Self::Yielded => write!(f, "gave way to a guaranteed prior snapshot"),
            Self::Unsupported(m) => write!(f, "not supported on this OS: {m}"),
            Self::Io(e) => write!(f, "store i/o: {e}"),
            Self::Git(m) => write!(f, "store: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

fn git_err(what: &str) -> impl Fn(gix::Error) -> StoreError + '_ {
    move |e| StoreError::Git(format!("{what}: {e}"))
}

fn plumbing_err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> StoreError + '_ {
    move |e| StoreError::Git(format!("{what}: {e}"))
}

/// A snapshot id is the oplog's lowercase UUID: it becomes a ref name and a path.
pub fn is_snapshot_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn check_repo_id(repo_id: &str) -> Result<()> {
    if is_snapshot_id(repo_id) {
        Ok(())
    } else {
        Err(StoreError::InvalidInput("not a repo key".into()))
    }
}

/// The store of one repository.
pub struct StoreRepo {
    sync: gix::ThreadSafeRepository,
    path: PathBuf,
}

impl std::fmt::Debug for StoreRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreRepo")
            .field("path", &self.path)
            .finish()
    }
}

impl StoreRepo {
    /// Path of the store of `repo_id` under `tm_root` (`<data>/tm`).
    pub fn location(tm_root: &Path, repo_id: &str) -> Result<PathBuf> {
        check_repo_id(repo_id)?;
        Ok(tm_root.join(repo_id).join(STORE_DIR))
    }

    /// Creates the store of `repo_id`. `<tm_root>/<repo_id>/` must exist and be private; the
    /// store is built in a temporary folder and renamed into place, so a half-made store never
    /// appears.
    pub fn create(tm_root: &Path, repo_id: &str) -> Result<Self> {
        #[cfg(unix)]
        {
            let path = Self::location(tm_root, repo_id)?;
            let repo_dir = path.parent().expect("store has a parent").to_owned();
            durable::check_private_dir(&repo_dir)?;
            if path.symlink_metadata().is_ok() {
                return Err(StoreError::InvalidInput("store already exists".into()));
            }
            let nohooks = repo_dir.join(NOHOOKS_DIR);
            if nohooks.symlink_metadata().is_err() {
                durable::create_private_dir(&nohooks)?;
            }
            durable::check_private_dir(&nohooks)?;
            let tmp = repo_dir.join(format!("{STORE_DIR}.tmp-{}", durable::nanos()));
            layout(&tmp, &nohooks)?;
            std::fs::rename(&tmp, &path)?;
            durable::fsync_dir(&repo_dir)?;
            Self::open(tm_root, repo_id)
        }
        #[cfg(not(unix))]
        {
            let _ = (tm_root, repo_id);
            Err(StoreError::Unsupported("snapshot store"))
        }
    }

    /// Opens the store of `repo_id` after checking it can be trusted.
    pub fn open(tm_root: &Path, repo_id: &str) -> Result<Self> {
        #[cfg(unix)]
        {
            let path = Self::location(tm_root, repo_id)?;
            let root = tm_root
                .canonicalize()
                .map_err(|e| StoreError::Untrusted(format!("tm folder: {e}")))?;
            durable::check_private_dir(&root)?;
            durable::check_private_dir(path.parent().expect("store has a parent"))?;
            durable::check_private_dir(&path)?;
            let real = path.canonicalize()?;
            if !real.starts_with(&root) {
                return Err(StoreError::Untrusted("store outside the tm folder".into()));
            }
            let options = gix::open::Options::isolated()
                .strict_config(true)
                .bail_if_untrusted(true);
            let sync = gix::ThreadSafeRepository::open_opts(&real, options)
                .map_err(|e| StoreError::Untrusted(format!("open: {e}")))?;
            let repo = sync.to_thread_local();
            if !repo.is_bare() {
                return Err(StoreError::Untrusted("store is not bare".into()));
            }
            let marked = repo
                .config_snapshot()
                .string(STORE_MARK)
                .is_some_and(|v| v.as_bstr() == "1");
            if !marked {
                return Err(StoreError::Untrusted("no store mark".into()));
            }
            Ok(Self { sync, path: real })
        }
        #[cfg(not(unix))]
        {
            let _ = (tm_root, repo_id);
            Err(StoreError::Unsupported("snapshot store"))
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A handle for the current thread. Handles share the object database and are cheap.
    pub fn handle(&self) -> StoreHandle {
        let mut repo = self.sync.to_thread_local();
        repo.objects.ignore_replacements = true;
        StoreHandle {
            repo,
            path: self.path.clone(),
        }
    }

    /// Bytes the store takes on disk (diagnostics, ADR-TMC-001 § 4).
    pub fn size_bytes(&self) -> io::Result<u64> {
        fn walk(dir: &Path) -> io::Result<u64> {
            let mut total = 0;
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let meta = entry.path().symlink_metadata()?;
                total += if meta.is_dir() {
                    walk(&entry.path())?
                } else {
                    meta.len()
                };
            }
            Ok(total)
        }
        walk(&self.path)
    }
}

/// Writes the layout of an empty bare store into `dir`, with the configuration of
/// ADR-TMC-001 § 4. Folders 0700, files 0600, everything synced.
#[cfg(unix)]
fn layout(dir: &Path, nohooks: &Path) -> Result<()> {
    durable::create_private_dir(dir)?;
    for sub in [
        "objects",
        "objects/info",
        "objects/pack",
        "refs",
        "refs/heads",
        "refs/tags",
        "info",
    ] {
        durable::create_private_dir(&dir.join(sub))?;
    }
    let hooks = nohooks
        .to_str()
        .ok_or_else(|| StoreError::InvalidInput("profile path is not UTF-8".into()))?;
    let hooks = hooks.replace('\\', "\\\\").replace('"', "\\\"");
    let config = format!(
        "[core]\n\
         \trepositoryformatversion = 0\n\
         \tbare = true\n\
         \tfilemode = true\n\
         \tlogAllRefUpdates = false\n\
         \thooksPath = \"{hooks}\"\n\
         \tcompression = 1\n\
         \tlooseCompression = 1\n\
         \tbigFileThreshold = 128k\n\
         \tfsync = committed\n\
         \tfsyncMethod = batch\n\
         [gc]\n\
         \tauto = 0\n\
         [maintenance]\n\
         \tauto = false\n\
         [pack]\n\
         \tcompression = 1\n\
         [receive]\n\
         \tdenyCurrentBranch = refuse\n\
         [gitraptor]\n\
         \tstore = 1\n"
    );
    durable::write_private_file(&dir.join("config"), config.as_bytes())?;
    durable::write_private_file(&dir.join("HEAD"), b"ref: refs/heads/main\n")?;
    for sub in [
        "objects/info",
        "objects/pack",
        "objects",
        "refs/heads",
        "refs/tags",
        "refs",
        "info",
    ] {
        durable::fsync_dir(&dir.join(sub))?;
    }
    durable::fsync_dir(dir)?;
    Ok(())
}

/// Access to the store from one thread. Every write of gitoxide goes through here.
pub struct StoreHandle {
    repo: gix::Repository,
    path: PathBuf,
}

impl StoreHandle {
    pub fn has(&self, id: Oid) -> bool {
        self.repo.has_object(id.0)
    }

    fn object_path(&self, id: Oid) -> PathBuf {
        let hex = id.0.to_string();
        self.path.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    /// Plain `fsync` of a loose object, if it is loose (a packed one is already durable).
    fn sync_object(&self, id: Oid) -> Result<()> {
        match durable::fsync_file(&self.object_path(id)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => Ok(other?),
        }
    }

    /// Plain `fsync` of many loose objects, on up to 8 threads.
    fn sync_objects(&self, ids: &[gix::ObjectId]) -> Result<()> {
        if ids.len() < 8 {
            for id in ids {
                self.sync_object(Oid(*id))?;
            }
            return Ok(());
        }
        let paths: Vec<PathBuf> = ids.iter().map(|id| self.object_path(Oid(*id))).collect();
        let chunk = paths.len().div_ceil(8);
        std::thread::scope(|s| {
            let jobs: Vec<_> = paths
                .chunks(chunk)
                .map(|part| {
                    s.spawn(move || -> io::Result<()> {
                        for p in part {
                            match durable::fsync_file(p) {
                                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                                other => other?,
                            }
                        }
                        Ok(())
                    })
                })
                .collect();
            for j in jobs {
                j.join()
                    .map_err(|_| StoreError::Git("fsync thread panicked".into()))??;
            }
            Ok(())
        })
    }

    /// Writes a blob from memory. Returns its id and whether the store lacked it.
    pub fn write_blob(&self, bytes: &[u8]) -> Result<(Oid, bool)> {
        use gix::objs::Write as _;
        let kind = gix::object::Kind::Blob;
        let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, kind, bytes)
            .map_err(plumbing_err("blob"))?;
        if self.repo.has_object(id) {
            return Ok((Oid(id), false));
        }
        self.repo
            .objects
            .write_buf_with_known_id(kind, bytes, id)
            .map_err(plumbing_err("blob"))?;
        self.sync_object(Oid(id))?;
        Ok((Oid(id), true))
    }

    /// Streams a blob of `size` bytes from `from`, hashing and compressing in one pass. `yield_now`
    /// is polled every 256 KiB: when it says so, the write is abandoned before the object is
    /// published (ADR-TMC-004 § 2). Fewer or more bytes than `size` fail the write: the file
    /// changed while it was read.
    pub fn write_blob_stream(
        &self,
        from: &mut dyn Read,
        size: u64,
        yield_now: &dyn Fn() -> bool,
    ) -> Result<Oid> {
        use gix::objs::Write as _;
        let mut reader = Checked {
            inner: from,
            left: size,
            since_poll: 0,
            yield_now,
            yielded: false,
        };
        let res = self
            .repo
            .objects
            .write_stream(gix::object::Kind::Blob, size, &mut reader);
        if reader.yielded {
            return Err(StoreError::Yielded);
        }
        let id = Oid(res.map_err(plumbing_err("blob"))?);
        // The source must end exactly at `size`.
        let mut probe = [0u8; 1];
        if reader.inner.read(&mut probe)? != 0 {
            return Err(StoreError::Io(io::Error::other("file grew while read")));
        }
        self.sync_object(id)?;
        Ok(id)
    }

    /// An editor of the tree `base` (the empty tree for a new one).
    pub fn edit_tree(&self, base: Oid) -> Result<TreeEdit<'_>> {
        let editor = self
            .repo
            .edit_tree(base.0)
            .map_err(git_err("tree"))?
            .detach();
        Ok(TreeEdit {
            editor,
            handle: self,
        })
    }

    /// Writes a commit of the store, with the Time Machine as author and committer.
    pub fn commit(&self, tree: Oid, parents: &[Oid], message: &str) -> Result<Oid> {
        let signature = gix::actor::Signature {
            name: "GitRaptor Time Machine".into(),
            email: "tm@gitraptor.invalid".into(),
            time: gix::date::Time::now_utc(),
        };
        let mut buf = gix::date::parse::TimeBuf::default();
        let sig = signature.to_ref(&mut buf);
        let commit = self
            .repo
            .new_commit_as(sig, sig, message, tree.0, parents.iter().map(|p| p.0))
            .map_err(git_err("commit"))?;
        let id = Oid(commit.id);
        self.sync_object(id)?;
        Ok(id)
    }

    /// The single full barrier of a capture, before its ref is created: `F_FULLFSYNC` on macOS,
    /// `fsync` elsewhere (ADR-TMC-001 § 4).
    pub fn barrier(&self) -> Result<()> {
        durable::full_barrier(&self.path.join("objects"))?;
        Ok(())
    }

    /// Creates `refs/tm/snap/<snapshot_id>`, which must not exist, and syncs it with its folder.
    pub fn create_ref(&self, snapshot_id: &str, commit: Oid) -> Result<()> {
        if !is_snapshot_id(snapshot_id) {
            return Err(StoreError::InvalidInput("not a snapshot id".into()));
        }
        let name = [SNAPSHOT_REF_PREFIX, snapshot_id].concat();
        self.repo
            .reference(
                name.as_str(),
                commit.0,
                gix::refs::transaction::PreviousValue::MustNotExist,
                "",
            )
            .map_err(git_err("ref"))?;
        let file = self.path.join(&name);
        durable::fsync_file(&file)?;
        let dir = file.parent().expect("ref has a folder");
        durable::fsync_dir(dir)?;
        durable::fsync_dir(dir.parent().expect("refs/tm"))?;
        Ok(())
    }

    /// Snapshot ids with a ref in the store, with their commit.
    pub fn snapshot_refs(&self) -> Result<Vec<(String, Oid)>> {
        let refs = self.repo.references().map_err(git_err("refs"))?;
        let mut out = Vec::new();
        for r in refs
            .prefixed(SNAPSHOT_REF_PREFIX)
            .map_err(git_err("refs"))?
        {
            let r = r.map_err(|e| StoreError::Git(format!("refs: {e}")))?;
            let name = r.name().as_bstr().to_str_lossy().into_owned();
            let Some(id) = name.strip_prefix(SNAPSHOT_REF_PREFIX) else {
                continue;
            };
            if let Some(target) = r.target().try_id() {
                out.push((id.to_owned(), Oid(target.to_owned())));
            }
        }
        out.sort();
        Ok(out)
    }

    /// Commit of a snapshot, if its ref exists.
    pub fn snapshot_commit(&self, snapshot_id: &str) -> Result<Option<Oid>> {
        if !is_snapshot_id(snapshot_id) {
            return Err(StoreError::InvalidInput("not a snapshot id".into()));
        }
        let name = [SNAPSHOT_REF_PREFIX, snapshot_id].concat();
        let found = self
            .repo
            .try_find_reference(name.as_str())
            .map_err(git_err("refs"))?;
        Ok(found.and_then(|r| r.target().try_id().map(|id| Oid(id.to_owned()))))
    }

    /// Deletes the refs of these snapshots in one transaction. Missing refs are skipped.
    pub fn delete_refs(&self, snapshot_ids: &[String]) -> Result<()> {
        use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};
        let mut edits = Vec::new();
        for id in snapshot_ids {
            if !is_snapshot_id(id) {
                return Err(StoreError::InvalidInput("not a snapshot id".into()));
            }
            if self.snapshot_commit(id)?.is_none() {
                continue;
            }
            let name = [SNAPSHOT_REF_PREFIX, id].concat();
            edits.push(RefEdit {
                change: Change::Delete {
                    expected: PreviousValue::MustExist,
                    log: RefLog::AndReference,
                },
                name: name.as_str().try_into().map_err(plumbing_err("ref name"))?,
                deref: false,
            });
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.repo.edit_references(edits).map_err(git_err("refs"))?;
        durable::fsync_dir(&self.path.join("refs/tm/snap"))?;
        Ok(())
    }

    /// Tree of a commit of the store.
    pub fn commit_tree(&self, commit: Oid) -> Result<Oid> {
        let c = self
            .repo
            .find_commit(commit.0)
            .map_err(|e| StoreError::Corrupt(format!("commit {commit}: {e}")))?;
        Ok(Oid(c
            .tree_id()
            .map_err(|e| StoreError::Corrupt(format!("commit {commit}: {e}")))?
            .detach()))
    }

    /// Parents of a commit of the store.
    pub fn commit_parents(&self, commit: Oid) -> Result<Vec<Oid>> {
        let c = self
            .repo
            .find_commit(commit.0)
            .map_err(|e| StoreError::Corrupt(format!("commit {commit}: {e}")))?;
        Ok(c.parent_ids().map(|p| Oid(p.detach())).collect())
    }

    /// Entry at `path` (with `/`) under `tree`.
    pub fn tree_entry(&self, tree: Oid, path: &BStr) -> Result<Option<(TreeEntryKind, Oid)>> {
        let t = self
            .repo
            .find_tree(tree.0)
            .map_err(|e| StoreError::Corrupt(format!("tree {tree}: {e}")))?;
        let entry = t
            .lookup_entry_by_path(path.to_str_lossy().as_ref())
            .map_err(|e| StoreError::Corrupt(format!("tree {tree}: {e}")))?;
        Ok(entry.map(|e| (TreeEntryKind::from_mode(e.mode()), Oid(e.object_id()))))
    }

    /// Every blob, link and gitlink under `tree`, by path, recursively.
    pub fn list_tree(&self, tree: Oid) -> Result<Vec<(String, TreeEntryKind, Oid)>> {
        let mut out = Vec::new();
        let mut stack = vec![(String::new(), tree)];
        while let Some((prefix, id)) = stack.pop() {
            let t = self
                .repo
                .find_tree(id.0)
                .map_err(|e| StoreError::Corrupt(format!("tree {id}: {e}")))?;
            let decoded = t
                .decode()
                .map_err(|e| StoreError::Corrupt(format!("tree {id}: {e}")))?;
            for e in &decoded.entries {
                let path = if prefix.is_empty() {
                    e.filename.to_str_lossy().into_owned()
                } else {
                    format!("{prefix}/{}", e.filename.to_str_lossy())
                };
                let kind = TreeEntryKind::from_mode(e.mode);
                if kind == TreeEntryKind::Tree {
                    stack.push((path, Oid(e.oid.to_owned())));
                } else {
                    out.push((path, kind, Oid(e.oid.to_owned())));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Content of a blob of the store.
    pub fn read_blob(&self, id: Oid) -> Result<Vec<u8>> {
        let blob = self
            .repo
            .find_blob(id.0)
            .map_err(|e| StoreError::Corrupt(format!("blob {id}: {e}")))?;
        Ok(blob.data.clone())
    }
}

/// Kind of a tree entry of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TreeEntryKind {
    Blob,
    Executable,
    Symlink,
    Gitlink,
    Tree,
}

impl TreeEntryKind {
    fn from_mode(mode: gix::objs::tree::EntryMode) -> Self {
        use gix::objs::tree::EntryKind as K;
        match mode.kind() {
            K::Blob => Self::Blob,
            K::BlobExecutable => Self::Executable,
            K::Link => Self::Symlink,
            K::Commit => Self::Gitlink,
            K::Tree => Self::Tree,
        }
    }

    /// The kind of an index or working tree entry.
    pub fn of(kind: EntryKind) -> Self {
        match kind {
            EntryKind::Blob => Self::Blob,
            EntryKind::Executable => Self::Executable,
            EntryKind::Symlink => Self::Symlink,
            EntryKind::Gitlink => Self::Gitlink,
        }
    }

    fn to_gix(self) -> gix::objs::tree::EntryKind {
        use gix::objs::tree::EntryKind as K;
        match self {
            Self::Blob => K::Blob,
            Self::Executable => K::BlobExecutable,
            Self::Symlink => K::Link,
            Self::Gitlink => K::Commit,
            Self::Tree => K::Tree,
        }
    }
}

/// Edits a tree of the store in memory; [`TreeEdit::write`] writes and syncs every changed tree.
pub struct TreeEdit<'a> {
    editor: gix::objs::tree::Editor<'a>,
    handle: &'a StoreHandle,
}

fn components(path: &BStr) -> impl Iterator<Item = &BStr> {
    path.split(|b| *b == b'/').map(ByteSlice::as_bstr)
}

impl TreeEdit<'_> {
    /// Inserts or replaces the entry at `path` (with `/`).
    pub fn upsert(&mut self, path: &BStr, kind: TreeEntryKind, id: Oid) -> Result<()> {
        self.editor
            .upsert(components(path), kind.to_gix(), id.0)
            .map_err(plumbing_err("tree edit"))?;
        Ok(())
    }

    /// Removes the entry at `path`; nothing happens if it does not exist.
    pub fn remove(&mut self, path: &BStr) -> Result<()> {
        self.editor
            .remove(components(path))
            .map_err(plumbing_err("tree edit"))?;
        Ok(())
    }

    /// Writes the changed trees, each one synced, and returns the root. The syncs run on several
    /// threads: a scattered delta rewrites about one tree per changed file.
    pub fn write(&mut self) -> Result<Oid> {
        let handle = self.handle;
        let mut written = Vec::new();
        let root = self.editor.write(|tree| -> Result<gix::ObjectId> {
            let id = handle
                .repo
                .write_object(tree)
                .map_err(git_err("tree"))?
                .detach();
            written.push(id);
            Ok(id)
        })?;
        handle.sync_objects(&written)?;
        Ok(Oid(root))
    }
}

/// How much blob data is streamed between two polls of `yield_now`.
const POLL_BYTES: usize = 256 << 10;

/// Reads exactly `left` bytes and polls `yield_now` every [`POLL_BYTES`].
struct Checked<'a> {
    inner: &'a mut dyn Read,
    left: u64,
    since_poll: usize,
    yield_now: &'a dyn Fn() -> bool,
    yielded: bool,
}

impl Read for Checked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        if self.since_poll >= POLL_BYTES {
            self.since_poll = 0;
            if (self.yield_now)() {
                self.yielded = true;
                return Err(io::Error::other("yielded to a guaranteed prior snapshot"));
            }
        }
        let max = usize::try_from(self.left)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = self.inner.read(&mut buf[..max])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file shrank while read",
            ));
        }
        self.left -= n as u64;
        self.since_poll += n;
        Ok(n)
    }
}
