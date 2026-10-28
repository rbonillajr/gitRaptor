//! The repo facts a conscious exception is normalized against (US-GRD-006, D3), read only after
//! the caller was authorized, on the connection thread, in the directory where its git acts.
//!
//! Branches, upstreams and oids come from the read layer (gitoxide and the Git CLI wrapper of
//! ADR-GRP-009), and so do `push.default`, the push remote (`branch.<x>.pushRemote`, then
//! `remote.pushDefault`) and the names of the `textconv` drivers. A fact that cannot be read
//! refuses the exception (fail closed) instead of being guessed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gitraptor_api::guard::ExceptionRefusal;
use gitraptor_git::cli::{ConfigKey, GitCli};
use gitraptor_git::{Invoker, RefName, RepoReader, SystemGit};
use gitraptor_policy::guard::exception::{Form, RepoFacts, Request, Upstream};

/// The `-c` every exception's git runs with (D10).
pub const NEUTRAL_CONFIG: [&str; 5] = [
    "core.fsmonitor=false",
    "core.pager=cat",
    "core.editor=false",
    "sequence.editor=false",
    "diff.external=",
];

/// The `-c` list of an issued exception: the fixed ones and one empty textconv per driver.
pub fn issued_config(drivers: &[String]) -> Vec<String> {
    NEUTRAL_CONFIG
        .iter()
        .map(|c| (*c).to_owned())
        .chain(drivers.iter().map(|d| format!("diff.{d}.textconv=")))
        .collect()
}

/// The root of the worktree `dir` is in: the nearest ancestor holding `.git`, or `dir`.
pub fn worktree_root(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|a| std::fs::symlink_metadata(a.join(".git")).is_ok())
        .unwrap_or(dir)
        .to_path_buf()
}

/// `dir` as the binding of a worktree compares it: canonical, at its worktree root.
pub fn worktree_key(dir: &Path) -> Option<String> {
    let dir = gitraptor_git::paths::canonicalize(dir).ok()?;
    Some(worktree_root(&dir).to_string_lossy().into_owned())
}

/// Where the git of an exception acts: `base`, then each `-C` in order as git applies them.
///
/// # Errors
///
/// `unsupported-form` when a directory does not exist or leaves the worktrees of `common`.
pub fn effective_dir(
    base: &Path,
    dirs: &[String],
    common: &Path,
) -> Result<(PathBuf, RepoReader), ExceptionRefusal> {
    let mut dir = base.to_path_buf();
    for d in dirs {
        dir = dir.join(d);
    }
    let dir = gitraptor_git::paths::canonicalize(&dir)
        .ok()
        .filter(|d| d.is_dir())
        .ok_or(ExceptionRefusal::UnsupportedForm)?;
    let root = worktree_root(&dir);
    let reader = super::authorship::worktree_reader(&root, common)
        .ok_or(ExceptionRefusal::UnsupportedForm)?;
    Ok((dir, reader))
}

fn refname(name: &str) -> Result<RefName, ExceptionRefusal> {
    RefName::new(name).map_err(|_| ExceptionRefusal::UnsupportedForm)
}

/// The textconv drivers of the configuration `dir` sees, to neutralize each one (D10).
///
/// # Errors
///
/// `unsupported-form` when they cannot be read or named: an unneutralized driver would run
/// with the token in its environment.
pub fn textconv_drivers(
    git: &SystemGit,
    invoker: &Invoker,
    dir: &Path,
) -> Result<Vec<String>, ExceptionRefusal> {
    GitCli::new(git, invoker, dir)
        .and_then(|cli| cli.textconv_drivers())
        .map_err(|_| ExceptionRefusal::UnsupportedForm)
}

/// Reads what `resolve` needs for `req` in `dir`.
///
/// # Errors
///
/// `unsupported-form` for a name that is not a valid ref name or a push configuration that
/// cannot be read; a missing ref or upstream is left out of the facts and makes `resolve`
/// refuse.
pub fn read(
    git: &SystemGit,
    invoker: &Invoker,
    dir: &Path,
    reader: &RepoReader,
    req: &Request,
) -> Result<RepoFacts, ExceptionRefusal> {
    let cli = GitCli::new(git, invoker, dir).map_err(|_| ExceptionRefusal::UnsupportedForm)?;
    let head = reader.head().ok().and_then(|h| h.branch);
    let config = |key: ConfigKey| {
        cli.config_get(&key)
            .map_err(|_| ExceptionRefusal::UnsupportedForm)
    };
    let push_default = config(ConfigKey::PushDefault)?;
    let branch_push_remote = match &head {
        Some(h) => config(ConfigKey::BranchPushRemote(refname(h)?))?,
        None => None,
    };
    let push_remote = match branch_push_remote {
        Some(r) => Some(r),
        None => config(ConfigKey::RemotePushDefault)?,
    };
    let mut facts = RepoFacts {
        head: head.clone(),
        push_default,
        remotes: Vec::new(),
        push_remote: push_remote.clone(),
        upstreams: BTreeMap::new(),
        tracking: BTreeMap::new(),
        revs: BTreeMap::new(),
    };
    let upstream_of = |branch: &str| -> Option<Upstream> {
        let name = RefName::new(branch).ok()?;
        let remote = cli
            .config_get(&ConfigKey::BranchRemote(name.clone()))
            .ok()??;
        let merge = cli.config_get(&ConfigKey::BranchMerge(name)).ok()??;
        Some(Upstream { remote, merge })
    };
    let rev = |name: &str| -> Result<Option<String>, ExceptionRefusal> {
        Ok(cli.rev_parse_verify(&refname(name)?).ok().flatten())
    };
    let mut branches: Vec<String> = head.iter().cloned().collect();
    let mut remotes: Vec<String> = vec!["origin".to_owned()];
    remotes.extend(push_remote);
    let mut names: Vec<String> = Vec::new();
    let mut dests: Vec<String> = Vec::new();
    match &req.form {
        Form::Push {
            remote, refspecs, ..
        } => {
            remotes.extend(remote.iter().cloned());
            for spec in refspecs {
                names.extend(spec.src.iter().cloned());
                dests.extend(spec.dst.iter().chain(spec.src.iter()).cloned());
            }
            dests.extend(head.iter().cloned());
            // A push without refspec pushes the current branch (by `push.default`).
            if refspecs.is_empty() {
                names.extend(head.iter().cloned());
            }
        }
        Form::BranchDelete { names: deleted, .. } => {
            for name in deleted {
                if let Some(oid) = rev(&format!("refs/heads/{name}"))? {
                    facts.revs.insert(name.clone(), oid);
                }
            }
        }
        Form::Rebase {
            onto,
            upstream,
            branch,
        } => {
            names.extend(onto.iter().chain(upstream).chain(branch).cloned());
            branches.extend(branch.iter().cloned());
        }
    }
    for branch in &branches {
        if let Some(up) = upstream_of(branch) {
            remotes.push(up.remote.clone());
            if let Some(short) = up.merge.strip_prefix("refs/heads/") {
                dests.push(short.to_owned());
            }
            facts.upstreams.insert(branch.clone(), up);
        }
    }
    for name in &names {
        if let Some(oid) = rev(name)? {
            facts.revs.insert(name.clone(), oid);
        }
    }
    remotes.sort();
    remotes.dedup();
    for remote in remotes {
        let Ok(name) = RefName::new(&remote) else {
            continue;
        };
        if !matches!(reader.remote_url(&name), Ok(Some(_))) {
            continue;
        }
        for dest in &dests {
            let short = dest.strip_prefix("refs/heads/").unwrap_or(dest);
            if short == "HEAD" {
                continue;
            }
            let tracking = format!("refs/remotes/{remote}/{short}");
            if let Some(oid) = RefName::new(&tracking)
                .ok()
                .and_then(|r| reader.resolve_ref(&r).ok().flatten())
            {
                facts
                    .tracking
                    .insert((remote.clone(), format!("refs/heads/{short}")), oid);
            }
        }
        facts.remotes.push(remote);
    }
    Ok(facts)
}

/// The base of a presented rebase: the oid of `upstream` (or of the configured upstream of the
/// branch) and the full ref of the branch rebased, read in the hook's worktree.
pub fn rebase_target(
    reader: &RepoReader,
    upstream: Option<&str>,
    branch: Option<&str>,
) -> Option<(String, String)> {
    let target = match branch {
        Some(b) => b.to_owned(),
        None => reader.head().ok()?.branch?,
    };
    let upstream = match upstream {
        Some(u) => u.to_owned(),
        None => reader.branch_upstream(&target)?,
    };
    let full = |name: &str| RefName::new(name).ok();
    let base = full(&upstream)
        .and_then(|r| reader.resolve_ref(&r).ok().flatten())
        .or_else(|| {
            full(&format!("refs/heads/{upstream}"))
                .and_then(|r| reader.resolve_ref(&r).ok().flatten())
        })
        .or_else(|| {
            full(&format!("refs/remotes/{upstream}"))
                .and_then(|r| reader.resolve_ref(&r).ok().flatten())
        })?;
    let refname = if target.starts_with("refs/") {
        target
    } else {
        format!("refs/heads/{target}")
    };
    Some((refname, base))
}
