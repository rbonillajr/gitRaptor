//! T005 (DS-US-MCP-010): `UserOp::AddPaths` and `UserOp::UnstagePaths`. Child module of
//! `user_ops`, attached there with `#[cfg(test)] #[path = "user_ops_add_tests.rs"] mod add_tests;`.

use super::*;
use crate::GitVersion;
use std::process::Command;

fn abs(unix: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{}", unix.replace('/', "\\")))
    } else {
        PathBuf::from(unix)
    }
}

fn editor() -> PathBuf {
    if cfg!(windows) {
        abs("/raptor/raptor-no-editor.exe")
    } else {
        PathBuf::from(FALLBACK_NO_EDITOR)
    }
}

fn fake_git() -> SystemGit {
    SystemGit {
        path: abs("/usr/bin/git"),
        version: GitVersion::parse("git version 2.50.0").unwrap(),
    }
}

fn build(op: UserOp) -> Result<UserGitCommand, String> {
    UserGitCommand::new(
        &fake_git(),
        &RepoTarget {
            git_dir: abs("/w/repo/.git"),
            work_tree: Some(abs("/w/repo")),
        },
        &op,
        None,
        &SessionEnv::default(),
        &editor(),
        Some(&abs("/home/u")),
    )
}

/// The words after the neutralizing `-c` pairs and the repo flags.
fn tail(argv: &[String]) -> Vec<String> {
    let at = argv
        .iter()
        .position(|a| a.starts_with("--work-tree="))
        .expect("an explicit work tree");
    argv[at + 1..].to_vec()
}

#[test]
fn add_paths_never_forces() {
    let paths = vec!["src/a.rs".to_string(), ":(glob)**".to_string()];
    let argv = build(UserOp::AddPaths {
        paths: paths.clone(),
    })
    .expect("AddPaths builds")
    .argv();
    assert_eq!(
        tail(&argv),
        ["add", "--", ":(literal)src/a.rs", ":(literal):(glob)**"],
        "AddPaths must be `add -- :(literal)<p>…`"
    );
    for forbidden in ["-f", "--force", "-A", "--all", "-u", "--update"] {
        assert!(
            !argv.iter().any(|a| a == forbidden),
            "AddPaths must never pass {forbidden}: {argv:?}"
        );
    }

    let argv = build(UserOp::UnstagePaths { paths })
        .expect("UnstagePaths builds")
        .argv();
    assert_eq!(
        tail(&argv),
        [
            "rm",
            "--cached",
            "--quiet",
            "--ignore-unmatch",
            "--",
            ":(literal)src/a.rs",
            ":(literal):(glob)**"
        ],
        "UnstagePaths must be `rm --cached --quiet --ignore-unmatch -- :(literal)<p>…`"
    );

    assert!(
        build(UserOp::AddPaths { paths: Vec::new() }).is_err(),
        "an empty AddPaths list must fail at construction"
    );
    assert!(
        build(UserOp::UnstagePaths { paths: Vec::new() }).is_err(),
        "an empty UnstagePaths list must fail at construction"
    );
}

#[cfg(unix)]
#[test]
fn add_beyond_a_symbolic_link_is_refused_by_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    let other = root.join("other");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("secret.txt"), "x").unwrap();
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(init.status.success(), "git init: {init:?}");
    std::os::unix::fs::symlink(&other, repo.join("link")).unwrap();

    let out = Command::new("git")
        .arg("--version")
        .output()
        .expect("git runs");
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let path = std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("git"))
                .find(|g| g.is_file())
        })
        .expect("git on PATH");
    let git = SystemGit {
        path,
        version: GitVersion::parse(&version).expect("a git version"),
    };
    let cmd = UserGitCommand::new(
        &git,
        &RepoTarget {
            git_dir: repo.join(".git"),
            work_tree: Some(repo.clone()),
        },
        &UserOp::AddPaths {
            paths: vec!["link/secret.txt".into()],
        },
        None,
        &SessionEnv::default(),
        &editor(),
        Some(&root),
    )
    .expect("AddPaths builds");
    let (mut child, collector) = cmd.spawn().expect("git spawns");
    let status = child.wait().unwrap();
    let _ = collector.collect();
    assert!(
        !status.success(),
        "git must refuse an add beyond a symbolic link (A22)"
    );
    let staged = Command::new("git")
        .args(["ls-files", "--cached"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        staged.stdout.is_empty(),
        "nothing beyond the link may be staged: {:?}",
        String::from_utf8_lossy(&staged.stdout)
    );
}
