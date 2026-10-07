//! The second line against `git commit --no-verify` (DS-US-GRD-018 D6, § 5.3), in the daemon.
//!
//! The hook client of `reference-transaction` `prepared` sends the facts of one new commit; the
//! daemon decides whether to evaluate it. Two things make it skip, both about the `git` process
//! that ran the hook (the nearest `git` ancestor of the client, found with the requester's walk
//! and identified by `(pid, start)`):
//!
//! - **One decision per operation** (ADR-GRD-003 § 6): that same process already had its
//!   authorship decision at `commit-msg`, so the hooks ran and decided. [`Decided`] remembers it
//!   in memory, bounded; an entry that is gone (evicted, daemon restarted) only means the commit
//!   is evaluated again.
//! - **The inverse list**: its subcommand is certainly `rebase`, `cherry-pick`, `revert` or `am`
//!   (D9). The command line is read only to classify it, never stored, logged or sent.
//!
//! Anything that cannot be proven (no `git` ancestor, a start time that changed while reading,
//! an unreadable command line) evaluates the commit: never skipped in silence.

use std::collections::VecDeque;
use std::sync::Mutex;

use gitraptor_api::guard::AuthorshipFacts;
use gitraptor_policy::authorship::subcommand::second_line_evaluates;

use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::requester::{file_name, parent};

/// Longest walk from the hook client to its `git` (client → dispatcher → `git`, with room).
const MAX_DEPTH: usize = 16;

/// `git` processes remembered (FIFO).
pub const REMEMBERED: usize = 256;

/// A `git` process: the identity of one operation.
pub type GitProcess = (u32, u64);

/// The nearest `git` ancestor of the hook client, verified on the way.
pub fn nearest_git(peer: AcceptedPeer, checks: &Checks<'_>) -> Option<GitProcess> {
    let mut current = checks.procs.read(peer.pid).ok()?;
    if current.start_us != peer.start_us || current.uid != checks.uid {
        return None;
    }
    for _ in 0..MAX_DEPTH {
        current = parent(&current, checks)?;
        let name = file_name(&current);
        if name == "git" || name == "git.exe" {
            return Some((current.pid, current.start_us));
        }
    }
    None
}

/// Whether the second line evaluates a commit made under `git`: its command line is read, and
/// counts only if the process is still the same one afterwards (a reused pid reads someone
/// else's).
pub fn evaluates(git: Option<GitProcess>, checks: &Checks<'_>) -> bool {
    let Some((pid, start)) = git else {
        return true;
    };
    let argv = checks.procs.args(pid);
    let same = checks.procs.read(pid).is_ok_and(|p| p.start_us == start);
    second_line_evaluates(argv.as_deref().filter(|_| same))
}

/// The `git` processes whose commit `commit-msg` already let through, with the facts it saw: the
/// second line skips only the same process with the same facts, so a second `commit-msg` run
/// by hand (an editor that calls the hook with another message) proves nothing about the
/// commit that lands.
#[derive(Debug, Default)]
pub struct Decided(Mutex<VecDeque<(GitProcess, AuthorshipFacts)>>);

impl Decided {
    pub fn record(&self, git: GitProcess, facts: &AuthorshipFacts) {
        let Ok(mut seen) = self.0.lock() else {
            return;
        };
        let entry = (git, facts.clone());
        if seen.contains(&entry) {
            return;
        }
        if seen.len() >= REMEMBERED {
            seen.pop_front();
        }
        seen.push_back(entry);
    }

    pub fn contains(&self, git: GitProcess, facts: &AuthorshipFacts) -> bool {
        self.0
            .lock()
            .is_ok_and(|seen| seen.iter().any(|(g, f)| *g == git && f == facts))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;
    use crate::channel::AgentMatcher;
    use crate::channel::authz::TERMINAL_PROOF;
    use crate::channel::peer::{ProcError, ProcInfo, ProcSource};

    /// A process tree; `args` are the readable command lines, `restart` pids that come back
    /// with another start time once their command line was read.
    #[derive(Default)]
    struct Tree {
        procs: HashMap<u32, ProcInfo>,
        args: HashMap<u32, Vec<OsString>>,
        restart: std::cell::Cell<Option<u32>>,
    }

    impl Tree {
        fn add(&mut self, pid: u32, ppid: u32, exe: &str, start: u64) {
            self.procs.insert(
                pid,
                ProcInfo {
                    pid,
                    ppid,
                    uid: 501,
                    start_us: start,
                    exe: Some(PathBuf::from(exe)),
                    controlling_terminal: true,
                    session: 1,
                    pgid: pid,
                },
            );
        }
    }

    impl ProcSource for Tree {
        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            let mut p = self.procs.get(&pid).cloned().ok_or(ProcError::Gone)?;
            if self.restart.get() == Some(pid) {
                p.start_us += 1;
            }
            Ok(p)
        }
        fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
            Some(false)
        }
        fn args(&self, pid: u32) -> Option<Vec<OsString>> {
            let args = self.args.get(&pid).cloned();
            if self.procs.get(&pid).is_some_and(|p| p.exe.is_none()) {
                self.restart.set(Some(pid));
            }
            args
        }
    }

    fn checks<'a>(t: &'a Tree, matcher: &'a AgentMatcher) -> Checks<'a> {
        Checks {
            uid: 501,
            procs: t,
            matcher,
            daemon: None,
            marks: None,
            terminal_proof: TERMINAL_PROOF,
        }
    }

    fn peer(pid: u32, start: u64) -> AcceptedPeer {
        AcceptedPeer {
            pid,
            start_us: start,
            accepted_us: u64::MAX,
        }
    }

    /// agent(10) → sh(20) → git(30) → sh(40, dispatcher) → raptor(50, hook client).
    fn tree(argv: Option<&str>) -> Tree {
        let mut t = Tree::default();
        t.add(10, 1, "/bin/claude", 1);
        t.add(20, 10, "/bin/sh", 2);
        t.add(30, 20, "/usr/bin/git", 3);
        t.add(40, 30, "/bin/sh", 4);
        t.add(50, 40, "/usr/local/bin/raptor", 5);
        if let Some(argv) = argv {
            t.args
                .insert(30, argv.split(' ').map(OsString::from).collect());
        }
        t
    }

    #[test]
    fn the_nearest_git_is_found_by_its_identity() {
        let t = tree(None);
        let m = AgentMatcher::default();
        assert_eq!(nearest_git(peer(50, 5), &checks(&t, &m)), Some((30, 3)));
        // A client that is not the process that connected: nothing.
        assert_eq!(nearest_git(peer(50, 6), &checks(&t, &m)), None);
        // A parent younger than its child (reused pid) breaks the walk.
        let mut t = tree(None);
        t.add(30, 20, "/usr/bin/git", 9);
        assert_eq!(nearest_git(peer(50, 5), &checks(&t, &m)), None);
    }

    #[test]
    fn only_a_certain_rebase_is_skipped() {
        let m = AgentMatcher::default();
        let t = tree(Some("git rebase main"));
        assert!(!evaluates(Some((30, 3)), &checks(&t, &m)));
        let t = tree(Some("git commit --no-verify -m x"));
        assert!(evaluates(Some((30, 3)), &checks(&t, &m)));
    }

    #[test]
    fn an_unreadable_or_unproven_command_line_is_evaluated() {
        let m = AgentMatcher::default();
        // No command line (Windows, permissions, gone).
        let t = tree(None);
        assert!(evaluates(Some((30, 3)), &checks(&t, &m)));
        // No `git` ancestor.
        assert!(evaluates(None, &checks(&t, &m)));
        // A start time that is not the one walked.
        let t = tree(Some("git rebase main"));
        assert!(evaluates(Some((30, 4)), &checks(&t, &m)));
        // The pid is reused while its command line is read.
        let mut t = tree(Some("git rebase main"));
        t.procs.get_mut(&30).unwrap().exe = None;
        assert!(evaluates(Some((30, 3)), &checks(&t, &m)));
    }

    #[test]
    fn decided_is_bounded_and_keyed_by_the_start_time_and_the_facts() {
        use gitraptor_api::AgentKind;
        let signed = AuthorshipFacts {
            coauthors: vec![Some(AgentKind::ClaudeCode)],
            trailer_table: 1,
            unreadable: false,
        };
        let unsigned = AuthorshipFacts {
            coauthors: vec![],
            ..signed.clone()
        };
        let d = Decided::default();
        d.record((30, 3), &signed);
        assert!(d.contains((30, 3), &signed));
        assert!(!d.contains((30, 4), &signed));
        assert!(!d.contains((30, 3), &unsigned));
        for pid in 0..REMEMBERED as u32 {
            d.record((1000 + pid, 1), &signed);
        }
        assert!(!d.contains((30, 3), &signed));
        assert!(d.contains((1000, 1), &signed));
    }
}
