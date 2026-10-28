//! INF-GRD-001 / ADR-GRD-007 validations 7 to 10 end to end (DS-US-GRD-006 T005, the suite that
//! blocks the merge): what the token of `raptor guard exec` binds, how the argv is normalized,
//! which executables are neutralized and that the token never leaks. Real `raptor`, daemon and
//! hook dispatcher over the temporary machine of `guard_machine` (NFR-01: temporary repos,
//! remotes, profile and daemon only). The daemon recognises only `raptor-fake-agent`.
//!
//! To touch the token the suite puts a wrapper where the daemon's Git goes
//! (`GITRAPTOR_TEST_GIT`, debug builds): it behaves as the real `git` unless the exception token
//! is in its environment. Then it writes the token down and, depending on `mode`, tries a copy
//! of it from a git that is a grandchild of the requester, changes what git is about to do, or
//! waits for the test to release it (bounded). Unix only; Windows is pending the
//! cross-platform validation.
#![cfg(unix)]

mod guard_machine;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_testkit::fixture::git_from_path;
use guard_machine::{Machine, Running, text};
use serde_json::Value;

#[test]
fn fake_agent_entry() {
    guard_machine::fake_agent_entry();
}

const TOKEN_ENV: &str = "GITRAPTOR_EXCEPTION_TOKEN";

struct Rig {
    m: Machine,
    cap: tempfile::TempDir,
}

impl Rig {
    /// The machine with the wrapper as the daemon's Git; nothing is started yet.
    fn new(window_ms: u64, env: &[(&'static str, &str)]) -> Self {
        let m = Machine::with_window(window_ms);
        let cap = tempfile::tempdir().unwrap();
        let wrapper = cap.path().join("bin/git");
        let body = format!(
            r#"#!/bin/sh
REAL='{real}'
CAP='{cap}'
REPO='{repo}'
if [ -n "${TOKEN_ENV}" ]; then
  printf '%s' "${TOKEN_ENV}" > "$CAP/token"
  mode=$(cat "$CAP/mode" 2>/dev/null)
  case "$mode" in
    grandchild)
      ( "$REAL" -C "$REPO" push -q -f origin feat-x > "$CAP/grand.out" 2>&1; echo $? > "$CAP/grand.exit" )
      ;;
    control)
      "$REAL" -C "$REPO" status -s > /dev/null 2>&1
      ;;
  esac
  case "$mode" in
    otherref)
      exec "$REAL" -C "$REPO" push -q -f origin feat-y
      ;;
    othervalue)
      env -u {TOKEN_ENV} "$REAL" -C "$REPO" update-ref refs/heads/feat-x "$(cat "$CAP/other")"
      exec "$REAL" "$@"
      ;;
  esac
  i=0
  while [ ! -e "$CAP/release" ] && [ "$i" -lt 300 ]; do i=$((i+1)); sleep 0.1; done
fi
exec "$REAL" "$@"
"#,
            real = git_from_path().display(),
            cap = cap.path().display(),
            repo = m.f.repo.display(),
        );
        m.script(&wrapper, &body);
        {
            let mut extra = m.extra_env.borrow_mut();
            extra.push(("GITRAPTOR_TEST_GIT", wrapper.into_os_string()));
            for (k, v) in env {
                extra.push((*k, OsString::from(*v)));
            }
        }
        Self { m, cap }
    }

    fn cap(&self, name: &str) -> PathBuf {
        self.cap.path().join(name)
    }

    fn mode(&self, mode: &str) {
        let _ = std::fs::remove_file(self.cap("release"));
        let _ = std::fs::remove_file(self.cap("token"));
        std::fs::write(self.cap("mode"), mode).unwrap();
    }

    fn release(&self) {
        std::fs::write(self.cap("release"), "").unwrap();
    }

    /// Registers and protects "demo", rewrites `feat-x` and goes back to `main`.
    fn start(&self) {
        let m = &self.m;
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.rewrite_feat_x();
        m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    }

    /// Waits (bounded) for the wrapper to write something down.
    fn wait_for(&self, name: &str) -> String {
        let path = self.cap(name);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists()
            || std::fs::read_to_string(&path)
                .unwrap_or_default()
                .is_empty()
        {
            assert!(
                Instant::now() < deadline,
                "{name} never written: the wrapper did not see a token (is `raptor guard exec` there?)"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::read_to_string(&path).unwrap()
    }
}

fn repo(m: &Machine) -> String {
    m.f.repo.to_string_lossy().into_owned()
}

fn exec(m: &Machine, extra: &[&str]) -> Running {
    let repo = repo(m);
    let mut args = vec!["guard", "exec", "--yes", "--", "git", "-C", &repo];
    args.extend_from_slice(extra);
    m.developer_spawn(&args)
}

fn force_push(m: &Machine, branch: &str) -> Running {
    exec(m, &["push", "-q", "-f", "origin", branch])
}

fn remote(m: &Machine, branch: &str) -> String {
    m.remote_ref(&format!("refs/heads/{branch}"))
        .unwrap_or_else(|| panic!("{branch} on the remote"))
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

fn count(entries: &[Value], kind: &str) -> usize {
    entries.iter().filter(|e| e["kind"] == kind).count()
}

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

// ---- Validation 7 ----

#[test]
fn a_copied_token_is_rejected() {
    let rig = Rig::new(300, &[]);
    rig.start();
    let m = &rig.m;
    let before = remote(m, "feat-x");
    rig.mode("grandchild");
    let running = force_push(m, "feat-x");
    let token = rig.wait_for("token");
    // A git started by the bound git: a grandchild of the requester.
    let grand = rig.wait_for("grand.exit");
    assert_ne!(
        grand.trim(),
        "0",
        "GRD006-TOKEN-COPIED: the force-push of a grandchild git should be denied: {}",
        std::fs::read_to_string(rig.cap("grand.out")).unwrap_or_default()
    );
    assert_eq!(remote(m, "feat-x"), before, "the grandchild pushed");
    // Another git, with the variable set by hand.
    let out =
        m.f.git_command(&m.f.repo, &["push", "-q", "-f", "origin", "feat-x"])
            .env(TOKEN_ENV, &token)
            .output()
            .unwrap();
    assert!(
        !out.status.success(),
        "GRD006-TOKEN-COPIED: a git with the variable set by hand should be denied: {}",
        text(&out)
    );
    assert_eq!(remote(m, "feat-x"), before, "the copy pushed");
    rig.release();
    let _ = running.finish();
    let entries = log_entries(m);
    assert!(
        count(&entries, "exception-rejected") >= 1,
        "GRD006-TOKEN-COPIED: the copies should leave exception-rejected: {entries:#?}"
    );
}

#[test]
fn an_expired_token_is_rejected() {
    let rig = Rig::new(300, &[("GITRAPTOR_TEST_GUARD_EXCEPTION_TTL_MS", "200")]);
    rig.start();
    let m = &rig.m;
    let before = remote(m, "feat-x");
    rig.mode("expired");
    let running = force_push(m, "feat-x");
    rig.wait_for("token");
    // The token lives 200 ms: let it lapse before the git presents it.
    std::thread::sleep(Duration::from_millis(800));
    rig.release();
    let out = running.finish();
    assert!(
        !out.status.success(),
        "GRD006-TOKEN-EXPIRED: the expired token should be rejected: {}",
        text(&out)
    );
    assert_eq!(remote(m, "feat-x"), before);
    let entries = log_entries(m);
    assert_eq!(
        count(&entries, "exception-rejected"),
        1,
        "GRD006-TOKEN-EXPIRED: {entries:#?}"
    );
    assert_eq!(count(&entries, "exception"), 0, "{entries:#?}");
}

#[test]
fn another_transition_is_not_covered() {
    let rig = Rig::new(300, &[]);
    let m = &rig.m;
    // A second branch, pushed and then rewritten, so pushing it needs a force too.
    m.git_ok(&m.f.repo, &["switch", "-q", "-c", "feat-y", "main"]);
    m.f.write("y.txt", "y\n");
    m.git_ok(&m.f.repo, &["add", "y.txt"]);
    m.git_ok(&m.f.repo, &["commit", "-q", "-m", "feat-y"]);
    m.git_ok(&m.f.repo, &["push", "-q", "origin", "feat-y"]);
    m.git_ok(
        &m.f.repo,
        &["commit", "-q", "--amend", "-m", "feat-y rewritten"],
    );
    m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
    rig.start();
    let (x, y) = (remote(m, "feat-x"), remote(m, "feat-y"));

    // Another ref: the token of feat-x does not cover feat-y.
    rig.mode("otherref");
    let out = force_push(m, "feat-x").finish();
    assert!(
        !out.status.success(),
        "GRD006-TOKEN-OTHER-TRANSITION: another ref should not be covered: {}",
        text(&out)
    );
    assert!(
        rig.cap("token").exists(),
        "GRD006-TOKEN-OTHER-TRANSITION: the exception should have issued a token for feat-x: {}",
        text(&out)
    );
    assert_eq!((remote(m, "feat-x"), remote(m, "feat-y")), (x.clone(), y));

    // Another new value for the same ref.
    let tree = m.git_ok(&m.f.repo, &["rev-parse", "feat-x^{tree}"]);
    let other = m.git_ok(
        &m.f.repo,
        &["commit-tree", &tree, "-p", "main", "-m", "other"],
    );
    std::fs::write(rig.cap("other"), other).unwrap();
    rig.mode("othervalue");
    let out = force_push(m, "feat-x").finish();
    assert!(
        !out.status.success(),
        "GRD006-TOKEN-OTHER-TRANSITION: another new value should not be covered: {}",
        text(&out)
    );
    assert!(
        rig.cap("token").exists(),
        "GRD006-TOKEN-OTHER-TRANSITION: the second exception should have issued a token: {}",
        text(&out)
    );
    assert_eq!(remote(m, "feat-x"), x);
    let entries = log_entries(m);
    assert_eq!(
        count(&entries, "exception-rejected"),
        2,
        "GRD006-TOKEN-OTHER-TRANSITION: {entries:#?}"
    );
    assert_eq!(count(&entries, "exception"), 0, "{entries:#?}");
}

// ---- Validation 8 ----

#[test]
fn argv_is_normalized_or_refused_before_the_token() {
    let rig = Rig::new(60_000, &[]);
    let m = &rig.m;
    // A second remote for the triangular flow, with feat-x behind.
    let fork = m.f.root.join("fork.git");
    m.f.git_in(&m.f.root, &["init", "-q", "--bare", fork.to_str().unwrap()]);
    m.git_ok(
        &m.f.repo,
        &["remote", "add", "fork", fork.to_str().unwrap()],
    );
    m.git_ok(&m.f.repo, &["push", "-q", "fork", "feat-x"]);
    rig.start();
    m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
    m.git_ok(&m.f.repo, &["config", "branch.feat-x.remote", "origin"]);
    m.git_ok(
        &m.f.repo,
        &["config", "branch.feat-x.merge", "refs/heads/feat-x"],
    );
    rig.mode("announce-only");

    let accepted: [(&str, &[&str], &[&str]); 6] = [
        (
            "explicit refspec",
            &[],
            &["push", "-q", "-f", "origin", "feat-x"],
        ),
        (
            "explicit src:dst",
            &[],
            &["push", "-q", "-f", "origin", "feat-x:feat-x"],
        ),
        (
            "push.default=upstream",
            &["push.default", "upstream"],
            &["push", "-q", "-f"],
        ),
        (
            "push.default=current",
            &["push.default", "current"],
            &["push", "-q", "-f", "origin"],
        ),
        (
            "push.default=simple",
            &["push.default", "simple"],
            &["push", "-q", "-f"],
        ),
        (
            "triangular simple",
            &["remote.pushDefault", "fork"],
            &["push", "-q", "-f"],
        ),
    ];
    for (name, config, args) in accepted {
        if let [key, value] = config {
            m.git_ok(&m.f.repo, &["config", key, value]);
        }
        let running = exec(m, args);
        let status = wait_pending(m);
        assert!(
            status["pending"]["exception"]["operation"].is_object(),
            "GRD006-TOKEN-ARGV: {name} should be normalized to an operation: {status}"
        );
        assert!(
            !rig.cap("token").exists(),
            "{name}: no token while the window is open"
        );
        let cancel = m.developer(&["guard", "cancel", &repo(m)]);
        assert!(cancel.status.success(), "{}", text(&cancel));
        let _ = running.finish();
    }

    let url = m.remote.to_string_lossy().into_owned();
    m.git_ok(&m.f.repo, &["config", "push.default", "matching"]);
    let refused: [(&str, Vec<&str>); 8] = [
        ("push.default=matching", vec!["push", "-f"]),
        ("--all", vec!["push", "--all", "origin"]),
        ("--mirror", vec!["push", "--mirror", "origin"]),
        (
            "wildcard",
            vec!["push", "-f", "origin", "refs/heads/*:refs/heads/*"],
        ),
        ("URL", vec!["push", "-f", &url, "feat-x"]),
        (
            "-c",
            vec!["-c", "core.sshCommand=x", "push", "-f", "origin", "feat-x"],
        ),
        ("pull", vec!["pull"]),
        ("rebase -i", vec!["rebase", "-i", "main"]),
    ];
    for (name, args) in refused {
        let repo = repo(m);
        let mut full = vec!["guard", "exec", "--yes", "--", "git"];
        if args[0] == "-c" {
            full.extend(&args[..2]);
            full.extend(["-C", &repo]);
            full.extend(&args[2..]);
        } else {
            full.extend(["-C", &repo]);
            full.extend(&args);
        }
        let out = m.developer(&full);
        assert!(!out.status.success(), "{name}: {}", text(&out));
        assert!(
            !text(&out).contains("unrecognized subcommand"),
            "GRD006-TOKEN-ARGV: {name} should be refused by exec, not by the parser: {}",
            text(&out)
        );
        assert!(
            m.status_json(&m.f.repo)["pending"].is_null(),
            "GRD006-TOKEN-ARGV: {name} should be refused before the announcement"
        );
        assert!(
            !rig.cap("token").exists(),
            "GRD006-TOKEN-ARGV: {name} should be refused without a token"
        );
    }
}

// ---- Validation 9 ----

#[test]
fn configured_executables_are_neutralized() {
    let rig = Rig::new(300, &[("GITRAPTOR_TEST_TM_HOOK_PRIOR_DEADLINE_MS", "0")]);
    rig.start();
    let m = &rig.m;
    m.git_ok(
        &m.f.repo,
        &["commit", "-q", "--allow-empty", "-m", "main moves"],
    );
    // Canaries: they only leave a mark when the exception token is in their environment, so
    // the daemon or the person running them does not count.
    let canary = |name: &str| -> String {
        let path = rig.cap(&format!("canary-{name}"));
        let marker = rig.cap(&format!("ran-{name}"));
        m.script(
            &path,
            &format!(
                "#!/bin/sh\nif [ -n \"${TOKEN_ENV}\" ]; then touch '{}'; fi\nexit 0\n",
                marker.display()
            ),
        );
        path.to_string_lossy().into_owned()
    };
    let config = [
        ("core.pager", canary("pager")),
        ("core.fsmonitor", canary("fsmonitor")),
        ("sequence.editor", canary("sequence-editor")),
        ("diff.canary.textconv", canary("textconv")),
    ];
    for (key, value) in &config {
        m.git_ok(&m.f.repo, &["config", key, value]);
    }
    std::fs::write(m.common().join("info/attributes"), "*.txt diff=canary\n").unwrap();
    let ran = |name: &str| rig.cap(&format!("ran-{name}")).exists();

    // Control: the canary does run when Git is not neutralized.
    rig.mode("control");
    let running = exec(m, &["rebase", "-q", "main", "feat-x"]);
    rig.wait_for("token");
    for _ in 0..500 {
        if ran("fsmonitor") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        ran("fsmonitor"),
        "GRD006-TOKEN-NEUTRALIZED: the control git should run core.fsmonitor (the test is not sensitive otherwise), or `raptor guard exec` is missing"
    );
    for name in ["pager", "fsmonitor", "sequence-editor", "textconv"] {
        let _ = std::fs::remove_file(rig.cap(&format!("ran-{name}")));
    }
    // The hostile environment of the developer's shell, only for this exec.
    let hostile = [
        ("GIT_PAGER", canary("git-pager")),
        ("GIT_EDITOR", canary("git-editor")),
        ("GIT_EXTERNAL_DIFF", canary("external-diff")),
        ("GIT_DIR", "/nonexistent-gitraptor-dir".to_owned()),
    ];
    m.extra_env.borrow_mut().extend(
        hostile
            .iter()
            .map(|(k, v)| (*k, OsString::from(v.as_str()))),
    );
    rig.release();
    let out = running.finish();
    m.extra_env
        .borrow_mut()
        .retain(|(k, _)| !hostile.iter().any(|(h, _)| h == k));
    assert_eq!(
        out.status.code(),
        Some(0),
        "GRD006-TOKEN-NEUTRALIZED: the rebase should run under exec: {}",
        text(&out)
    );
    for name in [
        "pager",
        "fsmonitor",
        "sequence-editor",
        "textconv",
        "git-pager",
        "git-editor",
        "external-diff",
    ] {
        assert!(
            !ran(name),
            "GRD006-TOKEN-NEUTRALIZED: the canary {name} ran under exec"
        );
    }
}

// ---- Validation 10 ----

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => files_under(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

#[test]
fn the_token_never_leaks() {
    let rig = Rig::new(300, &[]);
    let m = &rig.m;
    // A hook of the developer, chained by the install: it must not see the token.
    let seen = rig.cap("prior-hook-env");
    m.script(
        &m.common().join("hooks/pre-push"),
        &format!(
            "#!/bin/sh\nprintf '%s' \"${{{TOKEN_ENV}-unset}}\" > '{}'\nexit 0\n",
            seen.display()
        ),
    );
    rig.start();
    rig.mode("run");
    rig.release();
    let out = force_push(m, "feat-x").finish();
    assert_eq!(
        out.status.code(),
        Some(0),
        "GRD006-TOKEN-NEVER-LEAKS: the push should run under exec: {}",
        text(&out)
    );
    let token = rig.wait_for("token");
    assert_eq!(token.len(), 32, "a 128-bit token in hex: {token:?}");
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap_or_default(),
        "unset",
        "GRD006-TOKEN-NEVER-LEAKS: the chained prior hook should not see the token (and should run)"
    );
    let shown = [
        text(&out),
        text(&m.developer(&["guard", "log", "--json", &repo(m)])),
        text(&m.developer(&["guard", "status", "--json", &repo(m)])),
    ];
    for (i, s) in shown.iter().enumerate() {
        assert!(!s.contains(&token), "output {i} shows the token");
    }
    m.stop();
    let daemon_log = std::fs::read_to_string(m.dirs().state.join("daemon.log")).unwrap_or_default();
    assert!(
        !daemon_log.contains(&token),
        "the daemon log shows the token"
    );
    let mut files = Vec::new();
    files_under(&m.f.profile, &mut files);
    assert!(!files.is_empty(), "the profile is empty?");
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(
            !bytes.windows(token.len()).any(|w| w == token.as_bytes()),
            "GRD006-TOKEN-NEVER-LEAKS: the profile file {} holds the token",
            file.display()
        );
    }
}
