//! Guardrails on Windows (DS-US-GRD-001, Enmienda 2026-10-08): the install layer of
//! `crates/core` writes the hooks of a repo under a path with spaces, and the real native
//! dispatcher with plain Git denies a force-push and the deletion of the base branch while a
//! normal push goes through. Temporary profile, repo and remote, never this repo nor the real
//! profile (NFR-01).
//!
//! The install is called as the daemon does it, not through `raptor guard install`: that
//! command answers only to the console of a logged-in desktop session (TQ-14), which no test
//! process has. The real command is checked by hand on the machine (the PR says so). There is
//! no daemon, so the hooks run in degraded mode: the minimum set at its strictest, which is
//! the line these denials hold even when the service is gone.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use gitraptor_core::guardrails::GuardRegistry;
use gitraptor_core::guardrails::install::{self, GuardCtx};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_git::Invoker;
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_winsys::acl;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
/// Referenced so Cargo builds the dispatcher the install copies next to `raptor`.
const _HOOK: &str = env!("CARGO_BIN_EXE_raptor-hook");
const PROGRAM_FILES_GIT: &str = r"C:\Program Files\Git\cmd\git.exe";

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new(PROGRAM_FILES_GIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("run git")
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A repo "demo" under a path with spaces, `main` and `feat-x` pushed to a bare remote.
struct Machine {
    _root: tempfile::TempDir,
    repo: PathBuf,
    profile: PathBuf,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("path with spaces");
        let repo = parent.join("demo");
        let remote = root.path().join("remote.git");
        std::fs::create_dir_all(&repo).unwrap();
        git_ok(
            root.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git_ok(&repo, &["init", "-q", "-b", "main"]);
        git_ok(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git_ok(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&repo, &["push", "-q", "origin", "main"]);
        git_ok(&repo, &["switch", "-q", "-c", "feat-x"]);
        git_ok(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
        git_ok(&repo, &["push", "-q", "origin", "feat-x"]);
        let profile = root.path().join("profile");
        Self {
            _root: root,
            repo,
            profile,
        }
    }

    /// Observes the repo and installs the hooks as the daemon does; returns the common dir.
    fn install(&self) -> PathBuf {
        let (mut profile, _) = Profile::open(ProfileDirs::under_root(&self.profile)).unwrap();
        let (entry, _) = profile.add_repo(&self.repo.join(".git"), None, 1).unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let invoker = Invoker::default();
        let config = ResolveConfig {
            configured_path: Some(PROGRAM_FILES_GIT.into()),
            path_env: None,
            known_locations: vec![],
            shim_paths: vec![],
            toolchain_gits: vec![],
        };
        let Resolution::Found { git, .. } = resolve::resolve(&config, &invoker) else {
            panic!("no system Git");
        };
        let instance = profile.instance_id().to_owned();
        let dirs = profile.dirs().clone();
        let ctx = GuardCtx {
            git: &git,
            invoker: &invoker,
            dirs: &dirs,
            instance: &instance,
            raptor: Path::new(RAPTOR),
        };
        let common = entry.canonical_path.clone();
        let plan = install::plan(&ctx, &entry.repo_id, &common, &store);
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        install::install(
            &ctx,
            &entry.repo_id,
            &common,
            &mut store,
            &GuardRegistry::default(),
            2,
        )
        .unwrap_or_else(|e| panic!("install: {e:?}"));
        common
    }
}

#[test]
fn install_protects_a_repo_under_a_path_with_spaces() {
    let m = Machine::new();
    let common = m.install();
    // Drive form, never the verbatim prefix, in the key Git reads.
    let key = git(&m.repo, &["config", "--local", "core.hooksPath"]);
    let hooks = String::from_utf8_lossy(&key.stdout).trim().to_owned();
    assert!(!hooks.starts_with(r"\\?\"), "{hooks}");
    assert!(Path::new(&hooks).join("pre-push").is_file(), "{hooks}");
    // M-07: the folder and what it holds are private to the user.
    acl::verify_private_dir(&common.join("gitraptor")).unwrap();
    acl::verify_private_dir(&common.join("gitraptor").join("hooks")).unwrap();

    // A force-push is denied.
    git_ok(&m.repo, &["switch", "-q", "main"]);
    git_ok(&m.repo, &["commit", "-q", "--allow-empty", "-m", "three"]);
    git_ok(&m.repo, &["push", "-q", "origin", "main"]);
    git_ok(&m.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    let forced = git(&m.repo, &["push", "--force", "origin", "main"]);
    assert!(!forced.status.success());
    assert!(stderr(&forced).contains("GitRaptor"), "{}", stderr(&forced));

    // The base branch cannot be deleted, remote or local, by push, branch or update-ref.
    let remote = git(&m.repo, &["push", "origin", ":main"]);
    assert!(!remote.status.success());
    assert!(stderr(&remote).contains("GitRaptor"), "{}", stderr(&remote));
    git_ok(&m.repo, &["switch", "-q", "feat-x"]);
    for args in [
        &["branch", "-D", "main"][..],
        &["update-ref", "-d", "refs/heads/main"][..],
    ] {
        let out = git(&m.repo, args);
        assert!(!out.status.success(), "{args:?}");
        assert!(stderr(&out).contains("GitRaptor"), "{}", stderr(&out));
    }
    git_ok(&m.repo, &["rev-parse", "--verify", "main"]);

    // The normal work goes through.
    git_ok(&m.repo, &["commit", "-q", "--allow-empty", "-m", "four"]);
    git_ok(&m.repo, &["push", "-q", "origin", "feat-x"]);
}

/// A hook the repo already had is chained by the native dispatcher through Git's own `sh`, as
/// Git itself would run it (US-GRD-002, #193): a script with `#!`, with the same arguments and
/// input, whose exit code decides, and a denial of GitRaptor never reaches it.
#[test]
fn a_prior_script_hook_is_chained_through_git_for_windows_sh() {
    let m = Machine::new();
    let hooks = m.repo.join(".git").join("hooks");
    let marker = m.repo.parent().unwrap().join("prior ran.txt");
    let deny = m.repo.parent().unwrap().join("deny push");
    let unix = |p: &Path| p.to_string_lossy().replace('\\', "/");
    std::fs::write(
        hooks.join("pre-commit"),
        format!("#!/bin/sh\necho \"pre-commit $#\" >> '{}'\n", unix(&marker)),
    )
    .unwrap();
    std::fs::write(
        hooks.join("pre-push"),
        format!(
            "#!/bin/sh\ncat > /dev/null\nif [ -e '{}' ]; then echo 'prior says no' >&2; exit 1; fi\n",
            unix(&deny)
        ),
    )
    .unwrap();
    m.install();

    git_ok(&m.repo, &["commit", "-q", "--allow-empty", "-m", "three"]);
    let ran = std::fs::read_to_string(&marker).expect("the prior pre-commit ran");
    assert!(ran.contains("pre-commit 0"), "{ran}");
    git_ok(&m.repo, &["push", "-q", "origin", "feat-x"]);

    std::fs::write(&deny, "").unwrap();
    git_ok(&m.repo, &["commit", "-q", "--allow-empty", "-m", "four"]);
    let refused = git(&m.repo, &["push", "origin", "feat-x"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("prior says no"),
        "{}",
        stderr(&refused)
    );

    // GitRaptor's own denial comes first and the prior hook never sees it.
    std::fs::remove_file(&deny).unwrap();
    git_ok(&m.repo, &["reset", "-q", "--hard", "HEAD~2"]);
    let forced = git(&m.repo, &["push", "--force", "origin", "feat-x"]);
    assert!(!forced.status.success());
    assert!(stderr(&forced).contains("GitRaptor"), "{}", stderr(&forced));
}
