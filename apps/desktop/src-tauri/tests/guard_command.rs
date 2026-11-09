//! D7-CMD (DS-US-DSK-007 § 9.4, T001): the copyable command of a conscious exception as a pure
//! function of a log entry (`exception_command::exception`, `cancel`, `SKIPPABLE`). Entries are
//! built with `log_entry` of `support/fake_daemon.rs` from the `guard.log` wire form; no daemon
//! is started. The `raptor` is a path the function only quotes (the check that it is installed
//! belongs to `shell_command::raptor_path`, DS-US-DSK-008).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::collections::HashSet;
use std::path::Path;

use fake_daemon::log_entry;
use gitraptor_api::guard::Rule;
use gitraptor_desktop::engine::exception_command::{NotOffered, SKIPPABLE, cancel, exception};
use serde_json::{Value, json};

const RAPTOR: &str = "/opt/git raptor/raptor";
const PREFIX: &str = "'/opt/git raptor/raptor' 'guard' 'exec' '--' 'git' '-C' '/w/wt/feat-pagos'";

fn raptor() -> Option<&'static Path> {
    Some(Path::new(RAPTOR))
}

fn offered(overrides: Value) -> Result<String, NotOffered> {
    exception(raptor(), &log_entry(overrides))
}

fn push(remote: Value, refs: Value) -> Value {
    json!({"operation": {"kind": "push", "remote": remote, "refs": refs}})
}

fn text(value: &str) -> Value {
    json!({"untrusted": value})
}

fn forced(name: &str) -> Value {
    json!([{"name": text(name), "change": "force"}])
}

fn rules(names: &[&str]) -> Value {
    json!({"reasons": names
        .iter()
        .map(|rule| json!({"rule": rule, "level": "floor"}))
        .collect::<Vec<_>>()})
}

fn ref_transaction(refs: &[(&str, &str)]) -> Value {
    json!({"operation": {"kind": "ref-transaction", "refs": refs
        .iter()
        .map(|(name, change)| json!({"name": text(name), "change": change}))
        .collect::<Vec<_>>()}})
}

/// `overrides` of the default push denial with a rule list.
fn with_rules(mut overrides: Value, names: &[&str]) -> Value {
    overrides["reasons"] = rules(names)["reasons"].clone();
    overrides
}

#[test]
fn a_force_push_gives_the_exec_command_with_lease_if_includes_and_source_branch() {
    assert_eq!(
        offered(json!({})),
        Ok(format!(
            "{PREFIX} 'push' '--force-with-lease' '--force-if-includes' 'origin' 'feat-pagos:main'"
        )),
        "D7-CMD: a forced push of feat-pagos to main in origin gives raptor guard exec with \
         --force-with-lease --force-if-includes and the source branch"
    );
}

#[test]
fn a_push_without_force_has_no_lease_and_uses_the_ref_when_the_branch_is_missing_or_equal() {
    let update = |branch: Value, change: &str| {
        let mut o = push(
            text("origin"),
            json!([{"name": text("feat-pagos"), "change": change}]),
        );
        o["branch"] = branch;
        o
    };
    assert_eq!(
        offered(update(text("feat-pagos"), "update")),
        Ok(format!("{PREFIX} 'push' 'origin' 'feat-pagos'")),
        "D7-CMD: an update whose branch equals the ref is the bare ref, with no lease"
    );
    assert_eq!(
        offered(update(Value::Null, "create")),
        Ok(format!("{PREFIX} 'push' 'origin' 'feat-pagos'")),
        "D7-CMD: a create with no branch is the bare ref"
    );
    assert_eq!(
        offered(update(text("other"), "update")),
        Ok(format!("{PREFIX} 'push' 'origin' 'other:feat-pagos'")),
        "D7-CMD: an update from another branch is branch:ref"
    );
}

#[test]
fn a_push_that_deletes_a_remote_ref_gives_the_colon_refspec() {
    let got = offered(push(
        text("origin"),
        json!([{"name": text("old"), "change": "delete"}]),
    ));
    assert_eq!(
        got,
        Ok(format!("{PREFIX} 'push' 'origin' ':old'")),
        "D7-CMD: a ref delete of a push is :<ref>"
    );
}

#[test]
fn a_push_with_several_refs_lists_each_refspec_in_order() {
    let got = offered(push(
        text("origin"),
        json!([
            {"name": text("main"), "change": "force"},
            {"name": text("old"), "change": "delete"},
            {"name": text("v1"), "change": "create"}
        ]),
    ));
    assert_eq!(
        got,
        Ok(format!(
            "{PREFIX} 'push' '--force-with-lease' '--force-if-includes' 'origin' \
             'feat-pagos:main' ':old' 'feat-pagos:v1'"
        )),
        "D7-CMD: each ref gets its refspec, in the order of the entry; any force adds the lease"
    );
}

#[test]
fn the_command_never_carries_yes() {
    let entries = [
        json!({}),
        push(text("origin"), forced("main")),
        push(text("o'brien x"), forced("main")),
        ref_transaction(&[("refs/heads/old", "delete")]),
        with_rules(json!({}), &["permission.push"]),
    ];
    let mut commands = 0;
    for overrides in entries {
        if let Ok(command) = offered(overrides.clone()) {
            commands += 1;
            assert!(
                !command.contains("--yes") && !command.contains("'-y'"),
                "D7-CMD: {overrides} gave a command with --yes: {command}"
            );
        }
    }
    assert_eq!(
        commands, 5,
        "D7-CMD: the five entries are exceptable, so each gives a command to check for --yes"
    );
}

#[test]
fn a_quote_is_written_as_posix_escape() {
    let got = offered(push(text("o'brien x"), forced("main")));
    assert_eq!(
        got,
        Ok(format!(
            "{PREFIX} 'push' '--force-with-lease' '--force-if-includes' 'o'\\''brien x' \
             'feat-pagos:main'"
        )),
        "D7-CMD: a ' in an argument is written '\\'' and a space stays inside the quotes"
    );
}

#[test]
fn a_commit_is_not_offered() {
    let got = offered(json!({
        "operation": {"kind": "commit", "stage": "pre-commit"},
        "reasons": [{"rule": "policy.forbidden-path", "level": "floor"}]
    }));
    assert_eq!(
        got,
        Err(NotOffered::Commit),
        "D7-CMD: a commit has no exception (Q-GRD-47)"
    );
}

#[test]
fn a_rebase_is_not_offered_without_its_arguments() {
    let got = offered(with_rules(
        json!({"operation": {"kind": "rebase"}}),
        &["permission.rebase"],
    ));
    assert_eq!(
        got,
        Err(NotOffered::Rebase),
        "D7-CMD: the engine does not log upstream nor branch of a rebase, so no string is offered"
    );
}

#[test]
fn a_ref_move_is_not_offered() {
    let moved = offered(ref_transaction(&[("refs/heads/main", "update")]));
    let created = offered(ref_transaction(&[("refs/heads/new", "create")]));
    let forced_move = offered(ref_transaction(&[("refs/heads/main", "force")]));
    assert_eq!(
        (moved, created, forced_move),
        (
            Err(NotOffered::Operation),
            Err(NotOffered::Operation),
            Err(NotOffered::Operation)
        ),
        "D7-CMD: a ref transaction that moves or creates a ref has no exception"
    );
    // The contrast that tells a move from a delete: only the delete of local branches is offered.
    assert!(
        offered(ref_transaction(&[("refs/heads/old", "delete")])).is_ok(),
        "D7-CMD: the same transaction deleting a local branch is offered (branch -d)"
    );
    for operation in [
        json!({"kind": "protection-state", "from": "active", "to": "not-installed",
               "expected": false}),
        json!({"kind": "mcp-allowlist", "from": "full", "to": "hooks-only"}),
    ] {
        assert_eq!(
            offered(json!({"operation": operation})),
            Err(NotOffered::Operation),
            "D7-CMD: {operation} has no exception"
        );
    }
}

#[test]
fn a_branch_delete_gives_branch_lowercase_d_with_short_names() {
    let got = offered(with_rules(
        ref_transaction(&[
            ("refs/heads/old", "delete"),
            ("refs/heads/feat/viejo", "delete"),
        ]),
        &["permission.branch-delete"],
    ));
    let command = got.expect("D7-CMD: deleting local branches is offered");
    assert_eq!(
        command,
        format!("{PREFIX} 'branch' '-d' 'old' 'feat/viejo'"),
        "D7-CMD: branch -d (lowercase, never -D) with the names without refs/heads/"
    );
    assert!(
        !command.contains("'-D'"),
        "D7-CMD: git refuses to drop an unmerged branch with -d; -D is never offered"
    );
}

#[test]
fn a_ref_outside_refs_heads_is_not_offered() {
    let got = offered(ref_transaction(&[
        ("refs/heads/old", "delete"),
        ("refs/tags/v1", "delete"),
    ]));
    assert_eq!(
        got,
        Err(NotOffered::Operation),
        "D7-CMD: a delete of a ref outside refs/heads/ is not offered"
    );
    assert!(
        offered(ref_transaction(&[("refs/heads/old", "delete")])).is_ok(),
        "D7-CMD: the same delete under refs/heads/ is offered"
    );
}

#[test]
fn a_url_remote_is_not_offered() {
    for remote in [
        "https://github.com/acme/tienda.git",
        "ssh://git@host/acme/tienda",
        "git@github.com:acme/tienda.git",
    ] {
        assert_eq!(
            offered(push(text(remote), forced("main"))),
            Err(NotOffered::Remote),
            "D7-CMD: the URL remote {remote} is refused (D2 of DS-US-GRD-006)"
        );
    }
    assert_eq!(
        offered(push(Value::Null, forced("main"))),
        Err(NotOffered::Remote),
        "D7-CMD: a push without a remote is not offered"
    );
    assert!(
        offered(push(text("origin"), forced("main"))).is_ok(),
        "D7-CMD: the remote given by name is offered"
    );
}

#[test]
fn an_entry_without_an_absolute_worktree_is_not_offered() {
    for worktree in [
        Value::Null,
        text("feat-pagos"),
        text("../wt/feat-pagos"),
        text(""),
    ] {
        assert_eq!(
            offered(json!({"worktree": worktree})),
            Err(NotOffered::WhereUnknown),
            "D7-CMD: {worktree} is not an absolute worktree"
        );
    }
}

#[test]
fn a_rate_limited_entry_is_not_offered() {
    assert_eq!(
        offered(json!({"detail": "rate-limited"})),
        Err(NotOffered::RateLimited),
        "D7-CMD: an entry annotated without detail has no command"
    );
}

#[test]
fn a_rule_outside_the_skippable_list_is_not_offered() {
    let not_skippable = [
        "system.degraded",
        "system.channel-not-authentic",
        "system.repo-mismatch",
        "system.input-rejected",
        "system.internal-error",
        "system.snapshot-failed",
        "authorship.trailer-required",
        "authorship.human-author",
        "policy.config-protected",
        "config.relax-ignored",
    ];
    for rule in not_skippable {
        assert_eq!(
            offered(with_rules(json!({}), &[rule])),
            Err(NotOffered::Rule),
            "D7-CMD: {rule} cannot be skipped"
        );
    }
    assert_eq!(
        offered(with_rules(
            json!({}),
            &["minimum.force-push", "system.internal-error"]
        )),
        Err(NotOffered::Rule),
        "D7-CMD: one rule that cannot be skipped is enough to refuse"
    );
}

#[test]
fn every_skippable_rule_is_offered() {
    let skippable = [
        "minimum.force-push",
        "minimum.base-branch-delete",
        "policy.protected-branch",
        "policy.forbidden-path",
        "policy.diff-size",
        "policy.commit-format",
        "permission.commit",
        "permission.push",
        "permission.force-push",
        "permission.reset-hard",
        "permission.branch-delete",
        "permission.rebase",
        "permission.merge",
        "permission.worktree-add",
        "permission.worktree-remove",
    ];
    for rule in skippable {
        assert!(
            offered(with_rules(json!({}), &[rule])).is_ok(),
            "D7-CMD: a push denied only by {rule} is offered"
        );
    }
}

#[test]
fn skippable_is_fixed_to_the_rules_of_d5() {
    let expected: HashSet<Rule> = [
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
    ]
    .into_iter()
    .collect();
    let got: HashSet<Rule> = SKIPPABLE.iter().copied().collect();
    assert_eq!(
        got, expected,
        "D7-CMD: SKIPPABLE is the list of D8 (DS-US-GRD-006 D5) with every permission.* rule"
    );
    assert_eq!(
        SKIPPABLE.len(),
        expected.len(),
        "D7-CMD: SKIPPABLE lists no rule twice"
    );
}

#[test]
fn without_a_known_raptor_no_command_is_offered() {
    assert_eq!(
        exception(None, &log_entry(json!({}))),
        Err(NotOffered::RaptorUnknown),
        "D7-CMD: without an installed raptor no command is offered (never PATH)"
    );
}

#[test]
fn hostile_names_are_never_offered() {
    let hostile = [
        (
            "a branch with an OSC 52",
            json!({"branch": text("x\u{1b}]52;c;AAAA\u{7}")}),
        ),
        (
            "a folder with U+202E",
            json!({"worktree": text("/w/wt/fe\u{202e}at")}),
        ),
        (
            "a ref with U+2028",
            push(text("origin"), forced("ma\u{2028}in")),
        ),
        (
            "a ref with a tag character U+E0041",
            push(text("origin"), forced("ma\u{e0041}in")),
        ),
        (
            "a remote with ESC",
            push(text("or\u{1b}igin"), forced("main")),
        ),
    ];
    for (what, overrides) in hostile {
        assert_eq!(
            offered(overrides),
            Err(NotOffered::UnsafeText),
            "D7-CMD: {what} must give unsafe-text, never a command"
        );
    }
}

#[test]
fn names_the_daemon_would_refuse_are_unsafe_text() {
    let refs = [
        "", "-x", "+evil", "a:b", "feat*", "a?b", "a[b", "a\\b", "a!b",
    ];
    for name in refs {
        assert_eq!(
            offered(push(text("origin"), forced(name))),
            Err(NotOffered::UnsafeText),
            "D7-CMD: the ref name {name:?} would be refused by the daemon"
        );
    }
    let remotes = [
        "--receive-pack=/tmp/x",
        "-x",
        "+r",
        "a*",
        "a?b",
        "a[b",
        "a\\b",
        "a!b",
    ];
    for remote in remotes {
        assert_eq!(
            offered(push(text(remote), forced("main"))),
            Err(NotOffered::UnsafeText),
            "D7-CMD: the remote {remote:?} would be refused by the daemon"
        );
    }
    for remote in ["/tmp/r", "./r", "~/r"] {
        assert_eq!(
            offered(push(text(remote), forced("main"))),
            Err(NotOffered::UnsafeText),
            "D7-CMD: the remote {remote:?} starts like a path the daemon refuses"
        );
    }
    let branches = ["-x", "+evil", "a:b", "a*", "a?b", "a[b", "a\\b", "a!b", ""];
    for name in branches {
        assert_eq!(
            offered(ref_transaction(&[(
                &format!("refs/heads/{name}"),
                "delete"
            )])),
            Err(NotOffered::UnsafeText),
            "D7-CMD: the branch name {name:?} would be refused by the daemon"
        );
    }
}

#[test]
fn the_cancel_command_is_quoted() {
    assert_eq!(
        cancel(raptor(), "/w/o'brien/feat pagos"),
        Some("'/opt/git raptor/raptor' 'guard' 'cancel' '/w/o'\\''brien/feat pagos'".to_owned()),
        "D7-CMD: raptor guard cancel <path> with every argument in single quotes"
    );
    assert_eq!(
        cancel(None, "/w/tienda"),
        None,
        "D7-CMD: without a known raptor the cancel command is not offered"
    );
    assert_eq!(
        cancel(raptor(), "/w/fe\u{202e}at"),
        None,
        "D7-CMD: a path with a character that acts on the terminal is not pasted"
    );
}
