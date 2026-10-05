//! US-GRD-001 end to end: each scenario and criterion with the real `raptor` as daemon, client
//! and hook, the real `raptor-hook` dispatcher and plain Git, over a temporary machine of the
//! "intact repo" harness (INF-GRP-001, INF-GRD-001): temporary repos, remote and profile, never
//! this repo nor the real profile (NFR-01). No fixed waits: every step waits on an explicit
//! signal (a process exit, the daemon's pid file).
//!
//! The developer answers from their own terminal (a pty through `script`). Unix only; Windows
//! has no channel transport yet (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::guard::{GuardStatus, Permission, ProtectionState};
use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

/// The temporary machine of one scenario: the repo "demo" with `main` and `feat-x`, both
/// pushed to a bare remote, observed by a daemon of a temporary profile.
struct Machine {
    f: Fixture,
    remote: PathBuf,
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

/// `raptor <args>` from the developer's own terminal: a pty through `script`.
fn developer_command(args: &[OsString]) -> Command {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/script");
        c.args(["-q", "/dev/null", RAPTOR]).args(args);
        c
    } else {
        // util-linux: the command is one shell word list.
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let line = std::iter::once(RAPTOR.to_owned())
            .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
            .map(|a| quote(&a))
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new("script");
        c.args(["-q", "-e", "-c", &line, "/dev/null"]);
        c
    }
}

impl Machine {
    fn new() -> Self {
        let f = Fixture::with_commit(&git_from_path());
        Self::from_fixture(f)
    }

    fn from_fixture(f: Fixture) -> Self {
        // Without debug assertions `raptor` ignores GITRAPTOR_PROFILE_DIR and would use the
        // real profile (NFR-01): refuse to run.
        assert!(
            cfg!(debug_assertions),
            "build with debug assertions (CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true for --release)"
        );
        use std::os::unix::fs::PermissionsExt;
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
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
        Self { f, remote }
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
            // Debug builds only: the agents the daemon knows; none of this test's ancestors.
            ("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent".into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("HOME", self.f.home.clone().into_os_string()),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// `raptor <args>` from the developer's terminal. With `answer`, it is typed once the
    /// terminal shows `prompt` (the explicit signal; never a fixed wait).
    fn developer_answering(&self, args: &[&str], answer: Option<(&str, &str)>) -> Output {
        use std::io::Read;
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let mut child = developer_command(&args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut seen = Vec::new();
        if let Some((prompt, typed)) = answer {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !String::from_utf8_lossy(&seen).contains(prompt) {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(chunk) => seen.extend(chunk),
                    Err(_) => panic!("no prompt {prompt:?}: {}", String::from_utf8_lossy(&seen)),
                }
            }
            stdin.write_all(typed.as_bytes()).unwrap();
            stdin.flush().unwrap();
            // Kept open until the command ends: closing it would type an end of input.
            let status = child.wait().unwrap();
            drop(stdin);
            reader.join().unwrap();
            seen.extend(rx.try_iter().flatten());
            return Output {
                status,
                stdout: seen,
                stderr: Vec::new(),
            };
        }
        drop(stdin);
        let status = child.wait().unwrap();
        reader.join().unwrap();
        seen.extend(rx.try_iter().flatten());
        let mut stderr = Vec::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_end(&mut stderr);
        }
        Output {
            status,
            stdout: seen,
            stderr,
        }
    }

    /// `raptor <args>` from the developer's terminal, with nothing typed.
    fn developer(&self, args: &[&str], _input: &str) -> Output {
        self.developer_answering(args, None)
    }

    /// `raptor <args>` without a terminal.
    fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn add(&self, path: &Path) {
        let out = self.developer(&["repo", "add", path.to_str().unwrap()], "");
        assert!(out.status.success(), "{}", text(&out));
    }

    fn protect(&self, path: &Path) -> Output {
        self.developer(&["guard", "install", "--yes", path.to_str().unwrap()], "")
    }

    fn status(&self, path: &Path) -> GuardStatus {
        let out = self.raptor(&["guard", "status", "--json", path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Plain Git in `dir`, with the hooks active, as an agent would run it.
    fn git(&self, dir: &Path, args: &[&str]) -> Output {
        self.f.git_command(dir, args).output().unwrap()
    }

    fn git_ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git(dir, args);
        assert!(out.status.success(), "git {args:?}: {}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn remote_ref(&self, name: &str) -> Option<String> {
        let out = self.git(&self.remote, &["rev-parse", "--verify", "-q", name]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn local_ref(&self, name: &str) -> Option<String> {
        let out = self.git(&self.f.repo, &["rev-parse", "--verify", "-q", name]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Rewrites `feat-x` locally so pushing it needs a force.
    fn rewrite_feat_x(&self) {
        self.git_ok(&self.f.repo, &["switch", "-q", "feat-x"]);
        self.git_ok(
            &self.f.repo,
            &["commit", "-q", "--amend", "-m", "feat-x rewritten"],
        );
    }

    /// A protected "demo".
    fn protected() -> Self {
        let m = Self::new();
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m
    }

    fn stop(&self) {
        if running_pid(&self.dirs().state).unwrap().is_none() {
            return;
        }
        let out = self.developer(&["daemon", "stop", "--yes"], "");
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

fn common(m: &Machine) -> PathBuf {
    m.f.repo.join(".git")
}

/// What an install may change: the key and the folder in the repo, and the profile.
fn install_exceptions() -> Exceptions {
    Exceptions::guardrails_install("repo", Path::new(".git"))
        .and(Exceptions::engine_profile("profile"))
}

mod repo_intact {
    use super::*;

    // E1 · El desarrollador protege un repo con su permiso.
    #[test]
    fn e1_the_developer_protects_a_repo_with_permission() {
        let m = Machine::new();
        m.add(&m.f.repo);
        let before = m.status(&m.f.repo);
        assert_eq!(before.state, ProtectionState::Unprotected);
        assert!(before.offer);
        let mut out = None;
        let report = check("e1", &m.f, &install_exceptions(), || {
            out = Some(m.developer_answering(
                &["guard", "install", m.f.repo.to_str().unwrap()],
                Some(("[y/N]", "y\n")),
            ));
        });
        let out = out.unwrap();
        let shown = text(&out);
        assert!(out.status.success(), "{shown}");
        // What, where, why, how to revert, what it cannot prevent, which base is confirmed.
        for expected in [
            "What: the hooks pre-push, reference-transaction, pre-rebase",
            "Where: only this repository",
            "Why: so that no agent force-pushes or deletes the base branch",
            "To revert by hand:",
            "--unset core.hooksPath",
            "What the hooks cannot prevent in this repository:",
            "git reset --hard",
            "Base branch confirmed for this repository: main.",
            "Cost:",
            "Protect this repository? [y/N]",
        ] {
            assert!(
                shown.contains(expected),
                "missing {expected:?} in:\n{shown}"
            );
        }
        report.assert_intact();
        let after = m.status(&m.f.repo);
        assert_eq!(after.state, ProtectionState::HooksOnly);
        assert_eq!(after.permission, Permission::Granted);
        assert!(after.base_confirmed);
        assert_eq!(
            after
                .protected_bases
                .iter()
                .map(|b| b.raw())
                .collect::<Vec<_>>(),
            ["main"]
        );
        assert!(!after.offer);
        // ADR-GRD-001 Validación 15: only the mandatory dispatchers.
        let mut hooks: Vec<_> = std::fs::read_dir(common(&m).join("gitraptor/hooks"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        hooks.sort();
        assert_eq!(hooks, ["pre-push", "pre-rebase", "reference-transaction"]);
        // The key is absolute and local (ADR-GRD-001 § 1).
        let key = m.git_ok(&m.f.repo, &["config", "--local", "core.hooksPath"]);
        assert_eq!(Path::new(&key), common(&m).join("gitraptor/hooks"));
    }

    // E2 · Un force-push queda denegado por el mínimo seguro.
    #[test]
    fn e2_a_force_push_is_denied_by_the_safe_minimum() {
        let m = Machine::protected();
        let remote_before = m.remote_ref("refs/heads/feat-x").unwrap();
        m.rewrite_feat_x();
        let out = m.git(&m.f.repo, &["push", "-f", "origin", "feat-x"]);
        let shown = text(&out);
        assert!(!out.status.success(), "{shown}");
        assert!(
            shown.contains("Rule \"no force-push\" from the default minimum set"),
            "{shown}"
        );
        assert!(shown.contains("«feat-x»"), "{shown}");
        assert_eq!(m.remote_ref("refs/heads/feat-x").unwrap(), remote_before);
        // The same rule in Spanish (NFR-10).
        let mut es = m.f.git_command(
            &m.f.repo,
            &["push", "--force-with-lease", "origin", "feat-x"],
        );
        let out = es.env("LANG", "es_ES.UTF-8").output().unwrap();
        assert!(!out.status.success());
        assert!(
            text(&out).contains("Regla «prohibir force-push» del conjunto mínimo por defecto"),
            "{}",
            text(&out)
        );
        assert_eq!(m.remote_ref("refs/heads/feat-x").unwrap(), remote_before);
    }

    // E3 · Borrar la rama base queda denegado (local y remoto).
    #[test]
    fn e3_deleting_the_base_branch_is_denied() {
        let m = Machine::protected();
        let main = m.local_ref("refs/heads/main").unwrap();
        for args in [
            &["branch", "-D", "main"][..],
            &["update-ref", "-d", "refs/heads/main"],
            &["push", "origin", ":main"],
            &["push", "origin", "--delete", "main"],
        ] {
            let out = m.git(&m.f.repo, args);
            let shown = text(&out);
            assert!(!out.status.success(), "{args:?}: {shown}");
            assert!(
                shown.contains("Base branch protection"),
                "{args:?}: {shown}"
            );
        }
        assert_eq!(m.local_ref("refs/heads/main").unwrap(), main);
        assert_eq!(m.remote_ref("refs/heads/main").unwrap(), main);
    }

    // E4 · Una operación fuera del mínimo seguro se permite.
    #[test]
    fn e4_a_commit_outside_the_minimum_runs() {
        let m = Machine::protected();
        m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
        let before = m.local_ref("refs/heads/feat-x").unwrap();
        m.f.write("y.txt", "y\n");
        m.git_ok(&m.f.repo, &["add", "y.txt"]);
        let out = m.git(&m.f.repo, &["commit", "-q", "-m", "on feat-x"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).is_empty(),
            "no message on an allowed commit: {}",
            text(&out)
        );
        assert_ne!(m.local_ref("refs/heads/feat-x").unwrap(), before);
        // A fast-forward push and a feature branch deletion pass too.
        let out = m.git(&m.f.repo, &["push", "-q", "origin", "feat-x"]);
        assert!(out.status.success(), "{}", text(&out));
        m.git_ok(&m.f.repo, &["branch", "-q", "tmp"]);
        m.git_ok(&m.f.repo, &["branch", "-q", "-D", "tmp"]);
    }

    // E5 · Sin permiso no se instala nada ni se vuelve a preguntar.
    #[test]
    fn e5_without_permission_nothing_is_installed_nor_asked_again() {
        let m = Machine::new();
        m.add(&m.f.repo);
        // No answer: no terminal (and, with one, an empty answer) writes nothing.
        let profile_only = Exceptions::engine_profile("profile");
        let report = check("e5-no-answer", &m.f, &profile_only, || {
            let out = m.raptor(&["guard", "install", m.f.repo.to_str().unwrap()]);
            assert!(!out.status.success(), "{}", text(&out));
            assert!(
                text(&out).contains("no answer: nothing was installed"),
                "{}",
                text(&out)
            );
            let out = m.developer_answering(
                &["guard", "install", m.f.repo.to_str().unwrap()],
                Some(("[y/N]", "\n")),
            );
            assert!(!out.status.success(), "{}", text(&out));
        });
        report.assert_intact();
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::Unprotected);
        assert_eq!(status.permission, Permission::NotAsked);
        // A denial: nothing installed, recorded, never offered again.
        let report = check("e5-denied", &m.f, &profile_only, || {
            let out = m.developer_answering(
                &["guard", "install", m.f.repo.to_str().unwrap()],
                Some(("[y/N]", "n\n")),
            );
            assert!(out.status.success(), "{}", text(&out));
            assert!(
                text(&out).contains("will not offer it again"),
                "{}",
                text(&out)
            );
        });
        report.assert_intact();
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::Unprotected);
        assert_eq!(status.permission, Permission::Denied);
        assert!(!status.offer);
        // The developer activates it by hand later.
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
    }

    // E6 · El permiso de un repo no alcanza a otro.
    #[test]
    fn e6_the_permission_of_one_repo_does_not_reach_another() {
        let m = Machine::new();
        m.f.git_in(
            &m.f.other_repo,
            &["commit", "-q", "--allow-empty", "-m", "o"],
        );
        m.add(&m.f.repo);
        m.add(&m.f.other_repo);
        // Every scope but "demo" (`repo`) and the profile must stay identical: "otro" included.
        let report = check("e6", &m.f, &install_exceptions(), || {
            let out = m.protect(&m.f.repo);
            assert!(out.status.success(), "{}", text(&out));
        });
        report.assert_intact();
        let other = m.status(&m.f.other_repo);
        assert_eq!(other.state, ProtectionState::Unprotected);
        assert_eq!(other.permission, Permission::NotAsked);
        let out = m.git(&m.f.other_repo, &["config", "--get", "core.hooksPath"]);
        assert!(!out.status.success(), "otro has no key");
    }
}

/// Edits one constant of the installed dispatchers (simulates a moved or failing `raptor`).
fn set_constant(m: &Machine, key: &str, value: &str) {
    let conf = common(m).join("gitraptor/dispatch.conf");
    let text = std::fs::read_to_string(&conf).unwrap();
    let edited: String = text
        .lines()
        .map(|l| match l.split_once('\t') {
            Some((k, _)) if k == key => format!("{k}\t{value}\n"),
            _ => format!("{l}\n"),
        })
        .collect();
    std::fs::write(conf, edited).unwrap();
}

/// A program that stands for a `raptor` failing with an internal error (exit 3).
fn failing_raptor(m: &Machine) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = m.f.root.join("failing-raptor");
    std::fs::write(&path, "#!/bin/sh\ncat >/dev/null\nexit 3\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

mod criteria {
    use super::*;

    // ADR-GRD-002 Validación 3: every way of forcing a push is a force-push (H-05).
    #[test]
    fn repo_intact_force_push_variants_are_denied() {
        let m = Machine::protected();
        let remote_before = m.remote_ref("refs/heads/feat-x").unwrap();
        m.rewrite_feat_x();
        for args in [
            &["push", "origin", "+feat-x"][..],
            &["push", "--force-with-lease", "origin", "feat-x"],
            &["push", "origin", "+refs/heads/*:refs/heads/*"],
        ] {
            let out = m.git(&m.f.repo, args);
            assert!(!out.status.success(), "{args:?}: {}", text(&out));
            assert!(
                text(&out).contains("no force-push"),
                "{args:?}: {}",
                text(&out)
            );
        }
        // A graft that makes the rewrite look like a fast-forward: Git alone would accept it
        // without --force (SPIKE-GRD-001 F08b); the guard reads without replacements.
        let rewritten = m.local_ref("refs/heads/feat-x").unwrap();
        m.git_ok(
            &m.f.repo,
            &["replace", "--graft", &rewritten, &remote_before],
        );
        let out = m.git(&m.f.repo, &["push", "origin", "feat-x"]);
        assert!(!out.status.success(), "graft: {}", text(&out));
        assert!(text(&out).contains("no force-push"), "{}", text(&out));
        assert_eq!(m.remote_ref("refs/heads/feat-x").unwrap(), remote_before);
    }

    // ADR-GRD-002 Validación 3: a remote tip missing locally is treated as forced (H-05).
    #[test]
    fn repo_intact_a_remote_tip_missing_locally_is_a_force_push() {
        let m = Machine::protected();
        // Somebody else moves feat-x on the remote; this repo never fetched it.
        let other = m.f.root.join("other-clone");
        m.f.git_in(
            &m.f.root,
            &[
                "clone",
                "-q",
                m.remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        m.f.git_in(&other, &["switch", "-q", "feat-x"]);
        m.f.git_in(&other, &["commit", "-q", "--allow-empty", "-m", "theirs"]);
        m.f.git_in(&other, &["push", "-q", "origin", "feat-x"]);
        let theirs = m.remote_ref("refs/heads/feat-x").unwrap();
        m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
        m.git_ok(&m.f.repo, &["commit", "-q", "--allow-empty", "-m", "mine"]);
        let out = m.git(&m.f.repo, &["push", "-f", "origin", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("no force-push"), "{}", text(&out));
        assert_eq!(m.remote_ref("refs/heads/feat-x").unwrap(), theirs);
    }

    // ADR-GRD-002 Validación 3 and its amendment (F07): a shallow clone cannot prove a
    // fast-forward; the denial says so and suggests `git fetch --unshallow`.
    #[test]
    fn repo_intact_a_push_from_a_shallow_clone_is_denied_as_shallow() {
        let m = Machine::new();
        let shallow = m.f.root.join("shallow");
        let url = format!("file://{}", m.remote.display());
        m.f.git_in(
            &m.f.root,
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                &url,
                shallow.to_str().unwrap(),
            ],
        );
        m.add(&shallow);
        let out = m.protect(&shallow);
        assert!(out.status.success(), "{}", text(&out));
        m.git_ok(&shallow, &["commit", "-q", "--allow-empty", "-m", "ff"]);
        let out = m.git(&shallow, &["push", "origin", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("shallow clone"), "{}", text(&out));
        assert!(
            text(&out).contains("git fetch --unshallow"),
            "{}",
            text(&out)
        );
    }

    // ADR-GRD-002 Validación 4 and 5: every way of deleting the base branch, from every
    // worktree, through HEAD and through an alias, is denied.
    #[test]
    fn repo_intact_base_branch_deletion_through_worktrees_head_and_aliases() {
        let m = Machine::protected();
        let main = m.local_ref("refs/heads/main").unwrap();
        // A worktree created after the install is covered (ADR-GRD-001 Validación 4).
        let wt = m.f.root.join("wt-later");
        m.git_ok(
            &m.f.repo,
            &["worktree", "add", "-q", "-b", "later", wt.to_str().unwrap()],
        );
        let out = m.git(&wt, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        // `update-ref -d HEAD` with HEAD on main: Git 2.38 hands the line over as `HEAD`.
        m.git_ok(&wt, &["switch", "-q", "main"]);
        let out = m.git(&wt, &["update-ref", "-d", "HEAD"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("Base branch protection"),
            "{}",
            text(&out)
        );
        // `GIT_DIR` from another repo's folder: the dispatcher's repo is protected (M-02).
        let out = m.git(
            &m.f.other_repo,
            &[
                "--git-dir",
                common(&m).to_str().unwrap(),
                "branch",
                "-D",
                "main",
            ],
        );
        assert!(!out.status.success(), "{}", text(&out));
        // An alias of the base on a case-insensitive file system (H-06).
        let ignores_case = m.git(&m.f.repo, &["config", "--bool", "core.ignoreCase"]);
        if String::from_utf8_lossy(&ignores_case.stdout).trim() == "true" {
            let out = m.git(&m.f.repo, &["branch", "-D", "Main"]);
            assert!(!out.status.success(), "{}", text(&out));
            assert!(
                text(&out).contains("matches the base branch"),
                "{}",
                text(&out)
            );
        }
        assert_eq!(m.local_ref("refs/heads/main").unwrap(), main);
    }

    // ADR-GRD-002 Validación 10 and ADR-GRD-001 Validación 14: `pack-refs` and `gc` prune the
    // loose base branch without being taken for a deletion; the packed base stays protected.
    #[test]
    fn repo_intact_pack_refs_and_gc_pass_and_the_packed_base_stays_protected() {
        let m = Machine::protected();
        for args in [&["pack-refs", "--all"][..], &["gc", "-q"]] {
            let out = m.git(&m.f.repo, args);
            assert!(out.status.success(), "{args:?}: {}", text(&out));
        }
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(m.local_ref("refs/heads/main").is_some());
    }

    // ADR-GRD-003 Validación 9: with the daemon stopped, degraded mode decides alone, stricter.
    #[test]
    fn repo_intact_degraded_mode_without_the_daemon() {
        let m = Machine::protected();
        m.stop();
        m.rewrite_feat_x();
        let out = m.git(&m.f.repo, &["push", "-f", "origin", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("no force-push"), "{}", text(&out));
        assert!(text(&out).contains("service unreachable"), "{}", text(&out));
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        // A commit still runs, with the notice.
        let out = m.git(
            &m.f.repo,
            &["commit", "-q", "--allow-empty", "-m", "degraded"],
        );
        assert!(out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("service unreachable"), "{}", text(&out));
    }

    // ADR-GRD-003 Validación 6: a daemon of another profile instance at the channel path never
    // decides (degraded with instance-mismatch); a server that is not the installed binary is
    // not authentic (deny).
    #[test]
    fn repo_intact_the_channel_is_authenticated() {
        let m = Machine::protected();
        m.stop();
        // The profile is recreated: same folders, a new instance id.
        for entry in std::fs::read_dir(m.dirs().data).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::remove_file(path).unwrap();
            } else {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        m.rewrite_feat_x();
        let out = m.git(&m.f.repo, &["push", "-f", "origin", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("another GitRaptor profile answered"),
            "{}",
            text(&out)
        );
        m.stop();
        // An impostor at the socket path: this test binary, not `raptor`.
        let socket = m.dirs().runtime.unwrap().join("raptor.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let impostor = std::thread::spawn(move || {
            // Accepts and stays silent: an authentic client never sends a byte.
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            use std::io::Read;
            conn.read(&mut buf).unwrap_or(0)
        });
        let out = m.git(&m.f.repo, &["branch", "-D", "feat-x-tmp"]);
        let _ = out;
        m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
        let out = m.git(&m.f.repo, &["branch", "-D", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("could not be verified"),
            "{}",
            text(&out)
        );
        assert_eq!(impostor.join().unwrap(), 0, "the client sent nothing");
        let _ = std::fs::remove_file(&socket);
    }

    // ADR-GRD-001 Validación 7: a hostile environment changes nothing (constants only).
    #[test]
    fn repo_intact_a_hostile_environment_does_not_steer_the_hook() {
        let m = Machine::protected();
        let fake = m.f.root.join("fake");
        std::fs::create_dir_all(fake.join("run")).unwrap();
        let mut git = m.f.git_command(&m.f.repo, &["branch", "-D", "main"]);
        let out = git
            .env("PATH", format!("{}:/usr/bin:/bin", fake.display()))
            .env("HOME", &fake)
            .env("XDG_RUNTIME_DIR", fake.join("run"))
            .env("XDG_CONFIG_HOME", &fake)
            .env("GITRAPTOR_PROFILE_DIR", &fake)
            .env("LD_PRELOAD", fake.join("evil.so"))
            .env("DYLD_INSERT_LIBRARIES", fake.join("evil.dylib"))
            .output()
            .unwrap();
        assert!(!out.status.success(), "{}", text(&out));
        // Decided by the real daemon, not in degraded mode.
        assert!(
            text(&out).contains("Base branch protection"),
            "{}",
            text(&out)
        );
        assert!(
            !text(&out).contains("service unreachable"),
            "{}",
            text(&out)
        );
    }

    // ADR-GRD-001 Validación 5 and 14: without `raptor`, push and rebase fail with the recovery
    // message, a branch deletion exits 1, and commit, `pack-refs` and `gc` pass with a warning.
    #[test]
    fn repo_intact_without_raptor_risky_hooks_fail_closed() {
        let m = Machine::protected();
        set_constant(&m, "raptor", "/nonexistent/raptor");
        let out = m.git(&m.f.repo, &["commit", "-q", "--allow-empty", "-m", "c"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("protection inactive"), "{}", text(&out));
        let out = m.git(&m.f.repo, &["push", "origin", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("was not found"), "{}", text(&out));
        assert!(
            !text(&out).contains("hooksPath"),
            "no way to disable: {}",
            text(&out)
        );
        let out = m.git(&m.f.repo, &["rebase", "--force-rebase", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        for args in [&["pack-refs", "--all"][..], &["gc", "-q"]] {
            let out = m.git(&m.f.repo, args);
            assert!(out.status.success(), "{args:?}: {}", text(&out));
        }
        assert!(m.local_ref("refs/heads/main").is_some());
    }

    // ADR-GRD-001 Validación 6: an internal error of `raptor` lets a commit pass with a warning
    // and denies a branch deletion and a push (fail-closed only where it is risky).
    #[test]
    fn repo_intact_an_internal_error_fails_closed_only_where_risky() {
        let m = Machine::protected();
        let failing = failing_raptor(&m);
        set_constant(&m, "raptor", failing.to_str().unwrap());
        let out = m.git(&m.f.repo, &["commit", "-q", "--allow-empty", "-m", "c"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("internal error"), "{}", text(&out));
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
        let out = m.git(&m.f.repo, &["push", "origin", "feat-x"]);
        assert!(!out.status.success(), "{}", text(&out));
    }

    // ADR-GRD-001 § 2 (M-02): constants that are not the repo's own deny.
    #[test]
    fn repo_intact_moved_dispatchers_deny() {
        let m = Machine::protected();
        set_constant(&m, "common", m.f.other_repo.join(".git").to_str().unwrap());
        let out = m.git(&m.f.repo, &["commit", "-q", "--allow-empty", "-m", "c"]);
        assert!(!out.status.success(), "{}", text(&out));
        assert!(text(&out).contains("moved or altered"), "{}", text(&out));
    }

    // ADR-GRD-004 Validación 10: a repo with a team configuration installs without reading it;
    // the base stays unconfirmed and the minimum protects {main, main branch}.
    #[test]
    fn repo_intact_a_team_configuration_leaves_the_base_unconfirmed() {
        let m = Machine::new();
        m.git_ok(&m.f.repo, &["switch", "-q", "main"]);
        m.f.write(".gitraptor/settings.json", "{}\n");
        m.git_ok(&m.f.repo, &["add", ".gitraptor/settings.json"]);
        m.git_ok(&m.f.repo, &["commit", "-q", "-m", "team"]);
        // The remote's main branch is `trunk`.
        m.git_ok(&m.f.repo, &["branch", "-q", "trunk"]);
        m.git_ok(&m.f.repo, &["push", "-q", "origin", "main", "trunk"]);
        m.git_ok(&m.remote, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
        m.git_ok(&m.f.repo, &["remote", "set-head", "origin", "--auto"]);
        m.git_ok(&m.f.repo, &["switch", "-q", "feat-x"]);
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        assert!(
            text(&out).contains("base branch stays unconfirmed"),
            "{}",
            text(&out)
        );
        let status = m.status(&m.f.repo);
        assert!(!status.base_confirmed);
        let mut bases: Vec<_> = status.protected_bases.iter().map(|b| b.raw()).collect();
        bases.sort_unstable();
        assert_eq!(bases, ["main", "trunk"]);
        for base in ["main", "trunk"] {
            let out = m.git(&m.f.repo, &["branch", "-D", base]);
            assert!(!out.status.success(), "{base}: {}", text(&out));
        }
    }

    // D3, BR-EDGE-002: with hooks to chain, nothing is installed, the reason is recorded, and a
    // refusal is not a denial of the permission.
    #[test]
    fn repo_intact_prior_hooks_refuse_the_install_without_changes() {
        use std::os::unix::fs::PermissionsExt;
        let m = Machine::new();
        let hook = common(&m).join("hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        m.add(&m.f.repo);
        let report = check(
            "prior-hooks",
            &m.f,
            &Exceptions::engine_profile("profile"),
            || {
                let out = m.protect(&m.f.repo);
                assert!(!out.status.success(), "{}", text(&out));
                assert!(
                    text(&out).contains("already has Git hooks"),
                    "{}",
                    text(&out)
                );
            },
        );
        report.assert_intact();
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::Unprotected);
        assert_eq!(status.permission, Permission::NotAsked);
        assert!(!status.last_refusal.is_empty());
    }

    // The manual revert the explanation shows (unset the key, remove the folder) leaves the repo
    // unprotected in the status, and protecting it again works.
    #[test]
    fn repo_intact_the_manual_revert_is_seen_and_the_repo_can_be_protected_again() {
        let m = Machine::protected();
        let config = common(&m).join("config");
        m.git_ok(
            &m.f.repo,
            &[
                "config",
                "--file",
                config.to_str().unwrap(),
                "--unset",
                "core.hooksPath",
            ],
        );
        let folder = common(&m).join("gitraptor");
        for f in [
            "hooks/pre-push",
            "hooks/pre-rebase",
            "hooks/reference-transaction",
        ] {
            std::fs::remove_file(folder.join(f)).unwrap();
        }
        std::fs::remove_dir(folder.join("hooks")).unwrap();
        std::fs::remove_file(folder.join("dispatch.conf")).unwrap();
        std::fs::remove_file(folder.join("manifest.json")).unwrap();
        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::Unprotected);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(m.status(&m.f.repo).state, ProtectionState::HooksOnly);
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }

    // ADR-GRD-001 Validación 8: a `gitraptor` folder replaced by a link is never written through.
    #[test]
    fn repo_intact_a_linked_folder_is_not_written_through() {
        let m = Machine::new();
        let elsewhere = m.f.root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, common(&m).join("gitraptor")).unwrap();
        m.add(&m.f.repo);
        let out = m.protect(&m.f.repo);
        assert!(!out.status.success(), "{}", text(&out));
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        let out = m.git(&m.f.repo, &["config", "--get", "core.hooksPath"]);
        assert!(!out.status.success());
    }
}

/// Rewrites the journal of the protected repo as if the daemon had died before confirming
/// (ADR-GRD-001 § 4, Recuperación): the stage goes back to `installing`.
fn unfinish_install(m: &Machine) {
    use gitraptor_core::profile::Profile;
    let (profile, _) = Profile::open(m.dirs()).unwrap();
    let entry = profile.repo_by_common_dir(&common(m)).unwrap().unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let journal = store.guard_keys().unwrap().journal.unwrap();
    let installing = journal.replace(r#""stage":"confirmed""#, r#""stage":"installing""#);
    assert_ne!(journal, installing);
    store
        .set_guard_keys(Some(Some(&installing)), Some("not-asked"), None, None)
        .unwrap();
}

mod recovery {
    use super::*;

    // ADR-GRD-001 § 4: the key is not ours, so the folder goes and the repo is as before.
    #[test]
    fn repo_intact_an_unfinished_install_without_the_key_is_undone_at_startup() {
        let m = Machine::new();
        m.add(&m.f.repo);
        // The repo says the same afterwards (the `config` may be rewritten); the profile and
        // the channel socket of the restarted daemon change.
        let exceptions = Exceptions::guardrails_uninstalled("repo", Path::new(".git"))
            .and(Exceptions::engine_profile("profile"))
            .with(gitraptor_testkit::Exception::Subtree {
                scope: "profile".into(),
                prefix: "run".into(),
            });
        let before = m.f.snapshot(&exceptions);
        let out = m.protect(&m.f.repo);
        assert!(out.status.success(), "{}", text(&out));
        m.stop();
        unfinish_install(&m);
        m.git_ok(&m.f.repo, &["config", "--unset", "core.hooksPath"]);
        // The next start recovers before serving.
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(!common(&m).join("gitraptor").exists());
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::Unprotected);
        let after = m.f.snapshot(&exceptions);
        let changes = exceptions.filter(&gitraptor_testkit::diff(&before, &after), &before, &after);
        assert!(changes.is_empty(), "{changes:#?}");
    }

    // ADR-GRD-001 § 4: the key is ours and every worktree sees it: the install is confirmed.
    #[test]
    fn repo_intact_an_unfinished_install_with_the_key_is_confirmed_at_startup() {
        let m = Machine::protected();
        m.stop();
        unfinish_install(&m);
        let out = m.raptor(&["daemon", "status"]);
        assert!(out.status.success(), "{}", text(&out));
        let status = m.status(&m.f.repo);
        assert_eq!(status.state, ProtectionState::HooksOnly);
        assert_eq!(status.permission, Permission::Granted);
        let out = m.git(&m.f.repo, &["branch", "-D", "main"]);
        assert!(!out.status.success(), "{}", text(&out));
    }
}

/// NFR-GRD-04 / ADR-GRD-002 § 5, as a report (not a gate; INF-GRD-001 D11): what the hook layer
/// adds per command, protected against unprotected. Run on a machine at rest with
/// `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release -p gitraptor-cli --test
/// guard_us_grd_001 latency_report -- --ignored --nocapture` (debug assertions keep the
/// temporary profile).
#[test]
#[ignore = "latency report, run by hand on a machine at rest"]
fn latency_report() {
    const N: usize = 40;
    let percentile = |mut v: Vec<f64>, q: f64| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[((q * (v.len() - 1) as f64).round() as usize).min(v.len() - 1)]
    };
    let time = |m: &Machine, args: &[&str], setup: &dyn Fn(&Machine)| {
        let mut samples = Vec::new();
        for i in 0..N + 3 {
            setup(m);
            let start = Instant::now();
            let out = m.git(&m.f.repo, args);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(out.status.success(), "{args:?}: {}", text(&out));
            if i >= 3 {
                samples.push(ms);
            }
        }
        (percentile(samples.clone(), 0.5), percentile(samples, 0.95))
    };
    let plain = Machine::new();
    let guarded = Machine::protected();
    let no_setup = |_: &Machine| {};
    let tmp_branch = |m: &Machine| {
        let _ = m.git(&m.f.repo, &["branch", "-q", "tmp"]);
    };
    let tmp_tag = |m: &Machine| {
        let _ = m.git(&m.f.repo, &["tag", "-d", "t"]);
    };
    for (name, args, setup) in [
        (
            "commit",
            &["commit", "-q", "--allow-empty", "-m", "x"][..],
            &no_setup as &dyn Fn(&Machine),
        ),
        (
            "branch -D (governed evaluation)",
            &["branch", "-q", "-D", "tmp"],
            &tmp_branch,
        ),
        ("tag (fast path)", &["tag", "t"], &tmp_tag),
    ] {
        let (p50, p95) = time(&plain, args, setup);
        let (g50, g95) = time(&guarded, args, setup);
        println!(
            "{name:34} unprotected p50 {p50:6.1} p95 {p95:6.1} | protected p50 {g50:6.1} p95 {g95:6.1} | delta p50 {:+6.1} p95 {:+6.1} ms",
            g50 - p50,
            g95 - p95
        );
    }
}
