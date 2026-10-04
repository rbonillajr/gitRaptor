//! Hot-path reads with gitoxide (ADR-GRP-009 § 1).
//!
//! The repository is opened read-only with every user-configured program neutralized in memory:
//! no API here writes the index, refs, config or objects, and nothing spawns a process.

use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::sec::Permission;

use crate::{ReadError, RefName};

/// Options of [`RepoReader::open`].
#[derive(Debug, Clone, Default)]
pub struct ReaderOptions {
    /// Open as if the repository were not owned by the current user. Lowers trust only, so it
    /// can never make an untrusted repository readable; used to test the `safe.directory` path.
    pub force_reduced_trust: bool,
    /// Ignore the system, Git-installation and global (home) config, so only the repository's
    /// own config is read. Stricter only: it can drop a `safe.directory` allowlist, never add
    /// trust. Used to test the `safe.directory` path on machines whose ambient config trusts
    /// every directory (CI runners ship `safe.directory = *`). Never set from configuration or
    /// the MCP channel.
    #[doc(hidden)]
    pub ignore_ambient_config: bool,
}

/// A short-lived, read-only view of one repository or worktree. Open it per recompute and drop
/// it afterwards, so pack mappings are not held (consequence for Windows in ADR-GRP-009).
pub struct RepoReader {
    pub(crate) repo: gix::Repository,
}

impl std::fmt::Debug for RepoReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoReader")
            .field("git_dir", &self.repo.git_dir())
            .finish()
    }
}

/// Where `HEAD` points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// Short branch name, if `HEAD` is symbolic.
    pub branch: Option<String>,
    /// The commit `HEAD` resolves to, if any.
    pub commit: Option<String>,
    /// `HEAD` points directly at a commit.
    pub detached: bool,
    /// `HEAD` names a branch without commits.
    pub unborn: bool,
}

/// A local branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub commit: String,
}

/// A linked worktree registered in the common Git directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWorktree {
    /// Name under `.git/worktrees/`.
    pub id: String,
    /// Root of the working tree, as recorded by Git.
    pub path: PathBuf,
    /// Its private Git directory.
    pub git_dir: PathBuf,
    pub locked: bool,
}

/// An operation in progress, from its marker files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgress {
    ApplyMailbox,
    ApplyMailboxRebase,
    Bisect,
    CherryPick,
    CherryPickSequence,
    Merge,
    Rebase,
    RebaseInteractive,
    Revert,
    RevertSequence,
}

/// What changed in a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    TypeChanged,
    Conflicted,
}

/// One changed path, relative to the worktree root, with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

/// The status of a worktree, computed without filters (SEC-09).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// `HEAD` tree versus the index.
    pub staged: Vec<Change>,
    /// Index versus the working tree.
    pub unstaged: Vec<Change>,
    /// Untracked files, not ignored.
    pub untracked: Vec<String>,
}

impl Status {
    /// `true` if nothing is staged, modified or untracked.
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.untracked.is_empty()
    }
}

/// A bounded commit count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Exact(u64),
    /// The walk stopped at the limit: there are at least this many.
    AtLeast(u64),
}

fn unavailable(what: &str) -> impl Fn(gix::Error) -> ReadError + '_ {
    move |e| ReadError::Unavailable(format!("{what}: {e}"))
}

impl RepoReader {
    /// Open the repository or worktree at `path` (no upward discovery).
    pub fn open(path: &Path, options: &ReaderOptions) -> Result<Self, ReadError> {
        crate::paths::validate(path)?;
        let mut open = gix::open::Options::default()
            .permissions(permissions(options.ignore_ambient_config))
            .bail_if_untrusted(true);
        if options.force_reduced_trust {
            open = open.with(gix::sec::Trust::Reduced);
        }
        let mut repo = match gix::open_opts(path, open) {
            Ok(repo) => repo,
            Err(e) => {
                let message = e.to_string();
                // gix 0.88 erases this error; its wording is pinned by
                // `untrusted_repo_is_unavailable`, which fails if it changes.
                return Err(if message.contains("is considered unsafe") {
                    ReadError::Untrusted(message)
                } else {
                    ReadError::Unavailable(message)
                });
            }
        };
        neutralize_programs(&mut repo)?;
        // Replacement objects (`refs/replace/*`) never change what is read (SEC-GRD-17, H-05).
        repo.objects.ignore_replacements = true;
        Ok(Self { repo })
    }

    /// Where `HEAD` points.
    pub fn head(&self) -> Result<Head, ReadError> {
        let head = self.repo.head().map_err(unavailable("HEAD"))?;
        let branch = head
            .referent_name()
            .map(|n| n.shorten().to_str_lossy().into_owned());
        let commit = head.id().map(|id| id.to_string());
        Ok(Head {
            detached: head.is_detached(),
            unborn: head.is_unborn(),
            branch,
            commit,
        })
    }

    /// Local branches with the commit each points to.
    pub fn local_branches(&self) -> Result<Vec<Branch>, ReadError> {
        let refs = self.repo.references().map_err(unavailable("refs"))?;
        let mut branches = Vec::new();
        for r in refs.local_branches().map_err(unavailable("refs"))? {
            let mut r = r.map_err(|e| ReadError::Unavailable(format!("refs: {e}")))?;
            let name = r.name().shorten().to_str_lossy().into_owned();
            let commit = r.peel_to_id().map_err(unavailable("refs"))?.to_string();
            branches.push(Branch { name, commit });
        }
        Ok(branches)
    }

    /// Number of index entries. Reads the index, never refreshes or writes it.
    pub fn index_entry_count(&self) -> Result<usize, ReadError> {
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        Ok(index.entries().len())
    }

    /// Operation in progress, from the marker files.
    pub fn in_progress(&self) -> Option<InProgress> {
        use gix::state::InProgress as S;
        self.repo.state().map(|s| match s {
            S::ApplyMailbox => InProgress::ApplyMailbox,
            S::ApplyMailboxRebase => InProgress::ApplyMailboxRebase,
            S::Bisect => InProgress::Bisect,
            S::CherryPick => InProgress::CherryPick,
            S::CherryPickSequence => InProgress::CherryPickSequence,
            S::Merge => InProgress::Merge,
            S::Rebase => InProgress::Rebase,
            S::RebaseInteractive => InProgress::RebaseInteractive,
            S::Revert => InProgress::Revert,
            S::RevertSequence => InProgress::RevertSequence,
        })
    }

    /// Linked worktrees registered in the common Git directory.
    pub fn worktrees(&self) -> Result<Vec<LinkedWorktree>, ReadError> {
        let proxies = self
            .repo
            .worktrees()
            .map_err(|e| ReadError::Unavailable(format!("worktrees: {e}")))?;
        let mut out = Vec::new();
        for proxy in proxies {
            let path = proxy
                .base()
                .map_err(|e| ReadError::Unavailable(format!("worktree: {e}")))?;
            out.push(LinkedWorktree {
                id: proxy.id().to_str_lossy().into_owned(),
                git_dir: proxy.git_dir().to_owned(),
                locked: proxy.is_locked(),
                path,
            });
        }
        Ok(out)
    }

    /// Whether a worktree-relative path is ignored by the repository and user ignore rules.
    pub fn is_ignored(&self, rela_path: &str, is_dir: bool) -> Result<bool, ReadError> {
        if rela_path.starts_with('/') || rela_path.split('/').any(|c| c == "..") {
            return Err(ReadError::InvalidInput(
                "path must be relative to the worktree".into(),
            ));
        }
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        let mut excludes = self
            .repo
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(unavailable("ignore rules"))?;
        let mode = if is_dir {
            gix::index::entry::Mode::DIR
        } else {
            gix::index::entry::Mode::FILE
        };
        let platform = excludes
            .at_entry(rela_path, Some(mode))
            .map_err(|e| ReadError::Unavailable(format!("ignore rules: {e}")))?;
        Ok(platform.is_excluded())
    }

    /// URL of a remote, without userinfo (SEC-05).
    pub fn remote_url(&self, name: &RefName) -> Result<Option<String>, ReadError> {
        let Ok(remote) = self.repo.find_remote(name.as_str()) else {
            return Ok(None);
        };
        Ok(remote
            .url(gix::remote::Direction::Fetch)
            .map(|url| crate::redact::remote_url(&url.to_bstring().to_str_lossy())))
    }

    /// Commit a ref points to.
    pub fn resolve_ref(&self, name: &RefName) -> Result<Option<String>, ReadError> {
        Ok(self.commit_of(name)?.map(|id| id.to_string()))
    }

    fn commit_of(&self, name: &RefName) -> Result<Option<gix::ObjectId>, ReadError> {
        let Some(mut r) = self
            .repo
            .try_find_reference(name.as_str())
            .map_err(unavailable("refs"))?
        else {
            return Ok(None);
        };
        Ok(Some(r.peel_to_id().map_err(unavailable("refs"))?.detach()))
    }

    fn require_commit(&self, name: &RefName) -> Result<gix::ObjectId, ReadError> {
        self.commit_of(name)?
            .ok_or_else(|| ReadError::Unavailable(format!("ref not found: {name}")))
    }

    /// Best common ancestor of two refs, or `None` if they share no history.
    pub fn merge_base(&self, a: &RefName, b: &RefName) -> Result<Option<String>, ReadError> {
        let (a, b) = (self.require_commit(a)?, self.require_commit(b)?);
        match self.repo.merge_base(a, b) {
            Ok(id) => Ok(Some(id.to_string())),
            Err(e) if e.to_string().contains("No merge base") => Ok(None),
            Err(e) => Err(ReadError::Unavailable(format!("merge-base: {e}"))),
        }
    }

    /// Commits only in `a` (ahead) and only in `b` (behind), each walk bounded by `limit`.
    pub fn ahead_behind(
        &self,
        a: &RefName,
        b: &RefName,
        limit: u64,
    ) -> Result<(Count, Count), ReadError> {
        let (a, b) = (self.require_commit(a)?, self.require_commit(b)?);
        Ok((
            self.count_only_in(a, b, limit)?,
            self.count_only_in(b, a, limit)?,
        ))
    }

    fn count_only_in(
        &self,
        tip: gix::ObjectId,
        hidden: gix::ObjectId,
        limit: u64,
    ) -> Result<Count, ReadError> {
        let walk = self
            .repo
            .rev_walk([tip])
            .with_hidden([hidden])
            .all()
            .map_err(unavailable("rev-walk"))?;
        let mut n = 0;
        for info in walk {
            info.map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
            if n == limit {
                return Ok(Count::AtLeast(limit));
            }
            n += 1;
        }
        Ok(Count::Exact(n))
    }

    /// Status of the worktree: staged, unstaged and untracked paths. Computed with gitoxide and
    /// without filters, so a dirty-stat file whose bytes differ is reported as modified
    /// (SEC-09). Submodules are not entered. The index is never written.
    pub fn status(&self) -> Result<Status, ReadError> {
        use gix::status::index_worktree::Item as Wt;
        use gix::status::plumbing::index_as_worktree::{Change as WtChange, EntryStatus};

        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(unavailable("status"))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_submodules(gix::status::Submodule::Given {
                ignore: gix::submodule::config::Ignore::All,
                check_dirty: false,
            })
            .index_worktree_rewrites(None)
            // One thread, as before gix's `parallel` feature was enabled for the Time Machine
            // store writer (ADR-GRP-009 § 3: the engine's reads stay bounded).
            .index_worktree_options_mut(|o| o.thread_limit = Some(1))
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
            .into_iter(None)
            .map_err(unavailable("status"))?;

        let mut status = Status::default();
        for item in iter {
            let item = item.map_err(|e| ReadError::Unavailable(format!("status: {e}")))?;
            match item {
                gix::status::Item::TreeIndex(change) => {
                    use gix::diff::index::ChangeRef as C;
                    let (path, kind) = match change {
                        C::Addition { location, .. } => (location, ChangeKind::Added),
                        C::Deletion { location, .. } => (location, ChangeKind::Deleted),
                        C::Modification { location, .. } => (location, ChangeKind::Modified),
                        C::Rewrite { location, .. } => (location, ChangeKind::Added),
                    };
                    status.staged.push(Change {
                        path: path.to_str_lossy().into_owned(),
                        kind,
                    });
                }
                gix::status::Item::IndexWorktree(Wt::Modification {
                    rela_path,
                    status: entry_status,
                    ..
                }) => {
                    let kind = match entry_status {
                        EntryStatus::Conflict { .. } => Some(ChangeKind::Conflicted),
                        EntryStatus::Change(WtChange::Removed) => Some(ChangeKind::Deleted),
                        EntryStatus::Change(WtChange::Type { .. }) => Some(ChangeKind::TypeChanged),
                        EntryStatus::Change(WtChange::Modification { .. })
                        | EntryStatus::Change(WtChange::SubmoduleModification(_)) => {
                            Some(ChangeKind::Modified)
                        }
                        // Only the stat changed; the content is equal. Nothing is written back.
                        EntryStatus::NeedsUpdate(_) => None,
                        EntryStatus::IntentToAdd => Some(ChangeKind::Added),
                    };
                    if let Some(kind) = kind {
                        status.unstaged.push(Change {
                            path: rela_path.to_str_lossy().into_owned(),
                            kind,
                        });
                    }
                }
                gix::status::Item::IndexWorktree(Wt::DirectoryContents { entry, .. }) => {
                    if entry.status == gix::dir::entry::Status::Untracked {
                        status
                            .untracked
                            .push(entry.rela_path.to_str_lossy().into_owned());
                    }
                }
                gix::status::Item::IndexWorktree(Wt::Rewrite { .. }) => {}
            }
        }
        status.staged.sort_by(|a, b| a.path.cmp(&b.path));
        status.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        status.untracked.sort();
        Ok(status)
    }
}

/// Configuration sources gitoxide may read (M1, SEC-10).
fn permissions(ignore_ambient_config: bool) -> gix::open::Permissions {
    let mut p = gix::open::Permissions::default();
    // Never run `git` to discover the installation config or attributes (M1).
    p.config.git_binary = false;
    p.attributes.git_binary = false;
    // On Windows, locating the system config makes gix-path run `git`; skip it there.
    p.config.system = cfg!(not(windows));
    p.attributes.system = cfg!(not(windows));
    // Variables of the engine process do not steer the read (`GIT_CONFIG_PARAMETERS`, `GIT_DIR`…).
    p.config.env = false;
    p.env = gix::open::permissions::Environment::isolated();
    p.env.home = Permission::Allow;
    if ignore_ambient_config {
        p.config.system = false;
        p.config.git = false;
        p.config.user = false;
        p.attributes.system = false;
        p.attributes.git = false;
        p.env.home = Permission::Deny;
    }
    p
}

/// Remove, in memory only, every config section that names a program run while reading:
/// filter drivers (`filter.<name>.clean|smudge|process`) and diff drivers
/// (`diff.<name>.textconv|command`). Nothing is persisted (SEC-09).
fn neutralize_programs(repo: &mut gix::Repository) -> Result<(), ReadError> {
    let mut config = repo.config_snapshot_mut();
    for section in ["filter", "diff"] {
        let names: Vec<gix::bstr::BString> = config
            .sections_by_name(section)
            .into_iter()
            .flatten()
            .filter_map(|s| s.header().subsection_name().map(ToOwned::to_owned))
            .collect();
        for name in names {
            while config
                .remove_section(section, Some(name.as_bstr()))
                .is_some()
            {}
        }
    }
    config
        .commit()
        .map(|_| ())
        .map_err(|e| ReadError::Unavailable(format!("config: {e}")))
}
