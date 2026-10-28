//! US-GRD-006 end to end (DS-US-GRD-006 T005): the conscious exception, `raptor guard exec`.
//! Real `raptor` (daemon, client and hook), the real `raptor-hook` dispatcher and plain Git over
//! the temporary machine of `guard_machine` (NFR-01: temporary repos, remotes, profile and
//! daemon; never this repo nor the real profile). The daemon recognises only
//! `raptor-fake-agent` (`GITRAPTOR_AGENT_EXECUTABLES`), so the suite passes inside and outside a
//! Claude Code session. Every wait is on an explicit signal (`guard.status`, the event stream,
//! the audit, a process exit). Unix only; Windows is pending the cross-platform validation.
#![cfg(unix)]

mod guard_machine;

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::event::RESERVED_AUDIT;
use gitraptor_api::messages::{ClientKind, RefusalReason, RefusedData};
use gitraptor_api::methods;
use gitraptor_api::rpc::code;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::profile::Profile;
use gitraptor_testkit::fixture::git_from_path;
use guard_machine::{FAKE_AGENT, FAKE_AGENT_ARGV, Machine, Running, text};
use serde_json::{Value, json};

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const EVERYONE_MAIN: &str =
    r#"{"policies":{"protectedBranches":{"patterns":["main"],"appliesTo":"everyone"}}}"#;
const DENY_COMMIT: &str = r#"{"permissions":{"deny":["commit"]}}"#;

fn repo(m: &Machine) -> String {
    m.f.repo.to_string_lossy().into_owned()
}

/// "demo" protected, with `feat-x` rewritten locally (a push needs a force), on `main`.
fn machine(window_ms: u64) -> Machine {
    let m = Machine::with_window(window_ms);
    m.add(&m.f.repo);
    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    m.rewrite_feat_x();
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    m
}

/// `raptor guard exec --yes -- git -C <repo> <extra>` in the developer's terminal.
fn exec(m: &Machine, extra: &[&str]) -> Running {
    let repo = repo(m);
    let mut args = vec!["guard", "exec", "--yes", "--", "git", "-C", &repo];
    args.extend_from_slice(extra);
    m.developer_spawn(&args)
}

fn force_push(m: &Machine) -> Running {
    exec(m, &["push", "-q", "-f", "origin", "feat-x"])
}

/// Waits (bounded) until `guard.status` shows the pending exception: the announcement is in.
fn wait_pending(m: &Machine) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let status = m.status_json(&m.f.repo);
        if status["pending"]["action"] == "exception" {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "no pending exception in guard.status (is `raptor guard exec` there?): {status}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn log_entries(m: &Machine) -> Vec<Value> {
    let out = m.developer(&["guard", "log", "--json", &repo(m)]);
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find_map(|l| l.find('{').map(|i| &l[i..]))
        .unwrap_or_else(|| panic!("no JSON: {stdout}"));
    let log: Value = serde_json::from_str(line.trim()).unwrap();
    log["entries"].as_array().cloned().unwrap_or_default()
}

fn of_kind(entries: &[Value], kind: &str) -> Vec<Value> {
    entries
        .iter()
        .filter(|e| e["kind"] == kind)
        .cloned()
        .collect()
}

/// The outcomes of the audit rows of `guard.exception`, once the daemon is stopped.
fn exception_audit(m: &Machine) -> Vec<String> {
    m.stop();
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    profile
        .audit(0, 1000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.operation == "guard.exception")
        .map(|(_, row)| row.outcome)
        .collect()
}

fn feat_x_remote(m: &Machine) -> String {
    m.remote_ref("refs/heads/feat-x").expect("feat-x on remote")
}

// ---- Escenario 1 ----

/// A git run by `name` (a copy of this binary called so, as a child of the simulated agent).
fn git_as(m: &Machine, dir: &Path, name: &str, args: &[&str]) -> std::process::Output {
    let bin = dir.join(name);
    if !bin.exists() {
        std::fs::create_dir_all(dir).unwrap();
        gitraptor_testkit::fixture::clone_binary(&std::env::current_exe().unwrap(), &bin);
    }
    let mut argv = vec![git_from_path().to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| (*a).to_owned()));
    Command::new(&bin)
        .args([
            "fake_agent_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .envs(m.env())
        .envs([
            ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
            (
                FAKE_AGENT_ARGV,
                serde_json::to_string(&argv).unwrap().into(),
            ),
        ])
        .current_dir(&m.f.repo)
        .output()
        .unwrap()
}

#[test]
fn same_decision_for_every_actor() {
    let m = machine(200);
    let outside = tempfile::tempdir().unwrap();
    // The registered agent: `codex`, which the daemon does not detect, registered in "demo".
    let out = m.developer(&["agent", "register", "codex", "--worktree", &repo(&m)]);
    assert!(out.status.success(), "{}", text(&out));
    let push = ["push", "-q", "-f", "origin", "feat-x"];
    let before = feat_x_remote(&m);
    let mut reasons = Vec::new();
    for (actor, name) in [
        ("detected agent", Some(FAKE_AGENT)),
        ("registered agent", Some("codex")),
        ("unattributed process", None),
    ] {
        let out = match name {
            Some(name) => git_as(&m, outside.path(), name, &push),
            None => m.git(&m.f.repo, &push),
        };
        assert!(!out.status.success(), "{actor}: the force-push ran");
        let shown = text(&out);
        assert!(
            shown.contains("feat-x") && shown.to_lowercase().contains("force"),
            "{actor}: the denial should name the force-push of feat-x: {shown}"
        );
        // SEC-GRD-06: the denial never says how to skip it.
        assert!(!shown.contains("guard exec"), "{actor}: {shown}");
        // The harness of the simulated agent adds its own lines: compare GitRaptor's.
        let said: Vec<&str> = shown
            .lines()
            .filter_map(|l| l.find("GitRaptor:").map(|i| &l[i..]))
            .collect();
        assert!(!said.is_empty(), "{actor}: GitRaptor said nothing: {shown}");
        reasons.push(said.join("\n"));
    }
    assert_eq!(feat_x_remote(&m), before, "no actor changed the remote");
    assert!(
        reasons.windows(2).all(|w| w[0] == w[1]),
        "the same operation should get the same reason: {reasons:#?}"
    );
}

// ---- Escenario 2 ----

#[test]
fn exception_is_announced_waits_and_runs_once() {
    let m = machine(2_500);
    let mut other = Client::connect(&m.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let _: Value = other.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    let local = m.git_ok(&m.f.repo, &["rev-parse", "feat-x"]);
    let before = feat_x_remote(&m);
    assert_ne!(local, before);

    let running = force_push(&m);
    let status = wait_pending(&m);
    let plan = &status["pending"]["exception"];
    assert!(
        plan["rules"].to_string().contains("minimum.force-push"),
        "GRD006-ANNOUNCE-RUN: guard.status should show the rule the exception skips: {status}"
    );
    assert_eq!(
        feat_x_remote(&m),
        before,
        "nothing runs while the window is open"
    );
    // The announcement reaches every client as an accepted reserved.audit.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "GRD006-ANNOUNCE-RUN: no accepted reserved.audit of guard.exception on the stream"
        );
        let Some(n) = other.next_notification(Duration::from_millis(500)).unwrap() else {
            continue;
        };
        let event = &n.params["event"];
        if event["kind"] == RESERVED_AUDIT
            && event["data"]["operation"] == "guard.exception"
            && event["data"]["outcome"] == "accepted"
        {
            break;
        }
    }
    let status_text = m.raptor(&["guard", "status", &repo(&m)]);
    assert!(
        text(&status_text).to_lowercase().contains("exception"),
        "raptor guard status should show the pending exception: {}",
        text(&status_text)
    );

    let out = running.finish();
    assert_eq!(
        out.status.code(),
        Some(0),
        "exec exits with git's code: {}",
        text(&out)
    );
    assert_eq!(feat_x_remote(&m), local, "the remote has the new commit");
    let entries = log_entries(&m);
    let used = of_kind(&entries, "exception");
    assert_eq!(used.len(), 1, "one exception entry: {entries:#?}");
    assert!(
        used[0].to_string().contains("minimum.force-push"),
        "the entry names the skipped rule: {}",
        used[0]
    );
    assert!(
        used[0]["operation"].to_string().contains("push"),
        "the entry names the operation: {}",
        used[0]
    );
    assert!(used[0]["atMs"].is_i64(), "the entry says when: {}", used[0]);
}

// ---- Escenario 3 ----

#[test]
fn a_cancelled_exception_does_not_run() {
    let m = machine(60_000);
    let before = feat_x_remote(&m);
    let running = force_push(&m);
    wait_pending(&m);
    let cancel = m.developer(&["guard", "cancel", &repo(&m)]);
    assert!(cancel.status.success(), "{}", text(&cancel));
    let out = running.finish();
    assert!(
        !out.status.success(),
        "exec should not succeed: {}",
        text(&out)
    );
    assert_eq!(feat_x_remote(&m), before, "git was not launched");
    let entries = log_entries(&m);
    let cancelled = of_kind(&entries, "exception-cancelled");
    assert_eq!(
        cancelled.len(),
        1,
        "GRD006-CANCEL: one exception-cancelled entry: {entries:#?}"
    );
    assert!(
        cancelled[0].get("actor").is_some(),
        "the entry carries the actor that cancelled: {}",
        cancelled[0]
    );
    assert!(of_kind(&entries, "exception").is_empty());
    assert!(
        exception_audit(&m).contains(&"cancelled".to_owned()),
        "GRD006-CANCEL: the audit should say cancelled"
    );
}

#[test]
fn ctrl_c_in_the_window_cancels() {
    let m = machine(60_000);
    let before = feat_x_remote(&m);
    let mut running = force_push(&m);
    wait_pending(&m);
    // Ctrl-C on the developer's terminal: the pty turns it into SIGINT for `exec`.
    running.type_line("\u{3}");
    let out = running.finish();
    assert!(
        !out.status.success(),
        "exec should not succeed: {}",
        text(&out)
    );
    assert_eq!(feat_x_remote(&m), before, "git was not launched");
    let entries = log_entries(&m);
    assert_eq!(
        of_kind(&entries, "exception-cancelled").len(),
        1,
        "GRD006-CTRL-C: Ctrl-C should leave exception-cancelled: {entries:#?}"
    );
    assert!(
        exception_audit(&m).contains(&"cancelled".to_owned()),
        "GRD006-CTRL-C: the audit should say cancelled"
    );
}

// ---- Escenario 4 ----

#[test]
fn the_exception_covers_one_operation() {
    let m = machine(300);
    let out = force_push(&m).finish();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let pushed = feat_x_remote(&m);
    // Another process rewrites feat-x and force-pushes: denied as before.
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    m.git_ok(
        &m.f.repo,
        &["commit", "-q", "--amend", "-m", "feat-x rewritten again"],
    );
    let second = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
    assert!(
        !second.status.success(),
        "GRD006-ONE-OPERATION: the second force-push should be denied: {}",
        text(&second)
    );
    assert!(text(&second).contains("feat-x"), "{}", text(&second));
    assert_eq!(feat_x_remote(&m), pushed);
    let entries = log_entries(&m);
    assert_eq!(
        of_kind(&entries, "exception").len(),
        1,
        "GRD006-ONE-OPERATION: still one exception entry: {entries:#?}"
    );
}

// ---- Escenario 5 ----

#[test]
fn an_agent_cannot_use_the_exception() {
    let m = machine(300);
    let before = feat_x_remote(&m);
    let repo = repo(&m);
    let out = m.as_agent(&[
        "guard", "exec", "--yes", "--", "git", "-C", &repo, "push", "-q", "-f", "origin", "feat-x",
    ]);
    assert!(
        !out.status.success(),
        "the agent's exec succeeded: {}",
        text(&out)
    );
    assert_eq!(feat_x_remote(&m), before, "git was not launched");
    let entries = log_entries(&m);
    assert_eq!(
        of_kind(&entries, "exception-rejected").len(),
        1,
        "GRD006-AGENT: one exception-rejected entry: {entries:#?}"
    );
    assert!(of_kind(&entries, "exception").is_empty());
    let audit = exception_audit(&m);
    assert!(
        audit.contains(&"rejected".to_owned()),
        "GRD006-AGENT: the audit should say rejected: {audit:?}"
    );
}

/// The `tools/list` answer of `raptor-mcp`.
fn mcp_tools(m: &Machine) -> String {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let mcp = gitraptor_testkit::sibling_bin(
        Path::new(guard_machine::RAPTOR),
        "gitraptor-mcp",
        "raptor-mcp",
    );
    let mut child = Command::new(mcp)
        .env_clear()
        .envs(m.env())
        .current_dir(&m.f.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |v: Value| writeln!(stdin, "{v}").unwrap();
    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "claude-code", "version": "2.1.284"}}}),
    );
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    line.clear();
    stdout.read_line(&mut line).unwrap();
    drop(stdin);
    let _ = child.wait();
    line
}

#[test]
fn the_mcp_never_reaches_the_exception() {
    let m = machine(300);
    let mut mcp = Client::connect(&m.dirs(), ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    let err = mcp
        .call::<_, Value>(
            methods::GUARD_EXCEPTION,
            json!({"path": repo(&m), "argv": ["git", "push", "-f", "origin", "feat-x"]}),
        )
        .unwrap_err();
    let ClientError::Rpc(err) = err else {
        panic!("{err:?}")
    };
    assert_eq!(err.code, code::RESERVED_REFUSED, "{err:?}");
    let data: RefusedData = serde_json::from_value(err.data.unwrap()).unwrap();
    assert_eq!(data.reason, RefusalReason::NotAvailableToMcp);
    drop(mcp);
    let entries = log_entries(&m);
    let rejected = of_kind(&entries, "exception-rejected");
    assert_eq!(
        rejected.len(),
        1,
        "GRD006-MCP: the MCP attempt should leave exception-rejected: {entries:#?}"
    );
    assert_eq!(
        rejected[0]["layer"], "mcp",
        "GRD006-MCP: the layer should be mcp: {}",
        rejected[0]
    );
    assert!(
        exception_audit(&m).contains(&"rejected".to_owned()),
        "GRD006-MCP: the attempt is audited"
    );
    let tools = mcp_tools(&m);
    assert!(
        tools.contains("\"tools\"") && !tools.to_lowercase().contains("exception"),
        "the MCP catalog should not offer the exception: {tools}"
    );
}

// ---- Escenario 6 ----

#[test]
fn an_unsupported_operation_is_refused_before_the_announcement() {
    let m = Machine::with_window(300);
    m.f.write(".gitraptor/settings.json", DENY_COMMIT);
    m.git_ok(&m.f.repo, &["add", ".gitraptor/settings.json"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "team settings"]);
    m.add(&m.f.repo);
    assert!(m.protect(&m.f.repo).status.success());
    let head = m.git_ok(&m.f.repo, &["rev-parse", "HEAD"]);
    m.f.write("new.txt", "n\n");
    m.git_ok(&m.f.repo, &["add", "new.txt"]);

    let out = exec(&m, &["commit", "-q", "-m", "x"]).finish();
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out).to_lowercase();
    assert!(
        ["push", "branch", "rebase"]
            .iter()
            .all(|w| shown.contains(w)),
        "GRD006-UNSUPPORTED: the text should say what the exception admits (push, branch, rebase): {shown}"
    );
    assert_eq!(
        m.git_ok(&m.f.repo, &["rev-parse", "HEAD"]),
        head,
        "nothing ran"
    );

    let out = exec(&m, &["pull"]).finish();
    assert!(!out.status.success(), "{}", text(&out));
    let shown = text(&out).to_lowercase();
    assert!(
        shown.contains("fetch") && shown.contains("rebase"),
        "GRD006-UNSUPPORTED: for pull the text says git fetch + rebase: {shown}"
    );
    assert!(m.status_json(&m.f.repo)["pending"].is_null());
    let audit = exception_audit(&m);
    assert!(
        !audit.contains(&"accepted".to_owned()),
        "GRD006-UNSUPPORTED: nothing was announced: {audit:?}"
    );
}

// ---- Q-GRD-50 ----

#[test]
fn the_exception_skips_an_everyone_policy() {
    let m = Machine::with_window(300);
    m.f.write(".gitraptor/settings.json", EVERYONE_MAIN);
    m.git_ok(&m.f.repo, &["add", ".gitraptor/settings.json"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "team settings"]);
    m.git_ok(&m.f.repo, &["push", "-q", "origin", "main"]);
    m.git_ok(&m.f.repo, &["switch", "-q", "-c", "topic"]);
    m.f.write("topic.txt", "t\n");
    m.git_ok(&m.f.repo, &["add", "topic.txt"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "topic"]);
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    m.add(&m.f.repo);
    assert!(m.protect(&m.f.repo).status.success());
    let topic = m.git_ok(&m.f.repo, &["rev-parse", "topic"]);

    let denied = m.git(&m.f.repo, &["push", "-q", "origin", "topic:main"]);
    assert!(!denied.status.success(), "{}", text(&denied));
    assert_ne!(m.remote_ref("refs/heads/main").unwrap(), topic);

    let out = exec(&m, &["push", "-q", "origin", "topic:main"]).finish();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(m.remote_ref("refs/heads/main").unwrap(), topic);
    let entries = log_entries(&m);
    let used = of_kind(&entries, "exception");
    assert_eq!(used.len(), 1, "{entries:#?}");
    assert!(
        used[0].to_string().contains("policy.protected-branch"),
        "GRD006-EVERYONE-POLICY: the entry should name policy.protected-branch: {}",
        used[0]
    );
}

// ---- US-GRD-017: the exception without a recovery point ----

#[test]
fn exception_runs_without_a_recovery_point() {
    let m = Machine::with_window(300);
    m.extra_env.borrow_mut().extend([
        (
            "GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS",
            OsString::from("0"),
        ),
        ("LANG", OsString::from("es_ES.UTF-8")),
    ]);
    m.git_ok(&m.f.repo, &["branch", "topic", "feat-x"]);
    m.git_ok(
        &m.f.repo,
        &["commit", "-q", "--allow-empty", "-m", "main moves"],
    );
    m.add(&m.f.repo);
    assert!(m.protect(&m.f.repo).status.success());

    // Without the exception the failed prior denies both.
    for args in [
        &["branch", "-D", "topic"][..],
        &["rebase", "-q", "main", "feat-x"][..],
    ] {
        let out = m.git(&m.f.repo, args);
        assert!(!out.status.success(), "{args:?} ran: {}", text(&out));
    }
    let out = exec(&m, &["branch", "-D", "topic"]).finish();
    assert_eq!(
        out.status.code(),
        Some(0),
        "GRD006-NO-PRIOR: the branch delete should run under exec: {}",
        text(&out)
    );
    assert!(
        m.git(&m.f.repo, &["rev-parse", "--verify", "-q", "topic"])
            .stdout
            .is_empty(),
        "topic should be gone"
    );
    let out = exec(&m, &["rebase", "-q", "main", "feat-x"]).finish();
    assert_eq!(
        out.status.code(),
        Some(0),
        "GRD006-NO-PRIOR: the rebase should run under exec: {}",
        text(&out)
    );
    let entries = log_entries(&m);
    let used = of_kind(&entries, "exception");
    assert_eq!(used.len(), 2, "one entry per operation: {entries:#?}");
    for entry in &used {
        assert!(
            entry.to_string().contains("system.snapshot-failed"),
            "GRD006-NO-PRIOR: the entry should carry system.snapshot-failed: {entry}"
        );
    }
    let shown = text(&m.developer(&["guard", "log", &repo(&m)]));
    assert!(
        shown.contains("sin punto de recuperación"),
        "GRD006-NO-PRIOR: raptor guard log should say it ran without a recovery point: {shown}"
    );
}

// ---- D5 ----

#[test]
fn a_rule_not_announced_is_not_skipped() {
    let m = machine(4_000);
    let before = feat_x_remote(&m);
    let running = force_push(&m);
    wait_pending(&m);
    // Another configuration lands on `main` after the announcement: a rule it did not list.
    m.f.write(
        ".gitraptor/settings.json",
        r#"{"policies":{"protectedBranches":{"patterns":["feat-x"],"appliesTo":"everyone"}}}"#,
    );
    m.git_ok(&m.f.repo, &["add", ".gitraptor/settings.json"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "stricter policy"]);
    let out = running.finish();
    assert!(!out.status.success(), "the push ran: {}", text(&out));
    assert_eq!(feat_x_remote(&m), before, "the operation is denied");
    let entries = log_entries(&m);
    assert_eq!(
        of_kind(&entries, "exception-rejected").len(),
        1,
        "GRD006-RULE-NOT-ANNOUNCED: the presentation should leave exception-rejected: {entries:#?}"
    );
    assert!(of_kind(&entries, "exception").is_empty(), "{entries:#?}");
}

#[test]
fn an_allowed_operation_needs_no_exception() {
    let m = machine(300);
    m.git_ok(&m.f.repo, &["branch", "topic"]);
    let out = exec(&m, &["push", "-q", "origin", "topic"]).finish();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        m.remote_ref("refs/heads/topic").is_none(),
        "GRD006-NOT-DENIED: nothing may run when nothing denies the operation"
    );
    assert!(m.status_json(&m.f.repo)["pending"].is_null());
    assert!(
        text(&out).to_lowercase().contains("exception"),
        "GRD006-NOT-DENIED: the text should say no exception is needed: {}",
        text(&out)
    );
    assert!(!exception_audit(&m).contains(&"accepted".to_owned()));
}

// ---- D12 ----

#[test]
fn an_old_template_refuses_before_the_announcement() {
    let m = machine(300);
    let conf_path = m.common().join("gitraptor/dispatch.conf");
    let conf = std::fs::read_to_string(&conf_path).unwrap();
    let old: String = conf
        .lines()
        .map(|l| {
            if l.starts_with("template\t") {
                "template\t3"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&conf_path, old).unwrap();
    let before = feat_x_remote(&m);

    let out = force_push(&m).finish();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("guard install"),
        "GRD006-OLD-TEMPLATE: the refusal should say to reinstall with `raptor guard install`: {}",
        text(&out)
    );
    assert!(m.status_json(&m.f.repo)["pending"].is_null());
    assert_eq!(feat_x_remote(&m), before);

    let out = m.protect(&m.f.repo);
    assert!(out.status.success(), "{}", text(&out));
    let out = force_push(&m).finish();
    assert_eq!(
        out.status.code(),
        Some(0),
        "GRD006-OLD-TEMPLATE: exec should work after `raptor guard install`: {}",
        text(&out)
    );
}

// ---- Degraded ----

#[test]
fn no_exception_without_the_engine() {
    let m = machine(300);
    let before = feat_x_remote(&m);
    m.stop();
    let out = force_push(&m).finish();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).to_lowercase().contains("start"),
        "GRD006-NO-ENGINE: exec should say to start GitRaptor: {}",
        text(&out)
    );
    assert_eq!(feat_x_remote(&m), before, "nothing ran");
    let plain = m.git(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"]);
    assert!(
        !plain.status.success(),
        "a plain force-push stays denied without the engine: {}",
        text(&plain)
    );
    assert_eq!(feat_x_remote(&m), before);
}
