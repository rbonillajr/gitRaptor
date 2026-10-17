//! What the options of a user operation leave to the user's hooks and to Git itself: the
//! transport is closed only in a partial clone (whose missing objects Git would fetch on its own),
//! and the paths of a commit are taken literally without a variable the hooks would inherit.
//! Temporary repos only (NFR-01).

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::Fixture;
use gitraptor_git::Oid;
use gitraptor_git::user_ops::{FALLBACK_NO_EDITOR, RepoTarget, SessionEnv, UserGitCommand, UserOp};

fn no_editor(f: &Fixture) -> PathBuf {
    if cfg!(windows) {
        f.root().join("raptor-no-editor.exe")
    } else {
        PathBuf::from(FALLBACK_NO_EDITOR)
    }
}

fn command(f: &Fixture, op: &UserOp, message: Option<&str>) -> UserGitCommand {
    let target = RepoTarget {
        git_dir: f.repo.join(".git"),
        work_tree: Some(f.repo.clone()),
    };
    UserGitCommand::new(
        &f.git,
        &target,
        op,
        message,
        &SessionEnv::default(),
        &no_editor(f),
        Some(&f.home),
    )
    .unwrap()
}

/// Runs `cmd` to its end: success and stderr.
fn run(cmd: UserGitCommand) -> (bool, String) {
    let (mut child, out) = cmd.spawn().unwrap();
    let status = child.wait().unwrap();
    let (_, stderr) = out.collect();
    (
        status.success(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

/// `git <args>` in `git_dir` with `input` on stdin; its trimmed stdout.
fn git_stdin(f: &Fixture, git_dir: &Path, args: &[&str], input: &str) -> String {
    let mut c = Command::new(&f.git.path);
    c.arg(format!("--git-dir={}", git_dir.display()))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &f.home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn objects(git_dir: &Path) -> usize {
    let mut n = 0;
    for dir in std::fs::read_dir(git_dir.join("objects"))
        .unwrap()
        .flatten()
    {
        if dir.path().is_dir() {
            n += std::fs::read_dir(dir.path()).unwrap().count();
        }
    }
    n
}

/// A normal repo: no `protocol.allow` reaches the user's hooks.
#[test]
fn a_normal_repo_leaves_the_transport_alone() {
    let f = Fixture::with_commit();
    let argv = command(&f, &UserOp::MergeAbort, None).argv();
    assert!(
        !argv.iter().any(|a| a.starts_with("protocol.allow")),
        "{argv:?}"
    );
}

/// A partial clone: the transport is closed for our own `git`, which then never fetches a
/// missing object from its promisor, even a reachable one that has it.
#[test]
fn a_partial_clone_never_fetches() {
    let f = Fixture::with_commit();
    let git_dir = f.repo.join(".git");
    // The promisor: a bare repo, reachable by path, holding the blob the clone lacks.
    let promisor = f.root().join("promisor.git");
    f.git_in(
        &f.root(),
        &["init", "-q", "--bare", promisor.to_str().unwrap()],
    );
    let blob = git_stdin(&f, &promisor, &["hash-object", "-w", "--stdin"], "secret\n");
    // A commit of the clone whose tree names that blob, which the clone does not hold.
    let tree = git_stdin(
        &f,
        &git_dir,
        &["mktree", "--missing"],
        &format!("100644 blob {blob}\tsecret.txt\n"),
    );
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let commit = git_stdin(
        &f,
        &git_dir,
        &["commit-tree", &tree, "-p", &head, "-m", "lacks its blob"],
        "",
    );
    f.git(&["config", "core.repositoryformatversion", "1"]);
    f.git(&["config", "extensions.partialClone", "origin"]);
    f.git(&["remote", "add", "origin", promisor.to_str().unwrap()]);
    f.git(&["config", "remote.origin.promisor", "true"]);

    let op = UserOp::MergeOid {
        oid: Oid::from_hex(&commit).unwrap(),
        message: "Merge".into(),
    };
    let cmd = command(&f, &op, None);
    assert!(
        cmd.argv().iter().any(|a| a == "protocol.allow=never"),
        "{:?}",
        cmd.argv()
    );
    let before = objects(&git_dir);
    let (ok, stderr) = run(cmd);
    assert!(!ok, "the merge needed the missing blob: {stderr}");
    // Git did try its promisor, and the closed transport refused it.
    assert!(stderr.contains("not allowed"), "{stderr}");
    assert_eq!(objects(&git_dir), before, "something was fetched: {stderr}");
    assert!(
        !git_dir
            .join("objects")
            .join(&blob[..2])
            .join(&blob[2..])
            .exists(),
        "the blob was fetched"
    );
}

/// `:(literal)`: paths with pathspec magic characters commit exactly those files, never the
/// others their glob would match.
#[test]
fn commit_paths_are_literal() {
    let f = Fixture::with_commit();
    let magic = ["a*.txt", ":x", "[ab].txt"];
    let others = ["abc.txt", "a.txt", "b.txt"];
    for name in magic.iter().chain(&others) {
        if cfg!(windows) && (name.contains('*') || name.contains(':')) {
            continue;
        }
        f.write(name, &format!("{name}\n"));
        f.git(&["add", "--", &format!(":(literal){name}")]);
    }
    let paths: Vec<String> = magic
        .iter()
        .filter(|n| !(cfg!(windows) && (n.contains('*') || n.contains(':'))))
        .map(|n| (*n).to_owned())
        .collect();
    let (ok, stderr) = run(command(
        &f,
        &UserOp::CommitPaths {
            paths: paths.clone(),
        },
        Some("literal paths"),
    ));
    assert!(ok, "{stderr}");
    let committed = f.git(&["show", "--name-only", "--format=", "HEAD"]);
    let mut committed: Vec<&str> = committed.lines().collect();
    committed.sort_unstable();
    let mut expected: Vec<&str> = paths.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(committed, expected);
    // The files the globs would have matched are still only staged.
    let staged = f.git(&["diff", "--cached", "--name-only"]);
    for other in others {
        assert!(staged.lines().any(|l| l == other), "{other}: {staged}");
    }
}

/// A global `core.hooksPath` (the user's own `~/.gitconfig`, here a temporary HOME) runs under
/// the executor's `git`, not neutralized: its `pre-commit` reads the staged diff and rejects the
/// marker; without the marker the commit goes in.
#[cfg(unix)]
#[test]
fn a_global_hooks_path_runs_under_the_executor() {
    let f = Fixture::with_commit();
    let hooks = f.root().join("global-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    common::script(
        &hooks.join("pre-commit"),
        "if git diff --cached | grep -q 'FORBIDDEN-BY-LINTER'; then\n  echo 'lint: marker' >&2\n  exit 1\nfi\nexit 0\n",
    );
    let config = f.home.join(".gitconfig");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str(&format!("[core]\n\thooksPath = {}\n", hooks.display()));
    std::fs::write(&config, text).unwrap();
    let head = || f.git(&["rev-parse", "HEAD"]).trim().to_owned();

    let tip = head();
    f.write("lint.txt", "FORBIDDEN-BY-LINTER\n");
    f.git(&["add", "lint.txt"]);
    let (ok, stderr) = run(command(&f, &UserOp::CommitStaged, Some("marked")));
    assert!(!ok, "the global hook did not reject the marker");
    assert!(stderr.contains("lint: marker"), "{stderr}");
    assert_eq!(head(), tip, "the branch moved");

    f.git(&["reset", "-q"]);
    std::fs::remove_file(f.repo.join("lint.txt")).unwrap();
    f.write("clean.txt", "clean\n");
    f.git(&["add", "clean.txt"]);
    let (ok, stderr) = run(command(&f, &UserOp::CommitStaged, Some("clean")));
    assert!(ok, "{stderr}");
    assert_ne!(head(), tip, "the clean commit did not go in");
}
