//! Typed functions of the closed Git CLI allowlist (ADR-GRP-009 § 3).
//!
//! There is no free-text entry point: each function builds a fixed argv. `status`, `diff`,
//! `config --list` and `config --get-regexp` do not exist here on purpose (SEC-09, SEC-05).
//! Revisions go after `--end-of-options` so they are never parsed as options (SEC-02); in these
//! subcommands a plain `--` would turn them into paths.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::invoke::{Invoker, Subcommand};
use crate::{ReadError, RefName, SystemGit};

/// Fixed `log` format: commit, parents, author time and subject. Never `%G*` (it runs `gpg`).
pub const LOG_FORMAT: &str = "--format=%H%x00%P%x00%at%x00%s%x1e";

/// A Git CLI bound to a resolved executable and one repository.
#[derive(Debug, Clone)]
pub struct GitCli<'a> {
    git: &'a SystemGit,
    invoker: &'a Invoker,
    repo: PathBuf,
}

/// Configuration keys the layer may read. Anything else (`http.extraHeader`, credentials…) is
/// not representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigKey {
    /// `init.defaultBranch`.
    InitDefaultBranch,
    /// `remote.<name>.url`, returned without userinfo.
    RemoteUrl(RefName),
    /// `branch.<name>.remote`.
    BranchRemote(RefName),
    /// `branch.<name>.merge`.
    BranchMerge(RefName),
}

impl ConfigKey {
    fn name(&self) -> String {
        match self {
            Self::InitDefaultBranch => "init.defaultBranch".into(),
            Self::RemoteUrl(n) => format!("remote.{n}.url"),
            Self::BranchRemote(n) => format!("branch.{n}.remote"),
            Self::BranchMerge(n) => format!("branch.{n}.merge"),
        }
    }
}

/// A reference from `for-each-ref`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    pub name: String,
    pub id: String,
}

/// Which namespace `for-each-ref` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefNamespace {
    Heads,
    Remotes,
    Tags,
}

/// A worktree from `worktree list --porcelain -z`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// A commit from `log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub id: String,
    pub parents: Vec<String>,
    pub author_time: i64,
    pub subject: String,
}

impl<'a> GitCli<'a> {
    /// Bind `git` to the repository or worktree at `repo` (validated, SEC-02).
    pub fn new(git: &'a SystemGit, invoker: &'a Invoker, repo: &Path) -> Result<Self, ReadError> {
        crate::paths::validate(repo)?;
        Ok(Self {
            git,
            invoker,
            repo: repo.to_owned(),
        })
    }

    fn run(&self, sub: Subcommand, args: &[&OsStr], what: &str) -> Result<Vec<u8>, ReadError> {
        self.invoker
            .run(&self.git.path, Some(&self.repo), sub, args)?
            .into_success(what)
    }

    /// `git rev-parse --verify --quiet --end-of-options <rev>^{commit}`: the commit a ref points
    /// to, or `None` if it does not exist.
    pub fn rev_parse_verify(&self, rev: &RefName) -> Result<Option<String>, ReadError> {
        let spec = format!("{rev}^{{commit}}");
        let out = self.invoker.run(
            &self.git.path,
            Some(&self.repo),
            Subcommand::RevParse,
            &[
                "--verify".as_ref(),
                "--quiet".as_ref(),
                "--end-of-options".as_ref(),
                spec.as_ref(),
            ],
        )?;
        if out.success {
            return Ok(Some(text(&out.stdout).trim().to_owned()));
        }
        if out.code == Some(1) && out.stderr.is_empty() {
            return Ok(None);
        }
        out.into_success("rev-parse").map(|_| None)
    }

    /// `git for-each-ref` over one namespace with a fixed format.
    pub fn for_each_ref(&self, namespace: RefNamespace) -> Result<Vec<RefEntry>, ReadError> {
        let pattern = match namespace {
            RefNamespace::Heads => "refs/heads/",
            RefNamespace::Remotes => "refs/remotes/",
            RefNamespace::Tags => "refs/tags/",
        };
        let out = self.run(
            Subcommand::ForEachRef,
            &[
                "--format=%(refname)%00%(objectname)".as_ref(),
                "--end-of-options".as_ref(),
                pattern.as_ref(),
            ],
            "for-each-ref",
        )?;
        Ok(text(&out)
            .lines()
            .filter_map(|l| l.split_once('\0'))
            .map(|(name, id)| RefEntry {
                name: name.to_owned(),
                id: id.to_owned(),
            })
            .collect())
    }

    /// `git worktree list --porcelain -z`.
    pub fn worktree_list(&self) -> Result<Vec<WorktreeEntry>, ReadError> {
        let out = self.run(
            Subcommand::WorktreeList,
            &["--porcelain".as_ref(), "-z".as_ref()],
            "worktree list",
        )?;
        Ok(parse_worktree_list(&text(&out)))
    }

    /// `git rev-list --count --left-right --end-of-options <left>...<right>`: commits only in
    /// `left` and only in `right`.
    pub fn rev_list_left_right_count(
        &self,
        left: &RefName,
        right: &RefName,
    ) -> Result<(u64, u64), ReadError> {
        let range = format!("{left}...{right}");
        let out = self.run(
            Subcommand::RevList,
            &[
                "--count".as_ref(),
                "--left-right".as_ref(),
                "--end-of-options".as_ref(),
                range.as_ref(),
            ],
            "rev-list",
        )?;
        let out = text(&out);
        let mut parts = out.split_whitespace().map(str::parse::<u64>);
        match (parts.next(), parts.next()) {
            (Some(Ok(l)), Some(Ok(r))) => Ok((l, r)),
            _ => Err(ReadError::Unavailable("unexpected rev-list output".into())),
        }
    }

    /// `git merge-base --end-of-options <a> <b>`, or `None` without a common ancestor.
    pub fn merge_base(&self, a: &RefName, b: &RefName) -> Result<Option<String>, ReadError> {
        let out = self.invoker.run(
            &self.git.path,
            Some(&self.repo),
            Subcommand::MergeBase,
            &[
                "--end-of-options".as_ref(),
                a.as_str().as_ref(),
                b.as_str().as_ref(),
            ],
        )?;
        if out.success {
            return Ok(Some(text(&out.stdout).trim().to_owned()));
        }
        if out.code == Some(1) && out.stderr.is_empty() {
            return Ok(None);
        }
        out.into_success("merge-base").map(|_| None)
    }

    /// `git log` with [`LOG_FORMAT`], at most `max` commits from `rev`.
    pub fn log(&self, rev: &RefName, max: u32) -> Result<Vec<LogEntry>, ReadError> {
        debug_assert!(!LOG_FORMAT.contains("%G"));
        let max = format!("--max-count={max}");
        let out = self.run(
            Subcommand::Log,
            &[
                "--no-ext-diff".as_ref(),
                "--no-textconv".as_ref(),
                "--no-show-signature".as_ref(),
                "--no-color".as_ref(),
                "--no-decorate".as_ref(),
                LOG_FORMAT.as_ref(),
                max.as_ref(),
                "--end-of-options".as_ref(),
                rev.as_str().as_ref(),
                "--".as_ref(),
            ],
            "log",
        )?;
        Ok(text(&out)
            .split('\x1e')
            .map(|r| r.trim_start_matches('\n'))
            .filter(|r| !r.is_empty())
            .filter_map(|record| {
                let mut f = record.splitn(4, '\0');
                Some(LogEntry {
                    id: f.next()?.to_owned(),
                    parents: f.next()?.split_whitespace().map(str::to_owned).collect(),
                    author_time: f.next()?.parse().ok()?,
                    subject: f.next()?.to_owned(),
                })
            })
            .collect())
    }

    /// `git config --get <key>` for an allowlisted key. Remote URLs come back without userinfo.
    pub fn config_get(&self, key: &ConfigKey) -> Result<Option<String>, ReadError> {
        let name = key.name();
        let out = self.invoker.run(
            &self.git.path,
            Some(&self.repo),
            Subcommand::Config,
            &["--get".as_ref(), "--end-of-options".as_ref(), name.as_ref()],
        )?;
        if out.code == Some(1) {
            return Ok(None);
        }
        let value = text(&out.into_success("config --get")?)
            .trim_end()
            .to_owned();
        Ok(Some(match key {
            ConfigKey::RemoteUrl(_) => crate::redact::remote_url(&value),
            _ => value,
        }))
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_worktree_list(out: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for field in out.split('\0') {
        if field.is_empty() {
            if let Some(e) = current.take() {
                entries.push(e);
            }
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        if key == "worktree" {
            if let Some(e) = current.take() {
                entries.push(e);
            }
            current = Some(WorktreeEntry {
                path: value.into(),
                ..Default::default()
            });
            continue;
        }
        let Some(e) = current.as_mut() else { continue };
        match key {
            "HEAD" => e.head = Some(value.to_owned()),
            "branch" => e.branch = Some(value.to_owned()),
            "detached" => e.detached = true,
            "bare" => e.bare = true,
            "locked" => e.locked = true,
            "prunable" => e.prunable = true,
            _ => {}
        }
    }
    entries.extend(current);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_never_invokes_gpg() {
        assert!(!LOG_FORMAT.contains("%G"));
    }

    #[test]
    fn parses_worktree_porcelain() {
        let out = "worktree /r\0HEAD abc\0branch refs/heads/main\0\0worktree /w\0HEAD def\0detached\0locked reason\0\0";
        let list = parse_worktree_list(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(list[1].detached && list[1].locked);
        assert_eq!(list[1].path, PathBuf::from("/w"));
    }
}
