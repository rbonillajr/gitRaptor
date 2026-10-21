//! The facts the permissions per operation need (US-GRD-007, DS-US-GRD-007 D6, D10, D12, D14):
//! the chain of `git` processes above a hook client, what each line of a transaction is in the
//! catalog of operations, and the team's permissions a hook client applies on its own.
//!
//! The command line of a `git` process is read only to classify it: never stored, logged or
//! sent. Anything that cannot be proved is left to the pure function, which denies it as
//! `unverifiable` when a permission other than `allow` needs it.

use std::path::Path;

use gitraptor_api::guard::{GuardLogResult, NotPreventable, Operation, RefValue, Rule};
use gitraptor_git::{Hide, PathLimits, RefName, RepoReader};
use gitraptor_policy::guard::git_command::{self, GitCommand};
use gitraptor_policy::guard::permissions::{self as policy, Chain, Classified, Fresh, LineKind};
use gitraptor_policy::guard::refs;
use gitraptor_policy::settings::model::Operation as Op;
use gitraptor_policy::team::{EffectivePermissions, Permission};

use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::requester::{file_name, parent};

/// Consecutive `git` processes read above a hook client.
pub const MAX_CHAIN: usize = 4;

/// Longest walk from the hook client to its first `git` (client → dispatcher → `git`, with room).
const MAX_DEPTH: usize = 16;

fn is_git(info: &crate::channel::peer::ProcInfo) -> bool {
    let name = file_name(info);
    name == "git" || name == "git.exe"
}

/// The consecutive `git` ancestors of the hook client `peer`, nearest first (at most
/// [`MAX_CHAIN`]). A command line counts only if the process is still the same one after it
/// was read; a chain longer than the bound ends with [`GitCommand::Unknown`]. An empty chain
/// (no `git` found, the client is not the process that connected) proves nothing.
pub fn git_chain(peer: AcceptedPeer, checks: &Checks<'_>) -> Chain {
    let Ok(mut current) = checks.procs.read(peer.pid) else {
        return Chain::default();
    };
    if current.start_us != peer.start_us || current.uid != checks.uid {
        return Chain::default();
    }
    let mut commands = Vec::new();
    for _ in 0..MAX_DEPTH + MAX_CHAIN {
        let Some(next) = parent(&current, checks) else {
            break;
        };
        current = next;
        if !is_git(&current) {
            if commands.is_empty() {
                continue;
            }
            break;
        }
        if commands.len() == MAX_CHAIN {
            // Past the bound: what the outer processes are cannot be said.
            commands.push(GitCommand::Unknown);
            break;
        }
        let argv = checks.procs.args(current.pid);
        let same = checks
            .procs
            .read(current.pid)
            .is_ok_and(|p| p.start_us == current.start_us);
        commands.push(git_command::parse(argv.as_deref().filter(|_| same)));
    }
    Chain::new(commands)
}

/// [`git_chain`] of the running process itself: the hook client deciding without the daemon
/// (degraded mode, or a daemon without `guard.permissions`).
pub fn own_chain() -> Chain {
    use crate::channel::authz::{AgentMatcher, ORPHANS_MARKED, TERMINAL_PROOF};
    use crate::channel::peer::{ProcSource, SystemProcs, current_uid};
    let pid = std::process::id();
    let Ok(me) = SystemProcs.read(pid) else {
        return Chain::default();
    };
    let matcher = AgentMatcher::default();
    let checks = Checks {
        uid: current_uid(),
        procs: &SystemProcs,
        matcher: &matcher,
        daemon: None,
        marks: None,
        terminal_proof: TERMINAL_PROOF,
        orphans_marked: ORPHANS_MARKED,
    };
    git_chain(
        AcceptedPeer {
            pid,
            start_us: me.start_us,
            accepted_us: u64::MAX,
        },
        &checks,
    )
}

/// The team's permissions as a hook client applies them on its own (D10, D12): the floor and
/// the worktree of `worktree` (its working directory) only harden. No `allow`, no
/// `disableSafeMinimum` and no personal level count; the minimum is forced. A team level that
/// cannot be read adds nothing to the minimum.
pub fn team_only(reader: &RepoReader, worktree: Option<&Path>) -> EffectivePermissions {
    let own = worktree.and_then(|cwd| super::authorship::worktree_reader(cwd, reader.common_dir()));
    // Nothing confirmed: the floor is read only to harden, its relaxations are not read.
    match super::layers::LOADER.load(own.as_ref().unwrap_or(reader), None) {
        Ok(team) => team.permissions,
        Err(_) => gitraptor_policy::team::combine(None, false, &[]),
    }
}

/// Whether `rule` is a permission per operation.
pub fn is_permission(rule: Rule) -> bool {
    Op::ALL.iter().any(|op| policy::rule(*op) == rule)
}

/// A `guard.log` page for a connection without `guard.permissions` (D10): the permission reasons
/// are left out, and an entry whose reasons were all permission rules is not listed. An entry
/// that never had reasons (a protection change, for one) stays. The summary still counts them.
pub fn legacy_log(log: &mut GuardLogResult) {
    log.entries.retain_mut(|entry| {
        let had_reasons = !entry.reasons.is_empty();
        entry.reasons.retain(|r| !is_permission(r.rule));
        !had_reasons || !entry.reasons.is_empty()
    });
}

/// A transaction that creates or moves a branch: what its permissions classify by the `git`
/// processes above the hook (a deletion is `branch-delete` whatever runs it).
pub fn moves_a_branch(op: &Operation) -> bool {
    matches!(op, Operation::RefTransaction { updates, .. }
        if updates.iter().any(|u| u.refname.starts_with("refs/heads/") && !u.new.is_zero()))
}

/// The limits a connection without `guard.permissions` can read (D10): the reach of the
/// permissions is left out.
pub fn legacy_not_preventable(limits: &mut Vec<NotPreventable>) {
    limits.retain(|n| *n != NotPreventable::PermissionReach);
}

/// The operations a strict permission could need a fact for.
fn strict(p: &EffectivePermissions, op: Op) -> bool {
    p.permission(op) != Permission::Allow
}

/// The chain proves nothing about some process (an empty one included).
fn uncertain(chain: &Chain) -> bool {
    chain.commands().is_empty() || chain.commands().contains(&GitCommand::Unknown)
}

/// The command certainly creates the commit itself: the first look at its commits is enough
/// to deny (D14).
fn creates_commit(chain: &Chain) -> bool {
    chain.commands().iter().any(|c| {
        matches!(
            c.subcommand(),
            Some("commit" | "cherry-pick" | "revert" | "am")
        )
    })
}

/// Whether a classification denies anything under `p`.
fn denies(c: &Classified, p: &EffectivePermissions) -> bool {
    c.unverifiable || c.operations.iter().any(|op| strict(p, *op))
}

/// What each line of a `reference-transaction` is in the catalog (D6), aligned with its updates.
/// The chain of `git` processes is asked for only when `commit`, `merge`, `rebase` or
/// `worktree-add` is not `allow`, and the fresh commits only when `commit` or `merge` is not, or
/// when an unproved chain could hide a rebase or a merge (D14). Before a denial the fresh
/// commits are confirmed hiding every other branch, unless the command creates the commit.
pub fn classified(
    reader: &RepoReader,
    op: &Operation,
    p: &EffectivePermissions,
    chain: impl FnOnce() -> Chain,
) -> Vec<Classified> {
    let Operation::RefTransaction { updates, .. } = op else {
        return Vec::new();
    };
    let relevant = [Op::Commit, Op::Rebase, Op::Merge, Op::WorktreeAdd]
        .into_iter()
        .any(|op| strict(p, op));
    let refs_moved: Vec<&str> = updates.iter().map(|u| u.refname.as_str()).collect();
    let mut chain = Some(chain);
    let mut read: Option<Chain> = None;
    let mut out = Vec::with_capacity(updates.len());
    for u in updates {
        if !u.refname.starts_with("refs/heads/") || !refs::is_governed(&u.refname) {
            out.push(Classified::default());
            continue;
        }
        let RefValue::Oid(new) = &u.new else {
            out.push(match u.new {
                RefValue::Zero => policy::classify(LineKind::Delete, &Chain::default(), None, p),
                // A symbolic value follows another ref: nothing to classify by commits.
                _ => Classified::default(),
            });
            continue;
        };
        if !relevant {
            out.push(Classified::default());
            continue;
        }
        // Zero is also what Git sends when the writer gave no expected old value
        // (`update-ref <ref> <new>`): the value the ref still has in `prepared`.
        let old = match &u.old {
            RefValue::Oid(old) => Some(old.clone()),
            RefValue::Zero => RefName::new(&u.refname)
                .ok()
                .and_then(|name| reader.resolve_ref(&name).ok().flatten()),
            RefValue::Symbolic(_) => None,
        };
        let line = if old.is_some() {
            LineKind::Update
        } else {
            LineKind::Create
        };
        let chain = match (&read, chain.take()) {
            (Some(c), _) => c.clone(),
            (None, Some(f)) => {
                let c = f();
                read = Some(c.clone());
                c
            }
            (None, None) => Chain::default(),
        };
        let wants_fresh = strict(p, Op::Commit)
            || strict(p, Op::Merge)
            || (uncertain(&chain) && strict(p, Op::Rebase));
        if !wants_fresh {
            out.push(policy::classify(line, &chain, None, p));
            continue;
        }
        let count = |hide: Hide| -> Fresh {
            match reader.fresh_commits(
                old.as_deref(),
                new,
                &refs_moved,
                hide,
                &PathLimits::default(),
            ) {
                Ok(n) => Fresh {
                    commits: n.commits,
                    merges: n.merges,
                    unverifiable: n.unverifiable,
                },
                Err(_) => Fresh {
                    unverifiable: true,
                    ..Fresh::default()
                },
            }
        };
        // A cheap first look hiding only the old value: a superset of the fresh commits, final
        // when it denies nothing.
        let first = count(Hide::OldOnly);
        let classified = policy::classify(line, &chain, Some(&first), p);
        if !denies(&classified, p) || (creates_commit(&chain) && !first.unverifiable) {
            out.push(classified);
            continue;
        }
        let exact = count(Hide::OtherBranches);
        out.push(policy::classify(line, &chain, Some(&exact), p));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;
    use crate::channel::AgentMatcher;
    use crate::channel::authz::{ORPHANS_MARKED, TERMINAL_PROOF};
    use crate::channel::peer::{ProcError, ProcInfo, ProcSource};

    #[derive(Default)]
    struct Tree {
        procs: HashMap<u32, ProcInfo>,
        args: HashMap<u32, Vec<OsString>>,
    }

    impl Tree {
        fn add(&mut self, pid: u32, ppid: u32, exe: &str, argv: Option<&str>) {
            self.procs.insert(
                pid,
                ProcInfo {
                    pid,
                    ppid,
                    uid: 501,
                    start_us: u64::from(pid),
                    exe: Some(PathBuf::from(exe)),
                    controlling_terminal: true,
                    desktop_session: None,
                    session: 1,
                    pgid: pid,
                },
            );
            if let Some(argv) = argv {
                self.args
                    .insert(pid, argv.split(' ').map(OsString::from).collect());
            }
        }
    }

    impl ProcSource for Tree {
        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            self.procs.get(&pid).cloned().ok_or(ProcError::Gone)
        }
        fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
            Some(false)
        }
        fn args(&self, pid: u32) -> Option<Vec<OsString>> {
            self.args.get(&pid).cloned()
        }
    }

    fn chain_of(t: &Tree, client: u32) -> Chain {
        let m = AgentMatcher::default();
        let checks = Checks {
            uid: 501,
            procs: t,
            matcher: &m,
            daemon: None,
            marks: None,
            terminal_proof: TERMINAL_PROOF,
            orphans_marked: ORPHANS_MARKED,
        };
        git_chain(
            AcceptedPeer {
                pid: client,
                start_us: u64::from(client),
                accepted_us: u64::MAX,
            },
            &checks,
        )
    }

    #[test]
    fn the_chain_is_the_consecutive_git_ancestors_nearest_first() {
        // sh(2) → git pull(3) → git merge(4) → sh(5, dispatcher) → raptor(6).
        let mut t = Tree::default();
        t.add(2, 1, "/bin/sh", None);
        t.add(3, 2, "/usr/bin/git", Some("git pull origin main"));
        t.add(4, 3, "/usr/bin/git", Some("git merge FETCH_HEAD"));
        t.add(5, 4, "/bin/sh", None);
        t.add(6, 5, "/usr/local/bin/raptor", None);
        let chain = chain_of(&t, 6);
        assert_eq!(
            chain.commands(),
            [
                GitCommand::Certain("merge", None),
                GitCommand::Certain("pull", None)
            ]
        );
    }

    #[test]
    fn an_unreadable_or_too_long_chain_is_uncertain() {
        let mut t = Tree::default();
        t.add(2, 1, "/usr/bin/git", None);
        t.add(3, 2, "/bin/sh", None);
        t.add(4, 3, "/usr/local/bin/raptor", None);
        assert_eq!(chain_of(&t, 4).commands(), [GitCommand::Unknown]);
        // No `git` at all.
        let mut t = Tree::default();
        t.add(3, 1, "/bin/sh", None);
        t.add(4, 3, "/usr/local/bin/raptor", None);
        assert!(chain_of(&t, 4).commands().is_empty());
        // Five `git` in a row: the fifth is past the bound.
        let mut t = Tree::default();
        for pid in 2..=6 {
            t.add(pid, pid - 1, "/usr/bin/git", Some("git commit"));
        }
        t.add(7, 6, "/usr/local/bin/raptor", None);
        let chain = chain_of(&t, 7);
        assert_eq!(chain.commands().len(), MAX_CHAIN + 1);
        assert_eq!(chain.commands().last(), Some(&GitCommand::Unknown));
    }

    #[test]
    fn only_permission_reasons_are_hidden_from_a_legacy_log() {
        assert!(is_permission(Rule::PermissionMerge));
        assert!(!is_permission(Rule::MinimumForcePush));
    }
}
