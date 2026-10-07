//! The hook client (`raptor hook`, ADR-GRD-001 § 2, ADR-GRD-002 § 4, ADR-GRD-003 § 4): reads
//! and normalizes the hook input, checks that the transaction belongs to the repo of the
//! dispatcher, asks an authenticated daemon and, without one, decides alone in degraded mode.
//!
//! Every path and identity comes from the dispatcher's constants. The environment brings only
//! `GIT_DIR` (to read the transaction's own `HEAD`) and nothing else is derived from it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    AuthorshipFacts, Cause, CommitStage, Decision, Effect, EvaluateParams, Hook, Level, Operation,
    OrphanHead, PushUpdate, Reason, RefUpdate, RefValue, Rule,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_policy::guard::fastpath;
use gitraptor_policy::guard::input::{self, MAX_LINE_BYTES};

use super::constants::TEMPLATE_VERSION;
use super::evaluate;
use super::journal::{Snapshot, snapshot_path};
use crate::client::{Client, ClientError};

/// The constants a dispatcher passes, in this order, after `raptor hook`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArgs {
    pub template: u32,
    pub hook: Hook,
    pub repo: String,
    pub common: PathBuf,
    pub channel: PathBuf,
    pub instance: String,
    pub state: PathBuf,
    pub prior: String,
    /// What Git passed to the hook.
    pub git_args: Vec<OsString>,
}

impl HookArgs {
    /// Parses `<template> <hook> <repo> <common> <channel> <instance> <state> <prior> -- <args>`.
    pub fn parse(args: &[OsString]) -> Option<Self> {
        let text = |i: usize| args.get(i)?.to_str().map(str::to_owned);
        let template: u32 = text(0)?.parse().ok()?;
        // The current template and the previous one (ADR-GRD-001 § 8).
        if template == 0 || template > TEMPLATE_VERSION {
            return None;
        }
        if args.get(8).and_then(|a| a.to_str()) != Some("--") {
            return None;
        }
        let path = |i: usize| -> Option<PathBuf> {
            let p = PathBuf::from(args.get(i)?);
            p.is_absolute().then_some(p)
        };
        Some(Self {
            template,
            hook: Hook::from_git_name(&text(1)?)?,
            repo: text(2)?,
            common: path(3)?,
            channel: path(4)?,
            instance: text(5)?,
            state: path(6)?,
            prior: text(7)?,
            git_args: args[9..].to_vec(),
        })
    }
}

/// The process facts the hook client may use.
#[derive(Debug, Clone, Default)]
pub struct HookEnv {
    /// `GIT_DIR`, when Git set it (linked worktrees, `--git-dir`).
    pub git_dir: Option<PathBuf>,
    pub cwd: PathBuf,
}

/// Why the client decided alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degraded {
    DaemonUnreachable,
    InstanceMismatch,
}

/// What the hook does.
#[derive(Debug, Clone, PartialEq)]
pub struct HookOutcome {
    /// Exit 0 when `allow`; any other applied effect exits 1.
    pub decision: Option<Decision>,
    pub degraded: Option<Degraded>,
}

impl HookOutcome {
    fn allow() -> Self {
        Self {
            decision: None,
            degraded: None,
        }
    }

    fn deny(rule: Rule) -> Self {
        Self {
            decision: Some(evaluate::system_deny(rule)),
            degraded: None,
        }
    }

    pub fn allowed(&self) -> bool {
        self.decision
            .as_ref()
            .is_none_or(|d| d.applied_effect == Effect::Allow)
    }
}

/// Runs the hook on its whole standard input.
pub fn run(args: &HookArgs, env: &HookEnv, input: &[u8]) -> HookOutcome {
    let git_arg = |i: usize| args.git_args.get(i).and_then(|a| a.to_str());
    let op = match args.hook {
        Hook::ReferenceTransaction => {
            // Only `prepared` is evaluated (E-02-2).
            if git_arg(0) != Some("prepared") {
                return HookOutcome::allow();
            }
            match ref_transaction(args, env, input) {
                Ok(Some(op)) => op,
                Ok(None) => return HookOutcome::allow(),
                Err(rule) => return HookOutcome::deny(rule),
            }
        }
        Hook::PrePush => match push(git_arg(0), git_arg(1), input) {
            Ok(Some(op)) => op,
            Ok(None) => return HookOutcome::allow(),
            Err(rule) => return HookOutcome::deny(rule),
        },
        Hook::PreRebase => Operation::Rebase {
            upstream: git_arg(0).map(Untrusted::new),
            branch: git_arg(1).map(Untrusted::new),
        },
        Hook::PreCommit | Hook::CommitMsg => return commit(args, env),
    };
    // M-02, SEC-GRD-19: the transaction must be in the repo of the dispatcher.
    if !same_repo(&args.common, env) {
        return HookOutcome::deny(Rule::RepoMismatch);
    }
    decide(args, op)
}

/// The canonical common directory of the transaction: `GIT_DIR` or the repo of the cwd.
pub(crate) fn transaction_git_dir(env: &HookEnv) -> Option<PathBuf> {
    if let Some(dir) = &env.git_dir {
        let dir = if dir.is_absolute() {
            dir.clone()
        } else {
            env.cwd.join(dir)
        };
        return dir.canonicalize().ok().map(fastpath::simplified);
    }
    // Git runs hooks at the root of the worktree, or in `$GIT_DIR` for a bare repo.
    let dot_git = env.cwd.join(".git");
    match std::fs::symlink_metadata(&dot_git) {
        Ok(m) if m.is_dir() => dot_git.canonicalize().ok().map(fastpath::simplified),
        Ok(m) if m.is_file() => {
            let text = std::fs::read_to_string(&dot_git).ok()?;
            let target = text.trim_end().strip_prefix("gitdir: ")?;
            let target = Path::new(target);
            let target = if target.is_absolute() {
                target.to_path_buf()
            } else {
                env.cwd.join(target)
            };
            target.canonicalize().ok().map(fastpath::simplified)
        }
        _ => env
            .cwd
            .join("HEAD")
            .is_file()
            .then(|| env.cwd.canonicalize().ok().map(fastpath::simplified))
            .flatten(),
    }
}

pub(crate) fn common_of(git_dir: &Path) -> Option<PathBuf> {
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => {
            let c = Path::new(text.trim_end());
            let c = if c.is_absolute() {
                c.to_path_buf()
            } else {
                git_dir.join(c)
            };
            c.canonicalize().ok().map(fastpath::simplified)
        }
        Err(_) => Some(git_dir.to_path_buf()),
    }
}

fn same_repo(common: &Path, env: &HookEnv) -> bool {
    let Some(git_dir) = transaction_git_dir(env) else {
        return false;
    };
    let ours = fastpath::simplified(
        common
            .canonicalize()
            .unwrap_or_else(|_| common.to_path_buf()),
    );
    common_of(&git_dir).is_some_and(|c| c == ours)
}

/// The branch a `HEAD` line names, read from the right `HEAD` file: the line's own form decides
/// which worktree, and a plain `HEAD` is the transaction's `GIT_DIR`, never the cwd's guess of
/// another repo (E-02-3).
fn resolve_head(name: &str, common: &Path, env: &HookEnv) -> Option<Option<String>> {
    let dir = if name == "HEAD" {
        transaction_git_dir(env)?
    } else if name == "main-worktree/HEAD" {
        common.to_path_buf()
    } else {
        let id = name.strip_prefix("worktrees/")?.strip_suffix("/HEAD")?;
        common.join("worktrees").join(id)
    };
    let text = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    match text.trim_end().strip_prefix("ref: ") {
        // reftable keeps the real `HEAD` in its tables; the file only says so.
        Some("refs/heads/.invalid") => {
            let reader = evaluate::open(&dir)?;
            let head = gitraptor_git::RefName::new("HEAD").ok()?;
            Some(reader.symbolic_target(&head).ok()?)
        }
        target => Some(target.map(str::to_owned)),
    }
}

/// Normalizes a `reference-transaction` (ADR-GRD-002 § 4). `Ok(None)` when nothing governed
/// is left; `Err` fails closed.
fn ref_transaction(
    args: &HookArgs,
    env: &HookEnv,
    input: &[u8],
) -> Result<Option<Operation>, Rule> {
    let mut updates = Vec::new();
    for line in input.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let raw = input::parse_ref_update(line).map_err(|_| Rule::InputRejected)?;
        let mut refname = raw.refname;
        if fastpath::is_head(&refname) {
            // `ref:` values move `HEAD` itself; a created `HEAD` is a new worktree's.
            if matches!(raw.old, RefValue::Symbolic(_))
                || matches!(raw.new, RefValue::Symbolic(_))
                || (raw.old.is_zero() && !raw.new.is_zero())
            {
                continue;
            }
            match resolve_head(&refname, &args.common, env) {
                Some(Some(target)) => refname = target,
                // Detached: not governed.
                Some(None) => continue,
                // Unreadable: a deletion may be of a base branch (fail-closed); anything else
                // moves `HEAD` along with a branch line of its own.
                None if raw.new.is_zero() => return Err(Rule::InternalError),
                None => continue,
            }
        }
        if !fastpath::is_governed(&refname) {
            continue;
        }
        if fastpath::is_prune(as_fast(&raw.old), as_fast(&raw.new), &refname, &args.common) {
            continue;
        }
        updates.push(RefUpdate {
            refname,
            old: raw.old,
            new: raw.new,
        });
    }
    if updates.is_empty() {
        return Ok(None);
    }
    let orphan_head = updates
        .iter()
        .any(|u| u.new.is_zero())
        .then(|| orphan_head(env))
        .flatten();
    Ok(Some(Operation::RefTransaction {
        updates,
        orphan_head,
    }))
}

fn as_fast(v: &RefValue) -> fastpath::Value<'_> {
    match v {
        RefValue::Zero => fastpath::Value::Zero,
        RefValue::Oid(o) => fastpath::Value::Oid(o),
        RefValue::Symbolic(_) => fastpath::Value::Symbolic,
    }
}

/// `branch -M x <base>` with the files backend deletes `x` first, so `HEAD` names a branch that
/// is gone: its last oid is in the `HEAD` reflog (`renombrado-sobre-base`, E-02-7).
fn orphan_head(env: &HookEnv) -> Option<OrphanHead> {
    let git_dir = transaction_git_dir(env)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let target = head.trim_end().strip_prefix("ref: ")?.to_owned();
    let branch = target.strip_prefix("refs/heads/")?.to_owned();
    let common = common_of(&git_dir)?;
    let reader = evaluate::open(&common)?;
    let name = gitraptor_git::RefName::new(&target).ok()?;
    if reader.resolve_ref(&name).ok()?.is_some() {
        return None;
    }
    let log = std::fs::read_to_string(git_dir.join("logs").join("HEAD")).ok()?;
    let oid = log.lines().last()?.split(' ').nth(1)?.to_owned();
    let valid = matches!(oid.len(), 40 | 64)
        && oid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && oid.bytes().any(|b| b != b'0');
    valid.then_some(OrphanHead { branch, oid })
}

/// Normalizes a `pre-push`. `Ok(None)` when no governed remote ref is updated.
fn push(remote: Option<&str>, url: Option<&str>, input: &[u8]) -> Result<Option<Operation>, Rule> {
    let mut updates = Vec::new();
    for line in input.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let raw = input::parse_push_update(line).map_err(|_| Rule::InputRejected)?;
        if !fastpath::is_governed(&raw.remote_ref) {
            continue;
        }
        updates.push(PushUpdate {
            local_ref: raw.local_ref,
            local: raw.local,
            remote_ref: raw.remote_ref,
            remote: raw.remote,
        });
    }
    if updates.is_empty() {
        return Ok(None);
    }
    // The remote's name, or its URL without userinfo (M-06).
    let shown = match (remote, url) {
        (Some(name), Some(url)) if name == url => gitraptor_git::redact::remote_url(url),
        (Some(name), _) => name.to_owned(),
        (None, Some(url)) => gitraptor_git::redact::remote_url(url),
        (None, None) => String::new(),
    };
    Ok(Some(Operation::Push {
        remote: Untrusted::new(shown),
        updates,
    }))
}

/// `pre-commit` and `commit-msg` (US-GRD-018, D6, D11): only the commit authorship policy
/// governs a commit, and only an authentic daemon of this instance that grants
/// `guard.authorship` evaluates it. Without one the commit goes ahead as before: in degraded
/// mode the actor is "unattributed", so no authorship rule applies (D3, BR-EDGE-004).
fn commit(args: &HookArgs, env: &HookEnv) -> HookOutcome {
    if !same_repo(&args.common, env) {
        return HookOutcome::deny(Rule::RepoMismatch);
    }
    let mut client = match connect(args) {
        Ok(client) => client,
        Err(Asked::NotAuthentic) => return HookOutcome::deny(Rule::ChannelNotAuthentic),
        // Degraded: unattributed, nothing to decide. The `reference-transaction` of the same
        // commit already says the layer is degraded.
        Err(_) => return HookOutcome::allow(),
    };
    let granted = client.hello().capabilities.as_ref().is_some_and(|served| {
        served
            .iter()
            .any(|c| c == methods::CAP_GUARD_AUTHORSHIP.name)
    });
    if !granted {
        return HookOutcome::allow();
    }
    let (stage, facts) = match args.hook {
        Hook::PreCommit => (CommitStage::PreCommit, None),
        _ => (CommitStage::CommitMsg, Some(message_facts(args, env))),
    };
    let params = EvaluateParams {
        repo_id: args.repo.clone(),
        common_dir: args.common.to_string_lossy().into_owned(),
        hook: args.hook,
        operation: Operation::Commit { stage },
        authorship: facts,
    };
    match client.call::<_, Decision>(methods::GUARD_EVALUATE, &params) {
        Ok(decision) => HookOutcome {
            decision: Some(decision),
            degraded: None,
        },
        Err(_) => HookOutcome::deny(Rule::InternalError),
    }
}

/// The facts of the message file `commit-msg` received (D7): a regular file, never a link, at
/// most 64 KiB; anything else is unreadable. Only the facts leave this process.
fn message_facts(args: &HookArgs, env: &HookEnv) -> AuthorshipFacts {
    use gitraptor_policy::authorship::{self, Cleanup, MAX_MESSAGE_BYTES, MessageOptions};
    let Some(path) = args.git_args.first() else {
        return authorship::unreadable();
    };
    let path = env.cwd.join(path);
    let Some(bytes) = read_regular(&path, MAX_MESSAGE_BYTES as u64) else {
        return authorship::unreadable();
    };
    let mut options = MessageOptions::default();
    if let Some(git_dir) = transaction_git_dir(env)
        && let Ok(reader) =
            gitraptor_git::RepoReader::open(&git_dir, &gitraptor_git::ReaderOptions::default())
    {
        let (cleanup, comment) = reader.commit_message_config();
        if let Some(c) = cleanup {
            options.cleanup = Cleanup::from_config(&c);
        }
        // `auto` picks a character absent from the message: `#` unless it is used.
        match comment.as_deref() {
            Some("auto") | None => {}
            Some(c) if !c.is_empty() => options.comment = c.to_owned(),
            Some(_) => {}
        }
    }
    authorship::facts(&bytes, &options)
}

/// A regular file of at most `max` bytes, opened without following a link.
fn read_regular(path: &Path, max: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options.open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// Asks the daemon; without an authentic daemon of this instance, degraded mode.
fn decide(args: &HookArgs, op: Operation) -> HookOutcome {
    let params = EvaluateParams {
        repo_id: args.repo.clone(),
        common_dir: args.common.to_string_lossy().into_owned(),
        hook: args.hook,
        operation: op,
        authorship: None,
    };
    match ask_daemon(args, &params) {
        Asked::Decision(decision) => HookOutcome {
            decision: Some(decision),
            degraded: None,
        },
        Asked::NotAuthentic => HookOutcome::deny(Rule::ChannelNotAuthentic),
        Asked::Failed => HookOutcome::deny(Rule::InternalError),
        Asked::Degraded(cause) => degraded(args, &params.operation, cause),
    }
}

enum Asked {
    Decision(Decision),
    NotAuthentic,
    Failed,
    Degraded(Degraded),
}

fn ask_daemon(args: &HookArgs, params: &EvaluateParams) -> Asked {
    let mut client = match connect(args) {
        Ok(client) => client,
        Err(asked) => return asked,
    };
    match client.call::<_, Decision>(methods::GUARD_EVALUATE, params) {
        Ok(decision) => Asked::Decision(decision),
        Err(_) => Asked::Failed,
    }
}

/// An authentic daemon of this profile instance, or what to do without one.
fn connect(args: &HookArgs) -> Result<Client, Asked> {
    // The server must be the installed binary itself (H-03, SEC-GRD-16), checked before
    // anything is sent. A server whose executable cannot be read at all (replaced on disk
    // after an upgrade) is not trusted either, but decides nothing: degraded mode, stricter.
    let identity = std::cell::Cell::new(Identity::Different);
    let connected =
        Client::connect_runtime(&args.channel, ClientKind::Other, PROTOCOL_VERSION, |pid| {
            identity.set(same_executable(pid));
            identity.get() == Identity::Same
        });
    let client = match connected {
        Ok(client) => client,
        Err(ClientError::NotAuthentic) if identity.get() == Identity::Unknown => {
            return Err(Asked::Degraded(Degraded::DaemonUnreachable));
        }
        Err(ClientError::NotAuthentic) => return Err(Asked::NotAuthentic),
        Err(ClientError::Rpc(_) | ClientError::Protocol(_)) => return Err(Asked::Failed),
        // No daemon, a stale socket, another protocol or no transport (Windows).
        Err(_) => return Err(Asked::Degraded(Degraded::DaemonUnreachable)),
    };
    // …and this profile instance; another one decides nothing here (ADR-GRD-003 § 4).
    if client.hello().instance_id != args.instance {
        return Err(Asked::Degraded(Degraded::InstanceMismatch));
    }
    Ok(client)
}

/// What the client can tell of the channel server's executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Identity {
    Same,
    Different,
    /// Its executable cannot be read (on Linux, a binary replaced on disk is `(deleted)`).
    Unknown,
}

/// The executable of the channel server is the same file as this client's (ADR-GRD-003 § 4,
/// Enmienda 2026-10-05: the identity of the file instead of a signature or fingerprint).
fn same_executable(pid: u32) -> Identity {
    #[cfg(unix)]
    {
        use crate::channel::peer::{ProcSource, SystemProcs};
        let Ok(info) = SystemProcs.read(pid) else {
            return Identity::Unknown;
        };
        let Some(server) = info.exe else {
            return Identity::Unknown;
        };
        let Ok(own) = std::env::current_exe() else {
            return Identity::Unknown;
        };
        let id = |p: &Path| crate::channel::file_id(p);
        match (id(&server), id(&own)) {
            (Some(a), Some(b)) if a == b => Identity::Same,
            (Some(_), Some(_)) => Identity::Different,
            _ => Identity::Unknown,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Identity::Unknown
    }
}

/// Degraded mode (ADR-GRD-003 § 4): the same function, the minimum forced and the base branch
/// union {`main`, main branch, last confirmed base}. Never less strict than the daemon.
fn degraded(args: &HookArgs, op: &Operation, cause: Degraded) -> HookOutcome {
    let Some(reader) = evaluate::open(&args.common) else {
        return HookOutcome::deny(Rule::InternalError);
    };
    let mut bases = evaluate::default_bases(&reader);
    if let Some(snapshot) = read_snapshot(&args.state, &args.repo)
        && snapshot.common_dir == args.common.to_string_lossy()
    {
        for b in snapshot
            .confirmed_base
            .into_iter()
            .chain(snapshot.protected_bases)
        {
            if !bases.contains(&b) {
                bases.push(b);
            }
        }
    }
    let mut eval = evaluate::evaluate(&reader, &args.common, op, bases);
    if eval.effect != Effect::Allow {
        eval.reasons.push(Reason {
            rule: Rule::Degraded,
            level: Level::System,
            cause: Some(match cause {
                Degraded::DaemonUnreachable => Cause::DaemonUnreachable,
                Degraded::InstanceMismatch => Cause::InstanceMismatch,
            }),
            params: Vec::new(),
        });
    }
    HookOutcome {
        decision: Some(evaluate::decision(eval)),
        degraded: Some(cause),
    }
}

/// The read-only snapshot the daemon exported (never the SQLite store).
fn read_snapshot(state: &Path, repo: &str) -> Option<Snapshot> {
    if !repo.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    let path = snapshot_path(state, repo);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Longest input line accepted (re-exported for the CLI).
pub const MAX_INPUT_LINE: usize = MAX_LINE_BYTES;

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    #[test]
    fn arguments_are_the_dispatcher_constants() {
        // Absolute on every OS (`C:\…` on Windows).
        let abs = |p: &str| std::env::temp_dir().join(p).to_string_lossy().into_owned();
        let (r, c, st) = (abs("r/.git"), abs("run"), abs("state"));
        let a = HookArgs::parse(&argv(&[
            "1",
            "pre-push",
            "abc",
            &r,
            &c,
            "inst",
            &st,
            "",
            "--",
            "origin",
            "/remote.git",
        ]))
        .unwrap();
        assert_eq!(a.hook, Hook::PrePush);
        assert_eq!(a.git_args, argv(&["origin", "/remote.git"]));
        for bad in [
            &["9", "pre-push", "a", &r, &c, "i", &st, "", "--"][..],
            &["1", "post-commit", "a", &r, &c, "i", &st, "", "--"],
            &["1", "pre-push", "a", "rel", &c, "i", &st, "", "--"],
            &["1", "pre-push", "a", &r, &c, "i", &st, ""],
        ] {
            assert!(HookArgs::parse(&argv(bad)).is_none(), "{bad:?}");
        }
    }
}
