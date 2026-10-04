//! Git CLI invocation: allowlist, argv log, environment and timeout (ADR-GRP-009 § 3, SEC-10).
#![cfg(unix)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{ALLOWED, FIXED_PREFIX, Fixture, busy_repo, script};
use gitraptor_git::cli::{ConfigKey, GitCli, RefNamespace};
use gitraptor_git::resolve::{self, Rejection, Resolution, ResolveConfig};
use gitraptor_git::{GitVersion, MemoryArgvLog, ReadError, RefName, SystemGit};

#[test]
fn argv_log_only_contains_allowlisted_subcommands() {
    let f = busy_repo();
    let log = MemoryArgvLog::default();
    let invoker = f.invoker().with_argv_sink(Arc::new(log.clone()));
    let main = RefName::new("main").unwrap();
    let feature = RefName::new("feature").unwrap();
    let cli = GitCli::new(&f.git, &invoker, &f.repo).unwrap();
    cli.rev_parse_verify(&main).unwrap();
    assert_eq!(
        cli.rev_parse_verify(&RefName::new("missing").unwrap())
            .unwrap(),
        None
    );
    assert_eq!(cli.for_each_ref(RefNamespace::Heads).unwrap().len(), 2);
    assert_eq!(cli.worktree_list().unwrap().len(), 1);
    assert_eq!(
        cli.rev_list_left_right_count(&main, &feature).unwrap(),
        (1, 1)
    );
    assert!(cli.merge_base(&main, &feature).unwrap().is_some());
    assert_eq!(cli.log(&main, 10).unwrap().len(), 2);
    assert_eq!(
        cli.config_get(&ConfigKey::InitDefaultBranch).unwrap(),
        Some("main".into())
    );
    assert_eq!(
        cli.config_get(&ConfigKey::BranchRemote(main.clone()))
            .unwrap(),
        None
    );

    let entries = log.entries();
    assert_eq!(entries.len(), 9);
    for argv in entries {
        assert_eq!(argv[0], f.git.path.display().to_string());
        assert_eq!(&argv[1..=FIXED_PREFIX.len()], FIXED_PREFIX, "{argv:?}");
        let sub = &argv[FIXED_PREFIX.len() + 1];
        assert!(ALLOWED.contains(&sub.as_str()), "{sub} not allowlisted");
        for arg in &argv[FIXED_PREFIX.len() + 1..] {
            assert!(
                !["status", "diff", "--list", "--get-regexp", "-l"].contains(&arg.as_str()),
                "{arg} in {argv:?}"
            );
            assert!(!arg.contains("%G"), "{arg}");
        }
    }
}

/// A fake `git` that dumps its environment and argv, then reports `version`.
fn env_dumping_git(f: &Fixture) -> (std::path::PathBuf, std::path::PathBuf) {
    let bin = f.root().join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let dump = f.root().join("env-dump");
    let git = bin.join("git");
    script(
        &git,
        &format!(
            "env > '{d}'\necho \"$@\" >> '{d}'\necho 'git version 2.40.0'",
            d = dump.display()
        ),
    );
    (git, dump)
}

#[test]
fn hostile_parent_env_does_not_reach_child() {
    let f = Fixture::new();
    let (git, dump) = env_dumping_git(&f);
    let invoker = f.invoker().with_parent_env([
        ("HOME", "/home/dev"),
        ("PATH", ".:relative/bin:/usr/bin"),
        ("GIT_EXEC_PATH", "/tmp/evil"),
        ("GIT_SSH_COMMAND", "evil"),
        ("GIT_CONFIG_PARAMETERS", "'core.fsmonitor'='/tmp/evil'"),
        ("GIT_DIR", "/tmp/other"),
        ("LD_PRELOAD", "/tmp/evil.so"),
        ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
        ("XDG_CONFIG_HOME", "/tmp/evil"),
        ("GIT_TRACE", "/tmp/trace"),
    ]);
    let config = ResolveConfig {
        configured_path: Some(git.clone()),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    let Resolution::Found { git: found, .. } = resolve::resolve(&config, &invoker) else {
        panic!("fake git not selected");
    };
    assert_eq!(found.path, git.canonicalize().unwrap());
    let env = std::fs::read_to_string(&dump).unwrap();
    for var in [
        "GIT_EXEC_PATH",
        "GIT_SSH_COMMAND",
        "GIT_CONFIG_PARAMETERS",
        "GIT_DIR",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "XDG_CONFIG_HOME",
        "GIT_TRACE=",
    ] {
        assert!(!env.contains(var), "{var} reached the child:\n{env}");
    }
    assert!(env.contains("PATH=/usr/bin\n"), "{env}");
    assert!(env.contains("HOME=/home/dev\n"));
    assert!(env.contains("GIT_OPTIONAL_LOCKS=0\n"));
    assert!(env.contains("GIT_TERMINAL_PROMPT=0\n"));
    assert!(env.contains("GIT_PAGER=cat\n"));
    assert!(env.contains("--no-optional-locks -c core.fsmonitor=false"));
}

#[test]
fn hung_invocation_is_killed_and_temporarily_unavailable() {
    let f = busy_repo();
    let bin = f.root().join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    let hung = bin.join("git");
    script(&hung, "exec sleep 30");
    let git = SystemGit {
        path: hung.canonicalize().unwrap(),
        version: GitVersion {
            major: 2,
            minor: 40,
            patch: 0,
        },
    };
    let invoker = f.invoker().with_timeout(Duration::from_millis(300));
    let cli = GitCli::new(&git, &invoker, &f.repo).unwrap();
    let start = Instant::now();
    let err = cli.log(&RefName::new("main").unwrap(), 1).unwrap_err();
    assert!(
        matches!(err, ReadError::TemporarilyUnavailable(_)),
        "{err:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(5));

    // Resolution reports a hung candidate instead of waiting forever.
    let config = ResolveConfig {
        configured_path: Some(hung),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    let Resolution::NotFound { diagnostics } = resolve::resolve(&config, &invoker) else {
        panic!("hung git selected");
    };
    assert!(matches!(
        &diagnostics[0].rejection,
        Rejection::NoVersion(m) if m.contains("temporarily unavailable")
    ));
}

#[test]
fn option_like_ref_rejected() {
    assert!(matches!(
        RefName::new("--upload-pack=x"),
        Err(ReadError::InvalidInput(_))
    ));
    assert!(RefName::new("-n").is_err());
}

#[test]
fn cli_rejects_unc_and_relative_repo_paths() {
    let git = common::system_git();
    let invoker = gitraptor_git::Invoker::default();
    for p in ["//server/share/repo", "\\\\server\\share", "relative/repo"] {
        assert!(matches!(
            GitCli::new(&git, &invoker, std::path::Path::new(p)),
            Err(ReadError::InvalidInput(_))
        ));
    }
}
