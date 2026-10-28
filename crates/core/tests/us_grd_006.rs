//! US-GRD-006 PR-A (DS-US-GRD-006 D6, D9, D11, D18; prep S9-S11): the pure token store of the
//! conscious exception, in `gitraptor_core::guardrails::exception`. No repo, no I/O: clocks are
//! injected in ms and concurrency uses threads released by a `Barrier`.

use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    ExceptionRefusal, IssuedException, LoggedOperation, LoggedRef, PendingKind, RefChange, Rule,
};
use gitraptor_core::guardrails::exception::{
    Bound, CHILD_WAIT, CONSUMED_MAX, ExceptionPlan, ExceptionToken, Presentation, Presented,
    Rejection, TOKEN_TTL, Tokens, ct_eq, digest,
};
use gitraptor_core::guardrails::pending::{PendingActions, Requester};
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_policy::guard::exception::{Binding, Transition};

const ME: Requester = Requester {
    pid: 100,
    start_us: 1,
};
const OTHER: Requester = Requester {
    pid: 200,
    start_us: 2,
};
const CHILD: (u32, u64) = (101, 11);
const STRANGER: (u32, u64) = (999, 99);
const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const WT: &str = "/work/tree";
const TTL_MS: i64 = TOKEN_TTL.as_millis() as i64;

fn main_ref(new: &str) -> Transition {
    Transition::Ref {
        remote: Some("origin".into()),
        refname: "refs/heads/main".into(),
        old: Some(A.into()),
        new: Some(new.into()),
    }
}

fn rebase(refname: &str, base: &str) -> Transition {
    Transition::Rebase {
        refname: refname.into(),
        base: base.into(),
    }
}

fn plan_with(worktree: &str, transitions: Vec<Transition>, rules: Vec<Rule>) -> ExceptionPlan {
    ExceptionPlan {
        worktree: worktree.into(),
        binding: Binding {
            operation: LoggedOperation::Push {
                remote: Some(Untrusted::new("origin")),
                refs: vec![LoggedRef {
                    name: Untrusted::new("main"),
                    change: RefChange::Force,
                }],
            },
            transitions,
        },
        rules,
    }
}

fn bound(action: &str, repo: &str, transitions: Vec<Transition>) -> Bound {
    Bound {
        action_id: action.into(),
        repo_id: repo.into(),
        requester: ME,
        plan: plan_with(WT, transitions, vec![Rule::MinimumForcePush]),
    }
}

/// A store of a short child wait with `action` issued at `now_ms` for `repo`.
struct Fixture {
    tokens: Tokens,
    secret: ExceptionToken,
    transitions: Vec<Transition>,
}

fn fixture(repo: &str, action: &str, transitions: Vec<Transition>, now_ms: i64) -> Fixture {
    let tokens = Tokens::new(Duration::from_millis(50));
    let secret = issue_in(&tokens, repo, action, transitions.clone(), now_ms);
    Fixture {
        tokens,
        secret,
        transitions,
    }
}

fn issue_in(
    tokens: &Tokens,
    repo: &str,
    action: &str,
    tr: Vec<Transition>,
    now_ms: i64,
) -> ExceptionToken {
    tokens
        .issue(bound(action, repo, tr), now_ms)
        .expect("issue should return a token")
}

fn presentation<'a>(
    repo: &'a str,
    token: &'a ExceptionToken,
    git: (u32, u64),
    transitions: &'a [Transition],
    reasons: &'a [Rule],
) -> Presentation<'a> {
    Presentation {
        repo_id: repo,
        worktree: WT,
        token,
        git,
        git_parent: ME,
        transitions,
        reasons,
    }
}

impl Fixture {
    fn present(&self, repo: &str, now_ms: i64) -> Presented {
        let reasons = [Rule::MinimumForcePush];
        self.tokens.present(
            &presentation(repo, &self.secret, CHILD, &self.transitions, &reasons),
            now_ms,
        )
    }
}

fn applied(action: &str, first: bool) -> Presented {
    Presented::Applied {
        action_id: action.into(),
        first,
    }
}

// ---------------------------------------------------------------------------------- the token

#[test]
fn tokens_issue_128_bit_hex_tokens_that_differ() {
    let tokens = Tokens::default();
    let one = tokens
        .issue(bound("a1", "r1", vec![main_ref(B)]), 0)
        .expect("token 1");
    let two = tokens
        .issue(bound("a2", "r2", vec![main_ref(B)]), 0)
        .expect("token 2");
    for t in [&one, &two] {
        let s = t.expose();
        assert_eq!(s.len(), 32, "a token is 32 hex characters (128 bits): {s}");
        assert!(
            s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "a token is lowercase hex: {s}"
        );
    }
    assert_ne!(
        one.expose(),
        two.expose(),
        "two emissions should give different tokens"
    );
}

#[test]
fn tokens_keep_only_the_sha256_of_the_token() {
    // The digest is SHA-256 (FIPS 180-2 vector for "abc").
    let abc = digest("abc");
    let hex: String = abc.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "digest should be SHA-256"
    );

    let tokens = Tokens::default();
    let secret = issue_in(&tokens, "r1", "a1", vec![main_ref(B)], 0);
    assert_eq!(
        tokens.digest_of("a1"),
        Some(digest(secret.expose())),
        "the store should keep Sha256(token) for the action"
    );
    assert_eq!(
        tokens.digest_of("nope"),
        None,
        "an unknown action has no digest"
    );
    let stored = tokens.digest_of("a1").unwrap();
    assert_ne!(
        stored.as_slice(),
        secret.expose().as_bytes(),
        "the digest is not the token"
    );
    let dbg = format!("{tokens:?}");
    assert!(
        !dbg.contains(secret.expose()),
        "the Debug of Tokens shows the token: {dbg}"
    );
    assert!(
        !format!("{stored:?}").contains(secret.expose()),
        "the digest should not contain the token text"
    );
}

#[test]
fn tokens_compare_digests_with_ct_eq() {
    let base = digest("a-token");
    assert!(ct_eq(&base, &base), "equal digests should compare equal");
    for index in [0, 15, 31] {
        let mut other = base;
        other[index] ^= 1;
        assert!(
            !ct_eq(&base, &other),
            "a different byte at {index} should compare unequal"
        );
    }

    // A presentation that differs only in its first or last character is unknown.
    let fx = fixture("r1", "a1", vec![main_ref(B)], 0);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    let flip = |c: char| if c == '0' { '1' } else { '0' };
    let text = fx.secret.expose().to_owned();
    let mut first = text.clone();
    first.replace_range(0..1, &flip(text.chars().next().unwrap()).to_string());
    let mut last = text.clone();
    let n = last.len();
    last.replace_range(n - 1..n, &flip(text.chars().last().unwrap()).to_string());
    let reasons = [Rule::MinimumForcePush];
    for forged in [first, last] {
        let forged = ExceptionToken::from_secret(forged).expect("32 lowercase hex");
        assert_eq!(
            fx.tokens.present(
                &presentation("r1", &forged, CHILD, &fx.transitions, &reasons),
                1_000
            ),
            Presented::Rejected(Rejection::Unknown),
            "a token that differs in one character should be Rejected(Unknown)"
        );
    }
    assert_eq!(
        fx.present("r1", 1_000),
        applied("a1", true),
        "the real token still applies"
    );
}

#[test]
fn tokens_expire_only_before_the_first_presentation() {
    let fx = fixture("r1", "a1", vec![main_ref(B)], 1_000);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    assert_eq!(
        fx.present("r1", 1_000 + TTL_MS + 1),
        Presented::Rejected(Rejection::Expired),
        "a first presentation after the TTL should be Rejected(Expired)"
    );

    let fx = fixture("r1", "a1", vec![main_ref(B)], 1_000);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    assert_eq!(
        fx.present("r1", 1_000 + TTL_MS - 1_000),
        applied("a1", true),
        "a presentation within the TTL applies"
    );
    assert_eq!(
        fx.present("r1", 1_000 + 2 * TTL_MS),
        applied("a1", false),
        "after the first presentation the same git keeps the token beyond the TTL"
    );
}

#[test]
fn tokens_concurrent_presentations_consume_exactly_once() {
    const N: usize = 8;
    let fx = Arc::new(fixture("r1", "a1", vec![main_ref(B)], 0));
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    let barrier = Arc::new(Barrier::new(N));
    let handles: Vec<_> = (0..N)
        .map(|_| {
            let (fx, barrier) = (Arc::clone(&fx), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                fx.present("r1", 1_000)
            })
        })
        .collect();
    let results: Vec<Presented> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let firsts = results
        .iter()
        .filter(|r| matches!(r, Presented::Applied { first: true, .. }))
        .count();
    let applied_all = results
        .iter()
        .all(|r| matches!(r, Presented::Applied { action_id, .. } if action_id == "a1"));
    assert!(
        applied_all,
        "every valid concurrent presentation is Applied: {results:?}"
    );
    assert_eq!(
        firsts, 1,
        "exactly one concurrent presentation consumes the token: {results:?}"
    );
}

#[test]
fn tokens_reuse_covers_only_bound_transitions_from_the_same_git() {
    let reasons = [Rule::MinimumForcePush];

    // Ref transitions: the second is bound too; presenting only it applies.
    let second = Transition::Ref {
        remote: Some("origin".into()),
        refname: "refs/heads/dev".into(),
        old: None,
        new: Some(B.into()),
    };
    let fx = fixture("r1", "a1", vec![main_ref(B), second.clone()], 0);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    assert_eq!(fx.present("r1", 1_000), applied("a1", true));
    assert_eq!(
        fx.present("r1", 2_000),
        applied("a1", false),
        "the same git reuses the token"
    );
    let at = |tr: &[Transition], git: (u32, u64)| {
        fx.tokens
            .present(&presentation("r1", &fx.secret, git, tr, &reasons), 3_000)
    };
    assert_eq!(
        at(std::slice::from_ref(&second), CHILD),
        applied("a1", false),
        "any of the bound transitions applies"
    );
    let other_ref = Transition::Ref {
        remote: Some("origin".into()),
        refname: "refs/heads/release".into(),
        old: Some(A.into()),
        new: Some(B.into()),
    };
    let other_new = main_ref(A);
    for tr in [other_ref, other_new] {
        assert_eq!(
            at(std::slice::from_ref(&tr), CHILD),
            Presented::Rejected(Rejection::OtherTransition),
            "another ref or another new value is Rejected(OtherTransition): {tr:?}"
        );
    }
    assert_eq!(
        at(&fx.transitions, STRANGER),
        Presented::Rejected(Rejection::NotTheChild),
        "another git is Rejected(NotTheChild)"
    );

    // Rebase: the same ref and base are reused; another ref or base is not.
    let rb = vec![rebase("refs/heads/feat", A)];
    let fx = fixture("r2", "a2", rb.clone(), 0);
    assert_eq!(fx.tokens.bind_child("a2", ME, CHILD), Ok(()));
    let at = |tr: &[Transition]| {
        fx.tokens
            .present(&presentation("r2", &fx.secret, CHILD, tr, &reasons), 1_000)
    };
    assert_eq!(at(&rb), applied("a2", true));
    assert_eq!(
        at(&rb),
        applied("a2", false),
        "the rebase reuses the same ref and base"
    );
    for tr in [rebase("refs/heads/other", A), rebase("refs/heads/feat", B)] {
        assert_eq!(
            at(std::slice::from_ref(&tr)),
            Presented::Rejected(Rejection::OtherTransition),
            "another rebase ref or base is Rejected(OtherTransition): {tr:?}"
        );
    }
}

#[test]
fn tokens_reject_another_repo_or_worktree() {
    let fx = fixture("r1", "a1", vec![main_ref(B)], 0);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    assert_eq!(
        fx.present("r2", 1_000),
        Presented::Rejected(Rejection::Unknown),
        "the token presented in another repo is Rejected(Unknown)"
    );
    let reasons = [Rule::MinimumForcePush];
    let mut elsewhere = presentation("r1", &fx.secret, CHILD, &fx.transitions, &reasons);
    elsewhere.worktree = "/another/worktree";
    assert_eq!(
        fx.tokens.present(&elsewhere, 1_000),
        Presented::Rejected(Rejection::OtherWorktree),
        "the token presented in another worktree of the repo is Rejected(OtherWorktree)"
    );
    assert_eq!(
        fx.present("r1", 1_000),
        applied("a1", true),
        "the right place still applies"
    );
}

#[test]
fn tokens_refuse_a_rule_not_announced() {
    let fx = fixture("r1", "a1", vec![main_ref(B)], 0);
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    for reasons in [
        vec![Rule::MinimumForcePush, Rule::PermissionPush],
        vec![Rule::Degraded],
    ] {
        assert_eq!(
            fx.tokens.present(
                &presentation("r1", &fx.secret, CHILD, &fx.transitions, &reasons),
                1_000
            ),
            Presented::Rejected(Rejection::RuleNotAnnounced),
            "{reasons:?}: a reason that was not announced (or is not skippable) is rejected"
        );
    }
    assert_eq!(
        fx.present("r1", 1_000),
        applied("a1", true),
        "a rejected presentation should not consume the token"
    );
}

#[test]
fn tokens_bind_child_only_for_the_requester_once() {
    let fx = fixture("r1", "a1", vec![main_ref(B)], 0);
    assert_eq!(
        fx.tokens.bind_child("a1", OTHER, CHILD),
        Err(ExceptionRefusal::NotTheRequester),
        "another process cannot bind the child"
    );
    assert_eq!(
        fx.tokens.bind_child("nope", ME, CHILD),
        Err(ExceptionRefusal::NoPending),
        "an unknown action has no pending exception"
    );
    assert_eq!(fx.tokens.bind_child("a1", ME, CHILD), Ok(()));
    assert_eq!(
        fx.tokens.bind_child("a1", ME, (102, 12)),
        Err(ExceptionRefusal::ChildNotOurs),
        "a second bind of the same action is ChildNotOurs"
    );

    let reasons = [Rule::MinimumForcePush];
    let mut orphan = presentation("r1", &fx.secret, CHILD, &fx.transitions, &reasons);
    orphan.git_parent = OTHER;
    assert_eq!(
        fx.tokens.present(&orphan, 1_000),
        Presented::Rejected(Rejection::NotTheChild),
        "a git whose parent is not the requester is Rejected(NotTheChild)"
    );
}

#[test]
fn tokens_wait_for_the_child_at_most_child_wait() {
    assert_eq!(CHILD_WAIT, Duration::from_secs(2), "CHILD_WAIT is 2 s");

    let tokens = Tokens::new(Duration::from_millis(150));
    let secret = issue_in(&tokens, "r1", "a1", vec![main_ref(B)], 0);
    let trs = [main_ref(B)];
    let reasons = [Rule::MinimumForcePush];
    let started = Instant::now();
    let got = tokens.present(&presentation("r1", &secret, CHILD, &trs, &reasons), 1_000);
    let took = started.elapsed();
    assert_eq!(
        got,
        Presented::Rejected(Rejection::ChildNotBound),
        "no child bound"
    );
    assert!(
        took >= Duration::from_millis(100),
        "present should wait for the child up to the configured cap, took {took:?}"
    );
    assert!(
        took < Duration::from_millis(1_500),
        "present should not wait past the configured cap, took {took:?}"
    );

    // Tokens::default() uses CHILD_WAIT.
    let tokens = Tokens::default();
    let secret = issue_in(&tokens, "r1", "a1", vec![main_ref(B)], 0);
    let started = Instant::now();
    let got = tokens.present(&presentation("r1", &secret, CHILD, &trs, &reasons), 1_000);
    let took = started.elapsed();
    assert_eq!(got, Presented::Rejected(Rejection::ChildNotBound));
    assert!(
        took >= Duration::from_millis(1_500) && took < Duration::from_secs(5),
        "the default wait should be CHILD_WAIT (2 s), took {took:?}"
    );
}

#[test]
fn tokens_presentation_waits_for_a_child_bound_meanwhile() {
    let tokens = Arc::new(Tokens::new(Duration::from_secs(30)));
    let secret = issue_in(&tokens, "r1", "a1", vec![main_ref(B)], 0);
    let (tx, rx) = mpsc::channel();
    let presenter = {
        let tokens = Arc::clone(&tokens);
        std::thread::spawn(move || {
            let trs = [main_ref(B)];
            let reasons = [Rule::MinimumForcePush];
            tx.send(()).unwrap();
            let started = Instant::now();
            let got = tokens.present(&presentation("r1", &secret, CHILD, &trs, &reasons), 1_000);
            (got, started.elapsed())
        })
    };
    rx.recv().unwrap();
    assert_eq!(tokens.bind_child("a1", ME, CHILD), Ok(()));
    let (got, took) = presenter.join().unwrap();
    assert_eq!(
        got,
        applied("a1", true),
        "the waiting presentation applies once the child is bound"
    );
    assert!(
        took < Duration::from_secs(10),
        "it should not wait for the cap (30 s) once the child is bound, took {took:?}"
    );
}

#[test]
fn tokens_end_reports_consumption_and_forgets_the_token() {
    let tokens = Tokens::new(Duration::from_millis(50));
    let used = issue_in(&tokens, "r1", "a1", vec![main_ref(B)], 0);
    let unused = issue_in(&tokens, "r2", "a2", vec![main_ref(B)], 0);
    assert_eq!(tokens.bind_child("a1", ME, CHILD), Ok(()));
    let trs = [main_ref(B)];
    let reasons = [Rule::MinimumForcePush];
    let present = |repo: &str, secret: &ExceptionToken| {
        tokens.present(&presentation(repo, secret, CHILD, &trs, &reasons), 1_000)
    };
    assert_eq!(present("r1", &used), applied("a1", true));

    assert_eq!(
        tokens.end("a1", OTHER),
        Err(ExceptionRefusal::NotTheRequester),
        "another process cannot end it"
    );
    assert_eq!(
        tokens.end("nope", ME),
        Err(ExceptionRefusal::NoPending),
        "an unknown action has no pending exception"
    );
    assert_eq!(
        tokens.end("a1", ME),
        Ok(true),
        "end tells the token was consumed"
    );
    assert_eq!(
        present("r1", &used),
        Presented::Rejected(Rejection::Unknown),
        "after end the token is forgotten"
    );
    assert_eq!(
        tokens.end("a2", ME),
        Ok(false),
        "end tells the token was never consumed"
    );
    assert_eq!(
        present("r2", &unused),
        Presented::Rejected(Rejection::Unknown),
        "an ended token that was never presented is forgotten too"
    );
}

#[test]
fn tokens_sweep_drops_expired_and_stale_tokens() {
    let consumed_max = CONSUMED_MAX.as_millis() as i64;
    let tokens = Tokens::new(Duration::from_millis(50));
    let trs = [main_ref(B)];
    let reasons = [Rule::MinimumForcePush];
    let now = 1_000 + consumed_max + 1;

    // a1: never presented, TTL long gone. a2: consumed more than CONSUMED_MAX ago.
    // a3: fresh and unpresented. a4: consumed recently.
    let secrets: Vec<(String, ExceptionToken)> = [
        ("r1", "a1", 0),
        ("r2", "a2", 0),
        ("r3", "a3", now - 10_000),
        ("r4", "a4", now - 11_000),
    ]
    .into_iter()
    .map(|(repo, action, at)| {
        (
            repo.to_owned(),
            issue_in(&tokens, repo, action, vec![main_ref(B)], at),
        )
    })
    .collect();
    for (action, repo) in [("a2", "r2"), ("a4", "r4")] {
        assert_eq!(tokens.bind_child(action, ME, CHILD), Ok(()));
        let secret = &secrets.iter().find(|(r, _)| r == repo).unwrap().1;
        let at = if repo == "r2" { 1_000 } else { now - 6_000 };
        assert_eq!(
            tokens.present(&presentation(repo, secret, CHILD, &trs, &reasons), at),
            applied(action, true)
        );
    }

    let mut dropped = tokens.sweep(now);
    dropped.sort();
    assert_eq!(
        dropped,
        vec!["a1".to_owned(), "a2".to_owned()],
        "sweep should return the unpresented expired and the long-consumed tokens"
    );
    assert_eq!(tokens.digest_of("a1"), None, "a1 is gone");
    assert_eq!(tokens.digest_of("a2"), None, "a2 is gone");
    assert!(tokens.digest_of("a3").is_some(), "a fresh token is kept");
    assert!(
        tokens.digest_of("a4").is_some(),
        "a recently consumed token is kept"
    );
}

#[test]
fn tokens_never_show_the_token_in_debug() {
    let tokens = Tokens::default();
    let token = tokens
        .issue(bound("a1", "r1", vec![main_ref(B)]), 0)
        .expect("token");
    let secret = token.expose().to_owned();
    let secret_token = token.clone();
    let issued = IssuedException {
        token: token.clone(),
        git: Untrusted::new("/usr/bin/git"),
        config: vec!["core.x=y".into()],
        ttl_ms: 60_000,
    };
    assert_eq!(tokens.bind_child("a1", ME, CHILD), Ok(()));
    let trs = [main_ref(B)];
    let reasons = [Rule::MinimumForcePush];
    let presentation = presentation("r1", &secret_token, CHILD, &trs, &reasons);
    let shown = format!("{presentation:?}");
    let presented = tokens.present(&presentation, 1_000);
    for (what, text) in [
        ("Presentation", shown),
        ("ExceptionToken", format!("{token:?}")),
        ("IssuedException", format!("{issued:?}")),
        ("Tokens", format!("{tokens:?}")),
        ("Presented", format!("{presented:?}")),
    ] {
        assert!(
            !text.contains(&secret),
            "the Debug of {what} shows the token: {text}"
        );
    }
}

// ----------------------------------------------------------------------------- pending, registry

#[test]
fn tokens_pending_announcement_carries_the_plan() {
    let plan = plan_with(
        WT,
        vec![main_ref(B)],
        vec![Rule::MinimumForcePush, Rule::PermissionPush],
    );
    // The view: the normalized operation and the rules of the plan.
    let view = plan.view();
    assert_eq!(view.operation, plan.binding.operation, "view.operation");
    assert_eq!(view.rules, plan.rules, "view.rules");

    let mut pending = PendingActions::default();
    let window = Duration::from_secs(1);
    pending
        .announce("r1", PendingKind::Uninstall, ME, 0, window, "a1".into())
        .expect("announce");
    pending
        .announce("r2", PendingKind::Uninstall, ME, 0, window, "a2".into())
        .expect("announce");
    // Another action of the repo, and another repo, change nothing.
    pending.note_exception("r1", "other-action", plan.clone());
    pending.note_exception("r3", "a1", plan.clone());
    pending.note_exception("r1", "a1", plan.clone());

    let taken = |pending: &mut PendingActions, repo: &str, action: &str| {
        pending
            .take_due(repo, action, PendingKind::Uninstall, ME, 1_000)
            .expect("the window closed")
    };
    // `Entry.exception` is not in the skeleton: its derived Debug shows it.
    let with = format!("{:?}", taken(&mut pending, "r1", "a1"));
    assert!(
        with.contains("exception: Some(") && with.contains("MinimumForcePush"),
        "note_exception should store the plan in Entry.exception: {with}"
    );
    let without = format!("{:?}", taken(&mut pending, "r2", "a2"));
    assert!(
        without.contains("exception: None"),
        "another repo's action should keep no exception: {without}"
    );
}

#[test]
fn tokens_live_in_the_guard_registry() {
    let registry = Arc::new(GuardRegistry::default());
    let issuer = Arc::clone(&registry);
    let secret =
        std::thread::spawn(move || issue_in(issuer.exceptions(), "r1", "a1", vec![main_ref(B)], 0))
            .join()
            .unwrap();
    let reader = Arc::clone(&registry);
    let got = std::thread::spawn(move || {
        let tokens = reader.exceptions();
        assert_eq!(tokens.bind_child("a1", ME, CHILD), Ok(()));
        let trs = [main_ref(B)];
        let reasons = [Rule::MinimumForcePush];
        tokens.present(&presentation("r1", &secret, CHILD, &trs, &reasons), 1_000)
    })
    .join()
    .unwrap();
    assert_eq!(
        got,
        applied("a1", true),
        "a token issued through one reference should be presented through another of the registry"
    );
}
