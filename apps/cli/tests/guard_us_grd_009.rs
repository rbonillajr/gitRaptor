//! US-GRD-009 end to end (DS-US-GRD-009 § 6): the diff size and the commit format with the real
//! `raptor` as daemon and hook, the real `raptor-hook` dispatcher and plain Git, over temporary
//! repos, a bare remote and a temporary profile (NFR-01). The agent is `raptor-fake-agent`, a
//! copy of this test binary the debug daemon knows as Claude Code: `git` runs under it
//! (agent -> sh -> git -> hook), so the daemon resolves the actor from the ancestry, never from
//! a claim. The person is `git` with no agent above it. No fixed waits, exact line counts.
//!
//! No unwritten Rust API is used here: these tests fail by assertion today, because the two keys
//! are still `policy-not-supported`.
//!
//! Verified on the runner's Git while writing (T001 Reglas): `git revert` writes `Revert "<s>"`
//! and `This reverts commit <oid>.`, a revert of a revert writes `Reapply "<s>"`; `diff.renameLimit`
//! is unset and a plain `git diff --numstat -M` detected 1 100 inexact renames, so the default
//! limit of that Git is above D7's 1 000 (see `crates/git/tests/fresh_commit_changes.rs`).
//!
//! Unix only; Windows has no channel transport yet (Pendiente: etapa de validación
//! multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::{copy_executable, git_from_path};
use serde_json::Value;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
const FAKE_AGENT: &str = "raptor-fake-agent";
/// The command the simulated agent runs with `/bin/sh`.
const AGENT_CMD: &str = "RAPTOR_FAKE_AGENT_CMD";
/// The trailer `agents-commit` (the default) asks of an agent's commit.
const CLAUDE: &str = "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>";

/// `--fixup` and `--squash` open an editor for the trailer; this one accepts the template.
const ED: &str = "GIT_EDITOR=true ";
const DIFF_400: &str = r#"{"policies":{"diffSize":{"maxLines":400}}}"#;
const FORMAT: &str = r#"{"policies":{"commitFormat":{}}}"#;
const BOTH: &str = r#"{"policies":{"diffSize":{"maxLines":400},"commitFormat":{}}}"#;
const LISTED: [&str; 11] = [
    "build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
];

/// Entry point of the simulated Claude Code: when this binary runs as `raptor-fake-agent` with
/// [`AGENT_CMD`], it runs that command and exits with its status. As a normal test, nothing.
#[test]
fn fake_agent_entry() {
    let Some(cmd) = std::env::var_os(AGENT_CMD) else {
        return;
    };
    let status = Command::new("/bin/sh").arg("-c").arg(cmd).status().unwrap();
    std::process::exit(status.code().unwrap_or(2));
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// Exactly `n` lines, each different.
fn lines(n: usize) -> String {
    (0..n).map(|i| format!("line {i}\n")).collect()
}

/// `raptor <args>` from the developer's own terminal: a pty through `script`.
fn developer_command(args: &[&str]) -> Command {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null", RAPTOR]).args(args);
        c
    } else {
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let line = std::iter::once(RAPTOR)
            .chain(args.iter().copied())
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new("script");
        c.args(["-q", "-e", "-c", &line, "/dev/null"]);
        c
    }
}

/// "demo" with `main`, a bare remote with `main` and `feat-x`, the team configuration `settings`
/// committed on `main` (the floor), `secrets/a.txt`, and the repo protected by `raptor guard
/// install`. The branch the tests work on is `feat-x`.
struct Machine {
    f: Fixture,
    outside: tempfile::TempDir,
    remote: std::path::PathBuf,
}

impl Machine {
    fn new(settings: &str) -> Self {
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
        f.write("secrets/a.txt", "a\n");
        f.write(".gitraptor/settings.json", settings);
        f.git(&["add", "-A"]);
        f.git(&["commit", "-q", "-m", "seed and team settings"]);
        let remote = f.root.join("remote.git");
        f.git_in(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        f.git(&["push", "-q", "origin", "main"]);
        f.git(&["switch", "-q", "-c", "feat-x"]);
        f.git(&["push", "-q", "origin", "feat-x"]);
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("bin")).unwrap();
        let m = Self { f, outside, remote };
        let out = m.developer(&["repo", "add", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let out = m.developer(&["guard", "install", "--yes", m.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
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
            ("PATH", "/usr/bin:/bin".into()),
            ("GITRAPTOR_TEST_GIT", git_from_path().into_os_string()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    fn developer(&self, args: &[&str]) -> Output {
        developer_command(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn git_path(&self) -> String {
        self.f.git.to_str().unwrap().to_owned()
    }

    fn git_line(&self, args: &str) -> String {
        format!("'{}' {args}", self.git_path())
    }

    /// `git <args>` by the developer: no agent in the ancestry ("unattributed").
    fn human(&self, args: &str) -> Output {
        self.human_sh(&self.git_line(args))
    }

    fn human_sh(&self, line: &str) -> Output {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(line)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.repo)
            .output()
            .unwrap()
    }

    /// `git <args>` run by the simulated Claude Code.
    fn agent(&self, args: &str) -> Output {
        self.agent_sh(&self.git_line(args))
    }

    /// A shell line run by the simulated Claude Code.
    fn agent_sh(&self, line: &str) -> Output {
        let agent = self.outside.path().join("bin").join(FAKE_AGENT);
        if !agent.exists() {
            copy_executable(&std::env::current_exe().unwrap(), &agent);
        }
        Command::new(agent)
            .args([
                "fake_agent_entry",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .envs(self.env())
            .env(AGENT_CMD, line)
            .current_dir(&self.f.repo)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, out: Output) -> Output {
        assert!(out.status.success(), "{}", text(&out));
        out
    }

    fn rev(&self, what: &str) -> String {
        let out = self.human(&format!("rev-parse {what}"));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Writes `files` (a path with `None` content is removed) and stages them.
    fn stage(&self, files: &[(&str, Option<&str>)]) {
        for (path, content) in files {
            match content {
                Some(content) => {
                    self.f.write(path, content);
                    self.ok(self.human(&format!("add -- '{path}'")));
                }
                None => {
                    self.ok(self.human(&format!("rm -q -- '{path}'")));
                }
            }
        }
    }

    /// Stages `files`, then runs the `git commit` `command` under the agent (or the person) with
    /// `-F <subject, a blank line and the trailer>`.
    fn commit(
        &self,
        agent: bool,
        files: &[(&str, Option<&str>)],
        subject: &str,
        command: &str,
    ) -> Output {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.stage(files);
        let msg = self.outside.path().join(format!("msg{n}"));
        std::fs::write(&msg, format!("{subject}\n\n{CLAUDE}\n")).unwrap();
        let args = format!("{command} -F '{}'", msg.to_str().unwrap());
        if agent {
            self.agent(&args)
        } else {
            self.human(&args)
        }
    }

    /// A well formed commit by the person (the policies of these tests govern the agent by
    /// default), to set the scene.
    fn person_commit(&self, files: &[(&str, Option<&str>)], subject: &str) -> String {
        self.ok(self.commit(false, files, subject, "commit -q"));
        self.rev("HEAD")
    }

    /// The branches, the index and the working tree: what a denial must leave as it was.
    fn state(&self) -> (String, String, String) {
        let text = |args: &str| String::from_utf8_lossy(&self.human(args).stdout).into_owned();
        (
            text("for-each-ref refs/heads"),
            text("diff --cached"),
            text("status --porcelain"),
        )
    }

    /// Undoes whatever a denied operation left half done.
    fn clean(&self) {
        let _ = self.human("rebase --abort");
        let _ = self.human("cherry-pick --abort");
        let _ = self.human("merge --abort");
        self.ok(self.human("reset -q --hard"));
        self.ok(self.human("clean -fdq"));
    }

    /// `raptor guard log --json` of "demo".
    fn log(&self) -> Value {
        let out = self.developer(&["guard", "log", "--json", self.f.repo.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The pty of `script` may echo control characters before the output.
        let line = stdout
            .lines()
            .find_map(|l| l.find('{').map(|i| &l[i..]))
            .unwrap_or_else(|| panic!("no JSON: {stdout}"));
        serde_json::from_str(line.trim()).unwrap()
    }

    fn blocked(&self) -> u64 {
        self.log()["summary"]["blocked"].as_u64().unwrap_or(0)
    }

    fn profile_settings(&self, json: &str) {
        std::fs::write(self.dirs().config.join("settings.json"), json).unwrap();
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"]);
        assert!(out.status.success(), "{}", text(&out));
        let start = Instant::now();
        while running_pid(&self.dirs().state).unwrap().is_some() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "daemon did not stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Ok(Some(pid)) = running_pid(&self.dirs().state) {
            let _ = Command::new("/bin/kill").arg(pid.to_string()).status();
        }
    }
}

/// Conventional Commits 1.0.0 as D8 spells it, for the example a denial gives: the test does not
/// call the production parser, so it fails by assertion and not by compilation.
fn follows_conventional_commits(line: &str) -> Option<&str> {
    let (head, description) = line.split_once(": ")?;
    let head = head.strip_suffix('!').unwrap_or(head);
    let (ty, scope) = match head.split_once('(') {
        Some((ty, rest)) => (ty, Some(rest.strip_suffix(')')?)),
        None => (head, None),
    };
    let mut chars = ty.chars();
    let valid_type =
        chars.next()?.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '-');
    let valid_scope =
        scope.is_none_or(|s| (1..=64).contains(&s.len()) && !s.contains(['(', ')', '\n']));
    (valid_type && valid_scope && !description.trim().is_empty()).then_some(ty)
}

/// The text between « and » of a denial that looks like an example («type: description»).
fn example_of(shown: &str) -> Option<String> {
    shown
        .split('«')
        .skip(1)
        .filter_map(|rest| rest.split_once('»').map(|(inside, _)| inside))
        .find(|inside| inside.contains(": "))
        .map(str::to_owned)
}

/// Esc. 1 / D7: with a limit of 400 an agent's commit of 400 lines runs and one of 1 200 does
/// not, also with `--no-verify` and with `commit-tree` + `update-ref`; the branch, the index and
/// the working tree stay as they were and the message stays in `COMMIT_EDITMSG`.
#[test]
fn diff_limit_decides_the_commit() {
    let m = Machine::new(DIFF_400);

    let l400 = lines(400);
    let out = m.commit(
        true,
        &[("four.txt", Some(&l400))],
        "feat: four hundred",
        "commit -q",
    );
    m.ok(out);

    let tip = m.rev("feat-x");
    let l1200 = lines(1200);
    let out = m.commit(
        true,
        &[("big.txt", Some(&l1200))],
        "feat: twelve hundred",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "a 1200-line commit must not run: {}",
        text(&out)
    );
    let shown = text(&out);
    assert!(
        shown.contains("1200"),
        "the reason names the lines: {shown}"
    );
    assert!(shown.contains("400"), "the reason names the limit: {shown}");
    assert_eq!(m.rev("feat-x"), tip, "the branch did not move");
    let state = m.state();
    assert!(state.1.contains("+++ b/big.txt"), "still staged: {state:?}");
    assert!(
        m.f.repo.join("big.txt").is_file(),
        "still in the working tree"
    );
    let editmsg = std::fs::read_to_string(m.f.repo.join(".git/COMMIT_EDITMSG")).unwrap();
    assert!(editmsg.contains("feat: twelve hundred"), "{editmsg:?}");
    m.clean();

    // `--no-verify` skips the hooks of the commit, not the ref update.
    let out = m.commit(
        true,
        &[("big2.txt", Some(&l1200))],
        "feat: again",
        "commit -q --no-verify",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(text(&out).contains("1200"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    m.clean();

    // Plumbing: `commit-tree` + `update-ref`.
    m.f.write("big3.txt", &l1200);
    let g = m.git_path();
    let line = format!(
        "G='{g}'; $G add big3.txt && T=$($G write-tree) && \
         C=$($G commit-tree $T -p HEAD -m 'plumbing') && $G update-ref refs/heads/feat-x $C"
    );
    let out = m.agent_sh(&line);
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(text(&out).contains("1200"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
}

/// Esc. 2: "arreglos varios" with Conventional Commits does not run; the reason names Conventional
/// Commits, the list of types and an example that follows the grammar, and does not repeat the
/// message.
#[test]
fn an_unformatted_message_is_denied_with_a_valid_example() {
    let m = Machine::new(FORMAT);
    let tip = m.rev("feat-x");
    let out = m.commit(
        true,
        &[("x.txt", Some("x\n"))],
        "arreglos varios",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    let shown = text(&out);
    assert!(shown.contains("Conventional Commits"), "{shown}");
    for ty in LISTED {
        assert!(
            shown.contains(ty),
            "the list of types names `{ty}`: {shown}"
        );
    }
    let example = example_of(&shown).unwrap_or_else(|| panic!("no «example» in: {shown}"));
    let ty = follows_conventional_commits(&example)
        .unwrap_or_else(|| panic!("the example {example:?} does not follow the grammar"));
    assert!(
        LISTED.contains(&ty.to_ascii_lowercase().as_str()),
        "{example:?}"
    );
    assert!(
        !shown.contains("arreglos varios"),
        "the message is never repeated: {shown}"
    );
    assert_eq!(m.rev("feat-x"), tip);
    assert!(m.state().1.contains("x.txt"), "{:?}", m.state());
    let editmsg = std::fs::read_to_string(m.f.repo.join(".git/COMMIT_EDITMSG")).unwrap();
    assert!(
        editmsg.contains("arreglos varios"),
        "the message stays for the person: {editmsg:?}"
    );

    // The example is itself accepted.
    m.clean();
    let out = m.commit(true, &[("y.txt", Some("y\n"))], &example, "commit -q");
    m.ok(out);
}

/// Esc. 3.
#[test]
fn a_formatted_message_is_accepted() {
    let m = Machine::new(FORMAT);
    let tip = m.rev("feat-x");
    // Control: the policy is live, so what follows runs because it follows the format.
    let out = m.commit(
        true,
        &[("w.txt", Some("w\n"))],
        "arreglos varios",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "an unformatted message must be denied (the policy is not enforced): {}",
        text(&out)
    );
    m.clean();
    let out = m.commit(
        true,
        &[("x.txt", Some("x\n"))],
        "fix: handle empty branch name",
        "commit -q",
    );
    m.ok(out);
    assert_ne!(m.rev("feat-x"), tip, "the commit ran");
    let out = m.commit(
        true,
        &[("y.txt", Some("y\n"))],
        "feat(cli)!: scoped and breaking",
        "commit -q",
    );
    m.ok(out);
}

/// Esc. 4: no limit and no format at any level: 5 000 lines with "wip" run.
#[test]
fn nothing_is_required_without_configuration() {
    let m = Machine::new("{}");
    let tip = m.rev("feat-x");
    let big = lines(5000);
    let out = m.commit(true, &[("big.txt", Some(&big))], "wip", "commit -q");
    m.ok(out);
    assert_ne!(m.rev("feat-x"), tip);

    // Control: it ran because nothing is configured, not because the policies are not enforced.
    m.profile_settings(r#"{"policies":{"diffSize":{"maxLines":400}}}"#);
    let tip = m.rev("feat-x");
    let out = m.commit(true, &[("big2.txt", Some(&big))], "wip", "commit -q");
    assert!(
        !out.status.success(),
        "with a limit configured the same 5000 lines must be denied: {}",
        text(&out)
    );
    assert_eq!(m.rev("feat-x"), tip);
}

/// Esc. 5 / D3: the person passes with the default `appliesTo`; with `everyone`, not.
#[test]
fn the_person_passes_unless_everyone() {
    let m = Machine::new(BOTH);
    let big = lines(1200);
    let out = m.commit(false, &[("big.txt", Some(&big))], "wip", "commit -q");
    m.ok(out);
    m.stop();

    let m = Machine::new(
        r#"{"policies":{
            "diffSize":{"maxLines":400,"appliesTo":"everyone"},
            "commitFormat":{"appliesTo":"everyone"}}}"#,
    );
    let tip = m.rev("feat-x");
    let out = m.commit(false, &[("big.txt", Some(&big))], "wip", "commit -q");
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    let shown = text(&out);
    assert!(shown.contains("1200") && shown.contains("400"), "{shown}");
    assert!(shown.contains("Conventional Commits"), "{shown}");
    assert_eq!(m.rev("feat-x"), tip);
    // The same commit of a well formed small size passes for the person.
    m.clean();
    let out = m.commit(
        false,
        &[("small.txt", Some("s\n"))],
        "feat: small",
        "commit -q",
    );
    m.ok(out);
}

/// D4: `effect: ask` denies the agent (`ask-unavailable`, it counts in the KPI); the person with
/// `everyone` passes with a notice and does not count.
#[test]
fn ask_denies_the_agent_and_warns_the_person() {
    let m = Machine::new(
        r#"{"policies":{"diffSize":{"maxLines":400,"effect":"ask","appliesTo":"everyone"}}}"#,
    );
    let tip = m.rev("feat-x");
    let big = lines(1200);
    let out = m.commit(true, &[("big.txt", Some(&big))], "feat: big", "commit -q");
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(
        text(&out).to_lowercase().contains("confirm"),
        "{}",
        text(&out)
    );
    assert_eq!(m.rev("feat-x"), tip);
    let log = m.log();
    let causes: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["reasons"].as_array().unwrap().iter())
        .filter(|r| r["rule"] == "policy.diff-size")
        .filter_map(|r| r["cause"].as_str())
        .collect();
    assert_eq!(causes, ["ask-unavailable"], "{log:#}");
    let blocked = log["summary"]["blocked"].as_u64().unwrap();
    assert_eq!(blocked, 1, "the agent's ask counts: {log:#}");
    m.clean();

    // The person: it runs, with the notice, and it does not count.
    let out = m.commit(false, &[("big.txt", Some(&big))], "feat: big", "commit -q");
    let out = m.ok(out);
    assert!(
        text(&out).to_lowercase().contains("confirm"),
        "the person is told the policy asks for confirmation: {}",
        text(&out)
    );
    assert_ne!(m.rev("feat-x"), tip, "the person's commit ran");
    assert_eq!(m.blocked(), blocked, "the person's pass does not count");
}

/// D6: every new commit is judged, whatever the subcommand: `rebase -i` that squashes small
/// commits into a big one, a `reword` to an unformatted message and the `cherry-pick` of a big
/// commit are denied; a fast-forward to an existing branch and a `pull` of what the remote
/// already holds are not.
#[test]
fn every_new_commit_is_evaluated_whatever_the_subcommand() {
    let m = Machine::new(BOTH);
    let (a, b, c) = (lines(200), lines(201), lines(202));
    m.person_commit(&[("a.txt", Some(&a))], "feat: one");
    m.person_commit(&[("b.txt", Some(&b))], "feat: two");
    let tip = m.person_commit(&[("c.txt", Some(&c))], "feat: three");
    let sequence = "GIT_SEQUENCE_EDITOR=\"sed -i.bak -e '2s/^pick/squash/' -e '3s/^pick/squash/'\" GIT_EDITOR=true";

    // rebase -i: 200 + 201 + 202 lines squashed into one commit.
    let out = m.agent_sh(&format!("{sequence} {}", m.git_line("rebase -q -i HEAD~3")));
    assert!(
        !out.status.success(),
        "a squash of 603 lines must be denied: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("603"),
        "the squashed commit's lines: {}",
        text(&out)
    );
    m.clean();
    assert_eq!(m.rev("feat-x"), tip, "the branch did not move");

    // reword of an old commit to an unformatted message.
    let editor = m.outside.path().join("editor.sh");
    std::fs::write(
        &editor,
        format!("#!/bin/sh\nprintf 'arreglos varios\\n\\n{CLAUDE}\\n' > \"$1\"\n"),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let reword = format!(
        "GIT_SEQUENCE_EDITOR=\"sed -i.bak -e '2s/^pick/reword/'\" GIT_EDITOR='{}'",
        editor.display()
    );
    let out = m.agent_sh(&format!("{reword} {}", m.git_line("rebase -q -i HEAD~3")));
    assert!(
        !out.status.success(),
        "a reword without format must be denied: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("Conventional Commits"),
        "{}",
        text(&out)
    );
    m.clean();
    assert_eq!(m.rev("feat-x"), tip);

    // cherry-pick of a big commit (the person made it on `side`).
    m.ok(m.human("switch -q -c side main"));
    let big = lines(600);
    let side = m.person_commit(&[("side.txt", Some(&big))], "feat: side");
    m.ok(m.human("switch -q feat-x"));
    let out = m.agent(&format!("cherry-pick {side}"));
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(text(&out).contains("600"), "{}", text(&out));
    m.clean();
    assert_eq!(m.rev("feat-x"), tip);

    // A fast-forward to a branch that already exists brings nothing new.
    m.ok(m.human("switch -q -c ff main"));
    m.ok(m.agent("merge -q --ff-only side"));
    assert_eq!(m.rev("ff"), side);
    m.ok(m.human("switch -q feat-x"));

    // A pull of what the remote holds: the commit is not new to the repo after the fetch.
    // The remote first receives the local commits, so the pull is a fast-forward.
    m.ok(m.human("push -q origin feat-x"));
    let clone = m.outside.path().join("clone");
    m.f.git_in(
        &m.f.root,
        &[
            "clone",
            "-q",
            m.remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    m.f.git_in(&clone, &["switch", "-q", "feat-x"]);
    std::fs::write(clone.join("remote.txt"), lines(600)).unwrap();
    m.f.git_in(&clone, &["add", "."]);
    m.f.git_in(&clone, &["commit", "-q", "-m", "wip: remote work"]);
    m.f.git_in(&clone, &["push", "-q", "origin", "feat-x"]);
    let out = m.agent("pull -q --ff-only origin feat-x");
    m.ok(out);
    assert!(
        m.f.repo.join("remote.txt").is_file(),
        "the pull brought the remote commit"
    );
}

/// Esc. 6 / D6, D9: a merge with "Merge branch 'x'" passes without counting its size; a normal
/// commit with "Merge stuff" does not.
#[test]
fn merges_are_judged_by_their_message_only() {
    let m = Machine::new(BOTH);
    m.ok(m.human("switch -q -c side main"));
    let side = lines(600);
    m.person_commit(&[("side.txt", Some(&side))], "feat: side");
    m.ok(m.human("switch -q feat-x"));
    let other = lines(500);
    m.person_commit(&[("other.txt", Some(&other))], "feat: other");

    let out = m.agent(&format!(
        "merge -q --no-ff -m \"Merge branch 'side'\" -m '{CLAUDE}' side"
    ));
    m.ok(out);
    assert_eq!(m.rev("HEAD^2"), m.rev("side"), "it is a real merge commit");

    let out = m.commit(true, &[("n.txt", Some("n\n"))], "Merge stuff", "commit -q");
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("Conventional Commits"),
        "{}",
        text(&out)
    );
}

/// Esc. 6 / D9: `git revert` (and the `Reapply` of a revert), `commit --fixup` and `--squash` of
/// a commit with format pass.
#[test]
fn git_written_messages_pass() {
    let m = Machine::new(FORMAT);
    m.person_commit(&[("base.txt", Some("base\n"))], "feat: base");

    m.ok(m.agent("revert --no-edit HEAD"));
    let revert = String::from_utf8_lossy(&m.human("log -1 --format=%s").stdout)
        .trim()
        .to_owned();
    assert_eq!(revert, "Revert \"feat: base\"");
    m.ok(m.agent("revert --no-edit HEAD"));
    let reapply = String::from_utf8_lossy(&m.human("log -1 --format=%s").stdout)
        .trim()
        .to_owned();
    assert_eq!(
        reapply, "Reapply \"feat: base\"",
        "Git 2.43+ writes Reapply for a revert of a revert"
    );

    let trailer = format!("--trailer '{CLAUDE}'");
    m.stage(&[("fix.txt", Some("fix\n"))]);
    m.ok(m.agent_sh(&format!(
        "{ED}{}",
        m.git_line(&format!("commit -q --fixup=HEAD {trailer}"))
    )));
    m.stage(&[("squash.txt", Some("squash\n"))]);
    m.ok(m.agent_sh(&format!(
        "{ED}{}",
        m.git_line(&format!("commit -q --squash=HEAD {trailer}"))
    )));
    let subject = String::from_utf8_lossy(&m.human("log -1 --format=%s").stdout)
        .trim()
        .to_owned();
    assert!(subject.starts_with("squash! "), "{subject}");

    // The same kinds of message with a base that does not follow the format do not pass.
    m.ok(m.human("switch -q -c bad main"));
    m.person_commit(&[("bad.txt", Some("bad\n"))], "arreglos varios");
    m.stage(&[("bad2.txt", Some("bad\n"))]);
    let out = m.agent_sh(&format!(
        "{ED}{}",
        m.git_line(&format!("commit -q --fixup=HEAD {trailer}"))
    ));
    assert!(
        !out.status.success(),
        "a fixup of an unformatted subject is not exempt: {}",
        text(&out)
    );
}

/// D7: renaming a file of 2 000 lines counts 0, a binary counts 0 and `exclude: [Cargo.lock]`
/// does not count the lockfile; a plain big file still counts.
#[test]
fn renames_binaries_and_exclusions_are_not_lines() {
    let m = Machine::new(r#"{"policies":{"diffSize":{"maxLines":400,"exclude":["Cargo.lock"]}}}"#);
    let two_thousand = lines(2000);
    m.person_commit(&[("big.txt", Some(&two_thousand))], "feat: big file");

    // Rename: 0 lines.
    m.ok(m.human("mv big.txt moved.txt"));
    m.ok(m.human("add -A"));
    let out = m.commit(true, &[], "refactor: rename", "commit -q");
    m.ok(out);
    assert!(m.f.repo.join("moved.txt").is_file());

    // Binary: 0 lines, whatever its size.
    let mut blob = vec![0u8; 20_000];
    blob.extend((0..20_000u32).map(|i| (i % 251) as u8));
    std::fs::write(m.f.repo.join("blob.bin"), &blob).unwrap();
    m.ok(m.human("add blob.bin"));
    let out = m.commit(true, &[], "feat: add a binary", "commit -q");
    m.ok(out);

    // The lockfile is excluded: 5 000 lines of it and 10 of code.
    let lock = lines(5000);
    let code = lines(10);
    let out = m.commit(
        true,
        &[("Cargo.lock", Some(&lock)), ("src.txt", Some(&code))],
        "chore: update the lockfile",
        "commit -q",
    );
    m.ok(out);

    // Control: the same size in a file that is not excluded is over the limit.
    let tip = m.rev("feat-x");
    let out = m.commit(
        true,
        &[("other.lock", Some(&lock))],
        "chore: not excluded",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    assert!(text(&out).contains("5000"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
}

/// BR-CALC-001: size, format and a forbidden path in one commit: the three reasons in one decision.
#[test]
fn every_broken_rule_is_named_together() {
    let m = Machine::new(
        r#"{"policies":{
            "diffSize":{"maxLines":400},
            "commitFormat":{},
            "forbiddenPaths":{"patterns":["secrets/"]}}}"#,
    );
    let tip = m.rev("feat-x");
    let big = lines(1200);
    let out = m.commit(
        true,
        &[("secrets/new.txt", Some(&big))],
        "arreglos varios",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    let shown = text(&out);
    assert!(
        shown.contains("1200") && shown.contains("400"),
        "size: {shown}"
    );
    assert!(shown.contains("Conventional Commits"), "format: {shown}");
    assert!(shown.contains("«secrets/new.txt»"), "path: {shown}");
    assert_eq!(m.rev("feat-x"), tip);
}

/// US-GRD-010 filas 5 y 6: team 400 and personal 200 -> 200; team 400 and personal 1 000 -> 400.
#[test]
fn a_personal_level_only_lowers_the_limit() {
    let m = Machine::new(DIFF_400);
    m.profile_settings(r#"{"policies":{"diffSize":{"maxLines":200}}}"#);
    let tip = m.rev("feat-x");
    let l300 = lines(300);
    let out = m.commit(
        true,
        &[("a.txt", Some(&l300))],
        "feat: three hundred",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "a personal 200 holds: {}",
        text(&out)
    );
    assert!(text(&out).contains("200"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    m.clean();
    let l150 = lines(150);
    m.ok(m.commit(true, &[("b.txt", Some(&l150))], "feat: small", "commit -q"));

    m.profile_settings(r#"{"policies":{"diffSize":{"maxLines":1000}}}"#);
    let tip = m.rev("feat-x");
    let l500 = lines(500);
    let out = m.commit(
        true,
        &[("c.txt", Some(&l500))],
        "feat: five hundred",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "a personal 1000 does not raise the team's 400: {}",
        text(&out)
    );
    assert!(text(&out).contains("400"), "{}", text(&out));
    assert_eq!(m.rev("feat-x"), tip);
    m.clean();
    let l400 = lines(400);
    m.ok(m.commit(
        true,
        &[("d.txt", Some(&l400))],
        "feat: four hundred",
        "commit -q",
    ));
}

/// S-GRD-2, Q-GRD-51: with a team format, a personal one with more or fewer types does not count;
/// with none in the floor and one in the worktree, the worktree's holds and the personal does not
/// count; with no team format, the personal one requires its own.
#[test]
fn the_team_format_is_not_changed_by_a_personal_level() {
    let m = Machine::new(r#"{"policies":{"commitFormat":{"types":["feat","fix"]}}}"#);
    m.profile_settings(r#"{"policies":{"commitFormat":{"types":["feat","fix","wip"]}}}"#);
    let out = m.commit(
        true,
        &[("a.txt", Some("a\n"))],
        "wip: more types",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "`wip` is not the team's: {}",
        text(&out)
    );
    m.clean();
    m.profile_settings(r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#);
    m.ok(m.commit(
        true,
        &[("b.txt", Some("b\n"))],
        "fix: fewer types",
        "commit -q",
    ));
    m.stop();

    // No format in the floor; the person commits one in the worktree (the branch's own file).
    let m = Machine::new("{}");
    m.person_commit(
        &[(
            ".gitraptor/settings.json",
            Some(r#"{"policies":{"commitFormat":{"types":["docs"]}}}"#),
        )],
        "chore: worktree format",
    );
    m.profile_settings(r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#);
    let out = m.commit(
        true,
        &[("a.txt", Some("a\n"))],
        "feat: personal type",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the worktree's format holds: {}",
        text(&out)
    );
    m.clean();
    m.ok(m.commit(
        true,
        &[("b.txt", Some("b\n"))],
        "docs: worktree type",
        "commit -q",
    ));
    m.stop();

    // No team format at all: the personal one requires its own.
    let m = Machine::new("{}");
    m.profile_settings(r#"{"policies":{"commitFormat":{"types":["feat"]}}}"#);
    let out = m.commit(
        true,
        &[("a.txt", Some("a\n"))],
        "arreglos varios",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    m.clean();
    m.ok(m.commit(
        true,
        &[("b.txt", Some("b\n"))],
        "feat: personal",
        "commit -q",
    ));
}

/// D16: each denial is in `raptor guard log` with its rule, and the log keeps nothing of the
/// message or of the lines.
#[test]
fn commit_policy_denials_reach_the_decision_log() {
    let m = Machine::new(BOTH);
    let big = lines(1200);
    let out = m.commit(true, &[("big.txt", Some(&big))], "feat: big", "commit -q");
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );
    m.clean();
    let out = m.commit(
        true,
        &[("x.txt", Some("x\n"))],
        "arreglos varios secretos",
        "commit -q",
    );
    assert!(
        !out.status.success(),
        "the policy must deny this commit: {}",
        text(&out)
    );

    let log = m.log();
    let rules: Vec<String> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["reasons"].as_array().unwrap().iter())
        .map(|r| r["rule"].as_str().unwrap().to_owned())
        .collect();
    assert!(rules.iter().any(|r| r == "policy.diff-size"), "{log:#}");
    assert!(rules.iter().any(|r| r == "policy.commit-format"), "{log:#}");
    let shown = log.to_string();
    assert!(
        !shown.contains("arreglos") && !shown.contains("secretos"),
        "{shown}"
    );
    assert!(
        !shown.contains("line 5"),
        "no content of the files: {shown}"
    );
    assert!(
        !shown.contains("1200"),
        "the log keeps reasons without parameters: {shown}"
    );
}

/// D14: without the daemon the client applies the floor's rules for everyone; the ones for
/// agents need the actor and do not apply; the minimum still does.
#[test]
fn degraded_mode_applies_the_floor_rules_for_everyone() {
    let m = Machine::new(
        r#"{"policies":{
            "diffSize":{"maxLines":400,"appliesTo":"everyone"},
            "commitFormat":{}}}"#,
    );
    m.stop();
    let tip = m.rev("feat-x");
    let big = lines(1200);
    let out = m.commit(false, &[("big.txt", Some(&big))], "feat: big", "commit -q");
    assert!(
        !out.status.success(),
        "the rule for everyone holds without the daemon: {}",
        text(&out)
    );
    assert!(
        text(&out).contains("1200") && text(&out).contains("400"),
        "{}",
        text(&out)
    );
    assert_eq!(m.rev("feat-x"), tip);
    m.clean();

    // The format rule is for agents: the actor is unknown without the daemon, so it is not applied.
    let out = m.commit(
        true,
        &[("x.txt", Some("x\n"))],
        "arreglos varios",
        "commit -q",
    );
    m.ok(out);
    // The safe minimum holds whatever the policies say.
    let out = m.agent("branch -D main");
    assert!(
        !out.status.success(),
        "the safe minimum holds: {}",
        text(&out)
    );
}
