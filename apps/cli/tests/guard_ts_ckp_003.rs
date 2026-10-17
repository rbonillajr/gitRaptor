//! TS-CKP-003 at process level: the cockpit layer in Guardrails' decision, bound to the `git` the
//! executor launches. The real `raptor` daemon over a temporary profile (`GITRAPTOR_PROFILE_DIR`)
//! started with the debug-only test arms (`GITRAPTOR_TEST_EXECUTOR_ARMS=1`: commit, merge-into-base
//! fast-forward, create-worktree with a new branch, discard-worktree, atomic rebase-onto-base), the
//! real `raptor-hook` dispatcher and plain Git, over temporary repos, a bare remote and a
//! temporary profile (NFR-01). Never this repo nor the real profile.
//!
//! The tests speak only JSON-RPC (`operation.prepare`, `operation.run`, `guard.log`) and the CLI,
//! so they compile before the new types exist. Three kinds of client, all this test binary
//! (`rpc_client_entry`): the developer under a pty made with `script` (layer `cockpit`), an agent
//! (`raptor-fake-agent`, a copy of this binary the daemon knows as Claude Code: layer `mcp`), and
//! a plain process. No fixed waits: every child has a deadline.
//!
//! macOS only. Linux and Windows: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fingerprint::{Scope, Snapshot};
use gitraptor_testkit::fixture::{clone_binary, git_from_path};
use gitraptor_testkit::{Fixture, diff};
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
const RPC_ENV: &str = "RAPTOR_TEST_RPC";
const DEADLINE: Duration = Duration::from_secs(120);
/// The trailer `agents-commit` (the default) asks of an agent's commit.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";
/// `code::OPERATION_REJECTED`'s guardrails case (ADR-CKP-002 § 2).
const REJECTED: i64 = -32014;

const PROTECT_MAIN_EVERYONE: &str =
    r#"{"policies":{"protectedBranches":{"patterns":["main"],"appliesTo":"everyone"}}}"#;
const PROTECT_PX_EVERYONE: &str =
    r#"{"policies":{"protectedBranches":{"patterns":["protected-x"],"appliesTo":"everyone"}}}"#;
const FORBID_AGENTS: &str = r#"{"policies":{"forbiddenPaths":{"patterns":["agents/"]}}}"#;
const WARN_AGENT_COMMIT: &str =
    r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#;

/// The simulated agent: runs `RAPTOR_FAKE_AGENT_ARGV` as its child.
#[test]
fn fake_agent_entry() {
    let Some(argv) = std::env::var_os(FAKE_AGENT_ARGV) else {
        return;
    };
    let argv: Vec<String> = serde_json::from_str(argv.to_str().unwrap()).unwrap();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .env_remove(FAKE_AGENT_ARGV)
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

/// The client: `{"root", "steps": [{"run": <prepare params>} | {"call": [method, params]}]}`.
/// A `run` step prepares the plan and runs it whatever the decision of the plan was (a denial
/// is answered by `operation.run`). Prints the answers as one JSON line; an error keeps its code
/// and `data`.
#[test]
fn rpc_client_entry() {
    let Some(spec) = std::env::var_os(RPC_ENV) else {
        return;
    };
    let spec: Value = serde_json::from_str(spec.to_str().unwrap()).unwrap();
    let dirs = ProfileDirs::under_root(PathBuf::from(spec["root"].as_str().unwrap()));
    let mut client = Client::connect(&dirs, ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    let answer = |r: Result<Value, ClientError>| match r {
        Ok(v) => json!({ "ok": v }),
        Err(ClientError::Rpc(e)) => json!({ "code": e.code, "data": e.data }),
        Err(e) => json!({ "other": e.to_string() }),
    };
    let mut answers = Vec::new();
    for step in spec["steps"].as_array().unwrap() {
        if let Some(call) = step.get("call") {
            answers.push(answer(
                client.call(call[0].as_str().unwrap(), call[1].clone()),
            ));
            continue;
        }
        let prepared = answer(client.call(methods::OPERATION_PREPARE, step["run"].clone()));
        let ran = match prepared.get("ok") {
            Some(plan) => answer(client.call(
                methods::OPERATION_RUN,
                json!({
                    "plan_id": plan["plan_id"],
                    "accepted_warnings": plan["warnings"],
                    "confirmation": plan["challenge"],
                }),
            )),
            None => Value::Null,
        };
        answers.push(json!({ "prepare": prepared, "run": ran }));
    }
    println!("RPC-ANSWERS {}", serde_json::to_string(&answers).unwrap());
}

#[derive(Clone, Copy)]
enum As {
    /// The developer's terminal: a pty, no agent above (layer `cockpit`).
    Developer,
    /// The simulated Claude Code (layer `mcp`).
    Agent,
}

#[derive(Default)]
struct Opts {
    /// Team settings committed on `main` (the floor).
    settings: Option<&'static str>,
    /// Skip `raptor guard install` (D12).
    no_install: bool,
    /// The user's own `pre-commit`, written before the install so the dispatcher chains it.
    pre_commit: Option<String>,
    /// `main` and `feat-a` change `conflict.txt` differently.
    conflict: bool,
}

/// "demo" with `main`, the linked worktree `wt-feat-a` (branch `feat-a`, one commit ahead of
/// `main`: a fast-forward), `wt-px` (branch `protected-x`), a bare remote with `main` and
/// `feat-x`, and the local branch `rewritten` that cannot replace `feat-x` without a force.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    remote: PathBuf,
    wt: PathBuf,
    px: PathBuf,
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// Runs `cmd` to the end within [`DEADLINE`]; never waits without a bound.
fn run_within(mut cmd: Command, what: &str) -> Output {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(DEADLINE) {
        Ok(out) => out.unwrap(),
        Err(_) => {
            let _ = Command::new("/bin/kill")
                .args(["-9", &pid.to_string()])
                .status();
            panic!("{what} did not finish within {DEADLINE:?}");
        }
    }
}

impl Machine {
    fn new(o: Opts) -> Self {
        #[cfg(not(debug_assertions))]
        panic!(
            "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
        );
        let f = Fixture::with_commit(&git_from_path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        if let Some(settings) = o.settings {
            f.write(".gitraptor/settings.json", settings);
            f.git(&["add", "-A"]);
            f.git(&["commit", "-q", "-m", "team settings"]);
        }
        if o.conflict {
            f.write("conflict.txt", "base\n");
            f.git(&["add", "conflict.txt"]);
            f.git(&["commit", "-q", "-m", "conflict base"]);
        }
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.write("x.txt", "x\n");
        f.git(&["add", "x.txt"]);
        f.git(&["commit", "-q", "-m", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        f.git(&["switch", "-q", "main"]);
        f.git(&["switch", "-q", "-c", "rewritten"]);
        f.write("y.txt", "y\n");
        f.git(&["add", "y.txt"]);
        f.git(&["commit", "-q", "-m", "rewritten"]);
        f.git(&["switch", "-q", "main"]);
        f.git(&["branch", "feat-a"]);
        f.git(&["branch", "protected-x"]);
        let wt = f.add_worktree("feat-a", "feat-a");
        let px = f.add_worktree("px", "protected-x");
        let side = if o.conflict {
            "feat side\n"
        } else {
            "feature\n"
        };
        std::fs::write(wt.join("conflict.txt"), side).unwrap();
        f.git_in(&wt, &["add", "conflict.txt"]);
        f.git_in(&wt, &["commit", "-q", "-m", "feat-a work"]);
        if o.conflict {
            f.write("conflict.txt", "main side\n");
            f.git(&["add", "conflict.txt"]);
            f.git(&["commit", "-q", "-m", "main moved"]);
        }
        if let Some(body) = &o.pre_commit {
            gitraptor_testkit::hooks::write_script(
                &f.repo.join(".git/hooks/pre-commit"),
                &format!("#!/bin/sh\n{body}\nexit 0\n"),
            );
        }
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self {
            f,
            outside,
            remote,
            wt,
            px,
        };
        let repo = m.f.repo.to_str().unwrap().to_owned();
        m.ok(m.developer(&["repo", "add", &repo]));
        m.ok(m.developer(&["mcp", "enable", &repo]));
        if !o.no_install {
            m.ok(m.developer(&["guard", "install", "--yes", &repo]));
        }
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1".into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("PATH", "/usr/bin:/bin".into()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    /// `raptor <args>` from the developer's own terminal: a pty through `script`.
    fn developer(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new("/usr/bin/script");
        cmd.args(["-q", "/dev/null", RAPTOR])
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root);
        run_within(cmd, "raptor")
    }

    /// The client of [`rpc_client_entry`] as `who`, with `steps`; the answers in order.
    fn client(&self, who: As, steps: Value) -> Vec<Value> {
        let me = std::env::current_exe().unwrap();
        let entry = [
            "rpc_client_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ];
        let spec = json!({ "root": self.f.profile, "steps": steps });
        let client_argv: Vec<String> = std::iter::once(me.to_string_lossy().into_owned())
            .chain(entry.iter().map(|a| (*a).to_owned()))
            .collect();
        let mut cmd = match who {
            As::Developer => {
                let mut c = Command::new("/usr/bin/script");
                c.args(["-q", "/dev/null"]).arg(&me).args(entry);
                c
            }
            As::Agent => {
                let agent = self.outside.path().join("bin").join(FAKE_AGENT);
                if !agent.exists() {
                    clone_binary(&me, &agent);
                }
                let mut c = Command::new(agent);
                c.args([
                    "fake_agent_entry",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ]);
                c
            }
        };
        cmd.env_clear()
            .envs(self.env())
            .env(RPC_ENV, spec.to_string())
            .current_dir(&self.f.root);
        if matches!(who, As::Agent) {
            cmd.env(
                FAKE_AGENT_ARGV,
                serde_json::to_string(&client_argv).unwrap(),
            );
        }
        let out = run_within(cmd, "rpc client");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout
            .lines()
            .find_map(|l| {
                l.split_once("RPC-ANSWERS ")
                    .map(|(_, j)| j.trim().to_owned())
            })
            .unwrap_or_else(|| panic!("no answers: {} {}", stdout, text(&out)));
        serde_json::from_str(&line).unwrap()
    }

    /// One governed operation of `wt` as `who`: `{"prepare": answer, "run": answer}`.
    fn op(&self, who: As, operation: &str, wt: &Path, args: Value) -> Value {
        let step = json!({"run": {
            "operation": operation, "worktree": wt, "args": args, "surface": "tui",
        }});
        self.client(who, json!([step])).remove(0)
    }

    /// The decisions of the log of "demo" (the install's own change of state is not one), with
    /// the layers a connection that asks for `guard.log-layers` sees.
    fn decisions(&self) -> Vec<Value> {
        let call = json!({"call": [methods::GUARD_LOG, {"path": self.f.repo}]});
        let log = self.client(As::Developer, json!([call])).remove(0);
        log["ok"]["entries"]
            .as_array()
            .unwrap_or_else(|| panic!("no log: {log}"))
            .iter()
            .filter(|e| e["kind"] != "protection-state")
            .cloned()
            .collect()
    }

    fn git_out(&self, dir: &Path, args: &[&str]) -> String {
        self.f.git_in(dir, args).trim().to_owned()
    }

    fn rev(&self, dir: &Path, what: &str) -> String {
        self.git_out(dir, &["rev-parse", what])
    }

    fn remote_rev(&self, name: &str) -> String {
        self.git_out(&self.remote, &["rev-parse", name])
    }

    /// Stages `rela` with `content` in worktree `wt`.
    fn stage(&self, wt: &Path, rela: &str, content: &str) {
        let p = wt.join(rela);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        self.f.git_in(wt, &["add", "--", rela]);
    }

    /// What a denial must leave as it was: the repo and both worktrees, file by file.
    fn footprint(&self) -> Snapshot {
        Snapshot::take(
            &[
                Scope::new("repo", &self.f.repo),
                Scope::new("wt", &self.wt),
                Scope::new("px", &self.px),
            ],
            &Default::default(),
        )
    }

    /// The entries `raptor timeline --json` shows from `wt`: snapshots and operations.
    fn timeline(&self) -> Value {
        let mut cmd = Command::new("/usr/bin/script");
        cmd.args(["-q", "/dev/null", RAPTOR, "timeline", "--json"])
            .env_clear()
            .envs(self.env())
            .current_dir(&self.wt);
        let out = self.ok(run_within(cmd, "raptor timeline"));
        let shown = text(&out);
        let start = shown
            .find('{')
            .unwrap_or_else(|| panic!("no JSON: {shown}"));
        let timeline: Value = serde_json::from_str(shown[start..].trim())
            .unwrap_or_else(|e| panic!("not JSON ({e}): {shown}"));
        timeline["entries"].clone()
    }

    fn commit_message(&self, what: &str) -> String {
        format!("feat: {what}\n\n{CLAUDE}\n")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

fn rule0(answer: &Value) -> &Value {
    &answer["data"]["guardrails"]["reasons"][0]["rule"]
}

fn outcome(run: &Value) -> &Value {
    &run["ok"]["outcome"]
}

/// §9.4 Registro único: an allowed merge leaves no entry; a protected `discard-worktree` leaves
/// one `denial` with layer `cockpit`; an agent's commit with a warning leaves one `notice`
/// although `pre-commit`, `commit-msg` and `reference-transaction` all ask.
#[test]
fn a_plan_leaves_at_most_one_entry() {
    // An allowed merge (fast-forward) with no rule: no entry.
    let m = Machine::new(Opts::default());
    let merged = m.op(As::Developer, "merge-into-base", &m.wt, json!({}));
    assert_eq!(outcome(&merged["run"]), "done", "{merged:#}");
    assert!(m.decisions().is_empty(), "{:#?}", m.decisions());
    drop(m);

    // A protected branch for everyone: one denial of the cockpit layer.
    let m = Machine::new(Opts {
        settings: Some(PROTECT_PX_EVERYONE),
        ..Opts::default()
    });
    let discarded = m.op(As::Developer, "discard-worktree", &m.px, json!({}));
    assert_eq!(discarded["run"]["code"], REJECTED, "{discarded:#}");
    assert_eq!(rule0(&discarded["run"]), "policy.protected-branch");
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial");
    assert_eq!(entries[0]["layer"], "cockpit");
    assert_eq!(entries[0]["count"], 1);
    drop(m);

    // An agent's commit with a warning: one notice.
    let m = Machine::new(Opts {
        settings: Some(WARN_AGENT_COMMIT),
        ..Opts::default()
    });
    m.stage(&m.wt, "note.txt", "note\n");
    let msg = m.commit_message("warned");
    let committed = m.op(As::Agent, "commit", &m.wt, json!({ "message": msg }));
    assert_eq!(outcome(&committed["run"]), "done", "{committed:#}");
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "notice");
    assert_eq!(entries[0]["layer"], "mcp");
    assert_eq!(entries[0]["count"], 1);
}

/// §9.4 Antes de efectos: a denied plan leaves the repo's footprint as it was, no new point in
/// the timeline and no intent in the oplog.
#[test]
fn a_denied_plan_changes_nothing_and_takes_no_snapshot() {
    let m = Machine::new(Opts {
        settings: Some(PROTECT_MAIN_EVERYONE),
        ..Opts::default()
    });
    let before = m.footprint();
    let timeline = m.timeline();
    let denied = m.op(As::Developer, "merge-into-base", &m.wt, json!({}));
    assert_eq!(denied["run"]["code"], REJECTED, "{denied:#}");
    let changes = diff(&before, &m.footprint());
    assert!(changes.is_empty(), "{changes:#?}");
    assert_eq!(
        m.timeline(),
        timeline,
        "a denied plan is not in the timeline"
    );
}

/// §9.4 Lo no impedible: nothing in Git's hooks could stop a merge into a protected base; the
/// executor denies it with `-32014` and `policy.protected-branch`.
#[test]
fn a_merge_into_a_protected_base_is_denied_by_the_executor() {
    let m = Machine::new(Opts {
        settings: Some(PROTECT_MAIN_EVERYONE),
        ..Opts::default()
    });
    let main = m.rev(&m.f.repo, "main");
    let denied = m.op(As::Developer, "merge-into-base", &m.wt, json!({}));
    assert_eq!(denied["run"]["code"], REJECTED, "{denied:#}");
    assert_eq!(denied["run"]["data"]["reason"], "guardrails-denied");
    assert_eq!(
        rule0(&denied["run"]),
        "policy.protected-branch",
        "{denied:#}"
    );
    assert_eq!(m.rev(&m.f.repo, "main"), main);
}

/// §9.4 Ligadura (nieto, H-01): the user's `pre-commit` runs `git push --force` during an agent's
/// commit. That `git` is a grandchild: the minimum denies it, with its own entry of layer `hooks`
/// and the actor of the plan.
#[test]
fn a_grandchild_git_is_evaluated_with_the_plan_requester() {
    let git = git_from_path();
    let m = Machine::new(Opts {
        pre_commit: Some(format!(
            "'{}' push -q -f origin rewritten:feat-x >/dev/null 2>&1 || true",
            git.display()
        )),
        ..Opts::default()
    });
    let remote_feat_x = m.remote_rev("feat-x");
    m.stage(&m.wt, "note.txt", "note\n");
    let msg = m.commit_message("grandchild");
    let committed = m.op(As::Agent, "commit", &m.wt, json!({ "message": msg }));
    assert_eq!(outcome(&committed["run"]), "done", "{committed:#}");
    assert_eq!(
        m.remote_rev("feat-x"),
        remote_feat_x,
        "the force-push went through"
    );
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial");
    assert_eq!(entries[0]["layer"], "hooks");
    assert_eq!(entries[0]["actor"], "claude-code");
}

/// §9.4 Ligadura (transición distinta, D6): the user's `pre-commit` stages a forbidden path. The
/// new commit is not the registered transition: it is evaluated again with the plan's layer, the
/// branch does not move, one denial is left and the response carries the same `decision_id`.
#[test]
fn a_transition_other_than_the_registered_one_is_evaluated_again() {
    let git = git_from_path();
    let m = Machine::new(Opts {
        settings: Some(FORBID_AGENTS),
        pre_commit: Some(format!(
            "mkdir -p agents && echo secret > agents/new.txt\n'{}' add -- agents/new.txt",
            git.display()
        )),
        ..Opts::default()
    });
    let tip = m.rev(&m.wt, "HEAD");
    m.stage(&m.wt, "note.txt", "note\n");
    let msg = m.commit_message("other transition");
    let attempt = m.op(As::Agent, "commit", &m.wt, json!({ "message": msg }));
    assert!(attempt["prepare"]["ok"].is_object(), "{attempt:#}");
    assert_eq!(m.rev(&m.wt, "HEAD"), tip, "the branch moved: {attempt:#}");
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial");
    assert_eq!(entries[0]["layer"], "mcp");
    let run = &attempt["run"];
    let carried = &run["data"]["guardrails"]["decisionId"];
    assert!(
        carried.is_string(),
        "no guardrails in the answer: {attempt:#}"
    );
    assert_eq!(carried, &entries[0]["decisionId"], "{attempt:#}");
}

/// §9.4 Ligadura (git interno): `create-worktree` with a new branch leaves no entry and ends
/// `done`, whether Git makes the branch in a subprocess or not.
#[test]
fn a_worktree_add_with_a_new_branch_leaves_no_second_entry() {
    let m = Machine::new(Opts::default());
    let new = m.f.root.join("wt-new-br");
    let created = m.op(
        As::Developer,
        "create-worktree",
        &m.f.repo,
        json!({ "branch": "new-br", "path": new }),
    );
    assert_eq!(outcome(&created["run"]), "done", "{created:#}");
    assert!(new.is_dir());
    m.rev(&m.f.repo, "refs/heads/new-br");
    assert!(m.decisions().is_empty(), "{:#?}", m.decisions());
}

/// §9.4 Abort del rebase atómico: a rebase that conflicts ends `conflict-reverted`, the branch is
/// at its previous tip and no entry is left (the abort child reuses the plan's decision).
#[test]
fn an_atomic_rebase_that_conflicts_is_evaluated_once() {
    let m = Machine::new(Opts {
        conflict: true,
        ..Opts::default()
    });
    let tip = m.rev(&m.wt, "feat-a");
    let rebased = m.op(As::Developer, "rebase-onto-base", &m.wt, json!({}));
    assert_eq!(outcome(&rebased["run"]), "conflict-reverted", "{rebased:#}");
    assert_eq!(m.rev(&m.wt, "feat-a"), tip);
    assert!(m.decisions().is_empty(), "{:#?}", m.decisions());
}

/// §9.4 Capa fijada por el daemon (M-03): the simulated agent starts a TUI-surface client; the
/// commit with a forbidden path is denied and the entry carries layer `mcp`.
#[test]
fn an_agent_plan_from_the_tui_is_evaluated_and_logged_as_mcp() {
    let m = Machine::new(Opts {
        settings: Some(FORBID_AGENTS),
        ..Opts::default()
    });
    let tip = m.rev(&m.wt, "HEAD");
    m.stage(&m.wt, "agents/a.txt", "a\n");
    let msg = m.commit_message("forbidden");
    let attempt = m.op(As::Agent, "commit", &m.wt, json!({ "message": msg }));
    assert_eq!(attempt["run"]["code"], REJECTED, "{attempt:#}");
    assert_eq!(m.rev(&m.wt, "HEAD"), tip);
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial");
    assert_eq!(entries[0]["layer"], "mcp");
    assert_eq!(entries[0]["actor"], "claude-code");
}

/// D12: a repo without `raptor guard install` is governed by the executor all the same: a merge
/// into `main` ends in `-32014` with `policy.protected-branch` (the union {main, main branch,
/// base}), and the log shows one denial of layer `cockpit`.
#[test]
fn a_repo_without_hooks_is_still_governed_by_the_executor() {
    let m = Machine::new(Opts {
        no_install: true,
        ..Opts::default()
    });
    let main = m.rev(&m.f.repo, "main");
    let denied = m.op(As::Developer, "merge-into-base", &m.wt, json!({}));
    assert_eq!(denied["run"]["code"], REJECTED, "{denied:#}");
    assert_eq!(
        rule0(&denied["run"]),
        "policy.protected-branch",
        "{denied:#}"
    );
    assert_eq!(m.rev(&m.f.repo, "main"), main);
    let entries = m.decisions();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0]["kind"], "denial");
    assert_eq!(entries[0]["layer"], "cockpit");
}
