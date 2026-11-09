//! The copyable command of a conscious exception, as a pure function of a log entry.
//!
//! The command is built from the normalized operation the engine logged, never from an argv. It
//! never carries `--yes`: the terminal shows the plan and asks before it announces anything.

use std::path::Path;

use gitraptor_api::display::must_escape;
use gitraptor_api::guard::{GuardLogEntry, LogDetail, LoggedOperation, LoggedRef, RefChange, Rule};
use serde::Serialize;

use super::shell_command::posix;

/// Rules a conscious exception can skip. The daemon stays the judge: a rule missing here only
/// means the terminal would refuse it with its own reason.
pub const SKIPPABLE: &[Rule] = &[
    Rule::MinimumForcePush,
    Rule::MinimumBaseBranchDelete,
    Rule::ProtectedBranch,
    Rule::ForbiddenPath,
    Rule::DiffSize,
    Rule::CommitFormat,
    Rule::PermissionCommit,
    Rule::PermissionPush,
    Rule::PermissionForcePush,
    Rule::PermissionResetHard,
    Rule::PermissionBranchDelete,
    Rule::PermissionRebase,
    Rule::PermissionMerge,
    Rule::PermissionWorktreeAdd,
    Rule::PermissionWorktreeRemove,
];

/// Why no command is offered for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotOffered {
    Commit,
    Rebase,
    Operation,
    Rule,
    Remote,
    WhereUnknown,
    RateLimited,
    UnsafeText,
    RaptorUnknown,
}

/// The command that announces an exception for the denial in `entry`.
///
/// # Errors
///
/// A [`NotOffered`] reason when the entry cannot be turned into a safe command.
pub fn exception(raptor: Option<&Path>, entry: &GuardLogEntry) -> Result<String, NotOffered> {
    if entry.detail == LogDetail::RateLimited {
        return Err(NotOffered::RateLimited);
    }
    // The operation decides first: a commit or a rebase has no command whatever its rules.
    let plan = match &entry.operation {
        LoggedOperation::Commit { .. } => return Err(NotOffered::Commit),
        LoggedOperation::Rebase { .. } => return Err(NotOffered::Rebase),
        LoggedOperation::ProtectionState { .. } | LoggedOperation::McpAllowlist { .. } => {
            return Err(NotOffered::Operation);
        }
        LoggedOperation::Push { refs, .. } if refs.is_empty() => {
            return Err(NotOffered::Operation);
        }
        LoggedOperation::RefTransaction { refs } => branch_delete_names(refs)?,
        LoggedOperation::Push { .. } => Plan::Push,
    };
    if entry.reasons.is_empty() || !entry.reasons.iter().all(|r| SKIPPABLE.contains(&r.rule)) {
        return Err(NotOffered::Rule);
    }
    let mut git = vec![
        "git".to_owned(),
        "-C".to_owned(),
        worktree(entry)?.to_owned(),
    ];
    match plan {
        Plan::Branches(names) => {
            git.extend(["branch".to_owned(), "-d".to_owned()]);
            git.extend(names);
        }
        Plan::Push => {
            let LoggedOperation::Push { remote, refs } = &entry.operation else {
                return Err(NotOffered::Operation);
            };
            git.push("push".to_owned());
            if refs.iter().any(|r| r.change == RefChange::Force) {
                git.push("--force-with-lease".to_owned());
                git.push("--force-if-includes".to_owned());
            }
            git.push(remote_name(remote.as_ref().map(|r| r.raw()))?.to_owned());
            let branch = entry.branch.as_ref().map(|b| b.raw());
            for r in refs {
                git.push(refspec(branch, r)?);
            }
        }
    }
    let raptor = raptor
        .and_then(Path::to_str)
        .filter(|path| !path.chars().any(must_escape))
        .ok_or(NotOffered::RaptorUnknown)?;
    let mut args = vec![raptor, "guard", "exec", "--"];
    args.extend(git.iter().map(String::as_str));
    posix(&args).ok_or(NotOffered::UnsafeText)
}

/// The command that cancels the pending exception of the repo at `path`.
#[must_use]
pub fn cancel(raptor: Option<&Path>, path: &str) -> Option<String> {
    let raptor = raptor?.to_str()?;
    if !(Path::new(path).is_absolute() || path.starts_with('/')) {
        return None;
    }
    posix(&[raptor, "guard", "cancel", path])
}

enum Plan {
    Push,
    Branches(Vec<String>),
}

/// The short names of a ref transaction that only deletes local branches.
fn branch_delete_names(refs: &[LoggedRef]) -> Result<Plan, NotOffered> {
    if refs.is_empty() {
        return Err(NotOffered::Operation);
    }
    let mut names = Vec::with_capacity(refs.len());
    for r in refs {
        let short = r.name.raw().strip_prefix("refs/heads/");
        match (r.change, short) {
            (RefChange::Delete, Some(short)) => names.push(checked(short)?.to_owned()),
            _ => return Err(NotOffered::Operation),
        }
    }
    Ok(Plan::Branches(names))
}

/// One refspec: `:<ref>` to delete, `<branch>:<ref>` when the branch is known and differs.
fn refspec(branch: Option<&str>, r: &LoggedRef) -> Result<String, NotOffered> {
    let name = checked(r.name.raw())?;
    if r.change == RefChange::Delete {
        return Ok(format!(":{name}"));
    }
    match branch {
        Some(branch) if branch != name => Ok(format!("{}:{name}", checked(branch)?)),
        _ => Ok(name.to_owned()),
    }
}

/// A branch or ref name the daemon would accept (it refuses these as `unsafe-text`).
fn checked(name: &str) -> Result<&str, NotOffered> {
    let bad = name.is_empty()
        || name.starts_with(['-', '+'])
        || name.contains([':', '*', '?', '[', '\\', '!'])
        || name.chars().any(must_escape);
    if bad {
        Err(NotOffered::UnsafeText)
    } else {
        Ok(name)
    }
}

/// A remote given by name; a URL, a path or a missing remote is refused.
fn remote_name(remote: Option<&str>) -> Result<&str, NotOffered> {
    let remote = remote.ok_or(NotOffered::Remote)?;
    if remote.contains("://") || (remote.contains('@') && remote.contains(':')) {
        return Err(NotOffered::Remote);
    }
    if remote.starts_with(['/', '.', '~']) {
        return Err(NotOffered::UnsafeText);
    }
    checked(remote)
}

/// The absolute worktree the command runs in.
fn worktree(entry: &GuardLogEntry) -> Result<&str, NotOffered> {
    let path = entry.worktree.as_ref().map(|w| w.raw());
    match path {
        Some(path) if Path::new(path).is_absolute() || path.starts_with('/') => {
            if path.chars().any(must_escape) {
                Err(NotOffered::UnsafeText)
            } else {
                Ok(path)
            }
        }
        _ => Err(NotOffered::WhereUnknown),
    }
}
