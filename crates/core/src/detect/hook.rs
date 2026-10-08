//! S4 (ADR-GRP-012, DS-US-GRP-007 § 7): the signals Guardrails hooks leave.
//!
//! The `reference-transaction` hook runs inside the `git` that moves the refs, so its ancestry
//! has no race: when `guard.evaluate` lets a transaction through, the connection thread resolves
//! the hook client's requester (the requester's walk, unchanged) and, if a detected Claude Code
//! session is its direct ancestor, leaves one claim per branch it moves to a new oid. The
//! detector consumes the claim when the observer records the event of that same branch, old
//! and new oid, worktree and window. Without hooks there are no claims and nothing changes.
//!
//! Only `via: ancestry` counts: the multiplexer hint and an executor's mark do not prove that
//! this `git` descends from that session. The walk gets no executor marks, so it never waits for
//! a child of the daemon to register (the hook of an executor's own `git` waits for nothing,
//! ADR-GRD-003). Claims live in memory only, bounded and short-lived; nothing of the process but
//! its identity and folder is read (SEC-04).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::guard::{EvaluateParams, Operation, RefValue};
use gitraptor_api::timemachine::ResolvedVia;

use crate::channel::authz::{AcceptedPeer, Checks};
use crate::channel::requester;
use crate::guardrails::second_line::{GitProcess, nearest_git};
use crate::timemachine::oplog::{Requester, RequesterOrigin};

/// Claims kept (FIFO).
pub const REMEMBERED: usize = 256;

/// Age after which a claim is dropped: the event of its transaction arrives within a batch.
pub const CLAIM_TTL: Duration = Duration::from_secs(60);

/// How long before a batch's first mark a claim still belongs to it: the hook runs at
/// `prepared`, before Git writes the ref.
pub const CLAIM_LEAD: Duration = Duration::from_secs(5);

/// Whether the detector has a present session in a repo: without one there is nothing to
/// attribute, and the walk is skipped.
pub type Presence = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// One branch an agent's `git` is about to move.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    repo_id: String,
    /// The hook client's folder: Git runs hooks at the worktree's root.
    cwd: PathBuf,
    refname: String,
    /// `None`: the transaction gave no old value (a creation, or `update-ref` without one).
    old: Option<String>,
    new: String,
    session_id: String,
    git: Option<GitProcess>,
    /// Monotonic mark of the hook's request.
    t: u64,
}

/// A branch move of a Git event, as S4 matches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefMove<'a> {
    /// Short name (`feat-login`).
    pub branch: &'a str,
    pub old: Option<&'a str>,
    pub new: &'a str,
}

/// The claims of the hooks, shared by the channel (writer) and the detector (reader).
#[derive(Default)]
pub struct HookClaims {
    claims: Mutex<VecDeque<Claim>>,
    presence: Mutex<Option<Presence>>,
}

impl std::fmt::Debug for HookClaims {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookClaims").finish_non_exhaustive()
    }
}

impl HookClaims {
    /// The detector says which repos have a present session.
    pub(crate) fn attach(&self, presence: Presence) {
        if let Ok(mut p) = self.presence.lock() {
            *p = Some(presence);
        }
    }

    fn wants(&self, repo_id: &str) -> bool {
        let presence = self.presence.lock().ok().and_then(|p| p.clone());
        presence.is_some_and(|present| present(repo_id))
    }

    /// `guard.evaluate` answered `params`, asked by `peer`, letting it through when `allowed`
    /// (`appliedEffect = allow`): the claims of an allowed `reference-transaction` of a detected
    /// Claude Code session. Called before the reply, while the hook's `git` waits. `cwd` is read
    /// only when there is a claim to make.
    pub(crate) fn observe(
        &self,
        peer: AcceptedPeer,
        checks: &Checks<'_>,
        params: &EvaluateParams,
        allowed: bool,
        cwd: impl FnOnce() -> Option<PathBuf>,
        now: u64,
    ) {
        let Operation::RefTransaction { updates, .. } = &params.operation else {
            return;
        };
        if !allowed {
            return;
        }
        let moves: Vec<_> = updates
            .iter()
            .filter(|u| u.refname.starts_with("refs/heads/"))
            .filter_map(|u| Some((u, u.new.oid()?)))
            .collect();
        if moves.is_empty() {
            return;
        }
        // A later move of the same branch supersedes its earlier claims: their event already
        // came, or their transaction aborted after `prepared` and an orphan claim would hand
        // the next move (the developer's `reset`) to the agent.
        if let Ok(mut claims) = self.claims.lock() {
            claims.retain(|c| {
                c.repo_id != params.repo_id || !moves.iter().any(|(u, _)| u.refname == c.refname)
            });
        }
        if !self.wants(&params.repo_id) {
            return;
        }
        let Some(session_id) = session_of(peer, checks) else {
            return;
        };
        let Some(cwd) = cwd() else {
            return;
        };
        let git = nearest_git(peer, checks);
        let Ok(mut claims) = self.claims.lock() else {
            return;
        };
        let ttl = u64::try_from(CLAIM_TTL.as_nanos()).unwrap_or(u64::MAX);
        claims.retain(|c| now.saturating_sub(c.t) < ttl);
        for (update, new) in moves {
            if claims.len() >= REMEMBERED {
                claims.pop_front();
            }
            claims.push_back(Claim {
                repo_id: params.repo_id.clone(),
                cwd: cwd.clone(),
                refname: update.refname.clone(),
                old: match &update.old {
                    RefValue::Oid(o) => Some(o.clone()),
                    _ => None,
                },
                new: new.to_owned(),
                session_id: session_id.clone(),
                git,
                t: now,
            });
        }
    }

    /// A claim made by hand (detector tests).
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn claim_for_test(
        &self,
        repo_id: &str,
        cwd: &str,
        refname: &str,
        old: Option<&str>,
        new: &str,
        session_id: &str,
        t: u64,
    ) {
        self.claims.lock().unwrap().push_back(Claim {
            repo_id: repo_id.into(),
            cwd: cwd.into(),
            refname: refname.into(),
            old: old.map(str::to_owned),
            new: new.into(),
            session_id: session_id.into(),
            git: None,
            t,
        });
    }

    /// The second line denied the commit of `git`: its refs will not move.
    pub(crate) fn revoke(&self, git: GitProcess) {
        if let Ok(mut claims) = self.claims.lock() {
            claims.retain(|c| c.git != Some(git));
        }
    }

    /// Consumes the claims of `repo_id` for `moved` recorded in `[from, to]` whose folder
    /// `in_worktree` accepts, and returns their sessions.
    pub fn take(
        &self,
        repo_id: &str,
        moved: RefMove<'_>,
        from: u64,
        to: u64,
        in_worktree: impl Fn(&Path) -> bool,
    ) -> Vec<String> {
        let Ok(mut claims) = self.claims.lock() else {
            return Vec::new();
        };
        let refname = format!("refs/heads/{}", moved.branch);
        let mut sessions = Vec::new();
        claims.retain(|c| {
            let hit = c.repo_id == repo_id
                && c.refname == refname
                && c.new == moved.new
                && c.old.as_deref() == moved.old
                && c.t >= from
                && c.t <= to
                && in_worktree(&c.cwd);
            if hit && !sessions.contains(&c.session_id) {
                sessions.push(c.session_id.clone());
            }
            !hit
        });
        sessions
    }
}

/// The detected Claude Code session the hook client descends from, if the walk proves it.
fn session_of(peer: AcceptedPeer, checks: &Checks<'_>) -> Option<String> {
    let resolution = requester::resolve(peer, checks, None).ok()?;
    if resolution.via != ResolvedVia::Ancestry {
        return None;
    }
    match resolution.who.requester {
        Requester::Agent {
            origin: RequesterOrigin::Detected,
            session_id,
            ..
        } => Some(session_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use gitraptor_api::guard::{Hook, RefUpdate};

    use super::*;
    use crate::channel::AgentMatcher;
    use crate::channel::authz::TERMINAL_PROOF;
    use crate::channel::peer::{ProcError, ProcInfo, ProcSource};

    const OLD: &str = "1111111111111111111111111111111111111111";
    const NEW: &str = "2222222222222222222222222222222222222222";

    #[derive(Default)]
    struct Tree(HashMap<u32, ProcInfo>);

    impl Tree {
        fn add(&mut self, pid: u32, ppid: u32, exe: &str, start: u64) {
            self.0.insert(
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
            self.0.get(&pid).cloned().ok_or(ProcError::Gone)
        }
        fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
            Some(false)
        }
        fn pids_of(&self, _uid: u32) -> Option<Vec<u32>> {
            Some(self.0.keys().copied().collect())
        }
    }

    fn checks<'a>(
        t: &'a Tree,
        matcher: &'a AgentMatcher,
        daemon: Option<(u32, u64)>,
    ) -> Checks<'a> {
        Checks {
            uid: 501,
            procs: t,
            matcher,
            daemon,
            marks: None,
            terminal_proof: TERMINAL_PROOF,
        }
    }

    fn peer() -> AcceptedPeer {
        AcceptedPeer {
            pid: 50,
            start_us: 5,
            accepted_us: u64::MAX,
        }
    }

    /// `head`(10) → sh(20) → git(30) → sh(40, dispatcher) → raptor(50, hook client).
    fn tree(head: &str) -> Tree {
        let mut t = Tree::default();
        t.add(10, 1, head, 1);
        t.add(20, 10, "/bin/sh", 2);
        t.add(30, 20, "/usr/bin/git", 3);
        t.add(40, 30, "/bin/sh", 4);
        t.add(50, 40, "/usr/local/bin/raptor", 5);
        t
    }

    fn params(refname: &str, old: RefValue) -> EvaluateParams {
        EvaluateParams {
            repo_id: "r".into(),
            common_dir: "/r/.git".into(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: refname.into(),
                    old,
                    new: RefValue::Oid(NEW.into()),
                }],
                orphan_head: None,
            },
            authorship: None,
        }
    }

    fn claims(present: bool) -> HookClaims {
        let c = HookClaims::default();
        c.attach(Arc::new(move |repo| present && repo == "r"));
        c
    }

    fn observe(c: &HookClaims, t: &Tree, p: &EvaluateParams, allowed: bool) {
        let m = AgentMatcher::default();
        c.observe(
            peer(),
            &checks(t, &m, None),
            p,
            allowed,
            || Some(PathBuf::from("/r")),
            1_000,
        );
    }

    fn moved() -> RefMove<'static> {
        RefMove {
            branch: "main",
            old: Some(OLD),
            new: NEW,
        }
    }

    fn anywhere(_: &Path) -> bool {
        true
    }

    #[test]
    fn an_agents_allowed_transaction_leaves_one_claim_that_is_consumed() {
        let c = claims(true);
        observe(
            &c,
            &tree("/bin/claude"),
            &params("refs/heads/main", RefValue::Oid(OLD.into())),
            true,
        );
        assert_eq!(c.take("r", moved(), 0, 2_000, anywhere), vec!["10:1"]);
        // Consumed: one operation, one event.
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
    }

    #[test]
    fn the_event_must_be_that_same_move_in_that_worktree_and_window() {
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        observe(&c, &tree("/bin/claude"), &p, true);
        let other = |m: RefMove<'static>| c.take("r", m, 0, 2_000, anywhere);
        assert!(
            other(RefMove {
                branch: "feat",
                ..moved()
            })
            .is_empty()
        );
        assert!(
            other(RefMove {
                old: None,
                ..moved()
            })
            .is_empty()
        );
        assert!(
            other(RefMove {
                old: Some(NEW),
                ..moved()
            })
            .is_empty()
        );
        assert!(
            other(RefMove {
                new: OLD,
                ..moved()
            })
            .is_empty()
        );
        assert!(c.take("other", moved(), 0, 2_000, anywhere).is_empty());
        assert!(c.take("r", moved(), 1_001, 2_000, anywhere).is_empty());
        assert!(c.take("r", moved(), 0, 999, anywhere).is_empty());
        assert!(
            c.take("r", moved(), 0, 2_000, |p| p == Path::new("/wt"))
                .is_empty()
        );
        // Nothing matched, so it is still there.
        assert_eq!(
            c.take("r", moved(), 0, 2_000, |p| p == Path::new("/r"))
                .len(),
            1
        );
    }

    #[test]
    fn nothing_is_claimed_without_proof_of_an_agent_ancestor() {
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        // The developer's `git`.
        let c = claims(true);
        observe(&c, &tree("/bin/zsh"), &p, true);
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
        // A denied transaction does not move the ref.
        observe(&c, &tree("/bin/claude"), &p, false);
        observe(&c, &tree("/bin/claude"), &p, false);
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
        // No present session in the repo: the walk is not even made.
        let c = claims(false);
        observe(&c, &tree("/bin/claude"), &p, true);
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
        // Not a branch (a tag), or a deletion.
        let c = claims(true);
        observe(
            &c,
            &tree("/bin/claude"),
            &params("refs/tags/v1", RefValue::Zero),
            true,
        );
        let mut deletion = params("refs/heads/main", RefValue::Oid(OLD.into()));
        if let Operation::RefTransaction { updates, .. } = &mut deletion.operation {
            updates[0].new = RefValue::Zero;
        }
        observe(&c, &tree("/bin/claude"), &deletion, true);
        assert!(c.claims.lock().unwrap().is_empty());
    }

    /// The developer's `git` in a tmux pane while Claude Code runs in another: the requester's
    /// multiplexer hint names Claude Code, and S4 does not take it.
    #[test]
    fn a_shared_multiplexer_is_no_evidence() {
        let mut t = Tree::default();
        t.add(5, 1, "/opt/homebrew/bin/tmux", 1);
        t.add(6, 5, "/bin/claude", 2);
        t.add(20, 5, "/bin/zsh", 3);
        t.add(30, 20, "/usr/bin/git", 4);
        t.add(40, 30, "/bin/sh", 4);
        t.add(50, 40, "/usr/local/bin/raptor", 5);
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        observe(&c, &t, &p, true);
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
    }

    /// The daemon's own `git` (the executor): the walk stops at the daemon without waiting for
    /// any registration, and claims nothing.
    #[test]
    fn the_daemons_own_git_claims_nothing_and_does_not_wait() {
        let t = tree("/bin/claude");
        let m = AgentMatcher::default();
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        // The daemon is the shell between Claude Code and `git`.
        c.observe(
            peer(),
            &checks(&t, &m, Some((20, 2))),
            &p,
            true,
            || Some(PathBuf::from("/r")),
            1_000,
        );
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
    }

    #[test]
    fn the_second_line_revokes_and_old_claims_expire() {
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        observe(&c, &tree("/bin/claude"), &p, true);
        c.revoke((30, 3));
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
        // A claim older than the TTL is dropped when the next one comes.
        observe(&c, &tree("/bin/claude"), &p, true);
        let m = AgentMatcher::default();
        let later = 1_000 + u64::try_from(CLAIM_TTL.as_nanos()).unwrap();
        let mut p2 = p.clone();
        if let Operation::RefTransaction { updates, .. } = &mut p2.operation {
            updates[0].refname = "refs/heads/feat".into();
        }
        c.observe(
            peer(),
            &checks(&tree("/bin/claude"), &m, None),
            &p2,
            true,
            || Some(PathBuf::from("/r")),
            later,
        );
        assert!(c.take("r", moved(), 0, u64::MAX, anywhere).is_empty());
        assert_eq!(c.claims.lock().unwrap().len(), 1);
    }

    /// The agent's transaction aborted after `prepared` (another hook, a lock): the developer's
    /// next move of that branch to that same commit takes the orphan claim away.
    #[test]
    fn a_later_move_of_the_branch_drops_an_orphan_claim() {
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        observe(&c, &tree("/bin/claude"), &p, true);
        observe(&c, &tree("/bin/zsh"), &p, true);
        assert!(c.take("r", moved(), 0, 2_000, anywhere).is_empty());
        // Even with no present session to attribute to.
        let c = claims(true);
        observe(&c, &tree("/bin/claude"), &p, true);
        c.attach(Arc::new(|_| false));
        observe(&c, &tree("/bin/zsh"), &p, true);
        assert!(c.claims.lock().unwrap().is_empty());
        // Another branch keeps it.
        let c = claims(true);
        observe(&c, &tree("/bin/claude"), &p, true);
        observe(
            &c,
            &tree("/bin/zsh"),
            &params("refs/heads/feat", RefValue::Oid(OLD.into())),
            true,
        );
        assert_eq!(c.take("r", moved(), 0, 2_000, anywhere).len(), 1);
    }

    #[test]
    fn claims_are_bounded() {
        let c = claims(true);
        let p = params("refs/heads/main", RefValue::Oid(OLD.into()));
        // A flood of distinct moves.
        for n in 0..=REMEMBERED {
            let mut p = p.clone();
            if let Operation::RefTransaction { updates, .. } = &mut p.operation {
                updates[0].refname = format!("refs/heads/b{n}");
            }
            observe(&c, &tree("/bin/claude"), &p, true);
        }
        assert_eq!(c.claims.lock().unwrap().len(), REMEMBERED);
    }
}
