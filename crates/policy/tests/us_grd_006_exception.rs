//! US-GRD-006 PR-A (DS-US-GRD-006 D1, D2, D3, D5, D9; prep S5-S8): the pure side of the conscious
//! exception, in `gitraptor_policy::guard::exception`. Public API only, no I/O: the repo state is
//! a hand-built `RepoFacts`.

use std::collections::BTreeMap;

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, ExceptionRefusal, Level, LoggedOperation, LoggedRef, Reason, RefChange, Rule,
};
use gitraptor_policy::guard::exception::{
    Binding, Form, Refspec, RepoFacts, Request, Transition, Unsupported, Upstream, announce,
    covers, exceptable, parse, resolve,
};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const D: &str = "4444444444444444444444444444444444444444";

type RebaseCase = (
    &'static [&'static str],
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

fn av(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

fn ok(parts: &[&str]) -> Request {
    match parse(&av(parts)) {
        Ok(req) => req,
        Err(e) => panic!("{parts:?} should be admitted, got {e:?}"),
    }
}

fn refused(parts: &[&str]) -> Unsupported {
    match parse(&av(parts)) {
        Ok(req) => panic!("{parts:?} should be refused, got {req:?}"),
        Err(e) => e,
    }
}

fn push_of(req: Request) -> (Option<String>, Vec<Refspec>, bool) {
    match req.form {
        Form::Push {
            remote,
            refspecs,
            force,
        } => (remote, refspecs, force),
        other => panic!("expected a push, got {other:?}"),
    }
}

fn spec(src: Option<&str>, dst: Option<&str>, force: bool) -> Refspec {
    Refspec {
        src: src.map(str::to_owned),
        dst: dst.map(str::to_owned),
        force,
    }
}

fn some(s: &str) -> Option<String> {
    Some(s.to_owned())
}

// ---------------------------------------------------------------------------------------- parse

#[test]
fn parse_accepts_the_admitted_push_branch_and_rebase_forms() {
    // push with explicit refspecs: src, src:dst, +src:dst, :dst.
    let (remote, specs, force) = push_of(ok(&[
        "git",
        "push",
        "origin",
        "main",
        "feat:main",
        "+a:b",
        ":gone",
    ]));
    assert_eq!(
        remote,
        some("origin"),
        "parse should keep the remote as given"
    );
    assert_eq!(
        specs,
        vec![
            spec(Some("main"), None, false),
            spec(Some("feat"), Some("main"), false),
            spec(Some("a"), Some("b"), true),
            spec(None, Some("gone"), false),
        ],
        "parse should keep each refspec: src, src:dst, +src:dst and :dst"
    );
    assert!(!force, "no force option was given");

    // push force options.
    for flag in [
        "-f",
        "--force",
        "--force-with-lease",
        "--force-with-lease=main",
    ] {
        let (remote, specs, force) = push_of(ok(&["git", "push", flag, "origin", "main"]));
        assert_eq!(remote, some("origin"), "{flag}: remote");
        assert_eq!(
            specs,
            vec![spec(Some("main"), None, false)],
            "{flag}: refspecs"
        );
        assert!(force, "{flag} should mark the push as forced");
    }
    ok(&["git", "push", "--force-if-includes", "origin", "main"]);

    // push --delete / -d.
    for flag in ["--delete", "-d"] {
        let (remote, specs, _) = push_of(ok(&["git", "push", flag, "origin", "x"]));
        assert_eq!(remote, some("origin"), "{flag}: remote");
        assert_eq!(
            specs,
            vec![spec(None, Some("x"), false)],
            "{flag} x should be a deletion refspec (src none, dst x)"
        );
    }

    // push [<remote>] without refspec.
    let (remote, specs, _) = push_of(ok(&["git", "push"]));
    assert_eq!(
        (remote, specs),
        (None, vec![]),
        "bare push has no remote and no refspec"
    );
    let (remote, specs, _) = push_of(ok(&["git", "push", "origin"]));
    assert_eq!(
        (remote, specs),
        (some("origin"), vec![]),
        "push <remote> has no refspec"
    );

    // branch -d / -D / --delete [--force].
    let cases: [(&[&str], &[&str], bool); 4] = [
        (&["git", "branch", "-d", "a", "b"], &["a", "b"], false),
        (&["git", "branch", "-D", "a"], &["a"], true),
        (&["git", "branch", "--delete", "a"], &["a"], false),
        (&["git", "branch", "--delete", "--force", "a"], &["a"], true),
    ];
    for (argv, want_names, want_force) in cases {
        match ok(argv).form {
            Form::BranchDelete { names, force } => {
                assert_eq!(names, av(want_names), "{argv:?}: names");
                assert_eq!(force, want_force, "{argv:?}: force");
            }
            other => panic!("{argv:?}: expected a branch delete, got {other:?}"),
        }
    }

    // rebase [--onto <base>] [<upstream> [<branch>]].
    let rebases: [RebaseCase; 5] = [
        (&["git", "rebase"], None, None, None),
        (
            &["git", "rebase", "origin/main"],
            None,
            Some("origin/main"),
            None,
        ),
        (
            &["git", "rebase", "origin/main", "feat"],
            None,
            Some("origin/main"),
            Some("feat"),
        ),
        (
            &["git", "rebase", "--onto", "base", "up", "br"],
            Some("base"),
            Some("up"),
            Some("br"),
        ),
        (
            &["git", "rebase", "--onto", "base"],
            Some("base"),
            None,
            None,
        ),
    ];
    for (argv, onto, upstream, branch) in rebases {
        match ok(argv).form {
            Form::Rebase {
                onto: o,
                upstream: u,
                branch: b,
            } => {
                assert_eq!(o.as_deref(), onto, "{argv:?}: onto");
                assert_eq!(u.as_deref(), upstream, "{argv:?}: upstream");
                assert_eq!(b.as_deref(), branch, "{argv:?}: branch");
            }
            other => panic!("{argv:?}: expected a rebase, got {other:?}"),
        }
    }
}

#[test]
fn parse_accepts_only_dash_c_dir_as_global_option() {
    assert_eq!(
        ok(&["git", "-C", "/a", "-C", "b", "push", "origin", "x"]).dirs,
        av(&["/a", "b"]),
        "-C is repeatable and lands in Request.dirs in order"
    );
    assert_eq!(
        ok(&["git", "push", "origin", "x"]).dirs,
        Vec::<String>::new(),
        "no -C, no dirs"
    );
    let globals: [&[&str]; 10] = [
        &["-c", "core.x=y"],
        &["--git-dir=/x"],
        &["--git-dir", "/x"],
        &["--work-tree=/x"],
        &["--namespace=n"],
        &["--exec-path=/x"],
        &["--config-env=a=B"],
        &["-p"],
        &["--no-pager"],
        &["--bogus"],
    ];
    for global in globals {
        let mut argv = vec!["git"];
        argv.extend_from_slice(global);
        argv.extend(["push", "origin", "x"]);
        assert_eq!(
            refused(&argv),
            Unsupported::Form,
            "{global:?} should be refused as Unsupported::Form"
        );
    }
}

#[test]
fn parse_refuses_the_forms_outside_the_list_as_unsupported_form() {
    let forms: [&[&str]; 18] = [
        &["git", "push", "--all", "origin"],
        &["git", "push", "--mirror", "origin"],
        &["git", "push", "--tags", "origin"],
        &["git", "push", "--no-verify", "origin", "main"],
        &["git", "push", "origin", "refs/heads/*:refs/heads/*"],
        &["git", "push", "origin", "feat*"],
        &["git", "push", "https://example.com/r.git", "main"],
        &["git", "push", "git@example.com:r.git", "main"],
        &["git", "push", "/tmp/other", "main"],
        &["git", "rebase", "-i", "main"],
        &["git", "rebase", "--interactive", "main"],
        &["git", "rebase", "--exec", "make", "main"],
        &["git", "rebase", "-x", "make", "main"],
        &["git", "rebase", "--exec=make", "main"],
        &["git", "rebase", "--root"],
        &["git", "pull"],
        &["git", "pull", "origin", "main"],
        &["git", "push", "origin", "main", "--tags"],
    ];
    for argv in forms {
        assert_eq!(
            refused(argv),
            Unsupported::Form,
            "{argv:?} should be refused as Unsupported::Form"
        );
    }
    assert_eq!(
        Unsupported::Form.refusal(),
        ExceptionRefusal::UnsupportedForm,
        "Form should translate to ExceptionRefusal::UnsupportedForm"
    );
}

#[test]
fn parse_refuses_other_subcommands_as_unsupported_operation() {
    let ops: [&[&str]; 8] = [
        &["git", "commit", "-m", "x"],
        &["git", "merge", "feat"],
        &["git", "reset", "--hard", "HEAD~1"],
        &["git", "update-ref", "refs/heads/x", "HEAD"],
        &["git", "worktree", "add", "../w"],
        &["git", "switch", "feat"],
        &["git", "branch", "feat"],
        &["git", "st"],
    ];
    for argv in ops {
        assert_eq!(
            refused(argv),
            Unsupported::Operation,
            "{argv:?} should be refused as Unsupported::Operation"
        );
    }
    assert_eq!(
        Unsupported::Operation.refusal(),
        ExceptionRefusal::UnsupportedOperation,
        "Operation should translate to ExceptionRefusal::UnsupportedOperation"
    );
}

#[test]
fn parse_requires_literal_git_first() {
    assert_eq!(refused(&[]), Unsupported::Form, "an empty argv");
    for first in ["/usr/bin/git", "git.exe", "sh"] {
        assert_eq!(
            refused(&[first, "push", "origin", "main"]),
            Unsupported::Form,
            "{first} is not literally `git`"
        );
    }
    ok(&["git", "push", "origin", "main"]);
}

#[test]
fn parse_refuses_an_argv_over_its_bounds() {
    // 65 elements: git, push, origin and 62 refspecs.
    let mut over = av(&["git", "push", "origin"]);
    over.extend((0..62).map(|i| format!("b{i}")));
    assert_eq!(over.len(), 65);
    assert_eq!(
        parse(&over),
        Err(Unsupported::Form),
        "an argv of 65 elements should be refused"
    );

    // One element of 4097 bytes.
    let mut long = av(&["git", "push", "origin"]);
    long.push("a".repeat(4097));
    assert_eq!(
        parse(&long),
        Err(Unsupported::Form),
        "an element of 4097 bytes should be refused"
    );

    // Exactly 64 elements of 4096 bytes: not refused by size.
    let mut edge = av(&["git", "push", "origin"]);
    edge.extend((0..61).map(|_| "a".repeat(4096)));
    assert_eq!(edge.len(), 64);
    assert!(
        parse(&edge).is_ok(),
        "64 elements of up to 4096 bytes should not be refused by size"
    );
}

// -------------------------------------------------------------------------------------- resolve

fn facts() -> RepoFacts {
    let mut f = RepoFacts {
        head: some("feat"),
        push_default: None,
        remotes: av(&["origin"]),
        push_remote: some("origin"),
        upstreams: BTreeMap::new(),
        tracking: BTreeMap::new(),
        revs: BTreeMap::new(),
    };
    f.tracking
        .insert(("origin".into(), "refs/heads/main".into()), A.into());
    f.tracking
        .insert(("origin".into(), "refs/heads/feat".into()), B.into());
    f.revs.insert("feat".into(), C.into());
    f.revs.insert("HEAD".into(), C.into());
    f.revs.insert("main".into(), D.into());
    f.revs.insert("old".into(), A.into());
    f
}

fn req(form: Form) -> Request {
    Request { dirs: vec![], form }
}

fn push(remote: Option<&str>, refspecs: Vec<Refspec>, force: bool) -> Request {
    req(Form::Push {
        remote: remote.map(str::to_owned),
        refspecs,
        force,
    })
}

fn reference(
    remote: Option<&str>,
    refname: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> Transition {
    Transition::Ref {
        remote: remote.map(str::to_owned),
        refname: refname.to_owned(),
        old: old.map(str::to_owned),
        new: new.map(str::to_owned),
    }
}

fn bound(req: &Request, facts: &RepoFacts) -> Binding {
    match resolve(req, facts) {
        Ok(b) => b,
        Err(e) => panic!("{req:?} should resolve, got {e:?}"),
    }
}

#[test]
fn resolve_binds_explicit_refspecs_to_tracking_old_and_local_new() {
    let f = facts();
    let binding = bound(
        &push(
            Some("origin"),
            vec![
                spec(Some("feat"), Some("main"), false),
                spec(Some("main"), None, false),
                spec(Some("HEAD"), Some("fresh"), false),
            ],
            false,
        ),
        &f,
    );
    assert_eq!(
        binding.transitions,
        vec![
            reference(Some("origin"), "refs/heads/main", Some(A), Some(C)),
            reference(Some("origin"), "refs/heads/main", Some(A), Some(D)),
            reference(Some("origin"), "refs/heads/fresh", None, Some(C)),
        ],
        "each refspec should become a Ref with refs/heads/<dst>, the tracking oid as old (None \
         without a copy) and the local oid of src as new; HEAD is the current branch"
    );

    let unknown_remote = push(
        Some("elsewhere"),
        vec![spec(Some("main"), None, false)],
        false,
    );
    assert_eq!(
        resolve(&unknown_remote, &f),
        Err(Unsupported::Form),
        "a remote outside facts.remotes should be Unsupported::Form"
    );
    let unknown_rev = push(Some("origin"), vec![spec(Some("nope"), None, false)], false);
    assert_eq!(
        resolve(&unknown_rev, &f),
        Err(Unsupported::Form),
        "an unknown rev should be Unsupported::Form"
    );
}

#[test]
fn resolve_push_without_refspec_follows_push_default() {
    let upstream = |merge: &str| {
        BTreeMap::from([(
            "feat".to_owned(),
            Upstream {
                remote: "origin".into(),
                merge: merge.into(),
            },
        )])
    };
    let bare = || push(None, vec![], false);

    // Unset and `simple`: the current branch to its upstream.
    for default in [None, some("simple")] {
        let mut f = facts();
        f.push_default = default.clone();
        f.upstreams = upstream("refs/heads/feat");
        assert_eq!(
            bound(&bare(), &f).transitions,
            vec![reference(
                Some("origin"),
                "refs/heads/feat",
                Some(B),
                Some(C)
            )],
            "push.default {default:?} should push the current branch to its upstream"
        );
    }

    // `current`: same name on the push remote, upstream or not.
    let mut f = facts();
    f.push_default = some("current");
    assert_eq!(
        bound(&bare(), &f).transitions,
        vec![reference(
            Some("origin"),
            "refs/heads/feat",
            Some(B),
            Some(C)
        )],
        "push.default current should push to the same name"
    );

    // `upstream`: the merge ref of the upstream, whatever its name.
    let mut f = facts();
    f.push_default = some("upstream");
    f.upstreams = upstream("refs/heads/main");
    assert_eq!(
        bound(&bare(), &f).transitions,
        vec![reference(
            Some("origin"),
            "refs/heads/main",
            Some(A),
            Some(C)
        )],
        "push.default upstream should push to the upstream's ref"
    );

    // Refused: matching, nothing, detached HEAD, simple/upstream without upstream.
    for default in ["matching", "nothing"] {
        let mut f = facts();
        f.push_default = some(default);
        f.upstreams = upstream("refs/heads/feat");
        assert_eq!(
            resolve(&bare(), &f),
            Err(Unsupported::Form),
            "push.default {default} should be Unsupported::Form"
        );
    }
    let mut f = facts();
    f.head = None;
    f.push_default = some("current");
    assert_eq!(
        resolve(&bare(), &f),
        Err(Unsupported::Form),
        "a detached HEAD should be Unsupported::Form"
    );
    for default in [None, some("simple"), some("upstream")] {
        let mut f = facts();
        f.push_default = default.clone();
        assert_eq!(
            resolve(&bare(), &f),
            Err(Unsupported::Form),
            "push.default {default:?} without upstream should be Unsupported::Form"
        );
    }
}

fn changes(op: &LoggedOperation) -> Vec<RefChange> {
    match op {
        LoggedOperation::Push { refs, .. } | LoggedOperation::RefTransaction { refs } => {
            refs.iter().map(|r| r.change).collect()
        }
        other => panic!("expected refs, got {other:?}"),
    }
}

#[test]
fn resolve_binds_deletions_with_no_new_value() {
    let f = facts();

    // push --delete x and push <remote> :x parse to the same refspec.
    {
        let binding = bound(
            &push(Some("origin"), vec![spec(None, Some("main"), false)], false),
            &f,
        );
        assert_eq!(
            binding.transitions,
            vec![reference(Some("origin"), "refs/heads/main", Some(A), None)],
            "a push deletion should keep the tracking oid as old and no new value"
        );
        assert_eq!(
            changes(&binding.operation),
            vec![RefChange::Delete],
            "the normalized operation should say Delete"
        );
    }

    // branch -d / -D: old is the local branch oid.
    for force in [false, true] {
        let binding = bound(
            &req(Form::BranchDelete {
                names: vec!["old".into()],
                force,
            }),
            &f,
        );
        assert_eq!(
            binding.transitions,
            vec![reference(None, "refs/heads/old", Some(A), None)],
            "a branch delete should bind the local oid as old and no new value"
        );
        let json = serde_json::to_string(&binding.operation).unwrap();
        assert!(
            json.contains("\"delete\""),
            "the normalized branch delete should carry RefChange::Delete: {json}"
        );
    }

    // A local branch that does not exist.
    let missing = req(Form::BranchDelete {
        names: vec!["ghost".into()],
        force: false,
    });
    assert_eq!(
        resolve(&missing, &f),
        Err(Unsupported::Form),
        "deleting a branch that does not exist should be Unsupported::Form"
    );
}

#[test]
fn resolve_binds_the_rebase_to_its_ref_and_base() {
    let rebase = |onto: Option<&str>, upstream: Option<&str>, branch: Option<&str>| {
        req(Form::Rebase {
            onto: onto.map(str::to_owned),
            upstream: upstream.map(str::to_owned),
            branch: branch.map(str::to_owned),
        })
    };
    let rb = |refname: &str, base: &str| Transition::Rebase {
        refname: refname.to_owned(),
        base: base.to_owned(),
    };
    let mut f = facts();
    f.revs.insert("origin/main".into(), A.into());
    f.revs.insert("other".into(), B.into());
    f.revs.insert("topic".into(), D.into());
    f.upstreams.insert(
        "feat".into(),
        Upstream {
            remote: "origin".into(),
            merge: "refs/heads/main".into(),
        },
    );

    // Given upstream and branch.
    assert_eq!(
        bound(&rebase(None, Some("origin/main"), Some("topic")), &f).transitions,
        vec![rb("refs/heads/topic", A)],
        "the ref is the given branch and the base the oid of the given upstream"
    );
    // The current branch and its given upstream.
    assert_eq!(
        bound(&rebase(None, Some("origin/main"), None), &f).transitions,
        vec![rb("refs/heads/feat", A)],
        "without a branch the ref is the current one"
    );
    // --onto wins over the upstream.
    assert_eq!(
        bound(&rebase(Some("other"), Some("origin/main"), None), &f).transitions,
        vec![rb("refs/heads/feat", B)],
        "the base is the oid of --onto when given"
    );
    // The configured upstream of the branch.
    assert_eq!(
        bound(&rebase(None, None, None), &f).transitions,
        vec![rb("refs/heads/feat", A)],
        "without an upstream the base is the configured upstream of the branch"
    );

    // Refused: no upstream given or configured; detached HEAD without a branch.
    let mut bare = facts();
    bare.revs.insert("origin/main".into(), A.into());
    assert_eq!(
        resolve(&rebase(None, None, None), &bare),
        Err(Unsupported::Form),
        "no upstream given or configured should be Unsupported::Form"
    );
    let mut detached = f.clone();
    detached.head = None;
    assert_eq!(
        resolve(&rebase(None, Some("origin/main"), None), &detached),
        Err(Unsupported::Form),
        "a detached HEAD without a branch should be Unsupported::Form"
    );
}

#[test]
fn resolve_logs_only_the_normalized_operation() {
    let f = facts();
    let request = Request {
        dirs: vec!["/secret/workdir".into()],
        form: Form::Push {
            remote: some("origin"),
            refspecs: vec![
                spec(Some("feat"), Some("main"), true),
                spec(Some("feat"), Some("fresh"), false),
                spec(Some("feat"), Some("other"), false),
            ],
            force: true,
        },
    };
    let mut facts = f.clone();
    facts
        .tracking
        .insert(("origin".into(), "refs/heads/other".into()), A.into());
    let binding = bound(&request, &facts);

    let log_ref = |name: &str, change| LoggedRef {
        name: Untrusted::new(name),
        change,
    };
    assert_eq!(
        binding.operation,
        LoggedOperation::Push {
            remote: Some(Untrusted::new("origin")),
            refs: vec![
                log_ref("main", RefChange::Force),
                log_ref("fresh", RefChange::Create),
                log_ref("other", RefChange::Update),
            ],
        },
        "a push should log the remote and the short refs with Force (+), Create (no old) and Update"
    );
    let json = serde_json::to_string(&binding.operation).unwrap();
    for leaked in ["/secret/workdir", "-C", "--", A, B, C, D] {
        assert!(
            !json.contains(leaked),
            "the logged operation leaks `{leaked}`: {json}"
        );
    }

    // Rebase: upstream and branch, as names.
    let mut rb = facts.clone();
    rb.revs.insert("origin/main".into(), A.into());
    let binding = bound(
        &req(Form::Rebase {
            onto: None,
            upstream: some("origin/main"),
            branch: some("feat"),
        }),
        &rb,
    );
    assert_eq!(
        binding.operation,
        LoggedOperation::Rebase {
            upstream: Some(Untrusted::new("origin/main")),
            branch: Some(Untrusted::new("feat")),
        },
        "a rebase should log its upstream and branch"
    );
    let json = serde_json::to_string(&binding.operation).unwrap();
    for leaked in [A, B, C, D] {
        assert!(
            !json.contains(leaked),
            "the logged rebase leaks an oid: {json}"
        );
    }
}

// ----------------------------------------------------------------------------------- exceptable

const SKIPPABLE: [Rule; 14] = [
    Rule::MinimumForcePush,
    Rule::MinimumBaseBranchDelete,
    Rule::PermissionCommit,
    Rule::PermissionPush,
    Rule::PermissionForcePush,
    Rule::PermissionResetHard,
    Rule::PermissionBranchDelete,
    Rule::PermissionRebase,
    Rule::PermissionMerge,
    Rule::PermissionWorktreeAdd,
    Rule::PermissionWorktreeRemove,
    Rule::ProtectedBranch,
    Rule::ForbiddenPath,
    Rule::SnapshotFailed,
];

const NOT_SKIPPABLE: [Rule; 8] = [
    Rule::Degraded,
    Rule::ChannelNotAuthentic,
    Rule::RepoMismatch,
    Rule::InputRejected,
    Rule::InternalError,
    Rule::AuthorshipTrailerRequired,
    Rule::AuthorshipHumanAuthor,
    Rule::ConfigProtected,
];

fn reason(rule: Rule) -> Reason {
    Reason {
        rule,
        level: Level::Minimum,
        cause: Some(Cause::NotFastForward),
        params: vec![],
    }
}

#[test]
fn exceptable_lists_exactly_the_skippable_rules() {
    for rule in SKIPPABLE {
        assert!(exceptable(rule), "{rule:?} should be skippable");
    }
    for rule in NOT_SKIPPABLE.into_iter().chain([Rule::RelaxIgnored]) {
        assert!(!exceptable(rule), "{rule:?} should not be skippable");
    }
}

#[test]
fn exceptable_announcement_adds_the_snapshot_rule_and_refuses_otherwise() {
    let reasons = [
        reason(Rule::MinimumForcePush),
        reason(Rule::PermissionPush),
        reason(Rule::MinimumForcePush),
    ];
    assert_eq!(
        announce(&reasons, false),
        Ok(vec![Rule::MinimumForcePush, Rule::PermissionPush]),
        "the announcement lists the rules in order of appearance without duplicates"
    );
    assert_eq!(
        announce(&reasons, true),
        Ok(vec![
            Rule::MinimumForcePush,
            Rule::PermissionPush,
            Rule::SnapshotFailed
        ]),
        "an operation that asks for a prior adds system.snapshot-failed at the end"
    );
    assert_eq!(
        announce(&[], false),
        Err(ExceptionRefusal::NotDenied),
        "no reasons and no prior is NotDenied"
    );
    assert_eq!(
        announce(&[], true),
        Ok(vec![Rule::SnapshotFailed]),
        "no reasons but a prior still announces system.snapshot-failed"
    );
    assert_eq!(
        announce(&[reason(Rule::Degraded)], false),
        Err(ExceptionRefusal::NotExceptable),
        "a non-skippable reason is NotExceptable"
    );
    assert_eq!(
        announce(
            &[
                reason(Rule::MinimumForcePush),
                reason(Rule::ConfigProtected)
            ],
            true
        ),
        Err(ExceptionRefusal::NotExceptable),
        "one non-skippable reason among skippable ones is NotExceptable"
    );
}

#[test]
fn exceptable_covers_only_announced_skippable_reasons() {
    use Rule::{Degraded, MinimumForcePush as Force, PermissionPush as Push, ProtectedBranch};
    assert!(
        covers(&[Force, Push], &[Force]),
        "a subset of the announcement is covered"
    );
    assert!(
        covers(&[Force, Push], &[Push, Force]),
        "order does not matter"
    );
    assert!(
        !covers(&[Force], &[Force, ProtectedBranch]),
        "a skippable rule that was not announced is not covered"
    );
    assert!(
        !covers(&[Degraded], &[Degraded]),
        "a non-skippable rule announced by mistake is not covered"
    );
    assert!(
        !covers(&[Force, Degraded], &[Force, Degraded]),
        "one non-skippable reason spoils the whole presentation"
    );
}
